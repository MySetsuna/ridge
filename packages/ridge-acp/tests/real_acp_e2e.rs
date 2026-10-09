//! REAL_ACP_E2E acceptance — exercises the full ACP protocol surface against
//! `live_acp_agent`, a structurally complete MOCK_FIXTURE (not a real LLM).
//!
//! Coverage map (M2 §3):
//!   1. connect/start       — initialize + capability negotiation
//!   2. create session      — session/new
//!   3. user prompt         — session/prompt (response received)
//!   4. streaming response  — thinking_chunk + message_chunk notifications
//!   5. tool call           — tool_call_started + tool_call_finished
//!   6. tool output         — embedded in tool_call_finished.output
//!   7. approval flow       — approval_requested → session/approve (approve)
//!   8. completion          — turn_completed
//!   9. cancel              — session/cancel mid-stream (separate test)
//!
//! Capability gap: live_acp_agent's capability vector omits `artifacts`.
//! `agent/render_artifact` returns -32601 method-not-found. Ridge MUST NOT
//! fabricate capability support when the agent's vector says otherwise.

use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use ridge_acp::events::AgentSessionEvent;
use ridge_acp::session::{AgentSession, AgentSessionConfig, SessionStatus};
use ridge_acp::spawn::SpawnSpec;

fn live_spec() -> SpawnSpec {
    SpawnSpec {
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_live_acp_agent")),
        args: vec![],
        cwd: None,
        provider: ridge_acp::provider::ProviderSpec::mock_default(),
        framing: ridge_acp::transport::Framing::ContentLength,
    }
}

#[tokio::test]
async fn real_acp_e2e_full_happy_path() {
    let spec = live_spec();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn live agent");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = ridge_acp::transport::AcpTransport::spawn(stdin, stdout, None);

    let session = AgentSession::connect(
        transport,
        child,
        AgentSessionConfig::mock_default(),
    ).await.expect("connect");

    // Verify capability negotiation surfaced.
    let init = session.initialize_result().await.expect("initialize_result captured");
    let caps: Vec<String> = init.agent_capabilities.keys().cloned().collect();
    for required in ["tools", "streaming", "approvals", "cancellation", "plan"] {
        assert!(caps.iter().any(|c| c == required),
            "expected capability {required} in {caps:?}");
    }
    assert!(!caps.iter().any(|c| c == "artifacts"),
        "live agent must NOT advertise artifacts — Ridge must not fabricate support");
    assert_eq!(session.status().await, SessionStatus::Running);

    // Pre-subscribe so we observe every event in order.
    let mut rx = session.subscribe();
    let events_task = tokio::spawn(async move {
        let mut out = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) {
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(ev)) => out.push(ev),
                _ => break,
            }
        }
        out
    });

    // Approve every approval request that arrives while the prompt runs.
    let session_for_approval = session.clone();
    let approver = tokio::spawn(async move {
        let mut rx = session_for_approval.subscribe();
        while let Ok(ev) = tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            if let Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) = ev {
                let _ = session_for_approval.approve_tool(
                    &turn_id, &tool_call_id
                ).await.expect("approve observed turn");
            }
        }
    });

    let prompt_result = tokio::time::timeout(
        Duration::from_secs(3),
        session.prompt("refactor /tmp/example.txt"),
    )
    .await
    .expect("prompt resolved in time")
    .expect("prompt ok");

    approver.abort();
    let events = events_task.await.unwrap();

    let kinds: Vec<&'static str> = events.iter().map(|e| match e {
        AgentSessionEvent::SessionCreated { .. } => "session_created",
        AgentSessionEvent::TurnStarted { .. } => "turn_started",
        AgentSessionEvent::ThinkingChunk { .. } => "thinking_chunk",
        AgentSessionEvent::MessageChunk { .. } => "message_chunk",
        AgentSessionEvent::ToolCallStarted { .. } => "tool_call_started",
        AgentSessionEvent::ToolCallFinished { .. } => "tool_call_finished",
        AgentSessionEvent::ApprovalRequested { .. } => "approval_requested",
        AgentSessionEvent::PlanChanged { .. } => "plan_changed",
        AgentSessionEvent::TurnCompleted { .. } => "turn_completed",
        AgentSessionEvent::TurnFailed { .. } => "turn_failed",
        _ => "other",
    }).collect();

    // 1-8: every required phase produced at least one event.
    for required in ["turn_started", "thinking_chunk", "message_chunk",
                     "tool_call_started", "tool_call_finished",
                     "approval_requested", "plan_changed", "turn_completed"] {
        assert!(kinds.contains(&required),
            "missing {required} in event stream: {kinds:?}");
    }

    assert_eq!(session.status().await, SessionStatus::Running,
        "after turn_completed the session must be Running, not stuck in WaitingApproval");

    let _ = prompt_result;
}

