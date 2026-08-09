use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tauri::{Emitter, Manager, State, WebviewWindow};
use uuid::Uuid;

use crate::engine::pane_tree::PaneTree;
use crate::state::{AppState, Workspace, WorkspaceWindowClaim};

pub const FOCUS_WORKSPACE_EVENT: &str = "ridge://focus-workspace";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWindowClaimResult {
    pub claimed: bool,
    pub owner_window_label: String,
}

/// 工作区列表的唯一实现（A1 同源化）：桌面 IPC 命令与 remote_bridge 的
/// `HostStateAccessor::workspaces_list` 共用；序列形 = core `WorkspaceEntry`
///（camelCase id/index/name/displaySeq，替代原逐字重复的 `WorkspaceInfo`）。
pub fn list_workspaces_entries(
    state: &AppState,
) -> Vec<ridge_core::commands::workspace::WorkspaceEntry> {
    let order = state.workspace_order.read();
    let names = state.workspace_names.read();
    let map = state.workspaces.read();
    order
        .iter()
        .enumerate()
        .map(|(i, id)| ridge_core::commands::workspace::WorkspaceEntry {
            id: id.to_string(),
            index: i,
            name: names.get(id).cloned(),
            display_seq: map.get(id).map(|w| w.display_seq).unwrap_or(0),
        })
        .collect()
}

#[tauri::command]
pub fn list_workspaces(
    state: State<'_, AppState>,
) -> Result<Vec<ridge_core::commands::workspace::WorkspaceEntry>, String> {
    Ok(list_workspaces_entries(&state))
}

fn sorted_agent_ids<I>(ids: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    ids.into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Return the desktop's visible Agent identity projection without mutating
/// workspace state. The projection is intentionally global: the kernel roster
/// has no workspace field yet, so a duplicate Agent ID across workspaces is one
/// stable identity rather than a fabricated workspace-qualified identity.
fn desktop_agent_ids(state: &AppState) -> Vec<String> {
    let workspaces = state.workspaces.read();
    sorted_agent_ids(
        workspaces
            .values()
            .flat_map(|workspace| workspace.teammate_agent_pane_map.keys().cloned()),
    )
}

/// Read-only desktop/kernel convergence diagnostic. This command never
/// switches source of truth, claims windows, or persists either projection;
/// kernel transport/decoding failures remain visible to the caller.
#[tauri::command]
pub fn get_domain_convergence_report(
    state: State<'_, AppState>,
) -> Result<ridge_kernel::client::DomainConvergenceReport, String> {
    let endpoint = ridge_kernel::client::running_endpoint()
        .ok_or_else(|| "ridge-kernel domain convergence unavailable".to_string())?;
    let shell_workspaces = list_workspaces_entries(&state)
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let shell_agents = desktop_agent_ids(&state);
    ridge_kernel::client::read_domain_convergence(&endpoint, &shell_workspaces, &shell_agents, &[])
        .map_err(|error| format!("ridge-kernel domain convergence failed: {error}"))
}

#[tauri::command]
pub fn get_active_workspace_id(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.active_workspace_id().to_string())
}

/// Keep the kernel's active workspace aligned with the desktop projection.
/// The kernel remains authoritative for detached Remote, while the desktop
/// owns window-local selection; this bridge is idempotent and best-effort
/// during early startup before the kernel endpoint exists.
pub(crate) fn sync_kernel_active_workspace(workspace_id: Uuid) {
    let Some(endpoint) = ridge_kernel::client::running_endpoint() else {
        return;
    };
    if let Err(error) = ridge_kernel::client::request_json(
        &endpoint,
        "POST",
        &format!("/v1/domain/workspaces/{workspace_id}/activate"),
        None,
    ) {
        tracing::debug!(target: "ridge::workspace", %workspace_id, %error, "kernel active workspace sync deferred");
    }
}

