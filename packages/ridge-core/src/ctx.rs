//! The runtime-agnostic execution context handed to every migrated handler.
//!
//! `Ctx` is the seam that lets one command implementation run inside the Tauri
//! desktop host *and* the headless `ridge-cli` daemon with **zero Tauri
//! dependency** in this crate (§5.1, D4). It bundles the four abstraction faces
//! the handlers need:
//!
//! 1. **State handle** — an `Arc`-held [`CoreState`] the host owns (the
//!    `AppState` equivalent). Handlers borrow it through [`Ctx::state`].
//! 2. **Event emitter** — [`EventSink`], which distinguishes **broadcast**
//!    (CRUD / PTY output → all connections) from **single-connection** (focus /
//!    selection / scroll → originating connection only) per **D11**.
//! 3. **Background task spawn** — [`TaskSpawner`], wrapping `tokio` *directly*
//!    (never `tauri::async_runtime`, R3) so file-watcher / git-polling tasks
//!    can be launched from inside `ridge-core`.
//! 4. **Error mapping** — handlers return [`CoreError`](crate::error::CoreError),
//!    mapped at each boundary (see `error.rs`).
//!
//! Plus the **capability set** (D8) the dispatch entry consults for admission,
//! and an optional `connection_id` so per-connection events can be addressed.
//!
//! ## Lifetime / ownership
//!
//! The `CoreState` is `Arc`-held by the host (desktop = Tauri `manage`,
//! headless = the daemon). A `Ctx` is cheap to construct and is intended to be
//! built **per request** (it borrows `Arc`s and trait objects the host keeps
//! alive). All members are `Send + Sync` so handlers can run concurrently
//! across invokes (§5.1 "handler 必须 Send + Sync").

use std::sync::Arc;

use serde_json::Value;

use crate::capability::CapabilitySet;

/// Opaque identifier of the connection a request arrived on. Used to address
/// per-connection events (D11). `None` for the in-process desktop IPC path,
/// where there is exactly one implicit "connection".
pub type ConnectionId = Option<String>;

/// The host-owned state handle (the `AppState` equivalent), erased behind a
/// trait so `ridge-core` need not name the desktop `AppState` type.
///
/// The desktop host implements this on a wrapper around its `AppState`; the
/// headless host implements it on its own daemon state. Handlers obtain the
/// concrete type back via [`CoreState::as_any`] + downcast — the same pattern
/// `tauri::State` resolution uses internally, but Tauri-free.
///
/// `Send + Sync` so a `Ctx` (and therefore a handler) can move across threads
/// and run concurrently.
pub trait CoreState: Send + Sync {
    /// Upcast for downcasting back to the concrete host state type.
    fn as_any(&self) -> &dyn std::any::Any;
}

/// How an emitted event is routed (D11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventScope {
    /// Deliver to **every** connection. Used for shared-graph changes:
    /// workspace/pane CRUD, split layout, locked pane size, PTY output
    /// metadata, fs-changed / scm-refresh (D11 "共享(广播)").
    Broadcast,
    /// Deliver only to the **originating** connection. Used for per-connection
    /// view state: active workspace, focused pane, scroll, selection, theme
    /// (D11 "每连接(不广播)").
    Connection,
}

/// The event-emission abstraction (§5.1 "事件发射 trait").
///
/// On the desktop host this is implemented as `AppHandle::emit` (broadcast) /
/// a per-connection relay; on the transport host it is implemented as "encode
/// a control-JSON frame and push it onto the right channel". Splitting
/// `Broadcast` from `Connection` at the trait level is what lets the same
/// handler emit "focus changed → just me" without spamming every client (D11).
///
/// `Send + Sync` because it lives on `Ctx` and is shared across handler tasks.
pub trait EventSink: Send + Sync {
    /// Emit `payload` under the logical event `name` with the given routing
    /// scope. `connection` identifies the originating connection for
    /// [`EventScope::Connection`]; it is ignored for [`EventScope::Broadcast`].
    fn emit(&self, scope: EventScope, connection: &ConnectionId, name: &str, payload: Value);

