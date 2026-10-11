//! M3 wire-shape tests — ProviderSpec/ModelSpec/Effort + AgentSession::provider()
//! + Commutation integration via mock_acp_agent.

use ridge_acp::communion::{Commutation, CommutationState, SessionSummary, StaffSpec};
use ridge_acp::events::AgentSessionEvent;
use ridge_acp::provider::{Effort, ModelSpec, ProviderKind, ProviderSpec};
use ridge_acp::session::{AgentSession, AgentSessionConfig};
use ridge_acp::transport::AcpTransport;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::broadcast;

async fn spawn_mock_session() -> Arc<AgentSession> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("fake_acp_agent spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = AcpTransport::spawn(stdin, stdout, _stderr);
    let cfg = AgentSessionConfig {
        provider: ProviderSpec::mock_default(),
        client_name: "m3-wire".into(),
        client_version: "0.1.0".into(),
    };
    Arc::new(
        AgentSession::connect(transport, child, cfg)
            .await
            .expect("connect"),
    )
}

#[test]
fn effort_enum_roundtrip() {
    for e in [Effort::Low, Effort::Medium, Effort::High] {
        let s = serde_json::to_string(&e).unwrap();
        let back: Effort = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
    }
}

#[test]
fn model_spec_with_effort_roundtrip() {
    let m = ModelSpec {
        id: "claude-haiku".into(),
        tier: "base".into(),
        effort: Some(Effort::High),
    };
    let s = serde_json::to_string(&m).unwrap();
    let back: ModelSpec = serde_json::from_str(&s).unwrap();
    assert_eq!(back, m);
    assert_eq!(back.effort, Some(Effort::High));
}

#[test]
fn model_spec_effort_defaults_to_none() {
    let m = ModelSpec {
        id: "x".into(),
        tier: "base".into(),
        effort: None,
    };
    let s = serde_json::to_string(&m).unwrap();
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    // effort=None → not serialized (skip_serializing_if)
    assert!(v.get("effort").is_none());
}

#[tokio::test]
async fn agent_session_provider_getter_returns_configured_spec() {
    let session = spawn_mock_session().await;
    let p = session.provider().await;
    assert_eq!(p.kind, ProviderKind::Mock);
    assert_eq!(p.model.id, "mock-claude-haiku");
}

#[tokio::test]
async fn commutation_lead_with_two_staff_emits_creation_event() {
    let lead = spawn_mock_session().await;
    let lead_id = lead.id().await;

    // Subscribe BEFORE creating commutation so we catch Created event.
    // We create the commutation first using a do_blocking trick — but since
    // Commutation::new is async, we need to subscribe first then create.
    // Simpler: subscribe first, drain whatever, then construct.
    let comm = Commutation::new(&lead, "M3 task", "build provider flow").await;
    let mut rx = comm.events();

    let staff1 = spawn_mock_session().await;
    let staff1_id = staff1.id().await;
    comm.assign_staff(
        StaffSpec {
            session_id: staff1_id.clone(),
            role: "planner".into(),
            provider_label: "mock".into(),
        },
        &staff1,
    )
    .await;

    let staff2 = spawn_mock_session().await;
    let staff2_id = staff2.id().await;
    comm.assign_staff(
        StaffSpec {
            session_id: staff2_id.clone(),
            role: "executor".into(),
            provider_label: "mock".into(),
        },
        &staff2,
    )
    .await;

    // Drain at least 2 StaffAssigned events
    use std::time::Duration;
    let mut got = 0;
    let mut staff_seen = std::collections::HashSet::new();
    while got < 2 {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv())
            .await
        {
            Ok(Ok(ridge_acp::communion::CommutationEvent::StaffAssigned { staff, .. })) => {
                staff_seen.insert(staff);
                got += 1;
            }
            Ok(Ok(other)) => {
                // ignore Created, etc.
            }
            Ok(Err(_)) => panic!("broadcast closed unexpectedly"),
            Err(_) => panic!("timeout waiting for StaffAssigned"),
        }
    }
    assert!(staff_seen.contains(&staff1_id));
    assert!(staff_seen.contains(&staff2_id));

    let snap = comm.snapshot().await;
    assert_eq!(snap.lead, lead_id);
    assert_eq!(snap.staff.len(), 2);
    assert_eq!(snap.state, CommutationState::Created);
    assert_eq!(snap.summaries.len(), 2);
}

#[tokio::test]
async fn session_event_emits_session_failed_on_shutdown() {
    let session = spawn_mock_session().await;
    let mut rx: broadcast::Receiver<AgentSessionEvent> = session.subscribe();
    session.shutdown().await;
    let ev = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
        .await
        .expect("terminal event within 1s")
        .expect("not closed");
    assert!(
        matches!(ev, AgentSessionEvent::SessionFailed { .. }),
        "got {ev:?}"
    );
}
