// Cloud host command admission allow-list (audit ①-1 remediation).
//
// SECURITY: the cloud desktop host (cloudHostBridge.ts) executes controller
// JSON-RPC `invoke` requests locally via the real Tauri `invoke`. Without an
// allow-list it forwards ANY method — a controller can call host-privileged
// commands the LAN host deliberately excludes (e.g. `get_remote_info`, which
// leaks the LAN TOTP secret → full RCE; verified live, see
// docs/plans/remote-cloud-security-audit-2026-06-07.md §5.5).
//
// SOURCE OF TRUTH: packages/ridge-core/src/capability.rs
//   - REMOTE_ALLOWLIST  (the D8 "whitelist as data" the LAN/headless hosts share)
//   - MUTATING_METHODS  (the read-only session gate set)
// This file is a **TS mirror** for the cloud host while it remains a WebView/TS
// v1 scaffold (contract §8 — host WebRTC migrates to Rust(webrtc-rs) eventually,
// at which point it consumes the canonical Rust const directly and this mirror
// is deleted). Until then: **keep this byte-for-byte in sync with capability.rs**
// when commands migrate. The vitest in remoteAllowlist.test.ts pins the counts
// so an accidental divergence is at least caught locally.

/** Mirror of `ridge_core::capability::REMOTE_ALLOWLIST` (capability.rs). */
export const REMOTE_ALLOWLIST: readonly string[] = [
  // ── Filesystem ──
  'get_file_tree',
  'get_directory_children',
  'path_exists',
  'read_file',
  'write_file',
  'apply_file_edits',
  'rename_path',
  'delete_path',
  'create_file',
  'create_directory',
  'copy_path',
  'move_path',
  'reveal_in_file_manager',
  'read_file_for_editor',
  'get_current_project',
  // ── Filesystem / git watchers ──
  'start_watching_paths',
  'start_watching_repos',
  // ── Pane / terminal ──
  'get_pane_layout',
  'get_pane_layout_for',
  'split_pane',
  'dock_pane',
  'close_pane',
  'toggle_mode',
  'set_split_ratios_at_path',
  'set_split_ratios_batch',
  'create_pane',
  'activate_pane_pty',
  'change_pane_shell',
  'write_to_pty',
  'resize_pane',
  'detect_available_shells',
  'get_shell_history',
  // native (headless) tmux session discovery (desktop hosts); `summon` is a
  // structural pane op (not in MUTATING_METHODS — allowed in read-only sessions).
  'list_native_sessions',
  'summon_native_session',
  // ── Workspace (live) ──
  'get_active_workspace_id',
  'switch_workspace',
  'create_workspace',
  'close_workspace',
  'rename_workspace',
  'reorder_workspaces',
  // ── Workspace (persistence / .ridge) ──
  'save_workspace',
  'list_saved_workspaces',
  'delete_saved_workspace',
  'rename_saved_workspace',
  'list_workspace_save_info',
  'delete_workspace_file',
  'get_default_workspace_save_dir',
  'list_saved_workspace_files',
  'save_workspace_to_file',
  'open_workspace_from_file',
  'get_restore_set',
  'list_recent_workspaces',
  'clear_recent_workspaces',
  'get_last_opened_workspace_path',
  'get_startup_context',
  'browse_directory',
  // ── Theme / settings ──
  'get_theme_data',
  'set_active_theme',
  'set_user_default_cwd',
  // ── Search ──
  'text_search',
  'search',
  'filename_search',
  'text_search_diagnostics',
  'replace_in_files',
  // ── Git (read) ──
  'find_git_repo_root',
  'find_git_repos_below',
  'get_scm_status',
  'get_git_info_with_cwd',
  'get_git_commits_paginated',
  'git_list_branches',
  'git_diff_summary',
  'git_get_file_versions',
  'git_op_in_progress',
  'git_fetch',
  // ── Git (mutating) ──
  'git_stage',
  'git_unstage',
  'git_commit',
  'git_pull',
  'git_push',
  'git_sync',
  'git_checkout',
  'git_revert',
  'git_cherry_pick',
  'git_reset',
  'git_create_tag',
  'git_discard',
  'git_clean_untracked',
];

/** Mirror of `ridge_core::capability::MUTATING_METHODS` (capability.rs). */
export const MUTATING_METHODS: readonly string[] = [
  // ── Filesystem writes ──
  'write_file',
  'apply_file_edits',
  'rename_path',
  'delete_path',
  'create_file',
  'create_directory',
  'copy_path',
  'move_path',
  'replace_in_files',
  // ── Git (mutating) ──
  'git_stage',
  'git_unstage',
  'git_commit',
  'git_pull',
  'git_push',
  'git_sync',
  'git_checkout',
  'git_revert',
  'git_cherry_pick',
  'git_reset',
  'git_create_tag',
  'git_discard',
  'git_clean_untracked',
];

const ALLOW_SET: ReadonlySet<string> = new Set(REMOTE_ALLOWLIST);
const MUTATING_SET: ReadonlySet<string> = new Set(MUTATING_METHODS);

/** True if `method` is admissible for a remote (cloud) controller. */
export function isRemoteAllowed(method: string): boolean {
  return ALLOW_SET.has(method);
}

/** True if `method` mutates host state (the read-only session gate set). */
export function isMutatingMethod(method: string): boolean {
  return MUTATING_SET.has(method);
}
