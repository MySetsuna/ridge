use crate::fs::{DirectoryPage, FileNode, FileTree, ReplaceStats, SearchEngine, SearchOptions, SearchResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

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
    let store = state.project_store.as_ref()
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
    let store = state.project_store.as_ref()
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
    let store = state.project_store.as_ref()
        .ok_or("Project store not initialized")?;

    store
        .remove_project(project_id)
        .map_err(|e| format!("Failed to remove project: {}", e))?;

    Ok(())
}

/// 把前端传来的路径统一成系统原生形式，修复 Windows 下 `C:/a/b\c` 这类正/反斜杠混用
/// 时 `PathBuf::exists()` 偶发返回 false 的问题，也顺手去掉尾部分隔符、trim 空白。
fn normalize_path_input(input: &str) -> PathBuf {
    let trimmed = input.trim().trim_end_matches(|c: char| c == '/' || c == '\\');
    #[cfg(windows)]
    {
        // 统一为反斜杠；Windows API 接受两者，但混用时 Rust stdlib 某些内部路径比较会失败。
        let mut s = trimmed.replace('/', "\\");
        // 驱动器根（`C:/` / `C:\`）在上面被把尾分隔符削掉后会退化成 `C:`，
        // 而 Windows 里裸的 `C:` 不是"C 盘根"，是"进程最近一次在 C 盘的 cwd"，
        // `read_dir` 会读到 Ridge 自己的运行目录。补回分隔符还原真正的根。
        if s.len() == 2
            && s.as_bytes()[0].is_ascii_alphabetic()
            && s.as_bytes()[1] == b':'
        {
            s.push('\\');
        }
        PathBuf::from(s)
    }
    #[cfg(not(windows))]
    {
        // 对 POSIX 根 `/` 做同样的守卫：trim_end_matches 会把它削成空串。
        if trimmed.is_empty() && input.contains('/') {
            PathBuf::from("/")
        } else {
            PathBuf::from(trimmed)
        }
    }
}

/// Default lazy-load depth for the Explorer's initial tree request. Just
/// the root + its direct children; descendants load on first expand via
/// `get_directory_children`. Was 5 in the eager-load era.
const DEFAULT_TREE_DEPTH: usize = 1;
/// Default page size for `get_directory_children`. Set to balance "see
/// most directories in one shot" against "first paint stays snappy on
/// `node_modules`-class folders" (~ 1500 entries → 8 pages).
const DEFAULT_CHILDREN_PAGE_SIZE: usize = 200;

