//! M5.3 product semantics through real AgentSession + stdio fixture, not a cloned worker.
use std::sync::Arc;
use std::path::PathBuf;
use ridge_acp::{AgentSession, AgentSessionConfig, ProviderSpec, SpawnSpec, spawn_acp_agent};
use ridge_acp::task::{scaffold_plan, append_story, append_task, PlanStore, TaskPoint, TaskStatus};
use ridge_acp::workflow::{WorkflowCoordinator, WorkflowEvent, WorkflowState, aggregate_plan, persist_plan_store_atomic, restore_workflow};
use tokio::sync::Mutex;

async fn session(approval: bool) -> Arc<AgentSession> {
    let mut provider = ProviderSpec::mock_default();
    provider.config.insert("program".into(), serde_json::json!(env!("CARGO_BIN_EXE_fake_acp_agent")));
    provider.config.insert("args".into(), serde_json::json!(if approval { vec![] } else { vec!["--no-approval"] }));
    let spec = SpawnSpec { program: env!("CARGO_BIN_EXE_fake_acp_agent").into(), args: if approval { vec![] } else { vec!["--no-approval".into()] }, cwd: None, provider: provider.clone(), framing: Default::default() };
    let (transport, child) = spawn_acp_agent(&spec).unwrap();
    Arc::new(AgentSession::connect(transport, child, AgentSessionConfig { provider, ..AgentSessionConfig::mock_default() }).await.unwrap())
}
fn plan() -> ridge_acp::Plan {
    let mut plan = scaffold_plan("m53", "Acceptance", "A -> (B,C) -> D", std::env::current_dir().unwrap().to_string_lossy().into_owned());
    let story = append_story(&mut plan, "Story", "Acceptance");
    let a = append_task(&mut plan, &story, "A", "root", TaskPoint::Small, vec![]);
    let b = append_task(&mut plan, &story, "B", "branch", TaskPoint::Medium, vec![a.clone()]);
    let c = append_task(&mut plan, &story, "C", "branch", TaskPoint::Small, vec![a]);
    append_task(&mut plan, &story, "D", "join", TaskPoint::Large, vec![b,c]);
    plan
}
fn path() -> PathBuf { std::env::temp_dir().join(format!("ridge-m53-{}.json", ridge_acp::SessionId::new().0)) }
#[tokio::test]
async fn communion_requires_confirm_before_run_and_review_is_canonical() {
    let lead = session(false).await;
    let mut store = PlanStore::new(); store.insert(plan()).unwrap();
    store.review_task("m53", "task-1", "Edited A".into(), TaskPoint::ExtraLarge, vec![], None).unwrap();
    assert_eq!(store.get("m53").unwrap().find_task("task-1").unwrap().title, "Edited A");
    assert!(store.review_task("m53", "task-1", "Cycle".into(), TaskPoint::Small, vec!["task-4".into()], None).is_err());
    let coordinator = WorkflowCoordinator::new("m53".into(), store, lead.clone());
    assert!(coordinator.run_plan().await.unwrap_err().contains("confirmation"));
    assert!(lead.snapshot().await.turns.is_empty());
    assert!(coordinator.current_plan().await.all_tasks().all(|task| task.status == TaskStatus::Queued));
    lead.shutdown().await;
}
#[tokio::test]
async fn live_approval_same_turn_blocks_dependency_until_real_completion() {
    let lead = session(false).await; let first = session(false).await; let second = session(true).await;
    let mut plan = plan();
    let first_id = first.id().await.0; let second_id = second.id().await.0;
    plan.staff.insert(first_id.clone(), first.provider().await); plan.staff.insert(second_id.clone(), second.provider().await);
    plan.find_task_mut("task-1").unwrap().assignee = Some(first_id.clone());
    plan.find_task_mut("task-2").unwrap().assignee = Some(second_id.clone());
    plan.find_task_mut("task-3").unwrap().assignee = Some(first_id);
    let mut store = PlanStore::new(); store.insert(plan).unwrap(); store.confirm("m53").unwrap();
    let file = path();
    let coordinator = Arc::new(WorkflowCoordinator::new_shared("m53".into(), Arc::new(Mutex::new(store)), lead.clone(), Some(file.clone())));
    coordinator.register_staff(first.clone()).await; coordinator.register_staff(second.clone()).await;
    let mut events = coordinator.events(); let runner = coordinator.clone();
    let execution = tokio::spawn(async move { runner.run_plan().await });
    let (turn, tool) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop { if let WorkflowEvent::AgentEvent { task_id, event: ridge_acp::AgentSessionEvent::ApprovalRequested { turn_id, tool_call_id, .. }, .. } = events.recv().await.unwrap() {
            assert_eq!(task_id, "task-2"); break (turn_id, tool_call_id);
        } }
    }).await.unwrap();
    let paused = coordinator.current_plan().await;
    assert_eq!(paused.find_task("task-2").unwrap().status, TaskStatus::WaitingApproval);
    assert_eq!(paused.find_task("task-4").unwrap().status, TaskStatus::Queued);
    assert_eq!(PlanStore::load_from_path(&file).unwrap().get("m53").unwrap().find_task("task-2").unwrap().status, TaskStatus::WaitingApproval);
    assert_eq!(second.snapshot().await.turns.len(), 1);
    coordinator.approve_task("task-2", &turn, &tool, true).await.unwrap();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), execution).await.unwrap().unwrap().unwrap();
    assert_eq!(outcome.state, WorkflowState::Completed);
    assert_eq!(outcome.task_outcomes.len(), 4);
    assert!(outcome.failed_or_blocked.is_empty());
    let staff_snapshot = second.snapshot().await;
    assert!(staff_snapshot.turns.contains_key(&turn));
    assert_eq!(staff_snapshot.turns.values().filter(|record| record.prompt.contains("branch")).count(), 1, "approval must not issue a second prompt");
    assert!(lead.snapshot().await.turns.values().any(|record| record.prompt.contains("task_outcomes") && record.prompt.contains("artifacts")));
    assert!(outcome.task_outcomes.iter().all(|task| !task.handoff.summary.is_empty()));
    lead.shutdown().await; first.shutdown().await; second.shutdown().await;
    std::fs::remove_file(file).unwrap();
}
#[tokio::test]
async fn waiting_approval_after_restart_requires_resume_and_never_repeats_completed() {
    let lead = session(false).await;
    let mut plan = plan(); plan.confirmed = true;
    plan.find_task_mut("task-1").unwrap().status = TaskStatus::Completed;
    plan.find_task_mut("task-2").unwrap().status = TaskStatus::Completed;
    plan.find_task_mut("task-3").unwrap().status = TaskStatus::WaitingApproval;
    plan.find_task_mut("task-3").unwrap().assignee = Some("fresh-staff".into());
    plan.find_task_mut("task-3").unwrap().assigned_session_id = Some("lost-session".into());
    plan.staff.insert("fresh-staff".into(), lead.provider().await);
    let file = path(); let mut store = PlanStore::new(); store.insert(plan).unwrap(); persist_plan_store_atomic(&store, &file).await.unwrap();
    let (restored, _) = restore_workflow(&file).await.unwrap();
    let current = restored.get("m53").unwrap();
    assert_eq!(current.find_task("task-3").unwrap().status, TaskStatus::Interrupted);
    let recoverable = aggregate_plan(current);
    assert_eq!(recoverable.state, WorkflowState::Recoverable);
    assert_eq!(recoverable.task_outcomes.len(), 4);
    assert!(recoverable.failed_or_blocked.contains(&"task-3".to_string()));
    let coordinator = WorkflowCoordinator::new_shared("m53".into(), Arc::new(Mutex::new(restored)), lead.clone(), Some(file.clone()));
    assert!(coordinator.approve_task("task-3", &ridge_acp::TurnId("lost-turn".into()), "tool", true).await.unwrap_err().contains("Resume"));
    assert_eq!(coordinator.run_plan().await.unwrap().state, WorkflowState::Recoverable);
    assert!(lead.snapshot().await.turns.is_empty());
    coordinator.resume_task("task-3").await.unwrap();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), coordinator.run_plan()).await.unwrap().unwrap();
    assert_eq!(outcome.state, WorkflowState::Completed);
    let current = coordinator.current_plan().await;
    assert_ne!(current.find_task("task-3").unwrap().assigned_session_id.as_deref(), Some("lost-session"));
    assert_eq!(current.find_task("task-1").unwrap().assigned_session_id, None);
    assert_eq!(current.find_task("task-2").unwrap().assigned_session_id, None);
    assert_eq!(current.find_task("task-3").unwrap().id, "task-3");
    lead.shutdown().await;
    std::fs::remove_file(file).unwrap();
}
#[test]
fn interrupted_waiting_approval_and_blocked_are_not_success() {
    for (status, expected) in [(TaskStatus::Interrupted, WorkflowState::Recoverable), (TaskStatus::WaitingApproval, WorkflowState::Active), (TaskStatus::Blocked, WorkflowState::Active)] {
        let mut p = plan(); p.find_task_mut("task-1").unwrap().status = status;
        assert_eq!(aggregate_plan(&p).state, expected);
        assert!(!status.is_success_terminal());
    }
    let mut p = plan(); for task in p.all_tasks_mut() { task.status = TaskStatus::Blocked; }
    assert_eq!(aggregate_plan(&p).state, WorkflowState::FinishedWithFailure);
}

#[tokio::test]
async fn cancel_waiting_task_stops_runtime_and_keeps_cancelled_truth() {
    let lead = session(false).await; let staff = session(true).await;
    let mut store = PlanStore::new(); store.insert(plan()).unwrap(); store.confirm("m53").unwrap();
    let coordinator = Arc::new(WorkflowCoordinator::new("m53".into(), store, lead.clone()));
    let mut events = coordinator.events(); let runner = coordinator.clone(); let executing = staff.clone();
    let execution = tokio::spawn(async move { runner.execute_task(&"task-1".into(), executing, "cancelled task").await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop { if matches!(events.recv().await.unwrap(), WorkflowEvent::AgentEvent { event: ridge_acp::AgentSessionEvent::ApprovalRequested { .. }, .. }) { break; } }
    }).await.unwrap();
    coordinator.cancel().await.unwrap();
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), execution).await.unwrap().unwrap().unwrap();
    assert_eq!(result.status, TaskStatus::Cancelled);
    assert_eq!(staff.status().await, ridge_acp::SessionStatus::Cancelled);
    assert_eq!(coordinator.current_plan().await.find_task("task-1").unwrap().status, TaskStatus::Cancelled);
    lead.shutdown().await;
}
