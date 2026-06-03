//! Capability policy layer (decision **D8**: whitelist-as-data).
//!
//! The command-admission whitelist is **data held on the [`Ctx`](crate::ctx),
//! not code duplicated per host**. The desktop LAN server, the cloud desktop
//! host, and the headless `ridge-cli` host all construct a `CapabilitySet`
//! and hand it to the same [`dispatch`](crate::dispatch::dispatch) entry, which
//! does admission once. Re-implementing the whitelist per host is the
//! privilege-escalation hazard this layer exists to eliminate
//! (§5.4 "能力执行(D8)一致性", §7 R10).
//!
//! A `CapabilitySet` is a flat allow-list of method names. Host-privileged
//! commands (`get_remote_info`, `set_remote_enabled`, `disconnect_session`,
//! `enter_deep_root_mode`, `set_cloud_remote_active`, blacklist admin, …) are
//! simply **absent** from the remote set, so they can never be reached through
//! `dispatch` regardless of host. They remain reachable only via each host's
//! own privileged surface (e.g. the desktop `#[tauri::command]` IPC).

use std::collections::HashSet;

/// The set of method names a connection is permitted to dispatch.
///
/// Immutable after construction (build with [`CapabilitySet::from_methods`] or
/// the [`remote_default`](CapabilitySet::remote_default) preset). Membership is
/// the only question dispatch asks.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    allowed: HashSet<String>,
}

impl CapabilitySet {
    /// Build a capability set from an explicit list of allowed method names.
    pub fn from_methods<I, S>(methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed: methods.into_iter().map(Into::into).collect(),
        }
    }

    /// A capability set that allows **everything** — for the in-process
    /// desktop IPC path, where admission is already enforced by Tauri's own
    /// command registration (only registered `#[tauri::command]`s are callable).
    pub fn allow_all() -> Self {
        Self {
            allowed: HashSet::new(),
        }
        .with_allow_all()
    }

    fn with_allow_all(self) -> Self {
        // Represented by the sentinel below; see `is_allowed`.
        Self {
            allowed: ["*".to_string()].into_iter().collect(),
        }
    }

    /// True if `method` is permitted under this set.
    pub fn is_allowed(&self, method: &str) -> bool {
        self.allowed.contains("*") || self.allowed.contains(method)
    }

    /// Number of explicitly-allowed methods (the `*` sentinel counts as 1).
    pub fn len(&self) -> usize {
        self.allowed.len()
    }

    /// True if the set allows nothing.
    pub fn is_empty(&self) -> bool {
        self.allowed.is_empty()
    }

    /// The canonical **remote** capability set: the exact allow-list the
    /// LAN `dispatch_invoke_request` enforces today (fs / git / search / pane /
    /// terminal / workspace / theme), with host-privileged commands excluded.
    ///
    /// This is the single source of truth all three hosts (LAN-WS, cloud
    /// desktop, headless) share. As handlers migrate into `ridge-core`, the
    /// `dispatch` table grows; this list grows in lockstep. Methods present in
    /// this list but not yet migrated route through the host's bridge fallback
    /// during the migration window (see `s1-migration-ledger.md`).
    pub fn remote_default() -> Self {
        Self::from_methods(REMOTE_ALLOWLIST.iter().copied())
    }
}

/// The remote allow-list as a data constant (D8). Kept in one place so the
/// three hosts cannot drift. Mirrors the `match` arms in the legacy
/// `dispatch_invoke_request` (`src-tauri/src/remote/server.rs`), minus the
/// deliberately-excluded host-privileged commands documented there.
pub const REMOTE_ALLOWLIST: &[&str] = &[
    // ── Filesystem ──
    "get_file_tree",
    "get_directory_children",
    "path_exists",
    "read_file",
    "write_file",
    "apply_file_edits",
    "rename_path",
    "delete_path",
    "create_file",
    "create_directory",
    "copy_path",
    "move_path",
    "reveal_in_file_manager",
    "read_file_for_editor",
    "get_current_project",
    // ── Filesystem / git watchers ──
    "start_watching_paths",
    "start_watching_repos",
    // ── Pane / terminal ──
    "get_pane_layout",
    "get_pane_layout_for",
    "split_pane",
    "dock_pane",
    "close_pane",
    "toggle_mode",
    "set_split_ratios_at_path",
    "set_split_ratios_batch",
    "create_pane",
    "activate_pane_pty",
    "change_pane_shell",
    "write_to_pty",
    "resize_pane",
    "detect_available_shells",
    "get_shell_history",
    // ── Workspace (live) ──
    "get_active_workspace_id",
    "switch_workspace",
    "create_workspace",
    "close_workspace",
    "rename_workspace",
    "reorder_workspaces",
    // ── Workspace (persistence / .ridge) ──
    "save_workspace",
    "list_saved_workspaces",
    "delete_saved_workspace",
    "rename_saved_workspace",
    "list_workspace_save_info",
    "delete_workspace_file",
    "get_default_workspace_save_dir",
    "list_saved_workspace_files",
    "save_workspace_to_file",
    "open_workspace_from_file",
    "get_restore_set",
    "list_recent_workspaces",
    "clear_recent_workspaces",
    "get_last_opened_workspace_path",
    "get_startup_context",
    "browse_directory",
    // ── Theme / settings ──
    "get_theme_data",
    "set_active_theme",
    "set_user_default_cwd",
    // ── Search ──
    "text_search",
    // `search` is the alias the headless ridge-cli control protocol
    // (`ControlMsg::Search`) dispatches under; same handler as `text_search`.
    "search",
    "filename_search",
    "text_search_diagnostics",
    "replace_in_files",
    // ── Git (read) ──
    "find_git_repo_root",
    "find_git_repos_below",
    "get_scm_status",
    "get_git_info_with_cwd",
    "get_git_commits_paginated",
    "git_list_branches",
    "git_diff_summary",
    "git_get_file_versions",
    "git_op_in_progress",
    "git_fetch",
    // ── Git (mutating) ──
    "git_stage",
    "git_unstage",
    "git_commit",
    "git_pull",
    "git_push",
    "git_sync",
    "git_checkout",
    "git_revert",
    "git_cherry_pick",
    "git_reset",
    "git_create_tag",
    "git_discard",
    "git_clean_untracked",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_default_allows_migrated_methods() {
        let caps = CapabilitySet::remote_default();
        assert!(caps.is_allowed("set_active_theme"));
        assert!(caps.is_allowed("set_user_default_cwd"));
        assert!(caps.is_allowed("get_theme_data"));
        assert!(caps.is_allowed("read_file"));
    }

    #[test]
    fn remote_default_excludes_host_privileged_commands() {
        let caps = CapabilitySet::remote_default();
        // These are deliberately absent — the privilege-escalation guard (D8).
        assert!(!caps.is_allowed("get_remote_info"));
        assert!(!caps.is_allowed("set_remote_enabled"));
        assert!(!caps.is_allowed("disconnect_session"));
        assert!(!caps.is_allowed("enter_deep_root_mode"));
        assert!(!caps.is_allowed("set_cloud_remote_active"));
    }

    #[test]
    fn allow_all_permits_anything() {
        let caps = CapabilitySet::allow_all();
        assert!(caps.is_allowed("set_remote_enabled"));
        assert!(caps.is_allowed("anything_at_all"));
    }

    #[test]
    fn from_methods_is_exact() {
        let caps = CapabilitySet::from_methods(["a", "b"]);
        assert!(caps.is_allowed("a"));
        assert!(caps.is_allowed("b"));
        assert!(!caps.is_allowed("c"));
    }
}
