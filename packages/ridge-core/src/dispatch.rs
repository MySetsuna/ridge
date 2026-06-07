//! The single stringly-typed dispatch entry (GM decision **D-S1-1**).
//!
//! ```text
//! dispatch(method: &str, args: serde_json::Value, ctx: &Ctx)
//!     -> Result<serde_json::Value, CoreError>
//! ```
//!
//! The boundary is **stringly-typed** (`args` is a `serde_json::Value`,
//! `method` is a `&str`) — it sits flush against the wire `invoke` form and
//! carries the lowest "zero behaviour change" refactor risk. Per D-S1-1,
//! internal hot/error-prone commands may later converge onto typed enums; the
//! two can coexist behind this same boundary.
//!
//! ## Order of checks (matches the legacy desktop dispatcher)
//!
//! 1. **Capability admission (D8)** — `ctx.capabilities()` must allow `method`,
//!    else [`CoreError::CapabilityDenied`]. This is the whitelist-as-data gate;
//!    host-privileged commands are simply absent from the remote set.
//! 2. **Path-traversal guard** — any `..` in a path-bearing arg is rejected
//!    with [`CoreError::PathTraversal`] (mirrors the legacy `path_has_traversal`
//!    sweep over `path`/`from`/`to`/`repoRoot`/`root`/`cwd`/`paths`).
//! 3. **Sandbox / root-scoping guard (D8 / §5.6, R10)** — when the host has
//!    injected workspace roots via
//!    [`CapabilitySet::with_roots`](crate::capability::CapabilitySet::with_roots),
//!    every path-bearing arg must resolve **inside** some allowed root, else
//!    [`CoreError::OutsideSandbox`]. With **no roots configured the guard is a
//!    no-op** (unrestricted), preserving today's desktop / LAN behaviour. This
//!    is what stops a remote `fs` command on the headless host from escaping the
//!    workspace into `~/.ssh` / `/etc`.
//! 4. **Method table** — the `match` over migrated handlers. A method that is
//!    *allowed* but **not yet migrated** returns [`CoreError::MethodNotFound`];
//!    during the migration window the host bridge keeps serving those from the
//!    legacy dispatcher (see `s1-migration-ledger.md`), so this never regresses
//!    desktop behaviour.
//!
//! The read-only gate (mutating-invoke rejection) is host-policy that depends
//! on host state; it is applied by the host before/around `dispatch` for now
//! (the desktop wrapper preserves its existing `is_mutating_invoke` check) and
//! is a candidate to fold into the capability layer in a later slice — noted in
//! the ledger.

use serde_json::Value;

use crate::commands::{settings, shell, theme};
use crate::ctx::Ctx;
use crate::error::{CoreError, CoreResult};
use crate::fs::commands as fs_commands;
use crate::sandbox::RootScope;

/// Path-bearing argument keys swept for `..` traversal, identical to the legacy
/// desktop dispatcher's list.
const PATH_KEYS: &[&str] = &["path", "from", "to", "repoRoot", "root", "cwd"];

/// Visit every path-bearing string argument (the scalar [`PATH_KEYS`] plus each
/// element of the `paths` array), short-circuiting on the first `Err` from
/// `visit`. Both the traversal guard and the sandbox guard walk the exact same
/// argument surface, so they share this iterator.
fn for_each_path_arg<F>(args: &Value, mut visit: F) -> CoreResult<()>
where
    F: FnMut(&str) -> CoreResult<()>,
{
    for key in PATH_KEYS {
        if let Some(v) = args.get(*key).and_then(|x| x.as_str()) {
            visit(v)?;
        }
    }
    if let Some(arr) = args.get("paths").and_then(|x| x.as_array()) {
        for v in arr.iter().filter_map(|x| x.as_str()) {
            visit(v)?;
        }
    }
    Ok(())
}

/// True if `value` contains a `..` path-traversal segment. Mirrors the legacy
/// `path_has_traversal`: a `..` bounded by path separators (or string ends).
pub fn path_has_traversal(value: &str) -> bool {
    value.split(['/', '\\']).any(|seg| seg == "..")
}

/// Reject the request if any path-bearing arg contains traversal.
fn traversal_guard(args: &Value) -> CoreResult<()> {
    for_each_path_arg(args, |v| {
        if path_has_traversal(v) {
            Err(CoreError::PathTraversal)
        } else {
            Ok(())
        }
    })
}

