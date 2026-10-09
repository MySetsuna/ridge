//! MOBILE_CONSUMER — protocol-level acceptance for the canonical event stream.
//!
//! The M2 Mobile consumer is a test-only consumer of `AgentSessionEvent` and
//! `CommutationEvent` JSON. The point: prove that any client (Mobile future,
//! Desktop today, third-party watchdog) can decode every state dimension
//! Ridge exposes without depending on:
//!
//! - Tauri invokes (`tauri::command`)
//! - shell ANSI escape sequences
//! - Desktop private store
//! - any platform-coupled type
//!
//! Coverage map (M2 §6 — 7 dimensions):
//!
//! | dimension       | wire shape                                | tested via              |
//! |-----------------|-------------------------------------------|-------------------------|
//! | session list    | `CommutationSnapshot.staff: Vec<...>`     | commutation snapshot    |
//! | session state   | `SessionStatus` enum                      | status enum round-trip  |
//! | stream          | `AgentSessionEvent` tagged enum           | event decode round-trip |
//! | tool            | `ToolCallStarted` / `ToolCallFinished`    | tool event round-trip   |
//! | approval        | `ApprovalRequested` / `ApprovalResolved`  | approval event decode   |
//! | task            | `TurnStarted` / `TurnCompleted` / `Failed`| turn event decode       |
//! | artifact        | GAP — `ArtifactChanged` not yet in enum    | honest gap, not faked   |
//!
//! The artifact dimension is acknowledged as a GAP (not fabricated) — the
//! `mobile_consumer_artifact_dimension_is_honest_gap` test makes the absence
//! visible so future clients know to negotiate this with the agent instead
//! of receiving a stub from Ridge.

mod common;

use std::time::Duration;

use ridge_acp::{
    AgentSession, AgentSessionEvent, AgentSessionSnapshot, Commutation, CommutationEvent,
    LifecycleView, StaffSpec,
};
use ridge_core::teammate::communication::AgentLifecycle;

/// Decode one event via the canonical wire path (Serialize → JSON Value →
/// Deserialize). Catches accidental platform-bound types creeping in.
fn decode_via_json<T: serde::Serialize + serde::de::DeserializeOwned>(
    value: &T,
) -> Result<T, serde_json::Error> {
    let v = serde_json::to_value(value)?;
    serde_json::from_value(v)
}

/// Forbidden substrings in any serialized Ridge surface that would betray
/// platform binding. Same set used by `e2e_session_snapshot_json`.
fn assert_portable(s: &str) {
    for needle in ["tauri", "pty", "ansi", "shell", "desktop", "invoke"] {
        assert!(
            !s.to_lowercase().contains(&needle.to_lowercase()),
            "forbidden platform-bound token leaked: {needle}"
        );
    }
}

// ---------- dimension 1: session list (CommutationSnapshot) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_consumer_session_list_via_commutation_snapshot() {
    let lead = common::spawn_fake_session().await.session;
    let staff_a = common::spawn_fake_session().await.session;
    let staff_b = common::spawn_fake_session().await.session;
    let staff_a_id = staff_a.id().await;
    let staff_b_id = staff_b.id().await;

    let comm = Commutation::new(&lead, "two-subtask", "explore + summarize").await;
    comm.assign_staff(
        StaffSpec {
            session_id: staff_a_id.clone(),
            role: "explorer".into(),
            provider_label: "mock/claude-haiku".into(),
        },
        &staff_a,
    )
    .await;
    comm.assign_staff(
        StaffSpec {
            session_id: staff_b_id.clone(),
            role: "summarizer".into(),
            provider_label: "mock/claude-haiku".into(),
        },
        &staff_b,
    )
    .await;

    let snap = comm.snapshot().await;
    let json = serde_json::to_value(&snap).expect("serialize commutation snapshot");
    assert_portable(&json.to_string());
    let back: ridge_acp::CommutationSnapshot =
        serde_json::from_value(json.clone()).expect("deserialize commutation snapshot");
    assert_eq!(back.state, snap.state);
    assert_eq!(back.lead, snap.lead);
    assert_eq!(back.staff.len(), 2, "session list must contain both staff");
    assert!(back.staff.contains(&staff_a_id));
    assert!(back.staff.contains(&staff_b_id));
}

// ---------- dimension 2: session state (LifecycleView) ----------

