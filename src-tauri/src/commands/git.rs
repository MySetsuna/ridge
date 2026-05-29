use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns a `Command::new("git")` with CREATE_NO_WINDOW on Windows so
/// git subprocesses never flash a console window in the Tauri GUI app.
fn git_cmd() -> Command {
    let mut cmd = Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 与前端 GitGraph 约定一致
#[derive(Clone, Debug, Serialize)]
pub struct CommitNode {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub date: String,
    pub parents: Vec<String>,
    pub branch: Option<String>,
    /// Ref decorations attached to this commit (parsed from `git log
    /// --decorate=full`). Each entry is one of:
    ///   `branch:main`, `branch:origin/feat-x`, `tag:v1.2.3`, `head:`
    /// Frontend renders these as inline pills next to the commit row.
    /// Empty when the commit has no refs pointing to it.
    #[serde(default)]
    pub refs: Vec<String>,
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
    let output = git_cmd()
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
    let output = git_cmd()
        .args(["branch", "--show-current"])
        .current_dir(repo_path)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if branch.is_empty() {
                // 可能是 detached HEAD，尝试获取 refname
                let output = git_cmd()
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

/// Parse `%D` (ref names) into the structured `refs` vec the frontend
/// consumes. Input shapes from git:
///   `HEAD -> refs/heads/main, refs/heads/foo, tag: refs/tags/v1.0, refs/remotes/origin/main`
///   (or empty, when the commit has no refs)
/// Output entries:
///   `head:` for the bare HEAD pointer
///   `branch:main` / `branch:origin/main` for local + remote branches
///   `tag:v1.0` for tags
/// Order is preserved so the UI can paint HEAD-then-branches-then-tags
/// in the same order git reported them.
fn parse_decorations(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return out;
    }
    for part in trimmed.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        // `HEAD -> refs/heads/main` and `HEAD` (detached) both start with
        // HEAD; record the HEAD pointer first, then fall through to the
        // branch ref on the rhs of `->` so both badges show up.
        if let Some(rest) = p.strip_prefix("HEAD -> ") {
            out.push("head:".to_string());
            // rest is e.g. `refs/heads/main` — fall through to parse it
            // as a branch ref.
            if let Some(name) = rest.strip_prefix("refs/heads/") {
                out.push(format!("branch:{}", name));
            } else if let Some(name) = rest.strip_prefix("refs/remotes/") {
                out.push(format!("branch:{}", name));
            }
            continue;
        }
        if p == "HEAD" {
            out.push("head:".to_string());
            continue;
        }
        if let Some(name) = p.strip_prefix("tag: refs/tags/") {
            out.push(format!("tag:{}", name));
            continue;
        }
        if let Some(name) = p.strip_prefix("tag: ") {
            out.push(format!("tag:{}", name));
            continue;
        }
        if let Some(name) = p.strip_prefix("refs/heads/") {
            out.push(format!("branch:{}", name));
            continue;
        }
        if let Some(name) = p.strip_prefix("refs/remotes/") {
            out.push(format!("branch:{}", name));
            continue;
        }
        // Unknown shape — keep the raw decoration so it's at least visible.
        out.push(p.to_string());
    }
    out
}

// Field + record separators chosen from the ASCII control plane:
//   `\x1f` (UNIT SEPARATOR)   — between fields within one commit
//   `\x1e` (RECORD SEPARATOR) — between commits
// These are explicitly forbidden inside git ref names and effectively
// impossible inside author names / subjects, so they avoid the round-31
// review HIGH finding where `|` could collide with `user.name = "A|B"`
// or unusual ref decorations. Using control chars also lets us drop the
// fragile `---COMMIT-SEPARATOR---` literal that the old parser tried
// to split on but never actually did (it split on the literal two-char
// `%n` instead of git's newline expansion).
const FIELD_SEP: char = '\x1f';
const RECORD_SEP: char = '\x1e';

/// 解析 git log 输出为 CommitNode 列表
fn parse_git_log(output: &str) -> Vec<CommitNode> {
    let mut commits = Vec::new();

    // git log 输出格式：hash␟parents␟author␟date␟refs␟subject␞
    for commit_block in output.split(RECORD_SEP) {
        let parts: Vec<&str> = commit_block.split(FIELD_SEP).collect();
        if parts.len() < 6 {
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
            subject: parts[5].trim().to_string(),
            author: parts[2].trim().to_string(),
            date: parts[3].trim().to_string(),
            parents,
            branch: None, // 稍后会被填充
            refs: parse_decorations(parts[4]),
        });
    }

    commits
}

