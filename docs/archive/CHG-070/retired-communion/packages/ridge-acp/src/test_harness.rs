//! Controlled events and Tokio's paused clock exercise the production Worker driver.
use super::*;
use crate::turn::{TurnPhase, TurnResult};
use tokio::sync::mpsc;

async fn start() -> (broadcast::Sender<AgentSessionEvent>, mpsc::UnboundedReceiver<String>, tokio::task::JoinHandle<WorkerOutcome>) {
    let (tx, rx) = broadcast::channel(32);
    let (ack, mut received) = mpsc::unbounded_channel();
    let events = tx.clone();
    let worker = tokio::spawn(async move {
        drive_task(SessionId("s".into()), async move {
            events.send(AgentSessionEvent::TurnStarted { session_id: SessionId("s".into()), turn_id: TurnId("t".into()), prompt: "once".into(), ts_unix_ms: 0 }).unwrap();
            std::future::pending::<Result<TurnId, AcpErrorWrap>>().await
        }, rx, ExecutionPolicy { per_task_timeout: Duration::from_secs(120), auto_approve: false }, move |ev| {
            ack.send(ev.kind_str().to_string()).unwrap();
            async { Ok(()) }
        }).await
    });
    assert_eq!(received.recv().await.as_deref(), Some("turn_started"));
    (tx, received, worker)
}
fn completed() -> AgentSessionEvent {
    AgentSessionEvent::TurnCompleted { session_id: SessionId("s".into()), turn_id: TurnId("t".into()), result: TurnResult { phase: TurnPhase::Completed, message: "done".into() }, ts_unix_ms: 40 }
}
#[tokio::test(start_paused = true)]
async fn running_without_terminal_event_stays_running_past_old_boundary() {
    let (tx, _ack, worker) = start().await;
    tokio::time::advance(Duration::from_secs(31)).await;
    tokio::task::yield_now().await;
    assert!(!worker.is_finished());
    tx.send(completed()).unwrap();
    assert_eq!(worker.await.unwrap().status, TaskStatus::Completed);
}
#[tokio::test(start_paused = true)]
async fn terminal_event_before_policy_strictly_completes() {
    let (tx, _ack, worker) = start().await;
    tx.send(completed()).unwrap();
    let outcome = worker.await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed);
    assert_eq!(outcome.handoff.summary, "done");
    assert_eq!(outcome.turn_id, TurnId("t".into()));
}
#[tokio::test(start_paused = true)]
async fn policy_before_terminal_strictly_fails_timeout() {
    let (_tx, _ack, worker) = start().await;
    tokio::time::advance(Duration::from_secs(121)).await;
    let outcome = worker.await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Failed);
    assert_eq!(outcome.handoff.diff_check_status, "timeout");
    assert!(outcome.handoff.summary.contains("timeout"));
}
#[tokio::test(start_paused = true)]
async fn waiting_approval_live_continues_same_turn() {
    let (tx, mut ack, worker) = start().await;
    tx.send(AgentSessionEvent::ApprovalRequested { session_id: SessionId("s".into()), turn_id: TurnId("t".into()), tool_call_id: "tool".into(), reason: "write".into(), ts_unix_ms: 1 }).unwrap();
    assert_eq!(ack.recv().await.as_deref(), Some("approval_requested"));
    assert!(!worker.is_finished());
    tx.send(AgentSessionEvent::ApprovalResolved { session_id: SessionId("s".into()), turn_id: TurnId("t".into()), tool_call_id: "tool".into(), approved: true, ts_unix_ms: 2 }).unwrap();
    assert_eq!(ack.recv().await.as_deref(), Some("approval_resolved"));
    tx.send(completed()).unwrap();
    let outcome = worker.await.unwrap();
    assert_eq!(outcome.status, TaskStatus::Completed);
    assert!(outcome.approved);
    assert_eq!(outcome.turn_id, TurnId("t".into()));
    assert_eq!(ack.recv().await.as_deref(), Some("turn_completed"));
    assert!(ack.try_recv().is_err(), "a second prompt must never occur");
}
#[tokio::test(start_paused = true)]
async fn unrelated_turn_cannot_complete_worker() {
    let (tx, mut ack, worker) = start().await;
    let mut unrelated = completed();
    if let AgentSessionEvent::TurnCompleted { turn_id, .. } = &mut unrelated { *turn_id = TurnId("other".into()); }
    tx.send(unrelated).unwrap();
    tx.send(AgentSessionEvent::MessageChunk { session_id: SessionId("s".into()), turn_id: TurnId("t".into()), text: "still active".into(), ts_unix_ms: 1 }).unwrap();
    assert_eq!(ack.recv().await.as_deref(), Some("message_chunk"));
    assert!(!worker.is_finished());
    tx.send(completed()).unwrap();
    assert_eq!(worker.await.unwrap().status, TaskStatus::Completed);
}
#[test]
fn interrupted_not_success_or_terminal() {
    assert!(!TaskStatus::Interrupted.is_terminal());
    assert!(!TaskStatus::Interrupted.is_success_terminal());
    assert!(TaskStatus::Interrupted.is_recoverable());
    assert!(TaskStatus::Interrupted.is_execution_stopped());
    assert!(!TaskStatus::WaitingApproval.is_execution_stopped());
    assert!(!TaskStatus::Blocked.is_success_terminal());
}
