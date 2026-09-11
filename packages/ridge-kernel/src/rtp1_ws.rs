//! RTP1 over WebSocket transport binding.
//!
//! SPEC-L2-PROTO-001 §3.9 P5: RTP1 stays transport-neutral. This module is
//! the WS ↔ RTP1 adapter: it owns the WebSocket read/write loops, owns the
//! per-connection `AttachmentRegistry` entry, and drives [`Rtp1Session`].

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};

use crate::pty::{
    detached_output_lease, PtyExitNotification, PtyOutputLease, PtyOutputLeaseError,
    PtyOutputRead, PtyRegistry,
};
use crate::rtp1::{
    self, frame_from, AttachMode, AttachRequest, Capability, CapabilityAdvertise, DetachAck,
    DetachRequest, ErrorFrame, Frame, FrameFlags, MessageType, PingFrame, PongFrame, ReplayRequest,
    ResizeAck, ResizeRequest, ResyncRequest, SessionEvent,
};
use crate::rtp1_session::{
    AttachmentRegistry, AttachmentState, HostInfo, ReplayResult, Rtp1Session,
};

/// Per-connection context shared between the WS read loop and the
/// output-pump task.
#[derive(Clone)]
pub struct WsContext {
    pub session: Arc<Rtp1Session>,
    pub attachments: Arc<AttachmentRegistry>,
}

impl WsContext {
    pub fn new(host_id: String, ptys: Arc<PtyRegistry>, server_version: u32) -> Self {
        Self {
            session: Arc::new(Rtp1Session::new(host_id, ptys, server_version)),
            attachments: Arc::new(AttachmentRegistry::new()),
        }
    }

    pub fn host_info(&self) -> Option<HostInfo> {
        self.session.host_info()
    }
}

async fn send_frame(
    socket: &mut futures::stream::SplitSink<WebSocket, Message>,
    frame: &Frame,
) -> Result<(), String> {
    let bytes = rtp1::encode(frame).map_err(|e| e.to_string())?;
    socket
        .send(Message::Binary(bytes.into()))
        .await
        .map_err(|e| e.to_string())
}

pub async fn send_capability_advertise(
    socket: &mut futures::stream::SplitSink<WebSocket, Message>,
    capability: &Capability,
) -> Result<(), String> {
    let payload = CapabilityAdvertise {
        terminal_id: None,
        features: capability.features.clone(),
        max_realtime_frame: capability.max_realtime_frame,
        max_snapshot_chunk: capability.max_snapshot_chunk,
        replay_cap_bytes: capability.replay_cap_bytes,
        replay_cap_frames: capability.replay_cap_frames,
    };
    let frame = frame_from(MessageType::CapabilityAdvertise, &payload, FrameFlags::empty())
        .map_err(|e| e.to_string())?;
    send_frame(socket, &frame).await
}

pub async fn send_error(
    socket: &mut futures::stream::SplitSink<WebSocket, Message>,
    terminal_id: Option<&str>,
    code: &str,
    message: &str,
) -> Result<(), String> {
    let payload = ErrorFrame {
        terminal_id: terminal_id.map(str::to_string),
        controller_id: None,
        code: code.into(),
        message: message.into(),
    };
    let frame = frame_from(MessageType::Error, &payload, FrameFlags::empty())
        .map_err(|e| e.to_string())?;
    send_frame(socket, &frame).await
}

/// Output frames pushed by the spawn-pump task; the WS read loop drains this
/// channel and forwards each to the client.
type OutputTx = mpsc::Sender<Frame>;

