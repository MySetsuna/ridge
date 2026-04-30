use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{Manager, State};
use uuid::Uuid;

use crate::engine::pane_tree::PaneTree;
use crate::state::{AppState, Workspace};

#[derive(Debug, Serialize, Clone)]
pub struct WorkspaceInfo {
    pub id: String,
    pub index: usize,
    pub name: Option<String>,
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<WorkspaceInfo>, String> {
    let order = state.workspace_order.read();
    let names = state.workspace_names.read();
    Ok(order
        .iter()
        .enumerate()
        .map(|(i, id)| WorkspaceInfo {
            id: id.to_string(),
            index: i,
            name: names.get(id).cloned(),
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
                teammate_tmux_pane_cursor: 0,
                teammate_pane_titles: std::collections::HashMap::new(),
                pane_sizes: std::collections::HashMap::new(),
                last_pane_index: None,
                created_at: std::time::SystemTime::now(),
            teammate_pane_states: std::collections::HashMap::new(),
            teammate_agent_pane_map: std::collections::HashMap::new(),
            associated_file_path: None,
            pending_spawns: std::collections::HashMap::new(),
            teammate_metrics: crate::state::TeammateMetrics::default(),
            },
        );
    }
    state.workspace_order.write().push(id);
    *state.active_workspace.write() = id;
    Ok(id.to_string())
}

#[tauri::command]
pub fn close_workspace(state: State<'_, AppState>, workspace_id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let order = state.workspace_order.read();
    if order.len() <= 1 {
        return Err("无法关闭最后一个工作区".into());
    }
    drop(order);

    // 从工作区顺序中移除
    let mut order = state.workspace_order.write();
    if let Some(pos) = order.iter().position(|&x| x == id) {
        order.remove(pos);
    }
    drop(order);

    // 从工作区map中移除
    let mut map = state.workspaces.write();
    map.remove(&id);

    // 如果关闭的是当前活动工作区，切换到第一个
    let current_active = *state.active_workspace.read();
    if current_active == id {
        if let Some(&first) = state.workspace_order.read().first() {
            *state.active_workspace.write() = first;
        }
    }

    Ok(())
}

#[tauri::command]
pub fn reorder_workspaces(state: State<'_, AppState>, from_index: usize, to_index: usize) -> Result<(), String> {
    let mut order = state.workspace_order.write();
    if from_index >= order.len() || to_index >= order.len() {
        return Err("无效的索引".into());
    }
    let item = order.remove(from_index);
    order.insert(to_index, item);
    Ok(())
}

#[tauri::command]
pub fn rename_workspace(state: State<'_, AppState>, workspace_id: String, name: String) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    {
        let mut names = state.workspace_names.write();
        names.insert(id, name);
    }
    // 重命名需要立刻反映到 .ridge 文件的 `name` 字段，让磁盘侧与 UI 保持一致；
    // `schedule_auto_save` 仅在工作区已关联文件时才实际落盘，未保存工作区为 no-op。
    crate::commands::ridge_file::schedule_auto_save(&*state, id);
    Ok(())
}

// ============ Workspace History (Persistence) ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceHistoryItem {
    pub id: String,
    pub name: String,
    pub saved_at: String,
    pub pane_count: usize,
    pub is_pinned: bool,
    pub pane_tree_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceHistoryStore {
    pub items: Vec<WorkspaceHistoryItem>,
}

fn get_workspace_history_path(app_handle: &tauri::AppHandle) -> PathBuf {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    fs::create_dir_all(&app_data_dir).ok();
    app_data_dir.join("workspace_history.json")
}

fn load_history_store(app_handle: &tauri::AppHandle) -> WorkspaceHistoryStore {
    let path = get_workspace_history_path(app_handle);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(store) = serde_json::from_str(&content) {
                return store;
            }
        }
    }
    WorkspaceHistoryStore::default()
}