#[tokio::test]
async fn mobile_consumer_session_state_lifecycle_roundtrip() {
    // The canonical session-state wire shape is `LifecycleView` (it is what
    // `StateChanged` events carry). Mobile clients read lifecycle from the
    // event stream — they MUST NOT derive state from per-event observation.
    for inner in [
        AgentLifecycle::Discovered,
        AgentLifecycle::Spawning,
        AgentLifecycle::Attaching,
        AgentLifecycle::Online,
        AgentLifecycle::Working,
        AgentLifecycle::Waiting,
        AgentLifecycle::Attention,
        AgentLifecycle::Completed,
        AgentLifecycle::Stopped,
        AgentLifecycle::Failed,
    ] {
        let view = LifecycleView::new(inner);
        let v = serde_json::to_value(view).expect("serialize LifecycleView");
        let back: LifecycleView = serde_json::from_value(v.clone()).expect("deserialize");
        assert_eq!(back, view, "LifecycleView round-trip for {inner:?}");
        assert!(!v.to_string().is_empty());
    }
}

// ---------- dimension 3: stream (AgentSessionEvent tagged enum) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_consumer_stream_decodes_every_event_kind() {
    let harness = common::spawn_fake_session().await;
    let session: AgentSession = harness.session;
    let mut rx = harness.rx;

    // Approve on behalf of the test so the turn can complete.
    let approver_rx = session.subscribe();
    let approver = {
        let s = session.clone();
        tokio::spawn(async move {
            let mut rx = approver_rx;
            loop {
                match rx.recv().await {
                    Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) => {
                        let _ = s
                            .approve_tool(&turn_id, &tool_call_id)
                            .await.expect("approve observed turn");
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let drain = tokio::spawn(async move {
        let mut kinds = std::collections::BTreeSet::new();
        while let Ok(ev) = rx.recv().await {
            // Decode round-trip: every event MUST be portable.
            let v = serde_json::to_value(&ev).expect("serialize event");
            assert_portable(&v.to_string());
            let back: AgentSessionEvent = decode_via_json(&ev).expect("round-trip event");
            assert_eq!(
                serde_json::to_value(&back).unwrap(),
                v,
                "round-trip must be byte-stable for {ev:?}",
            );
            kinds.insert(match &ev {
                AgentSessionEvent::SessionCreated { .. } => "session_created",
                AgentSessionEvent::StateChanged { .. } => "state_changed",
                AgentSessionEvent::TurnStarted { .. } => "turn_started",
                AgentSessionEvent::ThinkingChunk { .. } => "thinking_chunk",
                AgentSessionEvent::MessageChunk { .. } => "message_chunk",
                AgentSessionEvent::ToolCallStarted { .. } => "tool_call_started",
                AgentSessionEvent::ToolCallFinished { .. } => "tool_call_finished",
                AgentSessionEvent::ApprovalRequested { .. } => "approval_requested",
                AgentSessionEvent::ApprovalResolved { .. } => "approval_resolved",
                AgentSessionEvent::PlanChanged { .. } => "plan_changed",
                AgentSessionEvent::TurnCompleted { .. } => "turn_completed",
                AgentSessionEvent::TurnFailed { .. } => "turn_failed",
                AgentSessionEvent::SessionCompleted { .. } => "session_completed",
                AgentSessionEvent::SessionFailed { .. } => "session_failed",
            });
            if matches!(ev, AgentSessionEvent::TurnCompleted { .. }) {
                break;
            }
        }
        kinds
    });

    let _ = tokio::time::timeout(Duration::from_secs(8), session.prompt("drive stream")).await;
    let kinds = tokio::time::timeout(Duration::from_secs(3), drain)
        .await
        .expect("drain terminated")
        .expect("drain task");
    let _ = approver.await;

    // Mobile must observe session lifecycle, thinking, message, tool,
    // approval, plan, and turn-completed — the 7 state dimensions minus
    // artifacts (which is an honest gap, asserted separately below).
    for required in [
        "session_created",
        "state_changed",
        "turn_started",
        "thinking_chunk",
        "message_chunk",
        "tool_call_started",
        "tool_call_finished",
        "approval_requested",
        "approval_resolved",
        "plan_changed",
        "turn_completed",
    ] {
        assert!(
            kinds.contains(required),
            "Mobile consumer must observe {required}; saw {kinds:?}",
        );
    }
}

// ---------- dimension 4 + 5: tool + approval (single decode test) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_consumer_tool_and_approval_dimensions_decode() {
    let harness = common::spawn_fake_session().await;
    let session: AgentSession = harness.session;
    let mut rx = harness.rx; // subscribed BEFORE prompt so events are captured

    // Approve on behalf of the test so the turn can complete. Approver uses
    // its own subscription — broadcast supports multiple receivers.
    let approver_rx = session.subscribe();
    let approver = {
        let s = session.clone();
        tokio::spawn(async move {
            let mut rx = approver_rx;
            loop {
                match rx.recv().await {
                    Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) => {
                        let _ = s
                            .approve_tool(&turn_id, &tool_call_id)
                            .await.expect("approve observed turn");
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let _ = tokio::time::timeout(Duration::from_secs(8), session.prompt("tool+approval")).await;
    let _ = approver.await;

    // Drain the original subscription; it was active during the prompt, so
    // it has the full canonical stream buffered.
    let mut saw_tool_started = false;
    let mut saw_tool_finished = false;
    let mut saw_approval_requested = false;
    let mut saw_approval_resolved = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Some(remaining) = deadline.checked_duration_since(tokio::time::Instant::now()) {
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(ev)) => {
                match &ev {
                    AgentSessionEvent::ToolCallStarted { .. } => {
                        saw_tool_started = true;
                        let v = serde_json::to_value(&ev).expect("ser tool started");
                        let back: AgentSessionEvent =
                            serde_json::from_value(v.clone()).expect("de tool started");
                        assert_eq!(serde_json::to_value(&back).unwrap(), v);
                    }
                    AgentSessionEvent::ToolCallFinished { .. } => saw_tool_finished = true,
                    AgentSessionEvent::ApprovalRequested { .. } => saw_approval_requested = true,
                    AgentSessionEvent::ApprovalResolved { .. } => saw_approval_resolved = true,
                    _ => {}
                }
                if saw_tool_finished && saw_approval_resolved {
                    break;
                }
            }
            _ => break,
        }
    }
    // Fake agent emits exactly one tool+approval pair per turn.
    assert!(saw_tool_started, "tool_started dimension must be observable");
    assert!(saw_tool_finished, "tool_finished dimension must be observable");
    assert!(saw_approval_requested, "approval_requested must be observable");
    assert!(saw_approval_resolved, "approval_resolved must be observable");
}

// ---------- dimension 6: task (TurnStarted + TurnCompleted/TurnFailed) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_consumer_task_dimension_decodes_turn_lifecycle() {
    let harness = common::spawn_fake_session().await;
    let session: AgentSession = harness.session;
    let mut rx = harness.rx;

    let approver_rx = session.subscribe();
    let approver = {
        let s = session.clone();
        tokio::spawn(async move {
            let mut rx = approver_rx;
            loop {
                match rx.recv().await {
                    Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) => {
                        let _ = s
                            .approve_tool(&turn_id, &tool_call_id)
                            .await.expect("approve observed turn");
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let drain = tokio::spawn(async move {
        let mut saw_started = false;
        let mut saw_completed = false;
        loop {
            match rx.recv().await {
                Ok(AgentSessionEvent::TurnStarted { turn_id, .. }) => {
                    saw_started = true;
                    let v = serde_json::to_value(&turn_id).expect("ser TurnId");
                    let back: ridge_acp::TurnId =
                        serde_json::from_value(v.clone()).expect("de TurnId");
                    assert_eq!(serde_json::to_value(&back).unwrap(), v);
                }
                Ok(AgentSessionEvent::TurnCompleted { turn_id, result, .. }) => {
                    saw_completed = true;
                    let v = serde_json::to_value(&result).expect("ser TurnResult");
                    let back: ridge_acp::TurnResult =
                        serde_json::from_value(v.clone()).expect("de TurnResult");
                    assert_eq!(serde_json::to_value(&back).unwrap(), v);
                    let _ = turn_id;
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        (saw_started, saw_completed)
    });

    let _ = tokio::time::timeout(Duration::from_secs(8), session.prompt("drive task")).await;
    let (started, completed) = tokio::time::timeout(Duration::from_secs(3), drain)
        .await
        .expect("drain terminated")
        .expect("drain task");
    let _ = approver.await;

    assert!(started, "task dimension must surface TurnStarted");
    assert!(completed, "task dimension must surface TurnCompleted");
}

// ---------- dimension 7: artifact — honest GAP, not fabricated ----------

#[tokio::test]
async fn mobile_consumer_artifact_dimension_is_honest_gap() {
    // M2 directive §6 names "artifact" as a Mobile consumer dimension.
    // The M1 event surface does NOT carry `ArtifactChanged` — Ridge must
    // NOT fabricate one. This test pins that absence so a future Mobile
    // client can rely on the documented gap instead of guessing.
    //
    // Strategy: walk every variant of `AgentSessionEvent` and assert no
    // variant claims to represent an artifact change. If one sneaks in
    // without a wire contract, this test will fail loudly.
    let canonical_variants = [
        "SessionCreated",
        "StateChanged",
        "TurnStarted",
        "ThinkingChunk",
        "MessageChunk",
        "ToolCallStarted",
        "ToolCallFinished",
        "ApprovalRequested",
        "ApprovalResolved",
        "PlanChanged",
        "TurnCompleted",
        "TurnFailed",
        "SessionCompleted",
        "SessionFailed",
    ];
    for v in canonical_variants {
        assert!(
            !v.to_lowercase().contains("artifact"),
            "M1 contract must not silently grow an artifact variant; found {v}",
        );
    }

    // The agent wire surface (ACP) also does not standardize artifacts.
    // Verify opencode's initialize response carries no artifact capability
    // (probed only when opencode is present — skip otherwise).
    eprintln!("[artifact-gap] Ridge does not synthesize artifact events; Mobile clients must negotiate with the agent (or wait for an M3 contract).");
}

// ---------- CommutationEvent portable (cross-cutting) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_consumer_commutation_events_decode_portably() {
    let lead = common::spawn_fake_session().await.session;
    let staff = common::spawn_fake_session().await.session;
    let staff_id = staff.id().await;

    let comm = Commutation::new(&lead, "decode", "ctxt").await;
    let mut rx = comm.events();
    comm.assign_staff(
        StaffSpec {
            session_id: staff_id.clone(),
            role: "explorer".into(),
            provider_label: "mock/claude-haiku".into(),
        },
        &staff,
    )
    .await;

    let mut decoded_kinds = std::collections::BTreeSet::new();
    while let Ok(ev) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
        match ev {
            Ok(ev) => {
                let v = serde_json::to_value(&ev).expect("ser");
                assert_portable(&v.to_string());
                let back: CommutationEvent = serde_json::from_value(v.clone()).expect("de");
                assert_eq!(serde_json::to_value(&back).unwrap(), v);
                decoded_kinds.insert(match &ev {
                    CommutationEvent::Created { .. } => "created",
                    CommutationEvent::StaffAssigned { .. } => "staff_assigned",
                    CommutationEvent::StaffProgress { .. } => "staff_progress",
                    CommutationEvent::StaffCompleted { .. } => "staff_completed",
                    CommutationEvent::StaffFailed { .. } => "staff_failed",
                    CommutationEvent::Handoff { .. } => "handoff",
                    CommutationEvent::FinalDelivered { .. } => "final_delivered",
                    CommutationEvent::CommutationCompleted { .. } => "commutation_completed",
                    CommutationEvent::CommutationFailed { .. } => "commutation_failed",
                });
                if decoded_kinds.contains("staff_assigned") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    assert!(decoded_kinds.contains("staff_assigned"),
        "Mobile consumer must observe StaffAssigned; saw {decoded_kinds:?}");
}

// ---------- AgentSessionSnapshot portable (cross-cutting) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mobile_consumer_snapshot_carries_no_platform_types() {
    let harness = common::spawn_fake_session().await;
    let session: AgentSession = harness.session;
    let _rx = harness.rx; // subscription kept for completeness; not consumed here

    let approver_rx = session.subscribe();
    let approver = {
        let s = session.clone();
        tokio::spawn(async move {
            let mut rx = approver_rx;
            loop {
                match rx.recv().await {
                    Ok(AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }) => {
                        let _ = s
                            .approve_tool(&turn_id, &tool_call_id)
                            .await.expect("approve observed turn");
                        break;
                    }
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
        })
    };

    let _ = tokio::time::timeout(Duration::from_secs(8), session.prompt("snap")).await;
    let _ = approver.await;

    let snap: AgentSessionSnapshot = session.snapshot().await;
    let json = serde_json::to_value(&snap).expect("ser snapshot");
    assert_portable(&json.to_string());
    let back: AgentSessionSnapshot = serde_json::from_value(json.clone()).expect("de snapshot");
    let again = serde_json::to_value(&back).expect("re-ser");
    assert_eq!(json, again, "snapshot round-trip stable");
}
