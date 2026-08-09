//! Git commands — **migrated to `ridge-core` (S1 ledger §2.1 "易迁")**.
//!
//! The implementation now lives in `packages/ridge-core/src/commands/git.rs`
//! (Tauri-free, the single source of truth shared with the headless `ridge-cli`
//! host). This module is a thin layer:
//!
//!   - it **re-exports** the public types + the one non-command helper
//!     (`git_info_for_path`) so every existing `crate::commands::git::…`
//!     reference (`remote/server.rs`, `commands/terminal.rs`) compiles
//!     unchanged;
//!   - it keeps the **`#[tauri::command]` registration wrappers** (the macro must
//!     sit in the desktop crate; `tauri::generate_handler!` references them by
//!     `commands::git::*`). Each wrapper is a one-line delegation to the core fn,
//!     preserving the exact signature + `Result<T, String>` shape — desktop
//!     behaviour is byte-for-byte identical, and the LAN/cloud remote path keeps
//!     calling these wrappers (which now delegate) so it is untouched.
//!
//! Concurrency back-pressure (`spawn_git_blocking` + the global git semaphore)
//! moved into `ridge-core` with the logic, so the desktop host and the headless
//! `ridge-cli` host share one gate instead of each re-deriving it.

// Public types + the non-command helper that `remote/server.rs` calls directly.
// `crate::commands::git::{Type|git_info_for_path}` paths resolve through these.
pub use ridge_core::commands::git::{
    git_info_for_path, BranchInfo, CommitFileEntry, CommitNode, GitDiffStatus, GitDiffSummary,
    GitFileVersions, GitGuardStats, GitOpInProgress, GitRepoInfo, ScmFile, ScmRepoStatus,
    StashEntry,
};

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

async fn run_kernel_git_mutation(request: Value) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::mutate_domain_git(&endpoint, &request)
    })
    .await
    .map_err(|error| format!("kernel Git mutation task failed: {error}"))?
}

fn mutation_output(value: Value) -> String {
    value
        .get("output")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

async fn run_kernel_git_read(request: Value) -> Result<Option<Value>, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::read_domain_git::<Value>(&endpoint, &request)
    })
    .await
    .map_err(|error| format!("kernel Git read task failed: {error}"))?
}

fn decode_kernel_git_read<T: DeserializeOwned>(
    value: Option<Value>,
    repo_root: &str,
) -> Result<T, String> {
    let value = value.ok_or_else(|| format!("Not a git repo: {repo_root}"))?;
    serde_json::from_value(value)
        .map_err(|error| format!("decode kernel Git read response: {error}"))
}

// ── `#[tauri::command]` registration wrappers (delegate to ridge-core) ──

/// OP-GIT-BYPASS: desktop diagnostics for timeout kills / acquire timeouts / caps.
#[tauri::command]
pub fn get_git_guard_stats() -> GitGuardStats {
    ridge_core::commands::git::git_guard_stats()
}

#[tauri::command]
pub fn is_git_repo(path: String) -> bool {
    ridge_core::commands::git::is_git_repo(path)
}

#[tauri::command]
pub fn find_git_repo_root(path: String) -> Option<String> {
    ridge_core::commands::git::find_git_repo_root(path)
}

#[tauri::command]
pub async fn find_git_repos_below(path: String, max_depth: Option<usize>) -> Vec<String> {
    ridge_core::commands::git::find_git_repos_below(path, max_depth).await
}

#[tauri::command]
pub async fn get_scm_status(
    repo_root: String,
    _slot: Option<String>,
) -> Result<ScmRepoStatus, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::read_domain_git_status(&endpoint, &repo_root)?
            .ok_or_else(|| format!("Not a git repo: {repo_root}"))
    })
    .await
    .map_err(|error| format!("kernel Git status task failed: {error}"))?
}

/// Remote first-paint variant; skips line-count subprocesses that the compact
/// mobile Git panel does not render.
pub async fn get_scm_status_fast(
    repo_root: String,
    _slot: Option<String>,
) -> Result<ScmRepoStatus, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::read_domain_git_status_fast(&endpoint, &repo_root)?
            .ok_or_else(|| format!("Not a git repo: {repo_root}"))
    })
    .await
    .map_err(|error| format!("kernel fast Git status task failed: {error}"))?
}

