//! Prompt loop — read agent notifications, dispatch to canonical events.
//!
//! M2 architecture: the reader task in `transport.rs` pushes
//! `AcpNotification` frames into a broadcast channel. This loop subscribes,
//! demuxes by `SessionUpdate::kind`, dispatches to `AgentSessionEvent`, and
//! updates the canonical `SessionStatus` state machine.
//!
//! Properties:
//! - **No lock-across-await**: the loop awaits the broadcast receiver
//!   directly. Transport calls (call/notify) take the `transport` Mutex only
//!   across synchronous enqueue + drop, never across an `await`.
//! - **Single state machine**: `SessionStatus` transitions happen here (and
//!   in `approve_tool` / `cancel`). UI / Communion read `status()` only.
//! - **Idempotent shutdown**: if the broadcast channel closes (reader task
//!   exited) we return `SessionClosed`. The loop never panics on dropped
//!   frames.

use tokio::sync::broadcast;

use crate::error::{AcpErrorWrap, AgentSessionError};
use crate::events::AgentSessionEvent;
use crate::protocol::{AcpNotification, SessionUpdate};
use crate::session::{AgentSession, SessionStatus};
use crate::turn::{
    PlanEntryView, PlanSnapshot, ToolCallRecord, ToolCallView, TurnId, TurnPhase, TurnResult,
};

