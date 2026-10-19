//! Authenticated native A2A boundary backed by the same Hub as MCP.
//!
//! This is deliberately a transport adapter, not a second message store:
//! inbound A2A messages become Hub entries through `call_tool_rpc`, and task
//! reads/cancellation use the same fenced receipt path. Agent Card discovery
//! is public and contains no secret; JSON-RPC operations require the host
//! token; inbox/status/cancellation route through the same authenticated Hub
//! call path.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
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
    base_url: Option<Arc<String>>,
    target: Option<Value>,
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
            base_url: None,
            target: None,
        }
    }

    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(Arc::new(tenant.into()));
        self
    }

    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = Some(Arc::new(base_url));
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
        .route("/api/v1/a2a/members", get(member_catalog))
        .route("/api/v1/a2a/members/:workspace/:agent/:generation/.well-known/agent-card.json", get(member_card))
        .route("/api/v1/a2a/members/:workspace/:agent/:generation", post(member_rpc))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(ctx)
}

fn card_value(ctx: &NativeA2aCtx) -> Value {
    let card = AgentCard {
        name: "Ridge".into(),
        description: "Ridge Agent Hub native A2A endpoint".into(),
        supported_interfaces: vec![AgentInterface {
            url: ctx.base_url.as_ref().map(|base|format!("{}{A2A_JSONRPC_PATH}",base.trim_end_matches('/'))).unwrap_or_else(||A2A_JSONRPC_PATH.into()),
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
            "bearerAuth": { "httpAuthSecurityScheme": { "scheme": "bearer" } }
        }),
        security_requirements: vec![json!({ "schemes": { "bearerAuth": { "list": [] } } })],
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
    serde_json::to_value(card).expect("Agent Card is serializable")
}

async fn agent_card(State(ctx): State<NativeA2aCtx>) -> Response {
    json_response(StatusCode::OK, card_value(&ctx))
}

fn member_endpoint(ctx: &NativeA2aCtx, member: &Value) -> Result<String, String> {
    let mut url=reqwest::Url::parse(ctx.base_url.as_deref().ok_or("A2A member discovery requires a canonical base URL")?).map_err(|_|"invalid A2A base URL")?;
    let workspace=member["workspaceId"].as_str().ok_or("member has no workspace")?;
    let agent=member["agentId"].as_str().ok_or("member has no Agent ID")?;
    let generation=member["generation"].as_u64().filter(|value|*value>0).ok_or("member has no generation")?.to_string();
    url.path_segments_mut().map_err(|_|"invalid A2A base URL")?.clear().extend(["api","v1","a2a","members",workspace,agent,&generation]);
    Ok(url.to_string())
}

fn bound_member(ctx: &NativeA2aCtx, workspace: &str, agent: &str, generation: u64) -> Result<NativeA2aCtx, String> {
    let profile=ctx.host.team_profile_for(Some(workspace)).map_err(|error|error.message())?;
    let member=profile["roster"].as_array().and_then(|rows|rows.iter().find(|member|
        member["workspaceId"]==workspace && member["agentId"]==agent && member["generation"]==generation
        && member["protocolBinding"].is_object() && member["lease"].as_str().is_some_and(|value|!value.is_empty())))
        .ok_or("registered A2A member binding is unavailable or stale")?;
    let mut bound=ctx.clone();bound.target=Some(member.clone());Ok(bound)
}

async fn member_catalog(State(ctx): State<NativeA2aCtx>, headers: HeaderMap) -> Response {
    if !auth_ok(&headers,&ctx.token) {return json_response(StatusCode::UNAUTHORIZED,json!({"error":"unauthorized"}));}
    let result=(||->Result<Value,String>{
        let profile=ctx.host.team_profile();
        let mut cards=Vec::new();
        for member in profile["roster"].as_array().ok_or("Agent roster unavailable")? {
            if member["protocolBinding"].is_object() && member["online"]==true {
                cards.push(format!("{}/.well-known/agent-card.json",member_endpoint(&ctx,member)?));
            }
        }
        Ok(json!({"agentCards":cards}))
    })();
    match result {Ok(value)=>json_response(StatusCode::OK,value),Err(error)=>json_response(StatusCode::SERVICE_UNAVAILABLE,json!({"error":error}))}
}

async fn member_card(State(ctx): State<NativeA2aCtx>, Path((workspace,agent,generation)):Path<(String,String,u64)>, headers:HeaderMap) -> Response {
    if !auth_ok(&headers,&ctx.token) {return json_response(StatusCode::UNAUTHORIZED,json!({"error":"unauthorized"}));}
    let result=(||->Result<Value,String>{
        let bound=bound_member(&ctx,&workspace,&agent,generation)?;
        let member=bound.target.as_ref().ok_or("missing member binding")?;
        let mut card=card_value(&bound);
        card["name"]=json!(agent);card["description"]=json!("Existing Commune conversation; prompt completion does not assert objective success");
        card["supportedInterfaces"][0]["url"]=json!(member_endpoint(&ctx,member)?);
        Ok(card)
    })();
    match result {Ok(value)=>json_response(StatusCode::OK,value),Err(error)=>json_response(StatusCode::NOT_FOUND,json!({"error":error}))}
}

async fn member_rpc(State(ctx):State<NativeA2aCtx>,Path((workspace,agent,generation)):Path<(String,String,u64)>,headers:HeaderMap,body:Bytes)->Response {
    if !auth_ok(&headers,&ctx.token) {return json_response(StatusCode::UNAUTHORIZED,json!({"error":"unauthorized"}));}
    match bound_member(&ctx,&workspace,&agent,generation) {
        Ok(bound)=>json_rpc(State(bound),headers,body).await,
        Err(error)=>rpc_error(serde_json::from_slice::<Value>(&body).ok().and_then(|value|value.get("id").cloned()).unwrap_or(Value::Null),-32001,error),
    }
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
    if headers.get("a2a-version").is_some_and(|value|value.as_bytes()!=b"1.0") {
        return rpc_error(request.id,-32602,"unsupported A2A version; expected 1.0");
    }

    let result = tokio::task::spawn_blocking(move||dispatch(&ctx, &request.method, &request.params)).await
        .unwrap_or_else(|_|Err(internal_error("A2A worker unavailable")));
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
    if ctx.target.is_some() {
        if message["role"]!="ROLE_USER" {return Err(invalid_params("member messages require ROLE_USER"));}
        if ["contextId","taskId"].iter().any(|key|message[*key].as_str().is_some_and(|value|!value.is_empty())) {
            return Err((-32004,"existing conversation does not support independent A2A context or task continuation".into()));
        }
    }
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
        "task": task_result(ctx, task_id, &entry, message)?
    }))
}

