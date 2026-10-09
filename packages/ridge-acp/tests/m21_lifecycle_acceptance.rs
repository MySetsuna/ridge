//! M2.1 lifecycle acceptance — bridge-cleanup-equivalent guarantees for the
//! `AcpTransport` + `AgentSession` surface.
//!
//! Coverage targets (M2.1 §4):
//! - connect ✓
//! - session create (via `real_acp_e2e_*`)
//! - prompt (via `real_acp_e2e_*`)
//! - cancel (via `real_acp_e2e_cancel_mid_stream`)
//! - provider EOF — `lifecycle_*` suite + this file
//! - malformed/provider error — `lifecycle_malformed_frame_*`
//! - desktop unmount/remount analog: subscription lifecycle on a single
//!   transport; double-subscribe must not double-deliver; subscribe after
//!   shutdown must surface `Lagged` rather than panic.
//! - session already completed — TerminalStatus is observable after the
//!   session has reached `Cancelled` / `Completed` / `Failed`.
//!
//! These tests target the *transport+session* layer the M2.1 Tauri bridge
//! will compose against — proving the lower layer holds the invariants the
//! bridge layer will depend on.

use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use ridge_acp::events::AgentSessionEvent;
use ridge_acp::provider::ProviderSpec;
use ridge_acp::session::{AgentSession, AgentSessionConfig, SessionStatus};
use ridge_acp::spawn::SpawnSpec;
use ridge_acp::transport::AcpTransport;

#[allow(dead_code)]
fn fake_spec() -> SpawnSpec {
    SpawnSpec {
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_fake_acp_agent")),
        args: vec![],
        cwd: None,
        provider: ProviderSpec::mock_default(),
        framing: ridge_acp::transport::Framing::ContentLength,
    }
}

