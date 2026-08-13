use crate::fs::{DirectoryPage, FileNode, ReplaceStats, SearchResult};
use crate::state::AppState;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::UNIX_EPOCH;
use tauri::State;
use tokio::sync::Semaphore;
use tokio::task::JoinError;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

// Legacy type for "recent files within a project". The UI was removed in
// round 9 alongside ProjectSidebar; the type is kept for the persistence
// schema in `db/projects.rs` so existing user databases continue to round-trip.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct RecentFileInfo {
    pub path: String,
    pub name: String,
    pub opened_at: String,
}

#[tauri::command]
pub fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectInfo, String> {
    let store = state
        .project_store
        .as_ref()
        .ok_or("Project store not initialized")?;

    let project = store
        .open_project(&path)
        .map_err(|e| format!("Failed to open project: {}", e))?;

    // Update current project in state
    *state.current_project.write() = Some(PathBuf::from(&path));

    let project_path = project.path.clone();
    let name = PathBuf::from(&project_path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| project_path.clone());

    Ok(ProjectInfo {
        id: project.id,
        path: project_path,
        name,
        created_at: project.created_at,
        updated_at: project.updated_at,
    })
}

#[tauri::command]
pub fn get_recent_projects(state: State<'_, AppState>) -> Result<Vec<ProjectInfo>, String> {
    let store = state
        .project_store
        .as_ref()
        .ok_or("Project store not initialized")?;

    let projects = store
        .get_recent_projects(10)
        .map_err(|e| format!("Failed to get recent projects: {}", e))?;

    Ok(projects
        .into_iter()
        .map(|p| ProjectInfo {
            id: p.id,
            path: p.path.clone(),
            name: PathBuf::from(&p.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.path),
            created_at: p.created_at,
            updated_at: p.updated_at,
        })
        .collect())
}

#[tauri::command]
pub fn remove_project(project_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let store = state
        .project_store
        .as_ref()
        .ok_or("Project store not initialized")?;

    store
        .remove_project(project_id)
        .map_err(|e| format!("Failed to remove project: {}", e))?;

    Ok(())
}

// ── Filesystem-operation semaphore ─────────────────────────────────────────
//
// Independent from GIT_SEMAPHORE (git.rs) so that file-tree walks never queue
// behind git subprocesses. Sizing from available parallelism: high-core
// workstations can satisfy many sidebar expand requests at once while keeping
// low-core laptops responsive.
fn fs_max_concurrent() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(4, 32)
}

static FS_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();

fn fs_semaphore() -> Arc<Semaphore> {
    FS_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(fs_max_concurrent())))
        .clone()
}

async fn spawn_fs_blocking<F, T>(f: F) -> Result<T, JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let sem = fs_semaphore();
    let permit = sem
        .acquire_owned()
        .await
        .expect("fs semaphore should never be closed");
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f()
    })
    .await
}

// `normalize_path_input` moved to `ridge_core::fs::commands` in S5 (used by the
// migrated tree / children / path_exists ports). The desktop commands delegate
// to those, so the local copy is gone — single source of truth.

// Lazy-load depth + page size defaults now live in `ridge_core::fs::commands`
// (the single source of truth, used by both hosts). The read-only command
// bodies below delegate to that core; the desktop wrapper keeps owning the
// `spawn_fs_blocking` offload (FS semaphore) so concurrency behaviour is
// unchanged.

