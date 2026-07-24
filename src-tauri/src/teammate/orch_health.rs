//! R17-TEAM-HEALTH: orchestration health snapshot for team collaboration UX.

use serde_json::{json, Value};

/// Snapshot of process-local teammate health counters (no I/O).
pub fn orchestration_health() -> Value {
    let suspended = super::suspend::suspended_count();
    let pending_hitl = super::hitl::pending_count();
    let hitl_enabled = super::hitl::is_enabled();
    json!({
        "suspendedAgents": suspended,
        "pendingHitl": pending_hitl,
        "hitlEnabled": hitl_enabled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn health_counts_suspended_and_pending() {
        let wid = Uuid::new_v4();
        let pane = Uuid::new_v4();
        super::super::suspend::resume(wid, pane); // clear if any
        let before = orchestration_health();
        let base_sus = before["suspendedAgents"].as_u64().unwrap_or(0);
        super::super::suspend::suspend(wid, pane);
        let mid = orchestration_health();
        assert_eq!(
            mid["suspendedAgents"].as_u64().unwrap(),
            base_sus + 1
        );
        assert!(mid["pendingHitl"].as_u64().is_some());
        assert!(mid.get("hitlEnabled").is_some());
        super::super::suspend::resume(wid, pane);
    }
}
