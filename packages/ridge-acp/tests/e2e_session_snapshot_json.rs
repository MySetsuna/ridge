//! E2E-C: `AgentSessionSnapshot` serializes to portable JSON.
//!
//! The whole point of the canonical event bus is that Mobile / Desktop
//! consumers can read it without depending on Tauri invokes, shell ANSI, or
//! Desktop private store. This test prints one snapshot to stdout as pretty
//! JSON and parses it back, asserting:
//! - the snapshot serializes with `Serialize`
//! - the deserialized value matches what the live session produced
//! - no Tauri / shell / runtime types appear in the JSON

mod common;

use std::time::Duration;

use ridge_acp::{AgentSession, AgentSessionEvent, AgentSessionSnapshot};

fn assert_snapshot_is_portable(s: &serde_json::Value) {
    let s = s.to_string();
    // Forbidden substrings — anything Tauri / shell / Desktop-private would
    // show up here and violate the boundary.
    for needle in [
        "tauri",
        "pty",
        "ansi",
        "shell",
        "desktop",
        "invoke",
        "Tauri",
    ] {
        assert!(
            !s.to_lowercase().contains(&needle.to_lowercase()),
            "snapshot contains forbidden boundary token: {needle}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_snapshot_json_roundtrip() {
    let harness = common::spawn_fake_session().await;
    let session: AgentSession = harness.session;
    let mut rx = harness.rx;

    // Subscribe + run one prompt to populate the snapshot.
    let approver = {
        let s = session.clone();
        tokio::spawn(async move {
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

    let _ = session.prompt("drive one turn").await.expect("prompt");
    let _ = approver.await;

    let snap: AgentSessionSnapshot = session.snapshot().await;
    let json = serde_json::to_value(&snap).expect("serialize snapshot");
    assert_snapshot_is_portable(&json);

    // Pretty-print to stdout as proof-of-boundary — visible in `--nocapture`.
    println!("\n--- AgentSessionSnapshot JSON ---");
    println!(
        "{}",
        serde_json::to_string_pretty(&json).expect("pretty")
    );
    println!("--- end ---\n");

    // Round-trip the snapshot through serde to prove schema stability.
    let back: AgentSessionSnapshot =
        serde_json::from_value(json.clone()).expect("deserialize snapshot");
    let again = serde_json::to_value(&back).expect("re-serialize");
    assert_eq!(json, again, "snapshot round-trip must be stable");

    // Sanity: there is at least one turn recorded.
    assert!(!snap.turns.is_empty(), "expected at least one turn");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_events_json_roundtrip() {
    // Drain canonical events into a Vec, serialize, deserialize, prove the
    // wire shape is stable end-to-end.
    let harness = common::spawn_fake_session().await;
    let session: AgentSession = harness.session;
    let mut rx = harness.rx;

    // Approver uses its own subscription — broadcast supports multiple
    // receivers, and rx is owned by the drain loop below.
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

    let mut drained: Vec<serde_json::Value> = Vec::new();
    let drain_handle = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev @ AgentSessionEvent::TurnCompleted { .. }) => {
                    let v = serde_json::to_value(&ev).expect("serialize");
                    assert_snapshot_is_portable(&v);
                    let back: AgentSessionEvent =
                        serde_json::from_value(v.clone()).expect("deserialize");
                    let again = serde_json::to_value(&back).expect("re-serialize");
                    assert_eq!(v, again, "event round-trip must be stable");
                    drained.push(v);
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        drained
    });

    // Prompt MUST be driven for events to flow (the prompt loop dispatches
    // notifications to the broadcast channel). Run it concurrently with drain.
    let _ = tokio::time::timeout(Duration::from_secs(8), session.prompt("drain events")).await;
    let drained = tokio::time::timeout(Duration::from_secs(3), drain_handle)
        .await
        .expect("drain did not finish")
        .expect("drain task join");
    let _ = approver.await;

    assert!(!drained.is_empty(), "expected at least one terminal event");
}
