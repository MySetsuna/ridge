//! Authenticated native A2A boundary backed by the same Hub as MCP.
//!
//! This is deliberately a transport adapter, not a second message store:
//! inbound A2A messages become Hub entries through `call_tool_rpc`, and task
//! reads/cancellation use the same fenced receipt path. Agent Card discovery
//! is public and contains no secret; JSON-RPC operations require the host
//! token.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::a2a::{AgentCapabilities, AgentCard, AgentInterface};
use crate::server::{call_tool_rpc, McpHost, McpSessionState};

const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_METHOD_BYTES: usize = 128;
const MAX_ID_BYTES: usize = 256;
const MAX_TENANT_BYTES: usize = 256;
const MAX_MESSAGE_ID_BYTES: usize = 256;
const MAX_TEXT_BYTES: usize = 1024 * 1024;

pub const AGENT_CARD_PATH: &str = "/.well-known/agent-card.json";
pub const A2A_JSONRPC_PATH: &str = "/api/v1/a2a";

#[derive(Clone)]
pub struct NativeA2aCtx {
    pub host: Arc<dyn McpHost>,
    pub token: Arc<String>,
    pub version: Arc<String>,
    pub state: Arc<McpSessionState>,
    pub tenant: Option<Arc<String>>,
}

impl NativeA2aCtx {
    pub fn new(
        host: Arc<dyn McpHost>,
        token: Arc<String>,
        version: impl Into<String>,
        state: Arc<McpSessionState>,
    ) -> Self {
        Self {
            host,
            token,
            version: Arc::new(version.into()),
            state,
            tenant: None,
        }
    }

    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(Arc::new(tenant.into()));
        self
    }
}

#[derive(Debug, Deserialize)]
struct RpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

pub fn router(ctx: NativeA2aCtx) -> Router {
    Router::new()
        .route(AGENT_CARD_PATH, get(agent_card))
        .route("/api/v1/a2a/agent-card", get(agent_card))
        .route(A2A_JSONRPC_PATH, post(json_rpc))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(ctx)
}

async fn agent_card(State(ctx): State<NativeA2aCtx>) -> Response {
    let card = AgentCard {
        name: "Ridge".into(),
        description: "Ridge Agent Hub native A2A endpoint".into(),
        supported_interfaces: vec![AgentInterface {
            url: A2A_JSONRPC_PATH.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0".into(),
            tenant: ctx.tenant.as_deref().map(ToString::to_string),
        }],
        provider: None,
        version: ctx.version.as_ref().clone(),
        documentation_url: None,
        capabilities: AgentCapabilities {
            streaming: false,
            push_notifications: false,
            extended_agent_card: false,
            extensions: Vec::new(),
        },
        security_schemes: json!({
            "bearerAuth": { "type": "http", "scheme": "bearer" }
        }),
        security_requirements: vec![json!({ "bearerAuth": [] })],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![json!({
            "id": "ridge-hub-message",
            "name": "Ridge Hub message delivery",
            "description": "Deliver a bounded text message through the fenced Hub"
        })],
        signatures: Vec::new(),
        icon_url: None,
    };
    json_response(
        StatusCode::OK,
        serde_json::to_value(card).expect("Agent Card is serializable"),
    )
}

async fn json_rpc(State(ctx): State<NativeA2aCtx>, headers: HeaderMap, body: Bytes) -> Response {
    if !auth_ok(&headers, &ctx.token) {
        return json_response(
            StatusCode::UNAUTHORIZED,
            json!({
                "error": "unauthorized"
            }),
        );
    }
    if body.len() > MAX_REQUEST_BYTES {
        return rpc_error(
            Value::Null,
            -32600,
            "request body exceeds the bounded limit",
        );
    }
    let request = match serde_json::from_slice::<RpcRequest>(&body) {
        Ok(request) => request,
        Err(_) => return rpc_error(Value::Null, -32700, "invalid JSON-RPC request"),
    };
    if request.jsonrpc != "2.0"
        || request.id.is_null()
        || request.method.is_empty()
        || request.method.len() > MAX_METHOD_BYTES
        || request.id.to_string().len() > MAX_ID_BYTES
    {
        return rpc_error(request.id, -32600, "invalid JSON-RPC request");
    }
    if let Err(error) = validate_tenant(&ctx, &request.params) {
        return rpc_error(request.id, -32602, error);
    }

    let result = dispatch(&ctx, &request.method, &request.params);
    match result {
        Ok(value) => bounded_rpc_response(request.id, value),
        Err(error) => rpc_error(request.id, error.0, error.1),
    }
}

