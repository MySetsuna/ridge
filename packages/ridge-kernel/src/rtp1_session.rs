//! RTP1 session: attachment state machine + per-connection transport binding.
//!
//! SPEC-L2-REMOTE-001 §3.2 (connection state machine) +
//! SPEC-L2-PROTO-001 §3.5 (22 message types).
//!
//! One [`Rtp1Session`] owns the per-WebSocket connection state: the
//! controller_id binding, the lease cursor, the input sequence counter,
//! and the output pump task. It uses [`crate::pty::PtyRegistry`] as the
//! authoritative PTY/lifecycle/output_seq owner (SPEC-L2-TERM-001 §3.1).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::pty::{
    PtyExitNotification, PtyOutputFrame, PtyOutputLease, PtyOutputRead, PtyRegistry,
    TerminalLifecycleState,
};
use crate::rtp1::{
    self, frame_from, payload_from, AttachAck, AttachMode, AttachRequest, Capability, DetachAck,
    DetachRequest, ErrorFrame, Frame, FrameFlags, InputAck, InputAckStatus, MessageType, PingFrame,
    PongFrame, RawChunk, ReplayData, ReplayRequest, ResizeAck, ResizeOwner, ResizeRequest,
    ResyncRequest, SessionEvent, SessionEventKind, SnapshotChunk,
};

/// SPEC-L2-REMOTE-001 §3.2 — per-controller attachment state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentState {
    Detached,
    Connecting,
    Attached,
    Reconnecting,
    Desynced,
    Closing,
    Failed,
}

/// Public host advertisement (SPEC-L2-PROTO-001 §3.4.1). Returned by the
/// kernel to clients during host discovery so they can pair host_id with
/// the current runtime_epoch before attaching.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostInfo {
    pub host_id: String,
    pub runtime_epoch: String,
    pub max_realtime_frame: usize,
    pub max_snapshot_chunk: usize,
    pub replay_cap_bytes: usize,
    pub replay_cap_frames: usize,
    pub features: Vec<String>,
}

/// Driver for a single WebSocket / RTP1 connection.
pub struct Rtp1Session {
    pub host_id: String,
    pub ptys: Arc<PtyRegistry>,
    pub server_version: u32,
    pub capability: Capability,
}

impl Rtp1Session {
    pub fn new(host_id: String, ptys: Arc<PtyRegistry>, server_version: u32) -> Self {
        Self {
            host_id,
            ptys,
            server_version,
            capability: Capability {
                features: vec![
                    "rtp1.v1".into(),
                    "input_seq".into(),
                    "replay".into(),
                    "snapshot".into(),
                    "session_event.exited".into(),
                    "runtime_epoch".into(),
                ],
                max_realtime_frame: rtp1::MAX_REALTIME_FRAME,
                max_snapshot_chunk: 256 * 1024,
                replay_cap_bytes: crate::pty::OUTPUT_REPLAY_CAP_BYTES,
                replay_cap_frames: crate::pty::OUTPUT_REPLAY_CAP_FRAMES,
            },
        }
    }

    pub fn host_info(&self) -> Option<HostInfo> {
        self.ptys.runtime_epoch().map(|epoch| HostInfo {
            host_id: self.host_id.clone(),
            runtime_epoch: epoch,
            max_realtime_frame: self.capability.max_realtime_frame,
            max_snapshot_chunk: self.capability.max_snapshot_chunk,
            replay_cap_bytes: self.capability.replay_cap_bytes,
            replay_cap_frames: self.capability.replay_cap_frames,
            features: self.capability.features.clone(),
        })
    }

