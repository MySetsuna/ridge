//! M5 — Lead + 2 Staff Plan execution E2E (simplified).
//!
//! Builds a Plan with 3 tasks: 1 root + 2 parallel leaves (depend on root).
//! Spawns 1 Lead + 2 Staff AgentSessions. Uses Commutation::new_with_plan +
//! PlanStore + the dependency scheduler (ready_tasks / blocked_by_failed_deps).
//! Asserts:
//! - PlanStore is the single source of truth for Task state
//! - Commutation holds PlanId reference only (no Task duplication)
//! - ready_tasks returns [root] initially; both leaves blocked
//! - after root marked Completed, ready_tasks returns both leaves
//! - blocked_by_failed_deps returns [] if root Failed

use std::process::Stdio;
use std::sync::Arc;

use tokio::process::Command;

use ridge_acp::communion::{Commutation, StaffSpec};
use ridge_acp::session::{AgentSession, AgentSessionConfig};
use ridge_acp::task::{
    append_story, append_task, scaffold_plan, PlanStore, TaskPoint, TaskStatus,
};

async fn spawn_one() -> Arc<AgentSession> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fake_acp_agent"));
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let _stderr = child.stderr.take();
    let transport = ridge_acp::transport::AcpTransport::spawn(stdin, stdout, _stderr);
    Arc::new(
        AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
            .await
            .expect("connect"),
    )
}

#[tokio::test]
async fn lead_2_staff_plan_executes_dag() {
    let lead = spawn_one().await;
    let staff1 = spawn_one().await;
    let staff2 = spawn_one().await;

    // M5 §1: Plan is the single source of truth.
    let mut plan = scaffold_plan("p-1", "M5 e2e", "Lead + 2 Staff flow", "/tmp");
    let story = append_story(&mut plan, "story-1", "do the thing");
    let root = append_task(
        &mut plan,
        &story,
        "root",
        "first step",
        TaskPoint::Medium,
        vec![],
    );
    let leaf_a = append_task(
        &mut plan,
        &story,
        "leaf-a",
        "branch a",
        TaskPoint::Small,
        vec![root.clone()],
    );
    let leaf_b = append_task(
        &mut plan,
        &story,
        "leaf-b",
        "branch b",
        TaskPoint::Small,
        vec![root.clone()],
    );

    let mut store = PlanStore::new();
    store.insert(plan.clone()).unwrap();

    // M5 §4: build Commutation with plan reference (not a copy).
    let comm = Commutation::new_with_plan(
        &lead,
        plan.title.clone(),
        plan.context.clone(),
        Some("p-1".into()),
    )
    .await;
    assert_eq!(comm.plan_id(), Some(&"p-1".to_string()));

    // Initially: only root is Ready (leaves blocked on root).
    let ready = comm.next_ready_tasks(&plan).await;
    assert_eq!(ready, vec![root.clone()]);

    // Mark root Completed via the store; ready grows to both leaves.
    {
        let mut p = store.get_mut("p-1").unwrap();
        p.find_task_mut(&root).unwrap().status = TaskStatus::Completed;
    }
    let plan2 = store.get("p-1").unwrap().clone();
    let ready = comm.next_ready_tasks(&plan2).await;
    assert_eq!(ready.len(), 2);
    assert!(ready.contains(&leaf_a));
    assert!(ready.contains(&leaf_b));

    // M5 §4: lead assigns each leaf to its own staff session.
    comm.assign_staff_for_task(
        leaf_a.clone(),
        StaffSpec {
            session_id: staff1.id().await,
            role: "planner".into(),
            provider_label: "mock".into(),
        },
        &staff1,
    )
    .await;
    comm.assign_staff_for_task(
        leaf_b.clone(),
        StaffSpec {
            session_id: staff2.id().await,
            role: "executor".into(),
            provider_label: "mock".into(),
        },
        &staff2,
    )
    .await;

    let in_flight = comm.in_flight_snapshot().await;
    assert_eq!(in_flight.len(), 2);
    let in_flight_ids: Vec<_> = in_flight.iter().map(|(tid, _)| tid.clone()).collect();
    assert!(in_flight_ids.contains(&leaf_a));
    assert!(in_flight_ids.contains(&leaf_b));

    // M5 §4: Staff completes → mark Completed in PlanStore, handoff carries summary.
    let handoff = ridge_acp::task::HandoffPayload {
        summary: "leaf-a done".into(),
        artifacts: vec!["file_a.ts".into()],
        diff_check_status: "ok".into(),
        notes: vec![],
    };
    comm.complete_task(&leaf_a, handoff.clone(), 1_700_000_000_000).await;
    {
        let mut p = store.get_mut("p-1").unwrap();
        p.find_task_mut(&leaf_a).unwrap().status = TaskStatus::Completed;
    }
    {
        let p = store.get("p-1").unwrap();
        assert!(matches!(
            p.find_task(&leaf_a).unwrap().status,
            TaskStatus::Completed
        ));
    }

    // After both leaves Completed, no more Ready tasks.
    let handoff2 = ridge_acp::task::HandoffPayload {
        summary: "leaf-b done".into(),
        artifacts: vec!["file_b.ts".into()],
        diff_check_status: "ok".into(),
        notes: vec![],
    };
    comm.complete_task(&leaf_b, handoff2, 1_700_000_001_000).await;
    {
        let mut p = store.get_mut("p-1").unwrap();
        p.find_task_mut(&leaf_b).unwrap().status = TaskStatus::Completed;
    }
    let plan3 = store.get("p-1").unwrap().clone();
    let ready = comm.next_ready_tasks(&plan3).await;
    assert!(ready.is_empty(), "no more tasks once both leaves done");

    // M5 §6: persistence round-trip.
    let tmp = std::env::temp_dir().join("plan-m5-lead-2-staff.json");
    store.save_to_path(&tmp).unwrap();
    let restored = PlanStore::load_from_path(&tmp).unwrap();
    assert_eq!(
        restored.get("p-1").unwrap().find_task(&leaf_a).unwrap().status,
        TaskStatus::Completed
    );
    assert_eq!(
        restored.get("p-1").unwrap().find_task(&leaf_b).unwrap().status,
        TaskStatus::Completed
    );
    std::fs::remove_file(&tmp).ok();
}

