use serde::Serialize;
use std::path::{Path, PathBuf};
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

/// 向下（从 `path` 起的子目录里）扫描所有 git 仓库根。
/// 规则：
///   - 广度优先，`max_depth` 限制递归深度（默认 4 层，避免 node_modules 级爆炸）；
///   - 命中 `.git` 后不再进入其子目录，避免 submodule/嵌套仓库的假阳性；
///   - 跳过典型的大型非源码目录（node_modules / target / dist / .venv 等），大幅加速；
///   - `path` 本身若带 `.git` 也会算作结果（即 path 就是仓库根）。
///
/// 前端 SourceControl 会对每个活动 pane 的 cwd 调用一次；结果再去重后即得到
/// 当前工作区视野中的全部仓库。返回的路径均为 Windows 下正斜杠形式，和
/// `paneCwdStore` 的键空间保持一致。
#[tauri::command]
pub fn find_git_repos_below(path: String, max_depth: Option<usize>) -> Vec<String> {
    const SKIP_DIRS: &[&str] = &[
        "node_modules", "target", "dist", "build", ".venv", "venv", "__pycache__",
        ".cache", ".next", ".nuxt", ".svelte-kit", ".parcel-cache", ".turbo", ".yarn",
    ];
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Vec::new();
    }
    let limit = max_depth.unwrap_or(4);
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root, 0)];
    while let Some((dir, depth)) = stack.pop() {
        if dir.join(".git").exists() {
            out.push(canonicalize_cwd(&dir));
            continue; // 不进入仓库内部
        }
        if depth >= limit {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') && name_str.as_ref() != ".git" {
                // 跳过 .github / .vscode 等配置目录；`.git` 本身已在上方单独处理。
                continue;
            }
            if SKIP_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            stack.push((entry.path(), depth + 1));
        }
    }
    out.sort();
    out.dedup();
    out
}

fn canonicalize_cwd(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    #[cfg(windows)]
    {
        s.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        s
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
/// amend=true 时等价 `git commit --amend -m`，用于修改最近一次提交。
#[tauri::command]
pub fn git_commit(repo_root: String, message: String, amend: Option<bool>) -> Result<(), String> {
    if message.trim().is_empty() { return Err("Commit message is empty".to_string()); }
    let path = Path::new(&repo_root);
    let mut cmd = Command::new("git");
    cmd.args(["commit", "-m", &message]);
    if amend.unwrap_or(false) { cmd.arg("--amend"); }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let s = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(if s.is_empty() { String::from_utf8_lossy(&out.stdout).to_string() } else { s });
    }
    Ok(())
}

// ─── VSCode-parity: 分支 / 远端同步 / 文件 diff ────────────────────────────

#[derive(Debug, Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
    /// upstream tracking ref, e.g. "origin/main"; None for detached / unset.
    pub upstream: Option<String>,
}

/// 列出本地 + 远端分支（去掉 HEAD 指针行），标记当前分支。
#[tauri::command]
pub fn git_list_branches(repo_root: String) -> Result<Vec<BranchInfo>, String> {
    let path = Path::new(&repo_root);
    let out = Command::new("git")
        .args([
            "branch",
            "--all",
            "--format=%(refname:short)%09%(HEAD)%09%(upstream:short)",
        ])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut result: Vec<BranchInfo> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end();
        if line.is_empty() { continue; }
        // 跳过 remotes/origin/HEAD -> origin/main 这种 symbolic ref
        if line.contains(" -> ") { continue; }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("").to_string();
        let head_mark = parts.next().unwrap_or("");
        let upstream = parts.next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if name.is_empty() { continue; }
        let is_current = head_mark == "*";
        let is_remote = name.starts_with("origin/") || name.starts_with("remotes/");
        result.push(BranchInfo { name, is_current, is_remote, upstream });
    }
    Ok(result)
}

/// 切换到指定分支。`create=true` 时基于当前 HEAD 创建新分支并切换（git checkout -b）。
#[tauri::command]
pub fn git_checkout(repo_root: String, branch: String, create: Option<bool>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = Command::new("git");
    if create.unwrap_or(false) {
        cmd.args(["checkout", "-b", &branch]);
    } else {
        // 远端分支（origin/foo）checkout 时自动创建本地 tracking 分支
        let local = branch.strip_prefix("origin/").unwrap_or(&branch);
        if local != branch {
            cmd.args(["checkout", "--track", &branch]);
        } else {
            cmd.args(["checkout", &branch]);
        }
    }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let s = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(if s.is_empty() { String::from_utf8_lossy(&out.stdout).to_string() } else { s });
    }
    Ok(())
}

#[tauri::command]
pub fn git_fetch(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let out = Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn git_pull(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let out = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

#[tauri::command]
pub fn git_push(repo_root: String, set_upstream: Option<bool>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = Command::new("git");
    if set_upstream.unwrap_or(false) {
        // 首次 push 需要 --set-upstream；前端在发现 upstream=None 时传 true。
        cmd.args(["push", "--set-upstream", "origin", "HEAD"]);
    } else {
        cmd.arg("push");
    }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

/// 同步当前分支：fetch + pull + push（失败任一步即中止并返回错误）。
/// 对应 VSCode SCM 的 "Sync Changes" 按钮语义。
#[tauri::command]
pub fn git_sync(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let steps: &[&[&str]] = &[
        &["fetch", "--all", "--prune"],
        &["pull", "--ff-only"],
        &["push"],
    ];
    for args in steps {
        let out = Command::new("git")
            .args(*args)
            .current_dir(path)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            // pull/push 无远端跟踪时给更友好的提示
            if err.contains("no upstream") {
                return Err("当前分支没有设置上游远端；请先执行 Push with Upstream。".into());
            }
            return Err(err);
        }
    }
    Ok(())
}

/// 文件 diff。`cached=true` 返回已暂存 diff (HEAD vs index)；false 返回工作区 diff (index vs working tree)。
#[tauri::command]
pub fn git_diff_file(repo_root: String, path: String, cached: Option<bool>) -> Result<String, String> {
    let repo = Path::new(&repo_root);
    let mut cmd = Command::new("git");
    cmd.args(["--no-pager", "diff", "--no-color", "--unified=3"]);
    if cached.unwrap_or(false) { cmd.arg("--cached"); }
    cmd.arg("--");
    cmd.arg(&path);
    let out = cmd.current_dir(repo).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
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