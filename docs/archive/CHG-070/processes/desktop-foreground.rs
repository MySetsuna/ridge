/// Returns the name of the foreground process running in the given pane's PTY,
/// or `None` if we cannot determine it (falls back to showing the shell name).
///
/// On Unix: reads /proc/<pgid>/comm via the PTY master's `process_group_leader()`.
/// On Windows: enumerates child processes of the shell PID via sysinfo and picks
/// the most-recently-started non-shell child. Falls back to None if unavailable.
#[tauri::command]
pub async fn get_pane_foreground_process(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
) -> Result<Option<String>, String> {
    let workspace_id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;

    let result = get_foreground_process_impl(&state, workspace_id, pane_id);
    Ok(result)
}

fn get_foreground_process_impl(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Option<String> {
    let map = state.workspaces.read();
    let ws = map.get(&workspace_id)?;
    let handle = ws.terminals.get(&pane_id)?;

    // Get the shell PID from the child process
    let shell_pid = handle._child.as_ref().and_then(|c| c.process_id())?;

    drop(map); // release lock before doing I/O

    ridge_core::commands::process::get_foreground_process_name(shell_pid)
}

