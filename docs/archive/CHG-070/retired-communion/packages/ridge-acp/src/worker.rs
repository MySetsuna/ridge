//! One event-driven task execution, including same-turn approval and host policy.

use std::future::Future;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use crate::error::AcpErrorWrap;
use crate::events::AgentSessionEvent;
use crate::session::{AgentSession, SessionId, TurnId};
use crate::task::{HandoffPayload, TaskResult, TaskStatus};
use crate::turn::TurnPhase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPolicy {
    pub per_task_timeout: Duration,
    pub auto_approve: bool,
}
impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self { per_task_timeout: Duration::from_secs(3600), auto_approve: false }
    }
}
#[derive(Debug, Clone)]
pub struct WorkerOutcome {
    pub status: TaskStatus,
    pub handoff: HandoffPayload,
    pub turn_id: TurnId,
    pub approved: bool,
}

pub async fn run_one_task(session: &AgentSession, prompt: &str, policy: ExecutionPolicy) -> WorkerOutcome {
    run_one_task_observed(session, prompt, policy, |_| async { Ok(()) }).await
}

pub async fn run_one_task_observed<F, Fut>(
    session: &AgentSession, prompt: &str, policy: ExecutionPolicy, mut observe: F,
) -> WorkerOutcome
where F: FnMut(AgentSessionEvent) -> Fut, Fut: Future<Output = Result<(), String>> {
    let rx = session.subscribe();
    let auto_approve = policy.auto_approve;
    let outcome = drive_task(session.id().await, session.prompt(prompt), rx, policy, |ev| {
        let observed = observe(ev.clone());
        async move {
            observed.await?;
            if auto_approve {
                if let AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. } = ev {
                    session.approve_tool(&turn_id, &tool_call_id).await.map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        }
    }).await;
    if outcome.handoff.diff_check_status == "timeout" || outcome.handoff.diff_check_status == "host_error" {
        session.shutdown().await;
    }
    outcome
}

// ponytail: one driver for production and controlled tests; no second worker implementation.
pub(crate) async fn drive_task<P, F, Fut>(
    session_id: SessionId, prompt: P, mut rx: broadcast::Receiver<AgentSessionEvent>,
    policy: ExecutionPolicy, mut observe: F,
) -> WorkerOutcome
where P: Future<Output = Result<TurnId, AcpErrorWrap>>,
      F: FnMut(AgentSessionEvent) -> Fut, Fut: Future<Output = Result<(), String>> {
    tokio::pin!(prompt);
    let deadline = tokio::time::sleep(policy.per_task_timeout);
    tokio::pin!(deadline);
    let mut prompt_done = false;
    let mut outcome = WorkerOutcome {
        status: TaskStatus::Running, turn_id: TurnId(String::new()), approved: false,
        handoff: HandoffPayload { diff_check_status: "unknown".into(), ..Default::default() },
    };
    let mut files = std::collections::HashMap::new();
    loop {
        tokio::select! {
            biased;
            _ = &mut deadline => return fail(outcome, format!("timeout after {:?}", policy.per_task_timeout), "timeout"),
            event = rx.recv() => {
                let ev = match event {
                    Ok(ev) => ev,
                    Err(e) => return fail(outcome, format!("session events lost: {e}"), "host_error"),
                };
                let value = serde_json::to_value(&ev).expect("canonical event serializes");
                if value["session_id"].as_str() != Some(&session_id.0) { continue; }
                if let AgentSessionEvent::TurnStarted { turn_id, .. } = &ev {
                    if outcome.turn_id.0.is_empty() { outcome.turn_id = turn_id.clone(); }
                }
                if let Some(id) = value["turn_id"].as_str() {
                    if id != outcome.turn_id.0 { continue; }
                }
                if let Err(e) = observe(ev.clone()).await {
                    return fail(outcome, format!("host state update failed: {e}"), "host_error");
                }
                match ev {
                    AgentSessionEvent::MessageChunk { text, .. } => outcome.handoff.summary.push_str(&text),
                    AgentSessionEvent::ToolCallStarted { tool_call, .. } if tool_call.kind == "write" => {
                        if let Some(path) = tool_call.arguments.get("path").and_then(|v| v.as_str()) {
                            files.insert(tool_call.id, path.to_string());
                        }
                    }
                    AgentSessionEvent::ToolCallFinished { tool_call_id, status, .. } => {
                        if status == "succeeded" {
                            if let Some(path) = files.remove(&tool_call_id) { outcome.handoff.artifacts.push(path); }
                        }
                    }
                    AgentSessionEvent::ApprovalResolved { approved, .. } => outcome.approved |= approved,
                    AgentSessionEvent::TurnCompleted { result, .. } => {
                        outcome.status = match result.phase {
                            TurnPhase::Completed => TaskStatus::Completed,
                            TurnPhase::Cancelled => TaskStatus::Cancelled,
                            _ => TaskStatus::Failed,
                        };
                        if outcome.handoff.summary.is_empty() { outcome.handoff.summary = result.message; }
                        return outcome;
                    }
                    AgentSessionEvent::TurnFailed { error, .. } => {
                        outcome.status = if error.message == "cancelled" { TaskStatus::Cancelled } else { TaskStatus::Failed };
                        outcome.handoff.notes.push(error.message);
                        return outcome;
                    }
                    AgentSessionEvent::SessionFailed { error, .. } => return fail(outcome, error.message, "host_error"),
                    AgentSessionEvent::SessionCompleted { .. } => return fail(outcome, "session closed before completion".into(), "host_error"),
                    _ => {}
                }
            }
            result = &mut prompt, if !prompt_done => {
                prompt_done = true;
                if let Err(e) = result { return fail(outcome, format!("prompt failed: {e}"), "failed"); }
                // Only a canonical terminal event can complete this task.
            }
        }
    }
}
fn fail(mut outcome: WorkerOutcome, message: String, check: &str) -> WorkerOutcome {
    outcome.status = TaskStatus::Failed;
    outcome.handoff.notes.push(message.clone());
    if outcome.handoff.summary.is_empty() { outcome.handoff.summary = message; }
    outcome.handoff.diff_check_status = check.into();
    outcome
}
pub fn outcome_to_result(outcome: &WorkerOutcome) -> TaskResult {
    TaskResult { status: outcome.status, completed_at_ms: Some(crate::workflow::now_unix_ms()), handoff: outcome.handoff.clone() }
}
#[cfg(test)]
#[path = "test_harness.rs"]
mod tests;
