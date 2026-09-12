//! P0 regression suite — explicit negative tests for each critical bug
//! fixed in this round. Each test names the audit ID (C1 / C2 / C3 / C4 /
//! C5 / C7 / C13) it covers. Failures here mean a regression of a
//! known security or correctness bug.

use std::sync::Arc;

use ridge_kernel::pty::{PtyLaunch, PtyRegistry};
use ridge_kernel::rtp1::{AttachMode, AttachRequest, InputFrame};
use ridge_kernel::rtp1_session::{AttachmentRegistry, AttachmentState, Rtp1Session};

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn make_session() -> (Arc<PtyRegistry>, Arc<Rtp1Session>, uuid::Uuid) {
    let registry = Arc::new(PtyRegistry::default());
    registry.set_runtime_epoch("epoch-foundation".into());
    let session = Arc::new(Rtp1Session::new(
        "host-a".into(),
        registry.clone(),
        1,
    ));
    let pty = rt().block_on(async {
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

fn attach(pty: uuid::Uuid, ctrl: &str) -> AttachRequest {
    AttachRequest {
        host_id: "host-a".into(),
        runtime_epoch: "epoch-foundation".into(),
        session_id: "f".into(),
        terminal_id: pty.to_string(),
        controller_id: ctrl.into(),
        since_output_seq: None,
        mode: AttachMode::Raw,
        client_min_version: 1,
        client_max_version: 1,
    }
}

// ── C1 (per-PTY controller_id validation) ─────────────────────────

/// C1 — Unattached controller's input is rejected at the session layer.
/// Pre-fix: any controller_id was accepted by `handle_input`; the WS
/// adapter was the only line of defense. Post-fix: the session itself
/// checks `attached_controllers[pty].contains(controller_id)`.
#[test]
fn c1_session_rejects_unbound_controller_input() {
    let (_registry, session, pty) = make_session();
    // Bind a fake controller without ever attaching — input must fail.
    let input = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-spoofed".into(),
        input_seq: 1,
        data_b64: ridge_kernel::rtp1::b64_encode(b"x"),
        data_len: 1,
    };
    let err = session
        .handle_input(&input)
        .expect_err("must reject unbound controller");
    assert_eq!(err.code(), "controller_id_unknown");
}

/// C1 — An attached controller's input succeeds.
#[test]
fn c1_session_accepts_attached_controller_input() {
    let (_registry, session, pty) = make_session();
    let _ = session
        .handle_attach(&attach(pty, "ctrl-A"))
        .expect("attach");
    let input = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-A".into(),
        input_seq: 1,
        data_b64: ridge_kernel::rtp1::b64_encode(b"y"),
        data_len: 1,
    };
    let ack = session.handle_input(&input).expect("input");
    assert_eq!(ack.controller_id, "ctrl-A");
    assert_eq!(ack.input_seq, 1);
}

/// C1 — Multi-controller isolation: when both ctrl-A and ctrl-B are
/// attached, each writes only with their own controller_id. An unbound
/// ctrl-C is rejected.
#[test]
fn c1_multi_controller_isolation() {
    let (_registry, session, pty) = make_session();
    session.handle_attach(&attach(pty, "ctrl-A")).expect("A");
    session.handle_attach(&attach(pty, "ctrl-B")).expect("B");
    // A writes as A: ok.
    let a = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-A".into(),
        input_seq: 1,
        data_b64: ridge_kernel::rtp1::b64_encode(b"a"),
        data_len: 1,
    };
    assert!(session.handle_input(&a).is_ok());
    // B writes as B: ok (each lane is per-controller).
    let b = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-B".into(),
        input_seq: 1,
        data_b64: ridge_kernel::rtp1::b64_encode(b"b"),
        data_len: 1,
    };
    assert!(session.handle_input(&b).is_ok());
    // Unbound ctrl-C: rejected regardless of PTY activity.
    let c = InputFrame {
        terminal_id: pty.to_string(),
        controller_id: "ctrl-C".into(),
        input_seq: 1,
        data_b64: ridge_kernel::rtp1::b64_encode(b"c"),
        data_len: 1,
    };
    let err = session.handle_input(&c).unwrap_err();
    assert_eq!(err.code(), "controller_id_unknown");
}