fn dispatch(ctx: &NativeA2aCtx, method: &str, params: &Value) -> Result<Value, (i64, String)> {
    match method {
        "SendMessage" => send_message(ctx, params),
        "GetTask" => get_task(ctx, params),
        "ListTasks" => list_tasks(ctx, params),
        "CancelTask" => cancel_task(ctx, params),
        "SendStreamingMessage"
        | "SubscribeToTask"
        | "CreateTaskPushNotificationConfig"
        | "GetTaskPushNotificationConfig"
        | "ListTaskPushNotificationConfigs"
        | "DeleteTaskPushNotificationConfig"
        | "GetExtendedAgentCard" => {
            Err((-32601, format!("A2A method is not advertised: {method}")))
        }
        _ => Err((-32601, format!("A2A method not found: {method}"))),
    }
}

fn send_message(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    let message = params
        .get("message")
        .ok_or_else(|| invalid_params("SendMessage requires message"))?;
    let message_id = bounded_string(message, "messageId", MAX_MESSAGE_ID_BYTES)?;
    let text = message_text(message)?;
    let target = target_from_params(ctx, params)?;
    let args = target_args(
        &target,
        [
            ("message", Value::String(text)),
            ("from", Value::String("a2a-native".into())),
            (
                "idempotency_key",
                Value::String(format!("a2a:{message_id}")),
            ),
        ],
    )?;
    let entry = call_tool_json("ridge_send_message", args, ctx)?;
    let task_id = entry
        .get("deliveryId")
        .and_then(Value::as_str)
        .ok_or_else(|| internal_error("Hub receipt has no deliveryId"))?;
    Ok(json!({
        "task": task_from_entry(task_id, &entry, message)
    }))
}

fn get_task(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    let task_id = bounded_string(params, "id", MAX_ID_BYTES)?;
    let target = target_from_params(ctx, params)?;
    let args = target_args(&target, [("receipt_id", Value::String(task_id.clone()))])?;
    let entry = call_tool_json("ridge_delivery_status", args, ctx)?;
    Ok(task_from_entry(&task_id, &entry, &Value::Null))
}

fn list_tasks(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    let target = target_from_params(ctx, params)?;
    let limit = params
        .get("pagination")
        .and_then(|value| value.get("pageSize"))
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 200);
    let args = target_args(
        &target,
        [
            ("peek", Value::Bool(true)),
            ("consume", Value::Bool(false)),
            ("limit", Value::Number(limit.into())),
        ],
    )?;
    let entries = call_tool_json("ridge_fetch_inbox", args, ctx)?
        .as_array()
        .cloned()
        .ok_or_else(|| internal_error("Hub inbox response is not an array"))?;
    let tasks = entries
        .iter()
        .filter_map(|entry| {
            entry
                .get("deliveryId")
                .and_then(Value::as_str)
                .map(|id| task_from_entry(id, entry, &Value::Null))
        })
        .collect::<Vec<_>>();
    Ok(json!({ "tasks": tasks, "nextPageToken": null }))
}

fn cancel_task(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    let task_id = bounded_string(params, "id", MAX_ID_BYTES)?;
    let target = target_from_params(ctx, params)?;
    let args = target_args(&target, [("delivery_id", Value::String(task_id.clone()))])?;
    let entry = call_tool_json("ridge_cancel_delivery", args, ctx)?;
    Ok(task_from_entry(&task_id, &entry, &Value::Null))
}

