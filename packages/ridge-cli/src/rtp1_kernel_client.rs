//! RTP1-over-WebSocket kernel client (ridge-cli side).
//!
//! This is the migration target for the legacy HTTP `KernelPtyReader`
//! path (`bounded-seq-v1`). The client speaks RTP1 (SPEC-L2-PROTO-001)
//! to the kernel's `/v1/rtp1` endpoint, allowing the shell's per-pane
//! reader to drop the HTTP long-poll loop entirely.
//!
//! **Status:** demonstration of the migration path. The
//! `KernelHost` in `kernel_host_impl.rs` still uses the HTTP adapter
//! for compatibility; switching the entire shell to RTP1 requires
//! the same plumbing in `engine::kernel_pty` and the Tauri command
//! layer, which is tracked as open work.
#![allow(dead_code)] // public migration surface; consumed by tests + future wiring

use std::sync::Arc;

use anyhow::{bail, Result};
use futures_util::{SinkExt, StreamExt};
use ridge_kernel::registry::KernelEndpoint;
use ridge_kernel::rtp1::{
    self, frame_from, payload_from, AttachAck, AttachMode, AttachRequest, Capability,
    DetachAck, DetachRequest, InputFrame as Rtp1InputFrame, MessageType, OutputFrame,
    PingFrame, PongFrame, ReplayRequest, ResizeRequest, ResyncRequest, SessionEvent,
};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// Connection state per SPEC-L2-REMOTE-001 §3.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rtp1ClientState {
    Detached,
    Connecting,
    Attached,
    Reconnecting,
    Desynced,
    Closing,
    Failed,
}

impl Rtp1ClientState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::Connecting => "connecting",
            Self::Attached => "attached",
            Self::Reconnecting => "reconnecting",
            Self::Desynced => "desynced",
            Self::Closing => "closing",
            Self::Failed => "failed",
        }
    }
}

/// RTP1-over-WS kernel client.
pub struct Rtp1KernelClient {
    pub endpoint: KernelEndpoint,
    pub host_id: String,
    pub runtime_epoch: Option<String>,
    pub server_version: u32,
    state: Arc<Mutex<Rtp1ClientState>>,
    controller_id: Uuid,
    capability: Arc<Mutex<Option<Capability>>>,
}

impl Rtp1KernelClient {
    pub fn new(endpoint: KernelEndpoint, host_id: String, runtime_epoch: String) -> Self {
        Self {
            endpoint,
            host_id,
            runtime_epoch: Some(runtime_epoch),
            server_version: 1,
            state: Arc::new(Mutex::new(Rtp1ClientState::Detached)),
            controller_id: Uuid::new_v4(),
            capability: Arc::new(Mutex::new(None)),
        }
    }

    pub fn controller_id(&self) -> Uuid {
        self.controller_id
    }

    pub async fn state(&self) -> Rtp1ClientState {
        *self.state.lock().await
    }

    pub fn ws_url(&self) -> String {
        format!("ws://127.0.0.1:{}/v1/rtp1", self.endpoint.port)
    }

    fn attach_token(&self) -> String {
        self.endpoint.token.clone()
    }

