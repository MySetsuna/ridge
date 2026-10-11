//! Canonical PlanStore execution and projections. Transport and Task truth are never duplicated.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use crate::communion::StaffSpec;
use crate::events::AgentSessionEvent;
use crate::session::{AgentSession, AgentSessionConfig, SessionId, TurnId};
use crate::task::{blocked_by_failed_deps, ready_tasks, HandoffPayload, Plan, PlanId, PlanStore, TaskId, TaskResult, TaskStatus};
use crate::worker::{outcome_to_result, run_one_task_observed, ExecutionPolicy};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    Completed,
    FinishedWithFailure,
    Recoverable,
    #[default]
    Active,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LeadFinalOutcome {
    pub plan_id: PlanId,
    pub state: WorkflowState,
    pub task_outcomes: Vec<TaskOutcome>,
    pub failed_or_blocked: Vec<TaskId>,
    pub summary: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskOutcome {
    pub task_id: TaskId,
    pub title: String,
    pub staff_session_id: Option<String>,
    pub status: TaskStatus,
    pub handoff: HandoffPayload,
    pub completed_at_ms: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowEvent {
    PlanUpdated { plan: Plan },
    AgentEvent { plan_id: PlanId, task_id: TaskId, event: AgentSessionEvent },
    LeadFinal { outcome: LeadFinalOutcome },
}

#[derive(Clone)]
pub struct WorkflowCoordinator {
    plan_id: PlanId,
    store: Arc<AsyncMutex<PlanStore>>,
    lead_session: Arc<AgentSession>,
    staff_pool: Arc<AsyncMutex<Vec<(StaffSpec, Arc<AgentSession>)>>>,
    running: Arc<AsyncMutex<HashMap<TaskId, Arc<AgentSession>>>>,
    final_outcome: Arc<AsyncMutex<Option<LeadFinalOutcome>>>,
    path: Option<PathBuf>,
    events: broadcast::Sender<WorkflowEvent>,
    run_guard: Arc<AsyncMutex<()>>,
}
impl WorkflowCoordinator {
    pub fn new(plan_id: PlanId, store: PlanStore, lead: Arc<AgentSession>) -> Self {
        Self::new_shared(plan_id, Arc::new(AsyncMutex::new(store)), lead, None)
    }
    pub fn new_shared(plan_id: PlanId, store: Arc<AsyncMutex<PlanStore>>, lead: Arc<AgentSession>, path: Option<PathBuf>) -> Self {
        Self { plan_id, store, lead_session: lead, staff_pool: Default::default(), running: Default::default(),
            final_outcome: Default::default(), path, events: broadcast::channel(256).0, run_guard: Default::default() }
    }
    pub fn events(&self) -> broadcast::Receiver<WorkflowEvent> { self.events.subscribe() }
    pub fn lead_session(&self) -> Arc<AgentSession> { self.lead_session.clone() }
    pub async fn final_outcome(&self) -> Option<LeadFinalOutcome> { self.final_outcome.lock().await.clone() }
    pub async fn current_plan(&self) -> Plan { self.store.lock().await.get(&self.plan_id).expect("plan exists").clone() }
    pub async fn ready_tasks(&self) -> Vec<TaskId> { ready_tasks(&self.current_plan().await) }
    pub async fn blocked_tasks(&self) -> Vec<TaskId> { blocked_by_failed_deps(&self.current_plan().await) }

    async fn update_task<F>(&self, task_id: &str, update: F) -> Result<(), String>
    where F: FnOnce(&mut crate::task::Task) {
        self.update_task_if(task_id, None, update).await
    }
    async fn update_task_if<F>(&self, task_id: &str, expected: Option<&[TaskStatus]>, update: F) -> Result<(), String>
    where F: FnOnce(&mut crate::task::Task) {
        let mut store = self.store.lock().await;
        let backup = store.get(&self.plan_id).ok_or("plan not found")?.clone();
        let plan = store.get_mut(&self.plan_id).unwrap();
        let task = plan.find_task_mut(task_id).ok_or("task not found")?;
        if expected.is_some_and(|states| !states.contains(&task.status)) { return Err("task is already active or stopped".into()); }
        update(task);
        plan.updated_at_ms = now_unix_ms();
        if let Some(path) = &self.path {
            if let Err(e) = persist_plan_store_atomic(&store, path).await { store.insert(backup)?; return Err(e); }
        }
        let _ = self.events.send(WorkflowEvent::PlanUpdated { plan: store.get(&self.plan_id).unwrap().clone() });
        Ok(())
    }
    pub async fn register_staff(&self, session: Arc<AgentSession>) {
        let spec = StaffSpec { session_id: session.id().await, role: "staff".into(), provider_label: session.provider().await.kind.as_str().into() };
        self.staff_pool.lock().await.push((spec, session));
    }
    pub async fn spawn_staff_for_task(&self, _task_id: &TaskId, spec: StaffSpec) -> Result<Arc<AgentSession>, String> {
        let existing = self.staff_pool.lock().await.iter().find(|(s, _)| s.session_id == spec.session_id).map(|(_, sess)| sess.clone());
        if let Some(session) = existing { return Ok(session); }
        let plan = self.current_plan().await;
        let mut provider = plan.staff.get(&spec.session_id.0).cloned().unwrap_or(self.lead_session.provider().await);
        provider.config.insert("cwd".into(), serde_json::json!(plan.workspace.cwd));
        let mut spawn = crate::provider_adapter::into_spawn_spec(&provider).map_err(|e| e.reason)?;
        spawn.cwd = Some(PathBuf::from(&plan.workspace.cwd));
        if let Some(program) = provider.config.get("program").and_then(|value| value.as_str()) { spawn.program = program.into(); }
        if let Some(args) = provider.config.get("args") { spawn.args = serde_json::from_value(args.clone()).map_err(|e| format!("provider args: {e}"))?; }
        let (transport, child) = crate::spawn::spawn_acp_agent(&spawn).map_err(|e| e.to_string())?;
        let session = Arc::new(AgentSession::connect(transport, child, AgentSessionConfig {
            provider, client_name: "ridge-workflow".into(), client_version: env!("CARGO_PKG_VERSION").into(),
        }).await.map_err(|e| e.to_string())?);
        self.staff_pool.lock().await.push((StaffSpec { session_id: spec.session_id.clone(), ..spec }, session.clone()));

        Ok(session)
    }
    pub async fn execute_task(&self, task_id: &TaskId, session: Arc<AgentSession>, prompt: &str) -> Result<TaskResult, String> {
        self.execute_task_with_policy(task_id, session, prompt, ExecutionPolicy::default()).await
    }
    pub async fn execute_task_with_policy(&self, task_id: &TaskId, session: Arc<AgentSession>, prompt: &str, policy: ExecutionPolicy) -> Result<TaskResult, String> {
        let session_id = session.id().await.0;
        self.update_task_if(task_id, Some(&[TaskStatus::Queued, TaskStatus::Assigned]), |task| { task.assigned_session_id = Some(session_id); task.status = TaskStatus::Assigned; task.result = None; }).await?;
        self.update_task(task_id, |task| task.status = TaskStatus::Running).await?;
        self.running.lock().await.insert(task_id.clone(), session.clone());
        let outcome = run_one_task_observed(&session, prompt, policy, |ev| async move {
            let status = match ev {
                AgentSessionEvent::ApprovalRequested { .. } => Some(TaskStatus::WaitingApproval),
                AgentSessionEvent::ApprovalResolved { .. } => Some(TaskStatus::Running),
                _ => None,
            };
            if let Some(status) = status { self.update_task(task_id, |task| task.status = status).await?; }
            let _ = self.events.send(WorkflowEvent::AgentEvent { plan_id: self.plan_id.clone(), task_id: task_id.clone(), event: ev });
            Ok(())
        }).await;
        let mut result = outcome_to_result(&outcome);
        if self.current_plan().await.find_task(task_id).is_some_and(|task| task.status == TaskStatus::Cancelled) { result.status = TaskStatus::Cancelled; result.handoff.summary = "cancelled by user".into(); }
        let persisted = self.persist_result(task_id, &session, result.clone()).await;
        self.running.lock().await.remove(task_id);
        persisted?;
        Ok(result)
    }
    pub async fn persist_result(&self, task_id: &TaskId, _session: &Arc<AgentSession>, result: TaskResult) -> Result<(), String> {
        self.update_task(task_id, |task| { task.status = result.status; task.result = Some(result); }).await
    }
    pub async fn approve_task(&self, task_id: &str, turn_id: &TurnId, tool_call_id: &str, approved: bool) -> Result<(), String> {
        let session = self.running.lock().await.get(task_id).cloned().ok_or("task has no live turn; Resume required")?;
        if approved { session.approve_tool(turn_id, tool_call_id).await } else { session.reject_tool(turn_id, tool_call_id).await }.map_err(|e| e.to_string())
    }
    pub async fn resume_task(&self, task_id: &str) -> Result<(), String> {
        let plan = self.current_plan().await;
        if plan.find_task(task_id).ok_or("task not found")?.status != TaskStatus::Interrupted { return Err("only Interrupted tasks can Resume".into()); }
        // Old transport is never reused; assigned id still references persisted provider configuration.
        if !self.running.lock().await.is_empty() { return Err("wait until the active workflow stops before Resume".into()); }
        let old_id = plan.find_task(task_id).unwrap().assignee.as_ref();
        self.staff_pool.lock().await.retain(|(spec, _)| Some(&spec.session_id.0) != old_id);
        self.update_task(task_id, |task| { task.status = TaskStatus::Queued; task.result = None; }).await
    }
    pub async fn cancel(&self) -> Result<(), String> {
        let sessions: Vec<_> = self.running.lock().await.iter().map(|(id, session)| (id.clone(), session.clone())).collect();
        let mut failure = None;
        for (id, session) in sessions {
            if let Err(e) = self.update_task(&id, |task| task.status = TaskStatus::Cancelled).await { failure = Some(e); }
            let _ = session.cancel(None).await;
            session.shutdown().await;
        }
        if self.lead_session.snapshot().await.active_turn.is_some() { self.lead_session.shutdown().await; }
        match failure { Some(e) => Err(e), None => Ok(()) }
    }
    async fn shutdown_staff(&self) {
        let pool = std::mem::take(&mut *self.staff_pool.lock().await);
        for (_, session) in pool { session.shutdown().await; }
    }
    pub async fn aggregate_lead_final(&self) -> LeadFinalOutcome { aggregate_plan(&self.current_plan().await) }
    pub async fn finalize_workflow(&self) -> LeadFinalOutcome {
        let outcome = self.aggregate_lead_final().await;
        if matches!(outcome.state, WorkflowState::Completed | WorkflowState::FinishedWithFailure) {
            *self.final_outcome.lock().await = Some(outcome.clone());
            let _ = self.events.send(WorkflowEvent::LeadFinal { outcome: outcome.clone() });
        }
        outcome
    }
    pub fn build_task_prompt(&self, plan: &Plan, task_id: &TaskId) -> Result<String, String> {
        let task = plan.find_task(task_id).ok_or("task not found")?;
        let handoffs: Vec<_> = task.dependencies.iter().filter_map(|id| plan.find_task(id).map(|task| (id, &task.result))).collect();
        Ok(format!("Task {}: {}\n{}\nWorkspace: {}\nRelevant files: {}\nStructured handoffs: {}", task.id, task.title, task.description,
            plan.workspace.cwd, serde_json::to_string(&plan.workspace.relevant_files).map_err(|e| e.to_string())?,
            serde_json::to_string(&handoffs).map_err(|e| e.to_string())?))
    }
    pub async fn run_plan(&self) -> Result<LeadFinalOutcome, String> {
        let _guard = self.run_guard.try_lock().map_err(|_| "workflow already running")?;
        if !self.current_plan().await.confirmed { return Err("plan requires user confirmation".into()); }
        crate::task::validate_dag(&self.current_plan().await)?;
        loop {
            // Propagate failed upstream state through the whole DAG, not just one layer.
            loop {
                let blocked = self.blocked_tasks().await;
                if blocked.is_empty() { break; }
                for id in blocked { self.update_task(&id, |task| task.status = TaskStatus::Blocked).await?; }
            }
            let plan = self.current_plan().await;
            let ready = ready_tasks(&plan);
            if ready.is_empty() { break; }
            let mut batch = tokio::task::JoinSet::new();
            let mut occupied = std::collections::HashSet::new();
            for id in ready {
                let task = plan.find_task(&id).unwrap();
                let pool = self.staff_pool.lock().await;
                let available = pool.iter().find(|(_, session)| !occupied.contains(&(Arc::as_ptr(session) as usize))).map(|(spec, _)| spec.session_id.0.clone());
                let alias = task.assignee.clone().or(available).or_else(|| {
                    let mut names: Vec<_> = plan.staff.keys().cloned().collect(); names.sort();
                    names.into_iter().find(|name| !pool.iter().any(|(spec, session)| spec.session_id.0 == *name && occupied.contains(&(Arc::as_ptr(session) as usize))))
                }).unwrap_or_else(|| format!("staff-{}", task.id));
                drop(pool);
                let spec = StaffSpec { session_id: SessionId(alias), role: "staff".into(), provider_label: "configured".into() };
                let session = match self.spawn_staff_for_task(&id, spec).await {
                    Ok(session) => session,
                    Err(e) => { self.update_task(&id, |task| { task.status = TaskStatus::Failed; task.result = Some(TaskResult { status: TaskStatus::Failed, completed_at_ms: Some(now_unix_ms()), handoff: HandoffPayload { summary: e, ..Default::default() } }); }).await?; continue; }
                };
                if !occupied.insert(Arc::as_ptr(&session) as usize) { continue; }
                let alias = self.staff_pool.lock().await.iter().find(|(_, existing)| Arc::ptr_eq(existing, &session)).unwrap().0.session_id.0.clone();
                self.update_task(&id, |task| task.assignee = Some(alias)).await?;
                let prompt = self.build_task_prompt(&plan, &id)?;
                let me = self.clone();
                batch.spawn(async move { me.execute_task(&id, session, &prompt).await });
            }
            while let Some(result) = batch.join_next().await {
                let error = match result { Ok(Ok(_)) => None, Ok(Err(e)) => Some(e), Err(e) => Some(e.to_string()) };
                if let Some(e) = error { batch.abort_all(); self.shutdown_staff().await; return Err(e); }
            }
        }
        let outcome = self.aggregate_lead_final().await;
        if matches!(outcome.state, WorkflowState::Completed | WorkflowState::FinishedWithFailure) {
            let prompt = format!("Summarize the following structured task outcome. Do not run tasks again: {}", serde_json::to_string(&outcome).map_err(|e| e.to_string())?);
            self.lead_session.prompt(&prompt).await.map_err(|e| e.to_string())?;
        }
        let final_outcome = self.finalize_workflow().await;
        if matches!(final_outcome.state, WorkflowState::Completed | WorkflowState::FinishedWithFailure) { self.shutdown_staff().await; }
        Ok(final_outcome)
    }
}

pub fn aggregate_plan(plan: &Plan) -> LeadFinalOutcome {
    let task_outcomes: Vec<_> = plan.all_tasks().map(|task| TaskOutcome {
        task_id: task.id.clone(), title: task.title.clone(), staff_session_id: task.assigned_session_id.clone(), status: task.status,
        handoff: task.result.as_ref().map(|result| result.handoff.clone()).unwrap_or_default(),
        completed_at_ms: task.result.as_ref().and_then(|result| result.completed_at_ms),
    }).collect();
    let state = if plan.all_tasks().any(|task| task.status.is_recoverable()) { WorkflowState::Recoverable }
        else if plan.all_tasks().any(|task| !task.status.is_execution_stopped()) { WorkflowState::Active }
        else if plan.all_tasks().all(|task| task.status.is_success_terminal()) { WorkflowState::Completed }
        else { WorkflowState::FinishedWithFailure };
    let failed_or_blocked: Vec<_> = plan.all_tasks().filter(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Blocked | TaskStatus::Interrupted)).map(|task| task.id.clone()).collect();
    let completed = plan.all_tasks().filter(|task| task.status.is_success_terminal()).count();
    LeadFinalOutcome { plan_id: plan.id.clone(), state, summary: format!("{completed}/{} tasks completed; {} failed/blocked/interrupted", task_outcomes.len(), failed_or_blocked.len()), task_outcomes, failed_or_blocked }
}
pub async fn persist_plan_store_atomic(store: &PlanStore, path: &PathBuf) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    store.save_to_path(&tmp)?;
    std::fs::rename(&tmp, path).map_err(|e| format!("plan rename: {e}"))
}
pub async fn restore_workflow(path: &PathBuf) -> Result<(PlanStore, HashMap<TaskId, TaskStatus>), String> {
    let mut store = PlanStore::load_from_path(path)?;
    let mut states = HashMap::new();
    for id in store.list() {
        for task in store.get_mut(&id).unwrap().all_tasks_mut() {
            if matches!(task.status, TaskStatus::Running | TaskStatus::WaitingApproval) { task.status = TaskStatus::Interrupted; task.result = None; }
            else if task.status == TaskStatus::Assigned { task.status = TaskStatus::Queued; }
            states.insert(task.id.clone(), task.status);
        }
    }
    persist_plan_store_atomic(&store, path).await?;
    Ok((store, states))
}
pub(crate) fn now_unix_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{append_story, append_task, scaffold_plan, TaskPoint};

    fn sample_plan() -> Plan {
        // A -> (B, C) -> D
        let mut p = scaffold_plan("p", "M5.1 test", "A->B,C->D", "/tmp");
        let s = append_story(&mut p, "s", "d");
        let a = append_task(&mut p, &s, "A", "root", TaskPoint::Medium, vec![]);
        let b = append_task(
            &mut p,
            &s,
            "B",
            "branch b",
            TaskPoint::Small,
            vec![a.clone()],
        );
        let c = append_task(
            &mut p,
            &s,
            "C",
            "branch c",
            TaskPoint::Small,
            vec![a.clone()],
        );
        append_task(&mut p, &s, "D", "join", TaskPoint::Medium, vec![b.clone(), c.clone()]);
        p
    }

    #[tokio::test]
    async fn workflow_ready_blocked_dag_correctness() {
        let p = sample_plan();
        let mut ready = ready_tasks(&p);
        ready.sort();
        assert_eq!(ready, vec!["task-1"]);
        let mut store = PlanStore::new();
        store.insert(p.clone()).unwrap();
        let p_mut = store.get_mut("p").unwrap();
        p_mut.find_task_mut("task-1").unwrap().status = TaskStatus::Completed;
        let ready = ready_tasks(p_mut);
        let mut ready = ready.clone();
        ready.sort();
        // After A is completed, B and C are Ready.
        assert_eq!(ready, vec!["task-2", "task-3"]);
        // Mark C failed.
        p_mut.find_task_mut("task-3").unwrap().status = TaskStatus::Failed;
        let blocked = blocked_by_failed_deps(p_mut);
        assert_eq!(blocked, vec!["task-4"]);
    }

    #[tokio::test]
    async fn workflow_restore_keeps_terminal_states() {
        let mut p = sample_plan();
        let mut store = PlanStore::new();
        p.find_task_mut("task-1").unwrap().status = TaskStatus::Completed;
        p.find_task_mut("task-2").unwrap().status = TaskStatus::Running;
        store.insert(p.clone()).unwrap();
        let dir = std::env::temp_dir().join("m51-restore.json");
        persist_plan_store_atomic(&store, &dir).await.unwrap();
        let (restored, map) = restore_workflow(&dir).await.unwrap();
        let p2 = restored.get("p").unwrap();
        assert_eq!(p2.find_task("task-1").unwrap().status, TaskStatus::Completed);
        assert_eq!(p2.find_task("task-2").unwrap().status, TaskStatus::Interrupted);
        assert_eq!(map.get("task-1"), Some(&TaskStatus::Completed));
        assert_eq!(map.get("task-2"), Some(&TaskStatus::Interrupted));
        std::fs::remove_file(&dir).ok();
    }

    #[tokio::test]
    async fn workflow_atomic_persistence_recovers_from_partial_write() {
        let p = sample_plan();
        let mut store = PlanStore::new();
        store.insert(p.clone()).unwrap();
        let dir = std::env::temp_dir().join("m51-atomic.json");
        // Write once.
        persist_plan_store_atomic(&store, &dir).await.unwrap();
        // Mutate.
        store.get_mut("p").unwrap().find_task_mut("task-1").unwrap().status = TaskStatus::Failed;
        // Write again — overwrites atomically.
        persist_plan_store_atomic(&store, &dir).await.unwrap();
        let (restored, _) = restore_workflow(&dir).await.unwrap();
        assert_eq!(
            restored.get("p").unwrap().find_task("task-1").unwrap().status,
            TaskStatus::Failed
        );
        std::fs::remove_file(&dir).ok();
    }

    #[tokio::test]
    async fn workflow_no_repeat_completed_tasks() {
        let mut p = sample_plan();
        let mut store = PlanStore::new();
        p.find_task_mut("task-1").unwrap().status = TaskStatus::Completed;
        store.insert(p.clone()).unwrap();
        // ready_tasks must NOT include completed task.
        let ready = ready_tasks(&p);
        assert!(!ready.contains(&"task-1".to_string()));
        // Run twice with Completed state — no double-dispatch.
        for _ in 0..2 {
            let mut p2 = p.clone();
            let ready = ready_tasks(&p2);
            assert_eq!(ready.len(), 2); // B + C
            assert!(!ready.contains(&"task-1".to_string()));
            let _ = &mut p2;
        }
    }

    #[tokio::test]
    async fn workflow_persistence_failure_returns_error_not_silent() {
        // Try writing to a non-existent directory.
        let bad_path = std::path::PathBuf::from("/this/path/does/not/exist/m51-bad.json");
        let store = PlanStore::new();
        let res = persist_plan_store_atomic(&store, &bad_path).await;
        assert!(res.is_err(), "silent success would be a bug");
    }

    #[tokio::test]
    async fn lead_final_aggregates_per_task_handoff() {
        let p = sample_plan();
        let mut store = PlanStore::new();
        store.insert(p.clone()).unwrap();
        let plan = store.get("p").unwrap().clone();
        let mut outcomes = vec![];
        for id in ["task-2", "task-3"] {
            outcomes.push(TaskOutcome {
                task_id: id.to_string(),
                title: id.to_string(),
                staff_session_id: None,
                status: TaskStatus::Completed,
                handoff: HandoffPayload {
                    summary: format!("{id} done"),
                    artifacts: vec![],
                    diff_check_status: "ok".into(),
                    notes: vec![],
                },
                completed_at_ms: Some(1),
            });
        }
        let failed_or_blocked = vec!["task-4".to_string()];
        let summary = format!("2/4 tasks completed; {} failed/blocked", failed_or_blocked.len());
        let lead = LeadFinalOutcome {
            state: WorkflowState::FinishedWithFailure,
            plan_id: "p".into(),
            task_outcomes: outcomes.clone(),
            failed_or_blocked: failed_or_blocked.clone(),
            summary: summary.clone(),
        };
        assert_eq!(lead.task_outcomes.len(), 2);
        assert_eq!(lead.failed_or_blocked.len(), 1);
        assert_eq!(lead.summary, "2/4 tasks completed; 1 failed/blocked");
        let _ = plan;
    }
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Assigned => "assigned",
            Self::Running => "running",
            Self::WaitingApproval => "waiting_approval",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

