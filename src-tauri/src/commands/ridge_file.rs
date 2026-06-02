//! `.ridge` 工作区文件：一个工作区的完整可移植快照。
//!
//! 文件结构（JSON，`version` 作为前向兼容锚点）：
//! ```json
//! {
//!   "version": 1,
//!   "name": "My Workspace",
//!   "saved_at": "2026-04-24T10:00:00Z",
//!   "pane_tree": { ... 工作区 PaneTree JSON ... },
//!   "git_repos": ["C:/code/ridge"],
//!   "index_path": null
//! }
//! ```
//!
//! 自动保存：
//! - 后端持有防抖调度器 `AutoSaveScheduler`，命令层在 pane_tree / cwd / git
//!   状态发生变化后调用 `schedule(workspace_id)`；
//! - 调度器合并 `AUTO_SAVE_DEBOUNCE_MS` 内的触发，到期后后台线程读快照并
//!   原子写入 `.ridge` 文件，主线程零阻塞。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;

use crate::engine::pane_tree::PaneTree;
use crate::state::AppState;

const RIDGE_FILE_VERSION: u32 = 1;
const AUTO_SAVE_DEBOUNCE_MS: u64 = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RidgeFile {
    pub version: u32,
    pub name: String,
    pub saved_at: String,
    pub pane_tree: serde_json::Value,
    #[serde(default)]
    pub git_repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_path: Option<String>,
    /// Per-pane teammate display names keyed by pane UUID string. Written by
    /// Claude Code's `new-window -n` / `split-window -n` and surfaced in the
    /// Ridge pane header. Runtime state (busy / idle, agent_id) is deliberately
    /// NOT persisted — it's session-scoped and would lie across restarts.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pane_titles: HashMap<String, String>,
}

// 旧版 `.ridge` 文件可能含有 `serialized_panes` 字段（前端 SerializeAddon 已移除）。
// serde 默认忽略未知字段（结构体未声明 `deny_unknown_fields`），旧文件可正常反序列化。

/// 默认保存目录：`<home>/ridge-workspaces/`（不存在时创建）。
fn default_save_dir() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("ridge-workspaces");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "workspace".into()
    } else {
        trimmed
    }
}

fn resolve_target_path(name: &str, explicit: Option<String>) -> PathBuf {
    if let Some(p) = explicit {
        let pb = PathBuf::from(&p);
        if pb.extension().and_then(|s| s.to_str()) == Some("ridge") {
            pb
        } else if pb.is_dir() || p.ends_with(std::path::MAIN_SEPARATOR) {
            pb.join(format!("{}.ridge", sanitize_filename(name)))
        } else {
            // 用户传的是 "<dir>/<stem>" 之类，补 `.ridge` 扩展。
            let mut with_ext = pb.clone();
            with_ext.set_extension("ridge");
            with_ext
        }
    } else {
        default_save_dir().join(format!("{}.ridge", sanitize_filename(name)))
    }
}

fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("ridge.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    // Tighten permissions to user-only after the rename. `.ridge` files now
    // include serialized scrollback (shell command history, env-printed
    // values, possibly secrets that scrolled past). Best-effort — failure
    // is logged but doesn't fail the save.
    if let Err(e) = set_secure_permissions(path) {
        eprintln!(
            "[ridge] warning: failed to set 0600 on {}: {e}",
            path.display()
        );
    }
    Ok(())
}

/// Restrict `path` to the current user only.
///
/// - **Unix**: `chmod 0600` via `PermissionsExt`.
/// - **Windows**: `set_readonly(false)` is the only knob `std::fs` exposes
///   without pulling in `winapi`/`windows-sys` ACL APIs. Real Windows ACL
///   tightening would require `SetSecurityInfo`/`SetEntriesInAcl`; we leave
///   the default ACL (inherits from parent, typically user-private under
///   `%USERPROFILE%`) and explicitly clear the readonly bit so future
///   atomic writes can rename over the file.
#[cfg(unix)]
fn set_secure_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)
}

#[cfg(windows)]
fn set_secure_permissions(path: &Path) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_readonly(false);
    std::fs::set_permissions(path, perms)
}

fn last_opened_pointer_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("last_workspace.txt")
}

fn recent_workspaces_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("recent_workspaces.json")
}

