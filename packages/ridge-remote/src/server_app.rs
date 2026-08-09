//! 泛型 LAN 远控应用路由 —— 把桌面 `src-tauri/src/remote/server.rs` 的
//! 路由 / `remote_gate` / verify / ws 握手 / workspace / file / session 迁为对
//! [`Arc<dyn RemoteHost>`](crate::host::RemoteHost) 泛型的一份共享装配。
//!
//! **零 Tauri 依赖**：所有宿主特有行为都经 [`RemoteHost`] trait 方法调用；每连接
//! WS 会话经 [`RemoteHost::serve_websocket`] 一个钩子交回宿主（路线 B）。
//!
//! ## 装配（与桌面旧内联逐点一致）
//! - `serve_router::<AppCtx>()` merge 进 `/`、`/assets/*`、CA 下载、SPA fallback；
//! - `security_headers` / `compression_layer` 装最外层 `.layer`（覆盖含 fallback 的
//!   全部路由）；
//! - `remote_gate` 用 `.route_layer`（不包裹 fallback —— fallback 自门控
//!   `remote_enabled`）。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{ws::WebSocketUpgrade, ConnectInfo, FromRef, Query, State},
    http::StatusCode,
    middleware::Next,
    response::IntoResponse,
    routing::{get, post},
    Form, Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::oneshot;

use crate::host::{HostError, RemoteHost, WsConn};
use crate::serve::ServeState;

/// 主路由 State：仅持有 `Arc<dyn RemoteHost>`。serve 的 handler 经
/// `FromRef<AppCtx> for ServeState` 子状态提取拿到 serve 配置。
#[derive(Clone)]
struct AppCtx {
    host: Arc<dyn RemoteHost>,
}

impl FromRef<AppCtx> for ServeState {
    fn from_ref(ctx: &AppCtx) -> Self {
        ServeState {
            cfg: ctx.host.serve_cfg(),
            tls_enabled: ctx.host.tls_enabled(),
            enabled: ctx.host.remote_enabled(),
        }
    }
}

// ── 请求/响应形状（与桌面 server.rs 逐字一致）──────────────────────────────

#[derive(Deserialize)]
struct ConnectQuery {
    code: Option<String>,
    token: Option<String>,
    device: Option<String>,
}

#[derive(Deserialize)]
struct VerifyForm {
    code: String,
    #[serde(default)]
    device: Option<String>,
}

#[derive(Serialize)]
struct StatusResponse {
    port: u16,
    ready: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InfoResponse {
    port: u16,
    lan_ip: String,
    ready: bool,
    machine_name: String,
    // SECURITY: 绝不在此暴露 otpauth:// URI / TOTP 秘密种子（见桌面 InfoResponse）。
}

#[derive(Serialize)]
struct VerifyResponse {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Deserialize)]
struct SessionQuery {
    token: String,
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
    token: Option<String>,
    #[serde(default)]
    device: Option<String>,
}

/// 令牌载体（`<img src>` / SW 绕过 fetch 无法带 `Authorization` 头，令牌也走 `?token=`）。
#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
    #[serde(default)]
    device: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceSwitchBody {
    workspace_id: String,
}

#[derive(Deserialize)]
struct WorkspaceCreateBody {
    name: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceCloseBody {
    workspace_id: String,
}

/// 统一 TOTP 校验失败信息（审计 M3：客户端不得区分拒绝原因）。
const VERIFY_FAIL_MSG: &str = "验证失败，请稍后重试 / Verification failed, please try again later";

// ── 公开入口 ───────────────────────────────────────────────────────────────

/// 组装完整远控应用路由（泛型于宿主，State 已 apply）。调用方拿到 `Router<()>`
/// 后可直接交 [`crate::server::serve_on`]。
pub fn router(host: Arc<dyn RemoteHost>) -> Router<()> {
    let ctx = AppCtx { host };
    let serve_state = ServeState::from_ref(&ctx);

    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .route("/info", get(info_handler))
        .route("/status", get(status_handler))
        .route("/verify", get(verify_handler_get).post(verify_handler_post))
        .route("/session", get(session_handler))
        .route("/file", get(file_handler))
        .route("/workspace/list", get(workspace_list_handler))
        .route("/workspace/switch", post(workspace_switch_handler))
        .route("/workspace/create", post(workspace_create_handler))
        .route("/workspace/close", post(workspace_close_handler))
        // 前端 serve 路由（`/`、`/assets/*`、CA 下载、PWA + SPA fallback）。
        .merge(crate::serve::serve_router::<AppCtx>())
        // 安全头 + 压缩层：最外层 .layer，覆盖含 fallback 的全部路由。
        .layer(axum::middleware::from_fn_with_state(
            serve_state,
            crate::serve::security_headers,
        ))
        .layer(crate::serve::compression_layer())
        // remote_enabled 门控（route_layer 不包裹 fallback）。
        .route_layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            remote_gate,
        ))
        .with_state(ctx)
}

