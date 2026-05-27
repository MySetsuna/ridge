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
    response::{Html, IntoResponse},
    routing::get,
    Form, Json, Router,
};
use futures::FutureExt;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::state::AppState;
use crate::types::PtyOutputEvent;

use super::auth::RemoteAuth;

#[derive(Clone)]
struct RemoteCtx {
    port: u16,
    lan_ip: String,
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
            let rt = match tokio::runtime::Builder::new_current_thread()
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
    let addr = if cfg!(debug_assertions) {
        "127.0.0.1:5175"
    } else {
        "0.0.0.0:0"
    };

    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote server bind failed");
            let _ = port_tx.send(None);
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
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

    let ctx = RemoteCtx {
        port,
        lan_ip,
        state,
        auth,
        static_dir,
    };

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/health", get(|| async { "ok" }))
        .route("/info", get(info_handler))
        .route("/status", get(status_handler))
        .route("/verify", get(verify_handler_get).post(verify_handler_post))
        .route("/ws", get(ws_handler))
        .route("/session", get(session_handler))
        .route("/assets/*path", get(assets_handler))
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

// ── Handlers ────────────────────────────────────────────────────────────────

async fn root_handler(State(ctx): State<RemoteCtx>) -> impl IntoResponse {
    if !ctx.state.remote_enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Remote control disabled").into_response();
    }
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
            let content_type = if path.ends_with(".js") {
                "application/javascript"
            } else if path.ends_with(".css") {
                "text/css"
            } else if path.ends_with(".wasm") {
                "application/wasm"
            } else if path.ends_with(".svg") {
                "image/svg+xml"
            } else if path.ends_with(".png") {
                "image/png"
            } else if path.ends_with(".woff2") {
                "font/woff2"
            } else {
                "application/octet-stream"
            };
            ([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn info_handler(State(ctx): State<RemoteCtx>) -> impl IntoResponse {
    let enabled = ctx.state.remote_enabled.load(Ordering::Relaxed);
    let machine_name = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());
    let uri = ctx.auth.otpauth_uri(&machine_name);
    Json(InfoResponse {
        port: ctx.port,
        lan_ip: ctx.lan_ip.clone(),
        otpauth_uri: uri,
        ready: enabled,
        machine_name,
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

/// Check if a session token is still valid.
async fn session_handler(
    Query(query): Query<SessionQuery>,
    State(ctx): State<RemoteCtx>,
) -> impl IntoResponse {
    let valid = ctx.state.remote_session_store.validate_token(&query.token);
    Json(serde_json::json!({ "valid": valid }))
}

async fn handle_ws(socket: WebSocket, ctx: RemoteCtx) {
    tracing::info!(target: "ridge::remote", "WebSocket client connected");

    use futures::{SinkExt, StreamExt};
    let (mut ws_tx, mut ws_rx) = socket.split();

    // Subscribe to PTY output events for terminal streaming.
    let mut pty_rx = ctx.state.pty_output_tx.subscribe();

    // Track which panes this client is subscribed to.
    let mut subscribed_panes: Vec<(Uuid, Uuid)> = Vec::new();

    // Initial handshake.
    let welcome = serde_json::json!({"type": "hello","version": 1,"protocol": "ridge-remote-ws"});
    if ws_tx.send(Message::Text(welcome.to_string())).await.is_err() {
        return;
    }

    let active_ws_id = ctx.state.active_workspace_id();

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
                        let mut pane_list = Vec::new();
                        {
                            let workspaces = ctx.state.workspaces.read();
                            if let Some(ws) = workspaces.get(&active_ws_id) {
                                subscribed_panes.clear();
                                for (pane_id, _) in &ws.terminals {
                                    let title = ws.teammate_pane_titles.get(pane_id)
                                        .cloned()
                                        .or_else(|| ws.pane_tree.panes.get(pane_id).and_then(|n| n.shell_kind.clone()))
                                        .unwrap_or_else(|| "terminal".to_string());
                                    pane_list.push(serde_json::json!({
                                        "id": pane_id.to_string(),
                                        "title": title,
                                        "cwd": ws.pane_tree.panes.get(pane_id)
                                            .and_then(|n| n.cwd.as_ref().map(|p| p.to_string_lossy().to_string()))
                                            .unwrap_or_default(),
                                    }));
                                    subscribed_panes.push((active_ws_id, *pane_id));
                                }
                                for (pane_id, _) in &ws.pending_spawns {
                                    let pair = (active_ws_id, *pane_id);
                                    if !subscribed_panes.contains(&pair) {
                                        subscribed_panes.push(pair);
                                    }
                                }
                            }
                        }
                        ws_tx.send(Message::Text(serde_json::json!({"type":"panes","panes":pane_list}).to_string())).await
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
                        if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                            let workspaces = ctx.state.workspaces.read();
                            if let Some(ws) = workspaces.get(&active_ws_id) {
                                if let Some(handle) = ws.terminals.get(&pane_id) {
                                    let _ = handle.master.lock().resize(
                                        PtySize {
                                            rows, cols,
                                            pixel_width: cols as u16 * 8,
                                            pixel_height: rows as u16 * 16,
                                        }
                                    );
                                }
                            }
                        }
                        Ok(())
                    }
                    Some("list-files") => {
                        let path = parsed["path"].as_str().unwrap_or("");
                        let base_dir = ctx.state.current_project.read().clone()
                            .or_else(|| dirs::home_dir())
                            .unwrap_or_else(|| PathBuf::from("/"));
                        let target = if path.is_empty() || path == "/" { base_dir.clone() } else { base_dir.join(path) };
                        let mut entries: Vec<serde_json::Value> = Vec::new();
                        if let Ok(read_dir) = std::fs::read_dir(&target) {
                            for entry in read_dir.flatten() {
                                let name = entry.file_name().to_string_lossy().to_string();
                                if name.starts_with('.') { continue; }
                                let ft = entry.file_type().ok();
                                entries.push(serde_json::json!({
                                    "name": name,
                                    "path": entry.path().strip_prefix(&base_dir)
                                        .unwrap_or(entry.path().as_path())
                                        .to_string_lossy().to_string().replace('\\', "/"),
                                    "type": if ft.map(|t| t.is_dir()).unwrap_or(false) { "dir" } else { "file" },
                                }));
                            }
                        }
                        entries.sort_by(|a, b| {
                            let a_dir = a["type"] == "dir";
                            let b_dir = b["type"] == "dir";
                            b_dir.cmp(&a_dir).then(a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
                        });
                        ws_tx.send(Message::Text(serde_json::json!({"type":"files","path":path,"entries":entries}).to_string())).await
                    }
                    Some("list-git-status") => {
                        let workspace_dir = ctx.state.current_project.read().clone()
                            .or_else(|| dirs::home_dir())
                            .unwrap_or_else(|| PathBuf::from("."));
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
                        ws_tx.send(Message::Text(serde_json::json!({"type":"git-status","staged":staged,"unstaged":unstaged,"commits":commits}).to_string())).await
                    }
                    _ => {
                        ws_tx.send(Message::Text(serde_json::json!({"type":"error","message":"unknown message type"}).to_string())).await
                    }
                };
            }
            event = pty_rx.recv() => {
                match event {
                    Ok(PtyOutputEvent { workspace_id, pane_id, data }) => {
                        if workspace_id == active_ws_id
                            && subscribed_panes.iter().any(|(_ws, p)| *p == pane_id)
                        {
                            if ws_tx.send(Message::Text(serde_json::json!({
                                "type": "output",
                                "paneId": pane_id.to_string(),
                                "data": data,
                            }).to_string())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(target: "ridge::remote", lagged = n, "WS client lagged behind PTY output");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    tracing::info!(target: "ridge::remote", "WebSocket client disconnected");
}