/// `restore_workspaces.json` 路径：close 时把当前已保存（associated_file_path != None）
/// 工作区的 .ridge 路径列表写到这里；下次非 cli 启动时回读用于自动恢复 tab。
fn restore_workspaces_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("restore_workspaces.json")
}

/// 收集当前所有 `associated_file_path` 非空的工作区路径，按 workspace_order 顺序写出。
/// 仅在窗口关闭事件里同步调用（进程即将退出，不能 spawn 异步）。
pub fn save_restore_set(app: &tauri::AppHandle, state: &AppState) {
    let order = state.workspace_order.read().clone();
    let map = state.workspaces.read();
    let paths: Vec<String> = order
        .iter()
        .filter_map(|wid| {
            map.get(wid).and_then(|ws| {
                ws.associated_file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
            })
        })
        .collect();
    drop(map);
    if let Ok(s) = serde_json::to_string_pretty(&paths) {
        let _ = std::fs::write(restore_workspaces_path(app), s);
    }
}

fn load_restore_set(app: &tauri::AppHandle) -> Vec<String> {
    let p = restore_workspaces_path(app);
    if !p.is_file() {
        return Vec::new();
    }
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

#[derive(Debug, Serialize)]
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
    let dir = default_save_dir();
    let mut out: Vec<SavedWorkspaceEntry> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ridge") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let mtime_secs = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(SavedWorkspaceEntry {
            name,
            path: path.to_string_lossy().to_string(),
            mtime_secs,
        });
    }
    out.sort_by(|a, b| b.mtime_secs.cmp(&a.mtime_secs));
    Ok(out)
}

/// Tauri command — front-end calls on startup (after deciding it's not a cli launch
/// with a cwd-resident .ridge) to fetch the workspaces it should auto-open.
/// Stale entries (file deleted) are filtered out and the on-disk list is rewritten.
#[tauri::command]
pub fn get_restore_set(app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    let raw = load_restore_set(&app_handle);
    let alive: Vec<String> = raw
        .iter()
        .filter(|p| PathBuf::from(p).is_file())
        .cloned()
        .collect();
    if alive.len() != raw.len() {
        if let Ok(s) = serde_json::to_string_pretty(&alive) {
            let _ = std::fs::write(restore_workspaces_path(&app_handle), s);
        }
    }
    Ok(alive)
}

const RECENT_MAX: usize = 10;

fn load_recent(app: &tauri::AppHandle) -> Vec<String> {
    let p = recent_workspaces_path(app);
    if !p.is_file() {
        return Vec::new();
    }
    match std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok()) {
        Some(v) => v,
        None => Vec::new(),
    }
}

fn save_recent(app: &tauri::AppHandle, list: &[String]) {
    if let Ok(s) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(recent_workspaces_path(app), s);
    }
}

/// 推入最近打开列表顶部并去重；截断到 `RECENT_MAX`。
fn push_recent(app: &tauri::AppHandle, path: &Path) {
    let canonical = path.to_string_lossy().to_string();
    let mut list = load_recent(app);
    list.retain(|p| p != &canonical);
    list.insert(0, canonical);
    list.truncate(RECENT_MAX);
    save_recent(app, &list);
}

fn set_last_opened(app: &tauri::AppHandle, path: &Path) {
    let _ = std::fs::write(last_opened_pointer_path(app), path.to_string_lossy().as_bytes());
    push_recent(app, path);
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceSaveInfo {
    pub workspace_id: String,
    pub file_path: Option<String>,
    pub name: Option<String>,
}

/// 构造当前工作区的 RidgeFile 快照。
fn snapshot_workspace(
    state: &AppState,
    workspace_id: Uuid,
    name: &str,
) -> Result<RidgeFile, String> {
    let map = state.workspaces.read();
    let ws = map.get(&workspace_id).ok_or_else(|| "工作区不存在".to_string())?;
    let tree_json = serde_json::to_value(&ws.pane_tree).map_err(|e| e.to_string())?;
    let git_repos: Vec<String> = ws
        .pane_tree
        .panes
        .values()
        .filter_map(|p| p.cwd.as_ref())
        .filter_map(|cwd| find_git_root(cwd))
        .map(|p| p.to_string_lossy().to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let pane_titles: HashMap<String, String> = ws
        .teammate_pane_titles
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    Ok(RidgeFile {
        version: RIDGE_FILE_VERSION,
        name: name.to_string(),
        saved_at: Utc::now().to_rfc3339(),
        pane_tree: tree_json,
        git_repos,
        index_path: None,
        pane_titles,
    })
}

/// 简单的向上查找 `.git` 目录，命中即返回仓库根。
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => return None,
        }
    }
}

