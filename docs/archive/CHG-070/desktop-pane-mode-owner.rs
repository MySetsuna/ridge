pub async fn toggle_mode(
    state: State<'_, AppState>,
    pane_id: String,
    mode: PaneMode,
) -> Result<(), String> {
    let parsed = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let wid = workspace_containing_pane(&state, parsed)
        .ok_or_else(|| AppError::PaneNotFound(parsed).to_string())?;
    toggle_mode_in_workspace(state, wid, pane_id, mode)
        .await
        .map_err(|e| e.to_string())
}

async fn toggle_mode_in_workspace(
    state: State<'_, AppState>,
    wid: Uuid,
    pane_id: String,
    mode: PaneMode,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&wid)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        let pane = ws
            .pane_tree
            .panes
            .get_mut(&pane_id)
            .ok_or(AppError::PaneNotFound(pane_id))?;
        pane.mode = mode.clone();
    }
    let _ = state
        .event_tx
        .send(GlobalEvent::PaneModeChanged {
            workspace_id: wid,
            pane_id,
            mode,
        })
        .await;
    Ok(())
}

