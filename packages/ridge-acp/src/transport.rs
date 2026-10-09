//! Stdio JSON-RPC 2.0 transport.
//!
//! M2 architecture: single-writer owner + single-reader owner, each in a
//! dedicated tokio task. The `AcpTransport` handle is `Clone` (Arc-shared).
//!
//! Framing is configurable via [`Framing`]. ACP itself does not pin a
//! single framing — different agents adopt different conventions:
//!
//! - **Content-Length** (LSP-style): `Content-Length: N\r\n\r\n<body>`.
//!   Used by my M1 fixture (`live_acp_agent`) and by codex-acp.
//! - **NDJSON** (newline-delimited JSON): `<body>\n` per message.
//!   Used by opencode-acp (verified against opencode 1.18.31 / 2.0.6).
//!
//! [`AcpTransport::call`] issues a request and awaits a response via a
//! per-call `oneshot` channel — no lock-across-await on the response path.
//! [`AcpTransport::notify`] sends a fire-and-forget notification.
//! [`AcpTransport::subscribe_notifications`] returns a broadcast receiver
//! of agent notifications.
//!
//! Concurrency invariants:
//!
//! - **No lock-across-await**: writers push pre-encoded frames into a bounded
//!   mpsc. Callers await a oneshot tied to their request id; the pending map
//!   is guarded by a synchronous `Mutex` and is touched only inside
//!   short, non-awaiting critical sections.
//! - **Single writer owner**: only the writer task touches `ChildStdin`.
//! - **Single reader owner**: only the reader task touches `ChildStdout`.
//! - **Bounded backpressure**: writer channel capacity is fixed (32).
//! - **Idempotent shutdown**: `Drop` closes the writer channel and signals
//!   shutdown. Writer and reader tasks both observe and exit cleanly.
//! - **No orphan tasks**: writer exits when its `Receiver` returns `None`;
//!   reader exits on stdout EOF or shutdown.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::error::AcpErrorWrap;
use crate::protocol::{decode_frame, locate_header_end, AcpNotification, AcpRequest, AcpResponse};

const WRITER_CHANNEL_CAPACITY: usize = 32;
const NOTIFY_BROADCAST_CAPACITY: usize = 256;

/// Wire framing. ACP itself does not mandate one — different agents adopt
/// different conventions. `ContentLength` is the LSP-style header used by
/// my M1 fixture (`live_acp_agent`). `NdJson` is the newline-delimited
/// style used by opencode-acp (verified 1.18.31 / 2.0.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Framing {
    /// `Content-Length: N\r\n\r\n<body>` — LSP-style.
    #[default]
    ContentLength,
    /// `<body>\n` per message.
    NdJson,
}

#[derive(Debug)]
enum WriterCmd {
    Call { body: Vec<u8> },
    Notify { body: Vec<u8> },
}

struct TransportInner {
    next_id: AtomicI64,
    writer_tx: mpsc::Sender<WriterCmd>,
    /// Pending call responses, keyed by request id. The reader task drains
    /// this map when a response arrives. Guarded by a synchronous Mutex
    /// because all access is brief and does not hold the lock across await.
    /// `Arc` lets the reader task own a clone of the same map so dispatch
    /// routes responses back to the original `call()` oneshot.
    pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>>,
    notify_tx: broadcast::Sender<AcpNotification>,
    shutdown_tx: watch::Sender<bool>,
    closed: AtomicBool,
    framing: Framing,
    _writer_handle: JoinHandle<()>,
    _reader_handle: JoinHandle<()>,
    _stderr_handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct AcpTransport {
    inner: Arc<TransportInner>,
}

impl AcpTransport {
    /// Boot a transport from a child process's stdio handles. Default
    /// framing is `ContentLength` (LSP-style) — the format my M1 fixture
    /// uses and that codex-acp also adopts.
    pub fn spawn(
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: Option<ChildStderr>,
    ) -> Self {
        Self::spawn_with_framing(stdin, stdout, stderr, Framing::ContentLength)
    }

    /// Boot a transport with an explicit wire framing. Use `Framing::NdJson`
    /// to talk to agents like opencode-acp that do not use Content-Length.
    pub fn spawn_with_framing(
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: Option<ChildStderr>,
        framing: Framing,
    ) -> Self {
        let (writer_tx, writer_rx) = mpsc::channel(WRITER_CHANNEL_CAPACITY);
        let (notify_tx, _) = broadcast::channel(NOTIFY_BROADCAST_CAPACITY);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>> =
            Arc::new(StdMutex::new(HashMap::new()));
        let writer_handle = tokio::spawn(writer_task(
            stdin,
            writer_rx,
            shutdown_rx.clone(),
            framing,
        ));
        let reader_handle = tokio::spawn(reader_task(
            BufReader::new(stdout),
            notify_tx.clone(),
            shutdown_rx.clone(),
            pending.clone(),
            framing,
        ));
        let stderr_handle = stderr.map(|e| {
            tokio::spawn(stderr_drain_task(BufReader::new(e), shutdown_rx))
        });

        Self {
            inner: Arc::new(TransportInner {
                next_id: AtomicI64::new(1),
                writer_tx,
                pending,
                notify_tx,
                shutdown_tx,
                closed: AtomicBool::new(false),
                framing,
                _writer_handle: writer_handle,
                _reader_handle: reader_handle,
                _stderr_handle: stderr_handle,
            }),
        }
    }