// ─── Commands ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn save_workspace_to_file(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
    name: String,
    path: Option<String>,
) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("工作区名不能为空".into());
    }
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let target = resolve_target_path(trimmed, path);

    let wf = snapshot_workspace(&state, id, trimmed)?;
    let json = serde_json::to_vec_pretty(&wf).map_err(|e| e.to_string())?;
    atomic_write(&target, &json).map_err(|e| e.to_string())?;

    // 记录工作区 ↔ 文件关联；同步更新显示名与 last-opened 指针。
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&id) {
            ws.associated_file_path = Some(target.clone());
        }
    }
    state.workspace_names.write().insert(id, trimmed.to_string());
    set_last_opened(&app_handle, &target);

    Ok(target.to_string_lossy().to_string())
}

#[tauri::command]
pub fn delete_workspace_file(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<(), String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let path_opt = {
        let mut map = state.workspaces.write();
        map.get_mut(&id).and_then(|ws| ws.associated_file_path.take())
    };
    if let Some(path) = path_opt {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        // 如果 last-opened 指向了这个文件，也要清理。
        let ptr = last_opened_pointer_path(&app_handle);
        if let Ok(s) = std::fs::read_to_string(&ptr) {
            if PathBuf::from(s.trim()) == path {
                let _ = std::fs::remove_file(&ptr);
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_workspace_save_info(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<WorkspaceSaveInfo, String> {
    let id = Uuid::parse_str(&workspace_id).map_err(|e| e.to_string())?;
    let map = state.workspaces.read();
    let ws = map.get(&id).ok_or_else(|| "工作区不存在".to_string())?;
    let name = state.workspace_names.read().get(&id).cloned();
    Ok(WorkspaceSaveInfo {
        workspace_id: id.to_string(),
        file_path: ws
            .associated_file_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        name,
    })
}

#[tauri::command]
pub fn list_workspace_save_info(
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceSaveInfo>, String> {
    let map = state.workspaces.read();
    let names = state.workspace_names.read();
    let order = state.workspace_order.read();
    Ok(order
        .iter()
        .filter_map(|id| {
            map.get(id).map(|ws| WorkspaceSaveInfo {
                workspace_id: id.to_string(),
                file_path: ws
                    .associated_file_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                name: names.get(id).cloned(),
            })
        })
        .collect())
}

#[tauri::command]
pub fn open_workspace_from_file(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let file_path = PathBuf::from(&path);
    if !file_path.is_file() {
        return Err(format!(".ridge 文件不存在：{path}"));
    }
    // 严格校验扩展名：只接受 .ridge —— 旧的 .wind 工作区文件已不再被支持，
    // 用户需要手动重命名 / 用旧版重新导出后再打开。
    let ext_ok = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("ridge"))
        .unwrap_or(false);
    if !ext_ok {
        return Err(format!("不再支持非 .ridge 工作区文件：{path}"));
    }
    let raw = std::fs::read(&file_path).map_err(|e| e.to_string())?;
    let wf: RidgeFile = serde_json::from_slice(&raw).map_err(|e| format!(".ridge 格式非法: {e}"))?;
    if wf.version != RIDGE_FILE_VERSION {
        return Err(format!(
            ".ridge 版本 {} 与当前 ({}) 不匹配",
            wf.version, RIDGE_FILE_VERSION
        ));
    }
    // 反序列化 pane_tree（用于重建布局；真实 PTY 由前端 Pane onMount 重起）。
    let tree: PaneTree =
        serde_json::from_value(wf.pane_tree.clone()).map_err(|e| format!("pane tree 解析失败: {e}"))?;

    // 如果这个文件已经关联到某个现存 workspace，直接切过去而不是重复打开。
    {
        let map = state.workspaces.read();
        let existing = map.iter().find_map(|(wid, ws)| {
            ws.associated_file_path
                .as_ref()
                .and_then(|p| if p == &file_path { Some(*wid) } else { None })
        });
        if let Some(wid) = existing {
            drop(map);
            *state.active_workspace.write() = wid;
            set_last_opened(&app_handle, &file_path);
            return Ok(wid.to_string());
        }
    }

    let new_id = Uuid::new_v4();
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
    let seq = state.allocate_workspace_seq();
    {
        let mut map = state.workspaces.write();
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
                associated_file_path: Some(file_path.clone()),
                pending_spawns: HashMap::new(),
                pty_generation: HashMap::new(),
                teammate_metrics: crate::state::TeammateMetrics::default(),
                display_seq: seq,
            },
        );
    }
    state.workspace_order.write().push(new_id);
    state.workspace_names.write().insert(new_id, wf.name.clone());
    *state.active_workspace.write() = new_id;
    set_last_opened(&app_handle, &file_path);
    Ok(new_id.to_string())
}

/// 启动上下文：当前进程 cwd + cwd 顶层第一个 `.ridge` 文件（若存在）。
///
/// 前端在 `onMount` 里读它来决定启动行为：
/// - `wind_file_in_cwd` 非空：打开该 .ridge 工作区（取代 last-opened 自动恢复）；
/// - 为空：默认工作区第一颗 pane 的 cwd 已在 `AppState::new` 中种为此 `cwd`，
///   直接沿用即可，前端无需额外动作。
#[derive(Debug, Serialize)]
pub struct StartupContext {
    pub cwd: String,
    pub wind_file_in_cwd: Option<String>,
    /// "cli" — process inherited a real working dir from a terminal.
    /// "menu" — process current_dir equals ridge.exe parent (双击启动).
    /// Frontend uses this to gate the restore-set logic: cli launch should not
    /// auto-open saved workspaces (user signalled intent via cwd).
    pub kind: String,
}

#[tauri::command]
pub fn get_startup_context(state: State<'_, AppState>) -> Result<StartupContext, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    // 只扫一层：避免在用户主目录 / 大型项目根下做深度遍历，也匹配用户预期
    // “cwd 内是否直接放着 .ridge”。
    let mut wind_files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cwd) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("ridge") {
                wind_files.push(path);
            }
        }
    }
    // 字典序取首个，结果稳定可预测；多数场景用户只会放 0 或 1 个 .ridge。
    wind_files.sort();
    let wind_file_in_cwd = wind_files
        .into_iter()
        .next()
        .map(|p| p.to_string_lossy().to_string());
    let kind = match state.startup_cwd_kind {
        crate::utils::cwd::StartupCwdKind::Cli => "cli",
        crate::utils::cwd::StartupCwdKind::Menu => "menu",
    }
    .to_string();
    Ok(StartupContext {
        cwd: cwd.to_string_lossy().to_string(),
        wind_file_in_cwd,
        kind,
    })
}

