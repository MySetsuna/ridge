//! Private shared Core command boundary and bounded business event stream.
use crate::server::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use futures::{SinkExt, StreamExt};
use ridge_core::{ConnectionId, EventScope, EventSink};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct BusinessEvents {
    inner: Arc<Mutex<(u64, broadcast::Sender<Value>)>>,
}
impl Default for BusinessEvents {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new((0, broadcast::channel(256).0))),
        }
    }
}
impl BusinessEvents {
    fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.inner
            .lock()
            .expect("business events lock")
            .1
            .subscribe()
    }
    pub fn workspace_list_changed(&self) {
        self.broadcast("workspace-list-changed", Value::Null);
    }
    pub fn pane_tree_changed(&self, workspace: uuid::Uuid) {
        self.broadcast("pane-tree-changed", json!({"workspaceId":workspace}));
    }
}
impl EventSink for BusinessEvents {
    fn emit(&self, scope: EventScope, connection: &ConnectionId, name: &str, payload: Value) {
        if scope == EventScope::Connection && connection.is_none() {
            return;
        }
        // Sequence assignment and enqueue share a lock, preserving concurrent emission order.
        let mut events = self.inner.lock().expect("business events lock");
        events.0 += 1;
        let _ = events.1.send(json!({"type":"event","name":name,"payload":payload,
            "sequence":events.0,"connectionId":if scope == EventScope::Connection {connection.clone()} else {None}}));
    }
}

fn authorized(headers: &HeaderMap, state: &AppState) -> bool {
    headers
        .get("x-ridge-kernel-token")
        .and_then(|value| value.to_str().ok())
        == Some(state.token.as_str())
}

fn targets_connection(event: &Value, connection: &str) -> bool {
    event["connectionId"].is_null() || event["connectionId"].as_str() == Some(connection)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandRequest {
    method: String,
    args: Value,
    native: bool,
    roots: Vec<PathBuf>,
    connection_id: String,
}
pub async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CommandRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers, &state) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if uuid::Uuid::parse_str(&request.connection_id).is_err() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let result = tokio::task::spawn_blocking(move || {
        let ctx = crate::host_core::runtime_ctx_with_events(
            &request.roots,
            request.native,
            Arc::new(state.business_events.clone()),
        )
        .with_connection(request.connection_id);
        let method = if request.native {
            ridge_core::protocol_guard::admit_desktop_method(&request.method)
        } else {
            ridge_core::protocol_guard::admit_remote_method(&request.method)
        };
        let result = method
            .map_err(ridge_core::CoreError::InvalidArgs)
            .and_then(|method| {
                // Operator scope and client scope both constrain execution;
                // a privately issued native grant cannot widen the Kernel root.
                let operator =
                    crate::host_core::runtime_ctx(state.fs_scope.roots(), request.native);
                ridge_core::dispatch::validate_request(&method, &request.args, &operator)?;
                ridge_core::dispatch(&method, request.args, &ctx)
            });
        match result {
            Ok(value) => json!({"ok":true,"value":value}),
            Err(error) => json!({"ok":false,"error":error.to_json_rpc()}),
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventQuery {
    connection_id: String,
}
pub async fn stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    if !authorized(&headers, &state) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if uuid::Uuid::parse_str(&query.connection_id).is_err() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut events = state.business_events.subscribe();
    // Snapshot invalidation on every attach also repairs workspace views after
    // an event gap/reconnect. It is scoped to this subscriber, not a mutation.
    state.business_events.emit(
        EventScope::Connection,
        &Some(query.connection_id.clone()),
        "workspace-list-changed",
        json!({"initial":true}),
    );
    Ok(upgrade.on_upgrade(move |socket| async move {
        let (mut sink, mut incoming) = socket.split();
        loop {
            tokio::select! {
                next = events.recv() => match next {
                    Ok(mut event) => {
                        if !targets_connection(&event, &query.connection_id) { continue; }
                        event["runtimeEpoch"] = json!(state.runtime_epoch);
                        if sink.send(Message::Text(event.to_string())).await.is_err() { break; }
                    }
                    // A lagging subscriber must reconnect and fetch snapshots, never silently skip facts.
                    Err(_) => break,
                },
                next = incoming.next() => match next {
                    Some(Ok(Message::Ping(bytes))) => { if sink.send(Message::Pong(bytes)).await.is_err() { break; } }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {},
                },
            }
        }
        let _ = sink.close().await;
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn broadcast_order_and_connection_privacy() {
        let events = BusinessEvents::default();
        let mut receiver = events.subscribe();
        events.emit(
            EventScope::Connection,
            &None,
            "selection",
            json!("must not broadcast"),
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        events.emit(
            EventScope::Connection,
            &Some("first".into()),
            "selection",
            json!("first"),
        );
        events.workspace_list_changed();
        let private = receiver.try_recv().unwrap();
        let shared = receiver.try_recv().unwrap();
        assert!(targets_connection(&private, "first"));
        assert!(!targets_connection(&private, "second"));
        assert!(targets_connection(&shared, "first") && targets_connection(&shared, "second"));
        assert_eq!(private["sequence"], 1);
        assert_eq!(shared["sequence"], 2);
    }
    #[test]
    fn slow_subscriber_gets_an_explicit_gap_instead_of_unbounded_memory() {
        let events = BusinessEvents::default();
        let mut receiver = events.subscribe();
        for _ in 0..300 {
            events.workspace_list_changed();
        }
        assert!(matches!(
            receiver.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
    }
}