#[tauri::command]
pub async fn git_stage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "stage",
        "repo_root": repo_root,
        "paths": paths,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_unstage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "unstage",
        "repo_root": repo_root,
        "paths": paths,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_discard(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "discard",
        "repo_root": repo_root,
        "paths": paths,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_clean_untracked(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "clean-untracked",
        "repo_root": repo_root,
        "paths": paths,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_commit(
    repo_root: String,
    message: String,
    amend: Option<bool>,
) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "commit",
        "repo_root": repo_root,
        "message": message,
        "amend": amend,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_list_branches(repo_root: String) -> Result<Vec<BranchInfo>, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::read_domain_git_branches(&endpoint, &repo_root)?
            .ok_or_else(|| format!("Not a git repo: {repo_root}"))
    })
    .await
    .map_err(|error| format!("kernel Git branches task failed: {error}"))?
}

#[tauri::command]
pub async fn git_checkout(
    repo_root: String,
    branch: String,
    create: Option<bool>,
    base: Option<String>,
) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "checkout",
        "repo_root": repo_root,
        "branch": branch,
        "create": create,
        "base": base,
    }))
    .await
    .map(|_| ())
}

/// 分支：合并进当前分支（SCM 图谱分支右键菜单）。
#[tauri::command]
pub async fn git_merge_branch(repo_root: String, branch: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "merge-branch",
        "repo_root": repo_root,
        "branch": branch,
    }))
    .await?;
    Ok(mutation_output(response))
}

/// 分支：删除本地分支（force=true → -D）。
#[tauri::command]
pub async fn git_delete_branch(
    repo_root: String,
    branch: String,
    force: Option<bool>,
) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "delete-branch",
        "repo_root": repo_root,
        "branch": branch,
        "force": force,
    }))
    .await?;
    Ok(mutation_output(response))
}

/// 分支：重命名本地分支。
#[tauri::command]
pub async fn git_rename_branch(
    repo_root: String,
    old_name: String,
    new_name: String,
) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "rename-branch",
        "repo_root": repo_root,
        "old_name": old_name,
        "new_name": new_name,
    }))
    .await?;
    Ok(mutation_output(response))
}

/// 分支：推送到 origin 并设上游。
#[tauri::command]
pub async fn git_push_branch(repo_root: String, branch: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "push-branch",
        "repo_root": repo_root,
        "branch": branch,
    }))
    .await?;
    Ok(mutation_output(response))
}

/// 变基：当前分支变基到 onto（分支/commit）。
#[tauri::command]
pub async fn git_rebase(repo_root: String, onto: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "rebase",
        "repo_root": repo_root,
        "onto": onto,
    }))
    .await?;
    Ok(mutation_output(response))
}

/// 标签：删除本地标签。
#[tauri::command]
pub async fn git_delete_tag(repo_root: String, name: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "delete-tag",
        "repo_root": repo_root,
        "name": name,
    }))
    .await?;
    Ok(mutation_output(response))
}

/// 标签：推送到 origin。
#[tauri::command]
pub async fn git_push_tag(repo_root: String, name: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "push-tag",
        "repo_root": repo_root,
        "name": name,
    }))
    .await?;
    Ok(mutation_output(response))
}

// ── Stash ────────────────────────────────────────────────────────────────────
#[tauri::command]
pub async fn git_stash_list(repo_root: String) -> Result<Vec<StashEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::read_domain_git_stashes(&endpoint, &repo_root)?
            .ok_or_else(|| format!("Not a git repo: {repo_root}"))
    })
    .await
    .map_err(|error| format!("kernel Git stashes task failed: {error}"))?
}

#[tauri::command]
pub async fn git_stash_push(
    repo_root: String,
    message: Option<String>,
    include_untracked: Option<bool>,
) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "stash-push",
        "repo_root": repo_root,
        "message": message,
        "include_untracked": include_untracked,
    }))
    .await?;
    Ok(mutation_output(response))
}