pub(super) async fn run(
    session: &AgentSession,
    notif_rx: &mut broadcast::Receiver<AcpNotification>,
    session_id: &crate::session::SessionId,
    turn_id: &TurnId,
    prompt_result_status: &str,
) -> Result<TurnId, AcpErrorWrap> {
    loop {
        match notif_rx.recv().await {
            Ok(notif) => {
                let params = notif.params.unwrap_or(serde_json::Value::Null);
                let update: SessionUpdate = match serde_json::from_value(params) {
                    Ok(u) => u,
                    Err(_) => continue, // malformed frame: skip, don't crash
                };
                match handle_update(
                    session,
                    session_id,
                    turn_id,
                    prompt_result_status,
                    update,
                )
                .await?
                {
                    Step::Continue => continue,
                    Step::Done(t) => return Ok(t),
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                close_turn(session, session_id, turn_id).await;
                return Err(AcpErrorWrap::SessionClosed);
            }
        }
    }
}

enum Step {
    Continue,
    Done(TurnId),
}

async fn handle_update(
    session: &AgentSession,
    session_id: &crate::session::SessionId,
    turn_id: &TurnId,
    prompt_result_status: &str,
    update: SessionUpdate,
) -> Result<Step, AcpErrorWrap> {
    match update {
        SessionUpdate::Plan { plan } => {
            let entries = plan
                .entries
                .into_iter()
                .map(|e| PlanEntryView {
                    id: e.id,
                    title: e.title,
                    status: e.status,
                })
                .collect();
            let plan_snapshot = PlanSnapshot { entries };
            session.record_plan(turn_id, plan_snapshot.clone()).await;
            session.emit(AgentSessionEvent::PlanChanged {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                plan: plan_snapshot,
                ts_unix_ms: now_unix_ms(),
            });
        }
        SessionUpdate::ThinkingChunk { text, .. } => {
            session.emit(AgentSessionEvent::ThinkingChunk {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                text,
                ts_unix_ms: now_unix_ms(),
            });
        }
        SessionUpdate::MessageChunk { text, .. } => {
            session.emit(AgentSessionEvent::MessageChunk {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                text,
                ts_unix_ms: now_unix_ms(),
            });
        }
        SessionUpdate::ToolCallStarted { tool_call, .. } => {
            let view = ToolCallView {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                arguments: tool_call.arguments.clone(),
                kind: tool_call.kind.clone(),
            };
            session
                .record_tool_call_started(
                    turn_id,
                    ToolCallRecord {
                        id: tool_call.id,
                        name: tool_call.name,
                        arguments: tool_call.arguments,
                        kind: tool_call.kind,
                        status: "started".into(),
                        output: None,
                    },
                )
                .await;
            session.emit(AgentSessionEvent::ToolCallStarted {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                tool_call: view,
                ts_unix_ms: now_unix_ms(),
            });
        }
        SessionUpdate::ToolCallFinished {
            tool_call_id,
            status,
            ..
        } => {
            session
                .record_tool_call_finished(turn_id, &tool_call_id, &status)
                .await;
            session.emit(AgentSessionEvent::ToolCallFinished {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                tool_call_id,
                status,
                ts_unix_ms: now_unix_ms(),
            });
        }
        SessionUpdate::ApprovalRequested {
            tool_call_id,
            reason,
            ..
        } => {
            session.inner.lock().await.pending_approval = Some((turn_id.clone(), tool_call_id.clone()));
            // Canonical state transition: turn is paused waiting on approval.
            // UI and Communion observe via `status()`.
            session.set_status(SessionStatus::WaitingApproval).await;
            session.emit(AgentSessionEvent::ApprovalRequested {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                tool_call_id,
                reason,
                ts_unix_ms: now_unix_ms(),
            });
        }
        SessionUpdate::TurnCompleted { status, .. } => {
            session.inner.lock().await.pending_approval = None;
            let phase = match status.as_str() {
                "cancelled" => TurnPhase::Cancelled,
                "failed" => TurnPhase::Failed,
                _ => TurnPhase::Completed,
            };
            session
                .finalize_turn_success(turn_id, phase, prompt_result_status)
                .await;
            session.emit(AgentSessionEvent::TurnCompleted {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                result: TurnResult {
                    phase,
                    message: prompt_result_status.to_string(),
                },
                ts_unix_ms: now_unix_ms(),
            });
            session.set_status(SessionStatus::Running).await;
            return Ok(Step::Done(turn_id.clone()));
        }
        SessionUpdate::TurnFailed { error, .. } => {
            session.inner.lock().await.pending_approval = None;
            // Preserve host-initiated cancel: `AgentSession::cancel` pre-marks
            // the active turn's `TurnPhase` as `Cancelled` before sending the
            // wire notification. Read the turn record under the same lock
            // the cancel path uses so the check is race-free. If the turn is
            // already `Cancelled`, do not overwrite it with `Failed`. This
            // keeps `SessionStatus` and `TurnPhase` in lockstep — single
            // source of truth.
            let cur_phase = {
                let g = session.inner.lock().await;
                g.turns.get(turn_id).map(|t| t.phase)
            };
            let (phase, message) = if cur_phase == Some(TurnPhase::Cancelled) {
                (TurnPhase::Cancelled, "cancelled".to_string())
            } else {
                (TurnPhase::Failed, error.message.clone())
            };
            session
                .finalize_turn_with_phase(turn_id, phase, &message)
                .await;
            session.emit(AgentSessionEvent::TurnFailed {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                error,
                ts_unix_ms: now_unix_ms(),
            });
            // Only flip the session status to `Failed` if the user has not
            // already set it to `Cancelled` (host-initiated cancel path).
            if session.status().await != SessionStatus::Cancelled {
                session.set_status(SessionStatus::Failed).await;
            }
            return Err(AcpErrorWrap::Session(AgentSessionError::AgentFailed {
                code: 0,
                message: "turn failed".into(),
            }));
        }
        SessionUpdate::SessionClosed { .. } => {
            close_turn(session, session_id, turn_id).await;
            return Err(AcpErrorWrap::SessionClosed);
        }
    }
    Ok(Step::Continue)
}

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

async fn close_turn(session: &AgentSession, session_id: &crate::session::SessionId, turn_id: &TurnId) {
    let cancelled = session.status().await == SessionStatus::Cancelled;
    let message = if cancelled { "cancelled" } else { "provider closed" };
    let phase = if cancelled { TurnPhase::Cancelled } else { TurnPhase::Failed };
    session.finalize_turn_with_phase(turn_id, phase, message).await;
    if !cancelled { session.set_status(SessionStatus::Failed).await; }
    session.emit(AgentSessionEvent::TurnFailed { session_id: session_id.clone(), turn_id: turn_id.clone(), error: crate::protocol::AcpErrorObj { code: -32001, message: message.into(), data: None }, ts_unix_ms: now_unix_ms() });
}