fn get_task(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    if params["historyLength"].as_u64().is_some_and(|length|length>0) {
        return Err((-32004,"full A2A conversation history is not supported".into()));
    }
    let task_id = bounded_string(params, "id", MAX_ID_BYTES)?;
    let target = target_from_params(ctx, params)?;
    let args = target_args(&target, [("delivery_id", Value::String(task_id.clone()))])?;
    let entry = call_tool_json("ridge_delivery_status", args, ctx)?;
    task_result(ctx, &task_id, &entry, &Value::Null)
}

fn list_tasks(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    if ctx.target.is_some() {return Err((-32601,"member task listing is not implemented; use GetTask with the returned task ID".into()));}
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
    task_result(ctx, &task_id, &entry, &Value::Null)
}

fn task_result(ctx:&NativeA2aCtx,id:&str,entry:&Value,message:&Value)->Result<Value,(i64,String)> {
    let mut task=task_from_entry(id,entry,message);
    if ctx.target.as_ref().is_some_and(|target|target["protocolBinding"].is_object()) {
        if task["status"]["state"]=="TASK_STATE_WORKING" && ctx.host.requires_permission(ctx.target.as_ref().unwrap()).map_err(|error|internal_error(error.message()))? {
            task["status"]["state"]=json!("TASK_STATE_AUTH_REQUIRED");
        }
        let outputs=ctx.state.protocol_outputs(id,true).map_err(internal_error)?;
        task["artifacts"]=Value::Array(outputs["items"].as_array().ok_or_else(||internal_error("protocol outputs unavailable"))?.iter()
            .map(|output|json!({"artifactId":output["id"],"name":"Reported protocol output","parts":[{"data":output}],"metadata":{"verified":false}})).collect());
    }
    Ok(task)
}

fn task_from_entry(id: &str, entry: &Value, message: &Value) -> Value {
    let state = match entry.get("status").and_then(Value::as_str) {
        Some("cancelled") => "canceled",
        Some("completed") | Some("agent_completed") => "completed",
        Some("failed") | Some("delivery_failed") | Some("delivery_rejected") => "failed",
        _ => "submitted",
    };
    let state = match entry["execution"]["state"].as_str() {
        Some("dispatching" | "running") => "working",
        Some("failed") => "failed",
        Some("cancelled") => "canceled",
        Some("rejected") => "rejected",
        // ACP end_turn proves a conversation turn ended, not that the user's
        // objective succeeded. Keep the raw response for an explicit decision.
        Some("turn_ended" | "unknown") => "input-required",
        _ => state,
    };
    let mut task = json!({
        "id": id,
        "contextId": id,
        "status": { "state": format!("TASK_STATE_{}",state.replace('-',"_").to_ascii_uppercase()) },
        "metadata": { "ridge": {
            "deliveryId": entry.get("deliveryId"),
            "messageId": entry.get("messageId"),
            "targetKey": entry.get("targetKey")
        }}
    });
    if let Some(execution) = entry.get("execution") {
        task["metadata"]["ridge"]["execution"] = execution.clone();
    }
    if !message.is_null() {
        task["history"] = json!([message]);
    }
    task
}

