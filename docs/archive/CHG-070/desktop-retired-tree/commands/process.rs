//! Process/cwd projections delegate to Runtime; this adapter owns no process or cwd state.

use uuid::Uuid;

use crate::state::AppState;

/// Resolve the foreground process through Runtime's exact PTY ownership.
#[tauri::command]
pub async fn get_pane_foreground_process(
    workspace_id: String,
    pane_id: String,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        super::runtime::request(
            "get_pane_foreground_process",
            serde_json::json!({"workspaceId":workspace_id,"paneId":pane_id}),
            true,
            Uuid::new_v4(),
        )
    })
    .await
    .map_err(|error| format!("Runtime process query failed: {error}"))?
}

/// Runtime observes cwd and owns its graph/file write-back and events.
#[tauri::command]
pub async fn get_pane_cwd(
    state: tauri::State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
) -> Result<Option<String>, String> {
    let workspace = Uuid::parse_str(&workspace_id).map_err(|error| error.to_string())?;
    let pane = Uuid::parse_str(&pane_id).map_err(|error| error.to_string())?;
    let cwd: Option<String> = tokio::task::spawn_blocking(move || {
        super::runtime::request(
            "get_pane_cwd",
            serde_json::json!({"workspaceId":workspace_id,"paneId":pane_id}),
            true,
            Uuid::new_v4(),
        )
    })
    .await
    .map_err(|error| format!("Runtime cwd query failed: {error}"))??;
    // Legacy consumers retain a projection only; persistence and events belong to Runtime.
    if let Some(cwd) = &cwd {
        if let Some(target) = state
            .workspaces
            .write()
            .get_mut(&workspace)
            .and_then(|workspace| workspace.pane_tree.panes.get_mut(&pane))
        {
            target.cwd = Some(cwd.into());
        }
    }
    Ok(cwd)
}

/// Read the last Runtime-owned cwd projection without triggering persistence.
pub(crate) fn current_pane_cwd_projection(
    _state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Option<String> {
    let endpoint = crate::kernel_lifecycle::read_endpoint()?;
    ridge_kernel::client::list_domain_ptys(&endpoint)
        .ok()?
        .into_iter()
        .find(|pty| pty.pty_id == pane_id && pty.workspace_id == Some(workspace_id))
        .and_then(|pty| pty.cwd)
}
