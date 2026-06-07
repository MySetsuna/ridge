use crate::fs::{DirectoryPage, FileNode, ReplaceStats, SearchResult};
use crate::state::AppState;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
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
pub async fn text_search(
    root: String,
    query: String,
    case_sensitive: Option<bool>,
    use_regex: Option<bool>,
    whole_word: Option<bool>,
    max_results: Option<usize>,
    include_globs: Option<Vec<String>>,
    exclude_globs: Option<Vec<String>>,
) -> Result<Vec<SearchResult>, String> {
    // §S5: delegate to the migrated `ridge_core` port (same exists check, same
    // SearchOptions defaults, same gitignore-aware walk + error string). The
    // host keeps the `spawn_blocking` offload.
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
        ridge_core::fs::commands::filename_search(&root, &pattern).map_err(|e| e.to_command_string())
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
    tokio::task::spawn_blocking(move || {
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return Vec::new(),
        };
        let session_dir = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("storage")
            .join("session_diff");
        let db_path = home
            .join(".local")
            .join("share")
            .join("opencode")
            .join("storage")
            .join("opencode.db");

        // Try to open the opencode SQLite database for session metadata
        let conn = Connection::open(&db_path).ok();

        // Normalise workspace CWDs for matching
        let ws_cwds: Vec<String> = workspace_cwds
            .unwrap_or_default()
            .into_iter()
            .map(|c| c.replace('\\', "/"))
            .collect();

        let mut entries = Vec::new();
        if let Ok(paths) = std::fs::read_dir(&session_dir) {
            let mut file_paths: Vec<_> = paths.filter_map(|p| p.ok()).collect();
            // Sort by modification time descending
            file_paths.sort_by(|a, b| {
                let a_meta = a.metadata().ok().and_then(|m| m.modified().ok());
                let b_meta = b.metadata().ok().and_then(|m| m.modified().ok());
                b_meta.cmp(&a_meta)
            });

            let offset = offset.unwrap_or(0);
            let limit = limit.unwrap_or(50);

            for path in file_paths.into_iter().skip(offset).take(limit) {
                if path.path().extension().and_then(|s| s.to_str()) == Some("json") {
                    let session_id = path
                        .path()
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .to_string();
                    let metadata = std::fs::metadata(path.path()).ok();
                    let updated_at = metadata
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);

                    let mut files = Vec::new();
                    let mut project = String::new();
                    let mut title = "New Session".to_string();

                    // Read session metadata from opencode SQLite database
                    if let Some(ref conn) = conn {
                        if let Ok(mut stmt) = conn
                            .prepare("SELECT s.title, s.directory FROM session s WHERE s.id = ?1")
                        {
                            if let Ok(row) = stmt.query_row(rusqlite::params![session_id], |row| {
                                let db_title: String = row.get(0)?;
                                let directory: String = row.get(1)?;
                                Ok((db_title, directory))
                            }) {
                                title = row.0;
                                project = row.1.replace('\\', "/");
                            }
                        }
                    }

                    // Read files from session_diff JSON
                    if let Ok(file) = std::fs::File::open(path.path()) {
                        if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file) {
                            if let Some(arr) = json.as_array() {
                                for item in arr {
                                    if let Some(f) = item.get("file").and_then(|p| p.as_str()) {
                                        files.push(f.to_string());
                                    }
                                }
                            }
                        }
                    }

                    // Fallback: infer project CWD by matching relative file paths
                    // against known workspace directories
                    if project.is_empty() && !ws_cwds.is_empty() && !files.is_empty() {
                        project = infer_project_from_workspace(&files, &ws_cwds);
                    }

                    entries.push(OpencodeHistoryEntry {
                        session_id,
                        title,
                        updated_at,
                        project,
                        files,
                    });
                }
            }
        }
        entries
    })
    .await
    .unwrap_or_default()
}

// ─── OpenCode history ─────────────────────────────────────────────────────

/// Infer the project working directory from a list of absolute file paths.
/// Walks up each file's directory tree looking for a `.git` folder; if found,
/// returns that repo root. Otherwise falls back to the longest common prefix.
fn infer_project_from_files(files: &[String]) -> String {
    if files.is_empty() {
        return String::new();
    }

    // Try to find a git repo root from any file
    for f in files {
        let path = std::path::Path::new(f);
        if let Some(ancestor) = path.ancestors().skip(1).find(|a| a.join(".git").exists()) {
            return ancestor.to_string_lossy().to_string();
        }
    }

    // Fallback: longest common prefix of all file paths
    // Normalize separators first
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
    // If prefix looks like a file path (not ending in /), get its parent
    if !prefix.is_empty() && !prefix.ends_with('/') {
        if let Some(pos) = prefix.rfind('/') {
            prefix = prefix[..=pos].to_string();
        }
    }
    // Convert back to native path format
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
    use std::process::Command;

    tokio::task::spawn_blocking(move || {
        // Validate CWD exists and is a directory
        let cwd_path = std::path::Path::new(&cwd);
        if !cwd_path.exists() || !cwd_path.is_dir() {
            return Ok(Vec::new());
        }

        let since_str = format!("{}", since);
        let until_str = format!("{}", until);

        // Use git log to find changed files in the time range
        let output = match Command::new("git")
            .current_dir(&cwd)
            .args(&[
                // Match git.rs `git_cmd()`: don't take optional index locks for
                // this read-only history scan (uniform policy; `log` itself
                // doesn't lock the index, but keeps every background git read
                // consistent and future-proof).
                "--no-optional-locks",
                "log",
                "--since",
                &since_str,
                "--until",
                &until_str,
                "--name-only",
                "--pretty=format:",
                "--diff-filter=ACMRT",
            ])
            .output()
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
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
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

#[tauri::command]
pub async fn path_exists(path: String) -> Result<bool, String> {
    // §S5: delegate to the migrated `ridge_core` port (same normalisation).
    Ok(ridge_core::fs::commands::path_exists(&path))
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
