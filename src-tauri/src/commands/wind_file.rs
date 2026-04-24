//! `.wind` 工作区文件：一个工作区的完整可移植快照。
//!
//! 文件结构（JSON，`version` 作为前向兼容锚点）：
//! ```json
//! {
//!   "version": 1,
//!   "name": "My Workspace",
//!   "saved_at": "2026-04-24T10:00:00Z",
//!   "pane_tree": { ... 工作区 PaneTree JSON ... },
//!   "git_repos": ["C:/code/wind"],
//!   "index_path": null
//! }
//! ```
//!
//! 自动保存：
//! - 后端持有防抖调度器 `AutoSaveScheduler`，命令层在 pane_tree / cwd / git
//!   状态发生变化后调用 `schedule(workspace_id)`；
//! - 调度器合并 `AUTO_SAVE_DEBOUNCE_MS` 内的触发，到期后后台线程读快照并
//!   原子写入 `.wind` 文件，主线程零阻塞。

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

const WIND_FILE_VERSION: u32 = 1;
const AUTO_SAVE_DEBOUNCE_MS: u64 = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindFile {
    pub version: u32,
    pub name: String,
    pub saved_at: String,
    pub pane_tree: serde_json::Value,
    #[serde(default)]
    pub git_repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_path: Option<String>,
}

/// 默认保存目录：`<home>/wind-workspaces/`（不存在时创建）。
fn default_save_dir() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("wind-workspaces");
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
        if pb.extension().and_then(|s| s.to_str()) == Some("wind") {
            pb
        } else if pb.is_dir() || p.ends_with(std::path::MAIN_SEPARATOR) {
            pb.join(format!("{}.wind", sanitize_filename(name)))
        } else {
            // 用户传的是 "<dir>/<stem>" 之类，补 `.wind` 扩展。
            let mut with_ext = pb.clone();
            with_ext.set_extension("wind");
            with_ext
        }
    } else {
        default_save_dir().join(format!("{}.wind", sanitize_filename(name)))
    }
}

fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("wind.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

fn last_opened_pointer_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("last_workspace.txt")
}

fn set_last_opened(app: &tauri::AppHandle, path: &Path) {
    let _ = std::fs::write(last_opened_pointer_path(app), path.to_string_lossy().as_bytes());
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkspaceSaveInfo {
    pub workspace_id: String,
    pub file_path: Option<String>,
    pub name: Option<String>,
}

/// 构造当前工作区的 WindFile 快照。
fn snapshot_workspace(state: &AppState, workspace_id: Uuid, name: &str) -> Result<WindFile, String> {
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
    Ok(WindFile {
        version: WIND_FILE_VERSION,
        name: name.to_string(),
        saved_at: Utc::now().to_rfc3339(),
        pane_tree: tree_json,
        git_repos,
        index_path: None,
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
        return Err(format!(".wind 文件不存在：{path}"));
    }
    let raw = std::fs::read(&file_path).map_err(|e| e.to_string())?;
    let wf: WindFile = serde_json::from_slice(&raw).map_err(|e| format!(".wind 格式非法: {e}"))?;
    if wf.version != WIND_FILE_VERSION {
        return Err(format!(
            ".wind 版本 {} 与当前 ({}) 不匹配",
            wf.version, WIND_FILE_VERSION
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
    {
        let mut map = state.workspaces.write();
        map.insert(
            new_id,
            crate::state::Workspace {
                pane_tree: tree,
                terminals: HashMap::new(),
                teammate_tmux_pane_cursor: 0,
                teammate_pane_titles: HashMap::new(),
                pane_sizes: HashMap::new(),
                last_pane_index: None,
                created_at: std::time::SystemTime::now(),
                teammate_pane_states: HashMap::new(),
                teammate_agent_pane_map: HashMap::new(),
                associated_file_path: Some(file_path.clone()),
            },
        );
    }
    state.workspace_order.write().push(new_id);
    state.workspace_names.write().insert(new_id, wf.name.clone());
    *state.active_workspace.write() = new_id;
    set_last_opened(&app_handle, &file_path);
    Ok(new_id.to_string())
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

/// 命令层在 cwd / 布局 / git 变化后调用。仅当工作区已关联 .wind 文件时实际落盘，
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
            tracing::error!(target: "wind::autosave", error = %e, "mutex poisoned");
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
        .name("wind-autosave".into())
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
                    target: "wind::autosave",
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
