//! MCP 传输层（axum）：WebSocket + HTTP 两条，**桌面与 rdg 共用同一份**。
//!
//! 宿主只提供 [`McpHost`] 实装与 token，路由、鉴权、握手、通知语义都在这里，
//! 不允许两端各写各的（历史上 MCP 只长在桌面，rdg 完全没有，能力也就分叉了）。
//!
//! 为什么两条传输：
//! - `ws://…/api/v1/mcp/ws` —— 老客户端与自写脚本；
//! - `POST …/api/v1/mcp` —— Claude Code 等只支持 stdio/sse/http 的客户端直连。
//!
//! Agent delivery stream：`GET /api/v1/agent-events/ws` 是 Ridge-owned 的
//! authenticated Runtime API/A2A bridge，不冒充第三方 CLI 私有协议。连接先发
//! `{type:register,adapter,agentId,generation,lease}`，服务端只在当前 roster
//! 身份通过授权与 fencing 后注册有界 route；服务端发送原始 Hub envelope，Agent
//! 可回 `{type:ack,deliveryId,status,detail}`。断连、替换 generation 与队列满均
//! 可观察且 fail-closed，MCP pull 仍保留为 durable recovery path。

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::delivery::{HubDeliveryAdapter, DELIVERY_ROUTE_CAP};
use crate::server::{handle_message_with_state, McpHost, McpSessionState};

pub const AGENT_DELIVERY_STREAM_PATH: &str = "/api/v1/agent-events/ws";
const DELIVERY_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const DELIVERY_FORWARD_CAP: usize = 1;
const DELIVERY_CONTROL_MAX_BYTES: usize = 4096;
const DELIVERY_AGENT_ID_MAX_BYTES: usize = 256;
const DELIVERY_LEASE_MAX_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeliveryRegistration {
    adapter: HubDeliveryAdapter,
    agent_id: String,
    generation: u64,
    lease: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDeliveryRegistration {
    #[serde(rename = "type")]
    message_type: String,
    adapter: String,
    #[serde(alias = "agent_id")]
    agent_id: String,
    generation: u64,
    lease: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeliveryAck {
    #[serde(rename = "type")]
    message_type: String,
    delivery_id: String,
    status: String,
    detail: Option<String>,
}

fn parse_delivery_registration(text: &str) -> Result<DeliveryRegistration, String> {
    if text.len() > DELIVERY_CONTROL_MAX_BYTES {
        return Err("delivery registration is too large".into());
    }
    let raw: RawDeliveryRegistration =
        serde_json::from_str(text).map_err(|_| "invalid delivery registration JSON".to_string())?;
    if raw.message_type != "register" {
        return Err("delivery stream requires a register message".into());
    }
    let adapter = match raw.adapter.as_str() {
        "runtime_api" | "RuntimeApi" => HubDeliveryAdapter::RuntimeApi,
        "a2a" | "A2a" => HubDeliveryAdapter::A2a,
        _ => return Err("delivery stream adapter must be runtime_api or a2a".into()),
    };
    if raw.agent_id.trim().is_empty() || raw.agent_id.len() > DELIVERY_AGENT_ID_MAX_BYTES {
        return Err("delivery registration has an invalid agent_id".into());
    }
    if raw.generation == 0
        || raw.lease.trim().is_empty()
        || raw.lease.len() > DELIVERY_LEASE_MAX_BYTES
    {
        return Err("delivery registration requires a bounded generation and lease".into());
    }
    Ok(DeliveryRegistration {
        adapter,
        agent_id: raw.agent_id,
        generation: raw.generation,
        lease: raw.lease,
    })
}

fn parse_delivery_ack(text: &str) -> Result<DeliveryAck, String> {
    if text.len() > DELIVERY_CONTROL_MAX_BYTES {
        return Err("delivery control message is too large".into());
    }
    let ack: DeliveryAck =
        serde_json::from_str(text).map_err(|_| "invalid delivery control JSON".to_string())?;
    if ack.message_type != "ack"
        || ack.delivery_id.trim().is_empty()
        || ack.delivery_id.len() > DELIVERY_CONTROL_MAX_BYTES
        || ack.status.trim().is_empty()
        || ack
            .detail
            .as_deref()
            .is_some_and(|detail| detail.len() > DELIVERY_CONTROL_MAX_BYTES)
    {
        return Err("invalid delivery acknowledgement".into());
    }
    Ok(ack)
}

/// 传输层共享状态：宿主实装 + 鉴权 token + 对外报的版本号。
#[derive(Clone)]
pub struct McpTransportCtx {
    pub host: Arc<dyn McpHost>,
    pub token: Arc<String>,
    pub version: Arc<String>,
    pub state: Arc<McpSessionState>,
}

impl McpTransportCtx {
    pub fn new(host: Arc<dyn McpHost>, token: Arc<String>, version: impl Into<String>) -> Self {
        Self::with_state(host, token, version, Arc::new(McpSessionState::default()))
    }

    pub fn with_state(
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
        }
    }
}

/// `x-ridge-token` 或 `Authorization: Bearer`，与 teammate 其它路由同口径。
fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    if headers
        .get("x-ridge-token")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == token)
    {
        return true;
    }
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|v| v == token)
}