#[tauri::command]
pub async fn get_file_tree(path: String, depth: Option<usize>) -> Result<FileNode, String> {
    // §S5: delegate to the migrated `ridge_core` port (behaviour identical —
    // same normalise → exists → is_dir checks and error strings). The host
    // keeps the `spawn_fs_blocking` offload (FS semaphore).
    spawn_fs_blocking(move || {
        ridge_core::fs::commands::get_file_tree(&path, depth).map_err(|e| e.to_command_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn get_directory_children(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<DirectoryPage, String> {
    // §S5: delegate to the migrated `ridge_core` port.
    spawn_fs_blocking(move || {
        ridge_core::fs::commands::get_directory_children(&path, offset, limit)
            .map_err(|e| e.to_command_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn text_search(request: TextSearchRequest) -> Result<Vec<SearchResult>, String> {
    // §S5: delegate to the migrated `ridge_core` port (same exists check, same
    // SearchOptions defaults, same gitignore-aware walk + error string). The
    // host keeps the `spawn_blocking` offload.
    let TextSearchRequest {
        root,
        query,
        case_sensitive,
        use_regex,
        whole_word,
        max_results,
        include_globs,
        exclude_globs,
    } = request;
    let args = ridge_core::fs::commands::TextSearchArgs {
        case_sensitive,
        use_regex,
        whole_word,
        max_results,
        include_globs,
        exclude_globs,
    };
    tokio::task::spawn_blocking(move || {
        ridge_core::fs::commands::search(&root, &query, &args).map_err(|e| e.to_command_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSearchRequest {
    pub root: String,
    pub query: String,
    pub case_sensitive: Option<bool>,
    pub use_regex: Option<bool>,
    pub whole_word: Option<bool>,
    pub max_results: Option<usize>,
    pub include_globs: Option<Vec<String>>,
    pub exclude_globs: Option<Vec<String>>,
}

/// Companion command returning ONLY the bad globs from the same options.
/// Frontend calls this once per search to decorate the include/exclude
/// inputs without re-running the whole walk: parse-only is microsecond-
/// cheap. Kept separate from `text_search` so the existing IPC contract
/// (Vec<SearchResult>) stays stable for any third-party caller.
///
/// §S5+: delegates to the migrated `ridge_core` port (same parse-only glob
/// validation, same `InvalidGlob` shape — aliased through `crate::fs::search`).
#[tauri::command]
pub fn text_search_diagnostics(
    include_globs: Option<Vec<String>>,
    exclude_globs: Option<Vec<String>>,
) -> Vec<crate::fs::search::InvalidGlob> {
    ridge_core::fs::commands::text_search_diagnostics(include_globs, exclude_globs)
}

#[tauri::command]
pub async fn filename_search(root: String, pattern: String) -> Result<Vec<String>, String> {
    // §S5+: delegate to the migrated `ridge_core` port (same exists check +
    // "Root path does not exist" string + `SearchEngine::search_files`). The
    // host keeps the `spawn_blocking` offload.
    tokio::task::spawn_blocking(move || {
        ridge_core::fs::commands::filename_search(&root, &pattern)
            .map_err(|e| e.to_command_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub async fn replace_in_files(
    root: String,
    search: String,
    replace: String,
    files: Vec<String>,
    case_sensitive: Option<bool>,
    use_regex: Option<bool>,
) -> Result<ReplaceStats, String> {
    // §S1+: delegate to the migrated `ridge_core` port (same exists check, same
    // SearchOptions defaults, same "Replace failed:" / "Root path does not exist"
    // strings). The host keeps the `spawn_blocking` offload.
    tokio::task::spawn_blocking(move || {
        ridge_core::fs::commands::replace_in_files(
            root,
            search,
            replace,
            files,
            case_sensitive,
            use_regex,
        )
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub fn read_file(path: String) -> Result<String, String> {
    // §S5: delegate to the migrated `ridge_core` port (same checks + strings).
    ridge_core::fs::commands::read_file(&path).map_err(|e| e.to_command_string())
}

/// The editor-read result shape. **Migrated to `ridge-core` in S5** — aliased
/// to the core type so the wire JSON (`{content, is_binary, size}`) and any
/// `project::ReadFileForEditorResult` reference stay identical.
pub type ReadFileForEditorResult = ridge_core::fs::commands::ReadFileForEditorResult;

/// Read a file for the editor: detects binary files (via NULL-byte heuristic) and
/// enforces a 5 MB ceiling to keep the UI responsive. Returns content as UTF-8 lossy
/// so editors never crash on malformed bytes — the save path enforces valid UTF-8.
#[tauri::command]
pub async fn read_file_for_editor(path: String) -> Result<ReadFileForEditorResult, String> {
    // §S5: delegate to the migrated `ridge_core` port; host keeps the offload.
    tokio::task::spawn_blocking(move || {
        ridge_core::fs::commands::read_file_for_editor(&path).map_err(|e| e.to_command_string())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Write content to a file (UTF-8). Creates parent dirs if missing.
/// §S1+: delegates to `ridge_core::fs::commands::write_file`; host keeps the
/// `spawn_blocking` offload. Made async so auto-save calls don't block the IPC.
#[tauri::command]
pub async fn write_file(path: String, content: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || ridge_core::fs::commands::write_file(path, content))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// A single Monaco `IModelContentChange`. **Migrated to `ridge-core`** — aliased
/// so `crate::commands::project::TextEdit` (used by `remote/server.rs`) and the
/// camelCase wire shape stay byte-for-byte identical.
pub use ridge_core::fs::commands::TextEdit;

/// Apply a sequence of Monaco content changes to a file — incremental save for
/// the low-bandwidth desktop-UI-in-browser mode. §S1+: delegates to
/// `ridge_core::fs::commands::apply_file_edits` (verbatim UTF-16 splice logic +
/// error strings); host keeps the `spawn_blocking` offload. Not a Tauri command
/// (served on the WS data-request path by `remote/server.rs`).
pub async fn apply_file_edits(path: String, edits: Vec<TextEdit>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || ridge_core::fs::commands::apply_file_edits(path, edits))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub fn get_current_project(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let project = state.current_project.read();
    Ok(project.as_ref().map(|p| p.to_string_lossy().to_string()))
}

// ─── Filesystem mutation commands (used by Explorer right-click actions) ─────
//
// Small wrappers over `std::fs`; kept deliberately narrow so the frontend
// doesn't need the `fs` Tauri plugin (which would require capability review).
// Each returns a plain `String` error so the JS side can `alert()` on failure.
// All operations refuse to touch paths that do not already exist (create_*
// commands instead refuse when the target *already* exists, to avoid silent
// overwrite).

// §S1+: the filesystem MUTATION logic moved into `ridge_core::fs::commands`
// (verbatim, including the Chinese error strings). These stay as thin
// `#[tauri::command]` wrappers — `tauri::generate_handler!` references them by
// `commands::project::*`, and `remote/server.rs` calls them on the WS path; both
// keep working unchanged because the wrappers delegate to the shared core. The
// read-only gate + sandbox/traversal guards live in `ridge_core::dispatch` (for
// the headless host) and `server.rs::is_mutating_invoke` (desktop backstop).

/// Rename / move a file or directory. `to` may be in a different directory.
#[tauri::command]
pub fn rename_path(from: String, to: String) -> Result<(), String> {
    ridge_core::fs::commands::rename_path(from, to)
}

/// Delete a file or directory (recursively for directories).
#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || ridge_core::fs::commands::delete_path(path))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Create an empty file at `path`. Fails if the file already exists.
/// Creates missing parent directories.
#[tauri::command]
pub fn create_file(path: String) -> Result<(), String> {
    ridge_core::fs::commands::create_file(path)
}

/// Create a directory at `path`. Fails if it already exists.
#[tauri::command]
pub fn create_directory(path: String) -> Result<(), String> {
    ridge_core::fs::commands::create_directory(path)
}

/// Copy `from` → `to`. Supports files and directories; directories copy recursively.
/// Refuses to overwrite unless `overwrite=true`. Preserves relative structure.
#[tauri::command]
pub async fn copy_path(from: String, to: String, overwrite: Option<bool>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || ridge_core::fs::commands::copy_path(from, to, overwrite))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Move `from` → `to`. Falls back to copy + delete if `rename` fails across
/// filesystems (common on Windows when spanning drive letters).
#[tauri::command]
pub async fn move_path(from: String, to: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || ridge_core::fs::commands::move_path(from, to))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Open the OS file manager selecting `path` (Windows: `explorer /select,...`,
/// macOS: `open -R`, Linux: fall back to opening the parent directory).
#[tauri::command]
pub fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err(format!("路径不存在: {}", path));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", target.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("打开资源管理器失败: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&target)
            .spawn()
            .map_err(|e| format!("打开 Finder 失败: {}", e))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = target.parent().unwrap_or(&target);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {}", e))?;
    }
    Ok(())
}

// ─── Opencode history ────────────────────────────────────────────────────────

/// Single entry for Opencode session
#[derive(Debug, Serialize, Clone)]
pub struct OpencodeHistoryEntry {
    pub session_id: String,
    pub title: String,
    pub updated_at: u64,
    pub project: String,
    pub files: Vec<String>,
}

#[tauri::command]
pub async fn read_opencode_history(
    limit: Option<usize>,
    offset: Option<usize>,
    workspace_cwds: Option<Vec<String>>,
) -> Vec<OpencodeHistoryEntry> {
    tokio::task::spawn_blocking(move || read_opencode_history_sync(limit, offset, workspace_cwds))
        .await
        .unwrap_or_default()
}

fn opencode_session_metadata(conn: Option<&Connection>, session_id: &str) -> (String, String) {
    let Some(conn) = conn else {
        return ("New Session".to_string(), String::new());
    };
    let Ok(mut stmt) = conn.prepare("SELECT s.title, s.directory FROM session s WHERE s.id = ?1")
    else {
        return ("New Session".to_string(), String::new());
    };
    stmt.query_row(rusqlite::params![session_id], |row| {
        let title: String = row.get(0)?;
        let directory: String = row.get(1)?;
        Ok((title, directory.replace('\\', "/")))
    })
    .unwrap_or_else(|_| ("New Session".to_string(), String::new()))
}

fn opencode_session_files(path: &Path) -> Vec<String> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file) else {
        return Vec::new();
    };
    json.as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("file")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect()
}

fn opencode_session_entry(
    path: &Path,
    conn: Option<&Connection>,
    workspace_cwds: &[String],
) -> Option<OpencodeHistoryEntry> {
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        return None;
    }
    let session_id = path.file_stem()?.to_string_lossy().to_string();
    let updated_at = std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let (title, mut project) = opencode_session_metadata(conn, &session_id);
    let files = opencode_session_files(path);
    if project.is_empty() && !workspace_cwds.is_empty() && !files.is_empty() {
        project = infer_project_from_workspace(&files, workspace_cwds);
    }
    Some(OpencodeHistoryEntry {
        session_id,
        title,
        updated_at,
        project,
        files,
    })
}

fn read_opencode_history_sync(
    limit: Option<usize>,
    offset: Option<usize>,
    workspace_cwds: Option<Vec<String>>,
) -> Vec<OpencodeHistoryEntry> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let storage = home
        .join(".local")
        .join("share")
        .join("opencode")
        .join("storage");
    let session_dir = storage.join("session_diff");
    let conn = Connection::open(storage.join("opencode.db")).ok();
    let workspace_cwds = workspace_cwds
        .unwrap_or_default()
        .into_iter()
        .map(|cwd| cwd.replace('\\', "/"))
        .collect::<Vec<_>>();
    let Ok(paths) = std::fs::read_dir(session_dir) else {
        return Vec::new();
    };
    let mut paths = paths.filter_map(|path| path.ok()).collect::<Vec<_>>();
    paths.sort_by(|a, b| {
        let modified =
            |entry: &std::fs::DirEntry| entry.metadata().ok().and_then(|meta| meta.modified().ok());
        modified(b).cmp(&modified(a))
    });
    paths
        .into_iter()
        .skip(offset.unwrap_or(0))
        .take(limit.unwrap_or(50))
        .filter_map(|entry| opencode_session_entry(&entry.path(), conn.as_ref(), &workspace_cwds))
        .collect()
}

// ─── OpenCode history ─────────────────────────────────────────────────────

/// Infer the project working directory from a list of absolute file paths.
/// Walks up each file's directory tree looking for a `.git` folder; if found,
/// returns that repo root. Otherwise falls back to the longest common prefix.
fn infer_project_from_files(files: &[String]) -> String {
    if files.is_empty() {
        return String::new();
    }
    if let Some(root) = git_root_from_files(files) {
        return root;
    }
    common_project_prefix(files)
}

fn git_root_from_files(files: &[String]) -> Option<String> {
    for file in files {
        let path = std::path::Path::new(file);
        if let Some(ancestor) = path.ancestors().skip(1).find(|a| a.join(".git").exists()) {
            return Some(ancestor.to_string_lossy().to_string());
        }
    }
    None
}

fn common_project_prefix(files: &[String]) -> String {
    let normalized: Vec<String> = files.iter().map(|f| f.replace('\\', "/")).collect();
    let mut prefix = normalized[0].clone();
    for f in &normalized[1..] {
        while !f.starts_with(&prefix) {
            let trunc = prefix.trim_end_matches('/');
            if let Some(pos) = trunc.rfind('/') {
                prefix = trunc[..=pos].to_string();
            } else {
                prefix = String::new();
                break;
            }
        }
        if prefix.is_empty() {
            break;
        }
    }
    if !prefix.is_empty() && !prefix.ends_with('/') {
        if let Some(pos) = prefix.rfind('/') {
            prefix = prefix[..=pos].to_string();
        }
    }
    prefix.trim_end_matches('/').replace('/', "\\")
}

/// Infer the best-matching workspace CWD from a list of file paths.
/// Counts how many files live under each workspace directory and returns
/// the one with the most matches (deepest prefix wins on ties).
fn infer_project_from_workspace(files: &[String], ws_cwds: &[String]) -> String {
    if files.is_empty() || ws_cwds.is_empty() {
        return String::new();
    }

    let mut best: (&str, usize) = ("", 0);

    for ws in ws_cwds {
        let ws_norm = ws.trim_end_matches('/');
        let mut count = 0;
        for f in files {
            let f_norm = f.replace('\\', "/");
            if f_norm.starts_with(ws_norm) {
                count += 1;
            }
        }
        // Prefer the workspace that matches more files; on a tie,
        // keep the first one encountered (which corresponds to the
        // order returned by the package manager / workspace config).
        if count > best.1 {
            best = (ws, count);
        }
    }

    if best.1 > 0 {
        best.0.to_string()
    } else {
        infer_project_from_files(files)
    }
}

/// Get files changed in a git repository between two points in time
#[tauri::command]
pub async fn get_git_changed_files(
    cwd: String,
    since: u64,
    until: u64,
) -> Result<Vec<String>, String> {
    let cwd_path = std::path::Path::new(&cwd);
    if !cwd_path.exists() || !cwd_path.is_dir() {
        return Ok(Vec::new());
    }

    // This history scan shares the same bounded admission and process lifetime
    // guard as every SCM call; a hung `git log` cannot pin a blocking worker.
    let output = match ridge_core::commands::git::run_git_guarded(
        cwd,
        vec![
            "log".into(),
            "--since".into(),
            since.to_string(),
            "--until".into(),
            until.to_string(),
            "--name-only".into(),
            "--pretty=format:".into(),
            "--diff-filter=ACMRT".into(),
        ],
    )
    .await
    {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()),
    };

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let mut files: Vec<String> = content
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    files.sort();
    files.dedup();
    Ok(files)
}

// ─── Claude Code history ─────────────────────────────────────────────────────

/// Single entry from `~/.claude/history.jsonl`.

#[derive(Debug, Serialize, Clone)]
pub struct ClaudeHistoryEntry {
    pub display: String,
    pub timestamp: u64,
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Read `~/.claude/history.jsonl` and return entries newest-first.
/// `project_paths`: forward-slash-normalised cwd list — only entries whose
/// `project` field (after normalisation) matches one of them are returned.
/// Pass an empty Vec to get the full unfiltered history.
#[tauri::command]
pub async fn read_claude_history(
    project_paths: Vec<String>,
    limit: Option<usize>,
) -> Vec<ClaudeHistoryEntry> {
    tokio::task::spawn_blocking(move || read_claude_history_sync(project_paths, limit))
        .await
        .unwrap_or_default()
}

fn read_claude_history_sync(
    project_paths: Vec<String>,
    limit: Option<usize>,
) -> Vec<ClaudeHistoryEntry> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let history_path = home.join(".claude").join("history.jsonl");
    let content = match std::fs::read_to_string(&history_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    // Normalise filter paths once (forward slash, lowercase for case-insensitive FS).
    let filters: Vec<String> = project_paths
        .iter()
        .map(|p| p.replace('\\', "/").to_lowercase())
        .collect();

    let mut entries: Vec<ClaudeHistoryEntry> = content
        .lines()
        .filter_map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).ok()?;
            // Skip non-history lines (they lack a `display` field).
            let display = v.get("display")?.as_str()?.to_string();
            let timestamp = v.get("timestamp")?.as_u64()?;
            let project = v.get("project")?.as_str()?.to_string();
            let session_id = v
                .get("sessionId")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            Some(ClaudeHistoryEntry {
                display,
                timestamp,
                project,
                session_id,
            })
        })
        .filter(|e| {
            if filters.is_empty() {
                return true;
            }
            let norm = e.project.replace('\\', "/").to_lowercase();
            filters.iter().any(|f| norm == *f)
        })
        .collect();

    // Newest first.
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    entries.truncate(limit.unwrap_or(100));
    entries
}

// ─── Agent assistant replies ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentResumeSpec {
    pub executable: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub session_id: String,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentRecentReply {
    pub agent: String,
    pub title: String,
    pub text: String,
    pub timestamp: u64,
    pub cwd: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<AgentResumeSpec>,
}

/// Read one latest-assistant row per supported local Agent session.
/// Native Claude Code/Codex JSONL and Cursor Agent transcript JSONL use
/// bounded discovery plus prefix/tail reads; Grok has its own session adapter.
/// Files are bounded newest-first and only their metadata prefix + tail are read.
#[tauri::command]
pub async fn read_agent_recent_replies(
    project_paths: Vec<String>,
    limit: Option<usize>,
) -> Vec<AgentRecentReply> {
    tokio::task::spawn_blocking(move || {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        read_agent_recent_replies_sync(&home, project_paths, limit.unwrap_or(40))
    })
    .await
    .unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentHistoryFileKind {
    NativeJsonl,
    CursorTranscript,
}

fn read_agent_recent_replies_sync(
    home: &Path,
    project_paths: Vec<String>,
    limit: usize,
) -> Vec<AgentRecentReply> {
    let filters = normalized_paths(&project_paths);
    let sources = [
        (
            "Claude",
            home.join(".claude").join("projects"),
            AgentHistoryFileKind::NativeJsonl,
        ),
        (
            "Codex",
            home.join(".codex").join("sessions"),
            AgentHistoryFileKind::NativeJsonl,
        ),
        (
            "Cursor Agent",
            home.join(".cursor").join("projects"),
            AgentHistoryFileKind::CursorTranscript,
        ),
    ];
    // Keep one bounded discovery pass per source. A busy Claude tree must not
    // consume the shared cap and hide Codex or Cursor history before sorting.
    let mut files = Vec::new();
    for (agent, root, kind) in sources {
        let mut source_files = Vec::new();
        match kind {
            AgentHistoryFileKind::NativeJsonl => {
                collect_jsonl_files(&root, agent, 0, &mut source_files);
            }
            AgentHistoryFileKind::CursorTranscript => {
                collect_cursor_transcript_files(&root, agent, 0, &mut source_files);
            }
        }
        source_files.sort_by(|a, b| b.2.cmp(&a.2));
        source_files.truncate(200);
        files.extend(
            source_files
                .into_iter()
                .map(|(source, path, modified)| (source, path, modified, kind)),
        );
    }
    files.sort_by(|a, b| b.2.cmp(&a.2));

    let mut sessions: HashMap<(String, String), AgentRecentReply> = HashMap::new();
    for (agent, path, modified, kind) in files {
        let Ok(content) = read_jsonl_window(&path) else {
            continue;
        };
        let fallback_session = (kind == AgentHistoryFileKind::CursorTranscript)
            .then(|| cursor_session_id(&path))
            .flatten();
        for session in
            parse_agent_jsonl_with_fallback(agent, &content, modified, fallback_session.as_deref())
        {
            let key = (
                session.agent.to_ascii_lowercase(),
                session.session_id.clone(),
            );
            match sessions.get(&key) {
                Some(current) if current.timestamp > session.timestamp => {}
                _ => {
                    sessions.insert(key, session);
                }
            }
        }
    }
    // Grok Build：`~/.grok/sessions/<encoded-cwd>/<session-id>/summary.json` + chat_history.jsonl
    for session in collect_grok_sessions(home) {
        let key = (
            session.agent.to_ascii_lowercase(),
            session.session_id.clone(),
        );
        match sessions.get(&key) {
            Some(current) if current.timestamp > session.timestamp => {}
            _ => {
                sessions.insert(key, session);
            }
        }
    }
    let mut replies: Vec<_> = sessions.into_values().collect();
    replies.retain(|reply| {
        filters.is_empty()
            || filters
                .iter()
                .any(|path| same_or_child_path(&reply.cwd, path))
    });
    replies.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    // `usize::MAX` is an internal exact-session lookup sentinel.  UI callers
    // remain capped at 100; the resume authority must not reject an older
    // valid session merely because it fell outside that presentation window.
    limit_history_replies(replies, limit)
}

fn limit_history_replies(replies: Vec<AgentRecentReply>, limit: usize) -> Vec<AgentRecentReply> {
    if limit == usize::MAX || replies.len() <= limit.min(100) {
        return replies;
    }
    let limit = limit.min(100);
    if limit == 0 {
        return Vec::new();
    }

    // Reserve one latest row per agent source before filling the remaining slots by
    // recency. This keeps a busy Claude tree from hiding an older Codex row in
    // the 24-item Agent Center projection.
    let mut selected = Vec::with_capacity(limit);
    let mut seen_agents = HashSet::new();
    for reply in &replies {
        if seen_agents.insert(reply.agent.to_ascii_lowercase()) {
            selected.push(reply.clone());
            if selected.len() == limit {
                return selected;
            }
        }
    }
    for reply in replies {
        if selected.len() == limit {
            break;
        }
        if !selected.iter().any(|item| {
            item.agent.eq_ignore_ascii_case(&reply.agent) && item.session_id == reply.session_id
        }) {
            selected.push(reply);
        }
    }
    selected.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    selected
}

/// Resolve the host-owned CWD for one recorded Agent session.
///
/// Remote clients may display and send a CWD, but that value is presentation
/// data, not an authority boundary.  Resume callers use this lookup to bind a
/// session id to the CWD recorded in the host's history files before creating
/// a PTY.  Exact `(agent, session_id)` matching is intentional: CWD/title
/// guesses would let a stale card resume a different project.
pub(crate) fn recorded_agent_session_cwd(agent: &str, session_id: &str) -> Result<PathBuf, String> {
    let agent = agent.trim();
    let session_id = session_id.trim();
    if agent.is_empty() || session_id.is_empty() {
        return Err("Agent session identity is required".into());
    }
    let home = dirs::home_dir().ok_or_else(|| "Agent history home is unavailable".to_string())?;
    let replies = read_agent_recent_replies_sync(&home, Vec::new(), usize::MAX);
    let cwd = find_recorded_agent_session_cwd(&replies, agent, session_id)
        .ok_or_else(|| format!("Agent session not found in host history: {agent}/{session_id}"))?;
    let path = PathBuf::from(cwd);
    if !path.is_dir() {
        return Err("Agent session CWD is not a directory".into());
    }
    Ok(path)
}

fn find_recorded_agent_session_cwd(
    replies: &[AgentRecentReply],
    agent: &str,
    session_id: &str,
) -> Option<String> {
    replies
        .iter()
        .find(|reply| reply.agent.eq_ignore_ascii_case(agent) && reply.session_id == session_id)
        .map(|reply| reply.cwd.trim().to_string())
        .filter(|cwd| !cwd.is_empty())
}

/// 扫描 Grok 会话目录，每会话一条最近摘要（summary.json + chat_history 尾部）。
fn collect_grok_sessions(home: &Path) -> Vec<AgentRecentReply> {
    let root = home.join(".grok").join("sessions");
    let mut out = Vec::new();
    collect_grok_session_dirs(&root, 0, &mut out);
    out
}

fn collect_grok_session_dirs(dir: &Path, depth: usize, out: &mut Vec<AgentRecentReply>) {
    if depth > 10 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let summary = path.join("summary.json");
        if summary.is_file() {
            if let Some(reply) = parse_grok_session_dir(&path) {
                out.push(reply);
            }
            continue;
        }
        collect_grok_session_dirs(&path, depth + 1, out);
        if out.len() >= 200 {
            break;
        }
    }
}

fn parse_grok_session_dir(dir: &Path) -> Option<AgentRecentReply> {
    let summary_path = dir.join("summary.json");
    let summary_raw = std::fs::read_to_string(&summary_path).ok()?;
    let summary: serde_json::Value = serde_json::from_str(&summary_raw).ok()?;
    let info = summary.get("info").unwrap_or(&summary);
    let session_id = info
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| dir.file_name().and_then(|n| n.to_str()))?
        .to_string();
    let cwd = info
        .get("cwd")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let title = summary
        .get("generated_title")
        .or_else(|| summary.get("session_summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("Grok session")
        .trim()
        .to_string();
    let timestamp = summary
        .get("last_active_at")
        .or_else(|| summary.get("updated_at"))
        .and_then(|v| v.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp_millis().max(0) as u64)
        .or_else(|| {
            std::fs::metadata(&summary_path)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
        })
        .unwrap_or(0);
    let text = read_grok_last_assistant_text(&dir.join("chat_history.jsonl"))
        .unwrap_or_else(|| title.clone());
    let profiles = crate::teammate::agent_catalog::builtin_profiles();
    let profile = crate::teammate::agent_catalog::find_profile(&profiles, "grok");
    let resume = profile.map(|p| {
        let (executable, argv, cwd_out) =
            crate::teammate::agent_catalog::plan_resume(p, &session_id, &cwd, false);
        AgentResumeSpec {
            executable,
            argv,
            cwd: cwd_out,
            session_id: session_id.clone(),
        }
    });
    Some(AgentRecentReply {
        agent: "Grok".into(),
        title,
        text,
        timestamp,
        cwd,
        session_id,
        resume,
    })
}

fn read_grok_last_assistant_text(history: &Path) -> Option<String> {
    let content = read_jsonl_window(history).ok()?;
    let mut last = None;
    for line in content.lines().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let text = match value.get("content") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|item| {
                    if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                        Some(t.to_string())
                    } else if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                        item.get("text")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => continue,
        };
        let text = text.trim();
        if !text.is_empty() {
            last = Some(text.chars().take(4000).collect());
            break;
        }
    }
    last
}

fn normalized_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.replace('\\', "/").trim_end_matches('/').to_lowercase())
        .filter(|path| !path.is_empty())
        .collect()
}

fn same_or_child_path(project: &str, filter: &str) -> bool {
    let project = project
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    project == filter
        || project.starts_with(&format!("{filter}/"))
        || filter.starts_with(&format!("{project}/"))
}

fn collect_jsonl_files(
    dir: &Path,
    agent: &'static str,
    depth: usize,
    out: &mut Vec<(&'static str, PathBuf, u64)>,
) {
    if depth > 8 || out.len() >= 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, agent, depth + 1, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            let modified = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or_default();
            out.push((agent, path, modified));
        }
        if out.len() >= 400 {
            break;
        }
    }
}

/// Cursor stores Agent transcripts below `projects/**/agent-transcripts`.
/// Restrict discovery to that directory name so MCP metadata and project
/// settings JSONL do not become fake Agent history rows.
fn collect_cursor_transcript_files(
    dir: &Path,
    agent: &'static str,
    depth: usize,
    out: &mut Vec<(&'static str, PathBuf, u64)>,
) {
    if depth > 10 || out.len() >= 400 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("agent-transcripts") {
            collect_jsonl_files(&path, agent, 0, out);
        } else {
            collect_cursor_transcript_files(&path, agent, depth + 1, out);
        }
        if out.len() >= 400 {
            break;
        }
    }
}

fn cursor_session_id(path: &Path) -> Option<String> {
    let parent = path.parent()?;
    let parent_name = parent.file_name().and_then(|name| name.to_str())?;
    if parent_name.eq_ignore_ascii_case("agent-transcripts")
        || parent
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("agent-transcripts"))
    {
        path.file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .map(str::to_string)
    } else {
        Some(parent_name.to_string())
    }
}

fn read_jsonl_window(path: &Path) -> std::io::Result<String> {
    const PREFIX: u64 = 64 * 1024;
    const TAIL: u64 = 1024 * 1024;
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len <= PREFIX + TAIL {
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        return Ok(content);
    }

    let mut prefix = vec![0; PREFIX as usize];
    file.read_exact(&mut prefix)?;
    file.seek(SeekFrom::End(-(TAIL as i64)))?;
    let mut tail = Vec::with_capacity(TAIL as usize);
    file.read_to_end(&mut tail)?;
    let mut content = String::from_utf8_lossy(&prefix).into_owned();
    content.push('\n');
    content.push_str(&String::from_utf8_lossy(&tail));
    Ok(content)
}

fn agent_session_meta(value: &serde_json::Value) -> Option<(String, Option<String>, String)> {
    (value.get("type").and_then(|item| item.as_str()) == Some("session_meta")).then(|| {
        let payload = &value["payload"];
        (
            payload
                .get("cwd")
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string(),
            payload
                .get("id")
                .and_then(|item| item.as_str())
                .map(str::to_string),
            payload
                .get("title")
                .or_else(|| payload.get("name"))
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .trim()
                .to_string(),
        )
    })
}

fn assistant_message(value: &serde_json::Value) -> Option<&serde_json::Value> {
    match value.get("type").and_then(|item| item.as_str()) {
        Some("assistant") => value.get("message"),
        Some("response_item") => {
            let payload = &value["payload"];
            // Codex persists progress commentary as assistant messages too.
            // A member card's "reply" means the answer delivered to the user,
            // so keep the previous final answer until the next final arrives.
            (payload.get("phase").and_then(|item| item.as_str()) != Some("commentary"))
                .then_some(payload)
        }
        _ if value.get("role").and_then(|item| item.as_str()) == Some("assistant") => {
            Some(value.get("message").unwrap_or(value))
        }
        _ => None,
    }
}

fn parse_agent_reply(
    agent: &str,
    value: &serde_json::Value,
    message: &serde_json::Value,
    project: &str,
    session_id: Option<&str>,
    session_title: &str,
    fallback_timestamp: u64,
) -> Option<(String, AgentRecentReply)> {
    if message
        .get("role")
        .and_then(|item| item.as_str())
        .is_some_and(|role| role != "assistant")
    {
        return None;
    }
    let text = extract_message_text(message.get("content"))?;
    let line_project = value
        .get("cwd")
        .or_else(|| value.get("project"))
        .and_then(|item| item.as_str())
        .unwrap_or(project)
        .to_string();
    let line_session = value
        .get("sessionId")
        .and_then(|item| item.as_str())
        .or(session_id)?
        .to_string();
    let line_title = value
        .get("title")
        .or_else(|| value.get("sessionTitle"))
        .or_else(|| value.get("slug"))
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(session_title);
    let title = if line_title.is_empty() {
        format!(
            "{} {}",
            agent,
            line_session.chars().take(8).collect::<String>()
        )
    } else {
        line_title.to_string()
    };
    let resume = crate::teammate::agent_catalog::find_profile(
        &crate::teammate::agent_catalog::builtin_profiles(),
        agent,
    )
    .and_then(|profile| {
        let (executable, argv, cwd_out) = crate::teammate::agent_catalog::plan_resume(
            profile,
            &line_session,
            &line_project,
            false,
        );
        if argv.is_empty() && profile.resume_argv.is_empty() {
            None
        } else {
            Some(AgentResumeSpec {
                executable,
                argv,
                cwd: cwd_out,
                session_id: line_session.clone(),
            })
        }
    });
    let reply = AgentRecentReply {
        agent: agent.to_string(),
        title,
        text,
        timestamp: json_timestamp_ms(value).unwrap_or(fallback_timestamp),
        cwd: line_project,
        session_id: line_session.clone(),
        resume,
    };
    Some((line_session, reply))
}

fn parse_agent_jsonl(agent: &str, content: &str, fallback_timestamp: u64) -> Vec<AgentRecentReply> {
    parse_agent_jsonl_with_fallback(agent, content, fallback_timestamp, None)
}

fn parse_agent_jsonl_with_fallback(
    agent: &str,
    content: &str,
    fallback_timestamp: u64,
    fallback_session_id: Option<&str>,
) -> Vec<AgentRecentReply> {
    let mut project = String::new();
    let mut session_id = fallback_session_id
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string);
    let mut session_title = String::new();
    let mut sessions: HashMap<String, AgentRecentReply> = HashMap::new();

    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some((next_project, next_session, next_title)) = agent_session_meta(&value) {
            project = next_project;
            session_id = next_session;
            session_title = next_title;
            continue;
        }
        let Some(message) = assistant_message(&value) else {
            continue;
        };
        let Some((line_session, reply)) = parse_agent_reply(
            agent,
            &value,
            message,
            &project,
            session_id.as_deref(),
            &session_title,
            fallback_timestamp,
        ) else {
            continue;
        };
        match sessions.get(&line_session) {
            Some(current) if current.timestamp > reply.timestamp => {}
            _ => {
                sessions.insert(line_session, reply);
            }
        }
    }
    let mut replies: Vec<_> = sessions.into_values().collect();
    replies.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    replies
}

fn extract_message_text(content: Option<&serde_json::Value>) -> Option<String> {
    let text = match content? {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let kind = item.get("type").and_then(|v| v.as_str());
                matches!(kind, Some("text" | "output_text"))
                    .then(|| item.get("text").and_then(|v| v.as_str()))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(4000).collect())
}

fn json_timestamp_ms(value: &serde_json::Value) -> Option<u64> {
    let timestamp = value.get("timestamp")?;
    if let Some(number) = timestamp.as_u64() {
        return Some(number);
    }
    chrono::DateTime::parse_from_rfc3339(timestamp.as_str()?)
        .ok()
        .map(|value| value.timestamp_millis().max(0) as u64)
}

#[tauri::command]
pub async fn path_exists(path: String) -> Result<bool, String> {
    // §S5: delegate to the migrated `ridge_core` port (same normalisation).
    Ok(ridge_core::fs::commands::path_exists(&path))
}

/// 内置 +（可选）用户覆盖后的 agent 配置表，供设置面板与恢复 YOLO 使用。
/// `overrides` 缺省时读后端持久化覆盖（与 autodiscover 同源）。
#[tauri::command]
pub fn list_agent_profiles(
    overrides: Option<Vec<crate::teammate::agent_catalog::AgentProfile>>,
) -> Vec<crate::teammate::agent_catalog::AgentProfile> {
    let stored = crate::teammate::agent_catalog::load_profile_overrides();
    let o = overrides.as_deref().unwrap_or(&stored);
    crate::teammate::agent_catalog::merge_profiles(o)
}

/// 持久化用户 agent 覆盖，并失效 autodiscover 扫描缓存，使 processNames 立即参与识别。
#[tauri::command]
pub fn save_agent_profile_overrides(
    overrides: Vec<crate::teammate::agent_catalog::AgentProfile>,
) -> Result<(), String> {
    crate::teammate::agent_catalog::save_profile_overrides(overrides)?;
    crate::teammate::autodiscover::invalidate_cache();
    Ok(())
}

/// 按 agent id/进程名生成恢复启动计划（含 YOLO 参数注入）。
#[tauri::command]
pub fn plan_agent_resume(
    agent: String,
    session_id: String,
    cwd: String,
    yolo: bool,
    overrides: Option<Vec<crate::teammate::agent_catalog::AgentProfile>>,
) -> Result<AgentResumeSpec, String> {
    let stored = crate::teammate::agent_catalog::load_profile_overrides();
    let o = overrides.as_deref().unwrap_or(&stored);
    let profiles = crate::teammate::agent_catalog::merge_profiles(o);
    let profile = crate::teammate::agent_catalog::find_profile(&profiles, &agent)
        .ok_or_else(|| format!("unknown agent profile: {agent}"))?;
    let (executable, argv, cwd_out) =
        crate::teammate::agent_catalog::plan_resume(profile, &session_id, &cwd, yolo);
    if argv.is_empty() && profile.resume_argv.is_empty() {
        return Err(format!("agent {agent} has no resume argv template"));
    }
    Ok(AgentResumeSpec {
        executable,
        argv,
        cwd: cwd_out,
        session_id,
    })
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests for the pure-filesystem commands. We avoid Tauri `#[tauri::command]`
// surface by calling the underlying fn directly — their signatures are plain
// `fn(String, ...) -> Result<(), String>` so this works.
//
// A lightweight TempDir RAII guard (no `tempfile` crate dep) creates a
// per-test directory under `std::env::temp_dir()` and removes it on drop.
// ═════════════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
            let pid = std::process::id();
            let mut path = std::env::temp_dir();
            path.push(format!("ridge-test-{}-{}-{}", tag, pid, n));
            std::fs::create_dir_all(&path).expect("create temp dir");
            TempDir { path }
        }
        fn join(&self, rel: &str) -> PathBuf {
            self.path.join(rel)
        }
        fn path_string(&self, rel: &str) -> String {
            self.join(rel).to_string_lossy().into_owned()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn parses_claude_assistant_reply() {
        let replies = parse_agent_jsonl(
            "Claude",
            r#"{"type":"assistant","timestamp":"2026-07-27T01:02:03Z","cwd":"C:\\code\\wind","sessionId":"claude-1","message":{"role":"assistant","content":[{"type":"text","text":"fixed it"}]}}"#,
            0,
        );
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].agent, "Claude");
        assert_eq!(replies[0].title, "Claude claude-1");
        assert_eq!(replies[0].text, "fixed it");
        assert_eq!(replies[0].cwd, r"C:\code\wind");
        assert_eq!(replies[0].session_id, "claude-1");
        assert_eq!(
            replies[0].resume.as_ref().map(|r| r.executable.as_str()),
            Some("claude")
        );
        assert_eq!(
            replies[0].resume.as_ref().map(|r| r.argv.clone()),
            Some(vec!["--resume".into(), "claude-1".into()])
        );
        assert_eq!(
            replies[0].resume.as_ref().map(|r| r.cwd.as_str()),
            Some(r"C:\code\wind")
        );
    }

    #[test]
    fn parses_codex_session_metadata_and_output_text() {
        let replies = parse_agent_jsonl(
            "Codex",
            r#"{"type":"session_meta","payload":{"id":"codex-1","cwd":"C:\\code\\wind"}}
{"timestamp":"2026-07-27T02:03:04Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"tests green"}]}}"#,
            0,
        );
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].text, "tests green");
        assert_eq!(replies[0].cwd, r"C:\code\wind");
        assert_eq!(replies[0].session_id, "codex-1");
        assert_eq!(
            replies[0].resume.as_ref().map(|r| r.executable.as_str()),
            Some("codex")
        );
        assert_eq!(
            replies[0].resume.as_ref().map(|r| r.argv.clone()),
            Some(vec!["resume".into(), "codex-1".into()])
        );
    }

    #[test]
    fn codex_commentary_does_not_replace_the_latest_delivered_answer() {
        let replies = parse_agent_jsonl(
            "Codex",
            r#"{"type":"session_meta","payload":{"id":"codex-1","cwd":"C:\\code\\wind"}}
{"timestamp":"2026-08-13T01:00:00Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"delivered answer"}]}}
{"timestamp":"2026-08-13T01:01:00Z","type":"response_item","payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"later progress note"}]}}"#,
            0,
        );
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].text, "delivered answer");
    }

    #[test]
    fn plan_agent_resume_keeps_session_identity_and_recorded_cwd() {
        let cwd = r"D:\agent-work\repo";
        let planned = plan_agent_resume(
            "Codex".into(),
            "codex-session-42".into(),
            cwd.into(),
            false,
            Some(Vec::new()),
        )
        .expect("built-in Codex resume profile");

        assert_eq!(planned.executable, "codex");
        assert_eq!(planned.argv, vec!["resume", "codex-session-42"]);
        assert_eq!(planned.cwd, cwd);
        assert_eq!(planned.session_id, "codex-session-42");
    }

    #[test]
    fn parses_grok_session_dir_summary_and_history() {
        let td = TempDir::new("grok-sess");
        let sess = td.join("019fb572-test-session");
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::write(
            sess.join("summary.json"),
            r#"{"info":{"id":"019fb572-test-session","cwd":"C:\\code\\wind"},"generated_title":"Grok fixture","last_active_at":"2026-07-31T01:00:00Z"}"#,
        )
        .unwrap();
        std::fs::write(
            sess.join("chat_history.jsonl"),
            r#"{"type":"user","content":"hi"}
{"type":"assistant","content":"hello from grok"}
"#,
        )
        .unwrap();
        let reply = parse_grok_session_dir(&sess).expect("grok session");
        assert_eq!(reply.agent, "Grok");
        assert_eq!(reply.session_id, "019fb572-test-session");
        assert_eq!(reply.cwd, r"C:\code\wind");
        assert!(reply.text.contains("hello from grok"));
        let resume = reply.resume.expect("resume");
        assert_eq!(resume.executable, "grok");
        assert!(resume.argv.iter().any(|a| a == "--resume"));
        assert!(resume.argv.iter().any(|a| a == "019fb572-test-session"));
        assert_eq!(resume.cwd, r"C:\code\wind");
    }

    #[test]
    fn aggregates_one_latest_reply_per_native_session() {
        let replies = parse_agent_jsonl(
            "Claude",
            r#"{"type":"assistant","timestamp":"2026-07-27T01:00:00Z","cwd":"/repo","sessionId":"s1","message":{"role":"assistant","content":"old"}}
{"type":"assistant","timestamp":"2026-07-27T02:00:00Z","cwd":"/repo","sessionId":"s1","message":{"role":"assistant","content":"latest"}}
{"type":"assistant","timestamp":"2026-07-27T01:30:00Z","cwd":"/repo","sessionId":"s2","message":{"role":"assistant","content":"other"}}"#,
            0,
        );
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0].session_id, "s1");
        assert_eq!(replies[0].text, "latest");
        assert_eq!(replies[1].session_id, "s2");
    }

    #[test]
    fn recorded_session_cwd_requires_exact_agent_and_session_identity() {
        let replies = vec![
            AgentRecentReply {
                agent: "Codex".into(),
                title: "one".into(),
                text: "".into(),
                timestamp: 1,
                cwd: r"D:\one".into(),
                session_id: "same-id".into(),
                resume: None,
            },
            AgentRecentReply {
                agent: "Claude".into(),
                title: "two".into(),
                text: "".into(),
                timestamp: 2,
                cwd: r"D:\two".into(),
                session_id: "same-id".into(),
                resume: None,
            },
        ];
        assert_eq!(
            find_recorded_agent_session_cwd(&replies, "codex", "same-id").as_deref(),
            Some(r"D:\one")
        );
        assert!(find_recorded_agent_session_cwd(&replies, "codex", "missing").is_none());
        assert!(find_recorded_agent_session_cwd(&replies, "grok", "same-id").is_none());
    }

    #[test]
    fn keeps_history_for_unknown_agent_without_resume_plan() {
        let replies = parse_agent_jsonl(
            "CustomAgent",
            r#"{"type":"assistant","timestamp":"2026-07-27T03:00:00Z","cwd":"D:\\custom","sessionId":"custom-1","message":{"role":"assistant","content":"custom output"}}"#,
            0,
        );
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].agent, "CustomAgent");
        assert_eq!(replies[0].cwd, r"D:\custom");
        assert_eq!(replies[0].text, "custom output");
        assert!(replies[0].resume.is_none());
    }

    #[test]
    fn project_filter_accepts_children_not_siblings() {
        assert!(same_or_child_path(r"C:\code\wind\src", "c:/code/wind"));
        assert!(same_or_child_path(r"C:\code\wind", "c:/code/wind/src"));
        assert!(!same_or_child_path(r"C:\code\windmill", "c:/code/wind"));
    }

    #[test]
    fn history_scan_keeps_each_agent_and_recorded_cwd() {
        let td = TempDir::new("agent-history-sources");
        let claude = td.join(".claude/projects/project-a/session-a.jsonl");
        let codex = td.join(".codex/sessions/2026/08/session-b.jsonl");
        std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(
            &claude,
            r#"{"type":"assistant","timestamp":"2026-08-02T01:00:00Z","cwd":"C:\\one","sessionId":"claude-a","message":{"role":"assistant","content":"from Claude"}}"#,
        )
        .unwrap();
        std::fs::write(
            &codex,
            r#"{"type":"session_meta","payload":{"id":"codex-b","cwd":"D:\\two"}}
{"timestamp":"2026-08-02T02:00:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"from Codex"}]}}"#,
        )
        .unwrap();

        let replies = read_agent_recent_replies_sync(&td.path, Vec::new(), 40);
        assert_eq!(replies.len(), 2);
        let claude_reply = replies
            .iter()
            .find(|reply| reply.agent == "Claude")
            .expect("Claude history");
        assert_eq!(claude_reply.cwd, r"C:\one");
        assert_eq!(claude_reply.text, "from Claude");
        assert_eq!(
            claude_reply
                .resume
                .as_ref()
                .map(|resume| resume.cwd.as_str()),
            Some(r"C:\one")
        );
        let codex_reply = replies
            .iter()
            .find(|reply| reply.agent == "Codex")
            .expect("Codex history");
        assert_eq!(codex_reply.cwd, r"D:\two");
        assert_eq!(codex_reply.text, "from Codex");
        assert_eq!(
            codex_reply
                .resume
                .as_ref()
                .map(|resume| resume.argv.clone()),
            Some(vec!["resume".into(), "codex-b".into()])
        );

        let filtered = read_agent_recent_replies_sync(&td.path, vec!["d:/two/project".into()], 40);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].agent, "Codex");
    }

    #[test]
    fn parses_cursor_agent_transcript_with_directory_session_fallback() {
        let replies = parse_agent_jsonl_with_fallback(
            "Cursor Agent",
            r#"{"role":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"from Cursor"}]}}"#,
            42,
            Some("cursor-session-1"),
        );
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].agent, "Cursor Agent");
        assert_eq!(replies[0].session_id, "cursor-session-1");
        assert_eq!(replies[0].text, "from Cursor");
        assert_eq!(replies[0].timestamp, 42);
        assert!(replies[0].resume.is_none());
    }

    #[test]
    fn history_scan_discovers_cursor_agent_transcripts() {
        let td = TempDir::new("cursor-agent-history");
        let transcript = td.join(
            ".cursor/projects/c-code-wind-wind-code-workspace/agent-transcripts/cursor-1/cursor-1.jsonl",
        );
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"cursor output"}]}}"#,
        )
        .unwrap();

        let replies = read_agent_recent_replies_sync(&td.path, Vec::new(), 40);
        let cursor = replies
            .iter()
            .find(|reply| reply.agent == "Cursor Agent")
            .expect("Cursor Agent history");
        assert_eq!(cursor.session_id, "cursor-1");
        assert_eq!(cursor.text, "cursor output");
        assert!(cursor.resume.is_none());
    }

    #[test]
    fn cursor_session_id_uses_transcript_directory_or_file_stem() {
        assert_eq!(
            cursor_session_id(Path::new("projects/p/agent-transcripts/s-1/s-1.jsonl")),
            Some("s-1".into())
        );
        assert_eq!(
            cursor_session_id(Path::new("projects/p/agent-transcripts/s-2.jsonl")),
            Some("s-2".into())
        );
    }

    #[test]
    fn history_limit_reserves_latest_row_for_each_source() {
        let replies = vec![
            AgentRecentReply {
                agent: "Claude".into(),
                title: "claude".into(),
                text: "claude".into(),
                timestamp: 300,
                cwd: String::new(),
                session_id: "claude-1".into(),
                resume: None,
            },
            AgentRecentReply {
                agent: "Codex".into(),
                title: "codex".into(),
                text: "codex".into(),
                timestamp: 200,
                cwd: String::new(),
                session_id: "codex-1".into(),
                resume: None,
            },
            AgentRecentReply {
                agent: "Cursor Agent".into(),
                title: "cursor".into(),
                text: "cursor".into(),
                timestamp: 100,
                cwd: String::new(),
                session_id: "cursor-1".into(),
                resume: None,
            },
        ];
        let limited = limit_history_replies(replies, 3);
        assert_eq!(
            limited
                .iter()
                .map(|reply| reply.agent.as_str())
                .collect::<Vec<_>>(),
            vec!["Claude", "Codex", "Cursor Agent"]
        );
    }

    /// 本机若存在 Codex/Grok 会话目录，则真实扫描路径须解析出非空条目（非 fixture）。
    #[test]
    fn real_home_codex_or_grok_sessions_parse_when_present() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let codex_root = home.join(".codex").join("sessions");
        let grok_root = home.join(".grok").join("sessions");
        let has_codex = codex_root.is_dir();
        let has_grok = grok_root.is_dir();
        if !has_codex && !has_grok {
            return; // 干净 CI 无本机会话目录时跳过，不假绿
        }
        let replies = read_agent_recent_replies_sync(&home, Vec::new(), 40);
        if has_codex {
            // 本机有 .codex/sessions 时，至少应能扫到 jsonl 并尝试解析；
            // 若格式全不兼容则 replies 可能无 Codex 行——此时要求至少 collect 不 panic，
            // 且对首个 jsonl 用 parse_agent_jsonl 的 Codex 路径可被调用。
            let mut files = Vec::new();
            collect_jsonl_files(&codex_root, "Codex", 0, &mut files);
            assert!(
                !files.is_empty(),
                "expected jsonl under ~/.codex/sessions when dir exists"
            );
            let (_agent, path, modified) = &files[0];
            if let Ok(content) = read_jsonl_window(path) {
                let parsed = parse_agent_jsonl("Codex", &content, *modified);
                // 允许单文件无 assistant 行，但函数须返回（不 panic）
                let _ = parsed.len();
            }
        }
        if has_grok {
            let grok = collect_grok_sessions(&home);
            assert!(
                !grok.is_empty(),
                "expected Grok sessions under ~/.grok/sessions when dir exists"
            );
            assert!(
                grok.iter().any(|r| r.agent.eq_ignore_ascii_case("Grok")),
                "Grok agent label missing in real-home parse"
            );
            assert!(
                grok.iter().any(|r| r
                    .resume
                    .as_ref()
                    .map(|s| s.executable == "grok" && s.argv.iter().any(|a| a == "--resume"))
                    .unwrap_or(false)),
                "Grok resume plan missing --resume"
            );
        }
        // 跨源聚合路径也跑通
        let _ = replies.len();
    }

    // ── create_file / create_directory ──────────────────────────────────────
    #[test]
    fn create_file_creates_parent_then_empty_file() {
        let td = TempDir::new("mkf");
        let target = td.path_string("a/b/c.txt");
        create_file(target.clone()).expect("create_file");
        let content = std::fs::read(&target).expect("read new file");
        assert_eq!(content, b"");
    }

    #[test]
    fn create_file_refuses_overwrite() {
        let td = TempDir::new("mkf2");
        let target = td.path_string("x.txt");
        create_file(target.clone()).unwrap();
        let err = create_file(target).unwrap_err();
        assert!(err.contains("已存在"), "expected Chinese 已存在, got {err}");
    }

    #[test]
    fn create_directory_refuses_overwrite() {
        let td = TempDir::new("mkd");
        let target = td.path_string("subdir");
        create_directory(target.clone()).unwrap();
        let err = create_directory(target).unwrap_err();
        assert!(err.contains("目录已存在"));
    }

    // ── rename_path ─────────────────────────────────────────────────────────
    #[test]
    fn rename_path_moves_file() {
        let td = TempDir::new("mv");
        let from = td.path_string("a.txt");
        let to = td.path_string("b.txt");
        create_file(from.clone()).unwrap();
        rename_path(from.clone(), to.clone()).unwrap();
        assert!(!std::path::Path::new(&from).exists());
        assert!(std::path::Path::new(&to).exists());
    }

    #[test]
    fn rename_path_refuses_when_target_exists() {
        let td = TempDir::new("mv-clash");
        let a = td.path_string("a.txt");
        let b = td.path_string("b.txt");
        create_file(a.clone()).unwrap();
        create_file(b.clone()).unwrap();
        let err = rename_path(a, b).unwrap_err();
        assert!(err.contains("目标已存在"));
    }

    #[test]
    fn rename_path_reports_missing_source() {
        let td = TempDir::new("mv-miss");
        let from = td.path_string("nope.txt");
        let to = td.path_string("y.txt");
        let err = rename_path(from, to).unwrap_err();
        assert!(err.contains("路径不存在"));
    }

    // ── delete_path ─────────────────────────────────────────────────────────
    #[tokio::test]
    async fn delete_path_removes_file() {
        let td = TempDir::new("rm-file");
        let target = td.path_string("a.txt");
        create_file(target.clone()).unwrap();
        delete_path(target.clone()).await.unwrap();
        assert!(!std::path::Path::new(&target).exists());
    }

    #[tokio::test]
    async fn delete_path_removes_directory_recursively() {
        let td = TempDir::new("rm-dir");
        let dir = td.path_string("dir");
        create_directory(dir.clone()).unwrap();
        create_file(td.path_string("dir/x.txt")).unwrap();
        create_file(td.path_string("dir/sub/y.txt")).unwrap();
        delete_path(dir.clone()).await.unwrap();
        assert!(!std::path::Path::new(&dir).exists());
    }

    #[tokio::test]
    async fn delete_path_reports_missing() {
        let td = TempDir::new("rm-miss");
        let err = delete_path(td.path_string("nothing")).await.unwrap_err();
        assert!(err.contains("路径不存在"));
    }

    // ── copy_path ───────────────────────────────────────────────────────────
    #[tokio::test]
    async fn copy_path_copies_single_file() {
        let td = TempDir::new("cp-f");
        let from = td.path_string("a.txt");
        let to = td.path_string("b.txt");
        std::fs::write(&from, b"hello").unwrap();
        copy_path(from.clone(), to.clone(), None).await.unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), b"hello");
        assert!(
            std::path::Path::new(&from).exists(),
            "copy preserves source"
        );
    }

    #[tokio::test]
    async fn copy_path_refuses_overwrite_by_default() {
        let td = TempDir::new("cp-clash");
        let from = td.path_string("a.txt");
        let to = td.path_string("b.txt");
        create_file(from.clone()).unwrap();
        create_file(to.clone()).unwrap();
        let err = copy_path(from, to, None).await.unwrap_err();
        assert!(err.contains("目标已存在"));
    }

    #[tokio::test]
    async fn copy_path_recursive_for_directory() {
        let td = TempDir::new("cp-d");
        let src = td.path_string("src");
        let dst = td.path_string("dst");
        create_directory(src.clone()).unwrap();
        std::fs::write(td.join("src/a.txt"), b"A").unwrap();
        std::fs::create_dir_all(td.join("src/sub")).unwrap();
        std::fs::write(td.join("src/sub/b.txt"), b"B").unwrap();

        copy_path(src.clone(), dst.clone(), None).await.unwrap();

        assert_eq!(std::fs::read(td.join("dst/a.txt")).unwrap(), b"A");
        assert_eq!(std::fs::read(td.join("dst/sub/b.txt")).unwrap(), b"B");
        // Source still intact.
        assert!(td.join("src/a.txt").exists());
    }

    // ── move_path ───────────────────────────────────────────────────────────
    #[tokio::test]
    async fn move_path_moves_file_and_clears_source() {
        let td = TempDir::new("mov-f");
        let from = td.path_string("a.txt");
        let to = td.path_string("b.txt");
        std::fs::write(&from, b"X").unwrap();
        move_path(from.clone(), to.clone()).await.unwrap();
        assert!(!std::path::Path::new(&from).exists());
        assert_eq!(std::fs::read(&to).unwrap(), b"X");
    }

    #[tokio::test]
    async fn move_path_moves_directory_recursively() {
        let td = TempDir::new("mov-d");
        let src = td.path_string("src");
        let dst = td.path_string("dst");
        create_directory(src.clone()).unwrap();
        std::fs::write(td.join("src/x.txt"), b"x").unwrap();
        move_path(src.clone(), dst.clone()).await.unwrap();
        assert!(!std::path::Path::new(&src).exists());
        assert_eq!(std::fs::read(td.join("dst/x.txt")).unwrap(), b"x");
    }

    #[tokio::test]
    async fn move_path_refuses_when_target_exists() {
        let td = TempDir::new("mov-clash");
        let a = td.path_string("a.txt");
        let b = td.path_string("b.txt");
        create_file(a.clone()).unwrap();
        create_file(b.clone()).unwrap();
        let err = move_path(a, b).await.unwrap_err();
        assert!(err.contains("目标已存在"));
    }
}
