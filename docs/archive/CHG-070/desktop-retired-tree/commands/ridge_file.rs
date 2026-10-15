//! Transitional workspace-file registrations delegate to Runtime; only OS launch hints and read projections remain.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State};
use uuid::Uuid;

use crate::engine::pane_tree::PaneTree;
use crate::state::AppState;

pub use ridge_core::workspace::file::{RidgeFile, RIDGE_FILE_VERSION};

// 旧版 `.ridge` 文件可能含有 `serialized_panes` 字段（前端 SerializeAddon 已移除）。
// serde 默认忽略未知字段（结构体未声明 `deny_unknown_fields`），旧文件可正常反序列化。

/// 收集当前所有工作区路径，按 workspace_order 顺序写出。已保存工作区复用用户文件；
/// 未保存工作区同步写入应用私有 session 快照。仅在窗口关闭事件里调用（进程即将退出，
/// 不能 spawn 异步），故所有快照写入均有界且失败可见但不阻止退出。
pub fn save_restore_set<R: Runtime>(_app: &tauri::AppHandle<R>, _state: &AppState) {
    if let Err(error) = super::runtime::workspace_file::<Vec<String>>(
        serde_json::json!({"operation":"save-restore"}),
    ) {
        tracing::warn!(%error,"Runtime workspace restore-set commit failed");
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SavedWorkspaceEntry {
    pub name: String,
    pub path: String,
    /// Modification time in seconds since unix epoch (0 if not retrievable).
    pub mtime_secs: u64,
}

/// Scan the default save directory (`<home>/ridge-workspaces/`) for `.ridge`
/// files and return the entries newest-first. Used by the Explorer's
/// "已保存工作区" secondary button.
#[tauri::command]
pub fn list_saved_workspace_files() -> Result<Vec<SavedWorkspaceEntry>, String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"saved-list"}))
}

#[tauri::command]
pub fn delete_saved_workspace_file(
    _app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let _: serde_json::Value = super::runtime::workspace_file(
        serde_json::json!({"operation":"delete-saved","path":path}),
    )?;
    let target = PathBuf::from(&path);

    let detached_ids: Vec<Uuid> = {
        let mut map = state.workspaces.write();
        map.iter_mut()
            .filter_map(|(id, ws)| {
                let matches = ws
                    .associated_file_path
                    .as_ref()
                    .map(|p| p == &target || p == Path::new(&path))
                    .unwrap_or(false);
                if matches {
                    ws.associated_file_path = None;
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    };
    let mut names = state.workspace_names.write();
    for id in detached_ids {
        names.remove(&id);
    }
    drop(names);

    Ok(())
}

/// Tauri command — front-end calls on startup (after deciding it's not a cli launch
/// with a cwd-resident .ridge) to fetch the workspaces it should auto-open.
/// Stale entries (file deleted) are filtered out and the on-disk list is rewritten.
#[tauri::command]
pub fn get_restore_set(_app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"restore"}))
}

const RECENT_MAX: usize = 10;

/// 推入最近打开列表顶部并去重；截断到 `RECENT_MAX`。
fn push_recent<R: Runtime>(_app: &tauri::AppHandle<R>, path: &Path) {
    if let Err(error) = super::runtime::workspace_file::<serde_json::Value>(
        serde_json::json!({"operation":"record-recent","path":path}),
    ) {
        tracing::warn!(%error,"Runtime workspace history update failed");
    }
}

/// Record a workspace that the user explicitly closed while Ridge remains
/// running.  The taskbar Jump List reads the same bounded file, so there is no
/// second history store to drift from the workspace picker.
pub fn record_recent_workspace_path(app: &tauri::AppHandle, path: &Path) {
    if path.is_file() {
        push_recent(app, path);
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceSaveInfo {
    pub workspace_id: String,
    pub file_path: Option<String>,
    pub name: Option<String>,
}

// ─── Commands ──────────────────────────────────────────────────────────────

/// save_workspace_to_file 的核心（不带 Tauri wrapper），供桌面命令与 ridge-core
/// WorkspaceWriter 端口（远端经 dispatch 存盘）共用。非破坏：写 .ridge 存档文件。
pub fn save_workspace_to_file_core(
    _app_handle: &tauri::AppHandle,
    state: &AppState,
    workspace_id: &str,
    name: &str,
    path: Option<&str>,
) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("工作区名不能为空".into());
    }
    let id = Uuid::parse_str(workspace_id).map_err(|e| e.to_string())?;
    // Resolve only the shell's relative OS path; Runtime owns filename/save rules.
    let path = path.map(|path| {
        let value = PathBuf::from(path);
        if value.is_absolute() {
            Ok(value)
        } else {
            std::env::current_dir().map(|cwd| cwd.join(value)).map_err(|error| error.to_string())
        }
    }).transpose()?;
    let saved: serde_json::Value = super::runtime::workspace_file(
        serde_json::json!({"operation":"save","workspaceId":id,"name":trimmed,"path":path}),
    )?;
    let target = PathBuf::from(saved["path"].as_str().ok_or("Runtime omitted saved path")?);

    // 记录工作区 ↔ 文件关联；同步更新显示名与 last-opened 指针。
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&id) {
            ws.associated_file_path = Some(target.clone());
        }
    }
    state
        .workspace_names
        .write()
        .insert(id, trimmed.to_string());
    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
