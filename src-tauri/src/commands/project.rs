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

#[tauri::command]
pub fn get_file_tree(path: String, depth: Option<usize>) -> Result<FileNode, String> {
    let root = PathBuf::from(&path);
    if !root.exists() {
        return Err("Path does not exist".to_string());
    }
    if !root.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    let max_depth = depth.unwrap_or(5);
    FileTree::build(&root, max_depth)
        .map_err(|e| format!("Failed to build file tree: {}", e))
}

#[tauri::command]
pub fn get_directory_children(path: String) -> Result<Vec<FileNode>, String> {
    let dir = PathBuf::from(&path);
    if !dir.exists() {
        return Err("Path does not exist".to_string());
    }
    if !dir.is_dir() {
        return Err("Path is not a directory".to_string());
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

#[tauri::command]
pub fn get_current_project(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let project = state.current_project.read();
    Ok(project.as_ref().map(|p| p.to_string_lossy().to_string()))
}