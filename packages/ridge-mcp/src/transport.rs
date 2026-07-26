//! MCP 传输层（axum）：WebSocket + HTTP 两条，**桌面与 rdg 共用同一份**。
//!
//! 宿主只提供 [`McpHost`] 实装与 token，路由、鉴权、握手、通知语义都在这里，
//! 不允许两端各写各的（历史上 MCP 只长在桌面，rdg 完全没有，能力也就分叉了）。
//!
//! 为什么两条传输：
//! - `ws://…/api/v1/mcp/ws` —— 老客户端与自写脚本；
//! - `POST …/api/v1/mcp` —— Claude Code 等只支持 stdio/sse/http 的客户端直连。

use std::sync::Arc;

use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};

use crate::server::{handle_message, McpHost};

/// 传输层共享状态：宿主实装 + 鉴权 token + 对外报的版本号。
#[derive(Clone)]
pub struct McpTransportCtx {
    pub host: Arc<dyn McpHost>,
    pub token: Arc<String>,
    pub version: Arc<String>,
}

impl McpTransportCtx {
    pub fn new(host: Arc<dyn McpHost>, token: Arc<String>, version: impl Into<String>) -> Self {
        Self {
            host,
            token,
            version: Arc::new(version.into()),
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

/// 挂上两条 MCP 路由。宿主把它 `merge` 进自己的 router 即可。
pub fn mcp_router(ctx: McpTransportCtx) -> Router {
    Router::new()
        .route("/api/v1/mcp/ws", get(route_ws))
        .route("/api/v1/mcp", post(route_http))
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
        let Some(reply) = handle_message(&text, ctx.host.as_ref(), &ctx.version) else {
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
    match handle_message(&body, ctx.host.as_ref(), &ctx.version) {
        Some(reply) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            reply,
        )
            .into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}