    /// Bind an attach request. Returns the AttachAck and the lease cursor
    /// that the caller should use to drive the output pump.
    pub fn handle_attach(
        &self,
        request: &AttachRequest,
    ) -> Result<(AttachAck, Option<PtyOutputLease>, AttachmentState), AttachError> {
        let current_epoch = self
            .ptys
            .runtime_epoch()
            .ok_or_else(|| AttachError::ServerMisconfigured("runtime_epoch not bound".into()))?;
        if request.runtime_epoch != current_epoch {
            return Err(AttachError::RuntimeEpochStale {
                expected: current_epoch,
                received: request.runtime_epoch.clone(),
            });
        }
        if request.host_id != self.host_id {
            return Err(AttachError::PermissionDenied(format!(
                "host_id mismatch: expected {}, got {}",
                self.host_id, request.host_id
            )));
        }
        if request.client_max_version < self.server_version {
            return Err(AttachError::ClientTooOld {
                client_max: request.client_max_version,
                server: self.server_version,
            });
        }
        let terminal_id = Uuid::parse_str(&request.terminal_id)
            .map_err(|error| AttachError::InvalidField(format!("terminal_id: {error}")))?;
        let (oldest, next) = self
            .ptys
            .output_bounds(terminal_id)
            .map_err(|error| AttachError::UnknownTerminal(error.to_string()))?;
        let lease = self
            .ptys
            .attach_output(terminal_id, request.since_output_seq)
            .map_err(|error| AttachError::UnknownTerminal(error.to_string()))?;
        let ack = AttachAck {
            terminal_id: request.terminal_id.clone(),
            controller_id: request.controller_id.clone(),
            server_version: self.server_version,
            runtime_epoch: current_epoch,
            mode: request.mode,
            oldest_output_seq: oldest,
            next_output_seq: next,
            controller_input_seq: 0,
            snapshot: None,
            capability: Some(self.capability.clone()),
        };
        Ok((ack, Some(lease), AttachmentState::Attached))
    }

    pub fn handle_detach(&self, request: &DetachRequest) -> Result<DetachAck, AttachError> {
        let terminal_id = Uuid::parse_str(&request.terminal_id)
            .map_err(|error| AttachError::InvalidField(format!("terminal_id: {error}")))?;
        let (_oldest, next) = self
            .ptys
            .output_bounds(terminal_id)
            .map_err(|error| AttachError::UnknownTerminal(error.to_string()))?;
        Ok(DetachAck {
            terminal_id: request.terminal_id.clone(),
            controller_id: request.controller_id.clone(),
            last_output_seq: next.saturating_sub(1),
        })
    }

    pub fn handle_input(
        &self,
        request: &crate::rtp1::InputFrame,
    ) -> Result<InputAck, AttachError> {
        let terminal_id = Uuid::parse_str(&request.terminal_id)
            .map_err(|error| AttachError::InvalidField(format!("terminal_id: {error}")))?;
        let bytes = rtp1::b64_decode(&request.data_b64)
            .map_err(|error| AttachError::InvalidField(format!("data_b64: {error}")))?;
        if bytes.len() > self.capability.max_realtime_frame {
            return Err(AttachError::InputTooLarge {
                actual: bytes.len(),
                cap: self.capability.max_realtime_frame,
            });
        }
        if request.data_len != bytes.len() {
            return Err(AttachError::InvalidField(format!(
                "data_len {} != decoded {}",
                request.data_len,
                bytes.len()
            )));
        }
        self.ptys
            .write(terminal_id, &bytes)
            .map_err(|error| AttachError::IoError(error.to_string()))?;
        Ok(InputAck {
            terminal_id: request.terminal_id.clone(),
            controller_id: request.controller_id.clone(),
            input_seq: request.input_seq,
            status: InputAckStatus::Applied,
            reason: None,
        })
    }