/// Keep the detached kernel Remote projection aligned with the desktop pane
/// tree. This is best-effort during startup, but failures stay visible in logs
/// rather than silently changing the desktop's local topology.
pub(crate) fn sync_kernel_workspace_topology(state: &AppState, workspace_id: Uuid) {
    let Some(endpoint) = ridge_kernel::client::running_endpoint() else {
        return;
    };
    let pane_tree = state
        .workspaces
        .read()
        .get(&workspace_id)
        .map(|workspace| workspace.pane_tree.clone());
    let Some(pane_tree) = pane_tree else {
        return;
    };
    if let Err(error) =
        ridge_kernel::client::sync_domain_workspace_topology(&endpoint, workspace_id, &pane_tree)
    {
        tracing::warn!(target: "ridge::workspace", %workspace_id, %error, "kernel workspace topology sync failed");
    }
}

/// Kernel bootstrap runs after the first window is built. Reconcile every
/// restored workspace once the endpoint is healthy so a startup race cannot
/// leave Remote projecting an old single-leaf topology.
pub(crate) fn sync_kernel_workspace_topologies(state: &AppState) {
    let workspace_ids = state.workspaces.read().keys().copied().collect::<Vec<_>>();
    for workspace_id in workspace_ids {
        sync_kernel_workspace_topology(state, workspace_id);
    }
}

#[tauri::command]
pub fn get_window_active_workspace_id(
    state: State<'_, AppState>,
    window: WebviewWindow,
) -> Result<String, String> {
    Ok(state
        .active_workspace_for_window(window.label())
        .to_string())
}

/// Resolve one desktop window's initial workspace in one backend round-trip.
/// Existing selection wins; otherwise prefer the legacy active workspace, then
/// the first unowned workspace. If every workspace belongs to another window,
/// create and claim a fresh one for the requester.
#[tauri::command]
pub fn acquire_window_workspace(
    state: State<'_, AppState>,
    window: WebviewWindow,
) -> Result<String, String> {
    let preferred = state.active_workspace_id();
    let candidates = state.workspace_order.read().clone();
    // Keep the workspace map read-locked through claim admission. A concurrent
    // close can then only remove the workspace after observing and releasing
    // this claim, never between candidate snapshot and claim insertion.
    let workspaces = state.workspaces.read();
    let candidates: Vec<_> = candidates
        .into_iter()
        .filter(|id| workspaces.contains_key(id))
        .collect();
    let acquired =
        state
            .workspace_window_claims
            .acquire_available(window.label(), preferred, &candidates);
    drop(workspaces);
    if let Some(id) = acquired {
        return Ok(id.to_string());
    }

    let id = insert_new_workspace(&state, PaneTree::new(), None);
    let parsed = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    match state.workspace_window_claims.claim(parsed, window.label()) {
        WorkspaceWindowClaim::Acquired | WorkspaceWindowClaim::AlreadyOwned => {
            state
                .workspace_window_claims
                .select_owned(parsed, window.label());
            broadcast_workspace_list_changed(&state);
            Ok(id)
        }
        WorkspaceWindowClaim::OwnedBy(owner) => Err(format!(
            "new workspace was unexpectedly claimed by window {owner}"
        )),
    }
}

#[tauri::command]
pub fn claim_workspace_window(
    state: State<'_, AppState>,
    window: WebviewWindow,
    workspace_id: String,
) -> Result<WorkspaceWindowClaimResult, String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    if !state.workspaces.read().contains_key(&id) {
        return Err("工作区不存在".into());
    }
    let requester = window.label().to_owned();
    match state.workspace_window_claims.claim(id, &requester) {
        WorkspaceWindowClaim::Acquired | WorkspaceWindowClaim::AlreadyOwned => {
            state.workspace_window_claims.select_owned(id, &requester);
            Ok(WorkspaceWindowClaimResult {
                claimed: true,
                owner_window_label: requester,
            })
        }
        WorkspaceWindowClaim::OwnedBy(owner) => {
            if let Some(owner_window) = window.app_handle().get_webview_window(&owner) {
                let _ = owner_window.unminimize();
                let _ = owner_window.show();
                let _ = owner_window.set_focus();
                if let Err(error) = owner_window.emit(FOCUS_WORKSPACE_EVENT, &workspace_id) {
                    tracing::warn!(
                        target: "ridge::workspace",
                        %workspace_id,
                        owner_window = %owner,
                        %error,
                        "failed to tell owning window to select workspace"
                    );
                }
            } else {
                // A destroyed window can disappear just before its lifecycle event.
                // Clear that stale owner and make one bounded retry.
                state.workspace_window_claims.release_window(&owner);
                if matches!(
                    state.workspace_window_claims.claim(id, &requester),
                    WorkspaceWindowClaim::Acquired | WorkspaceWindowClaim::AlreadyOwned
                ) {
                    state.workspace_window_claims.select_owned(id, &requester);
                    return Ok(WorkspaceWindowClaimResult {
                        claimed: true,
                        owner_window_label: requester,
                    });
                }
            }
            Ok(WorkspaceWindowClaimResult {
                claimed: false,
                owner_window_label: owner,
            })
        }
    }
}

