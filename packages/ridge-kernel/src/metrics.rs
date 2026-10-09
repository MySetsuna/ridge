//! Kernel runtime metrics endpoint. Used by the v0.1.92 closeout soak
//! harness to capture live state without parsing logs.
//!
//! Returns a JSON snapshot of:
//! - workspace count
//! - pty count + per-pty subscriber count + replay-window pressure
//! - output-lease count
//! - remote host count
//! - group subscription total
//!
//! No PID/process metrics (those are measured externally via tasklist).

use axum::{extract::State, http::StatusCode, response::Json};
use serde::Serialize;

use crate::server::AppState;

#[derive(Serialize)]
pub struct MetricsSnapshot {
    pub ok: bool,
    pub role: &'static str,
    pub pid: u32,
    pub started_at_unix: u64,
    pub workspace_count: usize,
    pub pty_count: usize,
    pub output_lease_count: usize,
    pub remote_host_count: usize,
    pub group_subscription_total: usize,
    pub ptys: Vec<PtyMetricsEntry>,
}

#[derive(Serialize)]
pub struct PtyMetricsEntry {
    pub pty_id: String,
    pub lifecycle: &'static str,
    pub oldest_seq: u64,
    pub next_seq: u64,
    pub subscriber_count: usize,
}

pub async fn metrics_handler(
    State(st): State<AppState>,
) -> Result<Json<MetricsSnapshot>, StatusCode> {
    let workspace_count = st.workspaces.lock().map(|g| g.len()).unwrap_or(0);
    let output_lease_count = st.output_leases.lock().map(|m| m.len()).unwrap_or(0);
    let remote_host_count = st
        .remote_hosts
        .lock()
        .map(|g| g.records().len())
        .unwrap_or(0);
    let group_subscription_total = st
        .groups
        .lock()
        .map(|g| g.values().map(|s| s.len()).sum())
        .unwrap_or(0);

    let ptys_list = st.ptys.list();
    let mut pty_entries = Vec::with_capacity(ptys_list.len());
    for info in &ptys_list {
        let lifecycle = match st.ptys.lifecycle_state(info.id) {
            Some(crate::pty::TerminalLifecycleState::Starting) => "Starting",
            Some(crate::pty::TerminalLifecycleState::Running) => "Running",
            Some(crate::pty::TerminalLifecycleState::Exited) => "Exited",
            Some(crate::pty::TerminalLifecycleState::Reaped) => "Reaped",
            None => "Unknown",
        };
        let bounds = st.ptys.output_bounds(info.id).unwrap_or((0, 0));
        let subscriber_count = st
            .ptys
            .subscriber_count(info.id)
            .unwrap_or(0);
        pty_entries.push(PtyMetricsEntry {
            pty_id: info.id.to_string(),
            lifecycle,
            oldest_seq: bounds.0,
            next_seq: bounds.1,
            subscriber_count,
        });
    }

    Ok(Json(MetricsSnapshot {
        ok: true,
        role: "ridge-kernel",
        pid: st.pid,
        started_at_unix: st.started_at_unix,
        workspace_count,
        pty_count: ptys_list.len(),
        output_lease_count,
        remote_host_count,
        group_subscription_total,
        ptys: pty_entries,
    }))
}
