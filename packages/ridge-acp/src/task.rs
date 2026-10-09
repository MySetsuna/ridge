//! M5 — Plan / Story / Task model + persistence.
//!
//! Single source of truth for task state. Communion holds a `PlanId`
//! reference for coordination; the `PlanStore` holds the actual graph.
//! No UI / Communion / AgentSession stores its own task state — they
//! all read from / write to `PlanStore`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Fibonacci-ish point sizing used by the user-facing Plan editor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskPoint {
    /// 1 — trivial.
    Small,
    /// 2 — focused.
    Medium,
    /// 3 — multi-step.
    Large,
    /// 5 — spans sessions / requires handoff.
    ExtraLarge,
}

impl TaskPoint {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::ExtraLarge => "extra_large",
        }
    }
}

/// Status flow for one task. `Queued → Assigned → Running → WaitingApproval
/// → Completed | Failed | Cancelled`. `Blocked` is set when a dependency
/// fails; downstream tasks also become Blocked until dependency is retried.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Assigned,
    Running,
    WaitingApproval,
    Blocked,
    Completed,
    Failed,
    Cancelled,
    /// M5 §6: a task that was running when the host crashed/shut down.
    /// On workflow restore the user is asked whether to retry or abandon.
    Interrupted,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool { matches!(self, Self::Completed | Self::Failed | Self::Cancelled) }
    pub fn is_success_terminal(self) -> bool { self == Self::Completed }
    pub fn is_recoverable(self) -> bool { self == Self::Interrupted }
    pub fn is_execution_stopped(self) -> bool { self.is_terminal() || matches!(self, Self::Interrupted | Self::Blocked) }
}

pub type TaskId = String;
pub type StoryId = String;
pub type PlanId = String;

/// Workspace context reference — NOT the full workspace. The prompt
/// receives this as a citation; the actual file content is fetched on
/// demand by the Staff agent when it executes the task.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceRef {
    pub workspace_id: String,
    pub cwd: String,
    pub relevant_files: Vec<String>,
    pub git_state: Option<String>,
}

/// Structured handoff payload — what a Staff agent passes to the next
/// (or to Lead at the end). Aggregated by Communion into `LeadFinal`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HandoffPayload {
    pub summary: String,
    pub artifacts: Vec<String>,
    pub diff_check_status: String,
    pub notes: Vec<String>,
}

/// Result of one task. Persisted with the task so restore shows the
/// historical state without re-running.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskResult {
    pub status: TaskStatus,
    pub completed_at_ms: Option<u64>,
    pub handoff: HandoffPayload,
}

/// One task. Edits are allowed BEFORE Assigned (Lead planning). Once
/// Running, edits require an explicit pause + reassign.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub description: String,
    pub point: TaskPoint,
    pub dependencies: Vec<TaskId>,
    #[serde(default)]
    pub assignee: Option<String>,
    pub assigned_session_id: Option<String>,
    pub status: TaskStatus,
    pub result: Option<TaskResult>,
}

/// One Story groups related tasks. Stories don't have status — they're
/// containers for task DAGs that share a deliverable.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Story {
    pub id: StoryId,
    pub title: String,
    pub description: String,
    pub tasks: Vec<Task>,
}

/// Top-level Plan. Persisted to disk via `PlanStore`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Plan {
    #[serde(default)]
    pub lead_provider: Option<crate::provider::ProviderSpec>,
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub staff: HashMap<String, crate::provider::ProviderSpec>,
    pub id: PlanId,
    pub title: String,
    pub context: String,
    pub workspace: WorkspaceRef,
    pub stories: Vec<Story>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl Plan {
    /// Iterate every task in the plan (across all stories).
    pub fn all_tasks(&self) -> impl Iterator<Item = &Task> {
        self.stories.iter().flat_map(|s| s.tasks.iter())
    }

    /// Iterate mutable every task.
    pub fn all_tasks_mut(&mut self) -> impl Iterator<Item = &mut Task> {
        self.stories.iter_mut().flat_map(|s| s.tasks.iter_mut())
    }

    /// Find a task by id across all stories.
    pub fn find_task(&self, id: &str) -> Option<&Task> {
        self.all_tasks().find(|t| t.id == id)
    }

    pub fn find_task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.all_tasks_mut().find(|t| t.id == id)
    }
}

