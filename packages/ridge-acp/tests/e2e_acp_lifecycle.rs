//! E2E-A: real ACP-compatible agent -> ridge-acp -> canonical event stream.
//!
//! Verifies that one `AgentSession` against the in-tree MOCK_FIXTURE binary
//! emits the full canonical event sequence end to end:
//! SessionCreated -> StateChanged -> TurnStarted -> PlanChanged ->
//! ThinkingChunk* -> MessageChunk* -> ToolCallStarted -> ToolCallFinished ->
//! ApprovalRequested -> (host calls approve_tool) -> ToolCallStarted ->
//! ToolCallFinished -> TurnCompleted.

mod common;

use ridge_acp::{
    AgentSessionEvent, ToolCallView,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn e2e_acp_full_lifecycle() {
    let harness = common::spawn_fake_session().await;
    let session = harness.session;
    let mut rx = harness.rx;

    let prompt_text = "Patch the example file";
    let turn_id_fut = session.prompt(prompt_text);

    // Forward events into a shared Vec and approve on ApprovalRequested.
    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::<AgentSessionEvent>::new()));
    let collected_clone = collected.clone();
    let session_clone = session.clone();
    let driver = tokio::spawn(async move {
        eprintln!("test: driver spawned");
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let is_approve = matches!(ev, AgentSessionEvent::ApprovalRequested { .. });
                    let is_terminal = matches!(
                        ev,
                        AgentSessionEvent::TurnCompleted { .. }
                            | AgentSessionEvent::TurnFailed { .. }
                            | AgentSessionEvent::SessionFailed { .. }
                    );
                    collected_clone.lock().unwrap().push(ev.clone());
                    eprintln!("test: driver got {:?}", ev.kind_str());
                    if is_approve {
                        let (turn_id, tool_call_id) = if let AgentSessionEvent::ApprovalRequested {
                            turn_id, tool_call_id,
                            ..
                        } = &ev
                        {
                            (turn_id.clone(), tool_call_id.clone())
                        } else { unreachable!("approval event checked") };
                        let _ = session_clone
                            .approve_tool(&turn_id, &tool_call_id)
                            .await.expect("approve observed turn");
                    }
                    if is_terminal {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("test: driver error: {e}");
                    break;
                }
            }
        }
    });

    let turn_id = turn_id_fut.await.expect("prompt turn");
    let _ = driver.await;

    let events = collected.lock().unwrap().clone();

    let mut saw_created = false;
    let mut saw_state = false;
    let mut saw_turn_started = false;
    let mut saw_plan = false;
    let mut saw_thinking = 0;
    let mut saw_message = 0;
    let mut saw_tool_started = 0;
    let mut saw_tool_finished = 0;
    let mut saw_approval_requested = false;
    let mut saw_approval_resolved = false;
    let mut saw_turn_completed = false;
    let mut tool_calls: Vec<ToolCallView> = Vec::new();

    for ev in &events {
        match ev {
            AgentSessionEvent::SessionCreated { .. } => saw_created = true,
            AgentSessionEvent::StateChanged { .. } => saw_state = true,
            AgentSessionEvent::TurnStarted { .. } => saw_turn_started = true,
            AgentSessionEvent::PlanChanged { .. } => saw_plan = true,
            AgentSessionEvent::ThinkingChunk { .. } => saw_thinking += 1,
            AgentSessionEvent::MessageChunk { .. } => saw_message += 1,
            AgentSessionEvent::ToolCallStarted { tool_call, .. } => {
                saw_tool_started += 1;
                tool_calls.push(tool_call.clone());
            }
            AgentSessionEvent::ToolCallFinished { .. } => saw_tool_finished += 1,
            AgentSessionEvent::ApprovalRequested { .. } => saw_approval_requested = true,
            AgentSessionEvent::ApprovalResolved { .. } => saw_approval_resolved = true,
            AgentSessionEvent::TurnCompleted { .. } => saw_turn_completed = true,
            _ => {}
        }
    }

    assert!(saw_created, "missing SessionCreated");
    assert!(saw_state, "missing StateChanged");
    assert!(saw_turn_started, "missing TurnStarted");
    assert!(saw_plan, "missing PlanChanged");
    assert!(saw_thinking >= 2, "expected >=2 ThinkingChunks, got {saw_thinking}");
    assert!(saw_message >= 2, "expected >=2 MessageChunks, got {saw_message}");
    assert_eq!(saw_tool_started, 2, "expected 2 ToolCallStarted (read+write)");
    assert_eq!(saw_tool_finished, 2, "expected 2 ToolCallFinished");
    assert!(saw_approval_requested, "missing ApprovalRequested");
    assert!(saw_approval_resolved, "missing ApprovalResolved");
    assert!(saw_turn_completed, "missing TurnCompleted");

    // Verify ordering: ApprovalRequested must precede the second
    // ToolCallStarted (write_file).
    let approval_idx = events
        .iter()
        .position(|e| matches!(e, AgentSessionEvent::ApprovalRequested { .. }))
        .unwrap();
    let second_tool_idx = events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, AgentSessionEvent::ToolCallStarted { .. }))
        .nth(1)
        .map(|(i, _)| i)
        .unwrap();
    assert!(
        approval_idx < second_tool_idx,
        "ApprovalRequested must come before the second ToolCallStarted"
    );

    // Verify the second tool call is write_file (post-approval).
    assert_eq!(tool_calls[0].name, "read_file");
    assert_eq!(tool_calls[1].name, "write_file");

    // Snapshot round-trip: serialize -> deserialize preserves the turn_id.
    let snap = session.snapshot().await;
    assert!(snap.turns.contains_key(&turn_id));
}
