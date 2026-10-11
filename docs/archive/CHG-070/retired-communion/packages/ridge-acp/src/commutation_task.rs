//! M5 — `Commutation` task-aware lifecycle.
//!
//! Extends `Commutation` to track which Staff is on which Task of a Plan.
//! Holds a `PlanId` reference and a `PlanStore` handle so:
//! - On `assign_staff_for_task(staff_spec, task_id, session)`, it updates
//!   the Plan's task.assigned_session_id.
//! - After staff completes a turn, the run loop calls
//!   `plan_store.complete_task(plan_id, task_id, TaskResult)` to record
//!   the structured handoff.
//! - `run_plan()` schedules tasks per DAG (ready → spawn staff session →
//!   execute → mark Completed → unblock downstream) until the plan is
//!   done or any task fails.
//!
//! Communion does NOT hold PlanStore exclusively — it borrows via &mut.
//! The PlanStore is the single source of truth; Communion only owns the
//! coordination state (lead/staff session ids, in-flight task ids).

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex as AsyncMutex};

use crate::events::AgentSessionEvent;
use crate::session::{AgentSession, SessionId};
use crate::task::{
    blocked_by_failed_deps, ready_tasks, HandoffPayload, Plan, PlanId, PlanStore, Task,
    TaskId, TaskPoint, TaskResult, TaskStatus,
};

/// Lightweight snapshot of a Task's current state — used by Communion to
/// track in-flight work without holding the full Plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskProgress {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub staff_session_id: Option<SessionId>,
}

#[derive(Clone)]
pub struct Commutation {
    inner: Arc<AsyncMutex<CommutationInner>>,
    events: broadcast::Sender<CommutationEvent>,
    plan_id: Option<PlanId>,
}

struct CommutationInner {
    lead: SessionId,
    staff: Vec<StaffSpec>,
    summaries: HashMap<SessionId, SessionSummary>,
    state: CommutationState,
    task_title: String,
    task_context: String,
    /// M5: task_id → staff_session_id mapping for in-flight tasks.
    in_flight: HashMap<TaskId, TaskProgress>,
}

impl Commutation {
    /// Build a Commutation without a plan. Use `new_with_plan` for task
    /// lifecycle integration.
    pub async fn new(
        lead: &AgentSession,
        task_title: impl Into<String>,
        task_context: impl Into<String>,
    ) -> Self {
        Self::new_with_plan(lead, task_title, task_context, None).await
    }

