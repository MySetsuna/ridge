//! E2E-B: Commutation coordinator — 1 Lead + 2 Staff assigned to one task.
//!
//! Verifies:
//! - Commutation assigns both staff
//! - each staff turn runs through the canonical event sequence
//! - staff progress / staff completed events fire
//! - handoff event fires between staff
//! - commutation reaches `Completed`

mod common;

use std::time::Duration;

use tokio::sync::broadcast;

use ridge_acp::{
    AgentSessionEvent, Commutation, CommutationEvent, StaffSpec,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_commutation_one_lead_two_staff() {
    let lead = common::spawn_fake_session().await.session;
    let staff_a = common::spawn_fake_session().await.session;
    let staff_b = common::spawn_fake_session().await.session;

    let staff_a_id = staff_a.id().await;
    let staff_b_id = staff_b.id().await;

    let mut staff_a_rx = staff_a.subscribe();
    let mut staff_b_rx = staff_b.subscribe();

    // Each staff session will request approval once. Approve on their behalf
    // so the turn can complete.
    let approver_a = {
        let s = staff_a.clone();
        tokio::spawn(async move {
            loop {
                match staff_a_rx.recv().await {
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
    let approver_b = {
        let s = staff_b.clone();
        tokio::spawn(async move {
            loop {
                match staff_b_rx.recv().await {
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

    let comm = Commutation::new(&lead, "split into two sub-tasks", "explore /tmp/example.txt").await;
    let mut comm_rx: broadcast::Receiver<CommutationEvent> = comm.events();

    let spec_a = StaffSpec {
        session_id: staff_a_id.clone(),
        role: "explorer".into(),
        provider_label: "mock/claude-haiku".into(),
    };
    let spec_b = StaffSpec {
        session_id: staff_b_id.clone(),
        role: "summarizer".into(),
        provider_label: "mock/claude-haiku".into(),
    };

    let pool = vec![(spec_a.clone(), staff_a.clone()), (spec_b.clone(), staff_b.clone())];

    let lead_clone = lead.clone();
    let mut lead_rx = lead.subscribe();

    // We don't actually need to drive the Lead's turn for the commutation to
    // observe staff progress. The Lead session remains idle; Communion only
    // needs it for the snapshot summary.
    let _ = lead_clone;

    let runner = tokio::spawn(async move {
        comm.run(pool).await;
    });

    let mut saw_assigned_a = false;
    let mut saw_assigned_b = false;
    let mut saw_progress_a = false;
    let mut saw_progress_b = false;
    let mut saw_completed_a = false;
    let mut saw_completed_b = false;
    let mut saw_handoff = false;
    let mut saw_comm_completed = false;

    let drain = async {
        while let Ok(ev) = comm_rx.recv().await {
            match &ev {
                CommutationEvent::StaffAssigned { staff, .. } => {
                    if staff == &staff_a_id {
                        saw_assigned_a = true;
                    }
                    if staff == &staff_b_id {
                        saw_assigned_b = true;
                    }
                }
                CommutationEvent::StaffProgress { staff, .. } => {
                    if staff == &staff_a_id {
                        saw_progress_a = true;
                    }
                    if staff == &staff_b_id {
                        saw_progress_b = true;
                    }
                }
                CommutationEvent::StaffCompleted { staff, .. } => {
                    if staff == &staff_a_id {
                        saw_completed_a = true;
                    }
                    if staff == &staff_b_id {
                        saw_completed_b = true;
                    }
                }
                CommutationEvent::Handoff { .. } => saw_handoff = true,
                CommutationEvent::CommutationCompleted { .. } => {
                    saw_comm_completed = true;
                    break;
                }
                _ => {}
            }
        }
    };

    let _ = tokio::time::timeout(Duration::from_secs(10), drain).await;
    let _ = runner.await;
    let _ = approver_a.await;
    let _ = approver_b.await;

    // Drain the lead's broadcast (we subscribed but didn't use it) to keep
    // lag from accumulating during the assertion.
    while lead_rx.try_recv().is_ok() {}

    assert!(saw_assigned_a, "missing StaffAssigned for staff_a");
    assert!(saw_assigned_b, "missing StaffAssigned for staff_b");
    assert!(saw_progress_a, "missing StaffProgress for staff_a");
    assert!(saw_progress_b, "missing StaffProgress for staff_b");
    assert!(saw_completed_a, "missing StaffCompleted for staff_a");
    assert!(saw_completed_b, "missing StaffCompleted for staff_b");
    assert!(saw_handoff, "missing Handoff event between staff");
    assert!(saw_comm_completed, "missing CommutationCompleted");
}