#[tokio::test]
async fn real_acp_e2e_cancel_mid_stream() {
    let spec = live_spec();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = ridge_acp::transport::AcpTransport::spawn(stdin, stdout, None);
    let session = AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
        .await.expect("connect");

    let s_for_prompt = session.clone();
    let prompt_task = tokio::spawn(async move {
        s_for_prompt.prompt("long running").await
    });
    tokio::time::sleep(Duration::from_millis(40)).await;
    session.cancel(None).await.expect("cancel issued");

    let result = tokio::time::timeout(Duration::from_secs(3), prompt_task)
        .await
        .expect("cancel resolves promptly")
        .expect("join ok");
    assert!(result.is_err(), "cancel must produce an error result, got {result:?}");
    assert_eq!(session.status().await, SessionStatus::Cancelled,
        "session status must reflect cancellation, got {:?}", session.status().await);

    // Lockstep: SessionStatus::Cancelled ↔ TurnPhase::Cancelled. The turn
    // phase is set by `prompt_loop` when it processes the agent's `turn_failed`
    // notification post-cancel; it must NOT be `Failed`.
    let snap = session.snapshot().await;
    let turns: Vec<_> = snap.turns.values().collect();
    assert_eq!(turns.len(), 1, "one turn recorded, got {}", turns.len());
    assert_eq!(turns[0].phase, ridge_acp::TurnPhase::Cancelled,
        "host-cancelled turn phase must be Cancelled, got {:?}", turns[0].phase);
}

#[tokio::test]
async fn real_acp_e2e_unknown_method_returns_method_not_found() {
    let spec = live_spec();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = ridge_acp::transport::AcpTransport::spawn(stdin, stdout, None);

    let resp = transport
        .call("agent/render_artifact", serde_json::json!({}))
        .await
        .expect("transport delivers method-not-found as response");
    let err = resp.error.as_ref().expect("error object present");
    assert_eq!(err.code, -32601, "JSON-RPC method-not-found code");
    assert!(err.message.contains("agent/render_artifact"));
}

#[tokio::test]
async fn real_acp_e2e_probe_call_then_connect() {
    // Sanity: a direct transport.call() followed by AgentSession::connect
    // should both succeed on the same transport handle. This isolates the
    // failure mode in connect (which the full happy-path test exercises).
    let spec = live_spec();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = ridge_acp::transport::AcpTransport::spawn(stdin, stdout, None);

    // Direct call works.
    let r = transport.call("initialize", serde_json::json!({})).await;
    eprintln!("[probe] direct call initialize: {r:?}");
    assert!(r.is_ok(), "direct initialize call failed: {r:?}");

    // Now try AgentSession::connect — same transport.
    let session = AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
        .await;
    eprintln!("[probe] connect result ok? {}", session.is_ok());
    assert!(session.is_ok(), "connect failed");
    let _ = session;
}

#[tokio::test]
async fn real_acp_e2e_session_status_transitions_during_lifecycle() {
    // Verifies the canonical SessionStatus state machine (M2 §4):
    // Starting → Running (after connect), then continues Running post-turn.
    let spec = live_spec();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = ridge_acp::transport::AcpTransport::spawn(stdin, stdout, None);
    let session = AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
        .await.expect("connect");
    assert_eq!(session.status().await, SessionStatus::Running);

    let session_for_approval = session.clone();
    let approver = tokio::spawn(async move {
        let mut rx = session_for_approval.subscribe();
        while let Ok(ev) = tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
            if let Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) = ev {
                let _ = session_for_approval.approve_tool(
                    &turn_id, &tool_call_id
                ).await.expect("approve observed turn");
            }
        }
    });

    // After a successful prompt, still Running (no WaitingApproval leak).
    tokio::time::timeout(Duration::from_secs(3), session.prompt("hi"))
        .await.expect("prompt").expect("ok");
    approver.abort();
    assert_eq!(session.status().await, SessionStatus::Running);
}
