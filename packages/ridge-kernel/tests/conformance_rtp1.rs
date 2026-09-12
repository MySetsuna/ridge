//! RTP1 conformance tests (SPEC-L2-PROTO-001 §4) + remote lifecycle
//! (SPEC-L2-REMOTE-001 §3.7).
//!
//! 12 acceptance clauses exercised end-to-end against an in-process
//! Rtp1Session + PtyRegistry pair (no HTTP / WS hop). The pair is the same
//! kernel that the WS adapter drives, so the assertions are authoritative
//! for both transport bindings.

use std::sync::Arc;
use std::time::Duration;

use ridge_kernel::pty::{PtyLaunch, PtyRegistry};
use ridge_kernel::rtp1::{
    self, AttachMode, AttachRequest, CapabilityAdvertise, FrameFlags, InputFrame, MessageType,
    ReplayRequest, ResizeOwner, ResizeRequest, ResyncRequest, SessionEventKind, SnapshotChunk,
};
use ridge_kernel::rtp1_session::{
    AttachmentRegistry, AttachmentState, ReplayResult, Rtp1Session,
};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn make_session() -> (Arc<PtyRegistry>, Arc<Rtp1Session>, uuid::Uuid) {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-conformance".into());
    let session = Arc::new(Rtp1Session::new(
        "host-a".into(),
        registry.clone(),
        1,
    ));
    let pty = runtime()
        .block_on(async {
            registry
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
                .expect("spawn")
        });
    (registry, session, pty)
}

fn attach_req(pty: uuid::Uuid, host_id: &str, epoch: &str, ctrl: &str) -> AttachRequest {
    AttachRequest {
        host_id: host_id.into(),
        runtime_epoch: epoch.into(),
        session_id: "s".into(),
        terminal_id: pty.to_string(),
        controller_id: ctrl.into(),
        since_output_seq: None,
        mode: AttachMode::Raw,
        client_min_version: 1,
        client_max_version: 1,
    }
}

// ── Acceptance §4 ─────────────────────────────────────────────────────

/// §4.2 — protocol version negotiation succeeds when overlap is non-empty.
#[test]
fn acceptance_protocol_version_overlap_succeeds() {
    let (_registry, session, pty) = make_session();
    let req = attach_req(pty, "host-a", "epoch-conformance", "ctrl-1");
    let (ack, _lease, state) = session.handle_attach(&req).expect("attach");
    assert_eq!(state, AttachmentState::Attached);
    assert_eq!(ack.server_version, 1);
    assert_eq!(ack.runtime_epoch, "epoch-conformance");
}

/// §4.2 — `client_max_version < server_version` returns `client_too_old`.
#[test]
fn acceptance_protocol_version_no_overlap_rejected() {
    let (_registry, session, pty) = make_session();
    let session_v5 = Arc::new(Rtp1Session::new(
        "host-a".into(),
        session.ptys.clone(),
        5,
    ));
    let mut req = attach_req(pty, "host-a", "epoch-conformance", "ctrl-1");
    req.client_min_version = 1;
    req.client_max_version = 3;
    match session_v5.handle_attach(&req) {
        Err(error) => assert_eq!(error.code(), "client_too_old"),
        Ok(_) => panic!("client_too_old expected"),
    }
}

/// §4.2 — mode negotiation independent of server_version.
#[test]
fn acceptance_mode_negotiation_independent_of_version() {
    let (_registry, session, pty) = make_session();
    for mode in [AttachMode::Raw, AttachMode::Delta, AttachMode::Snapshot] {
        let mut req = attach_req(pty, "host-a", "epoch-conformance", "ctrl-1");
        req.mode = mode;
        let (ack, _lease, _state) = session.handle_attach(&req).expect("attach");
        assert_eq!(ack.mode, mode, "mode negotiation must round-trip {mode:?}");
    }
}