#[tauri::command]
pub fn get_last_opened_workspace_path(app_handle: tauri::AppHandle) -> Result<Option<String>, String> {
    let ptr = last_opened_pointer_path(&app_handle);
    if !ptr.is_file() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&ptr).map_err(|e| e.to_string())?;
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

/// 列出最近打开的 .ridge 路径，顺序新在前；只保留仍存在的文件，过滤掉已失效项。
#[tauri::command]
pub fn list_recent_workspaces(app_handle: tauri::AppHandle) -> Result<Vec<String>, String> {
    let raw = load_recent(&app_handle);
    let alive: Vec<String> = raw
        .into_iter()
        .filter(|p| PathBuf::from(p).is_file())
        .collect();
    // 若有过滤掉的，同步写回清理后的状态
    if alive.len() != load_recent(&app_handle).len() {
        save_recent(&app_handle, &alive);
    }
    Ok(alive)
}

#[tauri::command]
pub fn clear_recent_workspaces(app_handle: tauri::AppHandle) -> Result<(), String> {
    let p = recent_workspaces_path(&app_handle);
    if p.is_file() {
        std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn clear_last_opened_workspace_path(app_handle: tauri::AppHandle) -> Result<(), String> {
    let ptr = last_opened_pointer_path(&app_handle);
    if ptr.is_file() {
        std::fs::remove_file(&ptr).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_default_workspace_save_dir() -> Result<String, String> {
    Ok(default_save_dir().to_string_lossy().to_string())
}

#[derive(Debug, Serialize)]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub subdirs: Vec<String>,
}

/// 目录浏览：返回给定路径下的直接子目录列表 + 可返回的父目录。
/// 用于保存工作区对话框里的目录选择器：不存在的路径会被规范化到最近的已存在祖先。
#[tauri::command]
pub fn browse_directory(path: Option<String>) -> Result<DirListing, String> {
    let start = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
    };
    // 规范化：如果输入不存在，退回到最近的存在祖先。
    let mut cur = start.clone();
    while !cur.is_dir() {
        match cur.parent() {
            Some(p) => cur = p.to_path_buf(),
            None => {
                cur = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
                break;
            }
        }
    }
    let parent = cur.parent().map(|p| p.to_string_lossy().to_string());
    let mut subdirs: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&cur) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            // 过滤掉隐藏目录（Unix 约定 `.` 前缀）
            if name_str.starts_with('.') {
                continue;
            }
            subdirs.push(name_str);
        }
    }
    subdirs.sort_by_key(|s| s.to_lowercase());
    Ok(DirListing {
        path: cur.to_string_lossy().to_string(),
        parent,
        subdirs,
    })
}

