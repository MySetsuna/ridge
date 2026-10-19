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
    pub(crate) fn generation(&self)->Result<u64,String> {
        self.inner.lock().map(|events|events.0).map_err(|_|"business events unavailable".into())
    }
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.inner
            .lock()
            .expect("business events lock")
            .1
            .subscribe()
    }
    pub(crate) fn workspace_history_changed(&self) {
        self.broadcast("workspace-history-changed", Value::Null);
    }
    pub(crate) fn default_cwd_changed(&self) {
        self.broadcast("user-default-cwd-changed", Value::Null);
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

pub(crate) fn targets_connection(event: &Value, connection: &str) -> bool {
    event["connectionId"].is_null() || event["connectionId"].as_str() == Some(connection)
}
pub(crate) fn project_event(mut event: Value, native: bool) -> Option<Value> {
    if native { return Some(event); }
    match event["name"].as_str() {
        Some("teammate://hitl-approval-required" | "lsp://diagnostics") => return None,
        Some("commune-session-update") => {
            let record = &event["payload"];
            let (kind,text) = crate::commune_history::content(record,false);
            event["payload"] = json!({"workspaceId":record["workspaceId"],"agentId":record["agentId"],"sequence":record["sequence"],"timestamp":record["timestamp"],"phase":record["phase"],"kind":kind,"text":text.chars().take(4096).collect::<String>()});
        }
        Some("commune-task-changed") => {
            let record = &event["payload"];
            event["payload"] = json!({"workspaceId":record["workspaceId"],"groupTaskId":record["groupTaskId"].as_str().or(record["payload"]["groupContext"]["groupTaskId"].as_str()),
                "deliveryId":record["deliveryId"],"taskId":record["taskId"],"state":record["execution"]["state"]});
        }
        _ => {},
    }
    Some(event)
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
    if matches!(request.method.as_str(),"dispatch_remote_invoke"|"bind_remote_terminal") {
        let result=crate::remote_invoke::execute(state,request.args,request.native,request.connection_id,request.method=="bind_remote_terminal").await;
        return Ok(Json(match result {Ok(value)=>json!({"ok":true,"value":value}),Err(error)=>json!({"ok":false,"error":error})}));
    }
    let ctx = crate::host_core::runtime_ctx_with_events(
        &request.roots,
        request.native,
        Arc::new(state.business_events.clone()),
    )
    .with_connection(request.connection_id.clone());
    let admission = (|| {
        let method = if request.native {
            ridge_core::protocol_guard::admit_desktop_method(&request.method)
        } else {
            ridge_core::protocol_guard::admit_remote_method(&request.method)
        }
        .map_err(ridge_core::CoreError::InvalidArgs)?;
        let operator = crate::host_core::runtime_ctx(state.fs_scope.roots(), request.native);
        ridge_core::dispatch::validate_request(&method, &request.args, &operator)?;
        ridge_core::dispatch::validate_request(&method, &request.args, &ctx)
    })();
    let method = match admission {
        Ok(method) => method,
        Err(error) => return Ok(Json(json!({"ok":false,"error":error.to_json_rpc()}))),
    };
    let result = if method=="cloud_http" {
        crate::cloud::http::execute(&request.args,request.native).await
    } else if let Some(result)=crate::git_commands::dispatch(&method,&request.args,&state.fs_scope,&ctx).await {
        result
    } else if let Some(result)=crate::terminal_ops::dispatch(&state,&method,&request.args,&ctx).await {
        result
    } else if let Some(result)=crate::foreign_panes::dispatch(&state,&method,&request.args,request.native).await {
        result.map_err(ridge_core::CoreError::InvalidArgs)
    } else if let Some(result)=state.outbound_hosts.dispatch(&state,&method,&request.args,request.native).await {
        result.map_err(ridge_core::CoreError::InvalidArgs)
    } else if let Some(result)=state.cloud.dispatch(&state,&method,&request.args,request.native).await {
        result.map_err(ridge_core::CoreError::InvalidArgs)
    } else if let Some(result)=state.lan.dispatch(&state,&method,&request.args,request.native).await {
        result.map_err(ridge_core::CoreError::InvalidArgs)
    } else if crate::lsp::owns_method(&method) {
        crate::lsp::dispatch(&state, &method, &request.args, request.native, &ctx).await.map_err(ridge_core::CoreError::InvalidArgs)
    } else if crate::commune::owns_method(&method) {
        crate::commune::dispatch(&state, &method, &request.args, request.native)
            .await
            .map_err(ridge_core::CoreError::InvalidArgs)
    } else {
        tokio::task::spawn_blocking(move || {
            if let Some(result)=crate::clipboard_assets::dispatch(&state,&method,&request.args,&ctx){return result;}
            if let Some(result)=crate::headless::dispatch(&state,&method,&request.args,&ctx){return result;}
            if let Some(result) = crate::remote_auth::dispatch(&state, &method, &request.args, request.native) { return result; }
            if let Some(result) = crate::identity::dispatch(&state, &method, &request.args, request.native) { return result; }
            if let Some(result) = crate::remote_hosts::dispatch(&state, &method, &request.args, request.native) { return result; }
            if method == "get_workspace_file_in_directory" {
                let path=request.args["path"].as_str().ok_or_else(||ridge_core::CoreError::InvalidArgs("missing path".into()))?;
                return ridge_core::workspace::file::first_in_directory(std::path::Path::new(path),&[&state.fs_scope,ctx.capabilities().root_scope()]);
            }
            if method == "scan_workspace_context_files" {
                let root=request.args["workspaceRoot"].as_str().ok_or_else(||ridge_core::CoreError::InvalidArgs("missing workspaceRoot".into()))?;
                return ridge_core::teammate::context_files::scan_command(std::path::Path::new(root),&[&state.fs_scope,ctx.capabilities().root_scope()]);
            }
            if method == "get_user_default_cwd" {return crate::settings::get(&state,&ctx);}
            if method == "set_user_default_cwd" {return crate::settings::set(&state,&request.args,&ctx);}
            if let Some(result)=crate::workspace_history::dispatch(&state,&method,&request.args,&ctx) {return result;}
            if let Some(result)=crate::workspace_files::dispatch(&state,&method,&request.args,&ctx) {return result;}
            if let Some(result) = crate::processes::dispatch(&state,&method,&request.args,&ctx) {return result;}
            if let Some(result) = state.projects.dispatch(&method,&request.args,&ctx,&state.fs_scope) {return result;}
            if method == "start_watching_paths" {return state.fs_watches.sync(&request.args,&ctx,&state.fs_scope).map_err(ridge_core::CoreError::InvalidArgs);}
            if method == "start_watching_repos" {return state.fs_watches.sync_git(&request.args,&ctx,&state.fs_scope).map_err(ridge_core::CoreError::InvalidArgs);}
            crate::workspace_commands::dispatch(&state, &method, &request.args, &ctx)
                .unwrap_or_else(|| ridge_core::dispatch(&method, request.args, &ctx))
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    Ok(Json(match result {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error.to_json_rpc()}),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventQuery {
    connection_id: String,
    #[serde(default)]
    native: bool,
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
        let _watches=state.fs_watches.attach(query.connection_id.clone(),state.business_events.clone());
        let (mut sink, mut incoming) = socket.split();
        loop {
            tokio::select! {
                next = events.recv() => match next {
                    Ok(event) => {
                        if !targets_connection(&event, &query.connection_id) { continue; }
                        let Some(mut event) = project_event(event, query.native) else {continue;};
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
    fn remote_notifications_cannot_bypass_history_or_permission_redaction() {
        let event=json!({"name":"commune-session-update","payload":{"kind":"session_update","workspaceId":"workspace-a","params":{"update":{"sessionUpdate":"tool_call","rawInput":{"secret":"private-input"}}}}});
        assert!(project_event(event.clone(),true).unwrap().to_string().contains("private-input"));
        let projected=project_event(event,false).unwrap();
        assert!(!projected.to_string().contains("private-input"));
        assert_eq!(projected["payload"]["workspaceId"],"workspace-a");
        let unknown=json!({"name":"commune-session-update","payload":{"kind":"session_update","params":{"unknownPrivateField":"private-input"}}});
        assert!(!project_event(unknown,false).unwrap().to_string().contains("private-input"));
        let task=json!({"name":"commune-task-changed","payload":{"workspaceId":"workspace","execution":{"state":"unknown","reason":"private-input"}}});
        assert!(!project_event(task,false).unwrap().to_string().contains("private-input"));
        assert!(project_event(json!({"name":"teammate://hitl-approval-required"}),false).is_none());
        assert!(project_event(json!({"name":"lsp://diagnostics","payload":{"privateFile":"private"}}),false).is_none());
    }
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