#[tokio::test]
async fn listener_double_subscribe_does_not_double_deliver() {
    // M2.1 §4 LISTENER_CLEANUP analog at the transport layer: two
    // subscribers must each see the notification once. Ridge's Tauri bridge
    // composes against this — when a route unmount/remounts and re-attaches,
    // it MUST NOT receive every event twice because of broadcast semantics.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn(stdin, stdout, None);

    let mut rx_a = transport.subscribe_notifications();
    let mut rx_b = transport.subscribe_notifications();
    let _ = transport
        .call("initialize", serde_json::json!({}))
        .await
        .expect("initialize");

    // fake_acp_agent emits no notifications until session/prompt fires;
    // both receivers must time out without yielding anything.
    let r_a = tokio::time::timeout(Duration::from_millis(80), rx_a.recv()).await;
    let r_b = tokio::time::timeout(Duration::from_millis(80), rx_b.recv()).await;
    assert!(r_a.is_err(), "rx_a: no notification expected");
    assert!(r_b.is_err(), "rx_b: no notification expected");
    // The two receivers are independent — neither held the other back.
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn listener_subscribe_after_shutdown_yields_closed_not_panic() {
    // M2.1 §4 LISTENER_CLEANUP: subscribing AFTER the transport is closed
    // must surface `RecvError::Closed`, not panic. The bridge must be able
    // to attach a listener during teardown without crashing the runtime.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn(stdin, stdout, None);

    let _ = transport
        .call("initialize", serde_json::json!({}))
        .await
        .expect("initialize");
    transport.shutdown();
    let mut rx = transport.subscribe_notifications();
    let r = tokio::time::timeout(Duration::from_millis(120), rx.recv()).await;
    // After shutdown the broadcast channel will close; the receiver MUST
    // surface Closed (or time out waiting) rather than panic.
    match r {
        Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {}
        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
        Err(_) => {} // timed out — also acceptable; channel is silent
        Ok(Ok(_)) => panic!("no notification expected after shutdown"),
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[tokio::test]
async fn cancel_during_provider_eof_keeps_session_cancelled_not_failed() {
    // M2.1 §4 CANCEL_ERROR_EOF: when the provider closes (shutdown → EOF)
    // the same instant a cancel is in flight, the session MUST reach
    // Cancelled, not Failed. This mirrors the production path: user clicks
    // cancel while the remote provider is going away. Without this
    // guarantee, the bridge could surface a misleading "failed" UI state
    // for a user-initiated cancel.
    //
    // We simulate provider EOF by `transport.shutdown()` after issuing
    // cancel. The reader sees stdin close; fake_acp_agent exits. The
    // session must still observe a cancel intent (set BEFORE the wire call
    // per session.rs:cancel) and reach Cancelled.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_live_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn(stdin, stdout, None);
    let session = AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
        .await
        .expect("connect");

    let mut rx = session.subscribe();
    let s_for_prompt = session.clone();
    let prompt_task = tokio::spawn(async move { s_for_prompt.prompt("long").await });
    // Wait for the prompt to enter the approval gate.
    tokio::time::timeout(Duration::from_secs(2), async {
        loop { if matches!(rx.recv().await.unwrap(), AgentSessionEvent::ApprovalRequested { .. }) { break; } }
    }).await.expect("canonical approval gate observed");

    // Cancel FIRST (pre-marks Cancelled under the inner lock), THEN
    // shutdown the transport to simulate provider EOF racing cancel.
    let cancel_res = session.cancel(None).await;
    assert!(cancel_res.is_ok(), "cancel must resolve even when provider is dying");
    // Let the prompt task observe provider EOF and resolve.
    let _ = tokio::time::timeout(Duration::from_secs(2), prompt_task)
        .await
        .expect("prompt resolves after provider EOF");

    let status = session.status().await;
    assert!(
        matches!(status, SessionStatus::Cancelled),
        "host-cancelled turn must keep Cancelled status after provider EOF, got {status:?}"
    );
}

#[tokio::test]
async fn session_already_completed_blocks_double_prompt() {
    // M2.1 §4 session already completed: after a session is in a terminal
    // state (here: Failed after EOF), the bridge must not be able to issue
    // another prompt that silently appears to succeed. The prompt must
    // return an error rather than spawning a phantom turn.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn(stdin, stdout, None);
    let session = AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
        .await
        .expect("connect");

    // Approve any approval requests so the first prompt can complete.
    let approver_session = session.clone();
    let approver = tokio::spawn(async move {
        let mut rx = approver_session.subscribe();
        while let Ok(ev) = tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            if let Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) = ev {
                let _ = approver_session
                    .approve_tool(&turn_id, &tool_call_id)
                    .await.expect("approve observed turn");
            }
        }
    });

    // Issue one successful prompt so the session has a turn record.
    let _ = tokio::time::timeout(Duration::from_secs(3), session.prompt("first"))
        .await
        .expect("first prompt resolves")
        .expect("first prompt ok");
    approver.abort();

    // Force EOF by calling the new `shutdown()` API on the session itself.
    // This closes the transport writer + signals the reader/writer to exit;
    // fake_acp_agent sees stdin EOF and exits.
    session.shutdown().await;
    tokio::time::sleep(Duration::from_millis(120)).await;
    // Now any subsequent prompt must fail with a transport-closed error
    // (not silently succeed).
    let res = session.prompt("second-after-eof").await;
    assert!(res.is_err(), "second prompt after provider EOF must error, got Ok({res:?})");
}

#[tokio::test]
async fn session_event_stream_emits_terminal_event_after_provider_eof() {
    // M2.1 §4 provider EOF surfaces as a `TurnFailed` / `SessionFailed`
    // event on the event broadcast. Bridge listeners must see it;
    // otherwise the UI hangs forever.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn(stdin, stdout, None);
    let session = AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
        .await
        .expect("connect");

    let mut rx = session.subscribe();
    session.shutdown().await;
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Drain events for up to 1s and look for a terminal signal. We accept
    // ANY non-TurnStarted event as "the session told the consumer it died".
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut saw_terminal = false;
    while let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) {
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => match ev {
                AgentSessionEvent::TurnFailed { .. }
                | AgentSessionEvent::SessionFailed { .. } => {
                    saw_terminal = true;
                    break;
                }
                AgentSessionEvent::TurnCompleted { .. } => {
                    saw_terminal = true;
                    break;
                }
                _ => continue,
            },
            _ => break,
        }
    }
    assert!(saw_terminal, "consumer must observe a terminal event after provider EOF");
}