#[tauri::command]
pub fn switch_workspace(state: State<'_, AppState>, workspace_id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    if !state.workspaces.read().contains_key(&id) {
        return Err("工作区不存在".into());
    }
    *state.active_workspace.write() = id;
    sync_kernel_workspace_topology(&state, id);
    sync_kernel_active_workspace(id);
    Ok(())
}

#[tauri::command]
pub fn switch_window_workspace(
    state: State<'_, AppState>,
    window: WebviewWindow,
    workspace_id: String,
) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let map = state.workspaces.read();
    if !map.contains_key(&id) {
        return Err("工作区不存在".into());
    }
    drop(map);
    if !state
        .workspace_window_claims
        .select_owned(id, window.label())
    {
        return Err("工作区未被当前窗口占用".into());
    }
    // Keep legacy observers in sync; desktop commands still resolve through the
    // per-window selection, while Remote/CLI continue using the global command.
    *state.active_workspace.write() = id;
    sync_kernel_workspace_topology(&state, id);
    sync_kernel_active_workspace(id);
    Ok(())
}

/// 插入一个带指定 `pane_tree` 的新根工作区，切为活动区，返回新 id。**不广播**（调用方按需）。
/// `Workspace` 字面量的**唯一**产地——create（空树）与 restore（还原树）共用（DRY）。不触
/// PTY（terminals 为空）。
pub fn insert_new_workspace(state: &AppState, pane_tree: PaneTree, name: Option<&str>) -> String {
    let id = Uuid::new_v4();
    let seq = state.allocate_workspace_seq();
    {
        let mut map = state.workspaces.write();
        map.insert(
            id,
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
                teammate_owned_panes: std::collections::HashSet::new(),
                associated_file_path: None,
                pending_spawns: std::collections::HashMap::new(),
                pty_generation: std::collections::HashMap::new(),
                teammate_metrics: crate::state::TeammateMetrics::default(),
                display_seq: seq,
            },
        );
    }
    state.workspace_order.write().push(id);
    *state.active_workspace.write() = id;
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        state.workspace_names.write().insert(id, name.to_string());
    }
    sync_kernel_workspace_topology(state, id);
    id.to_string()
}

/// 新建根工作区的核心逻辑（不带 Tauri `State` 包装）：空分屏树 + 广播列表变更。抽出供
/// **桌面命令**与 **ridge-core `WorkspaceWriter` 端口**（远端经 dispatch 新建）共用。返回新 id。
pub fn create_workspace_core(state: &AppState, name: Option<&str>) -> String {
    let id = insert_new_workspace(state, PaneTree::new(), name);
    broadcast_workspace_list_changed(state);
    id
}

fn broadcast_workspace_list_changed(state: &AppState) {
    let _ = state
        .remote_structural_tx
        .send(crate::types::RemoteStructuralEvent::WorkspacesChanged);
    let _ = state
        .event_tx
        .try_send(crate::types::GlobalEvent::WorkspaceListChanged);
}

/// 新建根工作区：独立分屏树与终端表，并切换为当前活动区。
#[tauri::command]
pub fn create_workspace(
    state: State<'_, AppState>,
    name: Option<String>,
) -> Result<String, String> {
    Ok(create_workspace_core(&state, name.as_deref()))
}

