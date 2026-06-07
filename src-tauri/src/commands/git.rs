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
    GitFileVersions, GitOpInProgress, GitRepoInfo, ScmFile, ScmRepoStatus,
};

// ── `#[tauri::command]` registration wrappers (delegate to ridge-core) ──

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
pub async fn get_scm_status(repo_root: String) -> Result<ScmRepoStatus, String> {
    ridge_core::commands::git::get_scm_status(repo_root).await
}

#[tauri::command]
pub async fn git_stage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    ridge_core::commands::git::git_stage(repo_root, paths).await
}

#[tauri::command]
pub async fn git_unstage(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    ridge_core::commands::git::git_unstage(repo_root, paths).await
}

#[tauri::command]
pub async fn git_discard(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    ridge_core::commands::git::git_discard(repo_root, paths).await
}

#[tauri::command]
pub async fn git_clean_untracked(repo_root: String, paths: Vec<String>) -> Result<(), String> {
    ridge_core::commands::git::git_clean_untracked(repo_root, paths).await
}

#[tauri::command]
pub async fn git_commit(
    repo_root: String,
    message: String,
    amend: Option<bool>,
) -> Result<(), String> {
    ridge_core::commands::git::git_commit(repo_root, message, amend).await
}

#[tauri::command]
pub async fn git_list_branches(repo_root: String) -> Result<Vec<BranchInfo>, String> {
    ridge_core::commands::git::git_list_branches(repo_root).await
}

#[tauri::command]
pub async fn git_checkout(
    repo_root: String,
    branch: String,
    create: Option<bool>,
    base: Option<String>,
) -> Result<(), String> {
    ridge_core::commands::git::git_checkout(repo_root, branch, create, base).await
}

#[tauri::command]
pub async fn git_fetch(repo_root: String) -> Result<(), String> {
    ridge_core::commands::git::git_fetch(repo_root).await
}

#[tauri::command]
pub async fn git_pull(repo_root: String) -> Result<(), String> {
    ridge_core::commands::git::git_pull(repo_root).await
}

#[tauri::command]
pub async fn git_push(repo_root: String, set_upstream: Option<bool>) -> Result<(), String> {
    ridge_core::commands::git::git_push(repo_root, set_upstream).await
}

#[tauri::command]
pub async fn git_sync(repo_root: String) -> Result<(), String> {
    ridge_core::commands::git::git_sync(repo_root).await
}

#[tauri::command]
pub fn git_op_in_progress(repo_root: String) -> GitOpInProgress {
    ridge_core::commands::git::git_op_in_progress(repo_root)
}

#[tauri::command]
pub async fn git_cherry_pick_abort(repo_root: String) -> Result<(), String> {
    ridge_core::commands::git::git_cherry_pick_abort(repo_root).await
}

#[tauri::command]
pub async fn git_revert_abort(repo_root: String) -> Result<(), String> {
    ridge_core::commands::git::git_revert_abort(repo_root).await
}

#[tauri::command]
pub async fn git_cherry_pick(repo_root: String, hash: String) -> Result<(), String> {
    ridge_core::commands::git::git_cherry_pick(repo_root, hash).await
}

#[tauri::command]
pub async fn git_revert(repo_root: String, hash: String) -> Result<(), String> {
    ridge_core::commands::git::git_revert(repo_root, hash).await
}

#[tauri::command]
pub async fn git_diff_summary(repo_root: String) -> Result<GitDiffSummary, String> {
    ridge_core::commands::git::git_diff_summary(repo_root).await
}

#[tauri::command]
pub async fn git_get_file_versions(
    repo_root: String,
    path: String,
    cached: Option<bool>,
) -> Result<GitFileVersions, String> {
    ridge_core::commands::git::git_get_file_versions(repo_root, path, cached).await
}

#[tauri::command]
pub async fn git_get_commit_files(
    repo_root: String,
    hash: String,
) -> Result<Vec<CommitFileEntry>, String> {
    ridge_core::commands::git::git_get_commit_files(repo_root, hash).await
}

#[tauri::command]
pub async fn git_get_file_versions_at_commit(
    repo_root: String,
    path: String,
    hash: String,
) -> Result<GitFileVersions, String> {
    ridge_core::commands::git::git_get_file_versions_at_commit(repo_root, path, hash).await
}

#[tauri::command]
pub async fn git_create_tag(
    repo_root: String,
    name: String,
    hash: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    ridge_core::commands::git::git_create_tag(repo_root, name, hash, message).await
}

#[tauri::command]
pub async fn git_reset(repo_root: String, hash: String, mode: String) -> Result<(), String> {
    ridge_core::commands::git::git_reset(repo_root, hash, mode).await
}

#[tauri::command]
pub async fn git_diff_file(
    repo_root: String,
    path: String,
    cached: Option<bool>,
) -> Result<String, String> {
    ridge_core::commands::git::git_diff_file(repo_root, path, cached).await
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
    ridge_core::commands::git::get_git_info_with_cwd(cwd).await
}

#[tauri::command]
pub async fn get_git_commits_paginated(
    repo_root: String,
    offset: u32,
    limit: u32,
) -> Result<Vec<CommitNode>, String> {
    ridge_core::commands::git::get_git_commits_paginated(repo_root, offset, limit).await
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