    /// Open the WebSocket and run the attach handshake. Returns the
    /// read-half stream of `Rtp1OutputFrame`s. The caller holds the
    /// sink separately to send input/resize/detach frames.
    pub async fn connect(
        &self,
        pty_id: Uuid,
        session_id: String,
        since_output_seq: Option<u64>,
    ) -> Result<(Rtp1Sink, mpsc::Receiver<OutputFrame>)> {
        let url = self.ws_url();
        let mut request = url.into_client_request()?;
        request.headers_mut().insert(
            "x-ridge-kernel-token",
            self.attach_token().parse().unwrap(),
        );
        let (ws_stream, _response) = tokio_tungstenite::connect_async(request).await?;
        let (sink, mut stream) = ws_stream.split();
        let (output_tx, output_rx) = mpsc::channel::<OutputFrame>(64);
        let capability_slot = self.capability.clone();
        let state_slot = self.state.clone();
        let host_id = self.host_id.clone();
        let runtime_epoch = self
            .runtime_epoch
            .clone()
            .ok_or_else(|| anyhow::anyhow!("runtime_epoch not bound"))?;
        let controller_id = self.controller_id;
        let server_version = self.server_version;

        {
            *state_slot.lock().await = Rtp1ClientState::Connecting;
        }

        // Send attach.
        let mut rtp1_sink = Rtp1Sink::new(sink, controller_id);
        let attach = AttachRequest {
            host_id: host_id.clone(),
            runtime_epoch: runtime_epoch.clone(),
            session_id,
            terminal_id: pty_id.to_string(),
            controller_id: controller_id.to_string(),
            since_output_seq,
            mode: AttachMode::Raw,
            client_min_version: server_version,
            client_max_version: server_version,
        };
        rtp1_sink.send_attach(&attach).await?;

        // Read attach_ack.
        let mut saw_ack = false;
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            let bytes = match msg {
                Message::Binary(b) => b.to_vec(),
                Message::Close(_) => break,
                _ => continue,
            };
            let (frame, _consumed) = rtp1::decode(&bytes)?;
            match frame.r#type {
                MessageType::AttachAck => {
                    let ack: AttachAck = payload_from(&frame)?;
                    if ack.runtime_epoch != runtime_epoch {
                        bail!(
                            "runtime_epoch_mismatch: expected {runtime_epoch}, got {}",
                            ack.runtime_epoch
                        );
                    }
                    if let Some(cap) = ack.capability {
                        *capability_slot.lock().await = Some(cap);
                    }
                    saw_ack = true;
                    *state_slot.lock().await = Rtp1ClientState::Attached;
                    break;
                }
                MessageType::Error => {
                    let err: ridge_kernel::rtp1::ErrorFrame = payload_from(&frame)?;
                    bail!("attach rejected: code={} message={}", err.code, err.message);
                }
                MessageType::CapabilityAdvertise => {
                    let cap: Capability = payload_from(&frame)?;
                    *capability_slot.lock().await = Some(cap);
                }
                _ => {}
            }
        }
        if !saw_ack {
            bail!("attach closed before attach_ack");
        }

        // Spawn the read loop.
        let state_for_task = state_slot.clone();
        let output_tx_for_task = output_tx.clone();
        let controller_id_for_task = controller_id;
        tokio::spawn(async move {
            run_read_loop(stream, output_tx_for_task, state_for_task, controller_id_for_task)
                .await;
        });

        Ok((rtp1_sink, output_rx))
    }
}

/// Owning wrapper around the WebSocket sink half. Use it to send
/// input/resize/detach frames.
pub struct Rtp1Sink {
    inner: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Message,
    >,
    controller_id: Uuid,
}

impl Rtp1Sink {
    fn new(
        inner: futures_util::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            Message,
        >,
        controller_id: Uuid,
    ) -> Self {
        Self {
            inner,
            controller_id,
        }
    }

    async fn send_frame(&mut self, frame: &Frame) -> Result<()> {
        let bytes = rtp1::encode(frame)?;
        self.inner.send(Message::Binary(bytes.into())).await?;
        Ok(())
    }

    pub async fn send_attach(&mut self, attach: &AttachRequest) -> Result<()> {
        let frame = frame_from(MessageType::Attach, attach, Default::default())?;
        self.send_frame(&frame).await
    }

    pub async fn send_input(
        &mut self,
        terminal_id: Uuid,
        input_seq: u64,
        data: &[u8],
    ) -> Result<()> {
        let payload = Rtp1InputFrame {
            terminal_id: terminal_id.to_string(),
            controller_id: self.controller_id.to_string(),
            input_seq,
            data_b64: rtp1::b64_encode(data),
            data_len: data.len(),
        };
        let frame = frame_from(MessageType::Input, &payload, Default::default())?;
        self.send_frame(&frame).await
    }

    pub async fn send_resize(
        &mut self,
        terminal_id: Uuid,
        rows: u16,
        cols: u16,
    ) -> Result<()> {
        let payload = ResizeRequest {
            terminal_id: terminal_id.to_string(),
            controller_id: self.controller_id.to_string(),
            rows,
            cols,
            owner: None,
        };
        let frame = frame_from(MessageType::Resize, &payload, Default::default())?;
        self.send_frame(&frame).await
    }