pub fn save_workspace_to_file(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    name: String,
    path: Option<String>,
) -> Result<String, String> {
    save_workspace_to_file_core(&app_handle, &state, &workspace_id, &name, path.as_deref())
}

/// delete_workspace_file 的核心，供桌面命令与 ridge-core WorkspaceWriter 端口共用。
/// **破坏性**：删该工作区关联的 .ridge 存档文件（仅此文件，非任意路径）；语义与桌面命令一致。
pub fn delete_workspace_file_core(
    _app_handle: &tauri::AppHandle,
    state: &AppState,
    workspace_id: &str,
) -> Result<(), String> {
    let id = Uuid::parse_str(workspace_id).map_err(|e| e.to_string())?;
    let _: serde_json::Value = super::runtime::workspace_file(
        serde_json::json!({"operation":"delete-file","workspaceId":id}),
    )?;
    if let Some(workspace) = state.workspaces.write().get_mut(&id) {
        workspace.associated_file_path = None;
    }
    // 清除工作区的已保存名称，使其降级为默认名"工作区 N"。
    state.workspace_names.write().remove(&id);
    Ok(())
}

#[tauri::command]
pub fn delete_workspace_file(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<(), String> {
    delete_workspace_file_core(&app_handle, &state, &workspace_id)
}

#[tauri::command]
pub fn get_workspace_save_info(workspace_id: String) -> Result<WorkspaceSaveInfo, String> {
    super::runtime::workspace_file(
        serde_json::json!({"operation":"info","workspaceId":workspace_id}),
    )
}

#[tauri::command]
pub fn list_workspace_save_info() -> Result<Vec<WorkspaceSaveInfo>, String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"associations"}))
}

#[tauri::command]
pub fn open_workspace_from_file(
    _app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let opened: serde_json::Value =
        super::runtime::workspace_file(serde_json::json!({"operation":"open","path":path}))?;
    let new_id = Uuid::parse_str(
        opened["workspaceId"]
            .as_str()
            .ok_or("Runtime omitted imported workspace ID")?,
    )
    .map_err(|error| error.to_string())?;
    project_runtime_workspace(&state, new_id)
}

/// Legacy presentation projection reads the service identity and metadata.
pub(crate) fn project_runtime_workspace(state: &AppState, new_id: Uuid) -> Result<String, String> {
    let wf: RidgeFile = super::runtime::workspace_file(
        serde_json::json!({"operation":"snapshot","workspaceId":new_id}),
    )?;
    let info: WorkspaceSaveInfo = super::runtime::workspace_file(
        serde_json::json!({"operation":"info","workspaceId":new_id}),
    )?;
    let file_path = info.file_path.map(PathBuf::from);
    let tree: PaneTree =
        serde_json::from_value(wf.pane_tree.clone()).map_err(|error| error.to_string())?;
    let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
    let entry = ridge_kernel::client::read_domain_workspaces(&endpoint)?
        .entries
        .into_iter()
        .find(|entry| entry.id == new_id.to_string())
        .ok_or("Runtime omitted workspace projection")?;
    // Rehydrate teammate_pane_titles from the persisted map, skipping entries
    // whose UUID no longer exists in the restored tree (stale ids from edits
    // made after the last save). Runtime state (busy / idle) stays empty —
    // sessions always start clean, see RidgeFile docstring.
    let known: std::collections::HashSet<Uuid> = tree.panes.keys().copied().collect();
    let restored_titles: HashMap<Uuid, String> = {
        wf.pane_titles
            .iter()
            .filter_map(|(k, v)| {
                Uuid::parse_str(k)
                    .ok()
                    .filter(|id| known.contains(id))
                    .map(|id| (id, v.clone()))
            })
            .collect()
    };
    let seq = entry.display_seq;
    {
        let mut map = state.workspaces.write();
        if let Some(workspace) = map.get_mut(&new_id) {
            workspace.pane_tree = tree;
            workspace.associated_file_path = file_path;
            workspace.teammate_pane_titles = restored_titles;
            workspace.display_seq = seq;
        } else {
        map.insert(
            new_id,
            crate::state::Workspace {
                pane_tree: tree,
                terminals: HashMap::new(),
                teammate_tmux_pane_cursor: 0,
                teammate_pane_titles: restored_titles,
                pane_sizes: HashMap::new(),
                last_pane_index: None,
                created_at: std::time::SystemTime::now(),
                teammate_pane_states: HashMap::new(),
                teammate_agent_pane_map: HashMap::new(),
                teammate_owned_panes: std::collections::HashSet::new(),
                associated_file_path: file_path,
                pending_spawns: HashMap::new(),
                pty_generation: HashMap::new(),
                teammate_metrics: crate::state::TeammateMetrics::default(),
                display_seq: seq,
            },
        );
        }
    }
    {
        let mut order = state.workspace_order.write();
        if !order.contains(&new_id) {order.push(new_id);}
    }
    state
        .workspace_names
        .write()
        .insert(new_id, wf.name.clone());
    *state.active_workspace.write() = new_id;
    Ok(new_id.to_string())
}

