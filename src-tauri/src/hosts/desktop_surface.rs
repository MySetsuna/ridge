//! Desktop-only hosts / multi-host surface (OP-CAP-PARITY).
//!
//! These methods are registered on the Tauri desktop IPC and **must never**
//! appear in `REMOTE_ALLOWLIST` — Remote controllers attach to one host only;
//! multi-host outbound is a local-desktop controller concern.

/// Canonical list of desktop-local host commands (keep in sync with
/// `lib.rs` invoke_handler and capabilityContract tests).
pub const DESKTOP_ONLY_HOST_METHODS: &[&str] = &[
    "host_list_snapshot",
    "register_frontend_host",
    "connect_host",
    "disconnect_host",
    "forget_host",
    "attach_host_session",
    "detach_host_session",
    "list_host_sessions",
    "inject_host_output",
    "get_outbound_stats",
    "pump_host_output",
    "bind_mock_outbound_and_list",
    "step_host_reconnect",
    "cancel_host_reconnect",
    "get_foreign_history_tail",
    "append_foreign_history",
    "foreign_history_pull_budget",
    "set_foreign_history_cap",
    "get_live_backpressure",
];

/// Methods that mutate host connectivity / foreign panes.
pub const DESKTOP_ONLY_HOST_MUTATING: &[&str] = &[
    "connect_host",
    "register_frontend_host",
    "disconnect_host",
    "forget_host",
    "attach_host_session",
    "detach_host_session",
    "inject_host_output",
];

pub fn is_desktop_only_host_method(name: &str) -> bool {
    DESKTOP_ONLY_HOST_METHODS.contains(&name)
}

pub fn is_desktop_only_host_mutating(name: &str) -> bool {
    DESKTOP_ONLY_HOST_MUTATING.contains(&name)
}

/// Snapshot of outbound observability for desktop UI / diagnostics.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboundStatsDto {
    pub host_id: String,
    pub state: String,
    pub subscribed: Vec<String>,
    pub hello_ok: u64,
    pub list_ok: u64,
    pub subscribe_ok: u64,
    pub write_ok: u64,
    pub resize_ok: u64,
    pub fanout_bytes: u64,
    pub reconnect_attempts: u64,
    pub resubscribe_ok: u64,
    pub errors: u64,
    /// AC4-C8: live output buffer cap / dropped (backpressure).
    pub live_buffer_cap: u64,
    pub live_buffer_bytes: u64,
    pub live_dropped_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_only_set_is_closed_and_includes_detach() {
        assert!(is_desktop_only_host_method("detach_host_session"));
        assert!(is_desktop_only_host_method("attach_host_session"));
        assert!(is_desktop_only_host_method("register_frontend_host"));
        assert!(is_desktop_only_host_mutating("connect_host"));
        assert!(!is_desktop_only_host_mutating("host_list_snapshot"));
        assert!(!is_desktop_only_host_method("get_orchestration_health"));
        assert!(!is_desktop_only_host_method("write_to_pty"));
    }

    #[test]
    fn no_overlap_with_typical_remote_surface() {
        for m in [
            "get_workspace_snapshot",
            "list_hitl_pending",
            "resolve_hitl_remote",
            "get_orchestration_health",
        ] {
            assert!(
                !is_desktop_only_host_method(m),
                "{m} must stay remote-capable, not desktop-only host"
            );
        }
    }
}
