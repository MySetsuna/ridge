//! Read projections resolve exact Kernel-owned PTYs, never desktop state or process names.
use crate::server::AppState;
use ridge_core::{CoreError, CoreResult, Ctx};
use serde_json::{json, Value};
use uuid::Uuid;

pub(crate) fn dispatch(
    state: &AppState,
    method: &str,
    args: &Value,
    ctx: &Ctx,
) -> Option<CoreResult<Value>> {
    if !matches!(
        method,
        "get_pane_foreground_process" | "get_pty_runtime_identity" | "get_pane_cwd"
    ) {
        return None;
    }
    Some((|| {
        let id = |key| {
            args[key]
                .as_str()
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| CoreError::InvalidArgs(format!("invalid {key}")))
        };
        let workspace = id("workspaceId")?;
        let pane = id("paneId")?;
        let graph = state
            .workspaces
            .lock()
            .map_err(|_| CoreError::internal("workspace state unavailable"))?;
        let metadata = graph
            .workspace(workspace)
            .ok_or_else(|| CoreError::InvalidArgs("workspace not found".into()))?;
        let declared = metadata.pane_tree.panes.contains_key(&pane);
        drop(graph);
        let Ok(pty) = state.ptys.info(pane) else {
            return if declared {
                Ok(Value::Null)
            } else {
                Err(CoreError::InvalidArgs(
                    "pane does not belong to workspace".into(),
                ))
            };
        };
        if pty.workspace_id != Some(workspace) {
            return Err(CoreError::InvalidArgs("PTY workspace changed".into()));
        }
        let (Some(pid), Some(start)) = (pty.child_pid, pty.child_start_instance) else {
            return Ok(Value::Null);
        };
        let instance = ridge_core::process_identity::ProcessInstance {
            pid,
            start_instance: start,
        };
        if ridge_core::process_identity::validate(&instance).is_err() {
            return Ok(Value::Null);
        }
        let result = if method == "get_pane_cwd" {
            ridge_core::commands::process::get_process_cwd(pid)
                .map(ridge_core::commands::process::normalize_cwd)
                .map(Value::String)
                .unwrap_or(Value::Null)
        } else if method == "get_pane_foreground_process" {
            ridge_core::commands::process::get_foreground_process_name(pid)
                .map(Value::String)
                .unwrap_or(Value::Null)
        } else {
            state.roster.lock().map_err(|_|CoreError::internal("Agent roster unavailable"))?.agent_identities().into_iter()
                .find(|identity|identity.workspace_id==workspace.to_string() && identity.pane_id==pane.to_string()
                    && identity.online && identity.generation>0 && !identity.lease.trim().is_empty())
                .map(|identity|json!({"agentId":identity.agent_id,"generation":identity.generation,"lease":identity.lease}))
                .unwrap_or(Value::Null)
        };
        // OS enumeration can outlive its original child. Recheck before publishing the observation.
        if ridge_core::process_identity::validate(&instance).is_err()
            || state.ptys.info(pane).map_or(true, |current| {
                current.workspace_id != Some(workspace)
                    || current.child_pid != Some(instance.pid)
                    || current.child_start_instance.as_deref()
                        != Some(instance.start_instance.as_str())
            })
        {
            return Ok(Value::Null);
        }
        if method == "get_pane_cwd" {
            if let Some(cwd) = result.as_str() {
                crate::workspace_commands::update_cwd(state, workspace, pane, &instance, cwd, ctx)?;
            }
        }
        Ok(result)
    })())
}