/// OS 启动提示；目录文件发现由展示客户端经 Runtime 查询。
///
/// 前端在 `onMount` 里读它来决定启动行为：
/// - `workspace_file_arg` 非空：来自任务栏最近工作区入口，优先打开该文件；
/// - `wind_file_in_cwd` 非空：打开该 .ridge 工作区（取代 last-opened 自动恢复）；
/// - 为空：默认工作区第一颗 pane 的 cwd 已在 `AppState::new` 中种为此 `cwd`，
///   直接沿用即可，前端无需额外动作。
#[derive(Debug, Serialize)]
pub struct StartupContext {
    pub cwd: String,
    pub wind_file_in_cwd: Option<String>,
    /// A taskbar recent-workspace activation.  This wins over cwd/restore-set
    /// selection and is consumed once by the newly created WebView.
    pub workspace_file_arg: Option<String>,
    /// "cli" — process inherited a real working dir from a terminal.
    /// "menu" — process current_dir equals ridge.exe parent (双击启动).
    /// Frontend uses this to gate the restore-set logic: cli launch should not
    /// auto-open saved workspaces (user signalled intent via cwd).
    pub kind: String,
}

#[tauri::command]
pub fn get_startup_context(state: State<'_, AppState>) -> Result<StartupContext, String> {
    let cwd = state
        .startup_cli_cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "unable to determine startup cwd".to_string())?;
    let workspace_file_arg = crate::taskbar::take_pending_workspace_path().or_else(|| {
        let args = std::env::args().collect::<Vec<_>>();
        crate::taskbar::workspace_path_from_args(&args)
    });
    let kind = match state.startup_cwd_kind {
        crate::utils::cwd::StartupCwdKind::Cli => "cli",
        crate::utils::cwd::StartupCwdKind::Menu => "menu",
    }
    .to_string();
    Ok(StartupContext {
        cwd: cwd.to_string_lossy().to_string(),
        wind_file_in_cwd: None,
        workspace_file_arg,
        kind,
    })
}

#[tauri::command]
pub fn get_last_opened_workspace_path(
    _app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"last"}))
}

/// 列出最近打开的 .ridge 路径，顺序新在前；只保留仍存在的文件，过滤掉已失效项。
#[tauri::command]
pub fn list_recent_workspaces(_app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"recent"}))
}

#[tauri::command]
pub fn clear_recent_workspaces(_app_handle: tauri::AppHandle) -> Result<(), String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"clear-recent"}))
}

#[tauri::command]
pub fn clear_last_opened_workspace_path(_app_handle: tauri::AppHandle) -> Result<(), String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"clear-last"}))
}

#[tauri::command]
pub fn get_default_workspace_save_dir() -> Result<String, String> {
    super::runtime::workspace_file(serde_json::json!({"operation":"default-directory"}))
}

/// A directory-browse result. **Migrated to `ridge-core`** — aliased so
/// `crate::commands::ridge_file::DirListing` and the WS dispatch arm in
/// `remote/server.rs` stay identical.
pub use ridge_core::fs::commands::DirListing;

/// 目录浏览：返回给定路径下的直接子目录列表 + 可返回的父目录。§S1+: delegates to
/// `ridge_core::fs::commands::browse_directory` (verbatim nearest-existing-
/// ancestor normalisation + hidden-dir filter + case-insensitive sort).
#[tauri::command]
pub fn browse_directory(path: Option<String>) -> Result<DirListing, String> {
    super::runtime::request(
        "browse_directory",
        serde_json::json!({"path":path}),
        true,
        super::runtime::window_connection("workspace-files"),
    )
}

/// Transitional notifications enqueue work in Runtime; the desktop owns no save worker.
pub fn schedule_auto_save(state: &AppState, workspace_id: Uuid) {
    let path = state
        .workspaces
        .read()
        .get(&workspace_id)
        .and_then(|workspace| workspace.associated_file_path.clone());
    let Some(path) = path else { return };
    let name = state
        .workspace_names
        .read()
        .get(&workspace_id)
        .cloned()
        .unwrap_or_else(|| "workspace".into());
    if let Err(error) = super::runtime::workspace_file::<serde_json::Value>(
        serde_json::json!({"operation":"schedule-save","workspaceId":workspace_id,"name":name,"path":path}),
    ) {
        tracing::warn!(%workspace_id,%error,"Runtime autosave enqueue failed");
    }
}

#[tauri::command]
pub fn get_workspace_file_in_directory(path:String)->Result<Option<String>,String> {
    super::runtime::request("get_workspace_file_in_directory",serde_json::json!({"path":path}),true,
        super::runtime::window_connection("startup-file"))
}