/// Validate the DAG: each dependency must reference an existing task,
/// and there must be no cycles.
pub fn validate_dag(plan: &Plan) -> Result<(), String> {
    let task_ids: std::collections::HashSet<&str> =
        plan.all_tasks().map(|t| t.id.as_str()).collect();
    if task_ids.len() != plan.all_tasks().count() { return Err("duplicate task id".into()); }
    for t in plan.all_tasks() {
        for dep in &t.dependencies {
            if !task_ids.contains(dep.as_str()) {
                return Err(format!(
                    "task {} depends on missing task {}",
                    t.id, dep
                ));
            }
        }
    }
    // Cycle detection via DFS coloring.
    enum Color { White, Gray, Black }
    let mut color: HashMap<&str, Color> = HashMap::new();
    for t in plan.all_tasks() {
        color.insert(t.id.as_str(), Color::White);
    }
    fn dfs<'a>(
        node: &'a str,
        color: &mut HashMap<&'a str, Color>,
        plan: &'a Plan,
    ) -> Result<(), String> {
        color.insert(node, Color::Gray);
        if let Some(t) = plan.find_task(node) {
            for dep in &t.dependencies {
                match color.get(dep.as_str()) {
                    Some(Color::White) => dfs(dep, color, plan)?,
                    Some(Color::Gray) => {
                        return Err(format!("cycle: {node} -> {dep}"));
                    }
                    Some(Color::Black) | None => {}
                }
            }
        }
        color.insert(node, Color::Black);
        Ok(())
    }
    for t in plan.all_tasks() {
        if matches!(color.get(t.id.as_str()), Some(Color::White)) {
            dfs(t.id.as_str(), &mut color, plan)?;
        }
    }
    Ok(())
}

/// Compute the set of tasks that are Ready to start (status == Queued,
/// all dependencies in Completed). Edges depend on the task graph; this
/// function returns a snapshot.
pub fn ready_tasks(plan: &Plan) -> Vec<TaskId> {
    plan.all_tasks()
        .filter(|t| matches!(t.status, TaskStatus::Queued))
        .filter(|t| {
            t.dependencies.iter().all(|dep| {
                plan.find_task(dep)
                    .map(|d| matches!(d.status, TaskStatus::Completed))
                    .unwrap_or(false)
            })
        })
        .map(|t| t.id.clone())
        .collect()
}

/// Tasks whose dependencies are not all completed AND at least one
/// dependency is in a non-terminal state. Used to mark downstream as
/// Blocked when a dependency fails.
pub fn blocked_by_failed_deps(plan: &Plan) -> Vec<TaskId> {
    plan.all_tasks()
        .filter(|t| matches!(t.status, TaskStatus::Queued | TaskStatus::Assigned))
        .filter(|t| {
            t.dependencies.iter().any(|dep| {
                plan.find_task(dep)
                    .map(|d| matches!(d.status, TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Blocked))
                    .unwrap_or(false)
            })
        })
        .map(|t| t.id.clone())
        .collect()
}

/// Single source of truth for plan storage. Plans are persisted to a
/// JSON file; restore loads from the same file.
pub struct PlanStore {
    plans: HashMap<PlanId, Plan>,
}

impl PlanStore {
    pub fn new() -> Self {
        Self {
            plans: HashMap::new(),
        }
    }

