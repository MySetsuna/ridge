use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use portable_pty::PtySize;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, Query, State,
    },
    http::StatusCode,
    middleware::Next,
    response::{Html, IntoResponse},
    routing::{get, post},
    Form, Json, Router,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::{AppState, RemotePaneSub, RemoteSubId};

use super::auth::RemoteAuth;

#[derive(Clone)]
struct RemoteCtx {
    port: u16,
    lan_ip: String,
    machine_name: String,
    state: AppState,
    auth: Arc<RemoteAuth>,
    static_dir: PathBuf,
}

#[derive(Deserialize)]
struct ConnectQuery {
    code: Option<String>,
    token: Option<String>,
    /// Stable mobile-generated device id (localStorage UUID), used for the
    /// session list label and as the blacklist key.
    device: Option<String>,
}

#[derive(Deserialize)]
struct VerifyForm {
    code: String,
    /// Stable mobile-generated device id, for the blacklist check at verify time.
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
    otpauth_uri: String,
    ready: bool,
    machine_name: String,
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

/// Handle returned by `spawn_remote_server` — the caller receives the
/// allocated port and the background thread join handle.
pub struct ServerHandle {
    pub port: u16,
    pub thread: std::thread::JoinHandle<()>,
}

/// Spawn the remote-control WebSocket server on a background thread.
/// Listens on `0.0.0.0:0` (OS-assigned port).
///
/// Accepts a `shutdown_rx` one-shot receiver: when a value is sent on
/// the corresponding sender the server performs an orderly graceful
/// shutdown (drain in-flight requests, close listeners).
///
/// Returns `None` if binding failed.
pub fn spawn_remote_server(
    state: AppState,
    auth: Arc<RemoteAuth>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Option<ServerHandle> {
    let lan_ip = super::detect_lan_ip();

    let (port_tx, port_rx) = std::sync::mpsc::channel();

    let thread = std::thread::Builder::new()
        .name("ridge-remote-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(target: "ridge::remote", error = %e, "tokio runtime build failed");
                    let _ = port_tx.send(None);
                    return;
                }
            };
            rt.block_on(run_remote_server(
                state,
                auth,
                lan_ip,
                port_tx,
                shutdown_rx,
            ));
        })
        .expect("ridge-remote-http thread spawn");

    let port = port_rx.recv().ok().flatten()?;
    Some(ServerHandle { port, thread })
}

async fn run_remote_server(
    state: AppState,
    auth: Arc<RemoteAuth>,
    lan_ip: String,
    port_tx: std::sync::mpsc::Sender<Option<u16>>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) {
    // Fixed port 9527; if occupied, try 9528, 9529, … up to 10 attempts.
    // Bind a std listener up front: axum-server's `from_tcp_rustls` adopts a
    // std `TcpListener`, and the plain-HTTP fallback re-wraps it via
    // `tokio::net::TcpListener::from_std`. Either way we keep the port-probe.
    let base_port: u16 = 9527;
    let mut port = base_port;
    let std_listener = loop {
        let addr = format!("0.0.0.0:{}", port);
        match std::net::TcpListener::bind(&addr) {
            Ok(l) => break l,
            Err(_) if port < base_port + 10 => {
                port += 1;
                continue;
            }
            Err(e) => {
                tracing::error!(target: "ridge::remote", error = %e, port = base_port, "remote server bind failed (tried {}+10)", base_port);
                let _ = port_tx.send(None);
                return;
            }
        }
    };
    if let Err(e) = std_listener.set_nonblocking(true) {
        tracing::error!(target: "ridge::remote", error = %e, "remote server: set_nonblocking failed");
        let _ = port_tx.send(None);
        return;
    }
    let port = std_listener.local_addr().map(|a| a.port()).unwrap_or(port);
    tracing::info!(target: "ridge::remote", port, lan_ip = %lan_ip, "Remote control server listening");

    // Resolve the static files directory. The remote UI is built by
    // `pnpm build:remote` (vite.remote.config.js) into `static/remote/`, and
    // tauri.conf.json bundles `../static/remote` → `static/remote` next to the
    // exe. Try in order:
    // 1. CWD/static/remote — works in dev (`cargo tauri dev`) when CWD is project root.
    // 2. exe_dir/static/remote — works in production (NSIS install copies resources next to exe).
    // 3. exe_dir/../../../static/remote — works when running the exe directly from
    //    target/release/ (parent→target→src-tauri→project-root/static/remote).
    let static_dir = {
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("static").join("remote"),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("static").join("remote")))
                .unwrap_or_default(),
            std::env::current_exe()
                .ok()
                .and_then(|p| {
                    p.parent()?
                        .parent()?
                        .parent()?
                        .parent()?
                        .join("static")
                        .join("remote")
                        .into()
                })
                .unwrap_or_default(),
        ];
        candidates
            .into_iter()
            .find(|p| p.join("index.html").exists())
            .unwrap_or_else(|| PathBuf::from("static").join("remote"))
    };

    let machine_name = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());
    // Captured for the self-signed cert SANs (lan_ip + machine_name are moved
    // into `ctx` below).
    let tls_lan_ip = lan_ip.clone();
    let tls_hostname = machine_name.clone();

    let ctx = RemoteCtx {
        port,
        lan_ip,
        machine_name,
        state,
        auth,
        static_dir,
    };

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/ws", get(ws_handler))
        .route("/assets/*path", get(assets_handler))
        .route("/health", get(health_handler))
        .route("/info", get(info_handler))
        .route("/status", get(status_handler))
        .route("/verify", get(verify_handler_get).post(verify_handler_post))
        .route("/session", get(session_handler))
        .route("/workspace/list", get(workspace_list_handler))
        .route("/workspace/switch", post(workspace_switch_handler))
        .route("/workspace/create", post(workspace_create_handler))
        .route("/workspace/close", post(workspace_close_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            remote_gate,
        ))
        .with_state(ctx);

    let _ = port_tx.send(Some(port));
    // §sessions: serve with peer-address connect info so the WS handler can
    // capture each client's real IP (for the session list + blacklist).
    let make_svc = app.into_make_service_with_connect_info::<SocketAddr>();

    // Prefer HTTPS: browsers only expose WebGPU in a secure context, so the
    // LAN page must be served over TLS to unlock the GPU render path. A
    // self-signed cert is auto-generated on first run (see remote/tls.rs).
    // If TLS material can't be produced we fall back to plain HTTP so the
    // server still comes up (WebGPU then stays disabled on remote browsers).
    match super::tls::resolve_config(&tls_lan_ip, &tls_hostname).await {
        Some(tls_config) => {
            tracing::info!(target: "ridge::remote", "Remote server serving HTTPS (TLS)");
            let handle = axum_server::Handle::new();
            let shutdown_handle = handle.clone();
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                shutdown_handle.graceful_shutdown(Some(Duration::from_secs(3)));
            });
            if let Err(e) = axum_server::from_tcp_rustls(std_listener, tls_config)
                .handle(handle)
                .serve(make_svc)
                .await
            {
                tracing::error!(target: "ridge::remote", error = %e, "remote HTTPS server stopped");
            }
        }
        None => {
            tracing::warn!(target: "ridge::remote", "Remote TLS unavailable — serving plain HTTP (browser WebGPU disabled)");
            match tokio::net::TcpListener::from_std(std_listener) {
                Ok(listener) => {
                    let shutdown_signal = shutdown_rx.map(|_| ());
                    if let Err(e) = axum::serve(listener, make_svc)
                        .with_graceful_shutdown(shutdown_signal)
                        .await
                    {
                        tracing::error!(target: "ridge::remote", error = %e, "remote server stopped");
                    }
                }
                Err(e) => {
                    tracing::error!(target: "ridge::remote", error = %e, "remote server: failed to adopt listener for HTTP fallback");
                }
            }
        }
    }
}