#[tauri::command]
pub async fn git_stash_apply(repo_root: String, reference: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "stash-apply",
        "repo_root": repo_root,
        "reference": reference,
    }))
    .await?;
    Ok(mutation_output(response))
}

#[tauri::command]
pub async fn git_stash_pop(repo_root: String, reference: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "stash-pop",
        "repo_root": repo_root,
        "reference": reference,
    }))
    .await?;
    Ok(mutation_output(response))
}

#[tauri::command]
pub async fn git_stash_drop(repo_root: String, reference: String) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "stash-drop",
        "repo_root": repo_root,
        "reference": reference,
    }))
    .await?;
    Ok(mutation_output(response))
}

#[tauri::command]
pub async fn git_stash_branch(
    repo_root: String,
    branch: String,
    reference: String,
) -> Result<String, String> {
    let response = run_kernel_git_mutation(json!({
        "operation": "stash-branch",
        "repo_root": repo_root,
        "branch": branch,
        "reference": reference,
    }))
    .await?;
    Ok(mutation_output(response))
}

#[tauri::command]
pub async fn git_fetch(repo_root: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "fetch",
        "repo_root": repo_root,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_pull(repo_root: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "pull",
        "repo_root": repo_root,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_push(repo_root: String, set_upstream: Option<bool>) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "push",
        "repo_root": repo_root,
        "set_upstream": set_upstream,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_sync(repo_root: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "sync",
        "repo_root": repo_root,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub fn git_op_in_progress(repo_root: String) -> GitOpInProgress {
    ridge_core::commands::git::git_op_in_progress(repo_root)
}

#[tauri::command]
pub async fn git_cherry_pick_abort(repo_root: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "cherry-pick-abort",
        "repo_root": repo_root,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_revert_abort(repo_root: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "revert-abort",
        "repo_root": repo_root,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_cherry_pick(repo_root: String, hash: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "cherry-pick",
        "repo_root": repo_root,
        "hash": hash,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_revert(repo_root: String, hash: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "revert",
        "repo_root": repo_root,
        "hash": hash,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_diff_summary(
    repo_root: String,
    _slot: Option<String>,
) -> Result<GitDiffSummary, String> {
    tokio::task::spawn_blocking(move || {
        let endpoint = crate::kernel_lifecycle::ensure_kernel_running()?;
        ridge_kernel::client::read_domain_git_diff_summary(&endpoint, &repo_root)?
            .ok_or_else(|| format!("Not a git repo: {repo_root}"))
    })
    .await
    .map_err(|error| format!("kernel Git diff summary task failed: {error}"))?
}

#[tauri::command]
pub async fn git_get_file_versions(
    repo_root: String,
    path: String,
    cached: Option<bool>,
) -> Result<GitFileVersions, String> {
    let request = json!({
        "operation": "file-versions",
        "repo_root": repo_root,
        "path": path,
        "cached": cached,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

#[tauri::command]
pub async fn git_get_commit_files(
    repo_root: String,
    hash: String,
) -> Result<Vec<CommitFileEntry>, String> {
    let request = json!({
        "operation": "commit-files",
        "repo_root": repo_root,
        "hash": hash,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

#[tauri::command]
pub async fn git_get_file_versions_at_commit(
    repo_root: String,
    path: String,
    hash: String,
) -> Result<GitFileVersions, String> {
    let request = json!({
        "operation": "file-versions-at-commit",
        "repo_root": repo_root,
        "path": path,
        "hash": hash,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

/// 提交对比：两提交间某文件的版本对（图谱 Ctrl+Click 两提交点击文件的 diff）。
#[tauri::command]
pub async fn git_get_file_versions_between(
    repo_root: String,
    path: String,
    from: String,
    to: String,
) -> Result<GitFileVersions, String> {
    let request = json!({
        "operation": "file-versions-between",
        "repo_root": repo_root,
        "path": path,
        "from": from,
        "to": to,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

/// 提交对比：两提交间的变更文件列表。
#[tauri::command]
pub async fn git_compare_commits(
    repo_root: String,
    from: String,
    to: String,
) -> Result<Vec<ridge_core::commands::git::CommitFileEntry>, String> {
    let request = json!({
        "operation": "compare-commits",
        "repo_root": repo_root,
        "from": from,
        "to": to,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

#[tauri::command]
pub async fn git_create_tag(
    repo_root: String,
    name: String,
    hash: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "create-tag",
        "repo_root": repo_root,
        "name": name,
        "hash": hash,
        "message": message,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_reset(repo_root: String, hash: String, mode: String) -> Result<(), String> {
    run_kernel_git_mutation(json!({
        "operation": "reset",
        "repo_root": repo_root,
        "hash": hash,
        "mode": mode,
    }))
    .await
    .map(|_| ())
}

#[tauri::command]
pub async fn git_diff_file(
    repo_root: String,
    path: String,
    cached: Option<bool>,
) -> Result<String, String> {
    let request = json!({
        "operation": "diff-file",
        "repo_root": repo_root,
        "path": path,
        "cached": cached,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

/// 行级 blame：文件每行的最近提交信息（IDE FileEditor gutter/hover 用）。
#[tauri::command]
pub async fn git_blame(
    repo_root: String,
    path: String,
) -> Result<Vec<ridge_core::commands::git::BlameLine>, String> {
    let request = json!({
        "operation": "blame",
        "repo_root": repo_root,
        "path": path,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

/// 文件提交历史（IDE「查看本文件/本行历史」用，选中后复用 git_diff_file 看 diff）。
#[tauri::command]
pub async fn git_file_log(
    repo_root: String,
    path: String,
    limit: Option<u32>,
) -> Result<Vec<ridge_core::commands::git::FileCommit>, String> {
    let request = json!({
        "operation": "file-log",
        "repo_root": repo_root,
        "path": path,
        "limit": limit,
    });
    let repo_root = request["repo_root"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    decode_kernel_git_read(run_kernel_git_read(request).await?, &repo_root)
}

/// Stub retained for compatibility — see `ridge_core::commands::git`. Param
/// names preserved verbatim (`_workspace_id`/`_pane_id`) so the Tauri arg keys
/// are byte-identical to the pre-migration command.
#[tauri::command]
pub fn get_git_graph(_workspace_id: String, _pane_id: String) -> Result<GitRepoInfo, String> {
    ridge_core::commands::git::get_git_graph(_workspace_id, _pane_id)
}

#[tauri::command]
pub async fn get_git_info_with_cwd(cwd: String) -> Result<GitRepoInfo, String> {
    let request = json!({
        "operation": "info",
        "repo_root": cwd,
    });
    match run_kernel_git_read(request).await? {
        Some(value) => serde_json::from_value(value)
            .map_err(|error| format!("decode kernel Git read response: {error}")),
        None => Ok(GitRepoInfo::default()),
    }
}

#[tauri::command]
pub async fn get_git_commits_paginated(
    repo_root: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<CommitNode>, String> {
    let request = json!({
        "operation": "commits",
        "repo_root": repo_root,
        "offset": offset,
        "limit": limit,
    });
    match run_kernel_git_read(request).await? {
        Some(value) => serde_json::from_value(value)
            .map_err(|error| format!("decode kernel Git read response: {error}")),
        None => Ok(Vec::new()),
    }
}

/// Stub retained for compatibility — see `ridge_core::commands::git`.
#[tauri::command]
pub fn get_git_diff(_pane_id: String) -> Result<GitDiffStatus, String> {
    ridge_core::commands::git::get_git_diff(_pane_id)
}

/// Stub retained for compatibility (called by `commands/terminal.rs` and the
/// frontend). Param names preserved verbatim so the Tauri arg keys are identical.
#[tauri::command]
pub fn set_pane_workdir(_pane_id: String, _workdir: String) -> Result<(), String> {
    ridge_core::commands::git::set_pane_workdir(_pane_id, _workdir)
}