    /// Convenience: broadcast `payload` to all connections.
    fn broadcast(&self, name: &str, payload: Value) {
        self.emit(EventScope::Broadcast, &None, name, payload);
    }
}

/// Background-task spawn abstraction (§5.1 "后台任务派发").
///
/// `ridge-core` never calls `tauri::async_runtime`. Hosts back this with a
/// plain `tokio` spawn (the default [`TokioSpawner`]) so file-watcher / git
/// polling tasks launched from inside `ridge-core` stay Tauri-free (R3).
pub trait TaskSpawner: Send + Sync {
    /// Spawn `fut` to run in the background, detached.
    fn spawn(&self, fut: futures_boxed::BoxFuture);
}

/// Minimal boxed-future alias so we don't add a `futures` dependency just for
/// `BoxFuture`. A detached background task yields `()`.
pub mod futures_boxed {
    use std::future::Future;
    use std::pin::Pin;

    /// A heap-pinned, `Send` future returning `()`.
    pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

    /// Box and pin any `Send + 'static` future into a [`BoxFuture`].
    pub fn boxed<F>(fut: F) -> BoxFuture
    where
        F: Future<Output = ()> + Send + 'static,
    {
        Box::pin(fut)
    }
}

/// Default [`TaskSpawner`] backed by `tokio::spawn`. Requires a running tokio
/// runtime on the calling thread (both hosts have one). Tauri-free by design.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioSpawner;

impl TaskSpawner for TokioSpawner {
    fn spawn(&self, fut: futures_boxed::BoxFuture) {
        tokio::spawn(fut);
    }
}

/// The per-request execution context (see module docs).
///
/// Borrows the host-owned `Arc`s / trait objects; cheap to construct per
/// request. Generic over nothing — state is erased behind [`CoreState`] so the
/// stringly-typed dispatch boundary (decision **D-S1-1**) stays uniform.
#[derive(Clone)]
pub struct Ctx {
    state: Arc<dyn CoreState>,
    events: Arc<dyn EventSink>,
    spawner: Arc<dyn TaskSpawner>,
    capabilities: CapabilitySet,
    connection: ConnectionId,
}

impl Ctx {
    /// Construct a context. Hosts call this per request (or per connection,
    /// then cheaply re-`with_connection` per request).
    pub fn new(
        state: Arc<dyn CoreState>,
        events: Arc<dyn EventSink>,
        spawner: Arc<dyn TaskSpawner>,
        capabilities: CapabilitySet,
    ) -> Self {
        Self {
            state,
            events,
            spawner,
            capabilities,
            connection: None,
        }
    }

    /// Set the originating connection id (for per-connection event routing).
    pub fn with_connection(mut self, connection: impl Into<String>) -> Self {
        self.connection = Some(connection.into());
        self
    }

