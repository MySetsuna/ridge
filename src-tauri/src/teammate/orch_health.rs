//! R17-TEAM-HEALTH + OP-AGENT-CP: orchestration health snapshot for team UX.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Monotonic generation bumped on meaningful control-plane events (tests + UI refresh).
static HEALTH_GENERATION: AtomicU64 = AtomicU64::new(1);
/// Foreign pane attachments reported by hosts registry (desktop multi-host).
static FOREIGN_ATTACHED: AtomicUsize = AtomicUsize::new(0);
/// Outbound hosts currently Connected (desktop multi-host).
static OUTBOUND_HOSTS_CONNECTED: AtomicUsize = AtomicUsize::new(0);

/// Bump health generation (suspend/resume/hitl/hosts attach paths).
pub fn bump_health_generation() {
    HEALTH_GENERATION.fetch_add(1, Ordering::SeqCst);
}

pub fn health_generation() -> u64 {
    HEALTH_GENERATION.load(Ordering::SeqCst)
}

/// Hosts layer publishes attachment counts (no AppState dependency in pure snapshot).
pub fn publish_hosts_control_plane(foreign_attached: usize, outbound_connected: usize) {
    FOREIGN_ATTACHED.store(foreign_attached, Ordering::SeqCst);
    OUTBOUND_HOSTS_CONNECTED.store(outbound_connected, Ordering::SeqCst);
    bump_health_generation();
}

pub fn foreign_attached_count() -> usize {
    FOREIGN_ATTACHED.load(Ordering::SeqCst)
}

pub fn outbound_hosts_connected() -> usize {
    OUTBOUND_HOSTS_CONNECTED.load(Ordering::SeqCst)
}

/// Degraded when any agent suspended or HITL pending while HITL enabled.
pub fn compute_degraded(suspended: u64, pending_hitl: u64, hitl_enabled: bool) -> bool {
    suspended > 0 || (hitl_enabled && pending_hitl > 0)
}

/// Overall control-plane severity for UI badges: ok | watch | degraded.
pub fn control_plane_level(
    degraded: bool,
    foreign_attached: u64,
    outbound_connected: u64,
) -> &'static str {
    if degraded {
        return "degraded";
    }
    if foreign_attached > 0 || outbound_connected > 0 {
        return "watch";
    }
    "ok"
}

/// Snapshot of process-local teammate + multi-host control-plane counters (no I/O).
pub fn orchestration_health() -> Value {
    let suspended = super::suspend::suspended_count() as u64;
    let pending_hitl = super::hitl::pending_count() as u64;
    let hitl_enabled = super::hitl::is_enabled();
    let degraded = compute_degraded(suspended, pending_hitl, hitl_enabled);
    let foreign = foreign_attached_count() as u64;
    let outbound = outbound_hosts_connected() as u64;
    let level = control_plane_level(degraded, foreign, outbound);
    let audit = super::hitl_audit::audit_verdict_counts();
    json!({
        "suspendedAgents": suspended,
        "pendingHitl": pending_hitl,
        "hitlEnabled": hitl_enabled,
        "degraded": degraded,
        "generation": health_generation(),
        "foreignAttached": foreign,
        "outboundHostsConnected": outbound,
        "level": level,
        "foreignAttachedHint": "hosts foreign panes appear in roster when agent-tagged",
        "hitlAudit": audit,
        "pollHintMs": poll_hint_ms(level),
    })
}

/// UI poll interval hint by level.
pub fn poll_hint_ms(level: &str) -> u64 {
    match level {
        "degraded" => 1500,
        "watch" => 3000,
        _ => 8000,
    }
}

/// Compact badge string for Agent Center / Remote roster.
pub fn control_plane_badge(health: &Value) -> String {
    let level = health["level"].as_str().unwrap_or("ok");
    let pending = health["pendingHitl"].as_u64().unwrap_or(0);
    let suspended = health["suspendedAgents"].as_u64().unwrap_or(0);
    match level {
        "degraded" if pending > 0 => format!("HITL {pending}"),
        "degraded" => format!("暂停 {suspended}"),
        "watch" => {
            let f = health["foreignAttached"].as_u64().unwrap_or(0);
            let o = health["outboundHostsConnected"].as_u64().unwrap_or(0);
            format!("监视 · F{f}/H{o}")
        }
        _ => String::new(),
    }
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
        assert_eq!(mid["suspendedAgents"].as_u64().unwrap(), base_sus + 1);
        assert!(mid["pendingHitl"].as_u64().is_some());
        assert!(mid.get("hitlEnabled").is_some());
        assert_eq!(mid["degraded"], json!(true));
        assert!(mid["generation"].as_u64().unwrap() >= 1);
        super::super::suspend::resume(wid, pane);
    }

    #[test]
    fn compute_degraded_matrix() {
        assert!(!compute_degraded(0, 0, true));
        assert!(compute_degraded(1, 0, false));
        assert!(compute_degraded(0, 1, true));
        assert!(!compute_degraded(0, 1, false));
    }

    #[test]
    fn bump_advances_generation() {
        let g0 = health_generation();
        bump_health_generation();
        assert!(health_generation() > g0);
    }

    #[test]
    fn publish_hosts_enriches_snapshot_and_level() {
        publish_hosts_control_plane(2, 1);
        let h = orchestration_health();
        assert_eq!(h["foreignAttached"].as_u64().unwrap(), 2);
        assert_eq!(h["outboundHostsConnected"].as_u64().unwrap(), 1);
        // not degraded unless suspended/hitl; level may be watch
        assert!(matches!(
            h["level"].as_str().unwrap(),
            "watch" | "degraded" | "ok"
        ));
        assert_eq!(control_plane_level(false, 1, 0), "watch");
        assert_eq!(control_plane_level(true, 0, 0), "degraded");
        assert_eq!(control_plane_level(false, 0, 0), "ok");
        // reset for other tests
        publish_hosts_control_plane(0, 0);
    }

    #[test]
    fn suspend_path_bumps_generation() {
        let g0 = health_generation();
        let wid = Uuid::new_v4();
        let pane = Uuid::new_v4();
        super::super::suspend::suspend(wid, pane);
        assert!(health_generation() > g0);
        super::super::suspend::resume(wid, pane);
    }

    #[test]
    fn poll_hint_and_badge_and_audit_field() {
        assert_eq!(poll_hint_ms("degraded"), 1500);
        assert_eq!(poll_hint_ms("watch"), 3000);
        assert_eq!(poll_hint_ms("ok"), 8000);
        let h = json!({
            "level": "degraded",
            "pendingHitl": 2,
            "suspendedAgents": 0,
            "foreignAttached": 0,
            "outboundHostsConnected": 0,
        });
        assert!(control_plane_badge(&h).contains("HITL"));
        publish_hosts_control_plane(0, 0);
        let snap = orchestration_health();
        assert!(snap.get("hitlAudit").is_some());
        assert!(snap.get("pollHintMs").is_some());
    }
}