    /// Send one JSON-RPC request, await the matched response.
    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<AcpResponse, AcpErrorWrap> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(AcpErrorWrap::SessionClosed);
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let req = AcpRequest {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params: if params.is_null() { None } else { Some(params) },
        };
        // Serialize the JSON envelope; the writer task applies the configured
        // framing (Content-Length header or NDJSON newline).
        let body = serde_json::to_vec(&req).map_err(|e| AcpErrorWrap::InvalidFrame(e.to_string()))?;

        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.inner.pending.lock().expect("pending mutex");
            pending.insert(id, tx);
        }
        if self
            .inner
            .writer_tx
            .send(WriterCmd::Call { body })
            .await
            .is_err()
        {
            // Writer is gone — drain pending entry.
            let mut pending = self.inner.pending.lock().expect("pending mutex");
            pending.remove(&id);
            return Err(AcpErrorWrap::SessionClosed);
        }
        match rx.await {
            Ok(resp) => Ok(resp),
            Err(_) => Err(AcpErrorWrap::SessionClosed),
        }
    }

    /// Send one JSON-RPC notification (no response expected).
    pub async fn notify(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), AcpErrorWrap> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(AcpErrorWrap::SessionClosed);
        }
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: if params.is_null() { None } else { Some(params) },
        };
        let body = serde_json::to_vec(&notif).map_err(|e| AcpErrorWrap::InvalidFrame(e.to_string()))?;
        self.inner
            .writer_tx
            .send(WriterCmd::Notify { body })
            .await
            .map_err(|_| AcpErrorWrap::SessionClosed)?;
        Ok(())
    }

    /// Subscribe to the agent notification stream.
    pub fn subscribe_notifications(&self) -> broadcast::Receiver<AcpNotification> {
        self.inner.notify_tx.subscribe()
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }

    /// Wire framing this transport was spawned with.
    pub fn framing(&self) -> Framing {
        self.inner.framing
    }

    pub fn shutdown(&self) {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.inner.shutdown_tx.send(true);
    }
}

impl Drop for TransportInner {
    fn drop(&mut self) {
        // Signal shutdown so reader + stderr + writer all observe and exit.
        let _ = self.shutdown_tx.send(true);
    }
}

// ---------- task bodies ----------

async fn writer_task(
    mut stdin: ChildStdin,
    mut writer_rx: mpsc::Receiver<WriterCmd>,
    mut shutdown_rx: watch::Receiver<bool>,
    framing: Framing,
) {
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
            }
            cmd = writer_rx.recv() => {
                match cmd {
                    Some(WriterCmd::Call { body } | WriterCmd::Notify { body }) => {
                        let framed = match framing {
                            Framing::ContentLength => {
                                let mut v = Vec::with_capacity(body.len() + 32);
                                let header = format!("Content-Length: {}\r\n\r\n", body.len());
                                v.extend_from_slice(header.as_bytes());
                                v.extend_from_slice(&body);
                                v
                            }
                            Framing::NdJson => {
                                let mut v = Vec::with_capacity(body.len() + 1);
                                v.extend_from_slice(&body);
                                v.push(b'\n');
                                v
                            }
                        };
                        eprintln!("[I3.6 TRACE] sending {} bytes: {}",
                            framed.len(),
                            String::from_utf8_lossy(&framed)
                                .chars()
                                .take(400)
                                .collect::<String>());
                        if let Err(e) = stdin.write_all(&framed).await {
                            eprintln!("ridge-acp writer: write_all failed: {e}");
                            break;
                        }
                        if let Err(e) = stdin.flush().await {
                            eprintln!("ridge-acp writer: flush failed: {e}");
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

async fn reader_task(
    reader: BufReader<ChildStdout>,
    notify_tx: broadcast::Sender<AcpNotification>,
    shutdown_rx: watch::Receiver<bool>,
    pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>>,
    framing: Framing,
) {
    match framing {
        Framing::ContentLength => reader_task_content_length(
            reader, notify_tx, shutdown_rx, pending,
        ).await,
        Framing::NdJson => reader_task_ndjson(
            reader, notify_tx, shutdown_rx, pending,
        ).await,
    }
}

async fn reader_task_content_length(
    mut reader: BufReader<ChildStdout>,
    notify_tx: broadcast::Sender<AcpNotification>,
    mut shutdown_rx: watch::Receiver<bool>,
    pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>>,
) {    let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024);
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
            read = read_more(&mut reader, &mut buf) => {
                match read {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("ridge-acp reader: read failed: {e}");
                        break;
                    }
                }
            }
        }
        while let Some(header_end) = locate_header_end(&buf) {
            let Some(split) = decode_frame(header_end, &buf) else { break };
            let total = header_end + 4 + split.body.len();
            let body = split.body.to_vec();
            buf.drain(..total);
            dispatch_frame(&body, &notify_tx, &pending);
        }
    }
    drain_pending(&pending);
}

