use serde::Serialize;
use std::path::Path;
use std::process::Command;

/// 与前端 GitGraph 约定一致
#[derive(Clone, Debug, Serialize)]
pub struct CommitNode {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    pub parents: Vec<String>,
    pub branch: Option<String>,
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

/// Git 仓库信息（包含 graph 和 status）
#[derive(Clone, Debug, Serialize, Default)]
pub struct GitRepoInfo {
    pub is_git_repo: bool,
    pub commits: Vec<CommitNode>,
    pub branches: Vec<String>,
    pub current_branch: Option<String>,
    pub diff: GitDiffStatus,
}

/// 从 git 仓库获取分支列表
fn get_git_branches(repo_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .args(["branch", "-a", "--format=%(refname:short)"])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
        _ => vec![],
    }
}

/// 获取当前分支
fn get_current_branch(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if branch.is_empty() {
                // 可能是 detached HEAD，尝试获取 refname
                let output = Command::new("git")
                    .args(["rev-parse", "--short", "HEAD"])
                    .current_dir(repo_path)
                    .output();
                output.ok().and_then(|o| {
                    if o.status.success() {
                        Some(format!(
                            "(detached at {})",
                            String::from_utf8_lossy(&o.stdout).trim()
                        ))
                    } else {
                        None
                    }
                })
            } else {
                Some(branch)
            }
        }
        _ => None,
    }
}

/// 解析 git log 输出为 CommitNode 列表
fn parse_git_log(output: &str) -> Vec<CommitNode> {
    let mut commits = Vec::new();

    // git log 输出格式：hash|parents|author|date|subject
    // 每条提交记录以 %n 分隔
    for commit_block in output.split("%n") {
        let parts: Vec<&str> = commit_block.split('|').collect();
        if parts.len() < 5 {
            continue;
        }

        let hash = parts[0].trim().to_string();
        if hash.is_empty() {
            continue;
        }

        let parents: Vec<String> = if parts[1].trim().is_empty() {
            vec![]
        } else {
            parts[1]
                .trim()
                .split(' ')
                .map(|s| s.to_string())
                .collect()
        };

        commits.push(CommitNode {
            hash,
            subject: parts[4].trim().to_string(),
            author: parts[2].trim().to_string(),
            date: parts[3].trim().to_string(),
            parents,
            branch: None, // 稍后会被填充
        });
    }

    commits
}

/// 获取 git 提交历史
fn get_git_log(repo_path: &Path, limit: usize) -> Vec<CommitNode> {
    // 使用 git log 获取提交历史，格式：hash|parents|author|date|subject
    let format = "%H|%P|%an|%at|%s%n%b---COMMIT-SEPARATOR---%n";
    let output = Command::new("git")
        .args([
            "log",
            &format!("-{}", limit),
            "--format=format:",
            format,
        ])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut commits = parse_git_log(&stdout);

            // 获取当前分支以标记属于当前分支的提交
            if let Some(branch) = get_current_branch(repo_path) {
                // 获取当前分支的最新提交 hash
                let head_output = Command::new("git")
                    .args(["rev-parse", &format!("{}^{{commit}}", branch)])
                    .current_dir(repo_path)
                    .output();

                if let Ok(head_output) = head_output {
                    let head_hash = String::from_utf8_lossy(&head_output.stdout).trim().to_string();
                    for commit in &mut commits {
                        if commit.parents.contains(&head_hash) || commit.hash == head_hash {
                            commit.branch = Some(branch.clone());
                        }
                    }
                }
            }

            commits
        }
        _ => vec![],
    }
}

/// 检查路径是否是 git 仓库
#[tauri::command]
pub fn is_git_repo(path: String) -> bool {
    Path::new(&path).join(".git").exists()
}

/// 从 pane 的 cwd 获取 git 仓库信息
#[tauri::command]
pub fn get_git_graph(workspace_id: String, pane_id: String) -> Result<GitRepoInfo, String> {
    // 从 AppState 获取 workspace 和 pane
    // 这里我们需要访问 AppState，但 tauri command 不能直接访问
    // 所以改为接收 cwd 参数
    Err("Use get_git_info_with_cwd instead".to_string())
}

/// 根据 cwd 获取 git 仓库信息（前端调用此命令）
#[tauri::command]
pub fn get_git_info_with_cwd(cwd: String) -> Result<GitRepoInfo, String> {
    let repo_path = Path::new(&cwd);

    // 检查是否是 git 仓库
    let git_dir = repo_path.join(".git");
    if !git_dir.exists() {
        return Ok(GitRepoInfo {
            is_git_repo: false,
            commits: vec![],
            branches: vec![],
            current_branch: None,
            diff: GitDiffStatus {
                files: vec![],
                total_additions: 0,
                total_deletions: 0,
                is_git_repo: false,
            },
        });
    }

    // 获取提交历史（限制 50 条）
    let commits = get_git_log(repo_path, 50);

    // 获取分支列表
    let branches = get_git_branches(repo_path);

    // 获取当前分支
    let current_branch = get_current_branch(repo_path);

    // 获取 diff 状态
    let diff = get_git_diff_internal(repo_path);

    Ok(GitRepoInfo {
        is_git_repo: true,
        commits,
        branches,
        current_branch,
        diff,
    })
}

/// 内部函数：根据路径获取 git diff
fn get_git_diff_internal(repo_path: &Path) -> GitDiffStatus {
    // 获取 diff 输出
    let output = Command::new("git")
        .args(["diff", "--numstat", "--porcelain"])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
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

            GitDiffStatus {
                files,
                total_additions,
                total_deletions,
                is_git_repo: true,
            }
        }
        _ => GitDiffStatus {
            files: vec![],
            total_additions: 0,
            total_deletions: 0,
            is_git_repo: true,
        },
    }
}

/// 获取当前 pane 的 git diff 状态（使用静态存储的 cwd）
#[tauri::command]
pub fn get_git_diff(pane_id: String) -> Result<GitDiffStatus, String> {
    // 旧的实现保留以兼容，优先返回空结果让前端使用新的 get_git_info_with_cwd
    Ok(GitDiffStatus::default())
}

/// 设置 pane 的工作目录（保留旧接口）
#[tauri::command]
pub fn set_pane_workdir(pane_id: String, workdir: String) -> Result<(), String> {
    // 这个函数已经不再需要，因为 cwd 现在存储在 PaneTree 中
    // 保留此接口以兼容旧代码
    Ok(())
}

/// 注册新的 git 命令到 lib.rs
#[tauri::command]
pub fn get_git_info(workspace_id: String, pane_id: String) -> Result<GitRepoInfo, String> {
    // 暂时不使用，通过 cwd 获取
    Err("Use get_git_info_with_cwd instead".to_string())
}