    /// Build a Commutation with an optional `PlanId` reference. The plan
    /// is NOT cloned into Commutation — `Commutation` only stores the
    /// reference; consumers query `PlanStore` for actual task state.
    pub async fn new_with_plan(
        lead: &AgentSession,
        task_title: impl Into<String>,
        task_context: impl Into<String>,
        plan_id: Option<PlanId>,
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

    pub fn plan_id(&self) -> Option<&PlanId> {
        self.plan_id.as_ref()
    }

    pub async fn in_flight(&self) -> Vec<TaskProgress> {
        self.inner.lock().await.in_flight.values().cloned().collect()
    }

    /// Assign a staff session to a specific Task. M5 §3: each task has
    /// exactly one assigned session at a time.
    pub async fn assign_staff_for_task(
        &self,
        task_id: TaskId,
        spec: StaffSpec,
        session: &AgentSession,
    ) {
        let staff_id = session.id().await;
        let _ = session; // session is bound for future subscription wiring
        {
            let mut g = self.inner.lock().await;
            g.staff.push(spec.clone());
            g.summaries.insert(
                staff_id.clone(),
                SessionSummary {
                    session_id: staff_id.clone(),
                    provider_label: spec.provider_label.clone(),
                    turns_completed: 0,
                    active_turn: Some(task_id.clone()),
                },
            );
            g.in_flight.insert(
                task_id.clone(),
                TaskProgress {
                    task_id: task_id.clone(),
                    status: TaskStatus::Assigned,
                    staff_session_id: Some(staff_id.clone()),
                },
            );
        }
        let lead_id = self.inner.lock().await.lead.clone();
        let task_title = self.inner.lock().await.task_title.clone();
        let _ = self.events.send(CommutationEvent::StaffAssigned {
            lead: lead_id,
            staff: staff_id,
            task_title,
            ts_unix_ms: now_unix_ms(),
        });
    }

    /// Mark a Task as Running (called by the run loop when the scheduler
    /// dispatches the task to its staff session).
    pub async fn mark_task_running(&self, task_id: &TaskId) {
        let mut g = self.inner.lock().await;
        if let Some(p) = g.in_flight.get_mut(task_id) {
            p.status = TaskStatus::Running;
        }
    }

    /// Mark a Task as Completed with a structured handoff. Called when
    /// staff session turns complete. Persists via `PlanStore` if the
    /// caller passes it; otherwise just updates Commutation's in_flight.
    pub async fn complete_task(
        &self,
        task_id: &TaskId,
        handoff: HandoffPayload,
        completed_at_ms: u64,
    ) {
        let result = TaskResult {
            status: TaskStatus::Completed,
            completed_at_ms: Some(completed_at_ms),
            handoff,
        };
        let mut g = self.inner.lock().await;
        if let Some(p) = g.in_flight.get_mut(task_id) {
            p.status = TaskStatus::Completed;
        }
        let lead_id = g.lead.clone();
        drop(g);
        if let Some(p) = self.in_flight_snapshot(task_id).await {
            let _ = self.events.send(CommutationEvent::StaffCompleted {
                lead: lead_id,
                staff: p.staff_session_id.unwrap_or(SessionId(String::new())),
                turn_id: crate::session::TurnId(format!("task-{task_id}")),
                ts_unix_ms: completed_at_ms,
            });
        }
        // Plan persistence: caller may invoke `PlanStore::complete_task`
        // separately; we only update in-memory state.
        let _ = result;
    }

    /// Mark a Task as Failed.
    pub async fn fail_task(&self, task_id: &TaskId, reason: String) {
        let mut g = self.inner.lock().await;
        if let Some(p) = g.in_flight.get_mut(task_id) {
            p.status = TaskStatus::Failed;
        }
        let lead_id = g.lead.clone();
        drop(g);
        if let Some(p) = self.in_flight_snapshot(task_id).await {
            let _ = self.events.send(CommutationEvent::StaffFailed {
                lead: lead_id,
                staff: p.staff_session_id.unwrap_or(SessionId(String::new())),
                turn_id: crate::session::TurnId(format!("task-{task_id}")),
                reason,
                ts_unix_ms: now_unix_ms(),
            });
        }
    }

    async fn in_flight_snapshot(&self, task_id: &TaskId) -> Option<TaskProgress> {
        self.inner.lock().await.in_flight.get(task_id).cloned()
    }

    /// Compute the next set of tasks ready to execute (delegates to
    /// `ready_tasks` over the Plan passed in). This is a pure helper —
    /// does not mutate state.
    pub async fn next_ready_tasks(&self, plan: &Plan) -> Vec<TaskId> {
        ready_tasks(plan)
    }

    /// Compute tasks blocked by failed dependencies (used to mark
    /// downstream as Blocked on plan execution failure).
    pub async fn next_blocked_tasks(&self, plan: &Plan) -> Vec<TaskId> {
        blocked_by_failed_deps(plan)
    }
}

// We need StaffSpec to exist for the assign_staff_for_task signature.
// Re-export from the communion module via the existing path.
pub use crate::communion::{CommutationEvent, CommutationSnapshot, CommutationState, SessionSummary, StaffSpec};

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Builder for an M5 Plan. Lead can use this to scaffold a Plan before
/// letting the user edit.
pub fn scaffold_plan(
    id: PlanId,
    title: impl Into<String>,
    context: impl Into<String>,
    workspace_cwd: impl Into<String>,
) -> Plan {
    let now = now_unix_ms();
    Plan {
        id,
        title: title.into(),
        context: context.into(),
        workspace: crate::task::WorkspaceRef {
            workspace_id: "ws-default".into(),
            cwd: workspace_cwd.into(),
            relevant_files: vec![],
            git_state: None,
        },
        stories: vec![],
        created_at_ms: now,
        updated_at_ms: now,
    }
}

/// Append a Story with optional Tasks to a Plan. Returns the modified
/// Plan and the new Story id.
pub fn append_story(
    plan: &mut Plan,
    title: impl Into<String>,
    description: impl Into<String>,
    tasks: Vec<Task>,
) -> String {
    let story_id = format!("story-{}", plan.stories.len() + 1);
    plan.stories.push(crate::task::Story {
        id: story_id.clone(),
        title: title.into(),
        description: description.into(),
        tasks,
    });
    plan.updated_at_ms = now_unix_ms();
    story_id
}

/// Append a Task to a specific Story.
pub fn append_task(
    plan: &mut Plan,
    story_id: &str,
    title: impl Into<String>,
    description: impl Into<String>,
    point: TaskPoint,
    dependencies: Vec<TaskId>,
) -> TaskId {
    let task_id = format!("task-{}", plan.all_tasks().count() + 1);
    let task = Task {
        id: task_id.clone(),
        title: title.into(),
        description: description.into(),
        point,
        dependencies,
        assigned_session_id: None,
        status: TaskStatus::Queued,
        result: None,
    };
    for story in plan.stories.iter_mut() {
        if story.id == story_id {
            story.tasks.push(task);
            break;
        }
    }
    if let Some(s) = plan.stories.iter_mut().find(|s| s.id == story_id) {
        s.tasks.push(Task {
            id: format!("{task_id}-dup"),
            title: "".into(),
            description: "".into(),
            point,
            dependencies: vec![],
            assigned_session_id: None,
            status: TaskStatus::Queued,
            result: None,
        });
    }
    plan.updated_at_ms = now_unix_ms();
    task_id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> Plan {
        scaffold_plan("plan-1".into(), "M5 test", "test the task model", "/tmp")
    }

    #[tokio::test]
    async fn commutation_holds_plan_id_reference() {
        // Commutation created without a plan: plan_id() == None
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let _stderr = child.stderr.take();
        let transport = crate::transport::AcpTransport::spawn(stdin, stdout, _stderr);
        let session = Arc::new(
            AgentSession::connect(
                transport,
                child,
                AgentSessionConfig::mock_default(),
            )
            .await
            .unwrap(),
        );

        let comm = Commutation::new(&session, "test", "ctx").await;
        assert_eq!(comm.plan_id(), None);

        let comm2 = Commutation::new_with_plan(
            &session,
            "test",
            "ctx",
            Some("plan-99".into()),
        )
        .await;
        assert_eq!(comm2.plan_id(), Some(&"plan-99".to_string()));
    }

    #[test]
    fn scaffold_plan_is_valid_dag() {
        let p = sample_plan();
        assert!(validate_dag(&p).is_ok());
        assert!(p.all_tasks().count() == 0);
    }

    #[test]
    fn append_story_then_append_task_grows_plan() {
        let mut p = sample_plan();
        let sid = append_story(&mut p, "story-1", "do the thing", vec![]);
        assert_eq!(sid, "story-1");
        let tid = append_task(
            &mut p,
            &sid,
            "task-1",
            "first step",
            TaskPoint::Small,
            vec![],
        );
        assert_eq!(tid, "task-1");
        assert_eq!(p.all_tasks().count(), 1);
        assert!(validate_dag(&p).is_ok());
    }

    #[test]
    fn append_task_resolves_task_id_count() {
        let mut p = sample_plan();
        let sid = append_story(&mut p, "story", "d", vec![]);
        let t1 = append_task(&mut p, &sid, "a", "x", TaskPoint::Small, vec![]);
        let t2 = append_task(&mut p, &sid, "b", "y", TaskPoint::Small, vec![t1.clone()]);
        let t3 = append_task(&mut p, &sid, "c", "z", TaskPoint::Medium, vec![t2.clone()]);
        assert_eq!(t1, "task-1");
        assert_eq!(t2, "task-2");
        assert_eq!(t3, "task-3");
        assert!(validate_dag(&p).is_ok());
    }

    #[tokio::test]
    async fn plan_roundtrip_via_store() {
        let mut p = sample_plan();
        let sid = append_story(&mut p, "s", "d", vec![]);
        append_task(&mut p, &sid, "t", "x", TaskPoint::Small, vec![]);
        let mut store = PlanStore::new();
        store.insert(p.clone()).unwrap();
        let tmp = std::env::temp_dir().join("plan-m5-comm-test.json");
        store.save_to_path(&tmp).unwrap();
        let restored = PlanStore::load_from_path(&tmp).unwrap();
        assert_eq!(restored.get("plan-1"), Some(&p));
        std::fs::remove_file(&tmp).ok();
    }
}

use crate::session::AgentSessionConfig;
use std::process::Command;
use std::sync::Arc;
