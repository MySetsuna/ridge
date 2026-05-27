use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use portable_pty::PtySize;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    middleware::Next,
    response::{Html, IntoResponse},
    routing::{get, post},
    Form, Json, Router,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::state::{AppState, RemotePaneSub, RemoteSubId};
use crate::types::{PtyDeltaEvent, PtyOutputEvent};

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
}

#[derive(Deserialize)]
struct VerifyForm {
    code: String,
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
    let base_port: u16 = 9527;
    let mut port = base_port;
    let listener = loop {
        let addr = format!("0.0.0.0:{}", port);
        match TcpListener::bind(&addr).await {
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
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    tracing::info!(target: "ridge::remote", port, lan_ip = %lan_ip, "Remote control server listening");

    // Resolve the static files directory. Try in order:
    // 1. CWD/static/mobile — works in dev (`cargo tauri dev`) when CWD is project root.
    // 2. exe_dir/static/mobile — works in production (NSIS install copies resources next to exe).
    // 3. exe_dir/../../../static/mobile — works when running the exe directly from
    //    target/release/ (parent→target→src-tauri→project-root/static/mobile).
    let static_dir = {
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("static").join("mobile"),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("static").join("mobile")))
                .unwrap_or_default(),
            std::env::current_exe()
                .ok()
                .and_then(|p| {
                    p.parent()?.parent()?.parent()?.parent()?.join("static").join("mobile").into()
                })
                .unwrap_or_default(),
        ];
        candidates.into_iter().find(|p| p.join("index.html").exists())
            .unwrap_or_else(|| PathBuf::from("static").join("mobile"))
    };

    let machine_name = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());

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
        .route_layer(axum::middleware::from_fn_with_state(ctx.clone(), remote_gate))
        .with_state(ctx);

    let _ = port_tx.send(Some(port));
    let shutdown_signal = shutdown_rx.map(|_| ());
    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
    {
        tracing::error!(target: "ridge::remote", error = %e, "remote server stopped");
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
            // Fallback: embed a basic page directing the user to build the mobile app
            Html(format!(
                r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Ridge Remote</title></head><body style="background:#0d1117;color:#e6edf3;font-family:sans-serif;display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;margin:0"><h1>Ridge Remote</h1><p>Mobile UI not built yet.</p><p>Run: <code>pnpm build:mobile</code></p></body></html>"#,
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
    State(ctx): State<RemoteCtx>,
    Form(form): Form<VerifyForm>,
) -> Json<VerifyResponse> {
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
    State(ctx): State<RemoteCtx>,
) -> impl IntoResponse {
    if !ctx.state.remote_enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "remote control disabled").into_response();
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
    ws.on_upgrade(move |socket| handle_ws(socket, ctx)).into_response()
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
    let active = *ctx.state.active_workspace.read();
    let workspaces: Vec<serde_json::Value> = order.iter().map(|id| {
        serde_json::json!({
            "id": id.to_string(),
            "name": names.get(id).cloned().unwrap_or_else(|| format!("工作区 {}", ctx.state.next_workspace_seq.read().saturating_sub(1))),
            "active": *id == active,
        })
    }).collect();
    Json(serde_json::json!({ "workspaces": workspaces }))
}

async fn workspace_switch_handler(
    State(ctx): State<RemoteCtx>,
    Json(body): Json<WorkspaceSwitchBody>,
) -> (StatusCode, Json<serde_json::Value>) {
    let id = match Uuid::parse_str(&body.workspace_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success":false,"error":"invalid workspace id"}))),
    };
    let exists = ctx.state.workspaces.read().contains_key(&id);
    if !exists {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"success":false,"error":"workspace not found"})));
    }
    *ctx.state.active_workspace.write() = id;
    (StatusCode::OK, Json(serde_json::json!({ "success": true, "workspaceId": id.to_string() })))
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
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success":false,"error":"invalid workspace id"}))),
    };
    {
        let order = ctx.state.workspace_order.read();
        if order.len() <= 1 {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"success":false,"error":"cannot close last workspace"})));
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

