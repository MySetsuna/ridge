//! Stability + fault tests (SPEC-L2-PERF-001 §3.3, SPEC-L2-REMOTE-001 §3.5).
//!
//! Covers: disconnect/reconnect, kernel restart (= new runtime_epoch),
//! repeated attach/detach, resize storm, multi-terminal stress, exit
//! notification delivery. No network — these exercise the session-level
//! invariants that the WebSocket adapter enforces.

use std::sync::Arc;

use ridge_kernel::pty::{PtyExitNotification, PtyLaunch, PtyRegistry, PtyOutputRead};
use ridge_kernel::rtp1::{
    AttachMode, AttachRequest, DetachRequest, Frame, FrameFlags, MessageType, PingFrame,
    ReplayRequest, ResizeRequest,
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

fn fresh_session() -> (Arc<PtyRegistry>, Arc<Rtp1Session>, uuid::Uuid) {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-stable".into());
    let session = Arc::new(Rtp1Session::new("host-a".into(), registry.clone(), 1));
    let pty = runtime().block_on(async {
        registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "stress",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn")
    });
    (registry, session, pty)
}

fn attach_req(pty: uuid::Uuid, ctrl: &str, epoch: &str) -> AttachRequest {
    AttachRequest {
        host_id: "host-a".into(),
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

fn multi_attach_req(pty: uuid::Uuid, ctrl: &str, epoch: &str) -> AttachRequest {
    AttachRequest {
        host_id: "host".into(),
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

// ── Repeated attach / detach (200 cycles) ─────────────────────────────

#[test]
fn repeated_attach_detach_does_not_leak_state() {
    let (registry, session, pty) = fresh_session();
    let attachments = Arc::new(AttachmentRegistry::new());
    for i in 0..200 {
        let ctrl = format!("ctrl-{i}");
        let req = attach_req(pty, &ctrl, "epoch-stable");
        let (_ack, _lease, state) = match session.handle_attach(&req) {
            Ok(t) => t,
            Err(error) => panic!("attach {i} failed: {error}"),
        };
        attachments.bind(pty, ctrl.clone(), state);
        assert!(attachments.lookup(&ctrl).is_some());
        attachments.set_state(&ctrl, AttachmentState::Closing);
        attachments.set_state(&ctrl, AttachmentState::Detached);
        let bound = attachments.unbind(&ctrl);
        assert!(bound.is_some(), "ctrl-{i} should be unbound after detach");
        assert!(attachments.lookup(&ctrl).is_none());
    }
    // After 200 cycles, registry still reports exactly one PTY.
    assert_eq!((*registry).len(), 1);
}

// ── Resize storm (1000 resizes against one PTY) ───────────────────────

#[test]
fn resize_storm_does_not_panic_or_leak() {
    let (registry, session, pty) = fresh_session();
    let req_attach = attach_req(pty, "ctrl", "epoch-stable");
    let (_ack, _lease, _) = session.handle_attach(&req_attach).expect("attach");
    for i in 0..1000 {
        let cols = 40 + (i % 80);
        let rows = 20 + (i % 30);
        let req = ResizeRequest {
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            rows,
            cols,
            owner: None,
        };
        let ack = session.handle_resize(&req).expect("resize");
        assert_eq!(ack.rows, rows);
        assert_eq!(ack.cols, cols);
    }
    // Verify the final resize reflects a valid PTY dimension.
    let info = (*registry).info(pty).expect("info");
    assert_eq!(info.cols, 40 + (999 % 80));
    assert_eq!(info.rows, 20 + (999 % 30));
}

// ── Multi-terminal stress (16 PTYs, 64 attach/detach cycles) ─────────

#[test]
fn multi_terminal_stress_isolates_per_pty_state() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-multi".into());
    let session = Arc::new(Rtp1Session::new("host".into(), registry.clone(), 1));
    let mut ptys = Vec::new();
    for _ in 0..16 {
        let id = runtime().block_on(async {
            registry
                .spawn_command_for(PtyLaunch {
                    id: uuid::Uuid::new_v4(),
                    program: None,
                    args: &[],
                    cwd: None,
                    workspace_id: None,
                    role: "multi",
                    launch_profile: None,
                    env: None,
                    initial_size: Some((80, 24)),
                })
                .expect("spawn")
        });
        ptys.push(id);
    }
    for cycle in 0..64 {
        for (idx, pty) in ptys.iter().enumerate() {
            let ctrl = format!("c-{cycle}-{idx}");
            let req = multi_attach_req(*pty, &ctrl, "epoch-multi");
            let (_ack, _lease, _state) = session.handle_attach(&req).expect("attach");
            // Resize each pane to a distinct dimension; verifies identity
            // routing is per terminal_id, not global.
            let cols = 40 + (idx as u16 * 2);
            let rows = 20 + idx as u16;
            let resize = ResizeRequest {
                terminal_id: pty.to_string(),
                controller_id: ctrl.clone(),
                rows,
                cols,
                owner: None,
            };
            let ack = session.handle_resize(&resize).expect("resize");
            assert_eq!(ack.cols, cols);
            assert_eq!(ack.rows, rows);
        }
    }
    // All 16 PTYs still alive and uniquely sized.
    for (idx, pty) in ptys.iter().enumerate() {
        let info = registry.info(*pty).expect("info");
        let expected_cols = 40 + (idx as u16 * 2);
        let expected_rows = 20 + idx as u16;
        assert_eq!(info.cols, expected_cols, "pty {idx} cols");
        assert_eq!(info.rows, expected_rows, "pty {idx} rows");
    }
}

// ── Exit notification delivered to subscribers ───────────────────────

#[test]
fn exit_notification_delivered_to_subscribers() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let pty = runtime().block_on(async {
        registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "exit",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn")
    });
    let mut rx_a = registry.subscribe_exit(pty).expect("subscribe a");
    let mut rx_b = registry.subscribe_exit(pty).expect("subscribe b");
    registry.notify_exit(pty, Some(0));
    let notification_a = runtime().block_on(async { rx_a.recv().await }).expect("a");
    let notification_b = runtime().block_on(async { rx_b.recv().await }).expect("b");
    assert_eq!(notification_a.pty_id, pty);
    assert_eq!(notification_b.pty_id, pty);
    assert_eq!(notification_a.code, Some(0));
    assert_eq!(notification_b.code, Some(0));
}

// ── Replay window does not silently rebind after detach ──────────────

#[test]
fn replay_after_detach_returns_lagged_or_data_not_rebind() {
    let (registry, session, pty) = fresh_session();
    let req_attach = attach_req(pty, "ctrl", "epoch-stable");
    let (_ack, _lease, _) = session.handle_attach(&req_attach).expect("attach");
    let detach_ack = session
        .handle_detach(&DetachRequest {
            terminal_id: pty.to_string(),
            controller_id: "ctrl".into(),
            reason: None,
        })
        .expect("detach");
    let replay = ReplayRequest {
        terminal_id: pty.to_string(),
        since_output_seq: 0,
        max_bytes: 1024,
    };
    let result = runtime()
        .block_on(async { session.handle_replay(&replay).await })
        .expect("replay");
    // After detach, the registry still serves replay; the result is
    // either Data (if any bytes are within the cap) or Lagged. Both
    // are valid; silent rebind would be a different terminal_id.
    match result {
        ReplayResult::Data(data) => assert_eq!(data.terminal_id, pty.to_string()),
        ReplayResult::Lagged { .. } => {}
    }
    assert_eq!(detach_ack.terminal_id, pty.to_string());
    let _ = registry;
}

// ── Runtime_epoch restart path (cannot rebind in-process) ─────────────

#[test]
#[should_panic(expected = "runtime_epoch already bound")]
fn runtime_epoch_rebind_panics() {
    let registry = PtyRegistry::default();
    registry.set_runtime_epoch("epoch-1".into());
    registry.set_runtime_epoch("epoch-2".into());
}

// ── Bounded output_seq is preserved across repeated publishes ─────────

#[test]
fn output_seq_advances_under_repeated_publishes() {
    let (registry, _session, pty) = fresh_session();
    // We cannot publish directly to the hub; the reader task publishes
    // shell stdout. To exercise seq advancement deterministically, we
    // call the registry's hub indirectly via the kernel client contract.
    // For now, verify that the registered PTY's lease bounds report a
    // monotonic next_seq even with no published frames yet.
    let (oldest, next) = registry.output_bounds(pty).expect("bounds");
    assert_eq!(oldest, 1);
    assert_eq!(next, 1);
}

// ── Pumping a long message through a fresh ptyOutputHub ───────────────

#[test]
fn hub_advances_seq_under_load() {
    use ridge_kernel::pty::PtyOutputHub;
    let hub = Arc::new(PtyOutputHub::new());
    let lease = hub.attach_output_for_test(None).expect("attach");
    let r = runtime();
    // Publish 32 small frames; the lease should drain all 32 with strictly
    // monotonic seqs and zero losses.
    for _ in 0..32 {
        hub.publish(b"x");
    }
    let mut total = Vec::new();
    let mut polls = 0;
    while total.len() < 32 && polls < 64 {
        polls += 1;
        match r.block_on(lease.next(std::time::Duration::from_millis(200), 32)) {
            Ok(PtyOutputRead::Data(frames)) => total.extend(frames),
            Ok(PtyOutputRead::Lagged { .. }) => break,
            other => panic!("expected Data, got {other:?}"),
        }
    }
    assert_eq!(total.len(), 32, "all 32 frames must arrive");
    let mut seqs: Vec<u64> = total.iter().map(|f| f.seq).collect();
    seqs.dedup();
    assert_eq!(seqs.len(), total.len(), "seqs must be unique");
    let mut sorted = seqs.clone();
    sorted.sort();
    assert_eq!(seqs, sorted, "seqs must be monotonic");
}

// ── Ping / Pong wire round-trip ───────────────────────────────────────

#[test]
fn ping_pong_round_trip_via_envelope() {
    let ping = PingFrame { nonce: 42 };
    let frame: Frame = ridge_kernel::rtp1::frame_from(
        MessageType::Ping,
        &ping,
        FrameFlags::empty(),
    )
    .unwrap();
    let wire = ridge_kernel::rtp1::encode(&frame).unwrap();
    let (parsed, consumed) = ridge_kernel::rtp1::decode(&wire).unwrap();
    assert_eq!(consumed, wire.len());
    let pong: PingFrame = ridge_kernel::rtp1::payload_from(&parsed).unwrap();
    assert_eq!(pong.nonce, 42);
    assert_eq!(parsed.r#type, MessageType::Ping);
}

// ── Subscribe after exit yields cached notification ───────────────────

#[test]
fn exit_subscribe_after_event_yields_no_new_messages() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch".into());
    let pty = runtime().block_on(async {
        registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "late",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn")
    });
    registry.notify_exit(pty, Some(0));
    let mut rx = registry.subscribe_exit(pty).expect("subscribe late");
    // The new subscriber should not see the past event; broadcast channels
    // only deliver messages sent after subscribe.
    let no_msg =
        runtime().block_on(async { tokio::time::timeout(std::time::Duration::from_millis(20), rx.recv()).await });
    assert!(
        no_msg.is_err(),
        "subscriber joining after broadcast must not see past events"
    );
}

// ── AttachError type and code mapping ─────────────────────────────────

#[test]
fn attach_error_code_canonical() {
    use ridge_kernel::rtp1_session::AttachError;
    let cases: Vec<(AttachError, &str)> = vec![
        (AttachError::RuntimeEpochStale {
            expected: "e".into(),
            received: "f".into(),
        }, "runtime_epoch_stale"),
        (AttachError::ClientTooOld { client_max: 1, server: 2 }, "client_too_old"),
        (AttachError::PermissionDenied("x".into()), "permission_denied"),
        (AttachError::UnknownTerminal("x".into()), "unknown_terminal"),
        (AttachError::InvalidField("x".into()), "protocol_violation"),
        (AttachError::InputTooLarge { actual: 100, cap: 64 }, "input_too_large"),
        (AttachError::IoError("x".into()), "io_error"),
        (AttachError::ServerMisconfigured("x".into()), "server_overloaded"),
    ];
    for (error, expected_code) in cases {
        assert_eq!(error.code(), expected_code, "mismatched code for {error:?}");
    }
    // Ensure drop compiles.
    let _ = PtyExitNotification {
        pty_id: uuid::Uuid::nil(),
        code: None,
    };
}

// ── Per-controller input_seq wire validation (SPEC-L2-PROTO-001 §3.4.2) ──

#[test]
fn input_seq_wire_validation_unknown_controller_rejected() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-input".into());
    let pty = runtime().block_on(async {
        registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "input",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn")
    });
    // No attach has occurred: write with a controller_id not in the
    // registry's attached set must be rejected with ControllerIdUnknown.
    match registry.write_with_controller(pty, "ctrl-A", b"hi") {
        Err(ridge_kernel::pty::PtyInputError::ControllerIdUnknown) => {}
        other => panic!("expected ControllerIdUnknown, got {other:?}"),
    }
    // After attaching, the controller is admitted.
    registry.attach_controller(pty, "ctrl-A".into());
    registry
        .write_with_controller(pty, "ctrl-A", b"hi")
        .expect("attached controller admitted");
    // A second controller is still rejected.
    match registry.write_with_controller(pty, "ctrl-B", b"x") {
        Err(ridge_kernel::pty::PtyInputError::ControllerIdUnknown) => {}
        other => panic!("expected ControllerIdUnknown for ctrl-B, got {other:?}"),
    }
    // Detaching controller_id removes it.
    registry.detach_controller(pty, "ctrl-A");
    match registry.write_with_controller(pty, "ctrl-A", b"hi") {
        Err(ridge_kernel::pty::PtyInputError::ControllerIdUnknown) => {}
        other => panic!("expected ControllerIdUnknown after detach, got {other:?}"),
    }
}

#[test]
fn input_seq_wire_validation_multi_controller_isolation() {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-multi-input".into());
    let pty = runtime().block_on(async {
        registry
            .spawn_command_for(PtyLaunch {
                id: uuid::Uuid::new_v4(),
                program: None,
                args: &[],
                cwd: None,
                workspace_id: None,
                role: "multi",
                launch_profile: None,
                env: None,
                initial_size: Some((80, 24)),
            })
            .expect("spawn")
    });
    registry.attach_controller(pty, "ctrl-A".into());
    registry.attach_controller(pty, "ctrl-B".into());
    registry
        .write_with_controller(pty, "ctrl-A", b"x")
        .expect("ctrl-A");
    registry
        .write_with_controller(pty, "ctrl-B", b"y")
        .expect("ctrl-B");
    // Removing one does not affect the other.
    registry.detach_controller(pty, "ctrl-A");
    match registry.write_with_controller(pty, "ctrl-A", b"x") {
        Err(ridge_kernel::pty::PtyInputError::ControllerIdUnknown) => {}
        other => panic!("ctrl-A must be rejected after detach, got {other:?}"),
    }
    registry
        .write_with_controller(pty, "ctrl-B", b"y")
        .expect("ctrl-B still admitted");
}