/// 共享服务入口：装配路由并在调用方提供的（已绑定、非阻塞）listener 上 serve。
/// TLS/bind 的多网卡证书与 fail-closed 决策留给调用方（桌面/rdg 各自不同）。
pub async fn run(
    host: Arc<dyn RemoteHost>,
    std_listener: std::net::TcpListener,
    tls_config: Option<RustlsConfig>,
    shutdown_rx: oneshot::Receiver<()>,
    require_tls: bool,
) -> Result<u16> {
    let app = router(host);
    crate::server::serve_on(std_listener, app, tls_config, shutdown_rx, require_tls).await
}

// ── 中间件 ─────────────────────────────────────────────────────────────────

/// 全路由门控：`remote_enabled` 关闭时一切请求（含 WS 升级）都 503。
async fn remote_gate(
    State(ctx): State<AppCtx>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    if !ctx.host.remote_enabled().load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Remote control disabled").into_response();
    }
    next.run(req).await
}

// ── 鉴权辅助 ───────────────────────────────────────────────────────────────

/// 从 `Authorization: Bearer <t>` 头或 `?token=` 取令牌，按严格设备绑定校验。
fn is_request_authed(
    ctx: &AppCtx,
    headers: &axum::http::HeaderMap,
    query_token: Option<&str>,
    device_id: &str,
    ip: &str,
) -> bool {
    let header_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::trim));
    let token = header_token.or(query_token);
    token
        .map(|t| ctx.host.validate_token_device_strict(t, device_id, ip))
        .unwrap_or(false)
}

/// 规范化每个允许根后，判断 `target`（调用方已规范化）是否落在其中之一内。
fn is_within_allowed_roots(target: &std::path::Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|canon_root| target.starts_with(&canon_root))
            .unwrap_or(false)
    })
}

fn host_err_response(e: HostError) -> (StatusCode, Json<serde_json::Value>) {
    let (code, msg) = match e {
        HostError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
        HostError::NotFound(m) => (StatusCode::NOT_FOUND, m),
    };
    (code, Json(json!({ "success": false, "error": msg })))
}

// ── Handlers ───────────────────────────────────────────────────────────────

async fn health_handler() -> &'static str {
    "ok"
}

async fn info_handler(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let enabled = ctx.host.remote_enabled().load(Ordering::Relaxed);
    Json(InfoResponse {
        port: ctx.host.port(),
        lan_ip: ctx.host.lan_ip(),
        ready: enabled,
        machine_name: ctx.host.machine_name(),
    })
}

async fn status_handler(State(ctx): State<AppCtx>) -> Json<StatusResponse> {
    Json(StatusResponse {
        port: ctx.host.port(),
        ready: true,
    })
}

/// GET /verify —— 始终是移动 SPA 壳（桌面 SPA 自带 +layout 鉴权门）。
async fn verify_handler_get(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let cfg = ctx.host.serve_cfg();
    crate::serve::serve_shell(crate::serve::UiTarget {
        kind: crate::embed_ui::UiKind::Mobile,
        dir: cfg.disk_dir(crate::embed_ui::UiKind::Mobile),
    })
    .await
}

async fn verify_handler_post(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
    Form(form): Form<VerifyForm>,
) -> Json<VerifyResponse> {
    let device_id = form.device.clone().unwrap_or_default();
    let ip = addr.ip().to_string();
    // 暴力破解节流 + 黑名单闸门，统一失败信息（审计 C1/M3）。
    if ctx.host.is_blacklisted(&device_id, &ip)
        || ctx.host.pre_verify_gate(&ip, &device_id).is_err()
    {
        return Json(VerifyResponse {
            success: false,
            message: VERIFY_FAIL_MSG.to_string(),
            token: None,
        });
    }
    let valid = ctx.host.verify_code(&form.code);
    ctx.host.post_verify_record(&ip, &device_id, valid);
    let token = if valid {
        Some(ctx.host.create_session_token(&device_id, &ip))
    } else {
        None
    };
    Json(VerifyResponse {
        success: valid,
        message: if valid {
            "Verification successful".to_string()
        } else {
            VERIFY_FAIL_MSG.to_string()
        },
        token,
    })
}

/// 会话令牌探活。
async fn session_handler(
    Query(query): Query<SessionQuery>,
    State(ctx): State<AppCtx>,
) -> impl IntoResponse {
    let valid = ctx.host.validate_token(&query.token);
    Json(json!({ "valid": valid }))
}