/// 获取 git 提交历史
fn get_git_log(repo_path: &Path, limit: usize) -> Vec<CommitNode> {
    // 字段：hash␟parents␟author␟date␟refs␟subject␞
    // `%D` is the comma-separated ref name list (`HEAD -> refs/heads/main,
    // tag: refs/tags/v1.0`); `--decorate=full` gives us the unambiguous
    // refs/heads/ / refs/tags/ / refs/remotes/ prefixes that
    // `parse_decorations` keys off — `--decorate=short` would strip them
    // and we'd have to guess by name.
    //
    // CRITICAL: the format flag MUST be one argv element. The previous
    // shape `"--format=format:", format` made git treat the format
    // string as a positional revspec, blanking the output (round-31 review).
    let pretty = format!(
        "--pretty=format:%H{0}%P{0}%an{0}%at{0}%D{0}%s{1}",
        FIELD_SEP, RECORD_SEP
    );
    let output = git_cmd()
        .args([
            "log",
            "--decorate=full",
            &format!("-{}", limit),
            &pretty,
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
                let head_output = git_cmd()
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
pub async fn find_git_repos_below(path: String, max_depth: Option<usize>) -> Vec<String> {
    tokio::task::spawn_blocking(move || find_git_repos_below_sync(path, max_depth))
        .await
        .unwrap_or_default()
}

pub(crate) fn find_git_repos_below_sync(path: String, max_depth: Option<usize>) -> Vec<String> {
    // Directories we never descend into. Grouped roughly by ecosystem so future
    // additions land in the right bucket. Keep this list tight — each entry
    // short-circuits a potentially huge subtree scan. If a project does
    // actually keep a repo root inside (say) `vendor/`, users can point Ridge
    // at that path directly; we prefer the monorepo-happy-path default.
    const SKIP_DIRS: &[&str] = &[
        // JS / TS
        "node_modules", ".pnpm-store", ".yarn", ".next", ".nuxt",
        ".svelte-kit", ".parcel-cache", ".turbo", ".vite",
        // Rust
        "target",
        // Python
        ".venv", "venv", "__pycache__", ".tox", ".mypy_cache", ".pytest_cache",
        // JVM / Gradle / Android
        ".gradle", ".kotlin",
        // General build outputs
        "dist", "build", "out", "coverage", ".cache",
        // Vendor / package manager caches
        "vendor",
        // IDE / tooling metadata
        ".idea",
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
    /// Per-file added line count from `git diff --numstat`. 0 for untracked
    /// files (numstat doesn't see them) and for binary / rename-only changes
    /// where git emits `-`. Lets the SCM list render `+12 -3` after each
    /// file name without a second roundtrip per click.
    #[serde(default)]
    pub additions: u32,
    /// Per-file removed line count. Same caveats as `additions`.
    #[serde(default)]
    pub deletions: u32,
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
    /// True iff the current branch has an upstream tracking ref. Lets the UI
    /// surface "no upstream → push will need -u" without an extra git call.
    /// Detected from the `## branch...upstream` form in `git status -b`.
    #[serde(default)]
    pub has_upstream: bool,
}

/// 解析 `git status --porcelain=v1 -b` 的输出。
///
/// Returns `(branch, ahead, behind, has_upstream, staged, changes, untracked)`.
/// `has_upstream` reflects whether the `## branch...upstream` form actually
/// has a non-empty upstream segment after `...` — e.g. `## main` (no `...`)
/// and `## main...` (empty rhs) both mean "no upstream tracking ref", while
/// `## main...origin/main` means "tracking origin/main".
fn parse_porcelain_v1(stdout: &str) -> (Option<String>, u32, u32, bool, Vec<ScmFile>, Vec<ScmFile>, Vec<ScmFile>) {
    let mut branch: Option<String> = None;
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut has_upstream = false;
    let mut staged = Vec::<ScmFile>::new();
    let mut changes = Vec::<ScmFile>::new();
    let mut untracked = Vec::<ScmFile>::new();

    for line in stdout.lines() {
        if line.starts_with("##") {
            // e.g. "## main...origin/main [ahead 1, behind 2]"
            let rest = line.trim_start_matches("##").trim();
            let (head, tail) = rest.split_once(' ').unwrap_or((rest, ""));
            let mut split = head.splitn(2, "...");
            let head_branch = split.next().unwrap_or(head);
            // Upstream segment after "..." — only counts when non-empty.
            // `## main` → no split → no upstream.
            // `## main...` → empty rhs → no upstream.
            // `## main...origin/main` → tracking origin/main.
            if let Some(up) = split.next() {
                if !up.trim().is_empty() {
                    has_upstream = true;
                }
            }
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
            untracked.push(ScmFile {
                path: display_path,
                status: "?".to_string(),
                group: "untracked".to_string(),
                additions: 0,
                deletions: 0,
            });
            continue;
        }
        // Staged index column
        if x != ' ' && x != '?' {
            staged.push(ScmFile {
                path: display_path.clone(),
                status: x.to_string(),
                group: "staged".to_string(),
                additions: 0,
                deletions: 0,
            });
        }
        // Working-tree column
        if y != ' ' && y != '?' {
            changes.push(ScmFile {
                path: display_path,
                status: y.to_string(),
                group: "changes".to_string(),
                additions: 0,
                deletions: 0,
            });
        }
    }

    (branch, ahead, behind, has_upstream, staged, changes, untracked)
}

/// Parse `git diff --numstat` output into `path → (additions, deletions)`.
/// Numstat lines are TAB-separated `<added>\t<removed>\t<path>`; binary
/// changes report `-` for both counts (we clamp those to 0). Renames may
/// appear as `path => new` — we keep the new path as the key so it lines
/// up with porcelain output.
fn parse_numstat(stdout: &str) -> std::collections::HashMap<String, (u32, u32)> {
    let mut out = std::collections::HashMap::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(3, '\t');
        let a = parts.next().unwrap_or("0");
        let r = parts.next().unwrap_or("0");
        let path = match parts.next() {
            Some(p) => p,
            None => continue,
        };
        let path_key = if let Some(idx) = path.find(" => ") {
            path[idx + 4..].trim_end_matches('}').to_string()
        } else {
            path.to_string()
        };
        let add = a.parse::<u32>().unwrap_or(0);
        let del = r.parse::<u32>().unwrap_or(0);
        out.insert(path_key, (add, del));
    }
    out
}

/// 获取仓库的 VSCode 源代码管理视图（staged / changes / untracked 分组）。
#[tauri::command]
pub async fn get_scm_status(repo_root: String) -> Result<ScmRepoStatus, String> {
    tokio::task::spawn_blocking(move || get_scm_status_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn get_scm_status_sync(repo_root: String) -> Result<ScmRepoStatus, String> {
    let path = Path::new(&repo_root);
    if !path.join(".git").exists() {
        return Err(format!("Not a git repo: {}", repo_root));
    }
    let output = git_cmd()
        .args(["status", "--porcelain=v1", "-b", "--untracked-files=normal"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (branch_from_status, ahead, behind, has_upstream, mut staged, mut changes, untracked) =
        parse_porcelain_v1(&stdout);
    let branch = branch_from_status.or_else(|| get_current_branch(path));

    // Two parallel-style numstat calls: working-tree (index ↔ tree) for the
    // unstaged "更改" group, and `--cached` (HEAD ↔ index) for the staged
    // group. They're separate because staged and unstaged hunks don't share
    // a base — staging a partial change should still let the staged column
    // show its own +N/-N. Each is one process spawn; an order of magnitude
    // cheaper than the per-file path the modal used to take.
    let unstaged_counts = git_cmd()
        .args(["--no-pager", "diff", "--numstat", "--"])
        .current_dir(path)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o.stdout) } else { None })
        .map(|b| parse_numstat(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let staged_counts = git_cmd()
        .args(["--no-pager", "diff", "--cached", "--numstat", "--"])
        .current_dir(path)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o.stdout) } else { None })
        .map(|b| parse_numstat(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    for f in &mut changes {
        if let Some(&(a, d)) = unstaged_counts.get(&f.path) {
            f.additions = a;
            f.deletions = d;
        }
    }
    for f in &mut staged {
        if let Some(&(a, d)) = staged_counts.get(&f.path) {
            f.additions = a;
            f.deletions = d;
        }
    }

    Ok(ScmRepoStatus {
        repo_root,
        current_branch: branch,
        ahead,
        behind,
        staged,
        changes,
        untracked,
        has_upstream,
    })
}

/// 暂存指定文件（相对于 repo_root 的路径列表，空=全部）
#[tauri::command]
pub async fn git_stage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_stage_sync(repo_root, paths))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_stage_sync(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = git_cmd();
    cmd.arg("add");
    if paths.is_empty() { cmd.arg("--all"); } else { for p in &paths { cmd.arg(p); } }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).to_string()); }
    Ok(())
}

/// 撤销暂存（reset HEAD -- <paths>，空=全部）
#[tauri::command]
pub async fn git_unstage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_unstage_sync(repo_root, paths))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_unstage_sync(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = git_cmd();
    cmd.args(["reset", "HEAD", "--"]);
    if paths.is_empty() {
        // reset HEAD 不带路径只会重置索引到 HEAD——先拿到 diff --cached 的文件列表
        let cached = git_cmd()
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
pub async fn git_discard(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_discard_sync(repo_root, paths))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_discard_sync(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() { return Err("Refusing to discard all — specify paths".to_string()); }
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["checkout", "--"])
        .args(&paths)
        .current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).to_string()); }
    Ok(())
}