    pub fn insert(&mut self, plan: Plan) -> Result<(), String> {
        validate_dag(&plan)?;
        self.plans.insert(plan.id.clone(), plan);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Plan> {
        self.plans.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Plan> {
        self.plans.get_mut(id)
    }

    pub fn review_task(&mut self, plan_id: &str, task_id: &str, title: String, point: TaskPoint, dependencies: Vec<TaskId>, assignee: Option<String>) -> Result<(), String> {
        let mut plan = self.get(plan_id).ok_or("plan not found")?.clone();
        if plan.confirmed || plan.all_tasks().any(|task| task.status != TaskStatus::Queued) { return Err("plan is no longer in review".into()); }
        if title.trim().is_empty() { return Err("task title is required".into()); }
        if let Some(assignee) = &assignee { if !plan.staff.contains_key(assignee) { return Err("unknown assignee".into()); } }
        let task = plan.find_task_mut(task_id).ok_or("task not found")?;
        task.title = title; task.point = point; task.dependencies = dependencies; task.assignee = assignee;
        self.insert(plan)
    }
    pub fn confirm(&mut self, plan_id: &str) -> Result<(), String> {
        let plan = self.get(plan_id).ok_or("plan not found")?;
        if plan.confirmed { return Err("plan already confirmed".into()); }
        if plan.all_tasks().next().is_none() { return Err("plan has no tasks".into()); }
        if plan.all_tasks().any(|task| task.status != TaskStatus::Queued) { return Err("only a pending review can be confirmed".into()); }
        validate_dag(plan)?;
        self.get_mut(plan_id).unwrap().confirmed = true;
        Ok(())
    }
    pub fn remove(&mut self, id: &PlanId) -> Option<Plan> {
        self.plans.remove(id)
    }

    pub fn list(&self) -> Vec<PlanId> {
        let mut ids: Vec<_> = self.plans.keys().cloned().collect();
        ids.sort();
        ids
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.plans)
            .map_err(|e| format!("plan serialize: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("plan write: {e}"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("plan read: {e}"))?;
        let plans: HashMap<PlanId, Plan> = serde_json::from_str(&raw)
            .map_err(|e| format!("plan parse: {e}"))?;
        Ok(Self { plans })
    }
}

impl Default for PlanStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> Plan {
        let now = 0u64;
        Plan {
            lead_provider: None,
            confirmed: false,
            staff: HashMap::new(),
            id: "plan-1".into(),
            title: "M5 sample".into(),
            context: "implement M5 task model".into(),
            workspace: WorkspaceRef {
                workspace_id: "ws-1".into(),
                cwd: "/tmp".into(),
                relevant_files: vec!["task.rs".into()],
                git_state: None,
            },
            stories: vec![Story {
                id: "story-1".into(),
                title: "Implementation".into(),
                description: "do the thing".into(),
                tasks: vec![
                    Task {
                        id: "t-1".into(),
                        title: "root".into(),
                        description: "first".into(),
                        point: TaskPoint::Small,
                        dependencies: vec![],
                        assignee: None,
                        assigned_session_id: None,
                        status: TaskStatus::Queued,
                        result: None,
                    },
                    Task {
                        id: "t-2".into(),
                        title: "leaf".into(),
                        description: "second".into(),
                        point: TaskPoint::Medium,
                        dependencies: vec!["t-1".into()],
                        assignee: None,
                        assigned_session_id: None,
                        status: TaskStatus::Queued,
                        result: None,
                    },
                ],
            }],
            created_at_ms: now,
            updated_at_ms: now,
        }
    }

    #[test]
    fn plan_validate_dag_accepts_linear_chain() {
        let p = sample_plan();
        assert!(validate_dag(&p).is_ok());
    }

    #[test]
    fn plan_validate_dag_rejects_missing_dependency() {
        let mut p = sample_plan();
        p.find_task_mut("t-2").unwrap().dependencies.push("t-missing".into());
        assert!(validate_dag(&p).is_err());
    }

    #[test]
    fn plan_validate_dag_rejects_cycle() {
        let mut p = sample_plan();
        p.find_task_mut("t-1").unwrap().dependencies.push("t-2".into());
        assert!(validate_dag(&p).is_err());
    }

    #[test]
    fn ready_tasks_filters_by_completed_deps() {
        let mut p = sample_plan();
        // mark t-1 completed; t-2 becomes Ready
        p.find_task_mut("t-1").unwrap().status = TaskStatus::Completed;
        let ready = ready_tasks(&p);
        assert_eq!(ready, vec!["t-2".to_string()]);
        // now mark t-1 as Failed — t-2 is no longer Ready
        p.find_task_mut("t-1").unwrap().status = TaskStatus::Failed;
        let ready = ready_tasks(&p);
        assert!(ready.is_empty());
        // now mark t-1 as Running — t-2 still not Ready (deps must be terminal)
        p.find_task_mut("t-1").unwrap().status = TaskStatus::Running;
        let ready = ready_tasks(&p);
        assert!(ready.is_empty());
    }

    #[test]
    fn blocked_by_failed_deps_returns_correct_tasks() {
        let mut p = sample_plan();
        p.find_task_mut("t-1").unwrap().status = TaskStatus::Failed;
        let blocked = blocked_by_failed_deps(&p);
        assert_eq!(blocked, vec!["t-2".to_string()]);
    }

    #[test]
    fn plan_store_save_load_roundtrip() {
        let p = sample_plan();
        let mut store = PlanStore::new();
        store.insert(p.clone()).unwrap();
        let tmp = std::env::temp_dir().join("plan-m5-test.json");
        store.save_to_path(&tmp).unwrap();
        let restored = PlanStore::load_from_path(&tmp).unwrap();
        assert_eq!(restored.get(&p.id), Some(&p));
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn plan_status_terminal_predicate() {
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Cancelled.is_terminal());
        assert!(!TaskStatus::Interrupted.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Queued.is_terminal());
        assert!(!TaskStatus::WaitingApproval.is_terminal());
    }

    #[test]
    fn plan_all_tasks_iterates_across_stories() {
        let p = sample_plan();
        let tasks: Vec<_> = p.all_tasks().collect();
        assert_eq!(tasks.len(), 2);
        let ids: Vec<_> = tasks.iter().map(|t| t.id.clone()).collect();
        assert!(ids.contains(&"t-1".to_string()));
        assert!(ids.contains(&"t-2".to_string()));
    }

    #[test]
    fn plan_store_insert_rejects_invalid_dag() {
        let mut p = sample_plan();
        p.find_task_mut("t-2").unwrap().dependencies.push("t-bogus".into());
        let mut store = PlanStore::new();
        assert!(store.insert(p).is_err());
    }

    #[test]
    fn task_point_as_str() {
        assert_eq!(TaskPoint::Small.as_str(), "small");
        assert_eq!(TaskPoint::Medium.as_str(), "medium");
        assert_eq!(TaskPoint::Large.as_str(), "large");
        assert_eq!(TaskPoint::ExtraLarge.as_str(), "extra_large");
    }
}

/// Builder helpers for an M5 Plan. Lead can use these to scaffold a Plan
/// before letting the user edit (M5 §2 user edit step).
pub fn scaffold_plan(
    id: impl Into<PlanId>,
    title: impl Into<String>,
    context: impl Into<String>,
    workspace_cwd: impl Into<String>,
) -> Plan {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    Plan {
        lead_provider: None,
        confirmed: false,
        staff: HashMap::new(),
        id: id.into(),
        title: title.into(),
        context: context.into(),
        workspace: WorkspaceRef {
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

/// Append a Story to a Plan. Returns the new Story id.
pub fn append_story(
    plan: &mut Plan,
    title: impl Into<String>,
    description: impl Into<String>,
) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let story_id = format!("story-{}", plan.stories.len() + 1);
    plan.stories.push(Story {
        id: story_id.clone(),
        title: title.into(),
        description: description.into(),
        tasks: vec![],
    });
    plan.updated_at_ms = now;
    story_id
}

/// Append a Task to the first story whose id matches, otherwise to the
/// last story. Returns the new Task id.
pub fn append_task(
    plan: &mut Plan,
    story_id: &str,
    title: impl Into<String>,
    description: impl Into<String>,
    point: TaskPoint,
    dependencies: Vec<TaskId>,
) -> TaskId {
    let task_num = plan.all_tasks().count() + 1;
    let task_id = format!("task-{task_num}");
    let task = Task {
        id: task_id.clone(),
        title: title.into(),
        description: description.into(),
        point,
        dependencies,
        assignee: None,
                        assigned_session_id: None,
        status: TaskStatus::Queued,
        result: None,
    };
    let story_idx = plan.stories.iter().position(|s| s.id == story_id);
    match story_idx {
        Some(i) => plan.stories[i].tasks.push(task),
        None => {
            if let Some(last) = plan.stories.last_mut() {
                last.tasks.push(task);
            } else {
                plan.stories.push(Story {
                    id: story_id.to_string(),
                    title: story_id.to_string(),
                    description: String::new(),
                    tasks: vec![task],
                });
            }
        }
    }
    plan.updated_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    task_id
}

#[cfg(test)]
mod scaffold_tests {
    use super::*;

    #[test]
    fn scaffold_plan_is_empty_dag() {
        let p = scaffold_plan("plan-1", "M5 test", "ctx", "/tmp");
        assert_eq!(p.id, "plan-1");
        assert_eq!(p.title, "M5 test");
        assert!(p.stories.is_empty());
        assert!(validate_dag(&p).is_ok());
    }

    #[test]
    fn append_story_grows_plan() {
        let mut p = scaffold_plan("p", "t", "c", "/");
        let sid = append_story(&mut p, "S", "d");
        assert_eq!(sid, "story-1");
        assert_eq!(p.stories.len(), 1);
        assert!(validate_dag(&p).is_ok());
    }

    #[test]
    fn append_task_assigns_sequential_ids() {
        let mut p = scaffold_plan("p", "t", "c", "/");
        let s = append_story(&mut p, "S", "d");
        let t1 = append_task(&mut p, &s, "a", "x", TaskPoint::Small, vec![]);
        let t2 = append_task(&mut p, &s, "b", "y", TaskPoint::Medium, vec![t1.clone()]);
        assert_eq!(t1, "task-1");
        assert_eq!(t2, "task-2");
        assert!(validate_dag(&p).is_ok());
    }

    #[test]
    fn append_task_targets_named_story() {
        let mut p = scaffold_plan("p", "t", "c", "/");
        let s1 = append_story(&mut p, "S1", "d");
        let s2 = append_story(&mut p, "S2", "d");
        let t = append_task(&mut p, &s1, "x", "y", TaskPoint::Small, vec![]);
        assert_eq!(p.stories[0].id, s1);
        assert_eq!(p.stories[1].id, s2);
        assert_eq!(p.stories[0].tasks[0].id, t);
    }
}