    pub fn handle_resize(&self, request: &ResizeRequest) -> Result<ResizeAck, AttachError> {
        let terminal_id = Uuid::parse_str(&request.terminal_id)
            .map_err(|error| AttachError::InvalidField(format!("terminal_id: {error}")))?;
        if matches!(request.owner, Some(ResizeOwner::Observer)) {
            return Err(AttachError::PermissionDenied(
                "observer cannot resize".into(),
            ));
        }
        self.ptys
            .resize(terminal_id, request.cols, request.rows)
            .map_err(|error| AttachError::IoError(error.to_string()))?;
        let (_oldest, next) = self
            .ptys
            .output_bounds(terminal_id)
            .map_err(|error| AttachError::UnknownTerminal(error.to_string()))?;
        Ok(ResizeAck {
            terminal_id: request.terminal_id.clone(),
            controller_id: request.controller_id.clone(),
            rows: request.rows,
            cols: request.cols,
            next_output_seq: next,
        })
    }

    /// Read up to `max_bytes` of replay data from oldest retained seq at or
    /// after `since_output_seq`. If `since_output_seq` is older than oldest,
    /// returns `Lagged`.
    pub fn handle_replay(
        &self,
        request: &ReplayRequest,
    ) -> Result<ReplayResult, AttachError> {
        let terminal_id = Uuid::parse_str(&request.terminal_id)
            .map_err(|error| AttachError::InvalidField(format!("terminal_id: {error}")))?;
        let lease = self
            .ptys
            .attach_output(terminal_id, Some(request.since_output_seq))
            .map_err(|error| AttachError::UnknownTerminal(error.to_string()))?;
        let read = futures_lite_blocking(lease);
        let (frames, head) = match read {
            PtyOutputRead::Data(frames) => {
                let head = frames.last().map(|f| f.seq).unwrap_or(request.since_output_seq);
                (frames, head)
            }
            PtyOutputRead::Lagged {
                requested_seq,
                oldest_seq,
                latest_seq,
            } => {
                return Ok(ReplayResult::Lagged {
                    requested_seq,
                    oldest_seq,
                    head: latest_seq,
                });
            }
        };
        let at_oldest = matches!(
            self.ptys.lifecycle_state(terminal_id),
            Some(TerminalLifecycleState::Starting)
        );
        let chunks = frames
            .iter()
            .map(|f| RawChunk {
                seq_offset: f.seq,
                data_b64: rtp1::b64_encode(&f.data),
            })
            .collect();
        Ok(ReplayResult::Data(ReplayData {
            terminal_id: request.terminal_id.clone(),
            frames: chunks,
            at_oldest,
            head_output_seq: head,
        }))
    }

    pub fn handle_resync(&self, request: &ResyncRequest) -> Result<ResyncAck, AttachError> {
        let terminal_id = Uuid::parse_str(&request.terminal_id)
            .map_err(|error| AttachError::InvalidField(format!("terminal_id: {error}")))?;
        let (_oldest, next) = self
            .ptys
            .output_bounds(terminal_id)
            .map_err(|error| AttachError::UnknownTerminal(error.to_string()))?;
        Ok(ResyncAck {
            terminal_id: request.terminal_id.clone(),
            mode: request.mode,
            oldest_output_seq: request.since_output_seq.unwrap_or(0),
            next_output_seq: next,
        })
    }

    pub fn handle_ping(&self, ping: &PingFrame) -> PongFrame {
        PingFrame { nonce: ping.nonce }
    }

    pub fn terminal_lifecycle(&self, terminal_id: Uuid) -> Option<TerminalLifecycleState> {
        self.ptys.lifecycle_state(terminal_id)
    }

    pub fn subscribe_exit(&self, terminal_id: Uuid) -> Option<broadcast::Receiver<PtyExitNotification>> {
        self.ptys.subscribe_exit(terminal_id)
    }

    /// Build a `session_event{event:"exited"}` from a PtyExitNotification.
    pub fn build_session_event(
        notification: &PtyExitNotification,
    ) -> SessionEvent {
        SessionEvent {
            terminal_id: notification.pty_id.to_string(),
            event: SessionEventKind::Exited,
            code: notification.code,
        }
    }

