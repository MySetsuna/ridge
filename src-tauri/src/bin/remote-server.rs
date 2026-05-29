// Standalone Ridge Remote Server
//
// Usage (dev):   cargo run --bin remote-server
// Usage (build): ./target/release/remote-server.exe
//
// Serves the built remote app from <exe-dir>/static/remote/ (or ./static/remote/)
// and provides the remote-control API (TOTP auth, WebSocket, file browser).
//
// NOTE: WebSocket terminal control requires the full Ridge Tauri app running.
// In standalone mode, WebSocket accepts connections for UI testing but
// terminal operations are stubbed.

use std::path::PathBuf;
use std::sync::Arc;

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
use serde::{Deserialize, Serialize};
use sysinfo::System;
use tokio::net::TcpListener;

use ridge_lib::remote::auth::RemoteAuth;
use ridge_lib::remote::detect_lan_ip;

// ── State ──

#[derive(Clone)]
struct AppCtx {
    port: u16,
    lan_ip: String,
    auth: Arc<RemoteAuth>,
    machine_name: String,
    static_dir: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InfoResponse {
    port: u16,
    lan_ip: String,
    totp_code: String,
    otpauth_uri: String,
    ready: bool,
}

#[derive(Serialize)]
struct StatusResponse {
    port: u16,
    ready: bool,
}

#[derive(Deserialize)]
struct VerifyForm {
    code: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    success: bool,
    message: String,
}

#[derive(Deserialize)]
struct ConnectQuery {
    code: String,
}

// ── Handlers ──

async fn root_handler(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let index_path = ctx.static_dir.join("index.html");
    match tokio::fs::read_to_string(&index_path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => {
            Html(format!(
                r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Ridge Remote</title></head><body style="background:#0d1117;color:#e6edf3;font-family:sans-serif;display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;margin:0"><h1>Ridge Remote</h1><p>Mobile UI not built yet.</p><p>Run: <code>pnpm build:mobile</code></p></body></html>"#,
            ))
            .into_response()
        }
    }
}

async fn assets_handler(
    State(ctx): State<AppCtx>,
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
            } else {
                "application/octet-stream"
            };
            ([(axum::http::header::CONTENT_TYPE, content_type)], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn info_handler(State(ctx): State<AppCtx>) -> impl IntoResponse {
    let (code, uri) = ctx.auth.code_and_uri(&ctx.machine_name);
    Json(InfoResponse {
        port: ctx.port,
        lan_ip: ctx.lan_ip.clone(),
        totp_code: code,
        otpauth_uri: uri,
        ready: true,
    })
}

async fn status_handler(State(ctx): State<AppCtx>) -> Json<StatusResponse> {
    Json(StatusResponse {
        port: ctx.port,
        ready: true,
    })
}

async fn verify_handler_post(
    State(ctx): State<AppCtx>,
    Form(form): Form<VerifyForm>,
) -> Json<VerifyResponse> {
    let valid = ctx.auth.verify(&form.code);
    Json(VerifyResponse {
        success: valid,
        message: if valid {
            "Verification successful".to_string()
        } else {
            "Invalid TOTP code".to_string()
        },
    })
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<ConnectQuery>,
    State(ctx): State<AppCtx>,
) -> impl IntoResponse {
    if !ctx.auth.verify(&query.code) {
        return (StatusCode::UNAUTHORIZED, "invalid TOTP code").into_response();
    }
    ws.on_upgrade(move |socket| handle_ws(socket)).into_response()
}

async fn handle_ws(socket: WebSocket) {
    use futures::{SinkExt, StreamExt};
    let (mut ws_tx, mut ws_rx) = socket.split();

    let welcome = serde_json::json!({
        "type": "hello",
        "version": 1,
        "protocol": "ridge-remote-ws",
        "standalone": true,
        "message": "Standalone mode — terminal operations limited. Run full Ridge app for complete remote control."
    });
    if ws_tx.send(Message::Text(welcome.to_string())).await.is_err() {
        return;
    }

    loop {
        let msg = ws_rx.next().await;
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
            Some("list-panes") | Some("stdin") | Some("resize") | Some("list-files") | Some("list-git-status") => {
                ws_tx.send(Message::Text(serde_json::json!({
                    "type": "error",
                    "message": "This operation requires the full Ridge app (standalone mode)"
                }).to_string())).await
            }
            _ => {
                ws_tx.send(Message::Text(serde_json::json!({"type":"error","message":"unknown message type"}).to_string())).await
            }
        };
    }
}

// ── Main ──

#[tokio::main]
async fn main() {
    println!("Ridge Remote Server v{}", env!("CARGO_PKG_VERSION"));
    println!("Starting...");

    // Resolve static files directory: prefers cwd/static/remote/ (dev),
    // then exe-relative static/remote/ (installed), then fallback.
    let static_dir = PathBuf::from("static").join("remote");
    let static_dir = if static_dir.exists() {
        static_dir
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("static").join("remote")))
            .unwrap_or(static_dir)
    };

    println!("  Static dir: {}", static_dir.display());

    let auth = Arc::new(RemoteAuth::new());
    let lan_ip = detect_lan_ip();
    let machine_name = System::host_name().unwrap_or_else(|| "unknown".to_string());

    let listener = match TcpListener::bind("0.0.0.0:0").await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind: {e}");
            std::process::exit(1);
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    println!("  Port: {}", port);
    println!("  LAN IP: {}", lan_ip);
    println!();
    println!("Open http://localhost:{}/ in your browser", port);
    println!("Or scan the QR code from the Ridge PC app (Remote sidebar)");
    println!();

    let ctx = AppCtx {
        port,
        lan_ip,
        auth,
        machine_name,
        static_dir,
    };

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/health", get(|| async { "ok" }))
        .route("/info", get(info_handler))
        .route("/status", get(status_handler))
        .route("/verify", get(verify_handler_get).post(verify_handler_post))
        .route("/ws", get(ws_handler))
        .route("/assets/*path", get(assets_handler))
        .with_state(ctx);

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Server error: {e}");
    }
}

async fn verify_handler_get(State(ctx): State<AppCtx>) -> impl IntoResponse {
    root_handler(State(ctx)).await
}