fn target_from_params(ctx: &NativeA2aCtx, params: &Value) -> Result<Value, (i64, String)> {
    if let Some(target)=&ctx.target {
        if params.get("target").is_some() || params.pointer("/metadata/ridge/target").is_some() {
            return Err(invalid_params("member route does not accept a target override"));
        }
        return Ok(target.clone());
    }
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
        .filter(|value| !value.is_null());
    let agent_id = target
        .get("agentId")
        .or_else(|| target.get("agent_id"))
        .cloned()
        .filter(|value| !value.is_null());
    if pane_id.is_none() && agent_id.is_none() {
        return Err(invalid_params("A2A target requires agentId or paneId"));
    }
    let mut args = json!({
        "workspace_id": target.get("workspaceId").or_else(|| target.get("workspace_id")),
        "generation": target.get("generation"),
        "lease": target.get("lease")
    });
    let object = args
        .as_object_mut()
        .ok_or_else(|| internal_error("A2A target args are not an object"))?;
    if let Some(pane_id) = pane_id {
        object.insert("target_pane_id".into(), pane_id);
    }
    if let Some(agent_id) = agent_id {
        object.insert("agent_id".into(), agent_id);
    }
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
                    ,"protocolBinding": {"protocol":"app-server"}
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

    #[test]
    fn delivery_acceptance_and_prompt_end_do_not_claim_task_success() {
        let accepted = json!({"status":"adapter_accepted"});
        assert_eq!(
            task_from_entry("id", &accepted, &Value::Null)["status"]["state"],
            "TASK_STATE_SUBMITTED"
        );
        for (execution, expected) in [
            (json!({"state":"running"}), "TASK_STATE_WORKING"),
            (
                json!({"state":"turn_ended","response":{"stopReason":"end_turn"}}),
                "TASK_STATE_INPUT_REQUIRED",
            ),
            (
                json!({"state":"unknown","reason":"connection lost"}),
                "TASK_STATE_INPUT_REQUIRED",
            ),
            (
                json!({"state":"cancelled","response":{"stopReason":"cancelled"}}),
                "TASK_STATE_CANCELED",
            ),
            (json!({"state":"rejected","sent":false}), "TASK_STATE_REJECTED"),
        ] {
            let mut entry = accepted.clone();
            entry["execution"] = execution.clone();
            let task = task_from_entry("id", &entry, &Value::Null);
            assert_eq!(task["status"]["state"], expected);
            assert_eq!(task["metadata"]["ridge"]["execution"], execution);
        }
    }

    #[test]
    fn target_args_accepts_agent_id_without_pane_id() {
        let args = target_args(
            &json!({
                "agentId": "agent-a",
                "workspaceId": "workspace-a",
                "generation": 2,
                "lease": "lease-2"
            }),
            [("message", Value::String("hello".into()))],
        )
        .expect("agent_id-only A2A target");
        assert_eq!(args["agent_id"], "agent-a");
        assert!(args.get("target_pane_id").is_none());

        assert!(target_args::<0>(&json!({}), []).is_err());
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
    async fn member_routes_require_auth_and_reject_cross_scope_stale_generation_and_target_overrides() {
        let app=router(NativeA2aCtx::new(Arc::new(TestHost),Arc::new("token".into()),"test",Arc::new(McpSessionState::default()))
            .with_base_url("http://127.0.0.1:48000".into()));
        let path="/api/v1/a2a/members/workspace-a/agent-a/2";
        for uri in ["/api/v1/a2a/members".to_owned(),format!("{path}/.well-known/agent-card.json")] {
            let response=app.clone().oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::UNAUTHORIZED);
        }
        let response=app.clone().oneshot(Request::builder().uri("/api/v1/a2a/members").header("authorization","Bearer token").body(Body::empty()).unwrap()).await.unwrap();
        let catalog=body(response).await;
        assert_eq!(catalog["agentCards"],json!([format!("http://127.0.0.1:48000{path}/.well-known/agent-card.json")]));
        for uri in [path.replace("workspace-a","workspace-other"),path.replace("/2","/3")] {
            let response=app.clone().oneshot(Request::builder().uri(format!("{uri}/.well-known/agent-card.json")).header("authorization","Bearer token").body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::NOT_FOUND);
        }
        let request=json!({"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"hello"}]},"target":{"agentId":"someone-else"}}});
        let response=app.clone().oneshot(Request::builder().method("POST").uri(path).header("authorization","Bearer token").body(Body::from(request.to_string())).unwrap()).await.unwrap();
        let result=body(response).await;
        assert_eq!(result["error"]["code"],-32602);assert!(result["error"]["message"].as_str().unwrap().contains("target override"));
        let response=app.oneshot(Request::builder().method("POST").uri(path).header("authorization","Bearer token").header("a2a-version","0.3")
            .body(Body::from(json!({"jsonrpc":"2.0","id":2,"method":"GetTask","params":{"id":"task"}}).to_string())).unwrap()).await.unwrap();
        assert_eq!(body(response).await["error"]["message"],"unsupported A2A version; expected 1.0");
    }

    #[tokio::test]
    async fn send_get_cancel_share_authenticated_fenced_hub_receipt() {
        let target = json!({
            "agentId": "agent-a", "sessionId": "session-a", "workspaceId": "workspace-a",
            "generation": 2, "lease": "lease-2"
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
        assert_eq!(result["result"]["status"]["state"], "TASK_STATE_CANCELED");
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