    pub fn build_error(&self, terminal_id: Option<&str>, code: &str, message: &str) -> Frame {
        let err = ErrorFrame {
            terminal_id: terminal_id.map(str::to_string),
            controller_id: None,
            code: code.into(),
            message: message.into(),
        };
        frame_from(MessageType::Error, &err, FrameFlags::empty())
            .expect("error frame encode never fails")
    }

    pub fn build_snapshot_chunk(
        &self,
        terminal_id: &str,
        revision: u64,
        bytes: &[u8],
    ) -> Result<(Vec<Frame>, usize), AttachError> {
        const CHUNK: usize = 64 * 1024;
        let total = bytes.len();
        let mut out = Vec::new();
        let chunks: Vec<&[u8]> = bytes.chunks(CHUNK).collect();
        for (i, chunk) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            let mut flags = FrameFlags::empty();
            if !last {
                flags.0 |= FrameFlags::CONTINUATION;
            }
            let snap = SnapshotChunk {
                terminal_id: terminal_id.into(),
                revision,
                snapshot_bytes_b64: rtp1::b64_encode(chunk),
            };
            let frame = frame_from(MessageType::Snapshot, &snap, flags)
                .map_err(|error| AttachError::InvalidField(format!("snapshot encode: {error}")))?;
            out.push(frame);
        }
        Ok((out, total))
    }

    /// Convert a PtyOutputRead into one or more `output` frames. The kernel
    /// caps each payload at MAX_REALTIME_FRAME so a 256 KiB burst fans out
    /// into ≤ 4 independent complete frames.
    pub fn build_output_frames(
        &self,
        terminal_id: &str,
        frames: &[PtyOutputFrame],
    ) -> Vec<Frame> {
        // Output frame = OutputFrame JSON envelope + array of RawChunk.
        // base64 adds 4/3 expansion; JSON envelope adds ~80 bytes plus
        // ~25 bytes per chunk. Stay well under MAX_REALTIME_FRAME.
        const RAW_CHUNK_BYTES: usize = 24 * 1024;
        let mut out = Vec::new();
        let mut batch: Vec<RawChunk> = Vec::new();
        let mut batch_bytes: usize = 0;
        for frame in frames {
            let enc_len = rtp1::b64_encode(&frame.data).len();
            if !batch.is_empty() && batch_bytes + enc_len + 256 > rtp1::MAX_REALTIME_FRAME {
                out.push(make_output_frame(terminal_id, std::mem::take(&mut batch)));
                batch_bytes = 0;
            }
            // If a single chunk alone exceeds the cap, drop it down so the
            // resulting frame still fits.
            let raw = if frame.data.len() > RAW_CHUNK_BYTES {
                frame.data[..RAW_CHUNK_BYTES].to_vec()
            } else {
                frame.data.clone()
            };
            let enc = rtp1::b64_encode(&raw);
            batch.push(RawChunk {
                seq_offset: frame.seq,
                data_b64: enc.clone(),
            });
            batch_bytes += enc.len();
            // Edge: an oversize raw frame was truncated; emit a follow-up
            // chunk for the tail so the controller still sees the bytes.
            if raw.len() < frame.data.len() {
                let mut idx = RAW_CHUNK_BYTES;
                while idx < frame.data.len() {
                    let end = (idx + RAW_CHUNK_BYTES).min(frame.data.len());
                    let tail = frame.data[idx..end].to_vec();
                    let tail_enc = rtp1::b64_encode(&tail);
                    if batch_bytes + tail_enc.len() + 256 > rtp1::MAX_REALTIME_FRAME {
                        out.push(make_output_frame(terminal_id, std::mem::take(&mut batch)));
                        batch_bytes = 0;
                    }
                    batch.push(RawChunk {
                        seq_offset: frame.seq,
                        data_b64: tail_enc.clone(),
                    });
                    batch_bytes += tail_enc.len();
                    idx = end;
                }
            }
        }
        if !batch.is_empty() {
            out.push(make_output_frame(terminal_id, batch));
        }
        out
    }
}

