// Standalone Ridge Remote Server
//
// Usage (dev):   cargo run --bin remote-server
// Usage (build): ./target/release/remote-server.exe
//
// Serves the built mobile app from <exe-dir>/static/mobile/ (or ./static/mobile/)
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
use tokio::net::TcpListener;

mod auth {
    // Re-implement a minimal subset of the remote auth for the standalone binary.
    // The full auth lives in ridge_lib::remote::auth.
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    const TOTP_PERIOD: u64 = 30;
    const TOTP_DIGITS: u64 = 6;
    const TOTP_SKEW: i64 = 1;

    pub struct RemoteAuth {
        secret: Vec<u8>,
    }

    impl RemoteAuth {
        pub fn new() -> Self {
            Self {
                secret: generate_secret(),
            }
        }

        pub fn current_code(&self) -> String {
            let now = now_secs();
            totp_at(&self.secret, now)
        }

        pub fn verify(&self, code: &str) -> bool {
            if code.len() != TOTP_DIGITS as usize {
                return false;
            }
            let now = now_secs();
            for offset in -TOTP_SKEW..=TOTP_SKEW {
                let ts = if offset >= 0 {
                    now.saturating_add(offset as u64)
                } else {
                    now.saturating_sub((-offset) as u64)
                };
                if constant_time_eq(totp_at(&self.secret, ts).as_bytes(), code.as_bytes()) {
                    return true;
                }
            }
            false
        }

        pub fn code_and_uri(&self) -> (String, String) {
            (self.current_code(), self.otpauth_uri())
        }

        pub fn otpauth_uri(&self) -> String {
            format!(
                "otpauth://totp/Ridge:remote?secret={}&issuer=Ridge&algorithm=SHA256&digits={}&period={}",
                base32_encode(&self.secret),
                TOTP_DIGITS,
                TOTP_PERIOD,
            )
        }
    }

    fn generate_secret() -> Vec<u8> {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let mut rng = SimpleRng::new(seed as u64 ^ pid as u64);
        let mut buf = vec![0u8; 20];
        rng.fill(&mut buf);
        buf
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    fn totp_at(secret: &[u8], time_secs: u64) -> String {
        let counter = time_secs / TOTP_PERIOD;
        let counter_be = counter.to_be_bytes();
        let hmac_result = hmac_sha256(secret, &counter_be);
        let offset = (hmac_result[31] & 0x0f) as usize;
        let code = ((hmac_result[offset] & 0x7f) as u32) << 24
            | (hmac_result[offset + 1] as u32) << 16
            | (hmac_result[offset + 2] as u32) << 8
            | (hmac_result[offset + 3] as u32);
        let mod_val = 10u32.pow(TOTP_DIGITS as u32);
        let token = code % mod_val;
        format!("{:0width$}", token, width = TOTP_DIGITS as usize)
    }

    fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
        const BLOCK_SIZE: usize = 64;
        let mut k = key.to_vec();
        if k.len() > BLOCK_SIZE {
            k = Sha256::digest(&k).to_vec();
        }
        k.resize(BLOCK_SIZE, 0);
        let mut ipad = vec![0x36u8; BLOCK_SIZE];
        let mut opad = vec![0x5cu8; BLOCK_SIZE];
        for i in 0..k.len() {
            ipad[i] ^= k[i];
            opad[i] ^= k[i];
        }
        let inner = Sha256::digest(&[&ipad[..], msg].concat());
        Sha256::digest(&[&opad[..], &inner[..]].concat()).to_vec()
    }

    fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut result = 0u8;
        for (x, y) in a.iter().zip(b.iter()) {
            result |= x ^ y;
        }
        result == 0
    }

    fn base32_encode(input: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        let mut result = String::new();
        let mut buffer: u64 = 0;
        let mut bits = 0;
        for &byte in input {
            buffer = (buffer << 8) | byte as u64;
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                let idx = ((buffer >> bits) & 0x1f) as usize;
                result.push(ALPHABET[idx] as char);
            }
        }
        if bits > 0 {
            let idx = ((buffer << (5 - bits)) & 0x1f) as usize;
            result.push(ALPHABET[idx] as char);
        }
        result
    }

    struct SimpleRng {
        state: u64,
    }

    impl SimpleRng {
        fn new(seed: u64) -> Self {
            Self {
                state: seed.wrapping_add(0x9e3779b97f4a7c15),
            }
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x.wrapping_shl(13);
            x ^= x.wrapping_shr(7);
            x ^= x.wrapping_shl(17);
            self.state = x;
            x.wrapping_mul(0x9e3779b97f4a7c15)
        }
        fn fill(&mut self, buf: &mut [u8]) {
            for chunk in buf.chunks_mut(8) {
                let val = self.next_u64().to_le_bytes();
                for (d, s) in chunk.iter_mut().zip(val.iter()) {
                    *d = *s;
                }
            }
        }
    }
}

// ── State ──

#[derive(Clone)]
struct AppCtx {
    port: u16,
    lan_ip: String,
    auth: Arc<auth::RemoteAuth>,
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

// ── Detect LAN IP ──

fn detect_lan_ip() -> String {
    use std::net::ToSocketAddrs;
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("1.1.1.1:53").is_ok() {
            if let Ok(local) = socket.local_addr() {
                let ip = local.ip();
                if ip.is_ipv4() && !ip.is_loopback() {
                    return ip.to_string();
                }
            }
        }
    }
    let compname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string());
    if let Ok(addrs) = (compname.as_str(), 0u16).to_socket_addrs() {
        for addr in addrs {
            let ip = addr.ip();
            if ip.is_ipv4() && !ip.is_loopback() {
                return ip.to_string();
            }
        }
    }
    "localhost".to_string()
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
    let (code, uri) = ctx.auth.code_and_uri();
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

    // Resolve static files directory: prefers cwd/static/mobile/ (dev),
    // then exe-relative static/mobile/ (installed), then fallback.
    let static_dir = PathBuf::from("static").join("mobile");
    let static_dir = if static_dir.exists() {
        static_dir
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("static").join("mobile")))
            .unwrap_or(static_dir)
    };

    println!("  Static dir: {}", static_dir.display());

    let auth = Arc::new(auth::RemoteAuth::new());
    let lan_ip = detect_lan_ip();

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