    /// Borrow the host state, downcast to the concrete type `T`.
    /// Returns [`CoreError::HostUnavailable`] if the downcast fails (a host
    /// misconfiguration — wrong state type wired in).
    pub fn state<T: 'static>(&self) -> crate::error::CoreResult<&T> {
        self.state.as_any().downcast_ref::<T>().ok_or_else(|| {
            crate::error::CoreError::HostUnavailable(
                "host state type mismatch (ridge-core Ctx misconfigured)".to_string(),
            )
        })
    }

    /// The event sink for emitting broadcast / per-connection events.
    pub fn events(&self) -> &dyn EventSink {
        self.events.as_ref()
    }

    /// The background task spawner.
    pub fn spawner(&self) -> &dyn TaskSpawner {
        self.spawner.as_ref()
    }

    /// The capability set governing admission for this context (D8).
    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    /// The originating connection id, if any.
    pub fn connection(&self) -> &ConnectionId {
        &self.connection
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! In-memory `Ctx` fakes for `ridge-core`'s own unit tests. These let pure
    //! logic tests run with NO Tauri runtime and NO cdylib (safe on this
    //! machine, where `cargo test --lib` of the app crate crashes — see the
    //! orchestration log's environment constraints).

    use super::*;
    use parking_lot_free::Mutex;
    use std::sync::Arc;

    /// A no-op state for handlers that don't touch host state.
    pub struct EmptyState;
    impl CoreState for EmptyState {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Records every emitted event so tests can assert scope + payload.
    #[derive(Default)]
    pub struct RecordingSink {
        pub events: Mutex<Vec<(EventScope, Option<String>, String, Value)>>,
    }
    impl EventSink for RecordingSink {
        fn emit(&self, scope: EventScope, connection: &ConnectionId, name: &str, payload: Value) {
            self.events
                .lock()
                .push((scope, connection.clone(), name.to_string(), payload));
        }
    }

    /// A spawner that runs futures inline-immediately is impossible without a
    /// runtime, so this one simply drops them — tests that need real spawning
    /// use `#[tokio::test]` + `TokioSpawner`.
    pub struct NoopSpawner;
    impl TaskSpawner for NoopSpawner {
        fn spawn(&self, _fut: futures_boxed::BoxFuture) {}
    }

    /// Build a `Ctx` over a concrete state with a recording sink + noop spawner.
    pub fn ctx_with_state(
        state: Arc<dyn CoreState>,
        caps: CapabilitySet,
    ) -> (Ctx, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::default());
        let ctx = Ctx::new(state, sink.clone(), Arc::new(NoopSpawner), caps);
        (ctx, sink)
    }

    /// A tiny `parking_lot::Mutex` stand-in so test_support has no extra dep.
    /// (`ridge-core` deliberately avoids a `parking_lot` dependency.)
    pub mod parking_lot_free {
        use std::sync::Mutex as StdMutex;

        pub struct Mutex<T>(StdMutex<T>);
        impl<T: Default> Default for Mutex<T> {
            fn default() -> Self {
                Mutex(StdMutex::new(T::default()))
            }
        }
        impl<T> Mutex<T> {
            pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
                self.0.lock().unwrap_or_else(|e| e.into_inner())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::capability::CapabilitySet;
    use std::sync::Arc;

    #[test]
    fn state_downcast_round_trips() {
        struct MyState {
            value: u32,
        }
        impl CoreState for MyState {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        let (ctx, _sink) =
            ctx_with_state(Arc::new(MyState { value: 42 }), CapabilitySet::allow_all());
        let got = ctx.state::<MyState>().expect("downcast");
        assert_eq!(got.value, 42);
    }

    #[test]
    fn state_downcast_mismatch_is_host_unavailable() {
        let (ctx, _sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::allow_all());
        struct Other;
        impl CoreState for Other {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        match ctx.state::<Other>() {
            Ok(_) => panic!("expected downcast to fail"),
            Err(err) => assert_eq!(err.kind_tag(), "host_unavailable"),
        }
    }

    #[test]
    fn event_scope_routing_is_recorded() {
        let (ctx, sink) = ctx_with_state(Arc::new(EmptyState), CapabilitySet::allow_all());
        let ctx = ctx.with_connection("conn-1");
        ctx.events().broadcast("pane-added", serde_json::json!({"id": 1}));
        ctx.events().emit(
            EventScope::Connection,
            ctx.connection(),
            "focus-changed",
            serde_json::json!({"pane": 2}),
        );
        let recorded = sink.events.lock();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].0, EventScope::Broadcast);
        assert_eq!(recorded[0].1, None); // broadcast ignores connection
        assert_eq!(recorded[1].0, EventScope::Connection);
        assert_eq!(recorded[1].1, Some("conn-1".to_string()));
    }
}