/// Reject the request if any path-bearing arg resolves **outside** the host's
/// granted workspace roots (D8 / §5.6, R10). A no-op when `scope` is
/// unrestricted (empty roots), so this never changes behaviour for hosts that
/// inject no roots — the backward-compatible desktop / LAN default.
fn sandbox_guard(args: &Value, scope: &RootScope) -> CoreResult<()> {
    if scope.is_unrestricted() {
        return Ok(()); // fast path: sandbox off
    }
    for_each_path_arg(args, |v| {
        if scope.is_allowed(v) {
            Ok(())
        } else {
            Err(CoreError::OutsideSandbox)
        }
    })
}

/// Extract an optional string arg (camelCase keys, as the frontend sends).
fn opt_s(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|x| x.as_str()).map(String::from)
}

/// Extract a string arg, defaulting to `""` when absent (mirrors the legacy
/// LAN dispatcher's `s()` extractor, so empty-arg behaviour is identical).
fn s(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// Extract an optional bool arg.
fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|x| x.as_bool())
}

/// Extract an optional `usize` arg.
fn opt_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(|x| x.as_u64()).map(|n| n as usize)
}

/// Extract an optional string-array arg (e.g. include/exclude globs).
fn opt_vec_s(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(|x| x.as_array()).map(|a| {
        a.iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    })
}

