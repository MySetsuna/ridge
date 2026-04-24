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

/// 向上查找 path 所在的 git 仓库根目录（包含 .git 的目录）。
/// 返回绝对路径字符串；若 path 及其所有祖先都不在 git 仓库中，返回 None。
#[tauri::command]
pub fn find_git_repo_root(path: String) -> Option<String> {
    let mut cur = Path::new(&path).to_path_buf();
    // 规范化：若不存在直接按字面层级向上找
    loop {
        if cur.join(".git").exists() {
            return Some(cur.to_string_lossy().to_string());
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// VSCode-风格的 Source Control 文件条目：既能表示已暂存，也能表示未暂存/未跟踪。
#[derive(Clone, Debug, Serialize)]
pub struct ScmFile {
    /// 工作区相对路径
    pub path: String,
    /// 单字母状态：M=modified, A=added(staged new), D=deleted, R=renamed, C=copied,
    /// U=unmerged, ?=untracked
    pub status: String,
    /// staged / unstaged(工作区) / untracked
    pub group: String,
}

/// VSCode-风格的 repo 级状态摘要
#[derive(Clone, Debug, Serialize)]
pub struct ScmRepoStatus {
    pub repo_root: String,
    pub current_branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: Vec<ScmFile>,
    pub changes: Vec<ScmFile>,
    pub untracked: Vec<ScmFile>,
}

/// 解析 `git status --porcelain=v1 -b` 的输出。
fn parse_porcelain_v1(stdout: &str) -> (Option<String>, u32, u32, Vec<ScmFile>, Vec<ScmFile>, Vec<ScmFile>) {
    let mut branch: Option<String> = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut staged = Vec::<ScmFile>::new();
    let mut changes = Vec::<ScmFile>::new();
    let mut untracked = Vec::<ScmFile>::new();

    for line in stdout.lines() {
        if line.starts_with("##") {
            // e.g. "## main...origin/main [ahead 1, behind 2]"
            let rest = line.trim_start_matches("##").trim();
            let (head, tail) = rest.split_once(' ').unwrap_or((rest, ""));
            let head_branch = head.split("...").next().unwrap_or(head);
            if !head_branch.is_empty() && head_branch != "HEAD (no branch)" {
                branch = Some(head_branch.to_string());
            }
            if let Some(bracket) = tail.find('[') {
                let inner = &tail[bracket + 1..tail.rfind(']').unwrap_or(tail.len())];
                for seg in inner.split(',') {
                    let seg = seg.trim();
                    if let Some(n) = seg.strip_prefix("ahead ") {
                        ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = seg.strip_prefix("behind ") {
                        behind = n.parse().unwrap_or(0);
                    }
                }
            }
            continue;
        }
        if line.len() < 3 { continue; }
        let x = line.as_bytes()[0] as char;
        let y = line.as_bytes()[1] as char;
        let path_part = &line[3..];
        // rename: "R  old -> new" 只保留 new
        let display_path = if let Some(idx) = path_part.find(" -> ") {
            path_part[idx + 4..].to_string()
        } else {
            path_part.to_string()
        };

        if x == '?' && y == '?' {
            untracked.push(ScmFile { path: display_path, status: "?".to_string(), group: "untracked".to_string() });
            continue;
        }
        // Staged index column
        if x != ' ' && x != '?' {
            staged.push(ScmFile { path: display_path.clone(), status: x.to_string(), group: "staged".to_string() });
        }
        // Working-tree column
        if y != ' ' && y != '?' {
            changes.push(ScmFile { path: display_path, status: y.to_string(), group: "changes".to_string() });
        }
    }

    (branch, ahead, behind, staged, changes, untracked)
}

/// 获取仓库的 VSCode 源代码管理视图（staged / changes / untracked 分组）。
#[tauri::command]
pub fn get_scm_status(repo_root: String) -> Result<ScmRepoStatus, String> {
    let path = Path::new(&repo_root);
    if !path.join(".git").exists() {
        return Err(format!("Not a git repo: {}", repo_root));
    }
    let output = Command::new("git")
        .args(["status", "--porcelain=v1", "-b", "--untracked-files=normal"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (branch_from_status, ahead, behind, staged, changes, untracked) = parse_porcelain_v1(&stdout);
    let branch = branch_from_status.or_else(|| get_current_branch(path));
    Ok(ScmRepoStatus {
        repo_root,
        current_branch: branch,
        ahead,
        behind,
        staged,
        changes,
        untracked,
    })
}

/// 暂存指定文件（相对于 repo_root 的路径列表，空=全部）
#[tauri::command]
pub fn git_stage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = Command::new("git");
    cmd.arg("add");
    if paths.is_empty() { cmd.arg("--all"); } else { for p in &paths { cmd.arg(p); } }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).to_string()); }
    Ok(())
}

/// 撤销暂存（reset HEAD -- <paths>，空=全部）
#[tauri::command]
pub fn git_unstage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = Command::new("git");
    cmd.args(["reset", "HEAD", "--"]);
    if paths.is_empty() {
        // reset HEAD 不带路径只会重置索引到 HEAD——先拿到 diff --cached 的文件列表
        let cached = Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(path).output().map_err(|e| e.to_string())?;
        if !cached.status.success() { return Err(String::from_utf8_lossy(&cached.stderr).to_string()); }
        for l in String::from_utf8_lossy(&cached.stdout).lines() {
            if !l.trim().is_empty() { cmd.arg(l); }
        }
    } else {
        for p in &paths { cmd.arg(p); }
    }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).to_string()); }
    Ok(())
}

/// 丢弃工作区修改（checkout -- <paths>）——危险操作，前端应再次确认
#[tauri::command]
pub fn git_discard(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() { return Err("Refusing to discard all — specify paths".to_string()); }
    let path = Path::new(&repo_root);
    let out = Command::new("git")
        .args(["checkout", "--"])
        .args(&paths)
        .current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).to_string()); }
    Ok(())
}

/// 创建 commit（使用 -m message）。未 stage 的更改不会被提交。
#[tauri::command]
pub fn git_commit(repo_root: String, message: String) -> Result<(), String> {
    if message.trim().is_empty() { return Err("Commit message is empty".to_string()); }
    let path = Path::new(&repo_root);
    let out = Command::new("git")
        .args(["commit", "-m", &message])
        .current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let s = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(if s.is_empty() { String::from_utf8_lossy(&out.stdout).to_string() } else { s });
    }
    Ok(())
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