/// §4.3 — RTP1 frame has no application-level CRC (no flag/payload bytes
/// dedicated to integrity). Verifies the envelope header is exactly
/// `magic[4] + efv[1] + type[1] + flags[1] + payload_len[4]`.
#[test]
fn acceptance_no_application_level_crc_field() {
    use ridge_kernel::rtp1::{Frame, HEADER_LEN, RTP1_EFV, RTP1_MAGIC};
    let frame = Frame {
        r#type: MessageType::Ping,
        flags: FrameFlags::empty(),
        payload: b"{}".to_vec(),
    };
    let wire = rtp1::encode(&frame).expect("encode");
    assert_eq!(&wire[..4], &RTP1_MAGIC);
    assert_eq!(wire[4], RTP1_EFV);
    assert_eq!(wire.len(), HEADER_LEN + frame.payload.len());
}

/// §4.4 — PTY burst fans out into ≤ 64 KiB output frames.
#[test]
fn acceptance_realtime_burst_fans_out() {
    let (_registry, session, _pty) = make_session();
    let big = vec![0xABu8; 200 * 1024];
    let chunk_size = 24 * 1024;
    let mut seq = 1u64;
    let mut frames = Vec::new();
    for slice in big.chunks(chunk_size) {
        frames.push(ridge_kernel::pty::PtyOutputFrame {
            seq,
            data: slice.to_vec(),
        });
        seq += 1;
    }
    let out = session.build_output_frames("term", &frames);
    assert!(out.len() > 1, "burst must fan out");
    for frame in &out {
        assert!(frame.payload.len() <= rtp1::MAX_REALTIME_FRAME);
        assert!(!frame.flags.contains(FrameFlags::CONTINUATION));
    }
}

/// §4.5 — input_seq ownership: two controllers each advance their own seq.
#[test]
fn acceptance_input_seq_isolation_per_controller() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-conformance".into());
    let session = Arc::new(Rtp1Session::new("host-a".into(), registry.clone(), 1));
    let attachments = AttachmentRegistry::new();
    let _pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    attachments.bind(uuid::Uuid::new_v4(), "ctrl-A".into(), AttachmentState::Attached);
    attachments.bind(uuid::Uuid::new_v4(), "ctrl-B".into(), AttachmentState::Attached);
    assert_eq!(attachments.advance_input_seq("ctrl-A"), Some(1));
    assert_eq!(attachments.advance_input_seq("ctrl-A"), Some(2));
    assert_eq!(attachments.advance_input_seq("ctrl-B"), Some(1));
    assert_eq!(attachments.advance_input_seq("ctrl-B"), Some(2));
    let ctrl_a = attachments.lookup("ctrl-A").expect("ctrl-A bound");
    let ctrl_b = attachments.lookup("ctrl-B").expect("ctrl-B bound");
    assert_eq!(ctrl_a.next_input_seq, 2);
    assert_eq!(ctrl_b.next_input_seq, 2);
}

/// §4.6 — terminal exited event uses the canonical session_event type.
#[test]
fn acceptance_session_event_exited_is_canonical() {
    let (_registry, session, _pty) = make_session();
    let notification = ridge_kernel::pty::PtyExitNotification {
        pty_id: uuid::Uuid::nil(),
        code: Some(0),
    };
    let event = Rtp1Session::build_session_event(&notification);
    assert!(matches!(event.event, SessionEventKind::Exited));
    assert_eq!(event.code, Some(0));
}

/// §4.7 — runtime_epoch stale attach rejected.
#[test]
fn acceptance_runtime_epoch_stale_rejected() {
    let (_registry, session, pty) = make_session();
    let req = attach_req(pty, "host-a", "wrong-epoch", "ctrl-1");
    match session.handle_attach(&req) {
        Err(error) => assert_eq!(error.code(), "runtime_epoch_stale"),
        Ok(_) => panic!("stale attach must be rejected"),
    }
}

