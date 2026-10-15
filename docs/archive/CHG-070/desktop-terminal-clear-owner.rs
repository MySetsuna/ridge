pub async fn clear_pane_terminal(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
) -> Result<(), String> {
    let workspace_id =
        Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspace_id".to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || clear_pane_terminal_inner(&st, &app, workspace_id, pane_id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn clear_pane_terminal_inner(
    state: &AppState,
    app: &tauri::AppHandle,
    workspace_id: Uuid,
    pane_id: Uuid,
) -> Result<(), AppError> {
    let parser = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .map(|handle| handle.parser.clone())
            .ok_or(AppError::PaneNotFound(pane_id))?
    };

    let fallback = {
        let mut parser = parser.lock();
        let frame = parser.clear_terminal_preserving_prompt();
        (!state.replace_pane_delta_frame(workspace_id, pane_id, frame.clone())).then_some(frame)
    };
    if let Some(kernel) = state
        .workspaces
        .read()
        .get(&workspace_id)
        .and_then(|ws| ws.terminals.get(&pane_id))
        .and_then(|handle| handle.kernel_ref.clone())
    {
        kernel
            .clear()
            .map_err(|error| AppError::PtyError(format!("kernel clear failed: {error}")))?;
    }
    state.clear_pty_scrollback(workspace_id, pane_id);

    if let Some(frame) = fallback {
        emit_terminal_packet(app, workspace_id, pane_id, frame).map_err(AppError::PtyError)?;
    }
    Ok(())
}

