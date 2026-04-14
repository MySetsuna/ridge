use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use once_cell::sync::Lazy;

/// 与前端 GitGraph 约定一致；后续可换为 git2 拓扑数据。
#[derive(Clone, Debug, Serialize)]
pub struct CommitNode {
    pub lane: usize,
    pub parents: Vec<usize>,
    pub message: String,
}

/// Git diff 文件变更信息
#[derive(Clone, Debug, Serialize)]
pub struct DiffFile {
    pub path: String,
    pub additions: i32,
    pub deletions: i32,
    pub status: String, // "M", "A", "D", "R", "C"
}

/// Git diff 跟踪状态
#[derive(Clone, Debug, Serialize, Default)]
pub struct GitDiffStatus {
    pub files: Vec<DiffFile>,
    pub total_additions: i32,
    pub total_deletions: i32,
    pub is_git_repo: bool,
}

/// 跟踪每个 pane 的工作目录和上次状态
static PANE_WORKDIRS: Lazy<Mutex<HashMap<String, (String, String)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// 跟踪每个 pane 的最后已知状态 (commit hash)
static PANE_LAST_STATE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[tauri::command]
pub fn get_git_graph(_repo_path: String) -> Result<Vec<CommitNode>, String> {
    Ok(vec![])
}

/// 设置 pane 的工作目录
#[tauri::command]
pub fn set_pane_workdir(pane_id: String, workdir: String) -> Result<(), String> {
    let mut workdirs = PANE_WORKDIRS.lock().map_err(|e| e.to_string())?;
    workdirs.insert(pane_id, (workdir, String::new()));
    Ok(())
}

/// 获取当前 pane 的 git diff 状态
#[tauri::command]
pub fn get_git_diff(pane_id: String) -> Result<GitDiffStatus, String> {
    let workdirs = PANE_WORKDIRS.lock().map_err(|e| e.to_string())?;

    let (workdir, _) = workdirs
        .get(&pane_id)
        .ok_or_else(|| "No workdir set for this pane".to_string())?;

    let workdir_path = Path::new(workdir);

    // 检查是否是 git 仓库
    let git_dir = workdir_path.join(".git");
    if !git_dir.exists() {
        return Ok(GitDiffStatus {
            files: vec![],
            total_additions: 0,
            total_deletions: 0,
            is_git_repo: false,
        });
    }

    // 获取 diff 输出
    let output = Command::new("git")
        .args(["diff", "--numstat", "--porcelain"])
        .current_dir(workdir)
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    if !output.status.success() {
        return Ok(GitDiffStatus {
            files: vec![],
            total_additions: 0,
            total_deletions: 0,
            is_git_repo: true,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut files = Vec::new();
    let mut total_additions = 0i32;
    let mut total_deletions = 0i32;

    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }

        let (status, rest) = line.split_at(3);
        let status_str = status.trim();
        let parts: Vec<&str> = rest.trim().split('\t').collect();

        if parts.len() >= 2 {
            let path = parts[1].to_string();
            let additions: i32 = parts[0].parse().unwrap_or(0);
            let deletions: i32 = parts[1].parse().unwrap_or(0);

            total_additions += additions;
            total_deletions += deletions;

            files.push(DiffFile {
                path,
                additions,
                deletions,
                status: status_str.to_string(),
            });
        }
    }

    Ok(GitDiffStatus {
        files,
        total_additions,
        total_deletions,
        is_git_repo: true,
    })
}

/// 检查工作目录是否是 git 仓库
#[tauri::command]
pub fn is_git_repo(path: String) -> bool {
    Path::new(&path).join(".git").exists()
}