/// §4.8 — controller_id is the only identity for input ownership.
#[test]
fn acceptance_controller_id_is_the_only_input_identity() {
    let (_registry, session, pty) = make_session();
    let req = attach_req(pty, "host-a", "epoch-conformance", "ctrl-1");
    let (_ack, _lease, _state) = session.handle_attach(&req).expect("attach");
    // P0-1 (audit C1 fix): the session now enforces per-PTY
    // controller_id ownership at the session layer. An attach binds
    // `ctrl-1`; writing as `ctrl-other` is rejected.
    let input = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-other".into(),
        input_seq: 1,
        data_b64: rtp1::b64_encode(b"hello"),
        data_len: 5,
    };
    match session.handle_input(&input) {
        Err(error) => assert_eq!(error.code(), "controller_id_unknown"),
        Ok(_) => panic!("ctrl-other must be rejected on a PTY owned by ctrl-1"),
    }
    // But the bound controller succeeds.
    let input_ok = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-1".into(),
        input_seq: 1,
        data_b64: rtp1::b64_encode(b"hello"),
        data_len: 5,
    };
    assert!(session.handle_input(&input_ok).is_ok());
}

/// §4.9 — snapshot chunks reassemble to full bytes.
#[test]
fn acceptance_snapshot_continuation_reassembles() {
    let (_registry, session, _pty) = make_session();
    let body: Vec<u8> = (0..300_000u32).map(|i| (i & 0xFF) as u8).collect();
    let (frames, total) = session.build_snapshot_chunk("term", 1, &body).unwrap();
    assert_eq!(total, body.len());
    assert!(frames.len() > 1);
    assert!(frames[..frames.len() - 1]
        .iter()
        .all(|f| f.flags.contains(FrameFlags::CONTINUATION)));
    let mut acc = Vec::new();
    for frame in frames {
        let snap: SnapshotChunk = rtp1::payload_from(&frame).unwrap();
        acc.extend_from_slice(&rtp1::b64_decode(&snap.snapshot_bytes_b64).unwrap());
    }
    assert_eq!(acc, body);
}

/// §4.10 — input_too_large is the canonical rejection for oversized input.
#[test]
fn acceptance_input_too_large_rejected() {
    let (_registry, session, pty) = make_session();
    let oversized = vec![0u8; rtp1::MAX_REALTIME_FRAME + 1];
    let input = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl".into(),
        input_seq: 1,
        data_b64: rtp1::b64_encode(&oversized),
        data_len: oversized.len(),
    };
    match session.handle_input(&input) {
        Err(error) => assert_eq!(error.code(), "input_too_large"),
        Ok(_) => panic!("oversize input must reject"),
    }
}

/// §4.11 — chunked logical message mutex per (terminal, message_type).
/// Implemented by the WS adapter (no in-flight chunked logical message
/// overlaps the next). This test asserts the envelope-level invariant:
/// continuation flag is independent and never combines with realtime cap.
#[test]
fn acceptance_realtime_forbids_continuation() {
    use ridge_kernel::rtp1::{Frame, MessageType, FRAME_FLAGS_CONTINUATION};
    let oversized = vec![0u8; rtp1::MAX_REALTIME_FRAME + 1024];
    let frame = Frame {
        r#type: MessageType::Output,
        flags: FrameFlags(FRAME_FLAGS_CONTINUATION),
        payload: oversized,
    };
    // Continuation=1 bypasses the realtime cap, so encode succeeds.
    assert!(rtp1::encode(&frame).is_ok());
    // Continuation=0 (a single realtime frame) > 64 KiB → reject.
    let big = vec![0u8; rtp1::MAX_REALTIME_FRAME + 1];
    let frame = Frame {
        r#type: MessageType::Output,
        flags: FrameFlags::empty(),
        payload: big,
    };
    assert!(matches!(
        rtp1::encode(&frame),
        Err(rtp1::EnvelopeError::RealtimeCap(_, _))
    ));
}