// ─── Auto-save scheduler ───────────────────────────────────────────────────

struct AutoSaveState {
    /// 最近一次请求保存该工作区的时间戳。
    pending: HashMap<Uuid, Instant>,
    worker_running: bool,
}

static AUTO_SAVE: Lazy<Arc<Mutex<AutoSaveState>>> = Lazy::new(|| {
    Arc::new(Mutex::new(AutoSaveState {
        pending: HashMap::new(),
        worker_running: false,
    }))
});

/// 命令层在 cwd / 布局 / git 变化后调用。仅当工作区已关联 .ridge 文件时实际落盘，
/// 否则函数退化为无副作用的记录调用。
pub fn schedule_auto_save(state: &AppState, workspace_id: Uuid) {
    // Cheap gate: only panicked paths bother with this entry if there's an
    // associated file.
    let has_file = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.associated_file_path.as_ref())
            .is_some()
    };
    if !has_file {
        return;
    }
    let state_clone = state.clone();
    let mut guard = match AUTO_SAVE.lock() {
        Ok(g) => g,
        Err(e) => {
            tracing::error!(target: "ridge::autosave", error = %e, "mutex poisoned");
            return;
        }
    };
    guard.pending.insert(workspace_id, Instant::now());
    if guard.worker_running {
        return;
    }
    guard.worker_running = true;
    drop(guard);

    std::thread::Builder::new()
        .name("ridge-autosave".into())
        .spawn(move || auto_save_worker(state_clone))
        .ok();
}

fn auto_save_worker(state: AppState) {
    loop {
        std::thread::sleep(Duration::from_millis(AUTO_SAVE_DEBOUNCE_MS));
        let now = Instant::now();
        let due: Vec<Uuid> = {
            let mut guard = match AUTO_SAVE.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            let mut remove = Vec::new();
            for (&id, t) in guard.pending.iter() {
                if now.duration_since(*t) >= Duration::from_millis(AUTO_SAVE_DEBOUNCE_MS) {
                    remove.push(id);
                }
            }
            for id in &remove {
                guard.pending.remove(id);
            }
            if guard.pending.is_empty() {
                guard.worker_running = false;
            }
            remove
        };
        for id in due {
            if let Err(e) = write_workspace_snapshot(&state, id) {
                tracing::warn!(
                    target: "ridge::autosave",
                    workspace = %id,
                    error = %e,
                    "auto-save failed"
                );
            }
        }
        // 如果写盘过程中又被触发了新请求，保持 worker 运行；否则退出。
        let guard = match AUTO_SAVE.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !guard.worker_running {
            return;
        }
    }
}

fn write_workspace_snapshot(state: &AppState, workspace_id: Uuid) -> Result<(), String> {
    let (path, name) = {
        let map = state.workspaces.read();
        let ws = map.get(&workspace_id).ok_or_else(|| "workspace missing".to_string())?;
        let Some(path) = ws.associated_file_path.clone() else {
            return Ok(());
        };
        let name = state
            .workspace_names
            .read()
            .get(&workspace_id)
            .cloned()
            .unwrap_or_else(|| "workspace".to_string());
        (path, name)
    };
    let wf = snapshot_workspace(state, workspace_id, &name)?;
    let json = serde_json::to_vec_pretty(&wf).map_err(|e| e.to_string())?;
    atomic_write(&path, &json).map_err(|e| e.to_string())
}