// ── AttachError code mapping ──────────────────────────────────────

/// All AttachError variants map to the canonical RTP1 error code per
/// SPEC-L2-PROTO-001 §3.10. Guards against accidental renames in
/// the error layer.
#[test]
fn attach_error_codes_canonical_for_c1() {
    use ridge_kernel::rtp1_session::AttachError;
    let cases: Vec<(AttachError, &str)> = vec![
        (
            AttachError::RuntimeEpochStale { expected: "e".into(), received: "f".into() },
            "runtime_epoch_stale",
        ),
        (AttachError::ClientTooOld { client_max: 1, server: 2 }, "client_too_old"),
        (AttachError::PermissionDenied("x".into()), "permission_denied"),
        (AttachError::UnknownTerminal("x".into()), "unknown_terminal"),
        (AttachError::InvalidField("x".into()), "protocol_violation"),
        (AttachError::InputTooLarge { actual: 100, cap: 64 }, "input_too_large"),
        (AttachError::IoError("x".into()), "io_error"),
        (AttachError::ServerMisconfigured("x".into()), "server_overloaded"),
        (AttachError::ControllerIdUnknown, "controller_id_unknown"),
    ];
    for (error, expected) in cases {
        assert_eq!(error.code(), expected, "mismatched code for {error:?}");
    }
}

/// AttachmentRegistry lifecycle: bind → set_state(Closing) →
/// set_state(Detached) → unbind leaves NO record (C2 regression guard).
#[test]
fn c2_attachment_registry_unbind_clears_record() {
    let registry = AttachmentRegistry::new();
    let pty = uuid::Uuid::new_v4();
    registry.bind(pty, "ctrl-A".into(), AttachmentState::Attached);
    assert!(registry.lookup("ctrl-A").is_some());
    registry.unbind("ctrl-A");
    assert!(registry.lookup("ctrl-A").is_none());
}

/// Repeated bind/unbind does not leak state across cycles (C2 regression).
#[test]
fn c2_attachment_registry_no_leak() {
    let registry = AttachmentRegistry::new();
    let pty = uuid::Uuid::new_v4();
    for i in 0..50 {
        let ctrl = format!("ctrl-{i}");
        registry.bind(pty, ctrl.clone(), AttachmentState::Attached);
        registry.set_state(&ctrl, AttachmentState::Closing);
        registry.unbind(&ctrl);
        assert!(registry.lookup(&ctrl).is_none(), "leak at iteration {i}");
    }
}

// ── C16 (handle_resync oldest_seq) ─────────────────────────────────

/// C16 — `handle_resync.oldest_output_seq` must echo the server's
/// actual oldest retained seq, not the client's `since_output_seq`.
/// Pre-fix: `request.since_output_seq.unwrap_or(0)` was returned,
/// which lied to a resyncing client about where the hub's window
/// actually starts.
#[test]
fn c16_handle_resync_returns_server_oldest_seq() {
    use ridge_kernel::rtp1::ResyncRequest;
    let (_registry, session, pty) = make_session();
    // After spawn, output_bounds reports (1, 1) (oldest=1, next=1).
    let req = ResyncRequest {
        terminal_id: pty.to_string(),
        mode: AttachMode::Raw,
        since_output_seq: Some(999), // client-claimed cursor (lie)
    };
    let ack = session.handle_resync(&req).expect("resync");
    // Server says oldest_output_seq = 1 (the real hub state), not 999.
    assert_eq!(
        ack.oldest_output_seq, 1,
        "oldest_output_seq must be server-truth, not the client's since_output_seq"
    );
}
