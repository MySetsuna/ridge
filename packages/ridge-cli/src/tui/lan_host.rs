use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Form, Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};

use crate::config;
use crate::totp::RemoteTotp;
use ridge_core::workspace::pane_tree::SplitDirection;
use super::workspace::SharedWorkspace;

#[derive(Clone)]
struct AppCtx {
    port: u16,
    lan_ip: String,
    totp: Arc<RemoteTotp>,
    machine_name: String,
    static_dir: PathBuf,
    workspace: SharedWorkspace,
}

#[derive(Serialize)]
struct InfoResponse {
    port: u16,
    lan_ip: String,
    totp_code: String,
    otpauth_uri: String,
    ready: bool,
}

#[derive(Deserialize)]
struct VerifyForm {
    code: String,
}

#[derive(Deserialize)]
struct WsQuery {
    code: Option<String>,
}

pub async fn run(
    port: u16,
    totp: Arc<RemoteTotp>,
    workspace: SharedWorkspace,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<()> {
    let lan_ip = config::detect_lan_ip();
    let machine_name = host_name();
    let static_dir = resolve_static_dir();

    tracing::info!(
        target: "ridge_cli::lan_host",
        lan_ip = %lan_ip,
        port = port,
        "LAN remote service starting"
    );

    let ctx = AppCtx {
        port,
        lan_ip,
        totp,
        machine_name,
        static_dir,
        workspace,
    };

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/login", get(login_page_handler))
        .route("/terminal", get(terminal_page_handler))
        .route("/health", get(|| async { "ok" }))
        .route("/info", get(info_handler))
        .route("/verify", get(verify_get_handler).post(verify_handler))
        .route("/ws", get(ws_handler))
        .route("/assets/*path", get(assets_handler))
        .with_state(ctx.clone());

    let actual_port = ridge_remote::server::serve(
        port,
        app,
        &ctx.lan_ip,
        &ctx.machine_name,
        shutdown_rx,
        true,
    )
    .await?;

    tracing::info!(target: "ridge_cli::lan_host", port = actual_port, "LAN remote service stopped");
    Ok(())
}

// ── Static file helpers ──

fn resolve_static_dir() -> PathBuf {
    let candidates = [
        PathBuf::from("static").join("remote"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("static").join("remote")))
            .unwrap_or_default(),
    ];
    for c in &candidates {
        if c.join("index.html").exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

// ── Handlers ──

async fn root_handler() -> impl IntoResponse {
    Redirect::to("/login")
}

async fn login_page_handler() -> impl IntoResponse {
    Html(LOGIN_HTML)
}

async fn terminal_page_handler() -> impl IntoResponse {
    Html(TERMINAL_HTML)
}

async fn verify_get_handler() -> impl IntoResponse {
    Redirect::to("/login")
}

async fn verify_handler(
    State(ctx): State<AppCtx>,
    Form(form): Form<VerifyForm>,
) -> impl IntoResponse {
    let valid = ctx.totp.verify(&form.code);
    if valid {
        Redirect::to(&format!("/terminal?code={}", form.code))
    } else {
        Redirect::to("/login?error=1")
    }
}

async fn info_handler(State(ctx): State<AppCtx>) -> Json<InfoResponse> {
    Json(InfoResponse {
        port: ctx.port,
        lan_ip: ctx.lan_ip.clone(),
        totp_code: ctx.totp.current_code(),
        otpauth_uri: ctx.totp.otpauth_uri(&ctx.machine_name),
        ready: true,
    })
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

// ── WebSocket handler ──

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(ctx): State<AppCtx>,
) -> impl IntoResponse {
    let code = match query.code {
        Some(c) if !c.is_empty() => c,
        _ => return (StatusCode::UNAUTHORIZED, "missing TOTP code").into_response(),
    };

    if !ctx.totp.verify(&code) {
        return (StatusCode::UNAUTHORIZED, "invalid TOTP code").into_response();
    }

    ws.on_upgrade(move |socket| async move {
        run_ws(socket, ctx).await;
    })
}

async fn run_ws(socket: WebSocket, ctx: AppCtx) {
    let ws = Arc::new(Mutex::new(socket));
    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Writer: read from msg_rx and send through WebSocket
    let writer_ws = ws.clone();
    let writer = tokio::spawn(async move {
        while let Some(bytes) = msg_rx.recv().await {
            let mut guard = writer_ws.lock().await;
            if guard.send(Message::Binary(bytes)).await.is_err() {
                break;
            }
        }
    });

    // Send hello + pane list, then enter reader loop
    let panes_json = {
        let w = ctx.workspace.lock().unwrap();
        serde_json::json!({
            "type": "panes",
            "panes": w.sessions.iter().map(|s| {
                serde_json::json!({"id": s.id, "title": s.title, "cwd": s.cwd})
            }).collect::<Vec<_>>()
        })
        .to_string()
    };

    {
        let mut guard = ws.lock().await;
        let hello = serde_json::json!({"type":"hello","version":1,"protocol":"ridge-lan-ws"}).to_string();
        let _ = guard.send(Message::Text(hello)).await;
        let _ = guard.send(Message::Text(panes_json)).await;
    }

    let ws_clone = ws.clone();
    let msg_tx_for_reader = msg_tx.clone();
    let reader = tokio::spawn(async move {
        loop {
            let msg = {
                let mut guard = ws_clone.lock().await;
                match guard.recv().await {
                    Some(Ok(m)) => m,
                    _ => break,
                }
            };
            match msg {
                Message::Text(txt) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) else {
                        continue;
                    };
                    let typ = v["type"].as_str().unwrap_or("");
                    match typ {
                        "stdin" => {
                            let data = v["data"].as_str().unwrap_or("");
                            let pid = v["paneId"].as_str()
                                .filter(|s| !s.is_empty())
                                .map(String::from);
                            // Scope workspace lock to avoid holding MutexGuard across await
                            let target_pid = pid.or_else(|| -> Option<String> {
                                ctx.workspace.lock().unwrap().default_session_id().map(String::from)
                            });
                            if let Some(ref pid) = target_pid {
                                if let Ok(pid_uuid) = uuid::Uuid::parse_str(pid) {
                                    let w = ctx.workspace.lock().unwrap();
                                    if let Some(sess) = w.find(pid_uuid) {
                                        let _ = sess.send_input(data.as_bytes());
                                    }
                                }
                            }
                        }
                        "claim-pane" => {
                            if let Some(pid) = v["paneId"].as_str() {
                                if let Ok(pid_uuid) = uuid::Uuid::parse_str(pid) {
                                    let rows = v["rows"].as_u64().unwrap_or(24) as u16;
                                    let cols = v["cols"].as_u64().unwrap_or(80) as u16;
                                    let w = ctx.workspace.lock().unwrap();
                                    if let Some(sess) = w.find(pid_uuid) {
                                        let _ = sess.resize(cols, rows);
                                    }
                                }
                            }
                        }
                        "subscribe-pane" => {
                            if let Some(pid) = v["paneId"].as_str() {
                                if let Ok(pid_uuid) = uuid::Uuid::parse_str(pid) {
                                    let w = ctx.workspace.lock().unwrap();
                                    if let Some(sess) = w.find(pid_uuid) {
                                        let mut rx = sess.subscribe();
                                        let tx = msg_tx_for_reader.clone();
                                        let pid2 = pid.to_string();
                                        tokio::spawn(async move {
                                            while let Ok(bytes) = rx.recv().await {
                                                let mut buf = Vec::with_capacity(16 + bytes.len());
                                                let pbytes = pid2.as_bytes();
                                                let copy = pbytes.len().min(16);
                                                let mut hdr = [0u8; 16];
                                                hdr[..copy].copy_from_slice(&pbytes[..copy]);
                                                buf.extend_from_slice(&hdr);
                                                buf.extend_from_slice(&bytes);
                                                if tx.send(buf).is_err() {
                                                    break;
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                        "create-pane" => {
                            let cwd = v["cwd"].as_str();
                            // Scope the workspace lock — drop before await
                            let msg = {
                                let mut w = ctx.workspace.lock().unwrap();
                                match w.create_session(None, cwd, None, SplitDirection::Horizontal) {
                                    Ok(id) => serde_json::json!({"type":"create-pane-result","success":true,"paneId":id}).to_string(),
                                    Err(e) => serde_json::json!({"type":"create-pane-result","success":false,"message":format!("{e}")}).to_string(),
                                }
                            };
                            let mut guard = ws_clone.lock().await;
                            let _ = guard.send(Message::Text(msg)).await;
                        }
                        "list-panes" => {
                            let panes: Vec<serde_json::Value> = {
                                let w = ctx.workspace.lock().unwrap();
                                w.sessions.iter().map(|s| {
                                    serde_json::json!({"id": s.id, "title": s.title, "cwd": s.cwd})
                                }).collect()
                            };
                            let mut guard = ws_clone.lock().await;
                            let _ = guard.send(Message::Text(
                                serde_json::json!({"type":"panes","panes":panes}).to_string()
                            )).await;
                        }
                        "ping" => {
                            let mut guard = ws_clone.lock().await;
                            let _ = guard.send(Message::Text(
                                serde_json::json!({"type":"pong"}).to_string()
                            )).await;
                        }
                        _ => {}
                    }
                }
                Message::Ping(p) => {
                    let mut guard = ws_clone.lock().await;
                    let _ = guard.send(Message::Pong(p)).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let _ = reader.await;
    drop(msg_tx);
    let _ = writer.await;
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

// ── Inline HTML pages ──

const LOGIN_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ridge Remote — Login</title>
<style>
  *{margin:0;padding:0;box-sizing:border-box}
  body{background:#0d1117;color:#e6edf3;font-family:system-ui,sans-serif;display:flex;min-height:100vh;align-items:center;justify-content:center}
  .card{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:32px;max-width:400px;width:90%}
  h1{font-size:20px;margin-bottom:8px}
  p{font-size:13px;color:#8b949e;margin-bottom:20px}
  label{font-size:13px;display:block;margin-bottom:6px}
  input{width:100%;padding:10px 12px;background:#0d1117;border:1px solid #30363d;border-radius:6px;color:#e6edf3;font-size:18px;font-family:monospace;text-align:center;letter-spacing:4px}
  input:focus{outline:none;border-color:#d29922}
  button{width:100%;margin-top:16px;padding:10px;background:#d29922;color:#0d1117;border:none;border-radius:6px;font-weight:600;font-size:14px;cursor:pointer}
  button:hover{background:#c6901f}
  .error{color:#f85149;font-size:13px;margin-top:12px;display:none}
  .info{font-size:12px;color:#8b949e;margin-top:16px;text-align:center}
</style>
</head>
<body>
<div class="card">
  <h1>Ridge Remote</h1>
  <p>Enter the verification code from your Ridge CLI dashboard.</p>
  <form id="login-form" method="post" action="/verify">
    <label for="code">TOTP Verification Code</label>
    <input type="text" id="code" name="code" maxlength="6" inputmode="numeric" pattern="[0-9]*" autofocus required>
    <button type="submit">Verify &amp; Connect</button>
    <p class="error" id="error">Invalid code. Please try again.</p>
  </form>
  <div class="info">Code refreshes every 30 seconds.</div>
</div>
<script>
  (function() {
    const params = new URLSearchParams(location.search);
    if (params.get('error') === '1') {
      document.getElementById('error').style.display = 'block';
    }
  })();
</script>
</body>
</html>"#;

const TERMINAL_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ridge Remote — Terminal</title>
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/xterm@5.3.0/css/xterm.min.css">
<style>
  *{margin:0;padding:0;box-sizing:border-box}
  body{background:#0d1117;color:#e6edf3;font-family:system-ui,sans-serif;display:flex;flex-direction:column;height:100vh}
  #toolbar{display:flex;align-items:center;gap:12px;padding:8px 16px;background:#161b22;border-bottom:1px solid #30363d}
  #toolbar .title{font-weight:600;font-size:14px}
  #toolbar .status{font-size:12px;color:#8b949e;margin-left:auto}
  #toolbar .status.connected{color:#7fb069}
  #status-bar{display:flex;align-items:center;gap:12px;padding:4px 16px;background:#0d1117;border-bottom:1px solid #30363d;font-size:12px;color:#8b949e}
  #terminal{flex:1;padding:4px}
</style>
</head>
<body>
<div id="toolbar">
  <span class="title">Ridge Remote</span>
  <span class="status" id="conn-status">Disconnected</span>
</div>
<div id="status-bar">
  <span id="lan-ip"></span>
  <span id="totp-code" style="font-family:monospace;font-weight:600;color:#d29922"></span>
</div>
<div id="terminal"></div>
<script src="https://cdn.jsdelivr.net/npm/xterm@5.3.0/lib/xterm.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/xterm-addon-fit@0.8.0/lib/xterm-addon-fit.min.js"></script>
<script>
(async()=>{
  const term = new Terminal({cursorBlink:true,fontSize:14,theme:{background:'#0d1117',foreground:'#e6edf3',cursor:'#e6edf3'}});
  const fit = new FitAddon.FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById('terminal'));
  fit.fit();

  const statusEl = document.getElementById('conn-status');
  const lanIpEl = document.getElementById('lan-ip');
  const totpEl = document.getElementById('totp-code');

  const info = await (await fetch('/info')).json();
  lanIpEl.textContent = '\u{1F310} ' + info.lanIp + ':' + info.port;
  totpEl.textContent = 'TOTP: ' + info.totpCode;

  const params = new URLSearchParams(location.search);
  const code = params.get('code') || '';
  const protocol = location.protocol === 'https:' ? 'wss' : 'ws';
  const url = protocol + '://' + location.host + '/ws?code=' + encodeURIComponent(code);
  const ws = new WebSocket(url);
  statusEl.textContent = 'Connecting...';
  statusEl.className = 'status';

  ws.onopen = () => {
    statusEl.textContent = 'Connected';
    statusEl.className = 'status connected';
  };
  ws.onclose = () => {
    statusEl.textContent = 'Disconnected';
    statusEl.className = 'status';
  };
  ws.onmessage = (e) => {
    if (e.data instanceof Blob) {
      e.data.arrayBuffer().then(buf => {
        const paneId = new TextDecoder().decode(new Uint8Array(buf, 0, 16));
        const data = new Uint8Array(buf, 16);
        term.write(new TextDecoder().decode(data));
      });
    } else {
      const msg = JSON.parse(e.data);
      if (msg.type === 'panes' && msg.panes?.length) {
        const pane = msg.panes[0];
        ws.send(JSON.stringify({type:'subscribe-pane',paneId:pane.id}));
        ws.send(JSON.stringify({type:'claim-pane',paneId:pane.id,rows:term.rows,cols:term.cols,uuid:'cli-browser',seq:0}));
      }
    }
  };

  term.onData(data => {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({type:'stdin',data:data}));
    }
  });

  window.addEventListener('resize', () => {
    fit.fit();
    const pid = currentPaneId;
    if (pid && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({type:'claim-pane',paneId:pid,rows:term.rows,cols:term.cols,uuid:'cli-browser',seq:0}));
    }
  });
})();
</script>
</body>
</html>"#;