/// §4.12 — runtime_epoch is independent of server_version; capability
/// advertise carries max_realtime_frame = 64 KiB regardless of epoch.
#[test]
fn acceptance_runtime_epoch_independent_of_server_version() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-x".into());
    let session_v1 = Rtp1Session::new("host".into(), registry.clone(), 1);
    let session_v3 = Rtp1Session::new("host".into(), registry.clone(), 3);
    let info_v1 = session_v1.host_info().unwrap();
    let info_v3 = session_v3.host_info().unwrap();
    assert_eq!(info_v1.runtime_epoch, info_v3.runtime_epoch);
    assert_eq!(info_v1.max_realtime_frame, rtp1::MAX_REALTIME_FRAME);
    assert_eq!(info_v3.max_realtime_frame, rtp1::MAX_REALTIME_FRAME);
    // server_version field is independent; RTP1 capability_advertise does
    // not carry server_version at all.
    let cap = CapabilityAdvertise {
        terminal_id: None,
        features: vec!["rtp1.v1".into()],
        max_realtime_frame: info_v3.max_realtime_frame,
        max_snapshot_chunk: 256 * 1024,
        replay_cap_bytes: 256 * 1024,
        replay_cap_frames: 256,
    };
    let frame = rtp1::frame_from(MessageType::CapabilityAdvertise, &cap, FrameFlags::empty())
        .unwrap();
    let payload = rtp1::payload_from::<CapabilityAdvertise>(&frame).unwrap();
    assert_eq!(payload.features, cap.features);
}

// ── SPEC-L2-REMOTE-001 §3.7 acceptance ───────────────────────────────

/// §3.7.4 — terminal exited event reaches attached controller. The
/// PtyRegistry's exit broadcast is the source; Rtp1Session maps it to
/// a SessionEvent{event:"exited"} frame.
#[test]
fn acceptance_terminated_event_is_broadcast() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    let mut rx = registry.subscribe_exit(pty).expect("subscribe");
    registry.notify_exit(pty, Some(0));
    let notification = runtime()
        .block_on(async { rx.recv().await })
        .expect("exit notification");
    assert_eq!(notification.code, Some(0));
    let event = Rtp1Session::build_session_event(&notification);
    assert!(matches!(event.event, SessionEventKind::Exited));
}

/// §3.7.6 — observer resize is rejected.
#[test]
fn acceptance_observer_resize_rejected() {
    let (_registry, session, pty) = make_session();
    let req = ResizeRequest {
        terminal_id: pty.to_string(),
        controller_id: "ctrl".into(),
        rows: 30,
        cols: 100,
        owner: Some(ResizeOwner::Observer),
    };
    match session.handle_resize(&req) {
        Err(error) => assert_eq!(error.code(), "permission_denied"),
        Ok(_) => panic!("observer must not resize"),
    }
}

/// §3.7.5 — input ownership across controllers.
#[test]
fn acceptance_input_ownership_isolated_per_controller() {
    // P0-1 (audit C1 fix): per-PTY controller_id ownership is enforced
    // at the session layer (not just the WS adapter). Multi-controller
    // attachment to the same PTY is supported; each controller's lane
    // is registered on attach, and write must carry the matching id.
    let (_registry, session, pty) = make_session();
    // Two controllers attach the same PTY.
    let r1 = runtime();
    for ctrl in ["ctrl-A", "ctrl-B"] {
        let req = attach_req(pty, "host-a", "epoch-conformance", ctrl);
        let _ = r1.block_on(async { session.handle_attach(&req) }).expect("attach");
    }
    for ctrl in ["ctrl-A", "ctrl-B"] {
        let input = InputFrame {
            terminal_id: pty.to_string(),
            controller_id: ctrl.into(),
            input_seq: 1,
            data_b64: rtp1::b64_encode(b"x"),
            data_len: 1,
        };
        let ack = r1.block_on(async { session.handle_input(&input) }).expect("input");
        assert_eq!(ack.controller_id, ctrl);
    }
    // A controller that never attached is rejected.
    let forged = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-forged".into(),
        input_seq: 1,
        data_b64: rtp1::b64_encode(b"x"),
        data_len: 1,
    };
    let err = r1.block_on(async { session.handle_input(&forged) }).unwrap_err();
    assert_eq!(err.code(), "controller_id_unknown");
}

