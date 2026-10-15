#[tauri::command]
pub fn unsubscribe_pane_terminal(
    pane_id: String,
    workspace_id: String,
    activation_id: u64,
    state: State<AppState>,
) -> Result<(), String> {
    let pane = Uuid::parse_str(&pane_id).map_err(|_| "invalid paneId".to_string())?;
    let ws = Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspaceId".to_string())?;
    let key = (ws, pane, activation_id);
    let teardown = {
        let mut subs = state.cloud_pane_terminal_subs.lock();
        match subs.get_mut(&key) {
            Some(entry) => {
                entry.1 = entry.1.saturating_sub(1);
                if entry.1 == 0 {
                    subs.remove(&key)
                } else {
                    None
                }
            }
            None => None,
        }
    };
    if let Some((sub_id, _)) = teardown {
        state.unregister_remote_sub(ws, pane, sub_id);
    }
    Ok(())
}