/// 挂上共享 MCP 路由与 Agent delivery stream。宿主把它 `merge` 进自己的
/// router 即可；两者共用同一份 Hub state 与鉴权边界。
pub fn mcp_router(ctx: McpTransportCtx) -> Router {
    Router::new()
        .route("/api/v1/mcp/ws", get(route_ws))
        .route("/api/v1/mcp", post(route_http))
        .route(AGENT_DELIVERY_STREAM_PATH, get(route_delivery_stream))
        .with_state(ctx)
}

async fn route_ws(
    ws: WebSocketUpgrade,
    State(ctx): State<McpTransportCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| serve_ws(socket, ctx))
}

async fn serve_ws(mut socket: WebSocket, ctx: McpTransportCtx) {
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        // 通知（无 id）没有响应体：静默即正确。
        let Some(reply) =
            handle_message_with_state(&text, ctx.host.as_ref(), &ctx.version, ctx.state.as_ref())
        else {
            continue;
        };
        if socket.send(Message::Text(reply)).await.is_err() {
            break;
        }
    }
}

/// 一发一收的 JSON-RPC over HTTP：请求回 `application/json`，通知回 `202`。
async fn route_http(
    State(ctx): State<McpTransportCtx>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    match handle_message_with_state(&body, ctx.host.as_ref(), &ctx.version, ctx.state.as_ref()) {
        Some(reply) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            reply,
        )
            .into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn route_delivery_stream(
    ws: WebSocketUpgrade,
    State(ctx): State<McpTransportCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| serve_delivery_stream(socket, ctx))
}

async fn send_delivery_frame(socket: &mut WebSocket, value: Value) -> bool {
    socket
        .send(Message::Text(value.to_string().into()))
        .await
        .is_ok()
}

async fn send_delivery_error(socket: &mut WebSocket, message: impl Into<String>) -> bool {
    send_delivery_frame(
        socket,
        json!({
            "type": "error",
            "message": message.into(),
        }),
    )
    .await
}

