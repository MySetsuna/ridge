use std::sync::Arc;
use std::thread;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::state::AppState;

use super::auth::RemoteAuth;

#[derive(Clone)]
struct RemoteCtx {
    port: u16,
    state: AppState,
    handle: tauri::AppHandle,
    auth: Arc<RemoteAuth>,
}

#[derive(Deserialize)]
struct ConnectQuery {
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
    totp_code: String,
    otpauth_uri: String,
    ready: bool,
}

/// Spawn the remote-control WebSocket server on a background thread.
/// Listens on `0.0.0.0:0` (OS-assigned port) and returns the allocated
/// port, or `None` if binding failed.
///
/// Uses the existing `auth` from AppState so the TOTP secret is shared
/// between the HTTP server and the Tauri command `get_remote_info`.
pub fn spawn_remote_server(
    handle: tauri::AppHandle,
    state: AppState,
    auth: Arc<RemoteAuth>,
) -> Option<u16> {

    let handle_for_thread = handle.clone();
    let state_for_thread = state.clone();
    let auth_for_thread = auth.clone();

    let (port_tx, port_rx) = std::sync::mpsc::channel();

    thread::Builder::new()
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
                handle_for_thread,
                state_for_thread,
                auth_for_thread,
                port_tx,
            ));
        })
        .expect("ridge-remote-http thread spawn");

    port_rx.recv().ok().flatten()
}

async fn run_remote_server(
    handle: tauri::AppHandle,
    state: AppState,
    auth: Arc<RemoteAuth>,
    port_tx: std::sync::mpsc::Sender<Option<u16>>,
) {
    // Bind on all interfaces so LAN clients can reach us.
    let listener = match TcpListener::bind("0.0.0.0:0").await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote server bind failed");
            let _ = port_tx.send(None);
            return;
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    tracing::info!(target: "ridge::remote", port, "Remote control server listening");

    let ctx = RemoteCtx {
        port,
        state,
        handle,
        auth,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/info", get(info_handler))
        .route("/status", get(status_handler))
        .route("/ws", get(ws_handler))
        .with_state(ctx);

    let _ = port_tx.send(Some(port));
    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!(target: "ridge::remote", error = %e, "remote server stopped");
    }
}

// ── Handlers ────────────────────────────────────────────────────────────────

async fn info_handler(State(ctx): State<RemoteCtx>) -> Json<InfoResponse> {
    let (code, uri) = ctx.auth.code_and_uri();
    Json(InfoResponse {
        port: ctx.port,
        totp_code: code,
        otpauth_uri: uri,
        ready: true,
    })
}

async fn status_handler(State(ctx): State<RemoteCtx>) -> Json<StatusResponse> {
    Json(StatusResponse {
        port: ctx.port,
        ready: true,
    })
}

/// WebSocket upgrade handler. Requires a `?code=XXXXXX` query parameter
/// containing a valid TOTP 6-digit code. Returns 401 if the code is missing
/// or invalid.
async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<ConnectQuery>,
    State(ctx): State<RemoteCtx>,
) -> impl IntoResponse {
    if !ctx.auth.verify(&query.code) {
        return (StatusCode::UNAUTHORIZED, "invalid TOTP code").into_response();
    }
    ws.on_upgrade(move |socket| handle_ws(socket, ctx)).into_response()
}

async fn handle_ws(mut socket: WebSocket, _ctx: RemoteCtx) {
    tracing::info!(target: "ridge::remote", "WebSocket client connected");

    // Initial handshake: send server info.
    let welcome = serde_json::json!({
        "type": "hello",
        "version": 1,
        "protocol": "ridge-remote-ws",
    });
    if socket
        .send(Message::Text(welcome.to_string()))
        .await
        .is_err()
    {
        return;
    }

    // Message loop: echo for now; future iterations will stream
    // terminal output and forward keyboard input.
    loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                // Parse and dispatch messages.
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    match msg["type"].as_str() {
                        Some("ping") => {
                            let pong = serde_json::json!({"type": "pong"});
                            let _ = socket.send(Message::Text(pong.to_string())).await;
                        }
                        Some("list-panes") => {
                            // Return a list of active panes (basic stub).
                            let panes = serde_json::json!({"type": "panes", "panes": []});
                            let _ = socket.send(Message::Text(panes.to_string())).await;
                        }
                        _ => {
                            let err = serde_json::json!({"type": "error", "message": "unknown message type"});
                            let _ = socket.send(Message::Text(err.to_string())).await;
                        }
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => break,
            Some(Err(_)) => break,
            _ => {}
        }
    }

    tracing::info!(target: "ridge::remote", "WebSocket client disconnected");
}