fn task_from_entry(id: &str, entry: &Value, message: &Value) -> Value {
    let state = match entry.get("status").and_then(Value::as_str) {
        Some("cancelled") => "canceled",
        Some("completed") | Some("adapter_accepted") => "completed",
        Some("failed") | Some("delivery_failed") | Some("delivery_rejected") => "failed",
        _ => "submitted",
    };
    let mut task = json!({
        "id": id,
        "status": { "state": state },
        "metadata": { "ridge": {
            "deliveryId": entry.get("deliveryId"),
            "messageId": entry.get("messageId"),
            "targetKey": entry.get("targetKey")
        }}
    });
    if !message.is_null() {
        task["history"] = json!([message]);
    }
    task
}

fn target_from_params(_ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    let explicit = params
        .get("metadata")
        .and_then(|metadata| metadata.get("ridge"))
        .and_then(|ridge| ridge.get("target"))
        .or_else(|| params.get("target"));
    match explicit {
        Some(value) if value.is_object() => Ok(value.clone()),
        Some(_) => Err(invalid_params("A2A target must be an object")),
        None => Err(invalid_params(
            "A2A target is required in params.metadata.ridge.target",
        )),
    }
}

fn target_args<const N: usize>(
    target: &Value,
    extra: [(&str, Value); N],
) -> Result<Value, (i64, String)> {
    let pane_id = target
        .get("paneId")
        .or_else(|| target.get("pane_id"))
        .cloned()
        .ok_or_else(|| invalid_params("A2A target has no paneId"))?;
    let mut args = json!({
        "workspace_id": target.get("workspaceId").or_else(|| target.get("workspace_id")),
        "target_pane_id": pane_id,
        "agent_id": target.get("agentId").or_else(|| target.get("agent_id")),
        "generation": target.get("generation"),
        "lease": target.get("lease")
    });
    let object = args
        .as_object_mut()
        .ok_or_else(|| internal_error("A2A target args are not an object"))?;
    for (key, value) in extra {
        object.insert(key.into(), value);
    }
    Ok(args)
}

fn call_tool_json(name: &str, args: Value, ctx: &NativeA2aCtx) -> Result<Value, (i64, String)> {
    let response = call_tool_rpc(name, args, ctx.host.as_ref(), ctx.state.as_ref());
    if let Some(error) = response.get("error") {
        return Err((
            error.get("code").and_then(Value::as_i64).unwrap_or(-32603),
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Hub request failed")
                .to_string(),
        ));
    }
    let text = response
        .get("result")
        .and_then(|result| result.get("content"))
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|item| item.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| internal_error("Hub tool returned no text result"))?;
    serde_json::from_str(text).map_err(|_| internal_error("Hub tool returned invalid JSON"))
}

fn message_text(message: &Value) -> Result<String, (i64, String)> {
    let parts = message
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_params("A2A message requires parts"))?;
    if parts.is_empty() {
        return Err(invalid_params("A2A message requires at least one part"));
    }
    let mut text = String::new();
    for part in parts {
        let value = part
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_params("native A2A currently accepts text parts only"))?;
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(value);
        if text.len() > MAX_TEXT_BYTES {
            return Err(invalid_params("A2A text exceeds the bounded limit"));
        }
    }
    Ok(text)
}

fn bounded_string(value: &Value, key: &str, max: usize) -> Result<String, (i64, String)> {
    let value = value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid_params(format!("A2A requires {key}")))?;
    if value.len() > max {
        return Err(invalid_params(format!(
            "A2A {key} exceeds the bounded limit"
        )));
    }
    Ok(value.to_string())
}

