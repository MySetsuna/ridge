use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, Debouncer};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// 单进程最多 watch 多少个 root（每个 root 对应一个独立 debouncer）。
/// inotify (Linux) 每递归子目录占一个 watch，配合 mono-repo 必须留出余地。
const MAX_WATCHED_ROOTS: usize = 32;
/// 单次 emit 的路径数量上限，超过则降级为 `coalesced=true`，前端做整树 reload。
const MAX_PATHS_PER_EMIT: usize = 256;
/// debounce 窗口：编辑器/文件树要跟手，比 git 的 500ms 更短。
const DEBOUNCE_MS: u64 = 250;

/// 路径段黑名单——路径中任何一段命中即过滤，不 emit 给前端。
/// 不在 watcher level 排除（notify 不支持），只能在事件回调里压制噪音。
const SEGMENT_BLACKLIST: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    ".svelte-kit",
    ".nuxt",
    "__pycache__",
    ".venv",
    ".cache",
];

fn is_ignored_filename(name: &str) -> bool {
    if name == ".DS_Store" || name == "Thumbs.db" {
        return true;
    }
    // Emacs lock/auto-save、vim swap、通用 tmp/backup
    if name.starts_with(".#") {
        return true;
    }
    if name.len() >= 3 && name.starts_with('#') && name.ends_with('#') {
        return true;
    }
    if name.ends_with(".tmp") || name.ends_with(".swp") || name.ends_with('~') {
        return true;
    }
    false
}

fn should_ignore(path: &Path) -> bool {
    for comp in path.components() {
        if let Some(s) = comp.as_os_str().to_str() {
            if SEGMENT_BLACKLIST.contains(&s) {
                return true;
            }
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if is_ignored_filename(name) {
            return true;
        }
    }
    false
}

/// 与 CLAUDE.md "CWD path normalization" 保持一致：windows 反斜杠统一为正斜杠。
fn normalize_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[derive(Clone, Serialize)]
struct FsChangedPayload {
    root: String,
    paths: Vec<String>,
    /// true 表示本批次路径过多被丢弃，前端应该对该 root 整树刷新。
    coalesced: bool,
}

#[derive(Deserialize)]
pub struct WatchSpec {
    pub path: String,
    /// `true` 监听整棵子树（典型用法：Explorer column 的 cwd）；
    /// `false` 仅监听单个文件（典型用法：编辑器打开的"外部文件"）。
    pub recursive: bool,
}

/// 维护一组活跃 watcher，按 root 路径索引。
/// Debouncer 句柄持有期间 watcher 才存活；释放即停止监听。
pub struct FsWatcher {
    /// root → (debouncer, recursive)
    debouncers: Mutex<HashMap<PathBuf, (Debouncer<notify::RecommendedWatcher>, bool)>>,
}

impl FsWatcher {
    pub fn new() -> Self {
        Self {
            debouncers: Mutex::new(HashMap::new()),
        }
    }

    /// 幂等：已在监听的 root + 相同 recursive 标志直接返回 Ok。
    /// 超过 [`MAX_WATCHED_ROOTS`] 时拒绝注册并打印警告。
    pub fn watch(&self, root: PathBuf, recursive: bool, app: AppHandle) -> notify::Result<()> {
        let mut map = self.debouncers.lock();
        if map.contains_key(&root) {
            return Ok(());
        }
        if map.len() >= MAX_WATCHED_ROOTS {
            eprintln!(
                "FsWatcher: refused to register {} — already watching {} roots (cap)",
                root.display(),
                MAX_WATCHED_ROOTS
            );
            return Ok(());
        }

        let root_for_emit = root.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(DEBOUNCE_MS),
            move |events: Result<Vec<DebouncedEvent>, notify::Error>| {
                let Ok(events) = events else {
                    return;
                };
                let mut seen: HashSet<PathBuf> = HashSet::new();
                let mut paths: Vec<String> = Vec::new();
                for ev in events {
                    if !seen.insert(ev.path.clone()) {
                        continue;
                    }
                    if should_ignore(&ev.path) {
                        continue;
                    }
                    paths.push(normalize_path(&ev.path));
                }
                let coalesced = paths.len() > MAX_PATHS_PER_EMIT;
                if coalesced {
                    // 路径太多，丢弃明细让前端整树 reload，避免传输巨大数组。
                    paths.clear();
                } else if paths.is_empty() {
                    // 全部被过滤掉，无需打扰前端。
                    return;
                }
                let payload = FsChangedPayload {
                    root: normalize_path(&root_for_emit),
                    paths,
                    coalesced,
                };
                // §web-remote: also relay to desktop-browser clients (file tree /
                // editor live refresh). Forward before the move into app.emit.
                crate::remote::forward_event(&app, "fs-changed", &payload);
                let _ = app.emit("fs-changed", payload);
            },
        )?;

        let mode = if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        debouncer.watcher().watch(&root, mode)?;
        map.insert(root, (debouncer, recursive));
        Ok(())
    }

    /// 同步活跃 watcher 集合：移除不在 `live` 中的；新加入的注册。
    /// 若同一 root 的 recursive 标志变了，按"先卸后装"处理。
    pub fn sync_watched(&self, live: &[(PathBuf, bool)], app: &AppHandle) {
        {
            let live_map: HashMap<&PathBuf, bool> = live.iter().map(|(p, r)| (p, *r)).collect();
            let mut map = self.debouncers.lock();
            map.retain(|root, (_, rec)| live_map.get(root).is_some_and(|nr| *nr == *rec));
        }
        for (root, recursive) in live {
            let _ = self.watch(root.clone(), *recursive, app.clone());
        }
    }
}

// notify 的 RecommendedWatcher 是 Send 不 Sync；我们用 Mutex 序列化所有访问。
// 与 GitWatcher 同样的安全前提。
unsafe impl Send for FsWatcher {}
unsafe impl Sync for FsWatcher {}

// ─── Tauri command ────────────────────────────────────────────────────────────

/// 同步前端期望监听的路径集合。前端在 Explorer 列变更或编辑器打开/关闭外部文件时调用。
#[tauri::command]
pub async fn start_watching_paths(
    roots: Vec<WatchSpec>,
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), String> {
    let live: Vec<(PathBuf, bool)> = roots
        .into_iter()
        .map(|w| (PathBuf::from(w.path), w.recursive))
        .collect();
    state.fs_watcher.sync_watched(&live, &app);
    Ok(())
}
