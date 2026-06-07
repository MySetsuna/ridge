//! Teammate-protocol HTTP surface for the native tmux engine (feature = "http").
//!
//! This is **one** axum router over the engine in the crate root, mounted by:
//!   - the desktop teammate server (`src-tauri/src/teammate/server.rs`), which
//!     adds the GUI-only `summon` route on top and supplies a GUI-backed
//!     [`GuiSessionSource`]; and
//!   - the headless `ridge-cli` host, which mounts it with [`NoGuiSessions`] so
//!     the same `tmux` shim works against a server with no desktop workspaces.
//!
//! The only host-specific seam is [`GuiSessionSource`]: on the default socket
//! the engine folds the host's GUI workspace sessions into `find-target`
//! resolution (so `ls`/`has-session`/`resolve` see both). Headless hosts return
//! an empty set. The GUI-only `summon` (adopt a session into a visible
//! workspace) is NOT here — it needs the host's workspace state and stays in
//! the desktop server.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;

use crate::{GuiSession, NativeError, NewSessionReq};

// ===================== host seam =====================

/// Supplies the GUI workspace sessions that participate in default-socket
/// `find-target` resolution. The desktop host backs this with its live
/// workspace map; headless hosts use [`NoGuiSessions`].
pub trait GuiSessionSource: Send + Sync + 'static {
    /// GUI sessions to fold in for `socket` (empty for non-default sockets and
    /// for headless hosts).
    fn sessions_for(&self, socket: &str) -> Vec<GuiSession>;
    /// GUI workspace `ls` lines prepended to native `list-sessions` on the
    /// default socket (empty for headless hosts).
    fn session_lines(&self, fmt: Option<&str>) -> Vec<String>;
}

/// Headless host: no desktop workspaces exist, so no GUI sessions fold in.
pub struct NoGuiSessions;

impl GuiSessionSource for NoGuiSessions {
    fn sessions_for(&self, _socket: &str) -> Vec<GuiSession> {
        Vec::new()
    }
    fn session_lines(&self, _fmt: Option<&str>) -> Vec<String> {
        Vec::new()
    }
}

/// Shared axum state for the native routes: the auth token plus the host's
/// GUI-session seam. Cheap to clone (two `Arc`s).
#[derive(Clone)]
pub struct NativeHttpCtx {
    pub token: Arc<String>,
    pub gui: Arc<dyn GuiSessionSource>,
}

impl NativeHttpCtx {
    /// Convenience constructor for headless hosts (no GUI sessions).
    pub fn headless(token: impl Into<String>) -> Self {
        Self {
            token: Arc::new(token.into()),
            gui: Arc::new(NoGuiSessions),
        }
    }
}

// ===================== router =====================

/// The native `/api/v1/tmux/*` router (everything except GUI-only `summon`).
pub fn native_router(ctx: NativeHttpCtx) -> Router {
    Router::new()
        .route("/api/v1/tmux/new-session", post(route_new_session))
        .route("/api/v1/tmux/has-session", get(route_has_session))
        .route("/api/v1/tmux/resolve", get(route_resolve))
        .route("/api/v1/tmux/list-sessions", get(route_list_sessions))
        .route("/api/v1/tmux/list-panes", get(route_list_panes))
        .route("/api/v1/tmux/capture-pane", get(route_capture))
        .route("/api/v1/tmux/list-windows", get(route_list_windows))
        .route("/api/v1/tmux/display-message", get(route_display_message))
        .route("/api/v1/tmux/split-window", post(route_split_window))
        .route("/api/v1/tmux/send-keys", post(route_send_keys))
        .route("/api/v1/tmux/select", post(route_select))
        .route("/api/v1/tmux/kill", post(route_kill))
        .route("/api/v1/tmux/list-all-sessions", get(route_list_all_sessions))
        .with_state(ctx)
}

/// Bind `listener` and serve the native router until shutdown. Headless hosts
/// (`ridge-cli`) call this directly; the desktop server merges
/// [`native_router`] into its larger router instead.
pub async fn serve(listener: tokio::net::TcpListener, ctx: NativeHttpCtx) -> std::io::Result<()> {
    axum::serve(listener, native_router(ctx)).await
}