fn validate_tenant(ctx: &NativeA2aCtx, params: &Value) -> Result<(), String> {
    let supplied = params.get("tenant").and_then(Value::as_str);
    if supplied.is_some_and(|tenant| tenant.len() > MAX_TENANT_BYTES) {
        return Err("A2A tenant exceeds the bounded limit".into());
    }
    match (ctx.tenant.as_deref(), supplied) {
        (Some(expected), Some(actual)) if expected.as_str() == actual => Ok(()),
        (Some(_), Some(_)) => Err("A2A tenant does not match the native endpoint".into()),
        (Some(_), None) => Err("A2A tenant is required by the native endpoint".into()),
        (None, None) => Ok(()),
        (None, Some(_)) => Err("A2A tenant is not configured on the native endpoint".into()),
    }
}

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("x-ridge-token")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == token)
        || headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| value == token)
}

fn bounded_rpc_response(id: Value, result: Value) -> Response {
    let value = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    if serde_json::to_vec(&value)
        .map(|body| body.len() > MAX_RESPONSE_BYTES)
        .unwrap_or(true)
    {
        return rpc_error(
            value["id"].clone(),
            -32603,
            "A2A response exceeds the bounded limit",
        );
    }
    json_response(StatusCode::OK, value)
}

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Response {
    json_response(
        StatusCode::OK,
        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } }),
    )
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (
        status,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

fn invalid_params(message: impl Into<String>) -> (i64, String) {
    (-32602, message.into())
}

fn internal_error(message: impl Into<String>) -> (i64, String) {
    (-32603, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::RidgeUri;
    use crate::server::{HostError, HostResult, InputDispatch};
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    struct TestHost;

    impl McpHost for TestHost {
        fn team_profile(&self) -> Value {
            json!({
                "roster": [{
                    "agentId": "agent-a",
                    "sessionId": "session-a",
                    "workspaceId": "workspace-a",
                    "paneId": "pane-a",
                    "generation": 2,
                    "lease": "lease-2",
                    "online": true,
                    "lifecycle": "Online",
                    "capabilities": ["messages", "tasks"]
                }]
            })
        }

        fn team_profile_for(&self, _workspace_id: Option<&str>) -> HostResult<Value> {
            Ok(self.team_profile())
        }

        fn send_text(
            &self,
            _target: &Value,
            _text: &str,
            _submit: bool,
            _mark_busy: bool,
        ) -> HostResult<InputDispatch> {
            Ok(InputDispatch {
                terminal_accepted: true,
            })
        }

        fn capture_pane(&self, _target: &Value, _lines: usize) -> HostResult<String> {
            Ok(String::new())
        }

        fn split_pane(
            &self,
            _direction: &str,
            _role: &str,
            _initial_cmd: Option<&str>,
        ) -> HostResult<Value> {
            Ok(Value::Null)
        }

        fn read_resource(&self, _uri: &RidgeUri) -> HostResult<(String, String)> {
            Ok(("text/plain".into(), String::new()))
        }

        fn pane_key(&self, target: &Value) -> HostResult<String> {
            target
                .get("paneId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| HostError::InvalidParams("paneId missing".into()))
        }

        fn resolve_pane_target(
            &self,
            workspace_id: Option<&str>,
            target: &Value,
        ) -> HostResult<Value> {
            Ok(json!({
                "paneId": target.as_str().unwrap_or("pane-a"),
                "workspaceId": workspace_id.unwrap_or("workspace-a")
            }))
        }
    }

    fn app() -> Router {
        router(NativeA2aCtx::new(
            Arc::new(TestHost),
            Arc::new("token".into()),
            "test",
            Arc::new(McpSessionState::default()),
        ))
    }

    async fn body(response: Response) -> Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn rpc(app: &Router, request: Value) -> (StatusCode, Value) {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(A2A_JSONRPC_PATH)
                    .header("x-ridge-token", "token")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        (status, body(response).await)
    }

    #[tokio::test]
    async fn card_is_discoverable_and_never_contains_token() {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(AGENT_CARD_PATH)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let card = body(response).await;
        assert_eq!(card["supportedInterfaces"][0]["protocolVersion"], "1.0");
        assert!(!card.to_string().contains("token"));
    }

    #[tokio::test]
    async fn send_get_cancel_share_authenticated_fenced_hub_receipt() {
        let target = json!({
            "agentId": "agent-a", "sessionId": "session-a", "workspaceId": "workspace-a",
            "paneId": "pane-a", "generation": 2, "lease": "lease-2"
        });
        let send = json!({
            "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
            "params": { "message": { "messageId": "m-1", "parts": [{"text": "hello"}] },
                "metadata": { "ridge": { "target": target } } }
        });
        let app = app();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(A2A_JSONRPC_PATH)
                    .header("x-ridge-token", "token")
                    .header("content-type", "application/json")
                    .body(Body::from(send.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let result = body(response).await;
        assert!(
            result["error"].is_null(),
            "native A2A send failed: {result}"
        );
        let task_id = result["result"]["task"]["id"].as_str().unwrap().to_string();

        let get = json!({
            "jsonrpc": "2.0", "id": 2, "method": "GetTask",
            "params": { "id": task_id, "metadata": { "ridge": { "target": target } } }
        });
        let (_, result) = rpc(&app, get).await;
        assert_eq!(result["result"]["id"], task_id);

        let list = json!({
            "jsonrpc": "2.0", "id": 3, "method": "ListTasks",
            "params": { "pagination": { "pageSize": 10 },
                "metadata": { "ridge": { "target": target } } }
        });
        let (_, result) = rpc(&app, list).await;
        assert!(result["result"]["tasks"]
            .as_array()
            .is_some_and(|tasks| { tasks.iter().any(|task| task["id"] == task_id) }));

        let cancel = json!({
            "jsonrpc": "2.0", "id": 4, "method": "CancelTask",
            "params": { "id": task_id, "metadata": { "ridge": { "target": target } } }
        });
        let (_, result) = rpc(&app, cancel).await;
        assert_eq!(result["result"]["status"]["state"], "canceled");
    }

    #[tokio::test]
    async fn rejects_missing_target_stale_fence_and_tenant_mismatch() {
        let missing_target = json!({
            "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
            "params": { "message": { "messageId": "m-1", "parts": [{"text": "hello"}] } }
        });
        let (_, result) = rpc(&app(), missing_target).await;
        assert_eq!(result["error"]["code"], -32602);

        let stale_target = json!({
            "agentId": "agent-a", "sessionId": "session-a", "workspaceId": "workspace-a",
            "paneId": "pane-a", "generation": 1, "lease": "lease-1"
        });
        let stale = json!({
            "jsonrpc": "2.0", "id": 2, "method": "SendMessage",
            "params": { "message": { "messageId": "m-2", "parts": [{"text": "hello"}] },
                "metadata": { "ridge": { "target": stale_target } } }
        });
        let (_, result) = rpc(&app(), stale).await;
        assert!(
            result["error"].is_object(),
            "stale target unexpectedly accepted: {result}"
        );

        let tenant_app = router(
            NativeA2aCtx::new(
                Arc::new(TestHost),
                Arc::new("token".into()),
                "test",
                Arc::new(McpSessionState::default()),
            )
            .with_tenant("tenant-a"),
        );
        let wrong_tenant = json!({
            "jsonrpc": "2.0", "id": 3, "method": "ListTasks",
            "params": { "tenant": "tenant-b" }
        });
        let (_, result) = rpc(&tenant_app, wrong_tenant).await;
        assert_eq!(result["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn rejects_oversized_json_request_before_dispatch() {
        let request = json!({
            "jsonrpc": "2.0", "id": 1, "method": "ListTasks",
            "params": { "padding": "x".repeat(MAX_REQUEST_BYTES + 1) }
        });
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(A2A_JSONRPC_PATH)
                    .header("x-ridge-token", "token")
                    .header("content-type", "application/json")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn rejects_missing_auth_and_unsupported_method() {
        let request = json!({"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{}});
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(A2A_JSONRPC_PATH)
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let request = json!({"jsonrpc":"2.0","id":1,"method":"SubscribeToTask","params":{}});
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(A2A_JSONRPC_PATH)
                    .header("authorization", "Bearer token")
                    .body(Body::from(request.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(body(response).await["error"]["code"], -32601);
    }
}