#[tauri::command]
pub fn create_workspace_for_window(
    state: State<'_, AppState>,
    window: WebviewWindow,
    name: Option<String>,
) -> Result<String, String> {
    let id = insert_new_workspace(&state, PaneTree::new(), name.as_deref());
    let parsed = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state.workspace_window_claims.claim(parsed, window.label());
    state
        .workspace_window_claims
        .select_owned(parsed, window.label());
    broadcast_workspace_list_changed(&state);
    Ok(id)
}

/// close 的**唯一**实现（A1 同源化，iteration 10）：桌面命令、`WorkspaceWriter` 端口与
/// LAN `remote_host_impl` 三方共用——此前三副本中 LAN 版漏发双广播（关区不通知他端）、
/// `workspace_names` 清理各行其是，收敛于此一次修齐。
pub fn close_workspace_core(state: &AppState, id: Uuid) -> Result<(), String> {
    if state.workspace_order.read().len() <= 1 {
        return Err("无法关闭最后一个工作区".into());
    }
    let closed_file = state
        .workspaces
        .read()
        .get(&id)
        .and_then(|workspace| workspace.associated_file_path.clone());
    {
        let mut order = state.workspace_order.write();
        if let Some(pos) = order.iter().position(|&x| x == id) {
            order.remove(pos);
        }
    }
    state.workspaces.write().remove(&id);
    state.workspace_window_claims.release_workspace(id, None);
    // 名称随区清理（旧桌面/端口副本遗留残条——名称表以 id 为键，残条永不再被读）。
    state.workspace_names.write().remove(&id);
    // M1 切片一：暂停侧表 + sidecar 随区清理（设计定的单点钩；best-effort）。
    crate::teammate::suspend::remove_for(id);
    if *state.active_workspace.read() == id {
        if let Some(&first) = state.workspace_order.read().first() {
            *state.active_workspace.write() = first;
        }
    }
    if let (Some(app), Some(path)) = (state.app_handle.get(), closed_file.as_deref()) {
        crate::commands::ridge_file::record_recent_workspace_path(app, path);
        crate::taskbar::refresh_jump_list_async(app.clone());
    }
    broadcast_workspace_list_changed(state);
    Ok(())
}

#[tauri::command]
pub fn close_workspace(state: State<'_, AppState>, workspace_id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    close_workspace_core(&state, id)
}

#[tauri::command]
pub fn reorder_workspaces(
    state: State<'_, AppState>,
    from_index: usize,
    to_index: usize,
) -> Result<(), String> {
    let mut order = state.workspace_order.write();
    if from_index >= order.len() || to_index >= order.len() {
        return Err("无效的索引".into());
    }
    let item = order.remove(from_index);
    order.insert(to_index, item);
    Ok(())
}

/// rename 的**唯一**实现（A1 同源化，iteration 10）：桌面命令与 `WorkspaceWriter` 端口
/// 共用（此前逐字双副本，含三广播链）。
pub fn rename_workspace_core(state: &AppState, id: Uuid, name: String) -> Result<(), String> {
    state.workspace_names.write().insert(id, name);
    // 重命名需要立刻反映到 .ridge 文件的 `name` 字段，让磁盘侧与 UI 保持一致；
    // `schedule_auto_save` 仅在工作区已关联文件时才实际落盘，未保存工作区为 no-op。
    crate::commands::ridge_file::schedule_auto_save(state, id);
    let display_name = state
        .workspace_names
        .read()
        .get(&id)
        .cloned()
        .unwrap_or_default();
    let _ =
        state
            .remote_structural_tx
            .send(crate::types::RemoteStructuralEvent::WorkspaceRenamed {
                workspace_id: id,
                name: display_name,
            });
    let _ = state
        .remote_structural_tx
        .send(crate::types::RemoteStructuralEvent::WorkspacesChanged);
    let _ = state
        .event_tx
        .try_send(crate::types::GlobalEvent::WorkspaceListChanged);
    Ok(())
}

#[tauri::command]
pub fn rename_workspace(
    state: State<'_, AppState>,
    workspace_id: String,
    name: String,
) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    rename_workspace_core(&state, id, name)
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