// ===================== helpers =====================

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

fn native_err_to_response(e: NativeError) -> axum::response::Response {
    match e {
        // 命中 GUI 会话：让 shim 回退到 GUI 路径。
        NativeError::Gui(name) => (StatusCode::CONFLICT, format!("gui:{name}")).into_response(),
        NativeError::NotFound(m) | NativeError::Ambiguous(m) | NativeError::NoServer(m) => {
            (StatusCode::NOT_FOUND, m).into_response()
        }
        NativeError::Duplicate(m) => (StatusCode::BAD_REQUEST, m).into_response(),
        NativeError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m).into_response(),
    }
}

fn default_socket() -> String {
    "default".to_string()
}

fn q_socket(q: &HashMap<String, String>) -> String {
    q.get("socket")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(default_socket)
}

fn tmux_default_cols() -> u16 {
    80
}
fn tmux_default_rows() -> u16 {
    24
}

// ===================== request bodies =====================

#[derive(Deserialize)]
struct TmuxNewSessionBody {
    #[serde(default = "default_socket")]
    socket: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    window_name: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default = "tmux_default_cols")]
    width: u16,
    #[serde(default = "tmux_default_rows")]
    height: u16,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    attach_or_create: bool,
    #[serde(default)]
    print: bool,
    #[serde(default)]
    print_format: Option<String>,
}

#[derive(Deserialize)]
struct TmuxSplitBody {
    #[serde(default = "default_socket")]
    socket: String,
    #[serde(default)]
    target: String,
    /// 无头会话不需要方向，仅作占位以兼容客户端。
    #[serde(default)]
    #[allow(dead_code)]
    horizontal: bool,
    #[serde(default)]
    new_window: bool,
    #[serde(default)]
    window_name: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    print: bool,
    #[serde(default)]
    print_format: Option<String>,
}

