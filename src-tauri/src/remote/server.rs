use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use portable_pty::PtySize;

use crate::engine::parser::PaneParser;

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
    let map = ctx.state.workspaces.read();
    let active = *ctx.state.active_workspace.read();
    let workspaces: Vec<serde_json::Value> = order.iter().map(|id| {
        // §unify: per-workspace display_seq fallback name, matching the desktop
        // and the WS `list-workspaces` handler.
        let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
        serde_json::json!({
            "id": id.to_string(),
            "name": names.get(id).cloned().unwrap_or_else(|| format!("工作区 {}", display_seq)),
            "displaySeq": display_seq,
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

/// §multi-size: resize a pane's PTY to `rows`×`cols`, resize its canonical
/// `PaneParser`, and broadcast the resulting delta frame to BOTH the desktop
/// (via the pane delta channel) and every remote subscriber. This is the
/// shared path for the remote "first-connect auto-claim" and the explicit
/// "refresh-pane" button — i.e. the only places a render endpoint claims the
/// shared PTY size. Mirrors `commands::terminal::resize_pane_inner` minus the
/// Tauri-only ConPTY silence window (skipped, like the alt-screen path).
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
        let Some(handle) = ws.terminals.get(&pane_id) else { return };
        let _ = handle.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
        });
        // The canonical parser owns every viewer's grid now, so it must be
        // in delta mode and resized in lock-step with the PTY.
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
    // All remote viewers share this one canonical resize frame.
    let reg = ctx.state.pty_pane_registry.read();
    if let Some(entry) = reg.get(&(ws_id, pane_id)) {
        for sub in &entry.remote_subs {
            let _ = sub.delta_tx.try_send(PtyDeltaEvent {
                workspace_id: ws_id,
                pane_id,
                bytes: bytes.clone(),
            });
        }
    }
}

async fn handle_ws(socket: WebSocket, ctx: RemoteCtx) {
    use futures::{SinkExt, StreamExt};
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Register this client in the remote client registry so the desktop
    // RemotePanel can list and optionally disconnect it.
    let (client_id, kill_flag) = ctx.state.remote_client_registry.register(
        "unknown".to_string(),
        String::new(), // user-agent not available from axum WS directly
    );
    tracing::info!(target: "ridge::remote", client_id, "WebSocket client connected");

    // Per-client mpsc channels — isolated from other WS clients.
    let (output_tx, mut output_rx) = mpsc::channel::<PtyOutputEvent>(128);
    let (delta_tx, mut delta_rx) = mpsc::channel::<PtyDeltaEvent>(256);
    let sub_id = RemoteSubId::next();

    // Track which (ws, pane) this client is currently subscribed to.
    let mut current_pane: Option<(Uuid, Uuid)> = None;

    // Client-reported viewport grid dimensions, updated by the `resize` WS
    // message. Used for the first-connect auto-claim and the refresh button.
    let mut mobile_rows: u16 = 24;
    let mut mobile_cols: u16 = 80;
    // §own-active: there is NO connect-time auto-claim. A remote endpoint resizes
    // the shared PTY only when it becomes the active owner — i.e. on a genuine
    // user interaction (`claim-pane`) or the explicit refresh button
    // (`refresh-pane`). Merely connecting / changing viewport records the size
    // here but never stomps the PTY the desktop is using.

    // Initial handshake.
    let welcome = serde_json::json!({"type": "hello","version": 1,"protocol": "ridge-remote-ws"});
    if ws_tx.send(Message::Text(welcome.to_string())).await.is_err() {
        return;
    }

    // §state-sep: per-client active workspace. Seeded once from the global
    // active workspace at connect, then owned by THIS client. Switching /
    // creating / closing workspaces from a remote no longer rewrites the
    // shared `active_workspace` (which would drag the desktop and every other
    // client along) — only this `active_ws_id` moves. All readers below
    // (list-panes / subscribe / stdin / resize / output+delta filters) use it.
    let mut active_ws_id = ctx.state.active_workspace_id();

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
                            // Unregister from current pane (drops its parser too).
                            if let Some((ws, p)) = current_pane.take() {
                                ctx.state.unregister_remote_sub(ws, p, sub_id);
                            }
                            let new_key = (active_ws_id, pane_id);

                            // §multi-size: each remote client renders at its OWN
                            // grid size via a dedicated per-sub PaneParser (the
                            // lib.rs fan-out feeds PTY output through it and sends
                            // mobile-sized deltas). Ensure the canonical parser is
                            // in delta mode so the PTY reader runs the delta path
                            // that drives every sub.
                            {
                                let workspaces = ctx.state.workspaces.read();
                                if let Some(h) = workspaces
                                    .get(&active_ws_id)
                                    .and_then(|ws| ws.terminals.get(&pane_id))
                                {
                                    h.delta_mode.store(true, Ordering::Release);
                                }
                            }

                            // Per-sub parser at THIS client's viewport size. Feed
                            // it the recent scrollback so its state matches the
                            // bootstrap frame sent below — the client kernel and
                            // this parser then stay in lock-step from frame 0.
                            let sub_rows = mobile_rows.max(1);
                            let sub_cols = mobile_cols.max(1);
                            let sub_parser = Arc::new(parking_lot::Mutex::new(
                                PaneParser::new(sub_rows, sub_cols, 5000),
                            ));
                            let init_bytes = {
                                let mut mp = sub_parser.lock();
                                let replay = ctx.state.get_recent_scrollback_for(
                                    active_ws_id,
                                    pane_id,
                                    65536, // 64 KiB — covers ~500 lines of history
                                );
                                if !replay.is_empty() {
                                    mp.feed_and_diff(&replay);
                                }
                                let frame = mp.full_reframe_with_scrollback();
                                ridge_term::term::delta::encode_frame(&frame)
                                    .unwrap_or_default()
                            };

                            ctx.state.register_remote_sub(
                                active_ws_id, pane_id,
                                RemotePaneSub {
                                    id: sub_id,
                                    output_tx: output_tx.clone(),
                                    delta_tx: delta_tx.clone(),
                                    // §multi-size: per-client parser → the lib.rs
                                    // fan-out sends this sub its own mobile-sized
                                    // delta frames.
                                    parser: Some(sub_parser),
                                    rows: sub_rows,
                                    cols: sub_cols,
                                },
                            );
                            current_pane = Some(new_key);

                            // Send the bootstrap frame (goes out before any queued
                            // canonical delta, since this handler runs to
                            // completion before the select loop drains delta_rx).
                            if !init_bytes.is_empty() {
                                let mut payload =
                                    Vec::with_capacity(16 + init_bytes.len());
                                payload.extend_from_slice(pane_id.as_bytes());
                                payload.extend_from_slice(&init_bytes);
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
                        ws_tx.send(Message::Text(serde_json::json!({
                            "type": "create-workspace-result",
                            "success": true,
                            "workspaceId": id.to_string(),
                        }).to_string())).await
                    }
                    Some("close-workspace") => {
                        let id_str = parsed["workspaceId"].as_str().unwrap_or("");
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
                                    serde_json::json!({ "type": "close-workspace-result", "success": true })
                                }
                            }
                        } else {
                            serde_json::json!({ "type": "close-workspace-result", "success": false, "error": "invalid workspace id" })
                        };
                        ws_tx.send(Message::Text(result.to_string())).await
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
                        let rows = parsed["rows"].as_u64().unwrap_or(mobile_rows as u64) as u16;
                        let cols = parsed["cols"].as_u64().unwrap_or(mobile_cols as u64) as u16;
                        // §multi-size: record the client's viewport and reflow ONLY
                        // this client's per-sub parser — never the shared PTY (that
                        // stays owned by whoever last hit refresh/claim). Then push
                        // a full reframe so an idle app reflows immediately instead
                        // of waiting for the next PTY output chunk.
                        mobile_rows = rows.max(1);
                        mobile_cols = cols.max(1);
                        if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                            ctx.state.resize_remote_parser(
                                active_ws_id, pane_id, sub_id, mobile_rows, mobile_cols,
                            );
                            let sub_parser = {
                                let reg = ctx.state.pty_pane_registry.read();
                                reg.get(&(active_ws_id, pane_id)).and_then(|entry| {
                                    entry
                                        .remote_subs
                                        .iter()
                                        .find(|s| s.id == sub_id)
                                        .and_then(|s| s.parser.clone())
                                })
                            };
                            if let Some(mp) = sub_parser {
                                let bytes = {
                                    let mut p = mp.lock();
                                    let frame = p.full_reframe_with_scrollback();
                                    ridge_term::term::delta::encode_frame(&frame)
                                        .unwrap_or_default()
                                };
                                if !bytes.is_empty() {
                                    let mut payload = Vec::with_capacity(16 + bytes.len());
                                    payload.extend_from_slice(pane_id.as_bytes());
                                    payload.extend_from_slice(&bytes);
                                    let _ = ws_tx.send(Message::Binary(payload.into())).await;
                                }
                            }
                        }
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
                        let result = match crate::commands::pane::remote_create_pane(&ctx.state, active_ws_id, shell) {
                            Ok(new_id) => match crate::commands::terminal::activate_pane_pty_state(
                                &ctx.state, None, active_ws_id, new_id,
                                Some(mobile_rows), Some(mobile_cols),
                            ) {
                                Ok(()) => serde_json::json!({
                                    "type": "create-pane-result", "success": true, "paneId": new_id.to_string()
                                }),
                                Err(e) => serde_json::json!({
                                    "type": "create-pane-result", "success": false, "error": e.to_string()
                                }),
                            },
                            Err(e) => serde_json::json!({
                                "type": "create-pane-result", "success": false, "error": e.to_string()
                            }),
                        };
                        ws_tx.send(Message::Text(result.to_string())).await
                    }
                    Some("close-pane") => {
                        let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                        let result = match Uuid::parse_str(pane_id_str) {
                            Ok(pane_id) => match crate::commands::pane::remote_close_pane(&ctx.state, active_ws_id, pane_id).await {
                                Ok(()) => serde_json::json!({ "type": "close-pane-result", "success": true }),
                                Err(e) => serde_json::json!({ "type": "close-pane-result", "success": false, "error": e.to_string() }),
                            },
                            Err(_) => serde_json::json!({ "type": "close-pane-result", "success": false, "error": "invalid pane id" }),
                        };
                        ws_tx.send(Message::Text(result.to_string())).await
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