fn save_history_store(
    app_handle: &tauri::AppHandle,
    store: &WorkspaceHistoryStore,
) -> Result<(), String> {
    let path = get_workspace_history_path(app_handle);
    let content = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    // Atomic write: write to temp file first, then rename
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, content).map_err(|e| e.to_string())?;
    std::fs::rename(&temp_path, &path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_workspace_history(
    app_handle: tauri::AppHandle,
) -> Result<Vec<WorkspaceHistoryItem>, String> {
    let store = load_history_store(&app_handle);
    Ok(store.items)
}

/// save_workspace 的核心（不带 Tauri wrapper）：把当前活动工作区的 pane 树存进历史文件。
/// 抽出供桌面命令与 ridge-core WorkspaceWriter 端口（远端经 dispatch 保存）共用。返回 history id。
pub fn save_workspace_core(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    name: Option<&str>,
) -> Result<String, String> {
    let history_id = Uuid::new_v4().to_string();
    let active_id = state.active_workspace_id();
    let (pane_tree_json, pane_count, workspace_name) = {
        let map = state.workspaces.read();
        let names = state.workspace_names.read();
        map.get(&active_id)
            .map(|ws| {
                let pane_count = ws.pane_tree.get_all_leaves().len();
                let pane_tree_json = serde_json::to_string(&ws.pane_tree).unwrap_or_default();
                let workspace_name = name.map(|n| n.to_string()).unwrap_or_else(|| {
                    names.get(&active_id).cloned().unwrap_or_else(|| {
                        format!(
                            "Saved Workspace {}",
                            chrono::Utc::now().format("%Y-%m-%d %H:%M")
                        )
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
    let mut store = load_history_store(app_handle);
    store.items.push(item);
    save_history_store(app_handle, &store)?;
    Ok(history_id)
}

#[tauri::command]
pub fn save_workspace(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    name: Option<String>,
) -> Result<String, String> {
    save_workspace_core(&app_handle, &state, name.as_deref())
}

#[tauri::command]
pub fn delete_workspace_history(
    app_handle: tauri::AppHandle,
    history_id: String,
) -> Result<(), String> {
    let mut store = load_history_store(&app_handle);
    store.items.retain(|item| item.id != history_id);
    save_history_store(&app_handle, &store)
}

/// restore_workspace 的核心：按 history id 从历史文件还原 pane 树为新工作区。抽出供桌面命令
/// 与 ridge-core WorkspaceWriter 端口共用；复用 [`insert_new_workspace`]（Workspace 字面量零重复）。
pub fn restore_workspace_core(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    history_id: &str,
) -> Result<String, String> {
    let store = load_history_store(app_handle);
    let item = store
        .items
        .iter()
        .find(|i| i.id == history_id)
        .ok_or("历史工作区不存在")?;
    let pane_tree: PaneTree =
        serde_json::from_str(&item.pane_tree_json).map_err(|e| e.to_string())?;
    Ok(insert_new_workspace(state, pane_tree, None))
}

#[tauri::command]
pub fn restore_workspace(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    history_id: String,
) -> Result<String, String> {
    restore_workspace_core(&app_handle, &state, &history_id)
}

#[tauri::command]
pub fn toggle_pin_workspace_history(
    app_handle: tauri::AppHandle,
    history_id: String,
) -> Result<(), String> {
    let mut store = load_history_store(&app_handle);
    if let Some(item) = store.items.iter_mut().find(|i| i.id == history_id) {
        item.is_pinned = !item.is_pinned;
    }
    save_history_store(&app_handle, &store)
}

#[tauri::command]
pub fn rename_workspace_history(
    app_handle: tauri::AppHandle,
    history_id: String,
    name: String,
) -> Result<(), String> {
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
pub fn list_saved_workspaces(
    app_handle: tauri::AppHandle,
) -> Result<Vec<WorkspaceHistoryItem>, String> {
    list_workspace_history(app_handle)
}

#[tauri::command]
pub fn delete_saved_workspace(
    app_handle: tauri::AppHandle,
    history_id: String,
) -> Result<(), String> {
    delete_workspace_history(app_handle, history_id)
}

#[tauri::command]
pub fn rename_saved_workspace(
    app_handle: tauri::AppHandle,
    history_id: String,
    name: String,
) -> Result<(), String> {
    rename_workspace_history(app_handle, history_id, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn test_state() -> AppState {
        let (tx, _rx) = mpsc::channel(8);
        AppState::new(tx)
    }

    #[test]
    fn desktop_agent_projection_is_sorted_and_deduplicated() {
        assert_eq!(
            sorted_agent_ids(vec![
                "agent-b".to_string(),
                "agent-a".to_string(),
                "agent-b".to_string(),
            ]),
            vec!["agent-a".to_string(), "agent-b".to_string()]
        );
    }

    /// A1 同源化门禁（iteration 10）：close 唯一实现必须 ①清 names ②发
    /// WorkspacesChanged 广播（旧 LAN 第三副本漏发的实缺陷）③守最后一个。
    #[test]
    fn close_core_cleans_names_broadcasts_and_guards_last() {
        let state = test_state();
        let second = Uuid::parse_str(&create_workspace_core(&state, Some("b"))).unwrap();
        let mut rx = state.remote_structural_tx.subscribe();

        close_workspace_core(&state, second).unwrap();
        assert!(!state.workspace_order.read().contains(&second));
        assert!(!state.workspaces.read().contains_key(&second));
        assert!(!state.workspace_names.read().contains_key(&second));
        assert!(matches!(
            rx.try_recv(),
            Ok(crate::types::RemoteStructuralEvent::WorkspacesChanged)
        ));

        let last = *state.workspace_order.read().first().unwrap();
        assert!(close_workspace_core(&state, last).is_err());
    }

    /// rename 唯一实现：改名 + WorkspaceRenamed/WorkspacesChanged 双广播。
    #[test]
    fn rename_core_updates_name_and_broadcasts() {
        let state = test_state();
        let wid = state.active_workspace_id();
        let mut rx = state.remote_structural_tx.subscribe();

        rename_workspace_core(&state, wid, "新名".to_string()).unwrap();
        assert_eq!(
            state.workspace_names.read().get(&wid).map(String::as_str),
            Some("新名")
        );
        match rx.try_recv() {
            Ok(crate::types::RemoteStructuralEvent::WorkspaceRenamed { workspace_id, name }) => {
                assert_eq!(workspace_id, wid);
                assert_eq!(name, "新名");
            }
            other => panic!("expected WorkspaceRenamed, got {other:?}"),
        }
        assert!(matches!(
            rx.try_recv(),
            Ok(crate::types::RemoteStructuralEvent::WorkspacesChanged)
        ));
    }

    #[test]
    fn insert_workspace_preserves_tree_and_only_stores_non_empty_name() {
        let state = test_state();
        let mut tree = PaneTree::new();
        let root = tree.get_all_leaves()[0];
        let split = tree
            .split(
                root,
                ridge_core::workspace::pane_tree::SplitDirection::Horizontal,
            )
            .unwrap();

        let named =
            Uuid::parse_str(&insert_new_workspace(&state, tree.clone(), Some("remote"))).unwrap();
        assert_eq!(state.active_workspace_id(), named);
        let stored_tree = {
            let workspaces = state.workspaces.read();
            serde_json::to_value(&workspaces.get(&named).unwrap().pane_tree).unwrap()
        };
        assert_eq!(stored_tree, serde_json::to_value(&tree).unwrap());
        assert_eq!(
            state.workspace_names.read().get(&named).map(String::as_str),
            Some("remote")
        );
        assert!(state.workspace_order.read().contains(&named));

        let unnamed =
            Uuid::parse_str(&insert_new_workspace(&state, PaneTree::new(), Some(""))).unwrap();
        assert!(!state.workspace_names.read().contains_key(&unnamed));
        assert_ne!(named, unnamed);
        assert!(state
            .workspaces
            .read()
            .get(&named)
            .unwrap()
            .pane_tree
            .panes
            .contains_key(&split));
    }
}