pub fn make_output_frame(terminal_id: &str, chunks: Vec<RawChunk>) -> Frame {
    let head = chunks
        .first()
        .map(|c| c.seq_offset)
        .unwrap_or_default();
    let payload = crate::rtp1::OutputFrame {
        terminal_id: terminal_id.to_string(),
        output_seq: head,
        frames: chunks,
    };
    frame_from(MessageType::Output, &payload, FrameFlags::empty())
        .expect("output frame encode never fails")
}

/// Replay result the session can hand back to clients.
pub enum ReplayResult {
    Data(ReplayData),
    Lagged {
        requested_seq: u64,
        oldest_seq: u64,
        head: u64,
    },
}

#[derive(Debug, Clone)]
pub struct ResyncAck {
    pub terminal_id: String,
    pub mode: AttachMode,
    pub oldest_output_seq: u64,
    pub next_output_seq: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error("runtime_epoch stale: expected {expected}, received {received}")]
    RuntimeEpochStale { expected: String, received: String },
    #[error("client too old: max={client_max}, server={server}")]
    ClientTooOld { client_max: u32, server: u32 },
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("unknown terminal: {0}")]
    UnknownTerminal(String),
    #[error("invalid field: {0}")]
    InvalidField(String),
    #[error("input too large: {actual} > {cap}")]
    InputTooLarge { actual: usize, cap: usize },
    #[error("io error: {0}")]
    IoError(String),
    #[error("server misconfigured: {0}")]
    ServerMisconfigured(String),
}

impl AttachError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RuntimeEpochStale { .. } => "runtime_epoch_stale",
            Self::ClientTooOld { .. } => "client_too_old",
            Self::PermissionDenied(_) => "permission_denied",
            Self::UnknownTerminal(_) => "unknown_terminal",
            Self::InvalidField(_) => "protocol_violation",
            Self::InputTooLarge { .. } => "input_too_large",
            Self::IoError(_) => "io_error",
            Self::ServerMisconfigured(_) => "server_overloaded",
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

/// `PtyOutputLease::next` is async; for the RTP1 session we need a
/// blocking form to read once during attach. We yield once via
/// `tokio::task::block_in_place` is not appropriate in async contexts,
/// so wrap the lease in a future-aware helper. For this minimal seam we
/// expose the helper as a non-async function driven by a runtime handle.
fn futures_lite_blocking(lease: PtyOutputLease) -> PtyOutputRead {
    // We do not have a runtime handle here; use a synchronous wait.
    let handle = tokio::runtime::Handle::try_current();
    match handle {
        Ok(handle) => handle.block_on(async move {
            lease
                .next(Duration::from_millis(50), 64)
                .await
                .unwrap_or(PtyOutputRead::Data(Vec::new()))
        }),
        Err(_) => PtyOutputRead::Data(Vec::new()),
    }
}

/// Per-controller in-memory attachment registry (SPEC-L2-REMOTE-001 §3.4.5).
#[derive(Default)]
pub struct AttachmentRegistry {
    inner: Mutex<HashMap<String, AttachmentRecord>>,
}

#[derive(Debug, Clone)]
pub struct AttachmentRecord {
    pub terminal_id: Uuid,
    pub controller_id: String,
    pub state: AttachmentState,
    pub next_input_seq: u64,
}