/// WS 升级：`?code=`（TOTP）或 `?token=`（会话令牌）。鉴权通过后把连接交给
/// [`RemoteHost::serve_websocket`]。
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<ConnectQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
) -> impl IntoResponse {
    if !ctx.host.remote_enabled().load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "remote control disabled").into_response();
    }
    let remote_addr = addr.ip().to_string();
    let device_id = query.device.clone().unwrap_or_default();
    // 黑名单：即便令牌有效也拒绝被封设备/IP。
    if ctx.host.is_blacklisted(&device_id, &remote_addr) {
        return (StatusCode::FORBIDDEN, "device is blacklisted").into_response();
    }
    let valid = if let Some(ref t) = query.token {
        // 令牌路径不可暴力破解（256-bit CSPRNG），绕过 TOTP 节流；强制设备+IP 绑定。
        ctx.host
            .validate_token_device_strict(t, &device_id, &remote_addr)
    } else if let Some(ref c) = query.code {
        // `?code=` TOTP 路径可暴力破解，与 POST /verify 共享同一节流/封禁。
        if ctx.host.pre_verify_gate(&remote_addr, &device_id).is_err() {
            return (StatusCode::UNAUTHORIZED, "invalid authentication").into_response();
        }
        let ok = ctx.host.verify_code(c);
        ctx.host.post_verify_record(&remote_addr, &device_id, ok);
        ok
    } else {
        false
    };
    if !valid {
        return (StatusCode::UNAUTHORIZED, "invalid authentication").into_response();
    }
    let conn = WsConn {
        remote_addr,
        device_id,
        token: query.token.clone(),
    };
    let host = ctx.host.clone();
    ws.on_upgrade(move |socket| host.serve_websocket(socket, conn))
        .into_response()
}

/// 单文件字节 serve（桌面 UI 的 `convertFileSrc` shim；令牌鉴权 + 允许根包含校验）。
async fn file_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
    Query(q): Query<FileQuery>,
) -> impl IntoResponse {
    let ip = addr.ip().to_string();
    let device_id = q.device.clone().unwrap_or_default();
    let authed = q
        .token
        .as_deref()
        .map(|t| ctx.host.validate_token_bound(t, &device_id, &ip))
        .unwrap_or(false);
    if !authed {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    // 第一道字符串守卫（纵深防御；下面 canonical 校验才是权威）。
    if q.path.split(['/', '\\']).any(|c| c == "..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let full = PathBuf::from(&q.path);
    if !full.is_file() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let canon = match tokio::fs::canonicalize(&full).await {
        Ok(c) => c,
        Err(_) => return (StatusCode::NOT_FOUND, "not found").into_response(),
    };
    let roots = ctx.host.allowed_file_roots();
    let canon_for_check = canon.clone();
    let within =
        tokio::task::spawn_blocking(move || is_within_allowed_roots(&canon_for_check, &roots))
            .await
            .unwrap_or(false);
    if !within {
        tracing::warn!(target: "ridge::remote", path = %q.path, "file_handler rejected: outside allowed roots");
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    // 读规范化后的路径（关闭 TOCTOU 窗口，审计 L-1）。
    match tokio::fs::read(&canon).await {
        Ok(bytes) => {
            let name = canon.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let (content_type, _) = crate::serve::root_asset_headers(name);
            axum::response::Response::builder()
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .header(axum::http::header::CACHE_CONTROL, "private, max-age=60")
                .body(axum::body::Body::from(bytes))
                .unwrap()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response(),
    }
}

// ── Workspace 控制面 ────────────────────────────────────────────────────────

async fn workspace_list_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
    headers: axum::http::HeaderMap,
    Query(q): Query<TokenQuery>,
) -> axum::response::Response {
    let ip = addr.ip().to_string();
    let device_id = q.device.clone().unwrap_or_default();
    if !is_request_authed(&ctx, &headers, q.token.as_deref(), &device_id, &ip) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    Json(ctx.host.list_workspaces_json()).into_response()
}

async fn workspace_switch_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
    headers: axum::http::HeaderMap,
    Query(q): Query<TokenQuery>,
    Json(body): Json<WorkspaceSwitchBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let ip = addr.ip().to_string();
    let device_id = q.device.clone().unwrap_or_default();
    if !is_request_authed(&ctx, &headers, q.token.as_deref(), &device_id, &ip) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"success":false,"error":"invalid token"})),
        );
    }
    match ctx.host.switch_workspace(&body.workspace_id) {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => host_err_response(e),
    }
}

async fn workspace_create_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
    headers: axum::http::HeaderMap,
    Query(q): Query<TokenQuery>,
    Json(body): Json<WorkspaceCreateBody>,
) -> axum::response::Response {
    let ip = addr.ip().to_string();
    let device_id = q.device.clone().unwrap_or_default();
    if !is_request_authed(&ctx, &headers, q.token.as_deref(), &device_id, &ip) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    match ctx.host.create_workspace(body.name) {
        Ok(v) => Json(v).into_response(),
        Err(e) => host_err_response(e).into_response(),
    }
}

async fn workspace_close_handler(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<AppCtx>,
    headers: axum::http::HeaderMap,
    Query(q): Query<TokenQuery>,
    Json(body): Json<WorkspaceCloseBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let ip = addr.ip().to_string();
    let device_id = q.device.clone().unwrap_or_default();
    if !is_request_authed(&ctx, &headers, q.token.as_deref(), &device_id, &ip) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"success":false,"error":"invalid token"})),
        );
    }
    match ctx.host.close_workspace(&body.workspace_id) {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => host_err_response(e),
    }
}