// ── Middleware ───────────────────────────────────────────────────────────────

/// Gate all routes behind `remote_enabled`. When the toggle is off every
/// request (including WebSocket upgrades) gets a 503 so no part of the
/// remote surface remains usable.
async fn remote_gate(
    State(ctx): State<RemoteCtx>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    if !ctx.state.remote_enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Remote control disabled").into_response();
    }
    next.run(req).await
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn root_handler(State(ctx): State<RemoteCtx>) -> impl IntoResponse {
    let index_path = ctx.static_dir.join("index.html");
    match tokio::fs::read_to_string(&index_path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => {
            // Fallback: embed a basic page directing the user to build the remote app
            Html(format!(
                r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Ridge Remote</title></head><body style="background:#0d1117;color:#e6edf3;font-family:sans-serif;display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;margin:0"><h1>Ridge Remote</h1><p>Remote UI not built yet.</p><p>Run: <code>pnpm build:remote</code></p></body></html>"#,
            ))
            .into_response()
        }
    }
}

/// Serve static assets (JS, CSS, WASM) from the built mobile output directory.
async fn assets_handler(
    State(ctx): State<RemoteCtx>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    let file_path = ctx.static_dir.join("assets").join(&path);
    match tokio::fs::read(&file_path).await {
        Ok(bytes) => {
            let (content_type, cache_control) = if path.ends_with(".js") {
                ("application/javascript", "max-age=31536000, immutable")
            } else if path.ends_with(".css") {
                ("text/css", "max-age=31536000, immutable")
            } else if path.ends_with(".wasm") {
                ("application/wasm", "max-age=86400")
            } else if path.ends_with(".svg") {
                ("image/svg+xml", "max-age=86400")
            } else if path.ends_with(".png") {
                ("image/png", "max-age=86400")
            } else if path.ends_with(".woff2") {
                ("font/woff2", "max-age=31536000, immutable")
            } else {
                ("application/octet-stream", "max-age=3600")
            };
            let response = axum::response::Response::builder()
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .header(axum::http::header::CACHE_CONTROL, cache_control)
                .body(axum::body::Body::from(bytes))
                .unwrap();
            response
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn info_handler(State(ctx): State<RemoteCtx>) -> impl IntoResponse {
    let enabled = ctx.state.remote_enabled.load(Ordering::Relaxed);
    let uri = ctx.auth.otpauth_uri(&ctx.machine_name);
    Json(InfoResponse {
        port: ctx.port,
        lan_ip: ctx.lan_ip.clone(),
        otpauth_uri: uri,
        ready: enabled,
        machine_name: ctx.machine_name.clone(),
    })
}

async fn status_handler(State(ctx): State<RemoteCtx>) -> Json<StatusResponse> {
    Json(StatusResponse {
        port: ctx.port,
        ready: true,
    })
}

/// GET /verify serves the mobile Svelte app (same as /).
async fn verify_handler_get(State(ctx): State<RemoteCtx>) -> impl IntoResponse {
    root_handler(State(ctx)).await
}

async fn verify_handler_post(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<RemoteCtx>,
    Form(form): Form<VerifyForm>,
) -> Json<VerifyResponse> {
    // §blacklist: a barred device/IP can't even obtain a token.
    let device_id = form.device.clone().unwrap_or_default();
    if ctx
        .state
        .remote_blacklist
        .is_blocked(&device_id, &addr.ip().to_string())
    {
        return Json(VerifyResponse {
            success: false,
            message: "该设备已被加入黑名单".to_string(),
            token: None,
        });
    }
    let valid = ctx.auth.verify(&form.code);
    let token = if valid {
        Some(ctx.state.remote_session_store.create_session())
    } else {
        None
    };
    Json(VerifyResponse {
        success: valid,
        message: if valid {
            "Verification successful".to_string()
        } else {
            "Invalid TOTP code".to_string()
        },
        token,
    })
}

/// WebSocket upgrade handler. Accepts `?code=XXXXXX` (TOTP) or `?token=XXX` (session token).
/// Returns 401 if authentication fails, 503 if remote control is globally disabled.
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<ConnectQuery>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(ctx): State<RemoteCtx>,
) -> impl IntoResponse {
    if !ctx.state.remote_enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "remote control disabled").into_response();
    }
    let remote_addr = addr.ip().to_string();
    let device_id = query.device.clone().unwrap_or_default();
    // §blacklist: bar barred devices/IPs even with a valid token.
    if ctx
        .state
        .remote_blacklist
        .is_blocked(&device_id, &remote_addr)
    {
        return (StatusCode::FORBIDDEN, "device is blacklisted").into_response();
    }
    let valid = if let Some(ref t) = query.token {
        ctx.state.remote_session_store.validate_token(t)
    } else if let Some(ref c) = query.code {
        ctx.auth.verify(c)
    } else {
        false
    };
    if !valid {
        return (StatusCode::UNAUTHORIZED, "invalid authentication").into_response();
    }
    let token = query.token.clone();
    ws.on_upgrade(move |socket| handle_ws(socket, ctx, remote_addr, device_id, token))
        .into_response()
}

async fn health_handler() -> &'static str {
    "ok"
}

/// Check if a session token is still valid.
async fn session_handler(
    Query(query): Query<SessionQuery>,
    State(ctx): State<RemoteCtx>,
) -> impl IntoResponse {
    let valid = ctx.state.remote_session_store.validate_token(&query.token);
    Json(serde_json::json!({ "valid": valid }))
}

// ── Workspace HTTP handlers ─────────────────────────────────────────────

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

async fn workspace_list_handler(State(ctx): State<RemoteCtx>) -> impl IntoResponse {
    let order = ctx.state.workspace_order.read();
    let names = ctx.state.workspace_names.read();
    let map = ctx.state.workspaces.read();
    let active = *ctx.state.active_workspace.read();
    let workspaces: Vec<serde_json::Value> = order
        .iter()
        .map(|id| {
            // §unify: per-workspace display_seq fallback name, matching the desktop
            // and the WS `list-workspaces` handler.
            let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
            serde_json::json!({
                "id": id.to_string(),
                "name": names.get(id).cloned().unwrap_or_else(|| format!("工作区 {}", display_seq)),
                "displaySeq": display_seq,
                "active": *id == active,
            })
        })
        .collect();
    Json(serde_json::json!({ "workspaces": workspaces }))
}

async fn workspace_switch_handler(
    State(ctx): State<RemoteCtx>,
    Json(body): Json<WorkspaceSwitchBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = match Uuid::parse_str(&body.workspace_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success":false,"error":"invalid workspace id"})),
            )
        }
    };
    let exists = ctx.state.workspaces.read().contains_key(&id);
    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"success":false,"error":"workspace not found"})),
        );
    }
    *ctx.state.active_workspace.write() = id;
    (
        StatusCode::OK,
        Json(serde_json::json!({ "success": true, "workspaceId": id.to_string() })),
    )
}

async fn workspace_create_handler(
    State(ctx): State<RemoteCtx>,
    Json(body): Json<WorkspaceCreateBody>,
) -> impl IntoResponse {
    use std::collections::HashMap;
    let id = Uuid::new_v4();
    let seq = ctx.state.allocate_workspace_seq();
    {
        let mut map = ctx.state.workspaces.write();
        map.insert(
            id,
            crate::state::Workspace {
                pane_tree: crate::engine::pane_tree::PaneTree::new(),
                terminals: HashMap::new(),
                teammate_tmux_pane_cursor: 0,
                teammate_pane_titles: HashMap::new(),
                pane_sizes: HashMap::new(),
                last_pane_index: None,
                created_at: std::time::SystemTime::now(),
                teammate_pane_states: HashMap::new(),
                teammate_agent_pane_map: HashMap::new(),
                associated_file_path: None,
                pending_spawns: HashMap::new(),
                teammate_metrics: crate::state::TeammateMetrics::default(),
                display_seq: seq,
            },
        );
    }
    ctx.state.workspace_order.write().push(id);
    *ctx.state.active_workspace.write() = id;
    if let Some(name) = body.name.filter(|n| !n.is_empty()) {
        ctx.state.workspace_names.write().insert(id, name);
    }
    Json(serde_json::json!({ "success": true, "workspaceId": id.to_string() }))
}

