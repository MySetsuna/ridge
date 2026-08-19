use parking_lot::Mutex;
use portable_pty::MasterPty;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::engine::parser::PaneParser;
use crate::state::AppState;
use crate::teammate::layout_event::{LayoutChange, TEAMMATE_LAYOUT_CHANGED};
use crate::types::GlobalEvent;
use crate::utils::pty_log;
use ridge_core::pty::cwd;
use ridge_core::pty::decode::{flush_pending_eof, take_decoded_utf8};
use ridge_core::pty::osc_stream::OscSignalCarryover;

fn flush_pty_tail(osc_carryover: &mut OscSignalCarryover, utf8_pending: &mut Vec<u8>) -> String {
    let mut tail = osc_carryover.push(flush_pending_eof(utf8_pending));
    tail.push_str(&osc_carryover.finish());
    tail
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::mpsc;

    struct ChannelWriter(mpsc::Sender<Vec<u8>>);

    impl Write for ChannelWriter {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.0
                .send(data.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "capture closed"))?;
            Ok(data.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn flush_pty_tail_releases_unfinished_osc() {
        let mut carry = OscSignalCarryover::default();
        assert_eq!(carry.push("before\x1b]2;pending".into()), "before");
        let mut utf8_pending = Vec::new();

        assert_eq!(
            flush_pty_tail(&mut carry, &mut utf8_pending),
            "\x1b]2;pending"
        );
    }

    #[test]
    fn flush_pty_tail_completes_metadata_from_utf8_tail() {
        let mut carry = OscSignalCarryover::default();
        assert_eq!(carry.push("\x1b]7;file:///C:/wind".into()), "");
        let mut utf8_pending = b"\x1b\\after".to_vec();

        assert_eq!(
            flush_pty_tail(&mut carry, &mut utf8_pending),
            "\x1b]7;file:///C:/wind\x1b\\after"
        );
        assert!(utf8_pending.is_empty());
    }

    #[test]
    fn pane_cwd_matching_normalizes_existing_path_before_comparing() {
        assert!(pane_cwd_matches(
            Some(Path::new("C:/code/ridge")),
            "C:/code/ridge"
        ));
        assert!(!pane_cwd_matches(
            Some(Path::new("C:/code/ridge")),
            "C:/code/other"
        ));
        assert!(!pane_cwd_matches(None, "C:/code/ridge"));
    }

    #[test]
    fn pty_input_sink_preserves_order_across_batched_writes() {
        let (tx, rx) = mpsc::channel();
        let writer: Box<dyn Write + Send> = Box::new(ChannelWriter(tx));
        let sink = PtyInputSink::new(Arc::new(Mutex::new(writer)));

        sink.try_send(b"first".to_vec()).unwrap();
        sink.try_send(b"second".to_vec()).unwrap();

        let mut received = Vec::new();
        while received.len() < "firstsecond".len() {
            received.extend(rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap());
        }
        assert_eq!(received, b"firstsecond");
        drop(sink);
    }
}

fn normalize_cwd_str(raw: &str) -> String {
    ridge_core::commands::process::normalize_cwd(raw.to_string())
}

fn pane_cwd_matches(existing: Option<&Path>, normalized: &str) -> bool {
    existing
        .map(|path| normalize_cwd_str(&path.to_string_lossy()) == normalized)
        .unwrap_or(false)
}

pub struct PtyHandle {
    pub master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub input_sink: Arc<PtyInputSink>,
    pub _child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    pub native_ref: Option<(String, usize)>,
    pub native_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub remote_ref: Option<crate::hosts::RemoteRef>,
    pub kernel_ref: Option<crate::engine::kernel_pty::KernelPtyRef>,
    pub job: Option<crate::teammate::job_object::JobHandle>,
    pub child_pid: Option<u32>,
    pub resize_silence_deadline: Arc<AtomicI64>,
    pub parser: Arc<Mutex<PaneParser>>,
    pub delta_mode: Arc<AtomicBool>,
}

const PTY_INPUT_QUEUE_CAPACITY: usize = 256;
const PTY_INPUT_BATCH_BYTES: usize = 64 * 1024;

/// Bounded, cancellable stdin lane. Tauri commands only enqueue here; the
/// worker owns the potentially blocking ConPTY/kernel write and preserves
/// byte order without making the WebView wait for one HTTP request per key.
pub struct PtyInputSink {
    sender: SyncSender<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

impl PtyInputSink {
    pub fn new(writer: Arc<Mutex<Box<dyn Write + Send>>>) -> Arc<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(PTY_INPUT_QUEUE_CAPACITY);
        let closed = Arc::new(AtomicBool::new(false));
        let worker_closed = Arc::clone(&closed);
        let _ = std::thread::Builder::new()
            .name("ridge-pty-input".into())
            .spawn(move || {
                while let Ok(first) = receiver.recv() {
                    if worker_closed.load(Ordering::Acquire) {
                        continue;
                    }
                    let mut data = first;
                    while data.len() < PTY_INPUT_BATCH_BYTES {
                        match receiver.try_recv() {
                            Ok(next) => data.extend_from_slice(&next),
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                    let result = {
                        let mut sink = writer.lock();
                        sink.write_all(&data).and_then(|_| sink.flush())
                    };
                    if let Err(error) = result {
                        eprintln!("[ridge] PTY input worker stopped: {error}");
                        worker_closed.store(true, Ordering::Release);
                        break;
                    }
                }
            });
        Arc::new(Self { sender, closed })
    }

    pub fn try_send(&self, data: Vec<u8>) -> Result<(), String> {
        if self.closed.load(Ordering::Acquire) {
            return Err("PTY input worker is closed".into());
        }
        self.sender.try_send(data).map_err(|error| match error {
            TrySendError::Full(_) => "PTY input queue is full".into(),
            TrySendError::Disconnected(_) => "PTY input worker disconnected".into(),
        })
    }
}

impl Drop for PtyInputSink {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}

pub const RESIZE_SILENCE_WINDOW_MS: i64 = 80;

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn detach_terminal(state: &AppState, workspace_id: Uuid, pane_id: Uuid) {
    state.clear_pty_scrollback(workspace_id, pane_id);
    let mut map = state.workspaces.write();
    if let Some(ws) = map.get_mut(&workspace_id) {
        ws.terminals.remove(&pane_id);
    }
}

struct PtyReaderThread {
    state: AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    reader: Box<dyn Read + Send>,
    rt: tokio::runtime::Handle,
    buf: [u8; 8192],
    utf8_pending: Vec<u8>,
    carryover: String,
    osc_carryover: OscSignalCarryover,
    silence_deadline: Arc<AtomicI64>,
    native_ref_info: Option<(String, usize)>,
    my_pty_generation: u64,
}

impl PtyReaderThread {
    fn run(&mut self) {
        let result = catch_unwind(AssertUnwindSafe(|| self.read_loop()));
        if result.is_err() {
            eprintln!(
                "[ridge] PTY reader panicked (isolated to this thread) workspace={} pane={}",
                self.workspace_id, self.pane_id
            );
        }
    }

    fn read_loop(&mut self) {
        loop {
            if self.state.event_tx.is_closed() {
                break;
            }
            match self.reader.read(&mut self.buf) {
                Ok(0) => {
                    self.flush_tail();
                    pty_log::reader_eof(self.workspace_id, self.pane_id);
                    break;
                }
                Ok(n) => {
                    if !self.handle_chunk(n) {
                        break;
                    }
                }
                Err(error) => {
                    self.flush_tail();
                    pty_log::reader_io_err(self.workspace_id, self.pane_id, &error);
                    break;
                }
            }
        }
    }

    fn handle_chunk(&mut self, size: usize) -> bool {
        let raw = self
            .osc_carryover
            .push(take_decoded_utf8(&mut self.utf8_pending, &self.buf[..size]));
        if raw.is_empty() {
            return true;
        }

        let outcome = ridge_core::pty::chunk::process(
            raw,
            self.silence_deadline.load(Ordering::Acquire),
            now_epoch_ms(),
        );
        if outcome.clear_silence {
            self.silence_deadline.store(0, Ordering::Release);
        }
        let Some(signals) = outcome.emit else {
            return true;
        };
        if signals.text.is_empty() {
            return true;
        }

        self.state
            .append_pty_scrollback(self.workspace_id, self.pane_id, &signals.text);
        let prompt_seen = signals.prompt_seen;
        let title = signals.title;
        let cwd = signals.cwd;
        if !self.send_output(signals.text) {
            return false;
        }
        self.send_prompt_if_seen(prompt_seen);
        self.send_title(title);
        self.send_cwd(cwd);
        true
    }

    fn send_output(&mut self, data: String) -> bool {
        let payload = if self.carryover.is_empty() {
            data
        } else {
            let mut combined = std::mem::take(&mut self.carryover);
            combined.push_str(&data);
            combined
        };
        match self.state.event_tx.try_send(GlobalEvent::PtyOutput {
            workspace_id: self.workspace_id,
            pane_id: self.pane_id,
            data: payload,
        }) {
            Ok(()) => true,
            Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                if let GlobalEvent::PtyOutput { data, .. } = event {
                    self.carryover = data;
                }
                true
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    fn send_prompt_if_seen(&self, prompt_seen: bool) {
        if !prompt_seen {
            return;
        }
        let event_tx = self.state.event_tx.clone();
        let workspace_id = self.workspace_id;
        let pane_id = self.pane_id;
        let _ = self.rt.block_on(async move {
            let _ = event_tx
                .send(GlobalEvent::PanePromptDetected {
                    workspace_id,
                    pane_id,
                })
                .await;
        });
    }

    fn send_title(&self, title: Option<String>) {
        let Some(title) = title else {
            return;
        };
        let event_tx = self.state.event_tx.clone();
        let workspace_id = self.workspace_id;
        let pane_id = self.pane_id;
        let _ = self.rt.block_on(async move {
            let _ = event_tx
                .send(GlobalEvent::PaneTitleChanged {
                    workspace_id,
                    pane_id,
                    title,
                })
                .await;
        });
    }

    fn send_cwd(&self, cwd: Option<std::path::PathBuf>) {
        let Some(cwd) = cwd else {
            return;
        };
        let normalized = normalize_cwd_str(&cwd.to_string_lossy());
        if !self.update_pane_cwd(&normalized) {
            return;
        }
        let event_tx = self.state.event_tx.clone();
        let workspace_id = self.workspace_id;
        let pane_id = self.pane_id;
        let _ = self.rt.block_on(async move {
            let _ = event_tx
                .send(GlobalEvent::PaneCwdChanged {
                    workspace_id,
                    pane_id,
                    cwd: normalized,
                })
                .await;
        });
    }

    fn update_pane_cwd(&self, cwd: &str) -> bool {
        let mut map = self.state.workspaces.write();
        if let Some(ws) = map.get_mut(&self.workspace_id) {
            if let Some(pane) = ws.pane_tree.panes.get_mut(&self.pane_id) {
                if pane_cwd_matches(pane.cwd.as_deref(), cwd) {
                    return false;
                }
                pane.cwd = Some(std::path::PathBuf::from(cwd));
                return true;
            }
        }
        false
    }

    fn flush_tail(&mut self) {
        let tail = flush_pty_tail(&mut self.osc_carryover, &mut self.utf8_pending);
        if tail.is_empty() {
            return;
        }
        let tail_for_cwd = tail.clone();
        self.state
            .append_pty_scrollback(self.workspace_id, self.pane_id, &tail);
        let event_tx = self.state.event_tx.clone();
        let workspace_id = self.workspace_id;
        let pane_id = self.pane_id;
        let _ = self.rt.block_on(async move {
            let _ = event_tx
                .send(GlobalEvent::PtyOutput {
                    workspace_id,
                    pane_id,
                    data: tail,
                })
                .await;
        });
        if let Some(cwd) = cwd::parse_cwd_from_output(&tail_for_cwd) {
            self.update_tail_cwd(cwd);
        }
    }

    fn update_tail_cwd(&self, cwd: std::path::PathBuf) {
        let normalized = normalize_cwd_str(&cwd.to_string_lossy());
        if !self.update_pane_cwd(&normalized) {
            return;
        }
        crate::commands::ridge_file::schedule_auto_save(&self.state, self.workspace_id);
        let event_tx = self.state.event_tx.clone();
        let workspace_id = self.workspace_id;
        let pane_id = self.pane_id;
        let _ = self.rt.block_on(async move {
            let _ = event_tx
                .send(GlobalEvent::PaneCwdChanged {
                    workspace_id,
                    pane_id,
                    cwd: normalized,
                })
                .await;
        });
    }

    fn finish(&self) {
        if let Some((socket, gid)) = &self.native_ref_info {
            finish_native_pane(&self.state, self.workspace_id, self.pane_id, socket, *gid);
        } else {
            finish_ordinary_pane(
                &self.state,
                &self.rt,
                self.workspace_id,
                self.pane_id,
                self.my_pty_generation,
            );
        }
    }
}

fn finish_native_pane(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    socket: &str,
    global_id: usize,
) {
    crate::teammate::native::set_attachment(socket, global_id, None);
    state.clear_pty_scrollback(workspace_id, pane_id);
    state.unregister_pane_delta_channel(workspace_id, pane_id);
    let mut map = state.workspaces.write();
    if let Some(ws) = map.get_mut(&workspace_id) {
        ws.terminals.remove(&pane_id);
        let _ = ws.pane_tree.close(pane_id);
        ws.pane_sizes.remove(&pane_id);
        ws.teammate_pane_states.remove(&pane_id);
        ws.teammate_agent_pane_map
            .retain(|_, value| *value != pane_id);
        ws.pty_generation.remove(&pane_id);
    }
    drop(map);
    crate::teammate::profiles::remove_by_pane(workspace_id, pane_id);
    if let Some(app) = state.app_handle.get() {
        use tauri::Emitter;
        let _ = app.emit(
            TEAMMATE_LAYOUT_CHANGED,
            LayoutChange::detached(pane_id.to_string()),
        );
    }
}

fn finish_ordinary_pane(
    state: &AppState,
    rt: &tokio::runtime::Handle,
    workspace_id: Uuid,
    pane_id: Uuid,
    pty_generation: u64,
) {
    let event_tx = state.event_tx.clone();
    let _ = rt.block_on(async move {
        let _ = event_tx
            .send(GlobalEvent::PaneClosed {
                workspace_id,
                pane_id,
            })
            .await;
    });
    detach_terminal(state, workspace_id, pane_id);
    if demote_teammate_pane(state, workspace_id, pane_id, pty_generation) {
        crate::teammate::profiles::remove_by_pane(workspace_id, pane_id);
        if let Some(app) = state.app_handle.get() {
            use tauri::Emitter;
            let _ = app.emit(TEAMMATE_LAYOUT_CHANGED, LayoutChange::state());
        }
    }
}

fn demote_teammate_pane(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    pty_generation: u64,
) -> bool {
    let mut map = state.workspaces.write();
    let Some(ws) = map.get_mut(&workspace_id) else {
        return false;
    };
    let current_generation = ws.pty_generation.get(&pane_id).copied().unwrap_or(0);
    let is_current_pty = current_generation == pty_generation;
    let being_replaced = ws.pending_spawns.contains_key(&pane_id);
    if !is_current_pty || being_replaced || !ws.teammate_pane_states.contains_key(&pane_id) {
        return false;
    }
    ws.teammate_pane_states
        .insert(pane_id, crate::state::PaneState::Idle);
    ws.teammate_agent_pane_map
        .retain(|_, value| *value != pane_id);
    true
}

fn reader_snapshot(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> (Arc<AtomicI64>, Option<(String, usize)>, u64) {
    let map = state.workspaces.read();
    let silence_deadline = map
        .get(&workspace_id)
        .and_then(|ws| ws.terminals.get(&pane_id))
        .map(|handle| handle.resize_silence_deadline.clone())
        .unwrap_or_else(|| Arc::new(AtomicI64::new(0)));
    let native_ref = map
        .get(&workspace_id)
        .and_then(|ws| ws.terminals.get(&pane_id))
        .and_then(|handle| handle.native_ref.clone());
    let generation = map
        .get(&workspace_id)
        .and_then(|ws| ws.pty_generation.get(&pane_id).copied())
        .unwrap_or(0);
    (silence_deadline, native_ref, generation)
}

pub fn spawn_pty_reader(
    state: AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    reader: Box<dyn Read + Send>,
) {
    let handle = tokio::runtime::Handle::try_current();
    let (silence_deadline, native_ref_info, my_pty_generation) =
        reader_snapshot(&state, workspace_id, pane_id);
    let _ = std::thread::Builder::new()
        .name(format!("pty-reader-{pane_id}"))
        .spawn(move || {
            let Ok(rt) = handle else {
                pty_log::reader_no_runtime(workspace_id, pane_id);
                return;
            };
            let mut reader = PtyReaderThread {
                state,
                workspace_id,
                pane_id,
                reader,
                rt,
                buf: [0u8; 8192],
                utf8_pending: Vec::new(),
                carryover: String::new(),
                osc_carryover: OscSignalCarryover::default(),
                silence_deadline,
                native_ref_info,
                my_pty_generation,
            };
            reader.run();
            reader.finish();
        });
}