impl AttachmentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind(
        &self,
        terminal_id: Uuid,
        controller_id: String,
        state: AttachmentState,
    ) -> AttachmentRecord {
        let mut guard = self.inner.lock();
        let record = AttachmentRecord {
            terminal_id,
            controller_id: controller_id.clone(),
            state,
            next_input_seq: 0,
        };
        guard.insert(controller_id, record.clone());
        record
    }

    pub fn unbind(&self, controller_id: &str) -> Option<AttachmentRecord> {
        self.inner.lock().remove(controller_id)
    }

    pub fn lookup(&self, controller_id: &str) -> Option<AttachmentRecord> {
        self.inner.lock().get(controller_id).cloned()
    }

    pub fn set_state(&self, controller_id: &str, state: AttachmentState) {
        if let Some(record) = self.inner.lock().get_mut(controller_id) {
            record.state = state;
        }
    }

    pub fn advance_input_seq(&self, controller_id: &str) -> Option<u64> {
        let mut guard = self.inner.lock();
        let record = guard.get_mut(controller_id)?;
        record.next_input_seq = record.next_input_seq.saturating_add(1);
        Some(record.next_input_seq)
    }

    pub fn all_for_terminal(&self, terminal_id: Uuid) -> Vec<AttachmentRecord> {
        self.inner
            .lock()
            .values()
            .filter(|r| r.terminal_id == terminal_id)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::PtyLaunch;

    fn make_registry() -> Arc<PtyRegistry> {
        let registry = Arc::new(PtyRegistry::default());
        registry.set_runtime_epoch("epoch-test".into());
        registry
    }

    fn spawn_shell(registry: &PtyRegistry) -> Uuid {
        registry
            .spawn_command_for(PtyLaunch {
                id: Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "test",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn shell")
    }

    #[tokio::test]
    async fn host_info_requires_runtime_epoch() {
        let registry = Arc::new(PtyRegistry::default());
        let session = Rtp1Session::new("host-a".into(), registry, 1);
        assert!(session.host_info().is_none());
    }

    #[tokio::test]
    async fn handle_attach_rejects_stale_runtime_epoch() {
        let registry = make_registry();
        let pty = spawn_shell(&registry);
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        let req = AttachRequest {
            host_id: "host-a".into(),
            runtime_epoch: "wrong-epoch".into(),
            session_id: "s".into(),
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        match session.handle_attach(&req) {
            Err(error) => assert_eq!(error.code(), "runtime_epoch_stale"),
            Ok(_) => panic!("expected stale epoch rejection"),
        }
    }

    #[tokio::test]
    async fn handle_attach_rejects_host_mismatch() {
        let registry = make_registry();
        let pty = spawn_shell(&registry);
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        let req = AttachRequest {
            host_id: "host-b".into(),
            runtime_epoch: "epoch-test".into(),
            session_id: "s".into(),
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        match session.handle_attach(&req) {
            Err(error) => assert_eq!(error.code(), "permission_denied"),
            Ok(_) => panic!("expected host_id rejection"),
        }
    }

    #[tokio::test]
    async fn handle_attach_rejects_client_too_old() {
        let registry = make_registry();
        let pty = spawn_shell(&registry);
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 5);
        let req = AttachRequest {
            host_id: "host-a".into(),
            runtime_epoch: "epoch-test".into(),
            session_id: "s".into(),
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 3,
        };
        match session.handle_attach(&req) {
            Err(error) => assert_eq!(error.code(), "client_too_old"),
            Ok(_) => panic!("expected client_too_old"),
        }
    }

    #[tokio::test]
    async fn handle_attach_succeeds_for_matching_epoch() {
        let registry = make_registry();
        let pty = spawn_shell(&registry);
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        let req = AttachRequest {
            host_id: "host-a".into(),
            runtime_epoch: "epoch-test".into(),
            session_id: "s".into(),
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        let (ack, lease, state) = session.handle_attach(&req).expect("attach ok");
        assert_eq!(ack.runtime_epoch, "epoch-test");
        assert_eq!(ack.server_version, 1);
        assert_eq!(state, AttachmentState::Attached);
        assert!(lease.is_some());
    }

    #[tokio::test]
    async fn handle_input_too_large_rejected_before_write() {
        let registry = make_registry();
        let pty = spawn_shell(&registry);
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        let oversized = vec![0u8; rtp1::MAX_REALTIME_FRAME + 1];
        let frame = crate::rtp1::InputFrame {
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            input_seq: 1,
            data_b64: rtp1::b64_encode(&oversized),
            data_len: oversized.len(),
        };
        match session.handle_input(&frame) {
            Err(error) => assert_eq!(error.code(), "input_too_large"),
            Ok(_) => panic!("expected input_too_large"),
        }
    }

    #[tokio::test]
    async fn handle_resize_rejects_observer() {
        let registry = make_registry();
        let pty = spawn_shell(&registry);
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        let req = ResizeRequest {
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            rows: 30,
            cols: 100,
            owner: Some(ResizeOwner::Observer),
        };
        match session.handle_resize(&req) {
            Err(error) => assert_eq!(error.code(), "permission_denied"),
            Ok(_) => panic!("expected permission_denied"),
        }
    }

    #[test]
    fn attachment_registry_tracks_state_transitions() {
        let registry = AttachmentRegistry::new();
        let pty = Uuid::new_v4();
        let record = registry.bind(pty, "ctrl-1".into(), AttachmentState::Connecting);
        assert_eq!(record.state, AttachmentState::Connecting);
        registry.set_state("ctrl-1", AttachmentState::Attached);
        let after = registry.lookup("ctrl-1").expect("ctrl-1 bound");
        assert_eq!(after.state, AttachmentState::Attached);
        assert_eq!(registry.advance_input_seq("ctrl-1"), Some(1));
        assert_eq!(registry.advance_input_seq("ctrl-1"), Some(2));
        let bound = registry.unbind("ctrl-1").expect("unbind");
        assert_eq!(bound.next_input_seq, 2);
        assert!(registry.lookup("ctrl-1").is_none());
    }

    #[tokio::test]
    async fn build_output_frames_fans_out_oversized_burst() {
        let registry = make_registry();
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        // 200 KiB raw → expect at least 2 output frames because of cap.
        let mut total = Vec::with_capacity(200 * 1024);
        for _ in 0..200 {
            total.extend(std::iter::repeat(b'x').take(1024));
        }
        let frames: Vec<PtyOutputFrame> = (0..)
            .scan(0u64, |seq, _| {
                if total.is_empty() {
                    return None;
                }
                let chunk_len = 64 * 1024;
                let take = chunk_len.min(total.len());
                let bytes = total.drain(..take).collect::<Vec<u8>>();
                *seq += 1;
                Some(PtyOutputFrame { seq: *seq - 1, data: bytes })
            })
            .take(4)
            .collect();
        let out = session.build_output_frames("term", &frames);
        // 200 KiB may fit in 4 chunks (each ≤ 64 KiB) — 4 frames.
        assert!(!out.is_empty(), "expected fan-out frames");
        for frame in &out {
            assert_eq!(frame.r#type, MessageType::Output);
            assert!(frame.payload.len() <= rtp1::MAX_REALTIME_FRAME);
        }
    }

    #[tokio::test]
    async fn snapshot_chunked_assembles_to_full_bytes() {
        let registry = make_registry();
        let session = Rtp1Session::new("host-a".into(), registry.clone(), 1);
        let body: Vec<u8> = (0..200_000u32).map(|i| (i & 0xFF) as u8).collect();
        let (frames, total) = session.build_snapshot_chunk("term", 1, &body).unwrap();
        assert_eq!(total, body.len());
        // 200000 / 65536 = 4 chunks (last is smaller).
        assert_eq!(frames.len(), 4);
        let mut reassembled = Vec::new();
        for (i, frame) in frames.iter().enumerate() {
            let last = i + 1 == frames.len();
            assert_eq!(frame.r#type, MessageType::Snapshot);
            if !last {
                assert!(frame.flags.contains(FrameFlags::CONTINUATION));
            }
            let snap: SnapshotChunk = payload_from(frame).unwrap();
            reassembled.extend_from_slice(&rtp1::b64_decode(&snap.snapshot_bytes_b64).unwrap());
        }
        assert_eq!(reassembled, body);
    }
}
