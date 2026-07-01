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
use std::path::Path;

use crate::sandbox::RootScope;

/// The set of method names a connection is permitted to dispatch, plus the
/// filesystem [`RootScope`] sandbox that bounds path-bearing commands.
///
/// Immutable after construction (build with [`CapabilitySet::from_methods`] or
/// the [`remote_default`](CapabilitySet::remote_default) preset, then optionally
/// [`with_roots`](CapabilitySet::with_roots) to enable the fs sandbox).
/// Membership and root-containment are the two questions dispatch asks.
///
/// The sandbox is **off by default** (empty roots = unrestricted), so existing
/// desktop / LAN behaviour is unchanged until a host explicitly injects roots
/// (D8 / §5.6, R10).
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    allowed: HashSet<String>,
    roots: RootScope,
    /// When `true`, `dispatch` refuses any [`is_mutating`] method with
    /// [`CoreError::ReadOnly`](crate::error::CoreError::ReadOnly). Defaults to
    /// `false` (writable), so existing hosts are unaffected until they opt in —
    /// the same backward-compatible no-op posture as the empty-roots sandbox.
    readonly: bool,
}

impl CapabilitySet {
    /// Build a capability set from an explicit list of allowed method names.
    /// The fs sandbox starts **unrestricted**; enable it with
    /// [`with_roots`](CapabilitySet::with_roots).
    pub fn from_methods<I, S>(methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            allowed: methods.into_iter().map(Into::into).collect(),
            roots: RootScope::unrestricted(),
            readonly: false,
        }
    }

    /// A capability set that allows **everything** — for the in-process
    /// desktop IPC path, where admission is already enforced by Tauri's own
    /// command registration (only registered `#[tauri::command]`s are callable).
    /// The fs sandbox is unrestricted (the desktop owns the whole machine).
    pub fn allow_all() -> Self {
        Self {
            allowed: HashSet::new(),
            roots: RootScope::unrestricted(),
            readonly: false,
        }
        .with_allow_all()
    }

    fn with_allow_all(mut self) -> Self {
        // Represented by the sentinel below; see `is_allowed`.
        self.allowed = ["*".to_string()].into_iter().collect();
        self
    }

    /// Enable the filesystem sandbox by scoping path-bearing commands to the
    /// given workspace roots (D8 / §5.6, R10). Consuming-builder style so it
    /// composes with the presets: `CapabilitySet::remote_default().with_roots([..])`.
    ///
    /// Passing an **empty** iterator leaves the sandbox unrestricted (the
    /// backward-compatible default), so a host can unconditionally call this
    /// with whatever roots it has — zero roots simply means "don't sandbox".
    /// The cloud headless host injects its workspace root(s) here; the desktop
    /// host may leave it unset.
    pub fn with_roots<I, P>(mut self, roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        self.roots = RootScope::from_roots(roots);
        self
    }

    /// The filesystem sandbox scope. Empty ⇒ unrestricted (backward compatible).
    pub fn root_scope(&self) -> &RootScope {
        &self.roots
    }

    /// Mark this capability set **read-only**: `dispatch` will reject any
    /// [`is_mutating`] method with `CoreError::ReadOnly`. Consuming-builder so it
    /// composes with the presets, e.g.
    /// `CapabilitySet::remote_default().with_readonly(true)`. The desktop wires
    /// this from `AppState::remote_fs_readonly`; the headless host can expose a
    /// `--read-only` operator switch. `false` (the default) is the unchanged,
    /// writable posture.
    pub fn with_readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    /// True if this set forbids mutating methods (the read-only session gate).
    pub fn is_readonly(&self) -> bool {
        self.readonly
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
    // Paged scrollback (seq-cursor) — read-only. Lets a cloud controller seed a
    // pane with ~1.5 screens on subscribe and lazily page older history on
    // scroll-up (get_pane_scrollback_before), instead of the host dumping the
    // whole buffer at once. Same primitive the desktop RidgePane already uses.
    "get_pane_scrollback_tail",
    "get_pane_scrollback_before",
    // native (headless) tmux session discovery (desktop hosts). `list` is
    // read-only; `summon` is a structural pane op (adopts a session into the
    // caller's viewed workspace) — not a mutating fs/git method, so it is allowed
    // even in a read-only session, consistent with split/create/close pane.
    "list_native_sessions",
    "summon_native_session",
    // `new_headless_session` 起一个新无头会话；`terminate_native_session` 真正终止
    // 一个会话（杀子进程）。与 close_pane/summon 同属结构性 pane 操作（非 fs/git
    // 写），故允许只读会话调用，不列入 MUTATING_METHODS。真关闭的危险确认在前端。
    "new_headless_session",
    "terminate_native_session",
    // ── Workspace (live) ──
    // 只读：远程控制器（桌面 SPA）连上后枚举 host 工作区列表（refreshWorkspaces →
    // list_workspaces）。漏了它会导致 controller 取不到工作区 → 兜底逻辑每次连接新建一个
    // 工作区（连带 bug），故与同组读/写命令一并放行。
    "list_workspaces",
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
    "get_active_theme_entry",
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

/// Methods that MUTATE host state — the read-only session gate (D-GM-9 / S1
/// ledger §3.1) rejects these when [`CapabilitySet::is_readonly`] is set.
///
/// **Byte-for-byte mirror of `server.rs::is_mutating_invoke`** (which is
/// `is_mutating_method` ∪ {`replace_in_files`, `apply_file_edits`}). Kept as a
/// data constant in one place so the desktop pre-check and the `dispatch` gate
/// cannot drift. When new mutating commands migrate into `dispatch`, add them
/// here in lockstep.
pub const MUTATING_METHODS: &[&str] = &[
    // ── Filesystem writes ──
    "write_file",
    "apply_file_edits",
    "rename_path",
    "delete_path",
    "create_file",
    "create_directory",
    "copy_path",
    "move_path",
    "replace_in_files",
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

/// True if `method` mutates host state (see [`MUTATING_METHODS`]). The read-only
/// gate in [`dispatch`](crate::dispatch::dispatch) consults this.
pub fn is_mutating(method: &str) -> bool {
    MUTATING_METHODS.contains(&method)
}

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

    #[test]
    fn sandbox_is_unrestricted_by_default() {
        // All presets start with the sandbox off (backward compatible).
        assert!(CapabilitySet::remote_default()
            .root_scope()
            .is_unrestricted());
        assert!(CapabilitySet::allow_all().root_scope().is_unrestricted());
        assert!(CapabilitySet::from_methods(["a"])
            .root_scope()
            .is_unrestricted());
        assert!(CapabilitySet::default().root_scope().is_unrestricted());
    }

    #[test]
    fn with_roots_enables_the_sandbox_and_preserves_methods() {
        let caps = CapabilitySet::remote_default().with_roots(["/work/project"]);
        assert!(!caps.root_scope().is_unrestricted());
        // Method admission is untouched by adding roots.
        assert!(caps.is_allowed("read_file"));
        assert!(!caps.is_allowed("set_remote_enabled"));
    }

    #[test]
    fn with_empty_roots_stays_unrestricted() {
        // A host can call with_roots unconditionally; zero roots = no sandbox.
        let caps = CapabilitySet::remote_default().with_roots(Vec::<String>::new());
        assert!(caps.root_scope().is_unrestricted());
    }

    #[test]
    fn readonly_is_off_by_default_and_opt_in() {
        // Backward-compatible: presets start writable.
        assert!(!CapabilitySet::remote_default().is_readonly());
        assert!(!CapabilitySet::allow_all().is_readonly());
        assert!(!CapabilitySet::default().is_readonly());
        // Opt in, and it composes with the other builders / admission is untouched.
        let caps = CapabilitySet::remote_default()
            .with_roots(["/work"])
            .with_readonly(true);
        assert!(caps.is_readonly());
        assert!(caps.is_allowed("write_file"));
        assert!(!caps.root_scope().is_unrestricted());
    }

    #[test]
    fn mutating_set_mirrors_the_desktop_pre_check() {
        // Sample fs + git mutations are flagged…
        for m in ["write_file", "apply_file_edits", "replace_in_files", "git_commit", "git_reset"] {
            assert!(is_mutating(m), "{m} should be mutating");
        }
        // …and read-only / non-mutating methods are not.
        for m in ["read_file", "get_file_tree", "search", "get_scm_status", "git_list_branches"] {
            assert!(!is_mutating(m), "{m} should NOT be mutating");
        }
    }
}