#[tokio::test]
async fn dep_failure_marks_downstream_blocked() {
    let lead = spawn_one().await;
    let mut plan = scaffold_plan("p-2", "blocked test", "fail then block", "/tmp");
    let story = append_story(&mut plan, "s", "d");
    let root = append_task(&mut plan, &story, "root", "r", TaskPoint::Small, vec![]);
    let dep = append_task(&mut plan, &story, "dep", "d", TaskPoint::Small, vec![]);
    {
        let mut p = plan.clone();
        p.find_task_mut(&dep).unwrap().dependencies = vec![root.clone()];
        plan = p;
    }
    let mut store = PlanStore::new();
    store.insert(plan.clone()).unwrap();
    let comm = Commutation::new_with_plan(
        &lead,
        plan.title.clone(),
        plan.context.clone(),
        Some("p-2".into()),
    )
    .await;

    store.get_mut("p-2").unwrap().find_task_mut(&root).unwrap().status = TaskStatus::Failed;
    let plan2 = store.get("p-2").unwrap().clone();
    let blocked = comm.next_blocked_tasks(&plan2).await;
    assert!(blocked.contains(&dep));
    let ready = comm.next_ready_tasks(&plan2).await;
    assert!(!ready.contains(&dep), "dep with Failed ancestor is not Ready");
}

#[tokio::test]
async fn user_can_edit_plan_before_assigned() {
    let lead = spawn_one().await;
    let mut plan = scaffold_plan("p-3", "editable", "user edits", "/tmp");
    let story = append_story(&mut plan, "s", "d");
    let t1 = append_task(&mut plan, &story, "draft title", "draft desc", TaskPoint::Small, vec![]);
    {
        let mut p = plan.clone();
        p.find_task_mut(&t1).unwrap().title = "final title".into();
        p.find_task_mut(&t1).unwrap().point = TaskPoint::Large;
        p.find_task_mut(&t1).unwrap().dependencies = vec![];
        plan = p;
    }

    let mut store = PlanStore::new();
    store.insert(plan.clone()).unwrap();
    let _ = Commutation::new_with_plan(
        &lead,
        plan.title.clone(),
        plan.context.clone(),
        Some("p-3".into()),
    )
    .await;

    let stored = store.get("p-3").unwrap();
    assert_eq!(stored.find_task(&t1).unwrap().title, "final title");
    assert!(matches!(stored.find_task(&t1).unwrap().point, TaskPoint::Large));
}