async fn workspace_close_handler(
    State(ctx): State<RemoteCtx>,
    Json(body): Json<WorkspaceCloseBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = match Uuid::parse_str(&body.workspace_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success":false,"error":"invalid workspace id"})),
            )
        }
    };
    {
        let order = ctx.state.workspace_order.read();
        if order.len() <= 1 {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"success":false,"error":"cannot close last workspace"})),
            );
        }
    }
    ctx.state.workspaces.write().remove(&id);
    ctx.state.workspace_order.write().retain(|w| *w != id);
    ctx.state.workspace_names.write().remove(&id);
    // If we closed the active workspace, switch to the first remaining.
    if *ctx.state.active_workspace.read() == id {
        let first = ctx.state.workspace_order.read().first().cloned();
        if let Some(first_id) = first {
            *ctx.state.active_workspace.write() = first_id;
        }
    }
    (StatusCode::OK, Json(serde_json::json!({ "success": true })))
}

/// Resize a pane's PTY and canonical parser, broadcast the resulting
/// delta frame to the desktop (via the pane delta channel), and send
/// a PtyResized event to every remote subscriber so they can resize
/// their own wasm kernel. This is the shared path for the remote
/// "refresh-pane" / "claim-pane" commands.
fn apply_pane_resize(
    ctx: &RemoteCtx,
    ws_id: Uuid,
    pane_id: Uuid,
    rows: u16,
    cols: u16,
    pixel_width: u16,
    pixel_height: u16,
) {
    let rows = rows.max(1).min(500);
    let cols = cols.max(1).min(500);
    let frame_bytes = {
        let map = ctx.state.workspaces.read();
        let Some(ws) = map.get(&ws_id) else { return };
        let Some(handle) = ws.terminals.get(&pane_id) else {
            return;
        };
        let _ = handle.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
        });
        handle.delta_mode.store(true, Ordering::Release);
        let frame = {
            let mut p = handle.parser.lock();
            p.resize(rows, cols)
        };
        ridge_term::term::delta::encode_frame(&frame).ok()
    };
    {
        let mut map = ctx.state.workspaces.write();
        if let Some(ws) = map.get_mut(&ws_id) {
            ws.pane_sizes.insert(pane_id, (rows, cols));
        }
    }
    let Some(bytes) = frame_bytes else { return };
    // Desktop viewer (if attached via a delta channel).
    if let Some(sender) = ctx.state.get_pane_delta_channel(ws_id, pane_id) {
        sender(bytes.clone());
    }
    // All remote viewers receive a PtyResized event so their wasm
    // kernel can call kernel.resize() for reflow.
    ctx.state.broadcast_remote_event(
        ws_id,
        pane_id,
        crate::types::RemotePtyEvent::PtyResized {
            workspace_id: ws_id,
            pane_id,
            rows,
            cols,
        },
    );
}