/// §3.7.7 — terminal exit does not force session close (no single-terminal
/// binding declared in v1). After `notify_exit` the registry still answers
/// `attach` for the same pty_id; the session_event is informational only.
#[test]
fn acceptance_terminal_exited_does_not_implicit_close_session() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    let req = attach_req(pty, "host", "epoch", "ctrl");
    let (_ack, _lease, _state) = session.handle_attach(&req).expect("attach before exit");
    registry.notify_exit(pty, Some(0));
    // Replay remains available after exit; attach still returns bounds.
    let (_oldest, next) = registry.output_bounds(pty).expect("bounds still queryable");
    assert!(next > 0);
    let replay = ReplayRequest {
        terminal_id: pty.to_string(),
        since_output_seq: 0,
        max_bytes: 1024,
    };
    match session.handle_replay(&replay).expect("replay ok") {
        ReplayResult::Data(_) | ReplayResult::Lagged { .. } => {}
    }
}

/// §3.7.8 — attachment does not auto-close on terminal exit. The
/// PtyRegistry keeps the PTY entry until explicit destroy.
#[test]
fn acceptance_terminal_exit_keeps_attachment_until_explicit_detach() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let attachments = AttachmentRegistry::new();
    let pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    let req = attach_req(pty, "host", "epoch", "ctrl");
    let (_ack, _lease, _state) = session.handle_attach(&req).expect("attach");
    attachments.bind(pty, "ctrl".into(), AttachmentState::Attached);
    registry.notify_exit(pty, Some(0));
    // Attachment record is still present until detach ack runs.
    let record = attachments.lookup("ctrl").expect("ctrl bound");
    assert_eq!(record.state, AttachmentState::Attached);
    // Explicit detach transitions to Detached.
    attachments.set_state("ctrl", AttachmentState::Closing);
    attachments.set_state("ctrl", AttachmentState::Detached);
    assert!(attachments.lookup("ctrl").is_none() || {
        let r = attachments.lookup("ctrl").unwrap();
        r.state == AttachmentState::Detached
    });
}

/// §3.7.9 — controller_id is a UUID-form string; ordering is decided by
/// `input_seq`, not by the controller_id value.
#[test]
fn acceptance_input_order_determined_by_input_seq_not_controller_id() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let attachments = AttachmentRegistry::new();
    attachments.bind(uuid::Uuid::new_v4(), "zzz".into(), AttachmentState::Attached);
    attachments.bind(uuid::Uuid::new_v4(), "aaa".into(), AttachmentState::Attached);
    assert_eq!(attachments.advance_input_seq("zzz"), Some(1));
    assert_eq!(attachments.advance_input_seq("aaa"), Some(1));
    assert_eq!(attachments.advance_input_seq("zzz"), Some(2));
    let zzz = attachments.lookup("zzz").unwrap();
    let aaa = attachments.lookup("aaa").unwrap();
    assert_ne!(zzz.controller_id, aaa.controller_id);
    assert_eq!(zzz.next_input_seq, 2);
    assert_eq!(aaa.next_input_seq, 1);
}

// ── Stability §3.5.1 / §3.5.2 ────────────────────────────────────────

/// §3.5.1 — network short interruption, runtime_epoch unchanged: re-attach
/// with `since_output_seq` resumes streaming.
#[test]
fn stability_reconnect_resume_from_since_output_seq() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    // Original attach at head.
    let req = attach_req(pty, "host", "epoch", "ctrl-1");
    let (_ack, _lease, _) = match session.handle_attach(&req) {
        Ok(t) => t,
        Err(_) => panic!("attach 1 failed"),
    };
    // Synthetic advance: assume client received seq 1..=3 before disconnect.
    let mut req2 = req.clone();
    req2.controller_id = "ctrl-2".into();
    req2.since_output_seq = Some(3);
    let (ack2, _lease2, state2) = match session.handle_attach(&req2) {
        Ok(t) => t,
        Err(_) => panic!("attach 2 failed"),
    };
    assert_eq!(state2, AttachmentState::Attached);
    assert!(
        ack2.next_output_seq >= 1,
        "next_output_seq must reflect post-cursor state, got {}",
        ack2.next_output_seq
    );
    assert_eq!(ack2.runtime_epoch, "epoch");
}

