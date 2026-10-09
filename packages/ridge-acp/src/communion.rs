//! `Commutation` — the coordinator for Lead + N Staff over one task.
//!
//! Important: this is NOT a transport and does NOT speak ACP frames itself.
//! It only:
//! - tracks which `AgentSession` is Lead
//! - tracks N `AgentSession`s assigned to the task as Staff
//! - aggregates their canonical events into `CommutationEvent`
//! - forwards task ownership transitions (assignment, handoff, final)
//!
//! Communication goes through `AgentSession::prompt` and
//! `AgentSession::subscribe`. The model is: Communion = coordination state
//! only; ACP frames belong to `AcpTransport`; session facts belong to
//! `AgentSession`.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex as AsyncMutex};

use crate::events::AgentSessionEvent;
use crate::session::{AgentSession, SessionId};
use crate::turn::TurnId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StaffSpec {
    pub session_id: SessionId,
    pub role: String,
    pub provider_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub provider_label: String,
    pub turns_completed: usize,
    pub active_turn: Option<TurnId>,
    pub lifecycle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommutationState {
    Created,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommutationSnapshot {
    pub state: CommutationState,
    pub lead: SessionId,
    pub staff: Vec<SessionId>,
    pub summaries: HashMap<SessionId, SessionSummary>,
    pub task_title: String,
    pub task_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommutationEvent {
    Created {
        lead: SessionId,
        staff: Vec<StaffSpec>,
        task_title: String,
        ts_unix_ms: u64,
    },
    StaffAssigned {
        lead: SessionId,
        staff: SessionId,
        task_title: String,
        ts_unix_ms: u64,
    },
    StaffProgress {
        lead: SessionId,
        staff: SessionId,
        turn_id: TurnId,
        ts_unix_ms: u64,
    },
    StaffCompleted {
        lead: SessionId,
        staff: SessionId,
        turn_id: TurnId,
        ts_unix_ms: u64,
    },
    StaffFailed {
        lead: SessionId,
        staff: SessionId,
        turn_id: TurnId,
        reason: String,
        ts_unix_ms: u64,
    },
    Handoff {
        lead: SessionId,
        from: SessionId,
        to: SessionId,
        turn_id: TurnId,
        ts_unix_ms: u64,
    },
    FinalDelivered {
        lead: SessionId,
        ts_unix_ms: u64,
    },
    CommutationCompleted {
        lead: SessionId,
        ts_unix_ms: u64,
    },
    CommutationFailed {
        lead: SessionId,
        reason: String,
        ts_unix_ms: u64,
    },
}

#[derive(Clone)]
pub struct Commutation {
    inner: Arc<AsyncMutex<CommutationInner>>,
    events: broadcast::Sender<CommutationEvent>,
    /// M5: optional reference to the Plan this commutation is executing.
    /// Communion does NOT own the Plan — `PlanStore` is the single source
    /// of truth; `Commutation` only holds the reference + per-task progress.
    plan_id: Option<crate::task::PlanId>,
}

struct CommutationInner {
    lead: SessionId,
    staff: Vec<StaffSpec>,
    summaries: HashMap<SessionId, SessionSummary>,
    state: CommutationState,
    task_title: String,
    task_context: String,
    /// M5: per-task progress for in-flight staff assignments.
    in_flight: HashMap<crate::task::TaskId, crate::task::TaskStatus>,
}

impl Commutation {
    /// Build a Commutation without a plan. Use `new_with_plan` for
    /// task-aware coordination.
    pub async fn new(
        lead: &AgentSession,
        task_title: impl Into<String>,
        task_context: impl Into<String>,
    ) -> Self {
        Self::new_with_plan(lead, task_title, task_context, None).await
    }

    /// M5: build a Commutation with an optional `PlanId` reference. The
    /// Plan itself is NOT cloned — `PlanStore` is the single source of
    /// truth; `Commutation` only holds the reference + per-task progress.
    pub async fn new_with_plan(
        lead: &AgentSession,
        task_title: impl Into<String>,
        task_context: impl Into<String>,
        plan_id: Option<crate::task::PlanId>,
    ) -> Self {
        let (tx, _rx) = broadcast::channel(256);
        let lead_id = lead.id().await;
        let inner = CommutationInner {
            lead: lead_id,
            staff: Vec::new(),
            summaries: HashMap::new(),
            state: CommutationState::Created,
            task_title: task_title.into(),
            task_context: task_context.into(),
            in_flight: HashMap::new(),
        };
        Self {
            inner: Arc::new(AsyncMutex::new(inner)),
            events: tx,
            plan_id,
        }
    }

    /// M5: get the PlanId this commutation references (None if no plan).
    pub fn plan_id(&self) -> Option<&crate::task::PlanId> {
        self.plan_id.as_ref()
    }

    /// M5: snapshot per-task progress for in-flight staff assignments.
    pub async fn in_flight_snapshot(&self) -> Vec<(crate::task::TaskId, crate::task::TaskStatus)> {
        self.inner.lock().await.in_flight.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }

    /// M5: delegate to `ready_tasks` over the Plan passed in.
    pub async fn next_ready_tasks(&self, plan: &crate::task::Plan) -> Vec<crate::task::TaskId> {
        crate::task::ready_tasks(plan)
    }

    /// M5: delegate to `blocked_by_failed_deps` over the Plan.
    pub async fn next_blocked_tasks(&self, plan: &crate::task::Plan) -> Vec<crate::task::TaskId> {
        crate::task::blocked_by_failed_deps(plan)
    }

    /// M5 §3: assign a Staff session to a specific Task. The task is
    /// recorded in `in_flight` so the run loop can later mark it
    /// Completed/Failed when the staff session finishes its turn.
    pub async fn assign_staff_for_task(
        &self,
        task_id: crate::task::TaskId,
        spec: StaffSpec,
        session: &AgentSession,
    ) {
        let staff_id = session.id().await;
        let lead_id;
        let task_title;
        {
            let mut g = self.inner.lock().await;
            g.staff.push(spec.clone());
            g.summaries.insert(
                staff_id.clone(),
                SessionSummary {
                    session_id: staff_id.clone(),
                    provider_label: spec.provider_label.clone(),
                    turns_completed: 0,
                    active_turn: Some(crate::session::TurnId(task_id.clone())),
                    lifecycle: "online".into(),
                },
            );
            g.in_flight.insert(task_id.clone(), crate::task::TaskStatus::Assigned);
            lead_id = g.lead.clone();
            task_title = g.task_title.clone();
        }
        let _ = self.events.send(CommutationEvent::StaffAssigned {
            lead: lead_id,
            staff: staff_id,
            task_title,
            ts_unix_ms: now_unix_ms(),
        });
    }

    /// M5 §3: mark a Task as Completed with a structured handoff. The
    /// caller is responsible for persisting via `PlanStore` separately.
    pub async fn complete_task(
        &self,
        task_id: &crate::task::TaskId,
        handoff: crate::task::HandoffPayload,
        completed_at_ms: u64,
    ) {
        let lead_id;
        let staff_id_resolved = {
            let mut g = self.inner.lock().await;
            g.in_flight.insert(task_id.clone(), crate::task::TaskStatus::Completed);
            lead_id = g.lead.clone();
            g.summaries
                .iter()
                .find(|(_, s)| s
                    .active_turn
                    .as_ref()
                    .map(|t| t.0.as_str() == task_id.as_str())
                    .unwrap_or(false))
                .map(|(_, s)| s.session_id.clone())
                .unwrap_or_else(|| SessionId(String::new()))
        };
        let _ = self.events.send(CommutationEvent::StaffCompleted {
            lead: lead_id,
            staff: staff_id_resolved,
            turn_id: crate::session::TurnId(format!("task-{task_id}")),
            ts_unix_ms: completed_at_ms,
        });
        let _ = handoff; // caller persists via PlanStore
    }

    /// M5 §3: mark a Task as Failed.
    pub async fn fail_task(&self, task_id: &crate::task::TaskId, reason: String) {
        let lead_id;
        {
            let mut g = self.inner.lock().await;
            g.in_flight.insert(task_id.clone(), crate::task::TaskStatus::Failed);
            lead_id = g.lead.clone();
        }
        let _ = self.events.send(CommutationEvent::StaffFailed {
            lead: lead_id,
            staff: SessionId(String::new()),
            turn_id: crate::session::TurnId(format!("task-{task_id}")),
            reason,
            ts_unix_ms: now_unix_ms(),
        });
    }

    pub fn events(&self) -> broadcast::Receiver<CommutationEvent> {
        self.events.subscribe()
    }

    pub async fn snapshot(&self) -> CommutationSnapshot {
        let g = self.inner.lock().await;
        CommutationSnapshot {
            state: g.state.clone(),
            lead: g.lead.clone(),
            staff: g.staff.iter().map(|s| s.session_id.clone()).collect(),
            summaries: g.summaries.clone(),
            task_title: g.task_title.clone(),
            task_context: g.task_context.clone(),
        }
    }

    pub async fn assign_staff(&self, spec: StaffSpec, session: &AgentSession) {
        let mut g = self.inner.lock().await;
        g.staff.push(spec.clone());
        g.summaries.insert(
            spec.session_id.clone(),
            SessionSummary {
                session_id: spec.session_id.clone(),
                provider_label: spec.provider_label.clone(),
                turns_completed: 0,
                active_turn: None,
                lifecycle: "online".into(),
            },
        );
        let lead_id = g.lead.clone();
        let task_title = g.task_title.clone();
        drop(g);
        let _ = self.events.send(CommutationEvent::StaffAssigned {
            lead: lead_id,
            staff: spec.session_id,
            task_title,
            ts_unix_ms: now_unix_ms(),
        });
        let _ = session; // session is bound for future subscription wiring
    }

    /// Subscribe to one staff session and reflect its canonical events into
    /// `CommutationEvent`. Returns when the staff session goes terminal
    /// (`turn_completed` or `turn_failed` for the current turn).
    pub async fn await_staff_turn(
        &self,
        staff: &AgentSession,
        expected_turn: &TurnId,
    ) -> Result<(), String> {
        let mut rx = staff.subscribe();
        let lead_id = self.inner.lock().await.lead.clone();
        let staff_id = staff.id().await;
        loop {
            match rx.recv().await {
                Ok(AgentSessionEvent::TurnStarted { .. }) => {
                    let _ = self.events.send(CommutationEvent::StaffProgress {
                        lead: lead_id.clone(),
                        staff: staff_id.clone(),
                        turn_id: expected_turn.clone(),
                        ts_unix_ms: now_unix_ms(),
                    });
                }
                Ok(AgentSessionEvent::TurnCompleted { turn_id, .. }) if turn_id == *expected_turn => {
                    {
                        let mut g = self.inner.lock().await;
                        if let Some(s) = g.summaries.get_mut(&staff_id) {
                            s.turns_completed += 1;
                            s.active_turn = None;
                        }
                    }
                    let _ = self.events.send(CommutationEvent::StaffCompleted {
                        lead: lead_id.clone(),
                        staff: staff_id.clone(),
                        turn_id,
                        ts_unix_ms: now_unix_ms(),
                    });
                    return Ok(());
                }
                Ok(AgentSessionEvent::TurnFailed { turn_id, error, .. })
                    if turn_id == *expected_turn =>
                {
                    let _ = self.events.send(CommutationEvent::StaffFailed {
                        lead: lead_id.clone(),
                        staff: staff_id.clone(),
                        turn_id,
                        reason: error.message,
                        ts_unix_ms: now_unix_ms(),
                    });
                    return Err("staff turn failed".into());
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Err("staff session closed".into());
                }
            }
        }
    }

    /// Run the commutation: assign task to all staff, prompt each in
    /// sequence (parallel would be a future slice — M1 keeps it linear so
    /// the deterministic event stream is easy to assert against).
    pub async fn run(&self, staff_pool: Vec<(StaffSpec, AgentSession)>) {
        let lead_id = self.inner.lock().await.lead.clone();
        let _ = self.events.send(CommutationEvent::Created {
            lead: lead_id.clone(),
            staff: staff_pool.iter().map(|(s, _)| s.clone()).collect(),
            task_title: self.inner.lock().await.task_title.clone(),
            ts_unix_ms: now_unix_ms(),
        });
        {
            let mut g = self.inner.lock().await;
            g.state = CommutationState::Running;
        }

        let mut handoff_from: Option<SessionId> = None;
        for (idx, (spec, session)) in staff_pool.iter().enumerate() {
            self.assign_staff(spec.clone(), session).await;
            // Lead -> Staff handoff: the prior staff's completion becomes
            // the context for the next.
            if let Some(prev) = handoff_from.clone() {
                let _ = self.events.send(CommutationEvent::Handoff {
                    lead: lead_id.clone(),
                    from: prev,
                    to: spec.session_id.clone(),
                    turn_id: TurnId(format!("handoff-{idx}")),
                    ts_unix_ms: now_unix_ms(),
                });
            }
            // Snapshot inner state once — format! re-locking inner in each
            // argument is fragile under multi-thread contention.
            let (task_title, task_context) = {
                let g = self.inner.lock().await;
                (g.task_title.clone(), g.task_context.clone())
            };
            let prior = handoff_from
                .as_ref()
                .map(|s| s.0.clone())
                .unwrap_or_else(|| "none".into());
            let prompt_text = format!(
                "[Commutation task] {task_title}\n[Context] {task_context}\n[Prior staff handoff] {prior}"
            );

            // Subscribe BEFORE triggering prompt so the terminal event
            // for the just-issued turn is delivered to *this* subscription
            // (broadcast does not replay history).
            let mut rx = session.subscribe();
            let staff_id = session.id().await;
            let turn_id = match session.prompt(&prompt_text).await {
                Ok(t) => t,
                Err(_) => {
                    {
                        let mut g = self.inner.lock().await;
                        g.state = CommutationState::Failed;
                    }
                    let _ = self.events.send(CommutationEvent::CommutationFailed {
                        lead: lead_id.clone(),
                        reason: "staff prompt failed".into(),
                        ts_unix_ms: now_unix_ms(),
                    });
                    return;
                }
            };

            // Reflect progress from rx into commutation events.
            let mut terminal_ok = false;
            loop {
                match rx.recv().await {
                    Ok(AgentSessionEvent::TurnStarted { .. }) => {
                        let _ = self.events.send(CommutationEvent::StaffProgress {
                            lead: lead_id.clone(),
                            staff: staff_id.clone(),
                            turn_id: turn_id.clone(),
                            ts_unix_ms: now_unix_ms(),
                        });
                    }
                    Ok(AgentSessionEvent::TurnCompleted { turn_id: t, .. })
                        if t == turn_id =>
                    {
                        {
                            let mut g = self.inner.lock().await;
                            if let Some(s) = g.summaries.get_mut(&staff_id) {
                                s.turns_completed += 1;
                                s.active_turn = None;
                            }
                        }
                        let _ = self.events.send(CommutationEvent::StaffCompleted {
                            lead: lead_id.clone(),
                            staff: staff_id.clone(),
                            turn_id: t,
                            ts_unix_ms: now_unix_ms(),
                        });
                        terminal_ok = true;
                        break;
                    }
                    Ok(AgentSessionEvent::TurnFailed {
                        turn_id: t,
                        error,
                        ..
                    }) if t == turn_id => {
                        let _ = self.events.send(CommutationEvent::StaffFailed {
                            lead: lead_id.clone(),
                            staff: staff_id.clone(),
                            turn_id: t,
                            reason: error.message,
                            ts_unix_ms: now_unix_ms(),
                        });
                        break;
                    }
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            if !terminal_ok {
                {
                    let mut g = self.inner.lock().await;
                    g.state = CommutationState::Failed;
                }
                let _ = self.events.send(CommutationEvent::CommutationFailed {
                    lead: lead_id.clone(),
                    reason: "staff did not complete".into(),
                    ts_unix_ms: now_unix_ms(),
                });
                return;
            }
            handoff_from = Some(spec.session_id.clone());
        }

        {
            let mut g = self.inner.lock().await;
            g.state = CommutationState::Completed;
        }
        let _ = self.events.send(CommutationEvent::FinalDelivered {
            lead: lead_id.clone(),
            ts_unix_ms: now_unix_ms(),
        });
        let _ = self.events.send(CommutationEvent::CommutationCompleted {
            lead: lead_id,
            ts_unix_ms: now_unix_ms(),
        });
    }
}

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