async fn reader_task_ndjson(
    reader: BufReader<ChildStdout>,
    notify_tx: broadcast::Sender<AcpNotification>,
    mut shutdown_rx: watch::Receiver<bool>,
    pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>>,
) {
    let mut reader = reader;
    let mut line = Vec::with_capacity(4096);
    loop {
        line.clear();
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
            read = reader.read_until(b'\n', &mut line) => {
                match read {
                    Ok(0) => break,
                    Ok(n) => {
                        // Strip trailing newline (and optional \r).
                        let mut end = n;
                        if end > 0 && line[end - 1] == b'\n' { end -= 1; }
                        if end > 0 && line[end - 1] == b'\r' { end -= 1; }
                        if end == 0 { continue; }
                        dispatch_frame(&line[..end], &notify_tx, &pending);
                    }
                    Err(e) => {
                        eprintln!("ridge-acp reader: read failed: {e}");
                        break;
                    }
                }
            }
        }
    }
    drain_pending(&pending);
}

/// EOF / shutdown — wake all pending callers with SessionClosed.
fn drain_pending(pending: &Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>>) {
    let drained: Vec<oneshot::Sender<AcpResponse>> = {
        let mut p = pending.lock().expect("pending mutex");
        p.drain().map(|(_, tx)| tx).collect()
    };
    drop(drained);
}

fn dispatch_frame(
    body: &[u8],
    notify_tx: &broadcast::Sender<AcpNotification>,
    pending: &Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResponse>>>>,
) {
    // Iteration 3.6 — JSON-RPC demux correctness.
    //
    // The frame is classified FIRST by shape (notification vs response)
    // using a permissive `serde_json::Value` parse, then dispatched.
    // Previously we tried `AcpResponse::from_slice` first; that approach
    // is brittle when:
    //   * a notification-shaped frame also satisfies `AcpResponse`
    //     parsing because the receiver sends `id` defaults;
    //   * canonical ACP v1 frames (e.g. opencode) interleave a
    //     `session/update` notification BETWEEN an `initialize`
    //     response and the `session/new` response — the notification
    //     must not consume the `session/new` response.
    //
    // The shape check below is the smallest correct design:
    //   * `result` OR `error` is present → response (carry `id`)
    //   * `method` present                  → notification
    //   * else                              → malformed (diagnostic)
    //
    // `serde_json::Value` parse is conservative — we do NOT fabricate
    // missing fields. The notification broadcast path stays untouched.
    let value: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "ridge-acp reader: malformed JSON frame ({} bytes): {e}",
                body.len()
            );
            return;
        }
    };
    let obj = match value.as_object() {
        Some(o) => o,
        None => {
            eprintln!(
                "ridge-acp reader: frame is not a JSON object ({} bytes)",
                body.len()
            );
            return;
        }
    };
    let is_response = obj.contains_key("result") || obj.contains_key("error");
    let is_notification = obj.contains_key("method") && !is_response;
    if is_response {
        // Deserialize as response and route by id.
        let resp: AcpResponse = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "ridge-acp reader: response-shape frame fails AcpResponse parse: {e}"
                );
                return;
            }
        };
        let tx = {
            let mut p = pending.lock().expect("pending mutex");
            p.remove(&resp.id)
        };
        match tx {
            Some(tx) => {
                let _ = tx.send(resp);
            }
            None => {
                eprintln!(
                    "ridge-acp reader: dropped unsolicited response id={} ({} bytes)",
                    resp.id,
                    body.len()
                );
            }
        }
        return;
    }
    if is_notification {
        if let Ok(notif) = serde_json::from_value::<AcpNotification>(value) {
            let _ = notify_tx.send(notif);
        } else {
            eprintln!(
                "ridge-acp reader: notification-shape frame fails AcpNotification parse ({} bytes)",
                body.len()
            );
        }
        return;
    }
    eprintln!(
        "ridge-acp reader: frame has neither result/error nor method ({} bytes)",
        body.len()
    );
}

async fn read_more<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
) -> std::io::Result<usize> {
    let tmp = reader.fill_buf().await?;
    if tmp.is_empty() {
        return Ok(0);
    }
    let n = tmp.len();
    buf.extend_from_slice(tmp);
    reader.consume(n);
    Ok(n)
}

async fn stderr_drain_task(
    mut stderr: BufReader<ChildStderr>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut buf = String::new();
    loop {
        tokio::select! {
            biased;
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    break;
                }
                continue;
            }
            read = stderr.read_line(&mut buf) => {
                match read {
                    Ok(0) => break,
                    Ok(_) => {
                        let line = buf.trim_end();
                        if !line.is_empty() {
                            eprintln!("agent stderr: {line}");
                        }
                        buf.clear();
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_default_is_content_length() {
        // ACP itself does not mandate a wire framing — Content-Length is the
        // default because the M1 fixture (live_acp_agent) and codex-acp use
        // it. NDJSON is opt-in via spawn_with_framing for opencode-acp.
        assert_eq!(Framing::default(), Framing::ContentLength);
        assert_ne!(Framing::ContentLength, Framing::NdJson);
    }
}