async fn handle_ws(
    socket: WebSocket,
    ctx: RemoteCtx,
    remote_addr: String,
    device_id: String,
    token: Option<String>,
) {
    use futures::{SinkExt, StreamExt};
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Register this client in the remote client registry so the desktop
    // RemotePanel can list, disconnect, or blacklist it.
    let (client_id, kill_flag) = ctx.state.remote_client_registry.register(
        remote_addr,
        String::new(), // user-agent not available from axum WS directly
        device_id,
        token,
    );
    tracing::info!(target: "ridge::remote", client_id, "WebSocket client connected");

    // Per-client mpsc channel — isolated from other WS clients.
    let (raw_tx, mut raw_rx) = mpsc::channel::<crate::types::RemotePtyEvent>(512);
    let sub_id = RemoteSubId::next();

    // Shared with the active `RemotePaneSub`: the PTY fan-out (lib.rs) sets this
    // when it has to drop a frame because `raw_tx` is full. The WS task clears it
    // and re-syncs the client on the next forwarded frame.
    let desync = Arc::new(AtomicBool::new(false));
    // §resync-throttle: a resync replays up to 64 KiB of scrollback, so under a
    // sustained-overload feedback loop (slow client → drops → resync → slower)
    // we cap it to at most once per interval. The desync flag is only CONSUMED
    // when we actually resync — if throttled, it stays set so a later frame
    // (after the interval) performs the recovery instead of losing the signal.
    let mut last_resync: Option<Instant> = None;
    const RESYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

    // §rate-limit: per-connection token bucket for `data-request`. An
    // authenticated remote already has shell access, so this is an anti-abuse
    // / anti-DoS guard (scripted bulk FS/git calls), not an authz boundary.
    let mut dr_window_start = Instant::now();
    let mut dr_count: u32 = 0;
    const DR_WINDOW: Duration = Duration::from_secs(5);
    const DR_MAX_PER_WINDOW: u32 = 120;

    // Track which (ws, pane) this client is currently subscribed to.
    let mut current_pane: Option<(Uuid, Uuid)> = None;

    // Client-reported viewport grid dimensions, updated by the `resize` WS
    // message. Used for the first-connect auto-claim and the refresh button.
    let mut mobile_rows: u16 = 24;
    let mut mobile_cols: u16 = 80;

    // Subscribe to structural change broadcasts (pane/workspace add/close/rename)
    // so this client can push updated lists to the remote frontend without polling.
    let mut structural_rx = ctx.state.remote_structural_tx.subscribe();
    // §own-active: there is NO connect-time auto-claim. A remote endpoint resizes
    // the shared PTY only when it becomes the active owner — i.e. on a genuine
    // user interaction (`claim-pane`) or the explicit refresh button
    // (`refresh-pane`). Merely connecting / changing viewport records the size
    // here but never stomps the PTY the desktop is using.

    // Initial handshake.
    let welcome = serde_json::json!({"type": "hello","version": 1,"protocol": "ridge-remote-ws"});
    if ws_tx
        .send(Message::Text(welcome.to_string()))
        .await
        .is_err()
    {
        ctx.state.remote_client_registry.unregister(client_id);
        return;
    }

    // §theme: push the desktop's active theme so the remote chrome and the
    // terminal kernel follow it (passive — a snapshot taken at connect). Best
    // effort: on any failure the client keeps its own CSS-variable fallbacks.
    if let Some(entry) = crate::commands::theme::active_theme_entry_no_handle() {
        let theme_msg = serde_json::json!({
            "type": "theme",
            "themeType": entry.theme_type,
            "colors": entry.colors,
        });
        let _ = ws_tx.send(Message::Text(theme_msg.to_string())).await;
    }

    // §state-sep: per-client active workspace. Seeded once from the global
    // active workspace at connect, then owned by THIS client. Switching /
    // creating / closing workspaces from a remote no longer rewrites the
    // shared `active_workspace` (which would drag the desktop and every other
    // client along) — only this `active_ws_id` moves. All readers below
    // (list-panes / subscribe / stdin / resize / output+delta filters) use it.
    let mut active_ws_id = ctx.state.active_workspace_id();

    // Periodic health check: if remote control is toggled off, or this client
    // is force-disconnected / blacklisted (kill_flag), close the WS so the
    // mobile client gets a clean disconnect. Polled at 1s so an admin-triggered
    // disconnect takes effect promptly (just an atomic load per tick).
    let mut health_interval = tokio::time::interval(std::time::Duration::from_secs(1));
    health_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Message loop: forward PTY output to WS client, relay keystrokes back.
    loop {
        tokio::select! {
            msg = ws_rx.next() => {
                let Some(Ok(Message::Text(text))) = msg else {
                    break;
                };
                let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                let _ = match parsed["type"].as_str() {
                    Some("ping") => {
                        ws_tx.send(Message::Text(serde_json::json!({"type":"pong"}).to_string())).await
                    }
                    Some("list-panes") => {
                        let mut pane_list = {
                            let workspaces = ctx.state.workspaces.read();
                            let Some(ws) = workspaces.get(&active_ws_id) else {
                                drop(workspaces);
                                continue;
                            };
                            let mut list = Vec::new();
                            for (pane_id, handle) in &ws.terminals {
                                // §6: prefer the live OSC window title (matches the
                                // desktop pane header's variable title), then the
                                // teammate-assigned name, then the shell kind.
                                let osc_title = handle.parser.lock().title()
                                    .filter(|t| !t.trim().is_empty());
                                let title = osc_title
                                    .or_else(|| ws.teammate_pane_titles.get(pane_id).cloned())
                                    .or_else(|| ws.pane_tree.panes.get(pane_id).and_then(|n| n.shell_kind.clone()))
                                    .unwrap_or_else(|| "terminal".to_string());
                                list.push(serde_json::json!({
                                    "id": pane_id.to_string(),
                                    "title": title,
                                    "cwd": ws.pane_tree.panes.get(pane_id)
                                        .and_then(|n| n.cwd.as_ref().map(|p| p.to_string_lossy().to_string()))
                                        .unwrap_or_default(),
                                }));
                            }
                            for (pane_id, _) in &ws.pending_spawns {
                                list.push(serde_json::json!({
                                    "id": pane_id.to_string(),
                                    "title": "pending...",
                                    "cwd": "",
                                }));
                            }
                            list
                        };
                        pane_list.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
                        ws_tx.send(Message::Text(serde_json::json!({"type":"panes","panes":pane_list}).to_string())).await
                    }
                    Some("subscribe-pane") => {
                        let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                            // Unregister from current pane.
                            if let Some((ws, p)) = current_pane.take() {
                                ctx.state.unregister_remote_sub(ws, p, sub_id);
                            }
                            let new_key = (active_ws_id, pane_id);

                            // Ensure the canonical parser is in delta mode so
                            // the desktop frontend continues receiving deltas.
                            {
                                let workspaces = ctx.state.workspaces.read();
                                if let Some(h) = workspaces
                                    .get(&active_ws_id)
                                    .and_then(|ws| ws.terminals.get(&pane_id))
                                {
                                    h.delta_mode.store(true, Ordering::Release);
                                }
                            }

                            // Fresh subscription starts in-sync.
                            desync.store(false, Ordering::Release);
                            ctx.state.register_remote_sub(
                                active_ws_id, pane_id,
                                RemotePaneSub {
                                    id: sub_id,
                                    raw_tx: raw_tx.clone(),
                                    desync: desync.clone(),
                                },
                            );
                            current_pane = Some(new_key);

                            // Send recent scrollback as raw bytes so the client
                            // kernel can replay history via feed().
                            //
                            // Ordering note: we register BEFORE snapshotting the
                            // scrollback on purpose. This guarantees no GAP — every
                            // chunk is either in this snapshot or delivered live (or,
                            // in a sub-microsecond window, both → a harmless duplicate
                            // that a vte repaint absorbs). The reverse order would
                            // trade the benign dup for a dropped chunk, which is worse
                            // for a mirror. True dedup would require coupling the PTY
                            // reader's scrollback-append + fan-out under one lock — not
                            // worth the hot-path cost.
                            let history = ctx.state.get_recent_scrollback_for(
                                active_ws_id, pane_id, 65536,
                            );
                            if !history.is_empty() {
                                let mut payload =
                                    Vec::with_capacity(16 + history.len());
                                payload.extend_from_slice(pane_id.as_bytes());
                                payload.extend_from_slice(&history);
                                let _ =
                                    ws_tx.send(Message::Binary(payload.into())).await;
                            }
                        }
                        Ok(())
                    }
                    Some("current-project") => {
                        let path = ctx.state.current_project.read().clone()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ws_tx.send(Message::Text(serde_json::json!({
                            "type": "current-project",
                            "path": path,
                        }).to_string())).await
                    }
                    Some("list-workspaces") => {
                        let ws_list = {
                            let order = ctx.state.workspace_order.read();
                            let names = ctx.state.workspace_names.read();
                            let map = ctx.state.workspaces.read();
                            // §state-sep: the `active` flag reflects THIS client's
                            // selection, not the global one.
                            let active = active_ws_id;
                            order.iter().map(|id| {
                                // §unify: unnamed workspaces fall back to "工作区 {display_seq}",
                                // matching the desktop WorkspaceSidebar label.
                                let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
                                serde_json::json!({
                                    "id": id.to_string(),
                                    "name": names.get(id).cloned()
                                        .unwrap_or_else(|| format!("工作区 {}", display_seq)),
                                    "displaySeq": display_seq,
                                    "active": *id == active,
                                })
                            }).collect::<Vec<_>>()
                        };
                        ws_tx.send(Message::Text(serde_json::json!({
                            "type": "workspaces",
                            "workspaces": ws_list,
                        }).to_string())).await
                    }
                    Some("switch-workspace") => {
                        let id_str = parsed["workspaceId"].as_str().unwrap_or("");
                        let result = if let Ok(id) = Uuid::parse_str(id_str) {
                            let exists = ctx.state.workspaces.read().contains_key(&id);
                            if exists {
                                // §state-sep: switch THIS client only — do not touch
                                // the global active_workspace. Drop the current pane
                                // subscription so the old workspace's stream stops; the
                                // client re-subscribes after its next list-panes.
                                if let Some((ws, p)) = current_pane.take() {
                                    ctx.state.unregister_remote_sub(ws, p, sub_id);
                                }
                                active_ws_id = id;
                                serde_json::json!({ "type": "switch-workspace-result", "success": true, "workspaceId": id.to_string() })
                            } else {
                                serde_json::json!({ "type": "switch-workspace-result", "success": false, "error": "workspace not found" })
                            }
                        } else {
                            serde_json::json!({ "type": "switch-workspace-result", "success": false, "error": "invalid workspace id" })
                        };
                        ws_tx.send(Message::Text(result.to_string())).await
                    }
                    Some("create-workspace") => {
                        let name = parsed["name"].as_str().and_then(|n| if n.is_empty() { None } else { Some(n.to_string()) });
                        let id = Uuid::new_v4();
                        let seq = ctx.state.allocate_workspace_seq();
                        {
                            use std::collections::HashMap;
                            let mut map = ctx.state.workspaces.write();
                            map.insert(id, crate::state::Workspace {
                                pane_tree: crate::engine::pane_tree::PaneTree::new(),
                                terminals: HashMap::new(),
                                teammate_tmux_pane_cursor: 0,
                                teammate_pane_titles: HashMap::new(),
                                pane_sizes: HashMap::new(),
                                last_pane_index: None,
                                created_at: std::time::SystemTime::now(),
                                teammate_pane_states: HashMap::new(),
                                teammate_agent_pane_map: HashMap::new(),
                                associated_file_path: None,
                                pending_spawns: HashMap::new(),
                                teammate_metrics: crate::state::TeammateMetrics::default(),
                                display_seq: seq,
                            });
                        }
                        ctx.state.workspace_order.write().push(id);
                        // §state-sep: the new workspace is shared data (visible to all),
                        // but only THIS client jumps to it. Other clients / the desktop
                        // stay on their own selection.
                        if let Some((ws, p)) = current_pane.take() {
                            ctx.state.unregister_remote_sub(ws, p, sub_id);
                        }
                        active_ws_id = id;
                        if let Some(ref n) = name {
                            ctx.state.workspace_names.write().insert(id, n.clone());
                        }
                        let send = ws_tx.send(Message::Text(serde_json::json!({
                            "type": "create-workspace-result",
                            "success": true,
                            "workspaceId": id.to_string(),
                        }).to_string())).await;
                        // Broadcast structural change to all remote clients and desktop.
                        let _ = ctx.state.remote_structural_tx.send(
                            crate::types::RemoteStructuralEvent::WorkspacesChanged
                        );
                        let _ = ctx.state.event_tx.try_send(
                            crate::types::GlobalEvent::WorkspaceListChanged
                        );
                        send
                    }
                    Some("close-workspace") => {
                        let id_str = parsed["workspaceId"].as_str().unwrap_or("");
                        let mut success = false;
                        let result = if let Ok(id) = Uuid::parse_str(id_str) {
                            {
                                let order = ctx.state.workspace_order.read();
                                if order.len() <= 1 {
                                    serde_json::json!({ "type": "close-workspace-result", "success": false, "error": "cannot close last workspace" })
                                } else {
                                    drop(order);
                                    ctx.state.workspaces.write().remove(&id);
                                    ctx.state.workspace_order.write().retain(|w| *w != id);
                                    ctx.state.workspace_names.write().remove(&id);
                                    // Closing a workspace destroys shared data: if the
                                    // DESKTOP (global active) was viewing it, move the
                                    // global off so the desktop doesn't point at a dead
                                    // workspace. This is unavoidable — you can't view a
                                    // workspace that no longer exists.
                                    if *ctx.state.active_workspace.read() == id {
                                        let first = ctx.state.workspace_order.read().first().cloned();
                                        if let Some(first_id) = first {
                                            *ctx.state.active_workspace.write() = first_id;
                                        }
                                    }
                                    // §state-sep: if THIS client was on the closed
                                    // workspace, fall back to the first remaining one
                                    // (independently of the desktop / other clients).
                                    if active_ws_id == id {
                                        if let Some((ws, p)) = current_pane.take() {
                                            ctx.state.unregister_remote_sub(ws, p, sub_id);
                                        }
                                        if let Some(first_id) = ctx.state.workspace_order.read().first().cloned() {
                                            active_ws_id = first_id;
                                        }
                                    }
                                    success = true;
                                    serde_json::json!({ "type": "close-workspace-result", "success": true })
                                }
                            }
                        } else {
                            serde_json::json!({ "type": "close-workspace-result", "success": false, "error": "invalid workspace id" })
                        };
                        let send = ws_tx.send(Message::Text(result.to_string())).await;
                        if success {
                            let _ = ctx.state.remote_structural_tx.send(
                                crate::types::RemoteStructuralEvent::WorkspacesChanged
                            );
                            let _ = ctx.state.event_tx.try_send(
                                crate::types::GlobalEvent::WorkspaceListChanged
                            );
                        }
                        send
                    }
                    Some("stdin") => {
                        let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        let data_str = parsed["data"].as_str().unwrap_or("");
                        if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                            let workspaces = ctx.state.workspaces.read();
                            if let Some(ws) = workspaces.get(&active_ws_id) {
                                if let Some(handle) = ws.terminals.get(&pane_id) {
                                    let mut writer = handle.writer.lock();
                                    let _ = writer.write_all(data_str.as_bytes());
                                    let _ = writer.flush();
                                }
                            }
                        }
                        // no response needed
                        Ok(())
                    }
                    Some("resize") => {
                        // The client renders at the canonical PTY grid (driven by
                        // `pty-resized` from claim/refresh), so a viewport-only resize
                        // doesn't touch the shared PTY or the client kernel. We just
                        // record the clamped size as the fallback used by the next
                        // claim/refresh. The `.min(500)` is a defensive bound against a
                        // malformed viewport, not the anti-OOM guard it was when each
                        // sub owned a `rows × cols` parser.
                        let _pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        let rows = parsed["rows"].as_u64().unwrap_or(mobile_rows as u64) as u16;
                        let cols = parsed["cols"].as_u64().unwrap_or(mobile_cols as u64) as u16;
                        mobile_rows = rows.max(1).min(500);
                        mobile_cols = cols.max(1).min(500);
                        Ok(())
                    }
                    // §own-active: this client becomes the active size owner. Both
                    // the implicit "I just interacted" claim (`claim-pane`) and the
                    // explicit refresh button (`refresh-pane`) resize the shared PTY +
                    // canonical parser to this client's viewport and broadcast a full
                    // repaint to every viewer (desktop included). Last interaction wins.
                    Some("refresh-pane") | Some("claim-pane") => {
                        let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        let rows = parsed["rows"].as_u64().unwrap_or(mobile_rows as u64) as u16;
                        let cols = parsed["cols"].as_u64().unwrap_or(mobile_cols as u64) as u16;
                        let pixel_width = parsed["pixelWidth"].as_u64().unwrap_or(cols as u64 * 8) as u16;
                        let pixel_height = parsed["pixelHeight"].as_u64().unwrap_or(rows as u64 * 16) as u16;
                        mobile_rows = rows;
                        mobile_cols = cols;
                        if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                            apply_pane_resize(&ctx, active_ws_id, pane_id, rows, cols, pixel_width, pixel_height);
                        }
                        Ok(())
                    }
Some("create-pane") => {
                        // §6: create a terminal in THIS client's active workspace
                        // using the balanced-split chooser, then immediately activate
                        // it (Phase 2) at this client's viewport size. Remote clients
                        // can't call the front-end `activate_pane_pty` Tauri command,
                        // so without this the pane would sit in `pending_spawns`
                        // forever ("pending..."). Once live it streams to every viewer
                        // via the PTY fan-out on subscribe.
                        let shell = parsed["shell"].as_str()
                            .and_then(|s| if s.is_empty() { None } else { Some(s.to_string()) });
                        let mut success = false;
                        let result = match crate::commands::pane::remote_create_pane(&ctx.state, active_ws_id, shell) {
                            Ok(new_id) => match crate::commands::terminal::activate_pane_pty_state(
                                &ctx.state, None, active_ws_id, new_id,
                                Some(mobile_rows), Some(mobile_cols),
                            ) {
                                Ok(()) => {
                                    success = true;
                                    serde_json::json!({
                                        "type": "create-pane-result", "success": true, "paneId": new_id.to_string()
                                    })
                                }
                                Err(e) => serde_json::json!({
                                    "type": "create-pane-result", "success": false, "error": e.to_string()
                                }),
                            },
                            Err(e) => serde_json::json!({
                                "type": "create-pane-result", "success": false, "error": e.to_string()
                            }),
                        };
                        let send = ws_tx.send(Message::Text(result.to_string())).await;
                        if success {
                            let _ = ctx.state.remote_structural_tx.send(
                                crate::types::RemoteStructuralEvent::PanesChanged { workspace_id: active_ws_id }
                            );
                            let _ = ctx.state.event_tx.try_send(
                                crate::types::GlobalEvent::PaneTreeChanged { workspace_id: active_ws_id }
                            );
                        }
                        send
                    }
                    Some("close-pane") => {
                        let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        let mut success = false;
                        let result = match Uuid::parse_str(pane_id_str) {
                            Ok(pane_id) => match crate::commands::pane::remote_close_pane(&ctx.state, active_ws_id, pane_id).await {
                                Ok(()) => {
                                    success = true;
                                    serde_json::json!({ "type": "close-pane-result", "success": true })
                                }
                                Err(e) => serde_json::json!({ "type": "close-pane-result", "success": false, "error": e.to_string() }),
                            },
                            Err(_) => serde_json::json!({ "type": "close-pane-result", "success": false, "error": "invalid pane id" }),
                        };
                        let send = ws_tx.send(Message::Text(result.to_string())).await;
                        if success {
                            let _ = ctx.state.remote_structural_tx.send(
                                crate::types::RemoteStructuralEvent::PanesChanged { workspace_id: active_ws_id }
                            );
                            let _ = ctx.state.event_tx.try_send(
                                crate::types::GlobalEvent::PaneTreeChanged { workspace_id: active_ws_id }
                            );
                        }
send
                    }
                    Some("list-files") => {
                        let path_str = parsed["path"].as_str().unwrap_or("").to_string();
                        // §unify: base dir follows the subscribed pane's cwd (same as the
                        // desktop file tree), falling back to the active project, then home.
                        let base_dir = current_pane
                            .and_then(|(ws_id, pane_id)| {
                                let map = ctx.state.workspaces.read();
                                map.get(&ws_id)
                                    .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
                                    .and_then(|n| n.cwd.clone())
                            })
                            .or_else(|| ctx.state.current_project.read().clone())
                            .or_else(dirs::home_dir)
                            .unwrap_or_else(|| PathBuf::from("."));
                        let result = tokio::task::spawn_blocking(move || {
                            // Empty/"/" → base dir; absolute → as-is; else relative to base.
                            let target = if path_str.is_empty() || path_str == "/" {
                                base_dir.clone()
                            } else {
                                let p = PathBuf::from(&path_str);
                                if p.is_absolute() { p } else { base_dir.join(&path_str) }
                            };
                            // §unify: reuse the desktop file-tree pager so gitignore marking,
                            // OS-junk filtering and dir-first sorting match the desktop UI.
                            let page = crate::fs::tree::FileTree::page_children(&target, 0, 5000)
                                .unwrap_or(crate::fs::tree::DirectoryPage {
                                    entries: Vec::new(),
                                    total: 0,
                                    offset: 0,
                                    has_more: false,
                                });
                            let parent = target.parent().map(|p| p.to_string_lossy().to_string());
                            serde_json::json!({
                                "type": "files",
                                "path": target.to_string_lossy().to_string(),
                                "parent": parent,
                                "entries": page.entries,
                            })
                        }).await;
                        match result {
                            Ok(msg) => ws_tx.send(Message::Text(msg.to_string())).await,
                            Err(e) => {
                                tracing::warn!(target: "ridge::remote", error = %e, "list-files blocking task failed");
                                Ok(())
                            }
                        }
                    }
                    Some("list-remote-clients") => {
                        let clients = ctx.state.remote_client_registry.list();
                        let list: Vec<serde_json::Value> = clients.iter().map(|c| {
                            let elapsed = c.connected_at.elapsed()
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            serde_json::json!({
                                "id": c.id,
                                "connectedAt": elapsed,
                                "remoteAddr": c.remote_addr,
                                "userAgent": c.user_agent,
                            })
                        }).collect();
                        ws_tx.send(Message::Text(serde_json::json!({
                            "type": "remote-clients",
                            "clients": list,
                        }).to_string())).await
                    }
                    Some("kick-remote-client") => {
                        let target_id = parsed["id"].as_u64().unwrap_or(0);
                        let kicked = ctx.state.remote_client_registry.kick(target_id);
                        ws_tx.send(Message::Text(serde_json::json!({
                            "type": "kick-remote-client-result",
                            "success": kicked,
                            "clientId": target_id,
                        }).to_string())).await
                    }
                    Some("list-git-status") => {
                        // §unify: same cwd resolution as list-files (subscribed pane → project → home).
                        let base_dir = current_pane
                            .and_then(|(ws_id, pane_id)| {
                                let map = ctx.state.workspaces.read();
                                map.get(&ws_id)
                                    .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
                                    .and_then(|n| n.cwd.clone())
                            })
                            .or_else(|| ctx.state.current_project.read().clone())
                            .or_else(dirs::home_dir)
                            .unwrap_or_else(|| PathBuf::from("."));
                        let result = tokio::task::spawn_blocking(move || {
                            // §unify: reuse the desktop git module so branch / commits / diff
                            // come from the exact same source as the desktop Git panel.
                            let info = crate::commands::git::git_info_for_path(&base_dir);
                            serde_json::json!({
                                "type": "git-status",
                                "isGitRepo": info.is_git_repo,
                                "currentBranch": info.current_branch,
                                "branches": info.branches,
                                "files": info.diff.files,
                                "commits": info.commits,
                            })
                        }).await;
                        match result {
                            Ok(msg) => ws_tx.send(Message::Text(msg.to_string())).await,
                            Err(e) => {
                                tracing::warn!(target: "ridge::remote", error = %e, "list-git-status blocking task failed");
                                Ok(())
                            }
                        }
                    }
                    Some("search-files") => {
                        let query = parsed["query"].as_str().unwrap_or("").to_string();
                        // §unify: search root follows the subscribed pane's cwd (same as desktop).
                        let root = current_pane
                            .and_then(|(ws_id, pane_id)| {
                                let map = ctx.state.workspaces.read();
                                map.get(&ws_id)
                                    .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
                                    .and_then(|n| n.cwd.clone())
                            })
                            .or_else(|| ctx.state.current_project.read().clone())
                            .or_else(dirs::home_dir)
                            .unwrap_or_else(|| PathBuf::from("."));
                        let results = if query.trim().is_empty() {
                            Vec::new()
                        } else {
                            // §unify: reuse the desktop text_search engine (gitignore-aware ripgrep walk).
                            crate::commands::project::text_search(
                                root.to_string_lossy().to_string(),
                                query.clone(),
                                None, None, None, Some(200), None, None,
                            )
                            .await
                            .unwrap_or_default()
                        };
                        ws_tx.send(Message::Text(serde_json::json!({
                            "type": "search-results",
                            "query": query,
                            "results": results,
                        }).to_string())).await
                    }
                    Some("data-request") => {
                        // Backs the remote `WsDataProvider` (src/lib/transport/ws.ts).
                        // An authenticated remote already has shell stdin, so this
                        // mirrors the desktop `TauriDataProvider` 1:1 within the SAME
                        // trust boundary. Guards layered on top: a per-connection rate
                        // limit (below), a read-only toggle + path-traversal rejection
                        // + audit log of mutations (in `dispatch_data_request`). The
                        // reply carries `_reqId` plus `_result` (ok) or `_error` (fail).
                        let req_id = parsed["_reqId"].as_u64().unwrap_or(0);
                        let method = parsed["method"].as_str().unwrap_or("").to_string();

                        // §rate-limit: refill the window, then count this request.
                        if dr_window_start.elapsed() >= DR_WINDOW {
                            dr_window_start = Instant::now();
                            dr_count = 0;
                        }
                        dr_count += 1;
                        if dr_count > DR_MAX_PER_WINDOW {
                            tracing::warn!(
                                target: "ridge::remote",
                                client_id, method = %method,
                                "data-request rate limit exceeded; rejecting"
                            );
                            let reply = serde_json::json!({
                                "_reqId": req_id,
                                "_error": "rate limited: too many data requests",
                            });
                            ws_tx.send(Message::Text(reply.to_string())).await
                        } else {
                            let mut reply =
                                dispatch_data_request(&method, &parsed, &ctx.state).await;
                            if let Some(obj) = reply.as_object_mut() {
                                obj.insert("_reqId".to_string(), serde_json::json!(req_id));
                            }
                            ws_tx.send(Message::Text(reply.to_string())).await
                        }
                    }
                    _ => {
                        ws_tx.send(Message::Text(serde_json::json!({"type":"error","message":"unknown message type"}).to_string())).await
                    }
                };
            }
            event = raw_rx.recv() => {
                match event {
                    Some(crate::types::RemotePtyEvent::RawBytes { workspace_id, pane_id, bytes }) => {
                        if workspace_id == active_ws_id {
                            // §resync: if the fan-out dropped frames for this sub, the
                            // client's vte stream has a hole that would corrupt every
                            // subsequent parse. Reset the terminal (RIS) and replay
                            // fresh scrollback before the current bytes so the parser
                            // re-synchronises — but throttle it (see RESYNC_MIN_INTERVAL)
                            // so a sustained-overload loop can't amplify congestion. We
                            // only CONSUME the desync flag when we actually resync; if
                            // throttled it stays set for a later frame to handle.
                            if desync.load(Ordering::Acquire) {
                                let now = Instant::now();
                                let throttled = last_resync
                                    .is_some_and(|t| now.duration_since(t) < RESYNC_MIN_INTERVAL);
                                if !throttled {
                                    desync.store(false, Ordering::Release);
                                    last_resync = Some(now);
                                    let history = ctx.state.get_recent_scrollback_for(
                                        workspace_id, pane_id, 65536,
                                    );
                                    let mut resync = Vec::with_capacity(18 + history.len());
                                    resync.extend_from_slice(pane_id.as_bytes());
                                    resync.extend_from_slice(b"\x1bc"); // RIS — full reset
                                    resync.extend_from_slice(&history);
                                    if ws_tx.send(Message::Binary(resync.into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            let mut payload = Vec::with_capacity(16 + bytes.len());
                            payload.extend_from_slice(pane_id.as_bytes());
                            payload.extend_from_slice(&bytes);
                            if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(crate::types::RemotePtyEvent::Metadata { workspace_id, pane_id, title, cwd }) => {
                        if workspace_id == active_ws_id {
                            let _ = ws_tx.send(Message::Text(serde_json::json!({
                                "type": "pty-meta",
                                "paneId": pane_id.to_string(),
                                "title": title,
                                "cwd": cwd,
                            }).to_string())).await;
                        }
                    }
                    Some(crate::types::RemotePtyEvent::PtyResized { workspace_id, pane_id, rows, cols }) => {
                        if workspace_id == active_ws_id {
                            let _ = ws_tx.send(Message::Text(serde_json::json!({
                                "type": "pty-resized",
                                "paneId": pane_id.to_string(),
                                "rows": rows,
                                "cols": cols,
                            }).to_string())).await;
                        }
                    }
                    None => break,
                }
            }
            structural = structural_rx.recv() => {
                match structural {
                    Ok(crate::types::RemoteStructuralEvent::PanesChanged { workspace_id }) => {
                        if workspace_id == active_ws_id {
                            // Re-enumerate panes for this workspace and push to client.
                            let pane_list = {
                                let workspaces = ctx.state.workspaces.read();
                                if let Some(ws) = workspaces.get(&active_ws_id) {
                                    let mut list = Vec::new();
                                    for (pane_id, handle) in &ws.terminals {
                                        let osc_title = handle.parser.lock().title()
                                            .filter(|t| !t.trim().is_empty());
                                        let title = osc_title
                                            .or_else(|| ws.teammate_pane_titles.get(pane_id).cloned())
                                            .or_else(|| ws.pane_tree.panes.get(pane_id).and_then(|n| n.shell_kind.clone()))
                                            .unwrap_or_else(|| "terminal".to_string());
                                        list.push(serde_json::json!({
                                            "id": pane_id.to_string(),
                                            "title": title,
                                            "cwd": ws.pane_tree.panes.get(pane_id)
                                                .and_then(|n| n.cwd.as_ref().map(|p| p.to_string_lossy().to_string()))
                                                .unwrap_or_default(),
                                        }));
                                    }
                                    for (pane_id, _) in &ws.pending_spawns {
                                        list.push(serde_json::json!({
                                            "id": pane_id.to_string(),
                                            "title": "pending...",
                                            "cwd": "",
                                        }));
                                    }
                                    list.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
                                    list
                                } else {
                                    Vec::new()
                                }
                            };
                            let _ = ws_tx.send(Message::Text(serde_json::json!({
                                "type": "panes", "panes": pane_list
                            }).to_string())).await;
                        }
                    }
                    Ok(crate::types::RemoteStructuralEvent::WorkspacesChanged) => {
                        // Push updated workspace list.
                        let ws_list = {
                            let order = ctx.state.workspace_order.read();
                            let names = ctx.state.workspace_names.read();
                            let map = ctx.state.workspaces.read();
                            let active = active_ws_id;
                            order.iter().map(|id| {
                                let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
                                serde_json::json!({
                                    "id": id.to_string(),
                                    "name": names.get(id).cloned()
                                        .unwrap_or_else(|| format!("工作区 {}", display_seq)),
                                    "displaySeq": display_seq,
                                    "active": *id == active,
                                })
                            }).collect::<Vec<_>>()
                        };
                        let _ = ws_tx.send(Message::Text(serde_json::json!({
                            "type": "workspaces", "workspaces": ws_list
                        }).to_string())).await;
                    }
                    Ok(crate::types::RemoteStructuralEvent::WorkspaceRenamed { workspace_id, name }) => {
                        let _ = ws_tx.send(Message::Text(serde_json::json!({
                            "type": "workspace-renamed",
                            "workspaceId": workspace_id.to_string(),
                            "name": name,
                        }).to_string())).await;
                    }
                    Err(_) => {
                        // Lagged — skip; the next request-response cycle will fix it.
                    }
                }
            }
            _ = health_interval.tick() => {
                if !ctx.state.remote_enabled.load(Ordering::Relaxed)
                    || kill_flag.load(Ordering::Relaxed)
                {
                    let reason = if kill_flag.load(Ordering::Relaxed) {
                        "Disconnected by admin"
                    } else {
                        "Remote control disabled"
                    };
                    let _ = ws_tx.send(Message::Close(Some(
                        axum::extract::ws::CloseFrame {
                            code: 1000,
                            reason: std::borrow::Cow::Borrowed(reason),
                        }
                    ))).await;
                    break;
                }
            }
        }
    }

    // Clean up: unregister from all subscribed panes.
    if let Some((ws, pane)) = current_pane.take() {
        ctx.state.unregister_remote_sub(ws, pane, sub_id);
    }
    ctx.state.remote_client_registry.unregister(client_id);

    tracing::info!(target: "ridge::remote", client_id, "WebSocket client disconnected");
}

/// Dispatches one remote `data-request` `method` to the same backend command
/// the desktop `TauriDataProvider` (src/lib/transport/tauri.ts) invokes, with
/// the same arguments, and returns `{"_result": ...}` on success or
/// `{"_error": ...}` on failure. The caller stamps `_reqId`.
///
/// Paths arrive absolute (identical to the desktop IPC contract) and are passed
/// through unchanged — desktop and remote therefore behave identically. Most
/// backing commands are `async` and offload their own blocking work; the two
/// shape-mismatched methods (`git_status`, `search_files`) delegate to the
/// dedicated mappers below.
/// Methods that mutate the filesystem or git repository state. Gated by the
/// read-only toggle and audit-logged.
fn is_mutating_method(method: &str) -> bool {
    matches!(
        method,
        "write_file"
            | "rename_path"
            | "delete_path"
            | "create_file"
            | "create_directory"
            | "copy_path"
            | "move_path"
            | "git_stage"
            | "git_unstage"
            | "git_commit"
            | "git_pull"
            | "git_push"
            | "git_sync"
            | "git_checkout"
            | "git_revert"
            | "git_cherry_pick"
            | "git_reset"
            | "git_create_tag"
            | "git_discard"
            | "git_clean_untracked"
    )
}

/// Rejects a path that contains a `..` component (post-split, both separators).
/// Absolute paths still pass — this only blocks traversal tricks, not the
/// already-trusted absolute-path contract shared with the desktop.
fn path_has_traversal(p: &str) -> bool {
    !p.is_empty() && p.split(['/', '\\']).any(|c| c == "..")
}

async fn dispatch_data_request(
    method: &str,
    params: &serde_json::Value,
    state: &AppState,
) -> serde_json::Value {
    use crate::commands::{git, project};

    // §read-only gate: defence-in-depth for view-only remote sessions. (An
    // authenticated remote already has shell stdin, so this is a convenience
    // guard, not an isolation boundary.)
    if is_mutating_method(method) {
        if state.remote_fs_readonly.load(Ordering::Relaxed) {
            tracing::warn!(
                target: "ridge::remote::fs", method,
                "rejected mutating data-request: remote is read-only"
            );
            return serde_json::json!({ "_error": "remote filesystem is read-only" });
        }
        // §audit: record every mutation so a trust-but-verify operator has a trail.
        tracing::info!(target: "ridge::remote::fs", method, "remote mutating data-request");
    }

    // §traversal guard: reject `..` in any path-bearing field before it reaches
    // the filesystem layer.
    for key in ["path", "from", "to", "repoRoot"] {
        if let Some(v) = params.get(key).and_then(|x| x.as_str()) {
            if path_has_traversal(v) {
                tracing::warn!(
                    target: "ridge::remote::fs", method, key,
                    "rejected data-request: path traversal"
                );
                return serde_json::json!({ "_error": "path traversal rejected" });
            }
        }
    }
    if let Some(arr) = params.get("paths").and_then(|x| x.as_array()) {
        if arr
            .iter()
            .filter_map(|x| x.as_str())
            .any(path_has_traversal)
        {
            return serde_json::json!({ "_error": "path traversal rejected" });
        }
    }

    // Field extractors — keep each arm to a single readable line.
    fn s(v: &serde_json::Value, k: &str) -> String {
        v[k].as_str().unwrap_or("").to_string()
    }
    fn usize_opt(v: &serde_json::Value, k: &str) -> Option<usize> {
        v[k].as_u64().map(|n| n as usize)
    }
    fn path_list(v: &serde_json::Value) -> Vec<String> {
        v["paths"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
    // Result envelopes.
    fn unit(r: Result<(), String>) -> serde_json::Value {
        match r {
            Ok(()) => serde_json::json!({ "_result": null }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }
    fn val<T: Serialize>(r: Result<T, String>) -> serde_json::Value {
        match r {
            Ok(v) => serde_json::json!({ "_result": v }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }

    match method {
        // ── Filesystem ──
        "get_file_tree" => {
            val(project::get_file_tree(s(params, "path"), usize_opt(params, "depth")).await)
        }
        "get_directory_children" => val(project::get_directory_children(
            s(params, "path"),
            usize_opt(params, "offset"),
            usize_opt(params, "limit"),
        )
        .await),
        "path_exists" => val(project::path_exists(s(params, "path")).await),
        "read_file" => val(project::read_file(s(params, "path"))),
        "write_file" => unit(project::write_file(s(params, "path"), s(params, "content")).await),
        "rename_path" => unit(project::rename_path(s(params, "from"), s(params, "to"))),
        "delete_path" => unit(project::delete_path(s(params, "path")).await),
        "create_file" => unit(project::create_file(s(params, "path"))),
        "create_directory" => unit(project::create_directory(s(params, "path"))),
        "copy_path" => unit(project::copy_path(s(params, "from"), s(params, "to"), None).await),
        "move_path" => unit(project::move_path(s(params, "from"), s(params, "to")).await),

        // ── Git ── (all async; offload internally)
        "git_status" => git_status_result(s(params, "repoRoot")).await,
        "git_stage" => unit(git::git_stage(s(params, "repoRoot"), path_list(params)).await),
        "git_unstage" => unit(git::git_unstage(s(params, "repoRoot"), path_list(params)).await),
        "git_commit" => unit(
            git::git_commit(
                s(params, "repoRoot"),
                s(params, "message"),
                params["amend"].as_bool(),
            )
            .await,
        ),
        "git_pull" => unit(git::git_pull(s(params, "repoRoot")).await),
        "git_push" => {
            unit(git::git_push(s(params, "repoRoot"), params["setUpstream"].as_bool()).await)
        }
        "git_sync" => unit(git::git_sync(s(params, "repoRoot")).await),
        "git_checkout" => unit(
            git::git_checkout(
                s(params, "repoRoot"),
                s(params, "branch"),
                params["create"].as_bool(),
                None,
            )
            .await,
        ),
        "git_revert" => unit(git::git_revert(s(params, "repoRoot"), s(params, "hash")).await),
        "git_cherry_pick" => {
            unit(git::git_cherry_pick(s(params, "repoRoot"), s(params, "hash")).await)
        }
        // Frontend sends { mode, commit }; git_reset takes (repo_root, hash, mode).
        "git_reset" => unit(
            git::git_reset(
                s(params, "repoRoot"),
                s(params, "commit"),
                s(params, "mode"),
            )
            .await,
        ),
        "git_create_tag" => unit(
            git::git_create_tag(
                s(params, "repoRoot"),
                s(params, "name"),
                None,
                params["message"].as_str().map(String::from),
            )
            .await,
        ),
        "git_discard" => unit(git::git_discard(s(params, "repoRoot"), path_list(params)).await),
        "git_clean_untracked" => {
            unit(git::git_clean_untracked(s(params, "repoRoot"), Vec::new()).await)
        }

        // ── Search ──
        "search_files" => search_files_result(state, s(params, "query"), s(params, "path")).await,

        other => serde_json::json!({ "_error": format!("unknown data-request method: {}", other) }),
    }
}

/// Maps `ScmRepoStatus` (+ recent commit log) into the frontend `GitStatusResult`
/// shape: `{ staged, unstaged, untracked, commits }`. Commits aren't part of
/// `ScmRepoStatus`, so they're pulled separately via `git_info_for_path` on a
/// blocking thread (it shells out to `git log`).
async fn git_status_result(repo_root: String) -> serde_json::Value {
    let scm = match crate::commands::git::get_scm_status(repo_root.clone()).await {
        Ok(status) => status,
        Err(e) => return serde_json::json!({ "_error": e }),
    };
    let commits = tokio::task::spawn_blocking(move || {
        crate::commands::git::git_info_for_path(std::path::Path::new(&repo_root)).commits
    })
    .await
    .map(|list| {
        list.into_iter()
            .map(|c| serde_json::json!({ "hash": c.hash, "msg": c.subject, "time": c.date }))
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();

    let map_files = |files: Vec<crate::commands::git::ScmFile>| {
        files
            .into_iter()
            .map(|f| serde_json::json!({ "name": f.path, "status": f.status }))
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "_result": {
            "staged": map_files(scm.staged),
            "unstaged": map_files(scm.changes),
            "untracked": scm.untracked.into_iter().map(|f| f.path).collect::<Vec<_>>(),
            "commits": commits,
        }
    })
}

/// Runs the desktop text-search engine and remaps `fs::search::SearchResult`
/// (`{ file, content }`) onto the frontend `SearchResult` (`{ path, snippet }`).
/// An empty `path` falls back to the active project, then the home dir.
async fn search_files_result(state: &AppState, query: String, path: String) -> serde_json::Value {
    if query.trim().is_empty() {
        return serde_json::json!({ "_result": [] });
    }
    let root = if path.trim().is_empty() {
        state
            .current_project
            .read()
            .clone()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    } else {
        path
    };
    match crate::commands::project::text_search(
        root,
        query,
        None,
        None,
        None,
        Some(500),
        None,
        None,
    )
    .await
    {
        Ok(results) => {
            let mapped = results
                .into_iter()
                .map(|r| serde_json::json!({ "path": r.file, "line": r.line, "column": r.column, "snippet": r.content }))
                .collect::<Vec<_>>();
            serde_json::json!({ "_result": mapped })
        }
        Err(e) => serde_json::json!({ "_error": e }),
    }
}
