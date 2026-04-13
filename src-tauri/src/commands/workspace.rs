use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::engine::pane_tree::PaneTree;
use crate::state::{AppState, Workspace};

#[derive(Debug, Serialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub index: usize,
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<WorkspaceInfo>, String> {
    let order = state.workspace_order.read();
    Ok(order
        .iter()
        .enumerate()
        .map(|(i, id)| WorkspaceInfo {
            id: id.to_string(),
            index: i,
        })
        .collect())
}

#[tauri::command]
pub fn get_active_workspace_id(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.active_workspace_id().to_string())
}

#[tauri::command]
pub fn switch_workspace(state: State<'_, AppState>, workspace_id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let map = state.workspaces.read();
    if !map.contains_key(&id) {
        return Err("工作区不存在".into());
    }
    drop(map);
    *state.active_workspace.write() = id;
    Ok(())
}

/// 新建根工作区：独立分屏树与终端表，并切换为当前活动区。
#[tauri::command]
pub fn create_workspace(state: State<'_, AppState>) -> Result<String, String> {
    let id = Uuid::new_v4();
    {
        let mut map = state.workspaces.write();
        map.insert(
            id,
            Workspace {
                pane_tree: PaneTree::new(),
                terminals: std::collections::HashMap::new(),
            },
        );
    }
    state.workspace_order.write().push(id);
    *state.active_workspace.write() = id;
    Ok(id.to_string())
}
