//! Settings handlers — migrated from `src-tauri/src/commands/settings.rs`.
//!
//! `set_user_default_cwd` is the simplest stateful handler: it normalises the
//! incoming string and writes it into the host's `AppState::user_default_cwd`
//! so the Rust-side cwd resolver can use it. Because `ridge-core` cannot name
//! the desktop `AppState`, the host exposes the one field this handler needs
//! through the [`UserDefaultCwdStore`] trait. The handler downcasts the
//! `Ctx` state to that trait and writes through it.
//!
//! Behaviour parity with the desktop original:
//!   - same trim + empty-filter normalisation (`""`/whitespace → clears the
//!     override; a real path → `Some(PathBuf)`);
//!   - same `Ok(())` return.

use std::path::PathBuf;

use crate::ctx::Ctx;
use crate::error::CoreResult;

/// Host-state capability needed by `set_user_default_cwd`: a slot holding the
/// user's configured default working directory. The desktop host implements
/// this on its `AppState` (writing the `user_default_cwd` `RwLock`); the
/// headless host implements it on its daemon state.
///
/// `Send + Sync` because it is reached through the `Ctx` state, which is shared
/// across handler tasks.
pub trait UserDefaultCwdStore: Send + Sync {
    /// Replace the stored default cwd. `None` clears the override.
    fn set_user_default_cwd(&self, path: Option<PathBuf>);
}

/// Normalise a raw default-cwd input exactly as the desktop original did:
/// trim, drop if empty, otherwise wrap as a `PathBuf`. Pure — unit-testable
/// without any host state.
pub fn normalize_default_cwd(path: Option<String>) -> Option<PathBuf> {
    path.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Handler: `set_user_default_cwd`. Writes the normalised path into the host
/// state's [`UserDefaultCwdStore`].
pub fn set_user_default_cwd(ctx: &Ctx, path: Option<String>) -> CoreResult<()> {
    let normalised = normalize_default_cwd(path);
    let store = ctx.state::<HostStateAccessor>()?;
    store.0.set_user_default_cwd(normalised);
    Ok(())
}

/// Internal accessor wrapper. Hosts register their state as
/// `Arc<HostStateAccessor>` (or implement [`crate::ctx::CoreState`] directly on
/// a type that derefs to a `UserDefaultCwdStore`). This indirection keeps the
/// downcast target a `ridge-core`-owned type, which is reliable across crate
/// boundaries (downcasting a foreign concrete type would require the host's
/// type to be nameable here, which defeats D4).
pub struct HostStateAccessor(pub std::sync::Arc<dyn UserDefaultCwdStore>);

impl crate::ctx::CoreState for HostStateAccessor {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::CapabilitySet;
    use crate::ctx::test_support::ctx_with_state;
    use std::sync::Arc;
    use std::sync::Mutex;

    #[test]
    fn normalize_trims_and_clears_empty() {
        assert_eq!(normalize_default_cwd(None), None);
        assert_eq!(normalize_default_cwd(Some("   ".into())), None);
        assert_eq!(normalize_default_cwd(Some("".into())), None);
        assert_eq!(
            normalize_default_cwd(Some("  /home/x  ".into())),
            Some(PathBuf::from("/home/x"))
        );
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
    fn handler_writes_normalised_path_through_store() {
        let store = Arc::new(FakeStore::default());
        let accessor: Arc<dyn crate::ctx::CoreState> =
            Arc::new(HostStateAccessor(store.clone()));
        let (ctx, _sink) = ctx_with_state(accessor, CapabilitySet::allow_all());

        set_user_default_cwd(&ctx, Some("  /tmp/work  ".into())).unwrap();
        assert_eq!(
            *store.last.lock().unwrap(),
            Some(Some(PathBuf::from("/tmp/work")))
        );

        set_user_default_cwd(&ctx, Some("   ".into())).unwrap();
        assert_eq!(*store.last.lock().unwrap(), Some(None));
    }
}
