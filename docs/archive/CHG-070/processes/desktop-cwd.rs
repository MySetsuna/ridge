/// Returns the current working directory of the shell running in the given pane,
/// by reading the OS-level cwd of the shell process. This is the reliable
/// cross-platform path — it does NOT rely on the shell emitting OSC 7, so
/// plain PowerShell / cmd on Windows also update correctly after `cd`.
///
/// 副作用：发现新的 cwd 时，顺手把它写回 `pane_tree.panes[pane].cwd` —— 这是后端
/// 唯一权威的 cwd 来源，后续 split 会从这里继承；同时触发 .ridge 自动保存。
#[tauri::command]
pub async fn get_pane_cwd(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
) -> Result<Option<String>, String> {
    let workspace_id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;

    let shell_pid = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .and_then(|handle| handle._child.as_ref().and_then(|c| c.process_id()))
    };
    let Some(shell_pid) = shell_pid else {
        return Ok(None);
    };

    let cwd_opt = ridge_core::commands::process::get_process_cwd(shell_pid)
        .map(ridge_core::commands::process::normalize_cwd);

    if let Some(ref cwd) = cwd_opt {
        let path = std::path::PathBuf::from(cwd);
        let mut changed = false;
        {
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&workspace_id) {
                if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                    if pane.cwd.as_deref() != Some(path.as_path()) {
                        pane.cwd = Some(path);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            // 除了写回 tree，还发一次 PaneCwdChanged：下游（Explorer/SCM）的监听器
            // 不再需要等 2.5s 的轮询返回值才知道 cwd 变了 —— 事件路径立刻到达。
            let _ = state
                .event_tx
                .try_send(crate::types::GlobalEvent::PaneCwdChanged {
                    workspace_id,
                    pane_id,
                    cwd: cwd.clone(),
                });
            crate::commands::ridge_file::schedule_auto_save(&*state, workspace_id);
        }
    }

    Ok(cwd_opt)
}

