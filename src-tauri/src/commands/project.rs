use crate::db::ProjectStore;
use crate::fs::{FileTree, FileNode, SearchEngine, SearchResult, ReplaceStats, SearchOptions};
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
        // `read_dir` 会读到 Wind 自己的运行目录。补回分隔符还原真正的根。
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

#[tauri::command]
pub fn get_file_tree(path: String, depth: Option<usize>) -> Result<FileNode, String> {
    let root = normalize_path_input(&path);
    if !root.exists() {
        return Err(format!("Path does not exist: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("Path is not a directory: {}", root.display()));
    }

    let max_depth = depth.unwrap_or(5);
    FileTree::build(&root, max_depth)
        .map_err(|e| format!("Failed to build file tree: {}", e))
}

#[tauri::command]
pub fn get_directory_children(path: String) -> Result<Vec<FileNode>, String> {
    let dir = normalize_path_input(&path);
    if !dir.exists() {
        return Err(format!("Path does not exist: {}", dir.display()));
    }
    if !dir.is_dir() {
        return Err(format!("Path is not a directory: {}", dir.display()));
    }

    FileTree::get_children(&dir)
        .map_err(|e| format!("Failed to get directory contents: {}", e))
}

#[tauri::command]
pub fn text_search(
    root: String,
    query: String,
    case_sensitive: Option<bool>,
    use_regex: Option<bool>,
    whole_word: Option<bool>,
    max_results: Option<usize>,
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
    };

    Ok(SearchEngine::search_text(&root_path, &query, &options))
}

#[tauri::command]
pub fn filename_search(root: String, pattern: String) -> Result<Vec<String>, String> {
    let root_path = PathBuf::from(&root);
    if !root_path.exists() {
        return Err("Root path does not exist".to_string());
    }

    Ok(SearchEngine::search_files(&root_path, &pattern))
}

#[tauri::command]
pub fn replace_in_files(
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
    };

    SearchEngine::replace_in_files(&root_path, &search, &replace, &files, &options)
        .map_err(|e| format!("Replace failed: {}", e))
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
pub fn read_file_for_editor(path: String) -> Result<ReadFileForEditorResult, String> {
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
    // Binary heuristic: NULL byte in first 8 KB, or a very high ratio of non-printable bytes
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
}

/// Write content to a file (UTF-8). Creates parent dirs if missing.
#[tauri::command]
pub fn write_file(path: String, content: String) -> Result<(), String> {
    let file_path = PathBuf::from(&path);
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;
        }
    }
    std::fs::write(&file_path, content).map_err(|e| format!("写入文件失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn get_current_project(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let project = state.current_project.read();
    Ok(project.as_ref().map(|p| p.to_string_lossy().to_string()))
}