#[derive(Deserialize)]
struct TmuxSendKeysBody {
    #[serde(default = "default_socket")]
    socket: String,
    #[serde(default)]
    target: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct TmuxSelectBody {
    #[serde(default = "default_socket")]
    socket: String,
    #[serde(default)]
    target: String,
}

#[derive(Deserialize)]
struct TmuxKillBody {
    #[serde(default = "default_socket")]
    socket: String,
    #[serde(default)]
    target: String,
    /// "session"（默认）| "pane" | "window" | "server"
    #[serde(default)]
    scope: String,
}

// ===================== handlers =====================

async fn route_new_session(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Json(body): Json<TmuxNewSessionBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let gui = ctx.gui.sessions_for(&body.socket);
    let print = if body.print {
        Some(body.print_format.clone())
    } else {
        None
    };
    let req = NewSessionReq {
        socket: body.socket,
        name: body.name,
        window_name: body.window_name,
        cwd: body.cwd,
        width: body.width,
        height: body.height,
        shell: body.shell,
        command: body.command,
        attach_or_create: body.attach_or_create,
        print,
    };
    match crate::new_session(req, &gui) {
        Ok(out) => (StatusCode::OK, out).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_has_session(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let target = q.get("target").cloned().unwrap_or_default();
    let gui = ctx.gui.sessions_for(&socket);
    match crate::has_session(&socket, &target, &gui) {
        Ok(_) => (StatusCode::OK, "").into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_resolve(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let target = q.get("target").cloned().unwrap_or_default();
    let gui = ctx.gui.sessions_for(&socket);
    match crate::resolve(&socket, &target, &gui) {
        Ok(r) => Json(serde_json::json!({
            "found": true,
            "kind": "native",
            "socket": r.socket,
            "session": r.session,
            "window": r.window_index,
            "pane": r.pane_index,
            "pane_id": format!("%{}", r.pane_global_id),
        }))
        .into_response(),
        Err(NativeError::Gui(name)) => Json(serde_json::json!({
            "found": true,
            "kind": "gui",
            "session": name,
        }))
        .into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "found": false, "error": e.message() })),
        )
            .into_response(),
    }
}

async fn route_list_sessions(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let format = q.get("format").cloned().filter(|s| !s.is_empty());
    let mut lines: Vec<String> = Vec::new();
    if socket == "default" {
        lines.extend(ctx.gui.session_lines(format.as_deref()));
    }
    lines.extend(crate::list_sessions_lines(&socket, format.as_deref()));
    (StatusCode::OK, lines.join("\n")).into_response()
}

async fn route_list_panes(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let target = q.get("target").cloned().unwrap_or_default();
    let format = q.get("format").cloned().filter(|s| !s.is_empty());
    let all = q.get("all").map(|s| s == "1").unwrap_or(false);
    let gui = ctx.gui.sessions_for(&socket);
    match crate::list_panes_lines(&socket, &target, &gui, format.as_deref(), all) {
        Ok(lines) => (StatusCode::OK, lines.join("\n")).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_list_windows(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let target = q.get("target").cloned().unwrap_or_default();
    let format = q.get("format").cloned().filter(|s| !s.is_empty());
    let gui = ctx.gui.sessions_for(&socket);
    match crate::list_windows_lines(&socket, &target, &gui, format.as_deref()) {
        Ok(lines) => (StatusCode::OK, lines.join("\n")).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_display_message(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let target = q.get("target").cloned().unwrap_or_default();
    let format = q
        .get("format")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "#{pane_id}".to_string());
    let gui = ctx.gui.sessions_for(&socket);
    match crate::display_message(&socket, &target, &gui, &format) {
        Ok(out) => (StatusCode::OK, out).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

/// `capture-pane -p`：渲染目标 native 面板当前屏为纯文本。`lines` 可选，取末 n 行。
async fn route_capture(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let socket = q_socket(&q);
    let target = q.get("target").cloned().unwrap_or_default();
    let lines = q.get("lines").and_then(|s| s.parse::<usize>().ok());
    let gui = ctx.gui.sessions_for(&socket);
    match crate::capture(&socket, &target, &gui, lines) {
        Ok(out) => (StatusCode::OK, out).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_split_window(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Json(body): Json<TmuxSplitBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let gui = ctx.gui.sessions_for(&body.socket);
    let print = if body.print {
        Some(body.print_format.as_deref())
    } else {
        None
    };
    match crate::add_pane(
        &body.socket,
        &body.target,
        &gui,
        body.new_window,
        body.window_name.as_deref(),
        body.cwd.as_deref(),
        body.shell.as_deref(),
        body.command.as_deref(),
        print,
    ) {
        Ok(out) => (StatusCode::OK, out).into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_send_keys(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Json(body): Json<TmuxSendKeysBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let gui = ctx.gui.sessions_for(&body.socket);
    match crate::send_keys(&body.socket, &body.target, &gui, &body.text) {
        Ok(_) => (StatusCode::OK, "").into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_select(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Json(body): Json<TmuxSelectBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let gui = ctx.gui.sessions_for(&body.socket);
    match crate::select(&body.socket, &body.target, &gui) {
        Ok(_) => (StatusCode::OK, "").into_response(),
        Err(e) => native_err_to_response(e),
    }
}

async fn route_kill(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
    Json(body): Json<TmuxKillBody>,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let gui = ctx.gui.sessions_for(&body.socket);
    let res = match body.scope.as_str() {
        "server" => crate::kill_server(&body.socket),
        "pane" => crate::kill_pane(&body.socket, &body.target, &gui),
        "window" => crate::kill_window(&body.socket, &body.target, &gui),
        _ => crate::kill_session(&body.socket, &body.target, &gui),
    };
    match res {
        Ok(_) => (StatusCode::OK, "").into_response(),
        Err(e) => native_err_to_response(e),
    }
}

/// `GET /api/v1/tmux/list-all-sessions` — 跨所有 socket 列出 native 会话摘要。
async fn route_list_all_sessions(
    State(ctx): State<NativeHttpCtx>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !auth_ok(&headers, &ctx.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let sessions = crate::list_all_sessions();
    Json(sessions).into_response()
}
