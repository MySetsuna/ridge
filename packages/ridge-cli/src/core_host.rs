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
/// canonical remote allow-list (D8).
pub fn headless_ctx() -> Ctx {
    let state: Arc<dyn ridge_core::CoreState> = Arc::new(HeadlessState);
    let events: Arc<dyn EventSink> = Arc::new(NoopSink);
    Ctx::new(
        state,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::remote_default(),
    )
}
