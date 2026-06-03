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
//! 3. **Method table** — the `match` over migrated handlers. A method that is
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

use crate::commands::{settings, theme};
use crate::ctx::Ctx;
use crate::error::{CoreError, CoreResult};

/// Path-bearing argument keys swept for `..` traversal, identical to the legacy
/// desktop dispatcher's list.
const PATH_KEYS: &[&str] = &["path", "from", "to", "repoRoot", "root", "cwd"];

/// True if `value` contains a `..` path-traversal segment. Mirrors the legacy
/// `path_has_traversal`: a `..` bounded by path separators (or string ends).
pub fn path_has_traversal(value: &str) -> bool {
    value.split(['/', '\\']).any(|seg| seg == "..")
}

/// Reject the request if any path-bearing arg contains traversal.
fn traversal_guard(args: &Value) -> CoreResult<()> {
    for key in PATH_KEYS {
        if let Some(v) = args.get(*key).and_then(|x| x.as_str()) {
            if path_has_traversal(v) {
                return Err(CoreError::PathTraversal);
            }
        }
    }
    if let Some(arr) = args.get("paths").and_then(|x| x.as_array()) {
        if arr
            .iter()
            .filter_map(|x| x.as_str())
            .any(path_has_traversal)
        {
            return Err(CoreError::PathTraversal);
        }
    }
    Ok(())
}

/// Extract an optional string arg (camelCase keys, as the frontend sends).
fn opt_s(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|x| x.as_str()).map(String::from)
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

    // 2. Path-traversal guard.
    traversal_guard(&args)?;

    // 3. Method table (vertical slice: settings + theme).
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
        assert_eq!(*store.last.lock().unwrap(), Some(Some(PathBuf::from("/work"))));
    }

    #[test]
    fn allowed_but_unmigrated_method_is_method_not_found() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::remote_default());
        // `read_file` is in the remote allow-list but not yet migrated.
        let err = dispatch("read_file", serde_json::json!({"path": "x"}), &ctx).unwrap_err();
        assert_eq!(err.kind_tag(), "method_not_found");
    }
}
