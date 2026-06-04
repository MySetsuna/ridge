//! Headless host glue for `ridge-core` (S5).
//!
//! The headless `ridge-cli` daemon reaches the SAME `fs::search` / `fs::tree`
//! implementation the desktop uses by funnelling its control messages through
//! [`ridge_core::dispatch`]. That entry needs a [`ridge_core::Ctx`] carrying the
//! four abstraction faces (state / events / spawner / capability). The read-only
//! filesystem commands this host serves (`search`, `get_directory_children`)
//! touch none of state/events/spawner, so this builds a minimal Ctx:
//!
//!   - **State** — an empty [`HeadlessState`] (no host state needed yet).
//!   - **Events** — a no-op sink (these commands emit nothing).
//!   - **Spawner** — [`ridge_core::TokioSpawner`] (the daemon has a tokio rt).
//!   - **Capability** — [`ridge_core::CapabilitySet::remote_default`], the SAME
//!     D8 allow-list the desktop LAN host enforces, so the headless host can
//!     never reach a host-privileged command either.
//!
//! As later slices migrate stateful commands into `ridge-core`, [`HeadlessState`]
//! grows to implement their state traits (the seam is here, mirroring the
//! desktop `core_bridge.rs`).

use std::path::PathBuf;
use std::sync::Arc;

use ridge_core::{CapabilitySet, ConnectionId, Ctx, EventScope, EventSink, TokioSpawner};
use serde_json::Value;

/// Headless host state. Empty for the S5 read-only fs slice; later slices add
/// the fields/trait impls their migrated commands need.
pub struct HeadlessState;

impl ridge_core::CoreState for HeadlessState {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Event sink that drops events. The read-only fs commands emit nothing; when a
/// later slice migrates an event-emitting command, this becomes the seam that
/// pushes a control-JSON frame onto the data channel.
struct NoopSink;

impl EventSink for NoopSink {
    fn emit(&self, _scope: EventScope, _connection: &ConnectionId, _name: &str, _payload: Value) {}
}

/// Build a per-request `ridge_core::Ctx` for the headless host, carrying the
/// canonical remote allow-list (D8) **and** the filesystem serving-root sandbox
/// (D8/§5.6, D-GM-9).
///
/// `roots` bounds every path-bearing fs command (`search`, `get_directory_children`,
/// `get_file_tree`, …) to those subtrees — enforced once inside
/// [`ridge_core::dispatch`] via `sandbox_guard`. An **empty** slice means
/// unrestricted (the backward-compatible no-op); on a public VPS the daemon is
/// expected to pass at least one root so a controller can never read
/// `~/.ssh` / `/etc/passwd` off the host. Build the slice with
/// [`resolve_serving_roots`].
pub fn headless_ctx(roots: &[PathBuf]) -> Ctx {
    let state: Arc<dyn ridge_core::CoreState> = Arc::new(HeadlessState);
    let events: Arc<dyn EventSink> = Arc::new(NoopSink);
    Ctx::new(
        state,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::remote_default().with_roots(roots.iter()),
    )
}

/// Resolve the filesystem serving root(s) for the headless `remote` daemon.
///
/// Precedence (first non-empty wins), secure-by-default:
///   1. `explicit` — operator's `--root` / `RIDGE_REMOTE_ROOT` (authoritative override).
///   2. `cwd` — the session shell's working dir (`--cwd`): the project the
///      operator is already serving, a natural fs boundary.
///   3. the process current dir — so a bare `ridge remote --daemon` still confines
///      fs to where it launched instead of exposing the whole host filesystem.
///   4. empty `Vec` — only if even the current dir is unreadable; the caller is
///      expected to log a warning, since empty = unrestricted (whole FS exposed).
///
/// Blank/whitespace-only strings are treated as unset so an exported but empty
/// `RIDGE_REMOTE_ROOT=` does not silently select an invalid root.
pub fn resolve_serving_roots(explicit: Option<&str>, cwd: Option<&str>) -> Vec<PathBuf> {
    let pick = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from);

    if let Some(root) = pick(explicit) {
        return vec![root];
    }
    if let Some(root) = pick(cwd) {
        return vec![root];
    }
    if let Ok(here) = std::env::current_dir() {
        return vec![here];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_root_wins_over_cwd() {
        let roots = resolve_serving_roots(Some("/srv/explicit"), Some("/home/cwd"));
        assert_eq!(roots, vec![PathBuf::from("/srv/explicit")]);
    }

    #[test]
    fn falls_back_to_cwd_when_no_explicit_root() {
        let roots = resolve_serving_roots(None, Some("/home/cwd"));
        assert_eq!(roots, vec![PathBuf::from("/home/cwd")]);
    }

    #[test]
    fn blank_values_are_treated_as_unset() {
        // Whitespace-only explicit + cwd → both skipped, falls through to current_dir.
        let roots = resolve_serving_roots(Some("   "), Some(""));
        assert_eq!(roots, vec![std::env::current_dir().unwrap()]);
    }

    #[test]
    fn defaults_to_current_dir_when_nothing_set() {
        // Secure-by-default: a bare daemon still confines fs to where it launched.
        let roots = resolve_serving_roots(None, None);
        assert_eq!(roots, vec![std::env::current_dir().unwrap()]);
    }
}
