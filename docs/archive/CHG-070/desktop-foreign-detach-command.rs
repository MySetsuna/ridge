pub fn detach_host_session(
    state: State<'_, AppState>,
    pane_id: String,
    workspace_id: Option<String>,
) -> Result<(), String> {
    let pid = uuid::Uuid::parse_str(&pane_id).map_err(|e| e.to_string())?;
    let remote = state.hosts.detach_foreign(pid)?;
    let wid = match workspace_id {
        Some(s) => uuid::Uuid::parse_str(&s).map_err(|e| e.to_string())?,
        None => state.active_workspace_id(),
    };
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&wid) {
            if ws.terminals.remove(&pid).is_some() {
                *ws.pty_generation.entry(pid).or_insert(0) += 1;
            }
        }
    }
    tracing::info!(
        target: "ridge::hosts",
        host = %remote.host_id,
        remote_pane = %remote.remote_pane_id,
        local_pane = %pane_id,
        "detached foreign view (remote session continues)"
    );
    Ok(())
}