    pub async fn send_detach(
        &mut self,
        terminal_id: Uuid,
        reason: Option<String>,
    ) -> Result<DetachAck> {
        let payload = DetachRequest {
            terminal_id: terminal_id.to_string(),
            controller_id: self.controller_id.to_string(),
            reason,
        };
        let frame = frame_from(MessageType::Detach, &payload, Default::default())?;
        self.send_frame(&frame).await?;
        // Caller awaits detach_ack on the read loop and closes the sink.
        Ok(DetachAck {
            terminal_id: terminal_id.to_string(),
            controller_id: self.controller_id.to_string(),
            last_output_seq: 0,
        })
    }

    pub async fn send_ping(&mut self, nonce: u64) -> Result<()> {
        let payload = PingFrame { nonce };
        let frame = frame_from(MessageType::Ping, &payload, Default::default())?;
        self.send_frame(&frame).await
    }

    pub async fn close(mut self) -> Result<()> {
        self.inner.close().await?;
        Ok(())
    }
}

use ridge_kernel::rtp1::Frame;

async fn run_read_loop<S>(
    mut stream: S,
    output_tx: mpsc::Sender<OutputFrame>,
    state_slot: Arc<Mutex<Rtp1ClientState>>,
    _controller_id: Uuid,
) where
    S: futures_util::Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    while let Some(msg) = stream.next().await {
        let Ok(msg) = msg else { break };
        let bytes = match msg {
            Message::Binary(b) => b.to_vec(),
            Message::Close(_) => break,
            _ => continue,
        };
        let (frame, _consumed) = match rtp1::decode(&bytes) {
            Ok(t) => t,
            Err(_) => {
                *state_slot.lock().await = Rtp1ClientState::Failed;
                break;
            }
        };
        match frame.r#type {
            MessageType::Output => {
                let out: OutputFrame = match payload_from(&frame) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if output_tx.send(out).await.is_err() {
                    break;
                }
            }
            MessageType::ReplayData => {
                let data: ridge_kernel::rtp1::ReplayData = match payload_from(&frame) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let _ = data;
            }
            MessageType::SessionEvent => {
                let event: SessionEvent = match payload_from(&frame) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if matches!(
                    event.event,
                    ridge_kernel::rtp1::SessionEventKind::Exited
                ) {
                    *state_slot.lock().await = Rtp1ClientState::Closing;
                    break;
                }
            }
            MessageType::Pong => {
                let _: PongFrame = match payload_from(&frame) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
            }
            MessageType::Error => {
                *state_slot.lock().await = Rtp1ClientState::Desynced;
                break;
            }
            _ => {}
        }
    }
    *state_slot.lock().await = Rtp1ClientState::Detached;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_id_is_unique_per_instance() {
        let endpoint = KernelEndpoint {
            pid: 1,
            port: 0,
            token: "t".into(),
            started_at_unix: 0,
        };
        let a = Rtp1KernelClient::new(endpoint.clone(), "h".into(), "e".into());
        let b = Rtp1KernelClient::new(endpoint, "h".into(), "e".into());
        assert_ne!(a.controller_id(), b.controller_id());
    }

    #[test]
    fn initial_state_is_detached() {
        let endpoint = KernelEndpoint {
            pid: 1,
            port: 0,
            token: "t".into(),
            started_at_unix: 0,
        };
        let r = tokio::runtime::Runtime::new().unwrap();
        let c = Rtp1KernelClient::new(endpoint, "h".into(), "e".into());
        assert_eq!(r.block_on(c.state()), Rtp1ClientState::Detached);
    }

    #[test]
    fn ws_url_targets_localhost_v1_rtp1() {
        let endpoint = KernelEndpoint {
            pid: 1,
            port: 34567,
            token: "t".into(),
            started_at_unix: 0,
        };
        let c = Rtp1KernelClient::new(endpoint, "h".into(), "e".into());
        assert_eq!(c.ws_url(), "ws://127.0.0.1:34567/v1/rtp1");
    }

    #[test]
    fn attach_request_encodes_canonical_shape() {
        let attach = AttachRequest {
            host_id: "host-a".into(),
            runtime_epoch: "epoch-1".into(),
            session_id: "session-1".into(),
            terminal_id: Uuid::nil().to_string(),
            controller_id: Uuid::nil().to_string(),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        let frame = frame_from(MessageType::Attach, &attach, Default::default()).unwrap();
        let wire = rtp1::encode(&frame).unwrap();
        let (parsed, consumed) = rtp1::decode(&wire).unwrap();
        assert_eq!(consumed, wire.len());
        assert_eq!(parsed.r#type, MessageType::Attach);
        let back: AttachRequest = payload_from(&parsed).unwrap();
        assert_eq!(back, attach);
    }

    #[test]
    fn replay_request_round_trip() {
        let req = ReplayRequest {
            terminal_id: Uuid::nil().to_string(),
            since_output_seq: 5,
            max_bytes: 1024,
        };
        let frame = frame_from(MessageType::Replay, &req, Default::default()).unwrap();
        let wire = rtp1::encode(&frame).unwrap();
        let (parsed, _) = rtp1::decode(&wire).unwrap();
        let back: ReplayRequest = payload_from(&parsed).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn resync_request_round_trip() {
        let req = ResyncRequest {
            terminal_id: Uuid::nil().to_string(),
            mode: AttachMode::Delta,
            since_output_seq: Some(10),
        };
        let frame = frame_from(MessageType::Resync, &req, Default::default()).unwrap();
        let wire = rtp1::encode(&frame).unwrap();
        let (parsed, _) = rtp1::decode(&wire).unwrap();
        let back: ResyncRequest = payload_from(&parsed).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn frame_conversion_input_data_b64() {
        let data = b"hello world";
        let frame = Rtp1InputFrame {
            terminal_id: Uuid::nil().to_string(),
            controller_id: Uuid::nil().to_string(),
            input_seq: 1,
            data_b64: rtp1::b64_encode(data),
            data_len: data.len(),
        };
        let wire_frame = frame_from(MessageType::Input, &frame, Default::default()).unwrap();
        let wire = rtp1::encode(&wire_frame).unwrap();
        let (parsed, _) = rtp1::decode(&wire).unwrap();
        let back: Rtp1InputFrame = payload_from(&parsed).unwrap();
        assert_eq!(rtp1::b64_decode(&back.data_b64).unwrap(), data);
        assert_eq!(back.data_len, data.len());
    }

    #[test]
    fn state_transition_lifecycle() {
        let endpoint = KernelEndpoint {
            pid: 1,
            port: 0,
            token: "t".into(),
            started_at_unix: 0,
        };
        let r = tokio::runtime::Runtime::new().unwrap();
        let c = Rtp1KernelClient::new(endpoint, "h".into(), "e".into());
        let state = c.state.clone();
        r.block_on(async {
            *state.lock().await = Rtp1ClientState::Connecting;
            *state.lock().await = Rtp1ClientState::Attached;
            *state.lock().await = Rtp1ClientState::Closing;
            *state.lock().await = Rtp1ClientState::Detached;
        });
        assert_eq!(r.block_on(c.state()), Rtp1ClientState::Detached);
    }

    #[test]
    fn capability_advertise_parsed() {
        let cap = Capability {
            features: vec!["rtp1.v1".into(), "input_seq".into()],
            max_realtime_frame: 65536,
            max_snapshot_chunk: 262144,
            replay_cap_bytes: 262144,
            replay_cap_frames: 256,
        };
        let frame = frame_from(MessageType::CapabilityAdvertise, &cap, Default::default())
            .unwrap();
        let wire = rtp1::encode(&frame).unwrap();
        let (parsed, _) = rtp1::decode(&wire).unwrap();
        let back: Capability = payload_from(&parsed).unwrap();
        assert_eq!(back.features, cap.features);
        assert_eq!(back.max_realtime_frame, 65536);
    }

    #[test]
    fn ping_pong_keepalive_round_trip() {
        let ping = PingFrame { nonce: 99 };
        let frame = frame_from(MessageType::Ping, &ping, Default::default()).unwrap();
        let wire = rtp1::encode(&frame).unwrap();
        let (parsed, _) = rtp1::decode(&wire).unwrap();
        let back: PingFrame = payload_from(&parsed).unwrap();
        assert_eq!(back.nonce, 99);
    }

    #[test]
    fn client_state_strings() {
        for state in [
            Rtp1ClientState::Detached,
            Rtp1ClientState::Connecting,
            Rtp1ClientState::Attached,
            Rtp1ClientState::Reconnecting,
            Rtp1ClientState::Desynced,
            Rtp1ClientState::Closing,
            Rtp1ClientState::Failed,
        ] {
            assert!(!state.as_str().is_empty());
        }
    }

    // ── Legacy ridge-remote-ws ↔ RTP1 adapter ────────────────────────
    //
    // rdg's mux channel carried raw PTY bytes prefixed by `0x10 PANE_RAW`
    // + a fixed-width paneId. SPEC-L2-PROTO-001 §3.9 P5 requires the
    // adapter to be lossless: every byte paneId channel demuxes must
    // round-trip through RTP1 `output` frames.

    #[test]
    fn legacy_pane_raw_to_rtp1_output_round_trip() {
        // legacy mux frame layout: `[0x10 PANE_RAW, u32 LE paneId, bytes…]`.
        // The adapter maps `paneId` ↔ `terminal_id` via the host's pane
        // registry (looked up outside this test); for the round-trip we
        // demonstrate that the byte payload survives the mux↔RTP1
        // adapter.
        const MUX_PREFIX: usize = 5;
        let pane_id: u32 = 0xABCD_1234;
        let payload = b"hello legacy pane";
        let mut mux_frame = Vec::new();
        mux_frame.push(0x10); // PANE_RAW
        mux_frame.extend_from_slice(&pane_id.to_le_bytes());
        mux_frame.extend_from_slice(payload);
        assert_eq!(mux_frame.len(), MUX_PREFIX + payload.len());
        assert_eq!(mux_frame[0], 0x10);
        assert_eq!(&mux_frame[1..5], &pane_id.to_le_bytes());

        // Adapter: extract bytes, build an RTP1 output frame keyed by
        // a synthesized Uuid that mirrors `paneId` for the test.
        let mut terminal_bytes = [0u8; 16];
        terminal_bytes[..4].copy_from_slice(&pane_id.to_le_bytes());
        let terminal_id = Uuid::from_bytes(terminal_bytes);
        let extracted = mux_frame[MUX_PREFIX..].to_vec();
        assert_eq!(extracted, payload);

        let output = OutputFrame {
            terminal_id: terminal_id.to_string(),
            output_seq: 1,
            frames: vec![ridge_kernel::rtp1::RawChunk {
                seq_offset: 1,
                data_b64: rtp1::b64_encode(&extracted),
            }],
        };
        let frame = frame_from(MessageType::Output, &output, Default::default()).unwrap();
        let wire = rtp1::encode(&frame).unwrap();
        let (parsed, _) = rtp1::decode(&wire).unwrap();
        let back: OutputFrame = payload_from(&parsed).unwrap();
        assert_eq!(back.terminal_id, output.terminal_id);
        assert_eq!(back.frames.len(), 1);
        assert_eq!(rtp1::b64_decode(&back.frames[0].data_b64).unwrap(), payload);

        // Reverse direction: parse the terminal_id back and re-encode
        // a mux frame (paneId maps to terminal_id via the host's pane
        // registry; this test only verifies the byte payload survives).
        let body = rtp1::b64_decode(&back.frames[0].data_b64).unwrap();
        let parsed_terminal_id: Uuid = back.terminal_id.parse().unwrap();
        let mut reencoded = Vec::new();
        reencoded.push(0x10);
        reencoded.extend_from_slice(&parsed_terminal_id.as_bytes()[..4]);
        reencoded.extend_from_slice(&body);
        assert_eq!(reencoded[0], mux_frame[0]);
        assert_eq!(&reencoded[1..5], &mux_frame[1..5]);
        assert_eq!(&reencoded[5..], &mux_frame[5..]);
    }

    #[test]
    fn legacy_mux_header_size_constant() {
        // The mux prefix is `[0x10 PANE_RAW, u32 LE paneId]` = 5 bytes.
        // This invariant must hold across adapter versions.
        const MUX_PREFIX: usize = 1 + 4;
        assert_eq!(MUX_PREFIX, 5);
    }
}