async fn handle_ws(socket: WebSocket, ctx: RemoteCtx) {
    tracing::info!(target: "ridge::remote", "WebSocket client connected");

    use futures::{SinkExt, StreamExt};
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Per-client mpsc channels — isolated from other WS clients.
    let (output_tx, mut output_rx) = mpsc::channel::<PtyOutputEvent>(128);
    let (delta_tx, mut delta_rx) = mpsc::channel::<PtyDeltaEvent>(256);
    let sub_id = RemoteSubId::next();

    // Track which (ws, pane) this client is currently subscribed to.
    let mut current_pane: Option<(Uuid, Uuid)> = None;

    // Initial handshake.
    let welcome = serde_json::json!({"type": "hello","version": 1,"protocol": "ridge-remote-ws"});
    if ws_tx.send(Message::Text(welcome.to_string())).await.is_err() {
        return;
    }

    let active_ws_id = ctx.state.active_workspace_id();

    // Periodic health check: if remote control is toggled off while this
    // WS connection is alive, close it so the mobile client gets a clean
    // disconnect.
    let mut health_interval = tokio::time::interval(std::time::Duration::from_secs(30));
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
                            for (pane_id, _) in &ws.terminals {
                                let title = ws.teammate_pane_titles.get(pane_id)
                                    .cloned()
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
                            // Unregister from current pane
                            if let Some((ws, p)) = current_pane.take() {
                                ctx.state.unregister_remote_sub(ws, p, sub_id);
                            }
                            let new_key = (active_ws_id, pane_id);
                            // Register for new pane
                            ctx.state.register_remote_sub(
                                active_ws_id, pane_id,
                                RemotePaneSub {
                                    id: sub_id,
                                    output_tx: output_tx.clone(),
                                    delta_tx: delta_tx.clone(),
                                },
                            );
                            current_pane = Some(new_key);
                            // Force delta mode + send full-reframe so the mobile
                            // client can bootstrap its kernel from a known baseline.
                            let delta_bytes = {
                                let workspaces = ctx.state.workspaces.read();
                                if let Some(ws) = workspaces.get(&active_ws_id) {
                                    if let Some(h) = ws.terminals.get(&pane_id) {
                                        h.delta_mode.store(true, Ordering::Release);
                                        let mut p = h.parser.lock();
                                        p.force_full_reframe();
                                        let frame = p.feed_and_diff(b"");
                                        ridge_term::term::delta::encode_frame(&frame).ok()
                                    } else { None }
                                } else { None }
                            };
                            if let Some(bytes) = delta_bytes {
                                let mut payload = Vec::with_capacity(16 + bytes.len());
                                payload.extend_from_slice(pane_id.as_bytes());
                                payload.extend_from_slice(&bytes);
                                let _ = ws_tx.send(Message::Binary(payload.into())).await;
                            }
                        }
                        Ok(())
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
                        let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        let rows = parsed["rows"].as_u64().unwrap_or(24) as u16;
                        let cols = parsed["cols"].as_u64().unwrap_or(80) as u16;
                        // Use real pixel dimensions from the client when available,
                        // so mobile devices get proper cell size calculation.
                        let pixel_width = parsed["pixelWidth"].as_u64().unwrap_or(cols as u64 * 8) as u16;
                        let pixel_height = parsed["pixelHeight"].as_u64().unwrap_or(rows as u64 * 16) as u16;
                        if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                            let workspaces = ctx.state.workspaces.read();
                            if let Some(ws) = workspaces.get(&active_ws_id) {
                                if let Some(handle) = ws.terminals.get(&pane_id) {
                                    let _ = handle.master.lock().resize(
                                        PtySize {
                                            rows, cols,
                                            pixel_width,
                                            pixel_height,
                                        }
                                    );
                                }
                            }
                        }
                        Ok(())
                    }
                    Some("list-files") => {
                        let path_str = parsed["path"].as_str().unwrap_or("").to_string();
                        let base_dir = ctx.state.current_project.read().clone()
                            .or_else(|| dirs::home_dir())
                            .unwrap_or_else(|| PathBuf::from("/"));
                        let result = tokio::task::spawn_blocking(move || {
                            let target = if path_str.is_empty() || path_str == "/" {
                                base_dir.clone()
                            } else {
                                base_dir.join(&path_str)
                            };
                            let gictx = crate::fs::tree::FileTreeContext::for_path(&target);
                            let mut entries: Vec<serde_json::Value> = Vec::new();
                            if let Ok(read_dir) = std::fs::read_dir(&target) {
                                for entry in read_dir.flatten() {
                                    let name = entry.file_name().to_string_lossy().to_string();
                                    if crate::fs::tree::FileTree::should_ignore(&entry.path()) { continue; }
                                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                                    let is_ignored = gictx.matches(&entry.path(), is_dir);
                                    let child_count = if is_dir {
                                        std::fs::read_dir(&entry.path()).ok().map(|rd| rd.flatten().filter(|e| !crate::fs::tree::FileTree::should_ignore(&e.path())).count())
                                    } else { None };
                                    entries.push(serde_json::json!({
                                        "name": name,
                                        "path": entry.path().strip_prefix(&base_dir)
                                            .unwrap_or(entry.path().as_path())
                                            .to_string_lossy().to_string().replace('\\', "/"),
                                        "is_dir": is_dir,
                                        "is_ignored": is_ignored,
                                        "child_count": child_count,
                                    }));
                                }
                            }
                            entries.sort_by(|a, b| {
                                let a_dir = a["is_dir"].as_bool().unwrap_or(false);
                                let b_dir = b["is_dir"].as_bool().unwrap_or(false);
                                b_dir.cmp(&a_dir).then(
                                    a["name"].as_str().unwrap_or("").to_lowercase()
                                        .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
                                )
                            });
                            serde_json::json!({"type":"files","path":path_str,"entries":entries})
                        }).await;
                        match result {
                            Ok(msg) => ws_tx.send(Message::Text(msg.to_string())).await,
                            Err(e) => {
                                tracing::warn!(target: "ridge::remote", error = %e, "list-files blocking task failed");
                                Ok(())
                            }
                        }
                    }
                    Some("list-git-status") => {
                        let workspace_dir = ctx.state.current_project.read().clone()
                            .or_else(|| dirs::home_dir())
                            .unwrap_or_else(|| PathBuf::from("."));
                        let result = tokio::task::spawn_blocking(move || {
                            let mut staged: Vec<String> = Vec::new();
                            let mut unstaged: Vec<serde_json::Value> = Vec::new();
                            let mut commits: Vec<serde_json::Value> = Vec::new();
                            if let Ok(output) = std::process::Command::new("git")
                                .args(["-C", &workspace_dir.to_string_lossy(), "status", "--porcelain"]).output()
                            {
                                for line in String::from_utf8_lossy(&output.stdout).lines() {
                                    if line.len() < 3 { continue; }
                                    let file = &line[3..];
                                    let st = line.as_bytes()[0];
                                    if st == b'?' || st == b' ' {
                                        unstaged.push(serde_json::json!({"name": file, "status": line[..2].to_string()}));
                                    } else {
                                        staged.push(file.to_string());
                                    }
                                }
                            }
                            if let Ok(output) = std::process::Command::new("git")
                                .args(["-C", &workspace_dir.to_string_lossy(), "log", "--oneline", "-10"]).output()
                            {
                                for line in String::from_utf8_lossy(&output.stdout).lines() {
                                    if let Some((hash, msg)) = line.split_once(' ') {
                                        commits.push(serde_json::json!({"hash": hash, "msg": msg, "time": ""}));
                                    }
                                }
                            }
                            serde_json::json!({"type":"git-status","staged":staged,"unstaged":unstaged,"commits":commits})
                        }).await;
                        match result {
                            Ok(msg) => ws_tx.send(Message::Text(msg.to_string())).await,
                            Err(e) => {
                                tracing::warn!(target: "ridge::remote", error = %e, "list-git-status blocking task failed");
                                Ok(())
                            }
                        }
                    }
                    _ => {
                        ws_tx.send(Message::Text(serde_json::json!({"type":"error","message":"unknown message type"}).to_string())).await
                    }
                };
            }
            event = output_rx.recv() => {
                match event {
                    Some(PtyOutputEvent { workspace_id, pane_id, data }) => {
                        if workspace_id == active_ws_id {
                            if ws_tx.send(Message::Text(serde_json::json!({
                                "type": "output",
                                "paneId": pane_id.to_string(),
                                "data": data,
                            }).to_string())).await.is_err() {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
            delta_event = delta_rx.recv() => {
                match delta_event {
                    Some(PtyDeltaEvent { workspace_id, pane_id, bytes }) => {
                        if workspace_id == active_ws_id {
                            let mut payload = Vec::with_capacity(16 + bytes.len());
                            payload.extend_from_slice(pane_id.as_bytes());
                            payload.extend_from_slice(&bytes);
                            if ws_tx.send(Message::Binary(payload.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
            _ = health_interval.tick() => {
                // If remote control was toggled off after this WS
                // connection was established, close the socket.
                if !ctx.state.remote_enabled.load(Ordering::Relaxed) {
                    let _ = ws_tx.send(Message::Close(None)).await;
                    break;
                }
            }
        }
    }

    // Clean up: unregister from all subscribed panes.
    if let Some((ws, pane)) = current_pane.take() {
        ctx.state.unregister_remote_sub(ws, pane, sub_id);
    }

    tracing::info!(target: "ridge::remote", "WebSocket client disconnected");
}
