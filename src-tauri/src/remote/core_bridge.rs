//! Desktop host glue for `ridge-core` (S1).
//!
//! This is the **thin host adapter** between the Tauri desktop process and the
//! runtime-agnostic [`ridge_core`] crate. It supplies the three host-provided
//! `Ctx` faces that `ridge-core` abstracts (§5.1):
//!
//!   - **State** — [`AppState`] wrapped as a `ridge_core::CoreState` via the
//!     handlers' own state traits (e.g. [`ridge_core::commands::settings::
//!     UserDefaultCwdStore`], implemented for `AppState` below).
//!   - **Events** — [`DesktopEventSink`], mapping `ridge_core` broadcast /
//!     per-connection emits onto the existing desktop event surfaces
//!     (`AppHandle::emit` + the `remote_ui_event_tx` broadcast bus). The
//!     vertical-slice handlers (settings/theme) emit nothing, so this is the
//!     pattern seed for later slices.
//!   - **Spawner** — [`ridge_core::TokioSpawner`] (tokio directly, never
//!     `tauri::async_runtime`).
//!
//! The capability set is [`ridge_core::CapabilitySet::remote_default`] for the
//! browser-facing remote path and [`allow_all`](ridge_core::CapabilitySet::allow_all)
//! for the in-process desktop IPC path (where Tauri command registration is
//! already the admission boundary).

use std::path::PathBuf;
use std::sync::Arc;

use ridge_core::commands::settings::{HostStateAccessor, UserDefaultCwdStore};
use ridge_core::{CapabilitySet, ConnectionId, Ctx, EventScope, EventSink, TokioSpawner};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::state::AppState;
use crate::types::RemoteUiEvent;

/// `AppState` exposes the one field `set_user_default_cwd` needs.
impl UserDefaultCwdStore for AppState {
    fn set_user_default_cwd(&self, path: Option<PathBuf>) {
        *self.user_default_cwd.write() = path;
    }
}

/// Event sink that mirrors `ridge_core` emits onto the desktop's event
/// surfaces. `Broadcast` events go to both the native WebView (`AppHandle::
/// emit`) and the desktop-browser remote clients (`remote_ui_event_tx`).
/// `Connection`-scoped events are addressed to a single browser connection;
/// for the in-process desktop path there is one implicit connection, so they
/// also go through `AppHandle::emit`. (No vertical-slice handler emits yet —
/// this is the seam later slices will use.)
pub struct DesktopEventSink {
    app: AppHandle,
    ui_event_tx: tokio::sync::broadcast::Sender<RemoteUiEvent>,
}

impl DesktopEventSink {
    pub fn new(app: AppHandle, state: &AppState) -> Self {
        Self {
            app,
            ui_event_tx: state.remote_ui_event_tx.clone(),
        }
    }
}

impl EventSink for DesktopEventSink {
    fn emit(&self, scope: EventScope, _connection: &ConnectionId, name: &str, payload: Value) {
        // Native WebView listeners always get the event.
        let _ = self.app.emit(name, payload.clone());
        // Broadcast events additionally fan out to desktop-browser clients.
        // (Per-connection routing for the browser path is refined in S3/S4
        // once the transport carries a connection id end-to-end.)
        if scope == EventScope::Broadcast {
            let _ = self.ui_event_tx.send(RemoteUiEvent {
                name: name.to_string(),
                payload,
            });
        }
    }
}

/// Build a `ridge_core::Ctx` for the **in-process desktop IPC** path. State is
/// the user-default-cwd accessor over `AppState`; capabilities are `allow_all`
/// because Tauri command registration already gates admission here.
pub fn desktop_ctx(app: &AppHandle, state: &AppState) -> Ctx {
    let accessor: Arc<dyn ridge_core::CoreState> =
        Arc::new(HostStateAccessor(Arc::new(state.clone())));
    let events: Arc<dyn EventSink> = Arc::new(DesktopEventSink::new(app.clone(), state));
    Ctx::new(
        accessor,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::allow_all(),
    )
}

/// Build a `ridge_core::Ctx` for the **browser-facing remote** path, carrying
/// the canonical remote allow-list (D8) and the originating `connection_id`.
/// Used by the remote server's invoke dispatcher as handlers migrate in.
pub fn remote_ctx(app: &AppHandle, state: &AppState, connection_id: impl Into<String>) -> Ctx {
    let accessor: Arc<dyn ridge_core::CoreState> =
        Arc::new(HostStateAccessor(Arc::new(state.clone())));
    let events: Arc<dyn EventSink> = Arc::new(DesktopEventSink::new(app.clone(), state));
    Ctx::new(
        accessor,
        events,
        Arc::new(TokioSpawner),
        CapabilitySet::remote_default(),
    )
    .with_connection(connection_id)
}