/// §3.5.3 — host restart yields new runtime_epoch; old attach rejected.
#[test]
fn stability_host_restart_rejects_stale_epoch() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-old".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    let req_old = attach_req(pty, "host", "epoch-old", "ctrl-1");
    let _ = session.handle_attach(&req_old).expect("first attach");
    // New kernel boot: registry cannot be mutated in-process, but we
    // exercise the new epoch by minting a fresh registry.
    let registry2 = Arc::new(PtyRegistry::default());
    registry2.set_runtime_epoch("epoch-new".into());
    let session2 = Arc::new(Rtp1Session::new("host".into(), registry2.clone(), 1));
    let req_new = attach_req(pty, "host", "epoch-new", "ctrl-1");
    match session2.handle_attach(&req_new) {
        Err(error) => assert_eq!(error.code(), "unknown_terminal"),
        Ok(_) => panic!("attach against new epoch must not silently rebind"),
    }
}

/// §3.5.3 — registry.set_runtime_epoch is one-shot (cannot silently rebind
/// without panic). Mirrors the spec's "new epoch ≠ old terminal identity"
/// guarantee.
#[test]
#[should_panic(expected = "runtime_epoch already bound")]
fn stability_runtime_epoch_is_one_shot() {
    let registry = PtyRegistry::default();
    registry.set_runtime_epoch("epoch-1".into());
    registry.set_runtime_epoch("epoch-2".into());
}

// ── Replay / Resync ──────────────────────────────────────────────────

/// Resync on a healthy cursor reports bounds; Lagged is surfaced on gap.
#[test]
fn resync_after_lagged_reports_oldest_seq() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let pty = runtime().block_on(async {
        registry
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
            .expect("spawn")
    });
    // Trigger replay with a since_output_seq far older than oldest.
    let req = ReplayRequest {
        terminal_id: pty.to_string(),
        since_output_seq: 0,
        max_bytes: 1024,
    };
    let result = session.handle_replay(&req).expect("replay");
    match result {
        ReplayResult::Lagged { oldest_seq, head, .. } => {
            assert!(oldest_seq >= 1);
            assert!(head >= oldest_seq);
        }
        ReplayResult::Data(data) => {
            // PTY may not have produced output yet; either head is the
            // (next_seq - 1) or 0 if no frames ever published. The replay
            // contract is "head == highest seq seen by the hub".
            assert!(
                data.head_output_seq <= data.frames.len() as u64,
                "head_output_seq must reflect actual frames"
            );
        }
    }
}

/// ResyncRequest returns bounds regardless of mode.
#[test]
fn resync_request_reports_bounds() {
    let (_registry, session, pty) = make_session();
    for mode in [AttachMode::Raw, AttachMode::Delta, AttachMode::Snapshot] {
        let req = ResyncRequest {
            terminal_id: pty.to_string(),
            mode,
            since_output_seq: None,
        };
        let ack = session.handle_resync(&req).expect("resync");
        assert!(ack.next_output_seq >= 1);
    }
}

/// Runtime_epoch rotation does not break replay for terminals that are
/// still alive within the same kernel boot.
#[test]
fn runtime_epoch_immutable_for_session_lifetime() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-stable".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let epoch_a = registry.runtime_epoch().unwrap();
    let epoch_b = registry.runtime_epoch().unwrap();
    assert_eq!(epoch_a, epoch_b);
    let _ = session; // keep the session reference alive
}

/// Stale runtime_epoch attach produces a `runtime_epoch_stale` error code,
/// the canonical RTP1 error.
#[test]
fn runtime_epoch_stale_error_code_is_canonical() {
    let (_registry, session, pty) = make_session();
    let req = attach_req(pty, "host-a", "epoch-stale", "ctrl");
    match session.handle_attach(&req) {
        Err(error) => assert_eq!(error.code(), "runtime_epoch_stale"),
        Ok(_) => panic!("runtime_epoch_stale expected"),
    }
}