/// 删除工作区里的指定 untracked 文件/目录。`git checkout --` 不会处理 untracked
/// （它们在索引里没有快照），需要 `git clean -fd -- <paths>`。
/// 路径必须由调用方明确给出 —— 拒绝空集合，避免 `git clean -fd` 全仓库清理。
#[tauri::command]
pub async fn git_clean_untracked(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_clean_untracked_sync(repo_root, paths))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_clean_untracked_sync(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    if paths.is_empty() {
        return Err("Refusing to clean — specify paths".to_string());
    }
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["clean", "-fd", "--"])
        .args(&paths)
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

/// 创建 commit（使用 -m message）。未 stage 的更改不会被提交。
/// amend=true 时等价 `git commit --amend -m`，用于修改最近一次提交。
#[tauri::command]
pub async fn git_commit(repo_root: String, message: String, amend: Option<bool>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_commit_sync(repo_root, message, amend))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_commit_sync(repo_root: String, message: String, amend: Option<bool>) -> Result<(), String> {
    if message.trim().is_empty() { return Err("Commit message is empty".to_string()); }
    let path = Path::new(&repo_root);
    let mut cmd = git_cmd();
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
pub async fn git_list_branches(repo_root: String) -> Result<Vec<BranchInfo>, String> {
    tokio::task::spawn_blocking(move || git_list_branches_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_list_branches_sync(repo_root: String) -> Result<Vec<BranchInfo>, String> {
    let path = Path::new(&repo_root);
    let out = git_cmd()
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

/// 切换到指定分支。`create=true` 时基于 `base`（默认 HEAD）创建新分支并切换：
/// `git checkout -b <branch> [<base>]`。`base` 可以是 `main`、`origin/main` 等
/// 任意 ref；空或省略表示从当前 HEAD 切。
#[tauri::command]
pub async fn git_checkout(
    repo_root: String,
    branch: String,
    create: Option<bool>,
    base: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_checkout_sync(repo_root, branch, create, base))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_checkout_sync(
    repo_root: String,
    branch: String,
    create: Option<bool>,
    base: Option<String>,
) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = git_cmd();
    if create.unwrap_or(false) {
        cmd.args(["checkout", "-b", &branch]);
        // Trim and ignore empty / pure-whitespace base — common when the
        // frontend passes a default-empty input.
        if let Some(base_ref) = base.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            cmd.arg(base_ref);
        }
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
pub async fn git_fetch(repo_root: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_fetch_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_fetch_sync(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let out = git_cmd()
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
pub async fn git_pull(repo_root: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_pull_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_pull_sync(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let out = git_cmd()
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
pub async fn git_push(repo_root: String, set_upstream: Option<bool>) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_push_sync(repo_root, set_upstream))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_push_sync(repo_root: String, set_upstream: Option<bool>) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let mut cmd = git_cmd();
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
///
/// Pre-flight: peek at `git status -b --porcelain=v1` to learn whether
/// the current branch has an upstream tracking ref. Without one, `pull`
/// and `push` would fail with locale-dependent error strings ("There is
/// no tracking information…" / "no upstream branch") that we used to
/// match by substring — fragile against `LC_ALL=zh_CN.UTF-8` etc. The
/// pre-flight gives a deterministic friendly message and avoids spawning
/// the failing subcommands at all.
#[tauri::command]
pub async fn git_sync(repo_root: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_sync_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_sync_sync(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);

    // Quick upstream probe — same parser used by get_scm_status, so the
    // detection logic is one-source-of-truth instead of "porcelain parser
    // says X, sync subcommand sniffs stderr for Y".
    let probe = git_cmd()
        .args(["status", "--porcelain=v1", "-b", "--untracked-files=no"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if probe.status.success() {
        let stdout = String::from_utf8_lossy(&probe.stdout);
        let (_branch, _ahead, _behind, has_upstream, _, _, _) = parse_porcelain_v1(&stdout);
        if !has_upstream {
            return Err(
                "当前分支没有设置上游远端；请先执行 Push with Upstream。".into(),
            );
        }
    }

    let steps: &[&[&str]] = &[
        &["fetch", "--all", "--prune"],
        &["pull", "--ff-only"],
        &["push"],
    ];
    for args in steps {
        let out = git_cmd()
            .args(*args)
            .current_dir(path)
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
    }
    Ok(())
}

/// Snapshot of a repo's "operation in progress" state so the frontend
/// can offer the right abort/continue affordances after a conflict.
/// Detected by the marker files git drops in `.git/`:
///   `.git/CHERRY_PICK_HEAD`  → cherry-pick paused on conflict
///   `.git/REVERT_HEAD`       → revert paused on conflict
///   `.git/MERGE_HEAD`        → merge paused on conflict
///   `.git/rebase-apply` or `.git/rebase-merge` (dir) → rebase in progress
#[derive(Clone, Debug, Serialize, Default)]
pub struct GitOpInProgress {
    pub cherry_pick: bool,
    pub revert: bool,
    pub merge: bool,
    pub rebase: bool,
}

#[tauri::command]
pub fn git_op_in_progress(repo_root: String) -> GitOpInProgress {
    let git = Path::new(&repo_root).join(".git");
    GitOpInProgress {
        cherry_pick: git.join("CHERRY_PICK_HEAD").exists(),
        revert: git.join("REVERT_HEAD").exists(),
        merge: git.join("MERGE_HEAD").exists(),
        rebase: git.join("rebase-apply").is_dir() || git.join("rebase-merge").is_dir(),
    }
}

/// Abort a cherry-pick that's paused on conflict — `git cherry-pick
/// --abort`. Restores the working tree to its pre-cherry-pick state.
#[tauri::command]
pub async fn git_cherry_pick_abort(repo_root: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_cherry_pick_abort_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_cherry_pick_abort_sync(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["cherry-pick", "--abort"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// Abort a revert that's paused on conflict — `git revert --abort`.
#[tauri::command]
pub async fn git_revert_abort(repo_root: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_revert_abort_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_revert_abort_sync(repo_root: String) -> Result<(), String> {
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["revert", "--abort"])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// Apply a single commit's changes onto the current branch via
/// `git cherry-pick <hash>`. Conflicts surface as a non-zero exit and
/// the stderr is forwarded — frontend then calls `git_op_in_progress`
/// to confirm the cherry-pick is paused and offers an abort path.
///
/// Use case: SCM commit-row right-click menu picks a commit from a
/// different branch and replays it here. Same flow as VS Code's "Git:
/// Cherry-Pick Commit" command.
#[tauri::command]
pub async fn git_cherry_pick(repo_root: String, hash: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_cherry_pick_sync(repo_root, hash))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_cherry_pick_sync(repo_root: String, hash: String) -> Result<(), String> {
    if hash.trim().is_empty() {
        return Err("commit hash 不能为空".into());
    }
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["cherry-pick", hash.trim()])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let s = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if s.is_empty() {
            String::from_utf8_lossy(&out.stdout).to_string()
        } else {
            s
        });
    }
    Ok(())
}

/// Inverse of cherry-pick: create a new commit that undoes a target
/// commit's changes. `--no-edit` skips the editor — frontend already
/// surfaces the auto-generated message in the next status refresh.
#[tauri::command]
pub async fn git_revert(repo_root: String, hash: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_revert_sync(repo_root, hash))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_revert_sync(repo_root: String, hash: String) -> Result<(), String> {
    if hash.trim().is_empty() {
        return Err("commit hash 不能为空".into());
    }
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["revert", "--no-edit", hash.trim()])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        let s = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if s.is_empty() {
            String::from_utf8_lossy(&out.stdout).to_string()
        } else {
            s
        });
    }
    Ok(())
}

/// Aggregated diff line counts for a repo — what the pane header pill shows.
/// Runs `git diff --numstat HEAD` which counts (added, removed) per file then
/// sums across every file. Renamed / binary files report `-` lines in numstat
/// output; we clamp those to 0 so the pill stays numeric. Returns `(0, 0)`
/// for a clean tree or when `git` isn't reachable — the frontend just shows
/// no counter in that case, matching "nothing to report".
#[derive(Debug, Serialize)]
pub struct GitDiffSummary {
    pub added: u32,
    pub removed: u32,
}

#[tauri::command]
pub async fn git_diff_summary(repo_root: String) -> Result<GitDiffSummary, String> {
    tokio::task::spawn_blocking(move || git_diff_summary_sync(repo_root))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_diff_summary_sync(repo_root: String) -> Result<GitDiffSummary, String> {
    let repo = Path::new(&repo_root);
    let out = git_cmd()
        .args(["--no-pager", "diff", "--numstat", "HEAD", "--"])
        .current_dir(repo)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        // Return zeros instead of erroring — pre-first-commit repos fail here,
        // and the pill should simply show nothing rather than surface a toast.
        return Ok(GitDiffSummary { added: 0, removed: 0 });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut added: u32 = 0;
    let mut removed: u32 = 0;
    for line in stdout.lines() {
        // Format: `<added>\t<removed>\t<path>` or `-\t-\t<path>` for binary.
        let mut parts = line.splitn(3, '\t');
        let a_raw = parts.next().unwrap_or("0");
        let r_raw = parts.next().unwrap_or("0");
        added = added.saturating_add(a_raw.parse::<u32>().unwrap_or(0));
        removed = removed.saturating_add(r_raw.parse::<u32>().unwrap_or(0));
    }
    Ok(GitDiffSummary { added, removed })
}

/// Side-by-side diff payload for the Monaco DiffEditor — returns the two
/// blobs the editor needs to render before/after panes side by side.
///
/// `cached`:
///   - `false` (working-tree view): `original` = index blob (`git show
///     :<path>`), `modified` = working tree contents (read from disk).
///     This matches what the unstaged diff in SCM shows.
///   - `true` (staged view): `original` = HEAD blob (`git show
///     HEAD:<path>`), `modified` = index blob.
///
/// Either side returns `""` when the file doesn't exist on that side
/// (new files have no HEAD/index blob; deleted files have no working tree).
/// We never error for "file missing" — the modal renders the empty side
/// as an additions-only / deletions-only view, the same as VS Code.
#[derive(Debug, Serialize, Default)]
pub struct GitFileVersions {
    pub original: String,
    pub modified: String,
}

#[tauri::command]
pub async fn git_get_file_versions(
    repo_root: String,
    path: String,
    cached: Option<bool>,
) -> Result<GitFileVersions, String> {
    tokio::task::spawn_blocking(move || git_get_file_versions_sync(repo_root, path, cached))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// 给定 commit hash，返回该 commit 涉及的文件清单（status: A/M/D/R...）。
/// 用于 GitGraph 单击 commit 时的 inline 详情面板。
#[derive(Debug, Serialize)]
pub struct CommitFileEntry {
    pub path: String,
    pub status: String,
}

#[tauri::command]
pub async fn git_get_commit_files(
    repo_root: String,
    hash: String,
) -> Result<Vec<CommitFileEntry>, String> {
    tokio::task::spawn_blocking(move || git_get_commit_files_sync(repo_root, hash))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_get_commit_files_sync(repo_root: String, hash: String) -> Result<Vec<CommitFileEntry>, String> {
    if hash.is_empty() {
        return Err("missing commit hash".to_string());
    }
    let path = Path::new(&repo_root);
    // `--name-status -m`：处理 merge commit 的合并视图；`-r` 递归不要折叠成目录。
    // `--pretty=format:` 抑制头部输出，只保留文件状态行。
    let out = git_cmd()
        .args(["show", "--name-status", "-m", "-r", "--pretty=format:", &hash])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut files = Vec::<CommitFileEntry>::new();
    let mut seen = std::collections::HashSet::<String>::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 行格式："M\tpath" 或 "R100\told\tnew"——R/C 后跟分数，路径在最后一列。
        let mut parts = line.split('\t');
        let status_raw = parts.next().unwrap_or("");
        // 取首字母作为简化状态（M/A/D/R/C/T/U）。
        let status = status_raw.chars().next().map(|c| c.to_string()).unwrap_or_default();
        let p = parts.last().unwrap_or("").to_string();
        if p.is_empty() || status.is_empty() {
            continue;
        }
        if seen.insert(p.clone()) {
            files.push(CommitFileEntry { path: p, status });
        }
    }
    Ok(files)
}

/// 取 commit 时刻该文件的 before/after 内容（hash^ vs hash）。
/// 复用 `git show` 拉取 blob，不存在的一侧返回空字符串（首次新增 / 已删除）。
#[tauri::command]
pub async fn git_get_file_versions_at_commit(
    repo_root: String,
    path: String,
    hash: String,
) -> Result<GitFileVersions, String> {
    tokio::task::spawn_blocking(move || git_get_file_versions_at_commit_sync(repo_root, path, hash))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_get_file_versions_at_commit_sync(
    repo_root: String,
    path: String,
    hash: String,
) -> Result<GitFileVersions, String> {
    if hash.is_empty() {
        return Err("missing commit hash".to_string());
    }
    let repo = Path::new(&repo_root);
    let show = |spec: &str| -> Option<String> {
        let out = git_cmd()
            .args(["--no-pager", "show", spec])
            .current_dir(repo)
            .output()
            .ok()?;
        if !out.status.success() {
            // Missing object（首次提交无父，或文件在该 commit 才创建）→ 空。
            return Some(String::new());
        }
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    };
    let original = show(&format!("{}^:{}", hash, path)).unwrap_or_default();
    let modified = show(&format!("{}:{}", hash, path)).unwrap_or_default();
    Ok(GitFileVersions { original, modified })
}

/// 创建 tag。message 为空时是 lightweight tag，非空则 annotated。
#[tauri::command]
pub async fn git_create_tag(
    repo_root: String,
    name: String,
    hash: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_create_tag_sync(repo_root, name, hash, message))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_create_tag_sync(
    repo_root: String,
    name: String,
    hash: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("tag name is empty".to_string());
    }
    let path = Path::new(&repo_root);
    let mut cmd = git_cmd();
    cmd.arg("tag");
    if let Some(msg) = message.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        cmd.args(["-a", &name, "-m", msg]);
    } else {
        cmd.arg(&name);
    }
    if let Some(h) = hash.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        cmd.arg(h);
    }
    let out = cmd.current_dir(path).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

/// `git reset` 三种模式：soft / mixed / hard。`hard` 是不可逆操作，前端必须二次确认。
#[tauri::command]
pub async fn git_reset(
    repo_root: String,
    hash: String,
    mode: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || git_reset_sync(repo_root, hash, mode))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_reset_sync(repo_root: String, hash: String, mode: String) -> Result<(), String> {
    if hash.is_empty() {
        return Err("missing target hash".to_string());
    }
    let mode_flag = match mode.as_str() {
        "soft" => "--soft",
        "mixed" | "" => "--mixed",
        "hard" => "--hard",
        other => return Err(format!("unsupported reset mode: {}", other)),
    };
    let path = Path::new(&repo_root);
    let out = git_cmd()
        .args(["reset", mode_flag, &hash])
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(())
}

pub(crate) fn git_get_file_versions_sync(
    repo_root: String,
    path: String,
    cached: Option<bool>,
) -> Result<GitFileVersions, String> {
    let repo = Path::new(&repo_root);
    let cached = cached.unwrap_or(false);

    // git show :<path> / HEAD:<path>. Use --textconv off to keep raw blob
    // bytes; passing through smudge filters could rewrite line endings and
    // make the diff look noisier than reality.
    let show = |spec: &str| -> Option<String> {
        let out = git_cmd()
            .args(["--no-pager", "show", spec])
            .current_dir(repo)
            .output()
            .ok()?;
        if !out.status.success() {
            // Object missing (new file) → empty side; don't propagate as err.
            return Some(String::new());
        }
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    };

    let (original_spec, modified_spec): (String, Option<String>) = if cached {
        // staged view: HEAD vs index
        (format!("HEAD:{}", path), Some(format!(":{}", path)))
    } else {
        // unstaged view: index vs working tree (modified is read from disk)
        (format!(":{}", path), None)
    };

    let original = show(&original_spec).unwrap_or_default();
    let modified = if let Some(spec) = modified_spec {
        show(&spec).unwrap_or_default()
    } else {
        // Working tree side — read the file directly. Two safety guards
        // before we touch the filesystem:
        //   1. Path-traversal: a frontend bug or compromised IPC could
        //      pass `../../etc/passwd` here and `repo.join` would happily
        //      resolve it. Canonicalise both sides and ensure the target
        //      stays inside the repo. `git show :<path>` already enforces
        //      this server-side; we mirror it for the disk path.
        //   2. Binary safety: use `fs::read` + `from_utf8_lossy` to match
        //      what the `git show` branch does — `read_to_string` would
        //      bail on any non-UTF-8 byte and surface as a modal error,
        //      while `from_utf8_lossy` substitutes U+FFFD and lets the
        //      diff render. Asymmetry between the two sides was a HIGH
        //      finding from round-26 review.
        let abs = repo.join(&path);
        if let (Ok(repo_abs), Ok(target_abs)) = (repo.canonicalize(), abs.canonicalize()) {
            if !target_abs.starts_with(&repo_abs) {
                return Err(format!(
                    "path escapes repo root: {}",
                    abs.display()
                ));
            }
        }
        match std::fs::read(&abs) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("read {}: {}", abs.display(), e)),
        }
    };

    Ok(GitFileVersions { original, modified })
}

/// 文件 diff。`cached=true` 返回已暂存 diff (HEAD vs index)；false 返回工作区 diff (index vs working tree)。
#[tauri::command]
pub async fn git_diff_file(repo_root: String, path: String, cached: Option<bool>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || git_diff_file_sync(repo_root, path, cached))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

pub(crate) fn git_diff_file_sync(repo_root: String, path: String, cached: Option<bool>) -> Result<String, String> {
    let repo = Path::new(&repo_root);
    let mut cmd = git_cmd();
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
pub fn get_git_graph(_workspace_id: String, _pane_id: String) -> Result<GitRepoInfo, String> {
    // 从 AppState 获取 workspace 和 pane
    // 这里我们需要访问 AppState，但 tauri command 不能直接访问
    // 所以改为接收 cwd 参数
    Err("Use get_git_info_with_cwd instead".to_string())
}

/// 根据 cwd 获取 git 仓库信息（前端调用此命令）
#[tauri::command]
pub async fn get_git_info_with_cwd(cwd: String) -> Result<GitRepoInfo, String> {
    tokio::task::spawn_blocking(move || get_git_info_with_cwd_sync(cwd))
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// T10：分页拉取更早的 commits。前端 GitGraph 滚动到底部时调用。
/// 返回空数组表示已到达 git log 末尾，不再有更早记录。
#[tauri::command]
pub async fn get_git_commits_paginated(
    repo_root: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<CommitNode>, String> {
    tokio::task::spawn_blocking(move || {
        let path = Path::new(&repo_root);
        Ok(get_git_log_with_skip(path, offset as usize, limit as usize))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// `get_git_log` 的分页变种 —— 多一个 `--skip` 参数。其它格式 / 解析与原函数完全一致。
fn get_git_log_with_skip(repo_path: &Path, offset: usize, limit: usize) -> Vec<CommitNode> {
    let pretty = format!(
        "--pretty=format:%H{0}%P{0}%an{0}%at{0}%D{0}%s{1}",
        FIELD_SEP, RECORD_SEP
    );
    let output = git_cmd()
        .args([
            "log",
            "--decorate=full",
            &format!("--skip={}", offset),
            &format!("-{}", limit),
            &pretty,
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_git_log(&stdout)
        }
        _ => Vec::new(),
    }
}

pub(crate) fn get_git_info_with_cwd_sync(cwd: String) -> Result<GitRepoInfo, String> {
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
    let output = git_cmd()
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
pub fn get_git_diff(_pane_id: String) -> Result<GitDiffStatus, String> {
    // 旧的实现保留以兼容，优先返回空结果让前端使用新的 get_git_info_with_cwd
    Ok(GitDiffStatus::default())
}

/// 设置 pane 的工作目录（保留旧接口）
#[tauri::command]
pub fn set_pane_workdir(_pane_id: String, _workdir: String) -> Result<(), String> {
    // 这个函数已经不再需要，因为 cwd 现在存储在 PaneTree 中
    // 保留此接口以兼容旧代码
    Ok(())
}

#[cfg(test)]
mod decoration_tests {
    use super::*;

    #[test]
    fn empty_decoration_yields_empty_vec() {
        assert!(parse_decorations("").is_empty());
        assert!(parse_decorations("   ").is_empty());
    }

    #[test]
    fn parses_head_pointing_to_branch() {
        let r = parse_decorations("HEAD -> refs/heads/main");
        assert_eq!(r, vec!["head:".to_string(), "branch:main".to_string()]);
    }

    #[test]
    fn parses_detached_head_alone() {
        let r = parse_decorations("HEAD");
        assert_eq!(r, vec!["head:".to_string()]);
    }

    #[test]
    fn parses_branches_and_tags_and_remotes() {
        let r = parse_decorations(
            "HEAD -> refs/heads/main, tag: refs/tags/v1.0, refs/remotes/origin/main",
        );
        assert_eq!(
            r,
            vec![
                "head:".to_string(),
                "branch:main".to_string(),
                "tag:v1.0".to_string(),
                "branch:origin/main".to_string(),
            ]
        );
    }

    #[test]
    fn unknown_decoration_falls_through_verbatim() {
        // Future git versions might emit shapes we don't yet handle;
        // surface them rather than silently dropping.
        let r = parse_decorations("refs/something/weird");
        assert_eq!(r, vec!["refs/something/weird".to_string()]);
    }

    #[test]
    fn parse_git_log_handles_pipe_in_author_name() {
        // Pre-fix this would shift parts[5] off into the void because
        // `|` was the field separator. With unit-separator now, even
        // pathological author names round-trip correctly.
        let s = format!(
            "abc123{f}deadbeef cafebabe{f}Alice | Bob{f}1700000000{f}HEAD -> refs/heads/main{f}fix: stuff{r}def456{f}abc123{f}Carol{f}1700000100{f}{f}initial commit",
            f = FIELD_SEP,
            r = RECORD_SEP
        );
        let commits = parse_git_log(&s);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc123");
        assert_eq!(commits[0].parents, vec!["deadbeef", "cafebabe"]);
        assert_eq!(commits[0].author, "Alice | Bob"); // pipe survives
        assert_eq!(commits[0].subject, "fix: stuff");
        assert_eq!(
            commits[0].refs,
            vec!["head:".to_string(), "branch:main".to_string()]
        );
        assert_eq!(commits[1].hash, "def456");
        assert_eq!(commits[1].refs, Vec::<String>::new());
    }
}

#[cfg(test)]
mod porcelain_tests {
    use super::*;

    #[test]
    fn detects_upstream_when_present() {
        let stdout = "## main...origin/main\n";
        let (_b, _a, _be, has_upstream, _s, _c, _u) = parse_porcelain_v1(stdout);
        assert!(has_upstream, "main...origin/main should have upstream");
    }

    #[test]
    fn no_upstream_when_branch_alone() {
        let stdout = "## main\n";
        let (b, _, _, has_upstream, _, _, _) = parse_porcelain_v1(stdout);
        assert_eq!(b.as_deref(), Some("main"));
        assert!(!has_upstream, "## main has no upstream");
    }

    #[test]
    fn no_upstream_when_trailing_dots_empty() {
        // Edge case: `git status -b` for a branch with no upstream sometimes
        // emits `## branch...` with nothing after — must NOT be treated as
        // tracking, otherwise the UI would hide its "no upstream" warning.
        let stdout = "## feature/x...\n";
        let (b, _, _, has_upstream, _, _, _) = parse_porcelain_v1(stdout);
        assert_eq!(b.as_deref(), Some("feature/x"));
        assert!(!has_upstream, "trailing ... with empty rhs is no upstream");
    }

    #[test]
    fn parses_ahead_behind_with_upstream() {
        let stdout = "## main...origin/main [ahead 1, behind 2]\n M src/foo.rs\n";
        let (b, ahead, behind, has_upstream, _, changes, _) = parse_porcelain_v1(stdout);
        assert_eq!(b.as_deref(), Some("main"));
        assert_eq!(ahead, 1);
        assert_eq!(behind, 2);
        assert!(has_upstream);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/foo.rs");
    }

    #[test]
    fn numstat_parses_basic_lines() {
        let stdout = "12\t3\tsrc/foo.rs\n0\t5\tsrc/bar.rs\n";
        let m = parse_numstat(stdout);
        assert_eq!(m.get("src/foo.rs"), Some(&(12, 3)));
        assert_eq!(m.get("src/bar.rs"), Some(&(0, 5)));
    }

    #[test]
    fn numstat_clamps_binary_dashes() {
        // Binary changes show `-\t-\tpath` — must clamp to 0/0 instead of
        // panicking or returning negatives.
        let stdout = "-\t-\tassets/logo.png\n4\t2\tsrc/baz.rs\n";
        let m = parse_numstat(stdout);
        assert_eq!(m.get("assets/logo.png"), Some(&(0, 0)));
        assert_eq!(m.get("src/baz.rs"), Some(&(4, 2)));
    }

    #[test]
    fn numstat_handles_renames() {
        // `git diff --numstat` rename form: `old => new` in the path slot.
        // We key by the new path so it lines up with porcelain output.
        let stdout = "1\t1\tsrc/old.rs => src/new.rs\n";
        let m = parse_numstat(stdout);
        assert_eq!(m.get("src/new.rs"), Some(&(1, 1)));
    }

    #[test]
    fn detached_head_has_no_upstream() {
        // The current `split_once(' ')` strips the `(no branch)` suffix and
        // surfaces the literal `HEAD` as the branch name — that's pre-existing
        // behavior. The new contract we need to lock in here is just that
        // detached HEAD never reports an upstream tracking ref.
        let stdout = "## HEAD (no branch)\n";
        let (_, _, _, has_upstream, _, _, _) = parse_porcelain_v1(stdout);
        assert!(!has_upstream);
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Mini tempdir (no `tempfile` dep, matches the pattern in project.rs tests).
    struct TempDir {
        path: PathBuf,
    }
    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
            let pid = std::process::id();
            let mut path = std::env::temp_dir();
            path.push(format!("ridge-scan-{tag}-{pid}-{n}"));
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
        fn mkdir(&self, rel: &str) -> PathBuf {
            let p = self.path.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
        fn mkrepo(&self, rel: &str) -> PathBuf {
            let p = self.mkdir(rel);
            std::fs::create_dir_all(p.join(".git")).unwrap();
            p
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn norm(p: &Path) -> String {
        super::canonicalize_cwd(p)
    }

    #[tokio::test]
    async fn finds_repo_at_the_scan_root() {
        let td = TempDir::new("root");
        td.mkrepo("");
        let out = find_git_repos_below(td.path.to_string_lossy().into(), Some(1)).await;
        assert_eq!(out, vec![norm(&td.path)]);
    }

    #[tokio::test]
    async fn finds_nested_repos_up_to_max_depth() {
        let td = TempDir::new("nested");
        let a = td.mkrepo("a");
        let b = td.mkrepo("sub/b");
        let mut out = find_git_repos_below(td.path.to_string_lossy().into(), Some(4)).await;
        out.sort();
        let mut expected = vec![norm(&a), norm(&b)];
        expected.sort();
        assert_eq!(out, expected);
    }

    #[tokio::test]
    async fn does_not_recurse_into_found_repo() {
        // A repo inside a repo should only surface the outer one — matches the
        // "`.git` marks a boundary" contract at line 228 (`continue`).
        let td = TempDir::new("boundary");
        let outer = td.mkrepo("outer");
        // Nested `.git` inside `outer/sub` — scanner should NOT report it.
        std::fs::create_dir_all(outer.join("sub/.git")).unwrap();
        let out = find_git_repos_below(td.path.to_string_lossy().into(), Some(4)).await;
        assert_eq!(out, vec![norm(&outer)]);
    }

    #[tokio::test]
    async fn skips_node_modules_and_target_trees() {
        let td = TempDir::new("skip");
        // Planting a repo inside node_modules / target should NOT be discovered,
        // saving us a huge BFS on a typical monorepo install.
        td.mkrepo("node_modules/some-pkg");
        td.mkrepo("target/cache-of-cargo");
        td.mkrepo(".idea/inline-git");
        // …but a real repo alongside should still show up.
        let real = td.mkrepo("app");
        let mut out = find_git_repos_below(td.path.to_string_lossy().into(), Some(4)).await;
        out.sort();
        assert_eq!(out, vec![norm(&real)]);
    }

    #[tokio::test]
    async fn respects_max_depth() {
        let td = TempDir::new("depth");
        let deep = td.mkrepo("l1/l2/l3/l4/l5");
        // Depth 2 means we can descend 2 levels from root; repo at L5 is out of reach.
        let out = find_git_repos_below(td.path.to_string_lossy().into(), Some(2)).await;
        assert!(out.is_empty(), "expected empty, got {out:?}");
        // Depth 5 finds it.
        let out = find_git_repos_below(td.path.to_string_lossy().into(), Some(5)).await;
        assert_eq!(out, vec![norm(&deep)]);
    }
}