/// Dispatch one request to the matching migrated handler.
///
/// See module docs for the order of admission / guard / table checks.
pub fn dispatch(method: &str, args: Value, ctx: &Ctx) -> CoreResult<Value> {
    // 1. Capability admission (D8).
    if !ctx.capabilities().is_allowed(method) {
        tracing::warn!(target: "ridge::core::dispatch", method, "capability denied");
        return Err(CoreError::CapabilityDenied(method.to_string()));
    }

    // 1.5 Read-only session gate (D-GM-9 / S1 ledger §3.1). A read-only
    // capability set refuses any mutating method up front, mirroring the legacy
    // `server.rs::is_mutating_invoke` + `remote_fs_readonly` pre-check — but now
    // enforced inside `dispatch`, so the headless host (which bypasses
    // `server.rs`) is covered too. No-op when the set is writable (the default),
    // so existing hosts are unaffected.
    if ctx.capabilities().is_readonly() && crate::capability::is_mutating(method) {
        tracing::warn!(target: "ridge::core::dispatch", method, "rejected mutating method: read-only");
        return Err(CoreError::ReadOnly);
    }

    // 2. Path-traversal guard.
    traversal_guard(&args)?;

    // 3. Sandbox / root-scoping guard (D8 / §5.6, R10). No-op when unrestricted.
    sandbox_guard(&args, ctx.capabilities().root_scope())?;

    // 4. Method table (vertical slice: settings + theme).
    match method {
        "get_theme_data" => {
            let tf = theme::get_theme_data();
            serde_json::to_value(tf).map_err(CoreError::internal)
        }
        "set_active_theme" => {
            let id = opt_s(&args, "themeId").ok_or_else(|| {
                CoreError::InvalidArgs("missing required arg: themeId".to_string())
            })?;
            theme::set_active_theme(&id)?;
            Ok(Value::Null)
        }
        "set_user_default_cwd" => {
            settings::set_user_default_cwd(ctx, opt_s(&args, "path"))?;
            Ok(Value::Null)
        }

        // ── Read-only filesystem (S5) ──
        // `get_file_tree` / `get_directory_children` / `read_file` /
        // `path_exists` / `read_file_for_editor` are line-for-line ports of the
        // matching read-only commands in `src-tauri/src/commands/project.rs`.
        "get_file_tree" => {
            let node = fs_commands::get_file_tree(&s(&args, "path"), opt_usize(&args, "depth"))?;
            serde_json::to_value(node).map_err(CoreError::internal)
        }
        "get_directory_children" => {
            let page = fs_commands::get_directory_children(
                &s(&args, "path"),
                opt_usize(&args, "offset"),
                opt_usize(&args, "limit"),
            )?;
            serde_json::to_value(page).map_err(CoreError::internal)
        }
        "read_file" => {
            let text = fs_commands::read_file(&s(&args, "path"))?;
            Ok(Value::String(text))
        }
        "path_exists" => Ok(Value::Bool(fs_commands::path_exists(&s(&args, "path")))),
        "read_file_for_editor" => {
            let res = fs_commands::read_file_for_editor(&s(&args, "path"))?;
            serde_json::to_value(res).map_err(CoreError::internal)
        }

        // ── Search (S5) ──
        // `text_search` is the desktop/LAN WS command name; `search` is the
        // alias the headless ridge-cli control protocol uses (`ControlMsg::
        // Search`). Both route through the single `fs::commands::search` port.
        "text_search" | "search" => {
            let sargs = fs_commands::TextSearchArgs {
                case_sensitive: opt_bool(&args, "caseSensitive"),
                use_regex: opt_bool(&args, "useRegex"),
                whole_word: opt_bool(&args, "wholeWord"),
                max_results: opt_usize(&args, "maxResults"),
                include_globs: opt_vec_s(&args, "includeGlobs"),
                exclude_globs: opt_vec_s(&args, "excludeGlobs"),
            };
            let results = fs_commands::search(&s(&args, "root"), &s(&args, "query"), &sargs)?;
            serde_json::to_value(results).map_err(CoreError::internal)
        }

        // ── Filename search + glob diagnostics (S5+) ──
        "filename_search" => {
            let hits = fs_commands::filename_search(&s(&args, "root"), &s(&args, "pattern"))
                .map_err(CoreError::internal)?;
            serde_json::to_value(hits).map_err(CoreError::internal)
        }
        "text_search_diagnostics" => {
            let bad = fs_commands::text_search_diagnostics(
                opt_vec_s(&args, "includeGlobs"),
                opt_vec_s(&args, "excludeGlobs"),
            );
            serde_json::to_value(bad).map_err(CoreError::internal)
        }

        // ── Shell discovery / history + directory browse (S1+) ──
        // Pure system / fs reads, valuable to a headless host serving a remote
        // IDE. `get_shell_history`'s legacy `shellKind` arg is ignored (as it was
        // on the desktop). `browse_directory`'s `path` is optional.
        "detect_available_shells" => {
            serde_json::to_value(shell::detect_available_shells()).map_err(CoreError::internal)
        }
        "get_shell_history" => {
            let lines = shell::get_shell_history().map_err(CoreError::internal)?;
            serde_json::to_value(lines).map_err(CoreError::internal)
        }
        "browse_directory" => {
            let listing =
                fs_commands::browse_directory(opt_s(&args, "path")).map_err(CoreError::internal)?;
            serde_json::to_value(listing).map_err(CoreError::internal)
        }

        // ── Filesystem writes (S1 ledger §2.1) ──
        // Mutating — guarded above by the read-only gate, and by the traversal +
        // sandbox guards. The handlers' exact (Chinese) error strings are wrapped
        // in `CoreError::Internal` so `to_command_string` / `to_json_rpc` render
        // them verbatim.
        "write_file" => {
            fs_commands::write_file(s(&args, "path"), s(&args, "content"))
                .map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "apply_file_edits" => {
            let edits: Vec<fs_commands::TextEdit> =
                serde_json::from_value(args.get("edits").cloned().unwrap_or(Value::Null))
                    .map_err(|e| CoreError::InvalidArgs(format!("invalid edits: {e}")))?;
            fs_commands::apply_file_edits(s(&args, "path"), edits).map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "rename_path" => {
            fs_commands::rename_path(s(&args, "from"), s(&args, "to"))
                .map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "delete_path" => {
            fs_commands::delete_path(s(&args, "path")).map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "create_file" => {
            fs_commands::create_file(s(&args, "path")).map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "create_directory" => {
            fs_commands::create_directory(s(&args, "path")).map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "copy_path" => {
            fs_commands::copy_path(s(&args, "from"), s(&args, "to"), opt_bool(&args, "overwrite"))
                .map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "move_path" => {
            fs_commands::move_path(s(&args, "from"), s(&args, "to")).map_err(CoreError::internal)?;
            Ok(Value::Null)
        }
        "replace_in_files" => {
            let stats = fs_commands::replace_in_files(
                s(&args, "root"),
                s(&args, "search"),
                s(&args, "replace"),
                opt_vec_s(&args, "files").unwrap_or_default(),
                opt_bool(&args, "caseSensitive"),
                opt_bool(&args, "useRegex"),
            )
            .map_err(CoreError::internal)?;
            serde_json::to_value(stats).map_err(CoreError::internal)
        }

        other => Err(CoreError::MethodNotFound(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::commands::settings::{HostStateAccessor, UserDefaultCwdStore};
    use crate::ctx::test_support::{ctx_with_state, EmptyState};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn capability_denied_for_method_not_in_set() {
        let (ctx, _sink) = ctx_with_state(
            Arc::new(EmptyState),
            CapabilitySet::from_methods(["get_theme_data"]),
        );
        let err = dispatch("set_remote_enabled", Value::Null, &ctx).unwrap_err();
        assert_eq!(err.kind_tag(), "capability_denied");
    }

    #[test]
    fn traversal_rejected_before_handler_runs() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::allow_all());
        let args = serde_json::json!({ "path": "../../etc/passwd" });
        let err = dispatch("read_file", args, &ctx).unwrap_err();
        assert_eq!(err.kind_tag(), "path_traversal");
    }

    #[test]
    fn get_theme_data_returns_a_catalog_value() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        let out = dispatch("get_theme_data", Value::Null, &ctx).unwrap();
        // Shape: { version, themes: [...] } — the same JSON the desktop emits.
        assert!(out.get("version").is_some());
        assert!(out.get("themes").is_some());
    }

    #[test]
    fn set_active_theme_requires_theme_id() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        let err = dispatch("set_active_theme", serde_json::json!({}), &ctx).unwrap_err();
        assert_eq!(err.kind_tag(), "invalid_args");
    }

    #[derive(Default)]
    struct FakeStore {
        last: Mutex<Option<Option<PathBuf>>>,
    }
    impl UserDefaultCwdStore for FakeStore {
        fn set_user_default_cwd(&self, path: Option<PathBuf>) {
            *self.last.lock().unwrap() = Some(path);
        }
    }

    #[test]
    fn set_user_default_cwd_routes_through_state() {
        let store = Arc::new(FakeStore::default());
        let accessor: Arc<dyn crate::ctx::CoreState> = Arc::new(HostStateAccessor(store.clone()));
        let (ctx, _sink) = ctx_with_state(accessor, CapabilitySet::remote_default());
        let out = dispatch(
            "set_user_default_cwd",
            serde_json::json!({ "path": "/work" }),
            &ctx,
        )
        .unwrap();
        assert_eq!(out, Value::Null);
        assert_eq!(
            *store.last.lock().unwrap(),
            Some(Some(PathBuf::from("/work")))
        );
    }

    #[test]
    fn allowed_but_unmigrated_method_is_method_not_found() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        // `split_pane` is in the remote allow-list but not yet migrated — the
        // pane / terminal / workspace domain commands still live in `src-tauri`
        // (the fs read+write, search, theme/settings slices are migrated). A
        // method that is allowed but absent from the table is MethodNotFound.
        let err = dispatch("split_pane", serde_json::json!({"paneId": "x"}), &ctx).unwrap_err();
        assert_eq!(err.kind_tag(), "method_not_found");
    }

    #[test]
    fn read_only_fs_commands_are_migrated() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        // `path_exists` on a path that doesn't exist returns `false` (infallible),
        // proving the arm is wired (a non-migrated method would be MethodNotFound).
        let out = dispatch(
            "path_exists",
            serde_json::json!({ "path": "definitely/not/here/xyz" }),
            &ctx,
        )
        .unwrap();
        assert_eq!(out, serde_json::Value::Bool(false));
    }

    // ── Sandbox / root-scoping at the dispatch boundary (D8 / §5.6, R10) ──

    #[test]
    fn sandbox_rejects_path_outside_roots() {
        let caps = CapabilitySet::remote_default().with_roots(["/work/project"]);
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), caps);
        let err = dispatch(
            "read_file",
            serde_json::json!({ "path": "/etc/passwd" }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind_tag(), "outside_sandbox");
    }

    #[test]
    fn sandbox_rejects_dotdot_escape_to_outside_root() {
        // `..` survives only if it does not contain a literal `..` segment; here
        // we use an absolute sibling so the traversal guard does not pre-empt,
        // proving the sandbox guard itself does the containment work.
        let caps = CapabilitySet::remote_default().with_roots(["/work/project"]);
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), caps);
        let err = dispatch(
            "get_file_tree",
            serde_json::json!({ "path": "/work/other-secret" }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind_tag(), "outside_sandbox");
    }

    #[test]
    fn sandbox_literal_dotdot_is_still_path_traversal() {
        // A literal `..` segment is caught by the traversal guard *before* the
        // sandbox guard runs, so it surfaces as path_traversal (defence in depth).
        let caps = CapabilitySet::remote_default().with_roots(["/work/project"]);
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), caps);
        let err = dispatch(
            "read_file",
            serde_json::json!({ "path": "/work/project/../secret" }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind_tag(), "path_traversal");
    }

    #[test]
    fn sandbox_allows_path_inside_root() {
        // A real temp dir as the root; a path inside it passes the sandbox and
        // reaches the handler (which returns the normal "not a file" error, NOT
        // outside_sandbox — proving the guard let it through).
        let td = std::env::temp_dir().join(format!(
            "ridge-core-dispatch-sandbox-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&td).unwrap();
        let root = td.to_string_lossy().into_owned();
        let inside = td.join("nope.txt").to_string_lossy().into_owned();

        let caps = CapabilitySet::remote_default().with_roots([root]);
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), caps);
        let err = dispatch("read_file", serde_json::json!({ "path": inside }), &ctx).unwrap_err();
        // Inside the root ⇒ NOT outside_sandbox; handler ran and said "missing".
        assert_eq!(err.kind_tag(), "internal");
        assert!(err.to_command_string().contains("does not exist"));

        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn no_roots_means_unrestricted_backward_compatible() {
        // remote_default() with NO roots injected = today's behaviour: an
        // absolute outside path is NOT a sandbox error (handler runs normally).
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        let err = dispatch(
            "read_file",
            serde_json::json!({ "path": "/definitely/not/here/xyz.txt" }),
            &ctx,
        )
        .unwrap_err();
        // Reaches the handler (missing file) rather than being sandbox-rejected.
        assert_eq!(err.kind_tag(), "internal");
    }

    #[test]
    fn sandbox_guards_paths_array_too() {
        let caps = CapabilitySet::remote_default().with_roots(["/work/project"]);
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), caps);
        // `delete_path` is allow-listed; the `paths` array carries an escapee.
        let err = dispatch(
            "delete_path",
            serde_json::json!({ "paths": ["/work/project/a.txt", "/etc/hosts"] }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind_tag(), "outside_sandbox");
    }

    #[test]
    fn search_alias_routes_to_same_handler() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        // Both `search` and `text_search` reach the same port; a missing root
        // yields the legacy "Root path does not exist" message under either name.
        for method in ["search", "text_search"] {
            let err = dispatch(
                method,
                serde_json::json!({ "root": "no/such/root/zzz", "query": "x" }),
                &ctx,
            )
            .unwrap_err();
            assert_eq!(err.kind_tag(), "internal");
            assert!(err.to_command_string().contains("Root path does not exist"));
        }
    }

    // ── Read-only session gate (D-GM-9 / S1 ledger §3.1) ──

    #[test]
    fn readonly_session_rejects_mutating_method() {
        let (ctx, _sink) = ctx_with_state(
            Arc::new(EmptyState),
            CapabilitySet::remote_default().with_readonly(true),
        );
        let err = dispatch(
            "write_file",
            serde_json::json!({ "path": "x.txt", "content": "y" }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind_tag(), "read_only");
        // Same message the legacy desktop read-only gate returned.
        assert_eq!(err.to_command_string(), "remote filesystem is read-only");
    }

    #[test]
    fn readonly_session_still_allows_reads() {
        let (ctx, _sink) = ctx_with_state(
            Arc::new(EmptyState),
            CapabilitySet::remote_default().with_readonly(true),
        );
        // A non-mutating read is NOT gated: it reaches the handler (missing file
        // ⇒ internal), proving read-only only blocks mutations.
        let err = dispatch(
            "read_file",
            serde_json::json!({ "path": "definitely/nope/xyz.txt" }),
            &ctx,
        )
        .unwrap_err();
        assert_eq!(err.kind_tag(), "internal");
    }

    #[test]
    fn writable_session_runs_write_file_and_round_trips() {
        let td = std::env::temp_dir().join(format!(
            "ridge-core-dispatch-write-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&td).unwrap();
        let file = td.join("w.txt").to_string_lossy().into_owned();

        // remote_default() is writable by default; no roots ⇒ sandbox off.
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        let out = dispatch(
            "write_file",
            serde_json::json!({ "path": file, "content": "hello" }),
            &ctx,
        )
        .unwrap();
        assert_eq!(out, Value::Null);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");

        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn shell_and_browse_arms_are_wired() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());

        // `detect_available_shells` returns a JSON array (≥0 entries).
        let shells = dispatch("detect_available_shells", Value::Null, &ctx).unwrap();
        assert!(shells.is_array());

        // `browse_directory` on the (existing) temp dir returns a listing whose
        // `path`/`subdirs` are present — proving the arm is wired, not MethodNotFound.
        let td = std::env::temp_dir().to_string_lossy().into_owned();
        let listing = dispatch("browse_directory", serde_json::json!({ "path": td }), &ctx).unwrap();
        assert!(listing.get("path").and_then(|v| v.as_str()).is_some());
        assert!(listing.get("subdirs").map(|v| v.is_array()).unwrap_or(false));
    }
}