/// Drive a single RTP1 over WebSocket connection. Returns when the client
/// closes the socket or the server tears down.
pub async fn drive(socket: WebSocket, ctx: WsContext) {
    let (mut tx, mut rx) = socket.split();
    if send_capability_advertise(&mut tx, &ctx.session.capability)
        .await
        .is_err()
    {
        return;
    }

    let session = ctx.session.clone();
    let attachments = ctx.attachments.clone();
    let (output_tx, mut output_rx) = mpsc::channel::<Frame>(64);
    let active_controller: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let output_pump_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(None));

    loop {
        tokio::select! {
            biased;
            ws_msg = rx.next() => {
                let Some(msg) = ws_msg else { break };
                let Ok(msg) = msg else { break };
                match msg {
                    Message::Binary(bytes) => {
                        let bytes = bytes.to_vec();
                        let frame = match rtp1::decode(&bytes) {
                            Ok((frame, _consumed)) => frame,
                            Err(error) => {
                                let _ = send_error(&mut tx, None, "protocol_violation", &error.to_string()).await;
                                break;
                            }
                        };
                        if handle_frame(
                            &mut tx,
                            &session,
                            &attachments,
                            &active_controller,
                            &output_pump_handle,
                            &output_tx,
                            frame,
                        ).await {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    Message::Ping(payload) => { let _ = tx.send(Message::Pong(payload)).await; }
                    Message::Pong(_) => {}
                    _ => {}
                }
            }
            out = output_rx.recv() => {
                let Some(frame) = out else { continue };
                if send_frame(&mut tx, &frame).await.is_err() {
                    break;
                }
            }
        }
    }

    if let Some(handle) = output_pump_handle.lock().await.take() {
        handle.abort();
    }
    let controller_id = active_controller.lock().await.take();
    if let Some(controller_id) = controller_id {
        attachments.unbind(&controller_id);
    }
}

/// Returns true when the connection should close after the frame was
/// processed (e.g. fatal protocol violation or controller teardown).
#[allow(clippy::too_many_arguments)]
async fn handle_frame(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    attachments: &Arc<AttachmentRegistry>,
    active_controller: &Arc<Mutex<Option<String>>>,
    output_pump_handle: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    output_tx: &OutputTx,
    frame: Frame,
) -> bool {
    match frame.r#type {
        MessageType::Attach => match rtp1::payload_from::<AttachRequest>(&frame) {
            Ok(req) => {
                handle_attach(
                    tx,
                    session,
                    attachments,
                    active_controller,
                    output_pump_handle,
                    output_tx.clone(),
                    req,
                )
                .await
            }
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::Detach => match rtp1::payload_from::<DetachRequest>(&frame) {
            Ok(req) => {
                handle_detach(
                    tx,
                    session,
                    attachments,
                    active_controller,
                    output_pump_handle,
                    req,
                )
                .await
            }
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::Input => match rtp1::payload_from::<crate::rtp1::InputFrame>(&frame) {
            Ok(input) => handle_input(tx, session, active_controller, input).await,
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::Resize => match rtp1::payload_from::<ResizeRequest>(&frame) {
            Ok(req) => handle_resize(tx, session, active_controller, req).await,
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::Replay => match rtp1::payload_from::<ReplayRequest>(&frame) {
            Ok(req) => handle_replay(tx, session, req).await,
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::Resync => match rtp1::payload_from::<ResyncRequest>(&frame) {
            Ok(req) => handle_resync(tx, session, req).await,
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::Ping => match rtp1::payload_from::<PingFrame>(&frame) {
            Ok(ping) => {
                let pong = PongFrame { nonce: ping.nonce };
                if let Ok(pong_frame) = frame_from(MessageType::Pong, &pong, FrameFlags::empty()) {
                    let _ = send_frame(tx, &pong_frame).await;
                }
                false
            }
            Err(error) => {
                let _ = send_error(tx, None, "protocol_violation", &error.to_string()).await;
                false
            }
        },
        MessageType::CapabilityAdvertise => false,
        _ => {
            let _ = send_error(tx, None, "protocol_violation", "unexpected message type").await;
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_attach(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    attachments: &Arc<AttachmentRegistry>,
    active_controller: &Arc<Mutex<Option<String>>>,
    output_pump_handle: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    output_tx: OutputTx,
    req: AttachRequest,
) -> bool {
    let terminal_id = match uuid::Uuid::parse_str(&req.terminal_id) {
        Ok(id) => id,
        Err(error) => {
            let _ = send_error(
                tx,
                Some(&req.terminal_id),
                "protocol_violation",
                &error.to_string(),
            )
            .await;
            return false;
        }
    };
    let (ack, lease_opt, state) = match session.handle_attach(&req) {
        Ok(tuple) => tuple,
        Err(error) => {
            let _ = send_error(tx, Some(&req.terminal_id), error.code(), &error.to_string()).await;
            attachments.bind(terminal_id, req.controller_id.clone(), AttachmentState::Failed);
            return false;
        }
    };
    attachments.bind(terminal_id, req.controller_id.clone(), state);
    session.ptys.attach_controller(terminal_id, req.controller_id.clone());
    let ack_frame = match frame_from(MessageType::AttachAck, &ack, FrameFlags::empty()) {
        Ok(frame) => frame,
        Err(_) => return false,
    };
    if send_frame(tx, &ack_frame).await.is_err() {
        return true;
    }

    // Snapshot mode: stream snapshot chunks then continue with delta.
    if matches!(ack.mode, AttachMode::Snapshot) {
        if let Ok(scrollback) = session.ptys.scrollback(terminal_id, 64 * 1024) {
            if let Ok((frames, _)) = session.build_snapshot_chunk(&req.terminal_id, 1, &scrollback) {
                for frame in frames {
                    if send_frame(tx, &frame).await.is_err() {
                        return true;
                    }
                }
            }
        }
    }

    // Spawn the output pump task.
    if let Some(lease) = lease_opt {
        let exit_recv = session.subscribe_exit(terminal_id);
        let session_for_task = session.clone();
        let output_tx_for_task = output_tx.clone();
        let terminal_id_str = terminal_id.to_string();
        let handle = tokio::spawn(async move {
            run_output_pump(
                lease,
                terminal_id_str,
                exit_recv,
                session_for_task,
                output_tx_for_task,
            )
            .await;
        });
        *output_pump_handle.lock().await = Some(handle);
    }

    *active_controller.lock().await = Some(req.controller_id.clone());
    // Keep the lease sentinel alive for the duration of this connection so
    // the borrowed &PtyOutputLease in the spawned pump has nothing to do
    // with a connection-scoped holder.
    let _ = detached_output_lease();
    false
}

async fn handle_detach(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    attachments: &Arc<AttachmentRegistry>,
    active_controller: &Arc<Mutex<Option<String>>>,
    output_pump_handle: &Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    req: DetachRequest,
) -> bool {
    let ack = match session.handle_detach(&req) {
        Ok(ack) => ack,
        Err(error) => {
            let _ = send_error(tx, Some(&req.terminal_id), error.code(), &error.to_string()).await;
            return false;
        }
    };
    if let Some(handle) = output_pump_handle.lock().await.take() {
        handle.abort();
    }
    attachments.set_state(&req.controller_id, AttachmentState::Closing);
    let ack_payload = DetachAck {
        terminal_id: ack.terminal_id.clone(),
        controller_id: ack.controller_id.clone(),
        last_output_seq: ack.last_output_seq,
    };
    if let Ok(frame) = frame_from(MessageType::DetachAck, &ack_payload, FrameFlags::empty()) {
        let _ = send_frame(tx, &frame).await;
    }
    attachments.set_state(&req.controller_id, AttachmentState::Detached);
    if let Ok(terminal_id) = uuid::Uuid::parse_str(&req.terminal_id) {
        session.ptys.detach_controller(terminal_id, &req.controller_id);
    }
    let mut guard = active_controller.lock().await;
    *guard = None;
    drop(guard);
    true
}

async fn handle_input(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    active_controller: &Arc<Mutex<Option<String>>>,
    input: crate::rtp1::InputFrame,
) -> bool {
    let controller_id = {
        let guard = active_controller.lock().await;
        guard.clone()
    };
    if controller_id.as_deref() != Some(input.controller_id.as_str()) {
        let _ = send_error(
            tx,
            Some(&input.terminal_id),
            "controller_id_unknown",
            "no attached controller matches input.controller_id",
        )
        .await;
        return false;
    }
    match session.handle_input(&input) {
        Ok(ack) => {
            if let Ok(frame) = frame_from(MessageType::InputAck, &ack, FrameFlags::empty()) {
                let _ = send_frame(tx, &frame).await;
            }
        }
        Err(error) => {
            let _ = send_error(tx, Some(&input.terminal_id), error.code(), &error.to_string()).await;
        }
    }
    false
}

async fn handle_resize(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    active_controller: &Arc<Mutex<Option<String>>>,
    req: ResizeRequest,
) -> bool {
    let controller_id = {
        let guard = active_controller.lock().await;
        guard.clone()
    };
    if controller_id.as_deref() != Some(req.controller_id.as_str()) {
        let _ = send_error(
            tx,
            Some(&req.terminal_id),
            "controller_id_unknown",
            "no attached controller matches resize.controller_id",
        )
        .await;
        return false;
    }
    match session.handle_resize(&req) {
        Ok(ack) => {
            let payload = ResizeAck {
                terminal_id: ack.terminal_id,
                controller_id: ack.controller_id,
                rows: ack.rows,
                cols: ack.cols,
                next_output_seq: ack.next_output_seq,
            };
            if let Ok(frame) = frame_from(MessageType::ResizeAck, &payload, FrameFlags::empty()) {
                let _ = send_frame(tx, &frame).await;
            }
        }
        Err(error) => {
            let _ = send_error(tx, Some(&req.terminal_id), error.code(), &error.to_string()).await;
        }
    }
    false
}

async fn handle_replay(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    req: ReplayRequest,
) -> bool {
    match session.handle_replay(&req) {
        Ok(ReplayResult::Data(data)) => {
            if let Ok(frame) = frame_from(MessageType::ReplayData, &data, FrameFlags::empty()) {
                let _ = send_frame(tx, &frame).await;
            }
        }
        Ok(ReplayResult::Lagged {
            requested_seq,
            oldest_seq,
            head,
        }) => {
            let _ = send_error(
                tx,
                Some(&req.terminal_id),
                "replay_unavailable",
                &format!("lagged: requested {requested_seq}, oldest {oldest_seq}, head {head}"),
            )
            .await;
        }
        Err(error) => {
            let _ = send_error(tx, Some(&req.terminal_id), error.code(), &error.to_string()).await;
        }
    }
    false
}

async fn handle_resync(
    tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    session: &Arc<Rtp1Session>,
    req: ResyncRequest,
) -> bool {
    let ack = match session.handle_resync(&req) {
        Ok(ack) => ack,
        Err(error) => {
            let _ = send_error(tx, Some(&req.terminal_id), error.code(), &error.to_string()).await;
            return false;
        }
    };
    let terminal_id = match uuid::Uuid::parse_str(&req.terminal_id) {
        Ok(id) => id,
        Err(_) => return false,
    };
    if matches!(ack.mode, AttachMode::Snapshot) {
        if let Ok(scrollback) = session.ptys.scrollback(terminal_id, 64 * 1024) {
            if let Ok((frames, _)) = session.build_snapshot_chunk(&req.terminal_id, 1, &scrollback)
            {
                for frame in frames {
                    if send_frame(tx, &frame).await.is_err() {
                        return true;
                    }
                }
            }
        }
    }
    false
}

async fn run_output_pump(
    lease: PtyOutputLease,
    terminal_id_str: String,
    mut exit_recv: Option<tokio::sync::broadcast::Receiver<PtyExitNotification>>,
    session: Arc<Rtp1Session>,
    output_tx: OutputTx,
) {
    let lease = lease;
    loop {
        tokio::select! {
            read = lease.next(Duration::from_millis(50), 64) => {
                match read {
                    Ok(PtyOutputRead::Data(frames)) => {
                        if frames.is_empty() {
                            continue;
                        }
                        for frame in session.build_output_frames(&terminal_id_str, &frames) {
                            if output_tx.send(frame).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(PtyOutputRead::Lagged { requested_seq, oldest_seq, latest_seq }) => {
                        let payload = ErrorFrame {
                            terminal_id: Some(terminal_id_str.clone()),
                            controller_id: None,
                            code: "replay_unavailable".into(),
                            message: format!("lagged: requested {requested_seq}, oldest {oldest_seq}, head {latest_seq}"),
                        };
                        if let Ok(frame) = frame_from(MessageType::Error, &payload, FrameFlags::empty()) {
                            if output_tx.send(frame).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(PtyOutputLeaseError::TimedOut) => continue,
                    Err(_) => return,
                }
            }
            exit = async {
                match exit_recv.as_mut() {
                    Some(rx) => match rx.recv().await {
                        Ok(notification) => Some(notification),
                        Err(_) => None,
                    },
                    None => std::future::pending::<Option<PtyExitNotification>>().await,
                }
            } => {
                if let Some(notification) = exit {
                    let event = SessionEvent {
                        terminal_id: notification.pty_id.to_string(),
                        event: crate::rtp1::SessionEventKind::Exited,
                        code: notification.code,
                    };
                    if let Ok(frame) = frame_from(MessageType::SessionEvent, &event, FrameFlags::empty()) {
                        if output_tx.send(frame).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::PtyLaunch;
    use crate::rtp1_session::AttachmentRegistry;

    fn setup() -> (Arc<PtyRegistry>, Arc<Rtp1Session>) {
        let registry = Arc::new(PtyRegistry::default());
        registry.set_runtime_epoch("epoch-test".into());
        let session = Arc::new(Rtp1Session::new("host-a".into(), registry.clone(), 1));
        (registry, session)
    }

    #[tokio::test]
    async fn capability_advertise_features_locked() {
        let (_registry, session) = setup();
        let cap = session.capability.clone();
        for required in [
            "rtp1.v1",
            "input_seq",
            "replay",
            "snapshot",
            "session_event.exited",
            "runtime_epoch",
        ] {
            assert!(
                cap.features.iter().any(|f| f == required),
                "missing capability {required}",
            );
        }
    }

    #[tokio::test]
    async fn ws_context_host_info_returns_epoch() {
        let (registry, _) = setup();
        let ctx = WsContext::new("host-a".into(), registry, 1);
        let info = ctx.host_info().expect("host info present");
        assert_eq!(info.host_id, "host-a");
        assert_eq!(info.runtime_epoch, "epoch-test");
    }

    #[tokio::test]
    async fn handle_attach_records_state() {
        let (registry, session) = setup();
        let pty = registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "test",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn");
        let attachments = Arc::new(AttachmentRegistry::new());
        let req = AttachRequest {
            host_id: "host-a".into(),
            runtime_epoch: "epoch-test".into(),
            session_id: "s".into(),
            terminal_id: pty.to_string(),
            controller_id: "ctrl-1".into(),
            since_output_seq: None,
            mode: AttachMode::Raw,
            client_min_version: 1,
            client_max_version: 1,
        };
        let (_ack, _lease, state) = session.handle_attach(&req).expect("attach");
        assert_eq!(state, AttachmentState::Attached);
        attachments.bind(pty, "ctrl-1".into(), state);
        assert!(attachments.lookup("ctrl-1").is_some());
    }

    #[tokio::test]
    async fn snapshot_chunked_continuation_chain_via_session() {
        let (_registry, session) = setup();
        let body = vec![0xCCu8; 200_000];
        let (frames, total) = session.build_snapshot_chunk("term", 1, &body).unwrap();
        assert_eq!(total, body.len());
        assert!(frames.len() > 1);
        assert!(frames[..frames.len() - 1]
            .iter()
            .all(|f| f.flags.contains(FrameFlags::CONTINUATION)));
        assert!(!frames
            .last()
            .unwrap()
            .flags
            .contains(FrameFlags::CONTINUATION));
        let mut acc = Vec::new();
        for frame in frames {
            let snap: crate::rtp1::SnapshotChunk = rtp1::payload_from(&frame).unwrap();
            acc.extend_from_slice(&rtp1::b64_decode(&snap.snapshot_bytes_b64).unwrap());
        }
        assert_eq!(acc, body);
    }

    #[tokio::test]
    async fn capability_frame_round_trip_via_encode_decode() {
        let cap = CapabilityAdvertise {
            terminal_id: None,
            features: vec!["rtp1.v1".into(), "input_seq".into()],
            max_realtime_frame: rtp1::MAX_REALTIME_FRAME,
            max_snapshot_chunk: 256 * 1024,
            replay_cap_bytes: 256 * 1024,
            replay_cap_frames: 256,
        };
        let frame = frame_from(MessageType::CapabilityAdvertise, &cap, FrameFlags::empty()).unwrap();
        let bytes = rtp1::encode(&frame).unwrap();
        let (parsed, _) = rtp1::decode(&bytes).unwrap();
        let back: CapabilityAdvertise = rtp1::payload_from(&parsed).unwrap();
        assert_eq!(back.features, cap.features);
    }
}