#[tauri::command]
pub async fn get_file_tree(path: String, depth: Option<usize>) -> Result<FileNode, String> {
    let root = normalize_path_input(&path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("Path is not a directory: {}", root.display()));
    }

    let max_depth = depth.unwrap_or(DEFAULT_TREE_DEPTH);
    tokio::task::spawn_blocking(move || {
        FileTree::build(&root, max_depth)
            .map_err(|e| format!("Failed to build file tree: {}", e))
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
    let dir = normalize_path_input(&path);
    if !dir.exists() {
        return Err(format!("Path does not exist: {}", dir.display()));
    }
    if !dir.is_dir() {
        return Err(format!("Path is not a directory: {}", dir.display()));
    }

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(DEFAULT_CHILDREN_PAGE_SIZE);
    tokio::task::spawn_blocking(move || {
        FileTree::page_children(&dir, offset, limit)
            .map_err(|e| format!("Failed to get directory contents: {}", e))
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
    let root_path = PathBuf::from(&root);
    if !root_path.exists() {
        return Err("Root path does not exist".to_string());
    }

    let options = SearchOptions {
        case_sensitive: case_sensitive.unwrap_or(false),
        use_regex: use_regex.unwrap_or(false),
        whole_word: whole_word.unwrap_or(false),
        include_hidden: false,
        max_results: max_results.unwrap_or(1000),
        include_globs: include_globs.unwrap_or_default(),
        exclude_globs: exclude_globs.unwrap_or_default(),
    };

    tokio::task::spawn_blocking(move || Ok(SearchEngine::search_text(&root_path, &query, &options)))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Companion command returning ONLY the bad globs from the same options.
/// Frontend calls this once per search to decorate the include/exclude
/// inputs without re-running the whole walk: parse-only is microsecond-
/// cheap. Kept separate from `text_search` so the existing IPC contract
/// (Vec<SearchResult>) stays stable for any third-party caller.
#[tauri::command]
pub fn text_search_diagnostics(
    include_globs: Option<Vec<String>>,
    exclude_globs: Option<Vec<String>>,
) -> Vec<crate::fs::search::InvalidGlob> {
    use crate::fs::search::InvalidGlob;
    use glob::Pattern;
    let mut bad: Vec<InvalidGlob> = Vec::new();
    for s in include_globs.unwrap_or_default() {
        if let Err(e) = Pattern::new(&s) {
            bad.push(InvalidGlob {
                pattern: s,
                error: e.to_string(),
                field: "include".to_string(),
            });
        }
    }
    for s in exclude_globs.unwrap_or_default() {
        if let Err(e) = Pattern::new(&s) {
            bad.push(InvalidGlob {
                pattern: s,
                error: e.to_string(),
                field: "exclude".to_string(),
            });
        }
    }
    bad
}

#[tauri::command]
pub async fn filename_search(root: String, pattern: String) -> Result<Vec<String>, String> {
    let root_path = PathBuf::from(&root);
    if !root_path.exists() {
        return Err("Root path does not exist".to_string());
    }

    tokio::task::spawn_blocking(move || Ok(SearchEngine::search_files(&root_path, &pattern)))
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
    let root_path = PathBuf::from(&root);
    if !root_path.exists() {
        return Err("Root path does not exist".to_string());
    }

    let options = SearchOptions {
        case_sensitive: case_sensitive.unwrap_or(false),
        use_regex: use_regex.unwrap_or(false),
        whole_word: false,
        include_hidden: false,
        max_results: usize::MAX,
        include_globs: Vec::new(),
        exclude_globs: Vec::new(),
    };

    tokio::task::spawn_blocking(move || {
        SearchEngine::replace_in_files(&root_path, &search, &replace, &files, &options)
            .map_err(|e| format!("Replace failed: {}", e))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[tauri::command]
pub fn read_file(path: String) -> Result<String, String> {
    let file_path = PathBuf::from(&path);
    if !file_path.exists() {
        return Err("File does not exist".to_string());
    }
    if !file_path.is_file() {
        return Err("Path is not a file".to_string());
    }

    std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read file: {}", e))
}

#[derive(serde::Serialize)]
pub struct ReadFileForEditorResult {
    pub content: String,
    pub is_binary: bool,
    pub size: u64,
}

/// Read a file for the editor: detects binary files (via NULL-byte heuristic) and
/// enforces a 5 MB ceiling to keep the UI responsive. Returns content as UTF-8 lossy
/// so editors never crash on malformed bytes — the save path enforces valid UTF-8.
#[tauri::command]
pub async fn read_file_for_editor(path: String) -> Result<ReadFileForEditorResult, String> {
    tokio::task::spawn_blocking(move || {
        let file_path = PathBuf::from(&path);
        if !file_path.exists() { return Err("File does not exist".to_string()); }
        if !file_path.is_file() { return Err("Path is not a file".to_string()); }

        let metadata = std::fs::metadata(&file_path).map_err(|e| e.to_string())?;
        let size = metadata.len();
        const MAX: u64 = 5 * 1024 * 1024;
        if size > MAX {
            return Err(format!("文件过大 ({:.2} MB)，编辑器上限 5 MB", size as f64 / 1024.0 / 1024.0));
        }

        let bytes = std::fs::read(&file_path).map_err(|e| e.to_string())?;
        let probe = &bytes[..bytes.len().min(8192)];
        let has_null = probe.iter().any(|&b| b == 0);
        let non_text = probe
            .iter()
            .filter(|&&b| b < 0x09 || (b > 0x0D && b < 0x20))
            .count();
        let ratio = if probe.is_empty() { 0.0 } else { non_text as f64 / probe.len() as f64 };
        let is_binary = has_null || ratio > 0.30;

        if is_binary {
            return Ok(ReadFileForEditorResult { content: String::new(), is_binary: true, size });
        }

        let content = String::from_utf8_lossy(&bytes).into_owned();
        Ok(ReadFileForEditorResult { content, is_binary: false, size })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Write content to a file (UTF-8). Creates parent dirs if missing.
/// Made async so auto-save calls don't block the IPC thread.
#[tauri::command]
pub async fn write_file(path: String, content: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let file_path = PathBuf::from(&path);
        if let Some(parent) = file_path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
            }
        }
        std::fs::write(&file_path, content).map_err(|e| format!("写入文件失败: {}", e))?;
        Ok(())
    })
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

/// Rename / move a file or directory. `to` may be in a different directory.
#[tauri::command]
pub fn rename_path(from: String, to: String) -> Result<(), String> {
    let from_path = PathBuf::from(&from);
    let to_path = PathBuf::from(&to);
    if !from_path.exists() {
        return Err(format!("路径不存在: {}", from));
    }
    if to_path.exists() {
        return Err(format!("目标已存在: {}", to));
    }
    std::fs::rename(&from_path, &to_path).map_err(|e| format!("重命名失败: {}", e))?;
    Ok(())
}

/// Delete a file or directory (recursively for directories).
#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let target = PathBuf::from(&path);
        if !target.exists() {
            return Err(format!("路径不存在: {}", path));
        }
        let meta = std::fs::symlink_metadata(&target).map_err(|e| format!("读取元数据失败: {}", e))?;
        if meta.is_dir() {
            std::fs::remove_dir_all(&target).map_err(|e| format!("删除目录失败: {}", e))?;
        } else {
            std::fs::remove_file(&target).map_err(|e| format!("删除文件失败: {}", e))?;
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Create an empty file at `path`. Fails if the file already exists.
/// Creates missing parent directories.
#[tauri::command]
pub fn create_file(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if target.exists() {
        return Err(format!("文件已存在: {}", path));
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
        }
    }
    std::fs::write(&target, []).map_err(|e| format!("创建文件失败: {}", e))?;
    Ok(())
}

/// Create a directory at `path`. Fails if it already exists.
#[tauri::command]
pub fn create_directory(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if target.exists() {
        return Err(format!("目录已存在: {}", path));
    }
    std::fs::create_dir_all(&target).map_err(|e| format!("创建目录失败: {}", e))?;
    Ok(())
}

/// Copy `from` → `to`. Supports files and directories; directories copy recursively.
/// Refuses to overwrite unless `overwrite=true`. Preserves relative structure.
#[tauri::command]
pub async fn copy_path(from: String, to: String, overwrite: Option<bool>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || copy_path_sync(from, to, overwrite))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

fn copy_path_sync(from: String, to: String, overwrite: Option<bool>) -> Result<(), String> {
    let from_path = PathBuf::from(&from);
    let to_path = PathBuf::from(&to);
    if !from_path.exists() {
        return Err(format!("源路径不存在: {}", from));
    }
    let overwrite = overwrite.unwrap_or(false);
    if to_path.exists() && !overwrite {
        return Err(format!("目标已存在: {}", to));
    }
    if let Some(parent) = to_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
        }
    }
    let meta = std::fs::symlink_metadata(&from_path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.is_dir() {
        // Recursive copy via walkdir. Mirror the tree relative to `from_path`.
        std::fs::create_dir_all(&to_path).map_err(|e| format!("创建目标目录失败: {}", e))?;
        for entry in walkdir::WalkDir::new(&from_path).min_depth(1) {
            let entry = entry.map_err(|e| format!("遍历源目录失败: {}", e))?;
            let rel = entry
                .path()
                .strip_prefix(&from_path)
                .map_err(|e| format!("strip_prefix: {}", e))?;
            let target = to_path.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target)
                    .map_err(|e| format!("创建子目录失败 ({}): {}", target.display(), e))?;
            } else {
                if let Some(parent) = target.parent() {
                    if !parent.exists() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            format!("创建目标父目录失败 ({}): {}", parent.display(), e)
                        })?;
                    }
                }
                std::fs::copy(entry.path(), &target)
                    .map_err(|e| format!("复制文件失败 ({}): {}", target.display(), e))?;
            }
        }
    } else {
        std::fs::copy(&from_path, &to_path).map_err(|e| format!("复制失败: {}", e))?;
    }
    Ok(())
}

/// Move `from` → `to`. Falls back to copy + delete if `rename` fails across
/// filesystems (common on Windows when spanning drive letters).
#[tauri::command]
pub async fn move_path(from: String, to: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || move_path_sync(from, to))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

fn move_path_sync(from: String, to: String) -> Result<(), String> {
    let from_path = PathBuf::from(&from);
    let to_path = PathBuf::from(&to);
    if !from_path.exists() {
        return Err(format!("源路径不存在: {}", from));
    }
    if to_path.exists() {
        return Err(format!("目标已存在: {}", to));
    }
    if let Some(parent) = to_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建父目录失败: {}", e))?;
        }
    }
    if std::fs::rename(&from_path, &to_path).is_ok() {
        return Ok(());
    }
    // Cross-device fallback: copy then delete source.
    copy_path_sync(from.clone(), to.clone(), Some(false))?;
    let meta = std::fs::symlink_metadata(&from_path).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.is_dir() {
        std::fs::remove_dir_all(&from_path).map_err(|e| format!("删除源目录失败: {}", e))?;
    } else {
        std::fs::remove_file(&from_path).map_err(|e| format!("删除源文件失败: {}", e))?;
    }
    Ok(())
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
) -> Vec<OpencodeHistoryEntry> {
    tokio::task::spawn_blocking(move || {
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return Vec::new(),
        };
        let session_dir = home.join(".local").join("share").join("opencode").join("storage").join("session_diff");
        
        let mut entries = Vec::new();
        if let Ok(paths) = std::fs::read_dir(session_dir) {
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
                    let session_id = path.path().file_stem().unwrap().to_string_lossy().to_string();
                    let metadata = std::fs::metadata(path.path()).ok();
                    let updated_at = metadata.and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    
                    let mut files = Vec::new();
                    let mut project = String::new();
                    let mut title = "New Session".to_string();

                    if let Ok(file) = std::fs::File::open(path.path()) {
                        if let Ok(json) = serde_json::from_reader::<_, serde_json::Value>(file) {
                            // Opencode session format: it's an array of change objects
                            if let Some(arr) = json.as_array() {
                                for item in arr {
                                    if let Some(f) = item.get("file").and_then(|p| p.as_str()) {
                                        files.push(f.to_string());
                                    }
                                }
                                // Try to find a common project root from the files or metadata if available
                                // For now we assume the session file itself might have some clues or we use a default
                                project = "Opencode Project".to_string(); 
                            }
                        }
                    }
                    
                    entries.push(OpencodeHistoryEntry { 
                        session_id, 
                        title, 
                        updated_at,
                        project,
                        files 
                    });
                }
            }
        }
        entries
    }).await.unwrap_or_default()
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
        let since_str = format!("{}", since);
        let until_str = format!("{}", until);
        
        // Use git log to find changed files in the time range
        let output = Command::new("git")
            .current_dir(&cwd)
            .args(&[
                "log", 
                "--since", &since_str, 
                "--until", &until_str, 
                "--name-only", 
                "--pretty=format:",
                "--diff-filter=ACMRT"
            ])
            .output()
            .map_err(|e| format!("Failed to execute git: {}", e))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).to_string());
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

fn read_claude_history_sync(project_paths: Vec<String>, limit: Option<usize>) -> Vec<ClaudeHistoryEntry> {
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
            let session_id = v.get("sessionId")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            Some(ClaudeHistoryEntry { display, timestamp, project, session_id })
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
        assert!(std::path::Path::new(&from).exists(), "copy preserves source");
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