async fn serve_delivery_stream(mut socket: WebSocket, ctx: McpTransportCtx) {
    let first = tokio::time::timeout(DELIVERY_HANDSHAKE_TIMEOUT, socket.recv()).await;
    let registration = match first {
        Ok(Some(Ok(Message::Text(text)))) => match parse_delivery_registration(&text) {
            Ok(registration) => registration,
            Err(error) => {
                let _ = send_delivery_error(&mut socket, error).await;
                return;
            }
        },
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return,
        Ok(Some(Err(_))) => return,
        Ok(Some(Ok(_))) => {
            let _ = send_delivery_error(&mut socket, "delivery stream requires text registration")
                .await;
            return;
        }
        Err(_) => {
            let _ = send_delivery_error(&mut socket, "delivery registration timed out").await;
            return;
        }
    };

    if let Err(error) = ctx.host.authorize_delivery_endpoint(
        &registration.agent_id,
        registration.generation,
        &registration.lease,
    ) {
        let _ = send_delivery_error(&mut socket, error.message()).await;
        return;
    }

    let receiver = match ctx.state.register_delivery_endpoint(
        registration.adapter,
        registration.agent_id.clone(),
        registration.generation,
        registration.lease.clone(),
    ) {
        Ok(receiver) => receiver,
        Err(error) => {
            let _ = send_delivery_error(&mut socket, error).await;
            return;
        }
    };
    if !send_delivery_frame(
        &mut socket,
        json!({
            "type": "registered",
            "adapter": registration.adapter.as_str(),
            "agentId": registration.agent_id,
            "generation": registration.generation,
            "queueCapacity": DELIVERY_ROUTE_CAP,
        }),
    )
    .await
    {
        let _ = ctx.state.unregister_delivery_endpoint(
            registration.adapter,
            &registration.agent_id,
            registration.generation,
            &registration.lease,
        );
        return;
    }

    let (forward_tx, mut forward_rx) = mpsc::channel(DELIVERY_FORWARD_CAP);
    let bridge = tokio::task::spawn_blocking(move || {
        while let Ok(entry) = receiver.recv() {
            if forward_tx.blocking_send(entry).is_err() {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            entry = forward_rx.recv() => {
                let Some(entry) = entry else { break };
                if !send_delivery_frame(&mut socket, entry).await { break; }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        match parse_delivery_ack(&text) {
                            Ok(ack) => {
                                let result = ctx.state.acknowledge_delivery(
                                    &registration.agent_id,
                                    registration.generation,
                                    &registration.lease,
                                    &ack.delivery_id,
                                    &ack.status,
                                    ack.detail.as_deref(),
                                );
                                let response = match result {
                                    Ok(entry) => json!({
                                        "type": "ack",
                                        "accepted": true,
                                        "deliveryId": ack.delivery_id,
                                        "entry": entry,
                                    }),
                                    Err(error) => json!({
                                        "type": "ack",
                                        "accepted": false,
                                        "deliveryId": ack.delivery_id,
                                        "error": error,
                                    }),
                                };
                                if !send_delivery_frame(&mut socket, response).await { break; }
                            }
                            Err(error) => {
                                if !send_delivery_error(&mut socket, error).await { break; }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }

    let _ = ctx.state.unregister_delivery_endpoint(
        registration.adapter,
        &registration.agent_id,
        registration.generation,
        &registration.lease,
    );
    let _ = bridge.await;
}

#[cfg(test)]
mod tests {
    use crate::delivery::{DeliveryOutcome, DeliveryProbe};
    use crate::resource::RidgeUri;
    use crate::server::{handle_message_with_state, HostError, HostResult, InputDispatch};
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message as ClientMessage;

    use super::*;

    #[test]
    fn delivery_registration_requires_fenced_identity_and_supported_adapter() {
        let registration = parse_delivery_registration(
            r#"{"type":"register","adapter":"runtime_api","agentId":"agent-a","generation":2,"lease":"lease-2"}"#,
        )
        .expect("valid registration");
        assert_eq!(registration.adapter, HubDeliveryAdapter::RuntimeApi);
        assert_eq!(registration.agent_id, "agent-a");
        assert!(parse_delivery_registration(
            r#"{"type":"register","adapter":"mcp_pull","agentId":"agent-a","generation":2,"lease":"lease-2"}"#,
        )
        .is_err());
        assert!(parse_delivery_registration(
            r#"{"type":"register","adapter":"a2a","agentId":"agent-a","generation":0,"lease":"lease-2"}"#,
        )
        .is_err());
    }

    #[test]
    fn delivery_ack_requires_ack_type_and_bounded_fields() {
        let ack = parse_delivery_ack(
            r#"{"type":"ack","deliveryId":"delivery-1","status":"agent_received","detail":"ok"}"#,
        )
        .expect("valid acknowledgement");
        assert_eq!(ack.delivery_id, "delivery-1");
        assert!(parse_delivery_ack(
            r#"{"type":"register","deliveryId":"delivery-1","status":"agent_received"}"#,
        )
        .is_err());
    }

    struct TestHost {
        state: Arc<McpSessionState>,
    }

    impl McpHost for TestHost {
        fn team_profile(&self) -> Value {
            json!({
                "roster": [{
                    "id": "agent-a",
                    "agentId": "agent-a",
                    "sessionId": "session-a",
                    "workspaceId": "workspace-a",
                    "paneId": "pane-a",
                    "generation": 2,
                    "lease": "lease-2",
                    "lifecycle": "Online",
                    "online": true,
                    "capabilities": ["messages"]
                }]
            })
        }

        fn probe_delivery(&self, target: &Value) -> HostResult<DeliveryProbe> {
            let mut probe = self.state.delivery_probe(target);
            probe.mcp_pull = true;
            Ok(probe)
        }

        fn deliver_runtime_api(
            &self,
            target: &Value,
            entry: &Value,
        ) -> HostResult<DeliveryOutcome> {
            self.state
                .deliver_registered_endpoint(HubDeliveryAdapter::RuntimeApi, target, entry)
                .map_err(HostError::Internal)
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
            Ok(target
                .get("paneId")
                .and_then(Value::as_str)
                .unwrap_or("pane-a")
                .into())
        }
    }

    #[test]
    fn delivery_stream_routes_real_socket_and_unregisters_on_disconnect() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let state = Arc::new(McpSessionState::default());
            let host = Arc::new(TestHost {
                state: state.clone(),
            });
            let app = mcp_router(McpTransportCtx::with_state(
                host.clone(),
                Arc::new("test-token".into()),
                "test",
                state.clone(),
            ));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind test listener");
            let address = listener.local_addr().expect("listener address");
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.expect("serve test router");
            });
            let url = format!("ws://{address}{AGENT_DELIVERY_STREAM_PATH}");
            assert!(tokio_tungstenite::connect_async(url.clone()).await.is_err());
            let mut request = url
                .clone()
                .into_client_request()
                .expect("websocket request");
            request
                .headers_mut()
                .insert("x-ridge-token", "test-token".parse().expect("token header"));
            let (mut socket, _) = tokio_tungstenite::connect_async(request)
                .await
                .expect("connect delivery stream");
            socket
                .send(ClientMessage::Text(
                    json!({
                        "type": "register",
                        "adapter": "runtime_api",
                        "agentId": "agent-a",
                        "generation": 2,
                        "lease": "lease-2"
                    })
                    .to_string(),
                ))
                .await
                .expect("register delivery stream");
            let registered = tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .expect("registration response timeout")
                .expect("registration response")
                .expect("registration frame");
            assert!(registered
                .to_text()
                .expect("text registration")
                .contains("registered"));

            let send = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "ridge_send_message",
                    "arguments": {
                        "target_pane_id": "pane-a",
                        "message": "cross-process",
                        "from": "agent-sender",
                        "idempotency_key": "stream-1"
                    }
                }
            })
            .to_string();
            let response = handle_message_with_state(&send, host.as_ref(), "test", &state)
                .expect("Hub response");
            let response: Value = serde_json::from_str(&response).expect("Hub JSON response");
            let entry: Value = serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .expect("Hub delivery entry"),
            )
            .expect("Hub delivery JSON");
            assert_eq!(entry["deliveryAdapter"], "runtime_api");
            let delivered = tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .expect("delivery response timeout")
                .expect("delivery response")
                .expect("delivery frame");
            let delivered: Value =
                serde_json::from_str(delivered.to_text().unwrap()).expect("delivery JSON");
            assert_eq!(delivered["messageId"], entry["messageId"]);
            assert_eq!(delivered["deliveryId"], entry["deliveryId"]);
            assert_eq!(delivered["payload"], entry["payload"]);
            let delivery_id = entry["deliveryId"].as_str().expect("delivery id");
            socket
                .send(ClientMessage::Text(
                    json!({
                        "type": "ack",
                        "deliveryId": delivery_id,
                        "status": "agent_acknowledged",
                        "detail": "received over stream"
                    })
                    .to_string(),
                ))
                .await
                .expect("ack delivery");
            let ack = tokio::time::timeout(Duration::from_secs(2), socket.next())
                .await
                .expect("ack response timeout")
                .expect("ack response")
                .expect("ack frame");
            let ack: Value = serde_json::from_str(ack.to_text().unwrap()).expect("ack JSON");
            assert_eq!(ack["type"], "ack");
            assert_eq!(ack["accepted"], true);
            let target = json!({
                "agentId": "agent-a",
                "generation": 2,
                "lease": "lease-2"
            });
            assert!(state.delivery_probe(&target).runtime_api);
            socket.close(None).await.expect("close delivery stream");
            tokio::time::timeout(Duration::from_secs(2), async {
                while state.delivery_probe(&target).runtime_api {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("disconnect unregister timeout");
            server.abort();
        });
    }
}