fn save_history_store(app_handle: &tauri::AppHandle, store: &WorkspaceHistoryStore) -> Result<(), String> {
    let path = get_workspace_history_path(app_handle);
    let content = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    // Atomic write: write to temp file first, then rename
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content).map_err(|e| e.to_string())?;
    std::fs::rename(&temp_path, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_workspace_history(app_handle: tauri::AppHandle) -> Result<Vec<WorkspaceHistoryItem>, String> {
    let store = load_history_store(&app_handle);
    Ok(store.items)
}

#[tauri::command]
pub fn save_workspace(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    name: Option<String>,
) -> Result<String, String> {
    let history_id = Uuid::new_v4().to_string();

    // Get current workspace pane tree
    let active_id = state.active_workspace_id();
    let (pane_tree_json, pane_count, workspace_name) = {
        let map = state.workspaces.read();
        let names = state.workspace_names.read();
        map.get(&active_id)
            .map(|ws| {
                let pane_count = ws.pane_tree.get_all_leaves().len();
                let pane_tree_json = serde_json::to_string(&ws.pane_tree).unwrap_or_default();
                // Use provided name, or fall back to saved workspace name, or auto-generate
                let workspace_name = name.unwrap_or_else(|| {
                    names.get(&active_id).cloned().unwrap_or_else(|| {
                        format!("Saved Workspace {}", chrono::Utc::now().format("%Y-%m-%d %H:%M"))
                    })
                });
                (pane_tree_json, pane_count, workspace_name)
            })
            .unwrap_or((String::new(), 0, "Unnamed Workspace".to_string()))
    };

    let item = WorkspaceHistoryItem {
        id: history_id.clone(),
        name: workspace_name,
        saved_at: chrono::Utc::now().to_rfc3339(),
        pane_count,
        is_pinned: false,
        pane_tree_json,
    };

    let mut store = load_history_store(&app_handle);
    store.items.push(item);
    save_history_store(&app_handle, &store)?;

    Ok(history_id)
}

#[tauri::command]
pub fn delete_workspace_history(app_handle: tauri::AppHandle, history_id: String) -> Result<(), String> {
    let mut store = load_history_store(&app_handle);
    store.items.retain(|item| item.id != history_id);
    save_history_store(&app_handle, &store)
}

#[tauri::command]
pub fn restore_workspace(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    history_id: String,
) -> Result<String, String> {
    let store = load_history_store(&app_handle);
    let item = store
        .items
        .iter()
        .find(|i| i.id == history_id)
        .ok_or("历史工作区不存在")?;

    // Create new workspace with restored pane tree
    let new_id = Uuid::new_v4();
    let pane_tree: PaneTree = serde_json::from_str(&item.pane_tree_json)
        .map_err(|e| e.to_string())?;

    {
        let mut map = state.workspaces.write();
        map.insert(
            new_id,
            Workspace {
                pane_tree,
                terminals: std::collections::HashMap::new(),
                teammate_tmux_pane_cursor: 0,
                teammate_pane_titles: std::collections::HashMap::new(),
                pane_sizes: std::collections::HashMap::new(),
                last_pane_index: None,
                created_at: std::time::SystemTime::now(),
            teammate_pane_states: std::collections::HashMap::new(),
            teammate_agent_pane_map: std::collections::HashMap::new(),
            associated_file_path: None,
            pending_spawns: std::collections::HashMap::new(),
            teammate_metrics: crate::state::TeammateMetrics::default(),
            },
        );
    }

    state.workspace_order.write().push(new_id);
    *state.active_workspace.write() = new_id;

    Ok(new_id.to_string())
}

#[tauri::command]
pub fn toggle_pin_workspace_history(app_handle: tauri::AppHandle, history_id: String) -> Result<(), String> {
    let mut store = load_history_store(&app_handle);
    if let Some(item) = store.items.iter_mut().find(|i| i.id == history_id) {
        item.is_pinned = !item.is_pinned;
    }
    save_history_store(&app_handle, &store)
}

#[tauri::command]
pub fn rename_workspace_history(app_handle: tauri::AppHandle, history_id: String, name: String) -> Result<(), String> {
    let mut store = load_history_store(&app_handle);
    if let Some(item) = store.items.iter_mut().find(|i| i.id == history_id) {
        item.name = name;
    }
    save_history_store(&app_handle, &store)
}

// ============ Frontend-compatible command aliases ============
// These forward to the existing "workspace_history" commands so the frontend
// can use the more intuitive "saved_workspaces" naming.

#[tauri::command]
pub fn list_saved_workspaces(app_handle: tauri::AppHandle) -> Result<Vec<WorkspaceHistoryItem>, String> {
    list_workspace_history(app_handle)
}

#[tauri::command]
pub fn delete_saved_workspace(app_handle: tauri::AppHandle, history_id: String) -> Result<(), String> {
    delete_workspace_history(app_handle, history_id)
}

#[tauri::command]
pub fn rename_saved_workspace(app_handle: tauri::AppHandle, history_id: String, name: String) -> Result<(), String> {
    rename_workspace_history(app_handle, history_id, name)
}
