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
    /// Full desktop SPA build (`web-remote-dist/`, from `pnpm build:desktop-web`),
    /// served to desktop browsers (UA-forked). Empty/missing → desktop UA falls
    /// back to the mobile SPA in `static_dir`.
    desktop_dir: PathBuf,
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
    ready: bool,
    machine_name: String,
    // SECURITY: the `otpauth://` URI embeds the raw TOTP *secret seed*. It is
    // intentionally NOT exposed over this HTTP endpoint — anyone who can reach
    // `/info` (pre-auth, on the LAN) could otherwise derive every future code,
    // defeating TOTP. The desktop pairing QR reads the URI in-process via the
    // `get_remote_info` Tauri command (see RemotePanel.svelte), never over HTTP.
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

    // Desktop SPA build (`web-remote-dist/`), resolved like static_dir. Lives
    // OUTSIDE `static/` so the SvelteKit static-adapter doesn't recurse.
    let desktop_dir = {
        let candidates: Vec<PathBuf> = vec![
            PathBuf::from("web-remote-dist"),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("web-remote-dist")))
                .unwrap_or_default(),
            std::env::current_exe()
                .ok()
                .and_then(|p| {
                    p.parent()?
                        .parent()?
                        .parent()?
                        .parent()?
                        .join("web-remote-dist")
                        .into()
                })
                .unwrap_or_default(),
        ];
        candidates
            .into_iter()
            .find(|p| p.join("index.html").exists())
            .unwrap_or_else(|| PathBuf::from("web-remote-dist"))
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
        desktop_dir,
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
        // Host file bytes for the desktop UI's convertFileSrc shim (token-auth'd).
        .route("/file", get(file_handler))
        // Local-CA download for the verification page's "trust this device"
        // flow — public (no token): the CA is public key material, and the
        // user needs it *before* authenticating to silence the warning.
        .route("/ridge-ca.crt", get(ca_crt_handler))
        .route("/ridge-ca.pem", get(ca_pem_handler))
        .route("/workspace/list", get(workspace_list_handler))
        .route("/workspace/switch", post(workspace_switch_handler))
        .route("/workspace/create", post(workspace_create_handler))
        .route("/workspace/close", post(workspace_close_handler))
        // PWA + SPA fallback: serve root-level static files emitted by the
        // remote build (sw.js, manifest.webmanifest, icons, …) and fall back to
        // index.html for client-side routes. Self-gates on `remote_enabled`
        // because `route_layer` middleware does not wrap the fallback.
        .fallback(spa_fallback_handler)
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

#[derive(Deserialize)]
struct UiQuery {
    /// Manual override for the UA fork (`?ui=desktop` / `?ui=mobile`), for
    /// testing and edge browsers.
    ui: Option<String>,
}

/// Decide which UI build to serve: the FULL desktop SPA for desktop browsers,
/// the lightweight mobile SPA otherwise. `?ui=` overrides. Falls back to the
/// mobile build if the desktop build (`web-remote-dist`) isn't present.
fn wants_desktop_ui(
    ctx: &RemoteCtx,
    headers: &axum::http::HeaderMap,
    ui_override: Option<&str>,
) -> bool {
    let prefer_desktop = match ui_override {
        Some("desktop") => true,
        Some("mobile") => false,
        _ => {
            let ua = headers
                .get(axum::http::header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            const MOBILE: [&str; 6] = [
                "android",
                "iphone",
                "ipad",
                "ipod",
                "mobile",
                "windows phone",
            ];
            !MOBILE.iter().any(|m| ua.contains(m))
        }
    };
    prefer_desktop && ctx.desktop_dir.join("index.html").exists()
}

/// The UI build directory for this request (desktop vs mobile).
fn ui_dir<'a>(
    ctx: &'a RemoteCtx,
    headers: &axum::http::HeaderMap,
    ui_override: Option<&str>,
) -> &'a PathBuf {
    if wants_desktop_ui(ctx, headers, ui_override) {
        &ctx.desktop_dir
    } else {
        &ctx.static_dir
    }
}

async fn root_handler(
    State(ctx): State<RemoteCtx>,
    headers: axum::http::HeaderMap,
    Query(q): Query<UiQuery>,
) -> impl IntoResponse {
    serve_index(ui_dir(&ctx, &headers, q.ui.as_deref())).await
}

/// Serve `index.html` with `Cache-Control: no-cache` so a freshly deployed
/// build (new hashed asset names, new service worker) is always picked up on
/// the next visit instead of being pinned by a stale cached shell.
async fn serve_index(dir: &std::path::Path) -> axum::response::Response {
    let index_path = dir.join("index.html");
    match tokio::fs::read(&index_path).await {
        Ok(bytes) => axum::response::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(axum::http::header::CACHE_CONTROL, "no-cache")
            .body(axum::body::Body::from(bytes))
            .unwrap(),
        Err(_) => {
            // Fallback: embed a basic page directing the user to build the remote app
            Html(
                r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Ridge Remote</title></head><body style="background:#0d1117;color:#e6edf3;font-family:sans-serif;display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;margin:0"><h1>Ridge Remote</h1><p>Remote UI not built yet.</p><p>Run: <code>pnpm build:remote</code></p></body></html>"#,
            )
            .into_response()
        }
    }
}

/// Serve the local root CA as a DER `.crt` — the form iOS / Android / Windows
/// expect when installing a certificate as a trust anchor.
///
/// Deliberately no `Content-Disposition: attachment`: on iOS the bare
/// `application/x-x509-ca-cert` response triggers the "install configuration
/// profile" prompt, whereas forcing a download can route it to Files instead.
/// Android / desktop get a sensible filename from the `.crt` URL and the
/// front-end anchor's `download` attribute (see CertTrustGuide.svelte).
async fn ca_crt_handler() -> impl IntoResponse {
    match super::tls::ca_cert_der() {
        Some(der) => axum::response::Response::builder()
            .header(
                axum::http::header::CONTENT_TYPE,
                "application/x-x509-ca-cert",
            )
            .header(axum::http::header::CACHE_CONTROL, "no-store")
            .body(axum::body::Body::from(der))
            .unwrap(),
        None => (StatusCode::NOT_FOUND, "no CA certificate available").into_response(),
    }
}

/// Serve the local root CA as PEM — for desktop trust-store import.
async fn ca_pem_handler() -> impl IntoResponse {
    match super::tls::ca_cert_pem() {
        Some(pem) => axum::response::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "application/x-pem-file")
            .header(
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"ridge-remote-ca.pem\"",
            )
            .header(axum::http::header::CACHE_CONTROL, "no-store")
            .body(axum::body::Body::from(pem))
            .unwrap(),
        None => (StatusCode::NOT_FOUND, "no CA certificate available").into_response(),
    }
}

/// Fallback for any unmatched GET: serve a root-level static file from the
/// build output (service worker, web manifest, PWA icons, favicon, …), or fall
/// back to the SPA shell. Path-traversal-guarded and gated on `remote_enabled`.
async fn spa_fallback_handler(
    State(ctx): State<RemoteCtx>,
    headers: axum::http::HeaderMap,
    Query(q): Query<UiQuery>,
    uri: axum::http::Uri,
) -> axum::response::Response {
    if !ctx.state.remote_enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Remote control disabled").into_response();
    }

    // §UA fork: a desktop browser resolves root-level files (and the SvelteKit
    // `_app/*` bundle) against the desktop build; the mobile SPA against
    // static_dir. The chosen dir is also the SPA shell for unknown client routes.
    let base = ui_dir(&ctx, &headers, q.ui.as_deref());

    // axum percent-decodes `uri.path()` before we see it, so a `%2e%2e`
    // traversal arrives as a literal `..`. First-line string guard rejects the
    // obvious escapes (`..`, drive-absolute `C:\`, leading `/`, backslashes)…
    let rel = uri.path().trim_start_matches('/');
    let safe = !rel.is_empty()
        && !rel.contains("..")
        && !rel.contains('\\')
        && !rel.contains(':')
        && !rel.starts_with('/');
    if !safe {
        return serve_index(base).await;
    }

    // …then a canonical-path containment check is the authoritative guard: the
    // resolved target (symlinks + `.` segments collapsed) must live inside the
    // chosen UI dir. `canonicalize` fails for non-existent paths, which naturally
    // routes unknown SPA client-side routes to the shell.
    let candidate = base.join(rel);
    let within = match (
        tokio::fs::canonicalize(&candidate).await,
        tokio::fs::canonicalize(base).await,
    ) {
        (Ok(real), Ok(root)) => real.starts_with(&root).then_some(real),
        _ => None,
    };
    let Some(real) = within else {
        return serve_index(base).await;
    };

    match tokio::fs::read(&real).await {
        Ok(bytes) => {
            // SvelteKit emits content-hashed bundles under `_app/immutable/` —
            // safe to cache forever; everything else revalidates.
            let (content_type, cache_control) = if rel.starts_with("_app/immutable/") {
                if rel.ends_with(".css") {
                    ("text/css", "max-age=31536000, immutable")
                } else if rel.ends_with(".wasm") {
                    ("application/wasm", "max-age=31536000, immutable")
                } else {
                    ("application/javascript", "max-age=31536000, immutable")
                }
            } else {
                root_asset_headers(rel)
            };
            axum::response::Response::builder()
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .header(axum::http::header::CACHE_CONTROL, cache_control)
                .body(axum::body::Body::from(bytes))
                .unwrap()
        }
        Err(_) => serve_index(base).await,
    }
}

/// Serve a single host file by absolute path for the desktop UI's
/// `convertFileSrc` shim (Markdown preview images, etc.). Token-authenticated
/// via query param (an `<img src>` can't set an Authorization header) and
/// traversal-guarded. Only reachable while remote control is enabled
/// (route_layer gate).
#[derive(Deserialize)]
struct FileQuery {
    path: String,
    token: Option<String>,
}

async fn file_handler(
    State(ctx): State<RemoteCtx>,
    Query(q): Query<FileQuery>,
) -> impl IntoResponse {
    let authed = q
        .token
        .as_deref()
        .map(|t| ctx.state.remote_session_store.validate_token(t))
        .unwrap_or(false);
    if !authed {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    if q.path.split(['/', '\\']).any(|c| c == "..") {
        return (StatusCode::BAD_REQUEST, "bad path").into_response();
    }
    let full = PathBuf::from(&q.path);
    if !full.is_file() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    match tokio::fs::read(&full).await {
        Ok(bytes) => {
            let name = full.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let (content_type, _) = root_asset_headers(name);
            axum::response::Response::builder()
                .header(axum::http::header::CONTENT_TYPE, content_type)
                .header(axum::http::header::CACHE_CONTROL, "private, max-age=60")
                .body(axum::body::Body::from(bytes))
                .unwrap()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "read error").into_response(),
    }
}

/// Content-type + cache policy for root-level build artifacts. The service
/// worker and manifest must revalidate (`no-cache`) so new versions are
/// detected; immutable hashed bundles live under `/assets` (see `assets_handler`).
fn root_asset_headers(path: &str) -> (&'static str, &'static str) {
    if path == "sw.js" || path.ends_with("/sw.js") {
        ("application/javascript", "no-cache")
    } else if path.ends_with(".webmanifest") {
        ("application/manifest+json", "no-cache")
    } else if path.ends_with(".js") {
        ("application/javascript", "no-cache")
    } else if path.ends_with(".json") {
        ("application/json", "no-cache")
    } else if path.ends_with(".css") {
        ("text/css", "max-age=86400")
    } else if path.ends_with(".png") {
        ("image/png", "max-age=86400")
    } else if path.ends_with(".svg") {
        ("image/svg+xml", "max-age=86400")
    } else if path.ends_with(".ico") {
        ("image/x-icon", "max-age=86400")
    } else if path.ends_with(".webp") {
        ("image/webp", "max-age=86400")
    } else if path.ends_with(".wasm") {
        ("application/wasm", "max-age=86400")
    } else {
        ("application/octet-stream", "max-age=3600")
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
    // NOTE: do NOT include the otpauth URI / TOTP secret here (see InfoResponse).
    Json(InfoResponse {
        port: ctx.port,
        lan_ip: ctx.lan_ip.clone(),
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
    // The /verify page is the mobile auth flow — always the mobile SPA shell.
    // (The desktop SPA carries its own auth gate in +layout.svelte.)
    serve_index(&ctx.static_dir).await
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
                pty_generation: HashMap::new(),
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

    // Track which (ws, pane) this client is currently subscribed to. The mobile
    // SPA views ONE pane at a time, so `current_pane` is a single slot that
    // subscribe-pane replaces. The desktop UI in a browser shows SPLITS (many
    // panes at once); when `use_global_ws` is set it keeps every subscribed pane
    // live in `subscribed_panes` instead (raw frames are pane-prefixed, so the
    // client demuxes them). Both are unregistered on workspace change / disconnect.
    let mut current_pane: Option<(Uuid, Uuid)> = None;
    let mut subscribed_panes: std::collections::HashSet<(Uuid, Uuid)> =
        std::collections::HashSet::new();

    // Client-reported viewport grid dimensions, updated by the `resize` WS
    // message. Used for the first-connect auto-claim and the refresh button.
    let mut mobile_rows: u16 = 24;
    let mut mobile_cols: u16 = 80;

    // Subscribe to structural change broadcasts (pane/workspace add/close/rename)
    // so this client can push updated lists to the remote frontend without polling.
    let mut structural_rx = ctx.state.remote_structural_tx.subscribe();
    // §web-remote: generic host → desktop-browser event relay (fs-changed, …).
    // The mobile SPA ignores `{type:'event'}` frames, so this is harmless to it.
    let mut ui_event_rx = ctx.state.remote_ui_event_tx.subscribe();
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

    // §web-remote global-workspace mode. The desktop-UI-in-browser client is a
    // second *peer desktop*: it switches workspaces through the real global
    // `switch_workspace` command (invoke-request), not the per-client WS
    // `switch-workspace` message. So when this flag is set (the client sends
    // `use-global-workspace` right after connect), `active_ws_id` is kept in sync
    // with the GLOBAL active workspace at the top of every loop iteration — which
    // runs before any incoming message/event is handled (incl. the subscribe-pane
    // that follows a switch), so all readers below see the right workspace.
    // Mobile clients never set it and keep their independent per-client view.
    let mut use_global_ws = false;

    // §S3 $/cancel registry (JSON-RPC leg only). Invokes on a single connection
    // are processed serially inside this loop, so there is no concurrent in-flight
    // request to abort mid-flight on the same socket. This set records ids the
    // client asked to cancel; a request whose id was pre-cancelled (a client that
    // pipelines `$/cancel` ahead of, or racing, its request) is short-circuited
    // with a "cancelled" error and never runs the backing command. Long tasks that
    // cannot be interrupted simply run to completion — the guard guarantees we
    // never crash and never send a stale result for a cancelled id. Bounded so a
    // hostile client cannot grow it without limit.
    let mut cancelled_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
    const MAX_CANCELLED_IDS: usize = 1024;

    // Periodic health check: if remote control is toggled off, or this client
    // is force-disconnected / blacklisted (kill_flag), close the WS so the
    // mobile client gets a clean disconnect. Polled at 1s so an admin-triggered
    // disconnect takes effect promptly (just an atomic load per tick).
    let mut health_interval = tokio::time::interval(std::time::Duration::from_secs(1));
    health_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Message loop: forward PTY output to WS client, relay keystrokes back.
    loop {
        // §web-remote: keep `active_ws_id` mirrored to the global active workspace
        // for desktop-browser peers. Runs whenever the loop iterates (i.e. before
        // handling each message/event), so a global switch via invoke-request is
        // reflected before the browser's following subscribe-pane is processed.
        if use_global_ws {
            let g = ctx.state.active_workspace_id();
            if g != active_ws_id {
                // Drop the stale subscriptions; the browser re-subscribes the new
                // workspace's panes on its own re-render. Their bytes are filtered
                // by `workspace_id == active_ws_id`, so nothing leaks across.
                for (ws, p) in subscribed_panes.drain() {
                    ctx.state.unregister_remote_sub(ws, p, sub_id);
                }
                if let Some((ws, p)) = current_pane.take() {
                    ctx.state.unregister_remote_sub(ws, p, sub_id);
                }
                active_ws_id = g;
            }
        }
        tokio::select! {
                    msg = ws_rx.next() => {
                        let Some(Ok(Message::Text(text))) = msg else {
                            break;
                        };
                        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };

                        // §S3 JSON-RPC 2.0 leg (additive). A frame is JSON-RPC iff
                        // `jsonrpc == "2.0"`; otherwise it is a legacy control frame and
                        // falls straight through to the `match parsed["type"]` below
                        // (old clients are byte-for-byte unchanged). JSON-RPC requests and
                        // `$/`-methods are fully handled here and `continue`; JSON-RPC
                        // notifications for ordinary control methods are normalised into
                        // the legacy `{type, …params}` shape and DELIBERATELY fall through
                        // so they reuse the exact same handlers (no duplicated logic).
                        let parsed = if parsed.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0") {
                            let method = parsed.get("method").and_then(|m| m.as_str()).map(String::from);
                            let has_id = parsed.get("id").map(|i| !i.is_null()).unwrap_or(false);
                            let empty = serde_json::json!({});
                            let params = parsed.get("params").unwrap_or(&empty).clone();

                            match (method.as_deref(), has_id) {
                                // ── D9 version + capability handshake ──
                                (Some("$/hello"), _) => {
                                    let reply = negotiate_hello(&params);
                                    let _ = ws_tx.send(Message::Text(reply.to_string())).await;
                                    continue;
                                }
                                // ── $/cancel: register the target id (notification, no id) ──
                                (Some("$/cancel"), _) => {
                                    if let Some(target) = params.get("id").and_then(|i| i.as_u64()) {
                                        if cancelled_ids.len() < MAX_CANCELLED_IDS {
                                            cancelled_ids.insert(target);
                                        }
                                        tracing::debug!(target: "ridge::remote", target, "received $/cancel");
                                    }
                                    continue;
                                }
                                // ── JSON-RPC request (has id) → dispatch + JSON-RPC reply ──
                                (Some(m), true) => {
                                    let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                    let id_u64 = id.as_u64();

                                    // §rate-limit: shared token bucket with the legacy leg.
                                    if dr_window_start.elapsed() >= DR_WINDOW {
                                        dr_window_start = Instant::now();
                                        dr_count = 0;
                                    }
                                    dr_count += 1;
                                    if dr_count > DR_MAX_PER_WINDOW {
                                        tracing::warn!(target: "ridge::remote", client_id, cmd = %m, "invoke (json-rpc) rate limit exceeded; rejecting");
                                        let err = serde_json::json!({
                                            "code": JSON_RPC_INTERNAL_ERROR,
                                            "message": "rate limited: too many invoke requests",
                                            "data": { "kind": "rate_limited" },
                                        });
                                        let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                        continue;
                                    }

                                    // §cancel: a request whose id was already cancelled never runs.
                                    if let Some(uid) = id_u64 {
                                        if cancelled_ids.remove(&uid) {
                                            let err = serde_json::json!({
                                                "code": JSON_RPC_INTERNAL_ERROR,
                                                "message": "request cancelled",
                                                "data": { "kind": "cancelled" },
                                            });
                                            let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                            continue;
                                        }
                                    }

                                    let reply = match dispatch_invoke_jsonrpc(m, &params, &ctx.state).await {
                                        Ok(result) => jsonrpc_result(&id, result),
                                        Err(error) => jsonrpc_error(&id, error),
                                    };
                                    // §cancel: if the client cancelled while the (serial) dispatch
                                    // was running, drop the stale result instead of sending it.
                                    let cancelled = id_u64.map(|uid| cancelled_ids.remove(&uid)).unwrap_or(false);
                                    if !cancelled {
                                        let _ = ws_tx.send(Message::Text(reply.to_string())).await;
                                    }
                                    continue;
                                }
                                // ── JSON-RPC notification (no id) → reuse legacy handlers ──
                                (Some(m), false) => {
                                    // Normalise `{jsonrpc, method, params}` → `{type: method,
                                    // …params}` so the legacy `match` below handles it once.
                                    let mut flat = match params {
                                        serde_json::Value::Object(map) => map,
                                        _ => serde_json::Map::new(),
                                    };
                                    flat.insert("type".to_string(), serde_json::json!(m));
                                    serde_json::Value::Object(flat)
                                }
                                // Malformed JSON-RPC (no method): reply error if it had an id.
                                (None, _) => {
                                    if has_id {
                                        let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                        let err = serde_json::json!({
                                            "code": JSON_RPC_INVALID_REQUEST,
                                            "message": "missing method",
                                            "data": { "kind": "invalid_request" },
                                        });
                                        let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                    }
                                    continue;
                                }
                            }
                        } else {
                            parsed
                        };

                        let _ = match parsed["type"].as_str() {
                            Some("ping") => {
                                ws_tx.send(Message::Text(serde_json::json!({"type":"pong"}).to_string())).await
                            }
                            Some("use-global-workspace") => {
                                // §web-remote: desktop-browser peer opts into global
                                // workspace semantics (see use_global_ws above). Seed
                                // immediately so the first list/subscribe is correct.
                                use_global_ws = true;
                                active_ws_id = ctx.state.active_workspace_id();
                                Ok(())
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
                                    let new_key = (active_ws_id, pane_id);
                                    // Mobile: single-pane view — replace the previous
                                    // subscription. Desktop (global): keep every split
                                    // pane subscribed; register each pane once.
                                    let do_register = if use_global_ws {
                                        subscribed_panes.insert(new_key)
                                    } else {
                                        if let Some((ws, p)) = current_pane.take() {
                                            ctx.state.unregister_remote_sub(ws, p, sub_id);
                                        }
                                        current_pane = Some(new_key);
                                        true
                                    };
                                    if !do_register {
                                        // Already subscribed (idempotent re-subscribe) —
                                        // skip re-registering and re-sending scrollback.
                                        continue;
                                    }

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

                                    // §D10 接入点 (integration point) — S5 per-pane screen
                                    // buffer: emit a `PaneSnapshotFrame` (rendered screen +
                                    // locked size) HERE, before the raw scrollback below, so a
                                    // late/reconnecting controller repaints exact terminal state
                                    // (cursor/alt-screen/scroll-region) ahead of the live raw
                                    // stream. See the D10 SCAFFOLD block near `PaneSnapshotFrame`.
                                    // Today we ship raw scrollback only (a working precursor).
                                    //
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
                                        pty_generation: HashMap::new(),
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
                            Some("invoke-request") => {
                                // Backs the browser-side Tauri `invoke()` shim
                                // (src/lib/transport/tauriShim/core.ts) used when the FULL
                                // desktop UI is served to a desktop browser. Same trust
                                // boundary, rate limit, read-only gate, traversal guard and
                                // audit as `data-request`; the explicit allowlist in
                                // `dispatch_invoke_request` is the security boundary (unknown
                                // cmd → error), so host-privileged / remote-admin commands
                                // (get_remote_info, set_remote_enabled, deep-root, …) stay
                                // unreachable. The reply carries `type:'invoke-result'` so it
                                // survives RemoteConnection's onmessage routing.
                                let req_id = parsed["_reqId"].as_u64().unwrap_or(0);
                                let cmd = parsed["cmd"].as_str().unwrap_or("").to_string();
                                if dr_window_start.elapsed() >= DR_WINDOW {
                                    dr_window_start = Instant::now();
                                    dr_count = 0;
                                }
                                dr_count += 1;
                                if dr_count > DR_MAX_PER_WINDOW {
                                    tracing::warn!(target: "ridge::remote", client_id, cmd = %cmd, "invoke-request rate limit exceeded; rejecting");
                                    let reply = serde_json::json!({
                                        "type": "invoke-result", "_reqId": req_id,
                                        "_error": "rate limited: too many invoke requests",
                                    });
                                    ws_tx.send(Message::Text(reply.to_string())).await
                                } else {
                                    let empty = serde_json::json!({});
                                    let args = parsed.get("args").unwrap_or(&empty);
                                    let mut reply = dispatch_invoke_request(&cmd, args, &ctx.state).await;
                                    if let Some(obj) = reply.as_object_mut() {
                                        obj.insert("_reqId".to_string(), serde_json::json!(req_id));
                                        obj.insert("type".to_string(), serde_json::json!("invoke-result"));
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
                                        "cwd": cwd.clone(),
                                    }).to_string())).await;
                                    // §web-remote: desktop UI listens to pane-cwd-changed-{ws}-{pane}.
                                    let _ = ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "event",
                                        "name": format!("pane-cwd-changed-{}-{}", workspace_id, pane_id),
                                        "payload": { "cwd": cwd },
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
                                    // §web-remote: desktop UI re-syncs layout on pane-tree-changed.
                                    let _ = ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "event",
                                        "name": "pane-tree-changed",
                                        "payload": { "workspaceId": active_ws_id.to_string() },
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
                                // §web-remote: desktop UI refreshes on workspace-list-changed.
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "event", "name": "workspace-list-changed", "payload": {},
                                }).to_string())).await;
                            }
                            Ok(crate::types::RemoteStructuralEvent::WorkspaceRenamed { workspace_id, name }) => {
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "workspace-renamed",
                                    "workspaceId": workspace_id.to_string(),
                                    "name": name,
                                }).to_string())).await;
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "event", "name": "workspace-list-changed", "payload": {},
                                }).to_string())).await;
                            }
                            Err(_) => {
                                // Lagged — skip; the next request-response cycle will fix it.
                            }
                        }
                    }
                    ui_evt = ui_event_rx.recv() => {
                        // §web-remote: relay a host Tauri event to the desktop browser's
                        // `listen()` shim. Broadcast to every client; the mobile SPA drops it.
                        //
                        // §S3 backpressure + coalesce (§5.2 / R8): `git checkout`,
                        // dependency installs etc. produce event STORMS (many
                        // `fs-changed`/scm-refresh in a tick). The broadcast bus is already
                        // bounded (capacity 256, drop-oldest on lag — the Lagged arm), but
                        // forwarding each event 1:1 to a slow WS client can still stall the
                        // socket. So after the first event we DRAIN everything currently
                        // queued without awaiting and COALESCE by event name (latest payload
                        // wins, insertion order preserved), collapsing a burst into one send
                        // per distinct event name. A slow client thus sees the *final* state,
                        // never an unbounded backlog.
                        if let Ok(first) = ui_evt {
                            // Insertion-ordered de-dup: name → payload, with a parallel order vec.
                            let mut order: Vec<String> = Vec::new();
                            let mut latest: std::collections::HashMap<String, serde_json::Value> =
                                std::collections::HashMap::new();
                            let mut push = |name: String, payload: serde_json::Value| {
                                if !latest.contains_key(&name) {
                                    order.push(name.clone());
                                }
                                latest.insert(name, payload);
                            };
                            push(first.name, first.payload);
                            // Drain everything already buffered (non-blocking); stop on empty.
                            // Bounded by the broadcast capacity (256), so this cannot spin.
                            loop {
                                match ui_event_rx.try_recv() {
                                    Ok(ev) => push(ev.name, ev.payload),
                                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                                        // Skipped some — the coalesced latest-state below still
                                        // converges the client; keep draining what remains.
                                        continue;
                                    }
                                    Err(_) => break, // Empty or Closed → done draining.
                                }
                            }
                            let mut send_failed = false;
                            for name in order {
                                if let Some(payload) = latest.remove(&name) {
                                    if ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "event",
                                        "name": name,
                                        "payload": payload,
                                    }).to_string())).await.is_err() {
                                        send_failed = true;
                                        break;
                                    }
                                }
                            }
                            if send_failed { break; }
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

    // Clean up: unregister from all subscribed panes (single-pane mobile slot +
    // the multi-pane desktop set).
    if let Some((ws, pane)) = current_pane.take() {
        ctx.state.unregister_remote_sub(ws, pane, sub_id);
    }
    for (ws, pane) in subscribed_panes.drain() {
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

/// FS/git-write commands gated by the remote read-only toggle. Structural
/// pane/workspace ops are interactive control (not filesystem writes), so they
/// stay allowed even in a read-only session.
fn is_mutating_invoke(cmd: &str) -> bool {
    is_mutating_method(cmd) || matches!(cmd, "replace_in_files" | "apply_file_edits")
}

/// Dispatches one browser `invoke-request` to the matching desktop command. The
/// real `#[tauri::command]` functions are called directly with a `State` /
/// `AppHandle` derived from the process-stashed handle (`AppState.app_handle`),
/// so behaviour is identical to the desktop IPC path. This is an ALLOWLIST:
/// unknown commands — including deliberately-excluded host-privileged ones
/// (`get_remote_info` exposes the live TOTP; `set_remote_enabled` /
/// `disconnect_session` / blacklist are remote-admin; `enter_deep_root_mode` /
/// `set_cloud_remote_active` / `summon_native_session` are host-only) — return
/// an error and never reach a handler.
async fn dispatch_invoke_request(
    cmd: &str,
    args: &serde_json::Value,
    state: &AppState,
) -> serde_json::Value {
    use crate::commands::{fs_watch, git, pane, project, ridge_file, terminal, watch, workspace};
    use tauri::Manager;

    // §read-only gate (defence-in-depth for view-only sessions).
    if is_mutating_invoke(cmd) {
        if state.remote_fs_readonly.load(Ordering::Relaxed) {
            tracing::warn!(target: "ridge::remote::fs", cmd, "rejected mutating invoke: remote is read-only");
            return serde_json::json!({ "_error": "remote filesystem is read-only" });
        }
        tracing::info!(target: "ridge::remote::fs", cmd, "remote mutating invoke");
    }
    // §traversal guard: reject `..` in any path-bearing field.
    for key in ["path", "from", "to", "repoRoot", "root", "cwd"] {
        if let Some(v) = args.get(key).and_then(|x| x.as_str()) {
            if path_has_traversal(v) {
                return serde_json::json!({ "_error": "path traversal rejected" });
            }
        }
    }
    if let Some(arr) = args.get("paths").and_then(|x| x.as_array()) {
        if arr
            .iter()
            .filter_map(|x| x.as_str())
            .any(path_has_traversal)
        {
            return serde_json::json!({ "_error": "path traversal rejected" });
        }
    }

    // ── arg extractors (frontend sends Tauri-style camelCase keys) ──
    fn s(v: &serde_json::Value, k: &str) -> String {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    }
    fn opt_s(v: &serde_json::Value, k: &str) -> Option<String> {
        v.get(k).and_then(|x| x.as_str()).map(String::from)
    }
    fn usize_opt(v: &serde_json::Value, k: &str) -> Option<usize> {
        v.get(k).and_then(|x| x.as_u64()).map(|n| n as usize)
    }
    fn usize_arg(v: &serde_json::Value, k: &str) -> usize {
        v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as usize
    }
    fn u16_opt(v: &serde_json::Value, k: &str) -> Option<u16> {
        v.get(k).and_then(|x| x.as_u64()).map(|n| n as u16)
    }
    fn u16_arg(v: &serde_json::Value, k: &str) -> u16 {
        v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u16
    }
    fn u32_arg(v: &serde_json::Value, k: &str) -> u32 {
        v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32
    }
    fn bool_opt(v: &serde_json::Value, k: &str) -> Option<bool> {
        v.get(k).and_then(|x| x.as_bool())
    }
    fn vec_s(v: &serde_json::Value, k: &str) -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn from_arg<T: serde::de::DeserializeOwned>(
        v: &serde_json::Value,
        k: &str,
    ) -> Result<T, String> {
        serde_json::from_value(v.get(k).cloned().unwrap_or(serde_json::Value::Null))
            .map_err(|e| e.to_string())
    }
    fn val<T: Serialize>(r: Result<T, String>) -> serde_json::Value {
        match r {
            Ok(v) => serde_json::json!({ "_result": v }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }
    fn unit(r: Result<(), String>) -> serde_json::Value {
        match r {
            Ok(()) => serde_json::json!({ "_result": null }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }
    fn plain<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::json!({ "_result": v })
    }
    // S1: map a `ridge_core::dispatch` result onto the legacy WS envelope.
    // `Ok(value)` → `{ "_result": value }`; `Err(core_err)` → `{ "_error":
    // message }` (the same human string the legacy handlers produced). The
    // structured JSON-RPC `{code,message,data}` object is reserved for the
    // JSON-RPC leg.
    //
    // §S3 (D-GM-2 resolved): the JSON-RPC leg now transmits the FULL structured
    // error. `dispatch_invoke_jsonrpc` calls `CoreError::to_json_rpc()` for
    // migrated core methods, so capability_denied=1001 / read_only=1002 /
    // path_traversal=1003 / … reach the client intact (asserted by the S7
    // conformance suite). This LEGACY leg stays message-only on purpose: old
    // clients (web-remote-dist, mobile SPA) consume the bare `_error` string and
    // must not change. Paired anchor: `lanWsAdapter.handleInbound`.
    fn core_result_to_envelope(
        r: Result<serde_json::Value, ridge_core::CoreError>,
    ) -> serde_json::Value {
        match r {
            Ok(v) => serde_json::json!({ "_result": v }),
            Err(e) => serde_json::json!({ "_error": e.to_command_string() }),
        }
    }

    // Most commands need a real Tauri context. The stashed handle gives us both
    // a managed `State<AppState>` (same Arcs as `state`) and an `AppHandle`.
    let handle = match state.app_handle.get() {
        Some(h) => h.clone(),
        None => return serde_json::json!({ "_error": "host application handle not ready" }),
    };
    // `handle.state::<AppState>()` panics if AppState isn't managed; guard once so
    // a misconfigured host degrades to an error instead of aborting the WS task.
    if handle.try_state::<AppState>().is_none() {
        return serde_json::json!({ "_error": "host application state unavailable" });
    }

    match cmd {
        // ── Filesystem (read-only: S5 migrated into ridge-core) ──
        // `get_file_tree` / `get_directory_children` / `path_exists` /
        // `read_file` / `read_file_for_editor` now live in `ridge_core::fs::
        // commands`; route them through the unified `ridge_core::dispatch` so the
        // LAN host shares the exact same implementation + capability gate (D8)
        // the headless host uses. The core error maps onto the legacy
        // `{_result|_error}` envelope below — wire behaviour is unchanged.
        "get_file_tree"
        | "get_directory_children"
        | "path_exists"
        | "read_file"
        | "read_file_for_editor" => {
            let ctx = crate::remote::core_bridge::remote_ctx(&handle, state, "remote");
            core_result_to_envelope(ridge_core::dispatch(cmd, args.clone(), &ctx))
        }
        "write_file" => unit(project::write_file(s(args, "path"), s(args, "content")).await),
        "apply_file_edits" => match from_arg::<Vec<project::TextEdit>>(args, "edits") {
            Ok(edits) => unit(project::apply_file_edits(s(args, "path"), edits).await),
            Err(e) => serde_json::json!({ "_error": format!("invalid edits: {e}") }),
        },
        "rename_path" => unit(project::rename_path(s(args, "from"), s(args, "to"))),
        "delete_path" => unit(project::delete_path(s(args, "path")).await),
        "create_file" => unit(project::create_file(s(args, "path"))),
        "create_directory" => unit(project::create_directory(s(args, "path"))),
        "copy_path" => unit(
            project::copy_path(s(args, "from"), s(args, "to"), bool_opt(args, "overwrite")).await,
        ),
        "move_path" => unit(project::move_path(s(args, "from"), s(args, "to")).await),
        "reveal_in_file_manager" => unit(project::reveal_in_file_manager(s(args, "path"))),
        // `read_file_for_editor` is handled by the read-only ridge-core arm above.
        "get_current_project" => val(project::get_current_project(handle.state())),

        // ── Filesystem / git watchers (live fs-changed / scm refresh) ──
        "start_watching_paths" => match from_arg::<Vec<fs_watch::WatchSpec>>(args, "roots") {
            Ok(roots) => {
                unit(fs_watch::start_watching_paths(roots, handle.clone(), handle.state()).await)
            }
            Err(e) => serde_json::json!({ "_error": format!("invalid roots: {e}") }),
        },
        "start_watching_repos" => unit(
            watch::start_watching_repos(vec_s(args, "roots"), handle.clone(), handle.state()).await,
        ),

        // ── Pane / terminal ──
        "get_pane_layout" => val(pane::get_pane_layout(handle.state())),
        "get_pane_layout_for" => val(pane::get_pane_layout_for(
            handle.state(),
            s(args, "workspaceId"),
        )),
        "split_pane" => {
            val(pane::split_pane(handle.state(), s(args, "paneId"), s(args, "direction")).await)
        }
        "dock_pane" => unit(
            pane::dock_pane(
                handle.state(),
                s(args, "sourcePaneId"),
                s(args, "targetPaneId"),
                s(args, "region"),
            )
            .await,
        ),
        "close_pane" => unit(pane::close_pane(handle.state(), s(args, "paneId")).await),
        "toggle_mode" => match from_arg(args, "mode") {
            Ok(mode) => unit(pane::toggle_mode(handle.state(), s(args, "paneId"), mode).await),
            Err(e) => serde_json::json!({ "_error": format!("invalid mode: {e}") }),
        },
        "set_split_ratios_at_path" => match (
            from_arg::<Vec<usize>>(args, "path"),
            from_arg::<Vec<f32>>(args, "ratios"),
        ) {
            (Ok(p), Ok(r)) => unit(pane::set_split_ratios_at_path(handle.state(), p, r).await),
            _ => serde_json::json!({ "_error": "invalid split-ratio args" }),
        },
        "set_split_ratios_batch" => match from_arg(args, "updates") {
            Ok(updates) => unit(pane::set_split_ratios_batch(handle.state(), updates).await),
            Err(e) => serde_json::json!({ "_error": format!("invalid updates: {e}") }),
        },
        "create_pane" => unit(
            terminal::create_pane(handle.state(), s(args, "paneId"), opt_s(args, "shell")).await,
        ),
        "activate_pane_pty" => unit(
            terminal::activate_pane_pty(
                handle.state(),
                handle.clone(),
                s(args, "workspaceId"),
                s(args, "paneId"),
                u16_opt(args, "rows"),
                u16_opt(args, "cols"),
            )
            .await,
        ),
        "change_pane_shell" => unit(
            terminal::change_pane_shell(handle.state(), s(args, "paneId"), s(args, "shell")).await,
        ),
        "write_to_pty" => {
            unit(terminal::write_to_pty(handle.state(), s(args, "paneId"), s(args, "data")).await)
        }
        "resize_pane" => unit(
            terminal::resize_pane(
                handle.state(),
                handle.clone(),
                s(args, "workspaceId"),
                s(args, "paneId"),
                u16_arg(args, "rows"),
                u16_arg(args, "cols"),
                bool_opt(args, "isAlt"),
                bool_opt(args, "isInlineTui"),
            )
            .await,
        ),
        "detect_available_shells" => plain(terminal::detect_available_shells()),
        "get_shell_history" => val(terminal::get_shell_history(s(args, "shellKind")).await),

        // ── Workspace (live) ──
        // `list_workspaces` is read-only and required by the desktop SPA
        // controller's boot (`refreshWorkspaces`): without it the web-remote
        // controller's `invoke('list_workspaces')` throws "command not available
        // remotely", aborting workspace init and stranding the controller on
        // "请先选择一个工作区". Mirrors `get_active_workspace_id` (val + State).
        "list_workspaces" => val(workspace::list_workspaces(handle.state())),
        "get_active_workspace_id" => val(workspace::get_active_workspace_id(handle.state())),
        "switch_workspace" => unit(workspace::switch_workspace(
            handle.state(),
            s(args, "workspaceId"),
        )),
        "create_workspace" => val(workspace::create_workspace(
            handle.state(),
            opt_s(args, "name"),
        )),
        "close_workspace" => unit(workspace::close_workspace(
            handle.state(),
            s(args, "workspaceId"),
        )),
        "rename_workspace" => unit(workspace::rename_workspace(
            handle.state(),
            s(args, "workspaceId"),
            s(args, "name"),
        )),
        "reorder_workspaces" => unit(workspace::reorder_workspaces(
            handle.state(),
            usize_arg(args, "fromIndex"),
            usize_arg(args, "toIndex"),
        )),

        // ── Workspace (persistence / .ridge) ──
        "save_workspace" => val(workspace::save_workspace(
            handle.clone(),
            handle.state(),
            opt_s(args, "name"),
        )),
        "list_saved_workspaces" => val(workspace::list_saved_workspaces(handle.clone())),
        "delete_saved_workspace" => unit(workspace::delete_saved_workspace(
            handle.clone(),
            s(args, "id"),
        )),
        "rename_saved_workspace" => unit(workspace::rename_saved_workspace(
            handle.clone(),
            s(args, "id"),
            s(args, "name"),
        )),
        "list_workspace_save_info" => val(ridge_file::list_workspace_save_info(handle.state())),
        "delete_workspace_file" => unit(ridge_file::delete_workspace_file(
            handle.clone(),
            handle.state(),
            s(args, "workspaceId"),
        )),
        "get_default_workspace_save_dir" => val(ridge_file::get_default_workspace_save_dir()),
        "list_saved_workspace_files" => val(ridge_file::list_saved_workspace_files()),
        "save_workspace_to_file" => val(ridge_file::save_workspace_to_file(
            handle.clone(),
            handle.state(),
            s(args, "workspaceId"),
            s(args, "name"),
            opt_s(args, "path"),
        )),
        "open_workspace_from_file" => val(ridge_file::open_workspace_from_file(
            handle.clone(),
            handle.state(),
            s(args, "path"),
        )),
        "get_restore_set" => val(ridge_file::get_restore_set(handle.clone())),
        "list_recent_workspaces" => val(ridge_file::list_recent_workspaces(handle.clone())),
        "clear_recent_workspaces" => unit(ridge_file::clear_recent_workspaces(handle.clone())),
        "get_last_opened_workspace_path" => {
            val(ridge_file::get_last_opened_workspace_path(handle.clone()))
        }
        "get_startup_context" => val(ridge_file::get_startup_context(handle.state())),
        "browse_directory" => val(ridge_file::browse_directory(opt_s(args, "path"))),

        // ── Theme / settings (S1: migrated into ridge-core) ──
        // These three handlers now live in `ridge_core`; route them through
        // the unified `ridge_core::dispatch` so the LAN host shares the exact
        // same implementation + capability gate (D8) the headless host will.
        // The core's `{code,message,data}` error maps onto the legacy
        // `{_result|_error}` WS envelope below — wire behaviour is unchanged.
        "get_theme_data" | "set_active_theme" | "set_user_default_cwd" => {
            let ctx = crate::remote::core_bridge::remote_ctx(&handle, state, "remote");
            core_result_to_envelope(ridge_core::dispatch(cmd, args.clone(), &ctx))
        }

        // ── Search ── (S5: `text_search` migrated into ridge-core)
        // Routes through the unified dispatch (the `search` alias shares the
        // same handler). camelCase arg keys are read by the core directly.
        "text_search" => {
            let ctx = crate::remote::core_bridge::remote_ctx(&handle, state, "remote");
            core_result_to_envelope(ridge_core::dispatch(cmd, args.clone(), &ctx))
        }
        "filename_search" => {
            val(project::filename_search(s(args, "root"), s(args, "pattern")).await)
        }
        "text_search_diagnostics" => plain(project::text_search_diagnostics(
            Some(vec_s(args, "includeGlobs")),
            Some(vec_s(args, "excludeGlobs")),
        )),
        "replace_in_files" => val(project::replace_in_files(
            s(args, "root"),
            s(args, "search"),
            s(args, "replace"),
            vec_s(args, "files"),
            bool_opt(args, "caseSensitive"),
            bool_opt(args, "useRegex"),
        )
        .await),

        // ── Git (read) ──
        "find_git_repo_root" => plain(git::find_git_repo_root(s(args, "path"))),
        "find_git_repos_below" => {
            plain(git::find_git_repos_below(s(args, "path"), usize_opt(args, "maxDepth")).await)
        }
        "get_scm_status" => val(git::get_scm_status(s(args, "repoRoot")).await),
        "get_git_info_with_cwd" => val(git::get_git_info_with_cwd(s(args, "cwd")).await),
        "get_git_commits_paginated" => val(git::get_git_commits_paginated(
            s(args, "repoRoot"),
            u32_arg(args, "offset"),
            u32_arg(args, "limit"),
        )
        .await),
        "git_list_branches" => val(git::git_list_branches(s(args, "repoRoot")).await),
        "git_diff_summary" => val(git::git_diff_summary(s(args, "repoRoot")).await),
        "git_get_file_versions" => val(git::git_get_file_versions(
            s(args, "repoRoot"),
            s(args, "path"),
            bool_opt(args, "cached"),
        )
        .await),
        "git_op_in_progress" => plain(git::git_op_in_progress(s(args, "repoRoot"))),
        "git_fetch" => unit(git::git_fetch(s(args, "repoRoot")).await),

        // ── Git (mutating; mirrors dispatch_data_request) ──
        "git_stage" => unit(git::git_stage(s(args, "repoRoot"), vec_s(args, "paths")).await),
        "git_unstage" => unit(git::git_unstage(s(args, "repoRoot"), vec_s(args, "paths")).await),
        "git_commit" => unit(
            git::git_commit(
                s(args, "repoRoot"),
                s(args, "message"),
                bool_opt(args, "amend"),
            )
            .await,
        ),
        "git_pull" => unit(git::git_pull(s(args, "repoRoot")).await),
        "git_push" => unit(git::git_push(s(args, "repoRoot"), bool_opt(args, "setUpstream")).await),
        "git_sync" => unit(git::git_sync(s(args, "repoRoot")).await),
        "git_checkout" => unit(
            git::git_checkout(
                s(args, "repoRoot"),
                s(args, "branch"),
                bool_opt(args, "create"),
                None,
            )
            .await,
        ),
        "git_revert" => unit(git::git_revert(s(args, "repoRoot"), s(args, "hash")).await),
        "git_cherry_pick" => unit(git::git_cherry_pick(s(args, "repoRoot"), s(args, "hash")).await),
        "git_reset" => {
            unit(git::git_reset(s(args, "repoRoot"), s(args, "commit"), s(args, "mode")).await)
        }
        "git_create_tag" => unit(
            git::git_create_tag(
                s(args, "repoRoot"),
                s(args, "name"),
                None,
                opt_s(args, "message"),
            )
            .await,
        ),
        "git_discard" => unit(git::git_discard(s(args, "repoRoot"), vec_s(args, "paths")).await),
        "git_clean_untracked" => {
            unit(git::git_clean_untracked(s(args, "repoRoot"), Vec::new()).await)
        }

        other => {
            tracing::warn!(target: "ridge::remote", cmd = %other, "invoke-request: command not in allowlist");
            serde_json::json!({ "_error": format!("command not available remotely: {}", other) })
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §S3 unified line protocol — JSON-RPC 2.0 leg (additive, backward-compatible).
//
// The LAN host historically spoke a bespoke envelope: invoke as
// `{type:'invoke-request', cmd, args, _reqId}` → `{type:'invoke-result',
// _reqId, _result|_error}`, control as flat `{type:'…', …}` frames. That LEGACY
// leg is left byte-for-byte unchanged (web-remote-dist + the mobile SPA depend
// on it). This section adds a *parallel* JSON-RPC 2.0 leg per the S0 contract
// (`docs/contracts/ridge-cloud-protocol.md` §7.0/§7.3/§7.4):
//
//   request       { "jsonrpc":"2.0", "id":…, "method":…, "params":… }
//   success resp  { "jsonrpc":"2.0", "id":…, "result":… }
//   error resp    { "jsonrpc":"2.0", "id":…, "error":{ code, message, data } }
//   notification  { "jsonrpc":"2.0", "method":…, "params":… }   (no id)
//   $/hello       D9 version + capability handshake
//   $/cancel      cancel an in-flight request by id
//
// The host replies in the SAME shape it received: a JSON-RPC request gets a
// JSON-RPC response; a legacy invoke-request gets the legacy result. A frame is
// treated as JSON-RPC iff `parsed["jsonrpc"] == "2.0"`.
// ════════════════════════════════════════════════════════════════════════════

/// Protocol version this host implements (D9). The controller SPA negotiates
/// the highest common version; v1 is the only version today.
const REMOTE_PROTOCOL_VERSION: u64 = 1;

/// Capabilities this host advertises in the `$/hello` handshake (D9). The
/// controller intersects this with its own set and greys out missing panels.
/// `pane`/`invoke` are transport-level; the rest mirror the command families in
/// `REMOTE_ALLOWLIST` (the capability *gate* for execution is still D8 — these
/// only drive which controller panels are shown, per S0 contract §7.3).
const HOST_CAPABILITIES: &[&str] = &[
    "pane",
    "invoke",
    "fs",
    "git",
    "search",
    "workspace",
    "theme",
];

/// Methods already migrated into `ridge-core` (mirrors the dedicated arm in
/// `dispatch_invoke_request`). For these the JSON-RPC leg passes the FULL
/// `CoreError::to_json_rpc()` `{code,message,data}` object through — resolving
/// the legacy "message-only" error-code loss documented at decision **D-GM-2**.
const CORE_MIGRATED_METHODS: &[&str] = &[
    // S1
    "get_theme_data",
    "set_active_theme",
    "set_user_default_cwd",
    // S5 — read-only filesystem + search
    "get_file_tree",
    "get_directory_children",
    "path_exists",
    "read_file",
    "read_file_for_editor",
    "text_search",
    "search",
];

/// Dispatch one **JSON-RPC** invoke. Returns `Ok(result_value)` or
/// `Err(json_rpc_error_object)` where the error object is `{code,message,data}`.
///
/// §D-GM-2: for `ridge-core`-migrated methods the error is produced by
/// `CoreError::to_json_rpc()`, so the structured `code` (capability_denied=1001,
/// read_only=1002, path_traversal=1003, …) and `data.kind` survive end-to-end —
/// no longer collapsed to a bare message. For not-yet-migrated legacy methods
/// the backing handler only produces a `String`, so its error maps to the
/// JSON-RPC `INTERNAL_ERROR` (-32603) code with that message preserved; the
/// `code`/`data` fidelity improves automatically as each handler migrates into
/// `ridge-core` (S1 ledger).
async fn dispatch_invoke_jsonrpc(
    cmd: &str,
    args: &serde_json::Value,
    state: &AppState,
) -> Result<serde_json::Value, serde_json::Value> {
    // JSON-RPC-native path for migrated core commands: pass `to_json_rpc()`
    // through verbatim (the D-GM-2 fix).
    if CORE_MIGRATED_METHODS.contains(&cmd) {
        let handle = match state.app_handle.get() {
            Some(h) => h.clone(),
            None => {
                return Err(serde_json::json!({
                    "code": ridge_core::error::CODE_HOST_UNAVAILABLE,
                    "message": "host application handle not ready",
                    "data": { "kind": "host_unavailable" },
                }))
            }
        };
        // Defence-in-depth read-only gate, identical to the legacy leg.
        if is_mutating_invoke(cmd) && state.remote_fs_readonly.load(Ordering::Relaxed) {
            return Err(ridge_core::CoreError::ReadOnly.to_json_rpc());
        }
        let ctx = crate::remote::core_bridge::remote_ctx(&handle, state, "remote");
        return ridge_core::dispatch(cmd, args.clone(), &ctx).map_err(|e| e.to_json_rpc());
    }

    // Legacy methods: reuse the single source of command routing
    // (`dispatch_invoke_request`) and translate its `{_result|_error}` envelope
    // into the JSON-RPC result/error shape. Un-migrated handlers only carry a
    // message, so the error code is the generic INTERNAL_ERROR (-32603).
    let envelope = dispatch_invoke_request(cmd, args, state).await;
    if let Some(err) = envelope.get("_error") {
        let message = err.as_str().unwrap_or("command failed").to_string();
        Err(serde_json::json!({
            "code": JSON_RPC_INTERNAL_ERROR,
            "message": message,
            "data": { "kind": "internal" },
        }))
    } else {
        Ok(envelope
            .get("_result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }
}

/// Standard JSON-RPC 2.0 reserved error codes used by the host leg.
const JSON_RPC_INVALID_REQUEST: i64 = -32600;
const JSON_RPC_INTERNAL_ERROR: i64 = -32603;

/// Build a JSON-RPC success response frame.
fn jsonrpc_result(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC error response frame from a `{code,message,data}` object.
fn jsonrpc_error(id: &serde_json::Value, error: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// Negotiate the `$/hello` handshake (D9). Given the controller's announced
/// `protocolVersion` + `capabilities`, return either the host's reply
/// `$/hello` notification (on a compatible version) or a `$/bye` notification
/// (no common version) for the caller to send. Capabilities are intersected so
/// the controller greys out panels this host does not serve.
fn negotiate_hello(params: &serde_json::Value) -> serde_json::Value {
    let peer_version = params
        .get("protocolVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // Highest common version. v1 host supports exactly {1}; the common max with
    // any peer that also supports v1 is 1, otherwise there is no overlap.
    if peer_version < REMOTE_PROTOCOL_VERSION {
        return serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/bye",
            "params": { "reason": "protocol-version-mismatch" },
        });
    }
    let peer_caps: std::collections::HashSet<String> = params
        .get("capabilities")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let agreed: Vec<&str> = HOST_CAPABILITIES
        .iter()
        .copied()
        .filter(|c| peer_caps.is_empty() || peer_caps.contains(*c))
        .collect();
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/hello",
        "params": {
            "protocolVersion": REMOTE_PROTOCOL_VERSION,
            "capabilities": agreed,
        },
    })
}

// ────────────────────────────────────────────────────────────────────────────
// §S3 · D10 attach screen-snapshot — SCAFFOLD ONLY (full per-pane screen buffer
// is deferred to S5; see contract §7.4). raw-byte (`0x10`) PTY output is NOT
// replayable, so a controller that attaches late or reconnects cannot recover
// history from the live stream. D10's terminal answer: the host keeps a
// per-pane SCREEN BUFFER and, on `subscribe-pane`, sends a SNAPSHOT first, then
// resumes the raw stream.
//
// CURRENT STATE (partial precursor, already shipping): `subscribe-pane` replays
// up to 64 KiB of recent scrollback as raw bytes before the live stream (see the
// `subscribe-pane` arm + the desync-resync path). That gives the kernel enough
// to repaint, which is why the LAN leg works today. It is byte-scrollback, NOT a
// rendered screen snapshot, so it does not capture alt-screen state / cursor /
// scroll-region precisely.
//
// SCAFFOLD this section defines (no behaviour change yet):
//   • [`PaneSnapshotFrame`] — the JSON control-frame shape a future host will
//     emit as the FIRST response to `subscribe-pane`, before raw `0x10` bytes.
//   • The接入点 (integration point) is marked inline in the `subscribe-pane`
//     handler with `// §D10 接入点`.
//
// FOLLOW-UP IMPLEMENTATION NOTES (S5 / per-pane screen buffer):
//   1. State: add a per-pane rendered-screen buffer on the PTY handle (reuse the
//      existing `parser` / vte `Terminal` — `terminals[pane].parser` already
//      tracks screen state for `title()`; expose a `screen_snapshot()` that emits
//      a repaint sequence incl. cursor pos, alt-screen flag, scroll region).
//   2. Emit: in the `subscribe-pane` arm, BEFORE the scrollback send, push a
//      `PaneSnapshotFrame` carrying that repaint sequence + the pane's LOCKED
//      render size (D11 shared property — see `lockedRows`/`lockedCols`).
//   3. Reconnect: the L2 client (rpcClient.onReconnected) re-sends
//      `subscribe-pane` per previously-subscribed pane; the host replies snapshot
//      → raw, exactly as a fresh attach (already wired client-side, bridge.ts).
//   4. Bound: the screen buffer is O(rows×cols), naturally bounded — unlike the
//      abandoned per-sub 11 MB delta PaneParser (the OOM that motivated raw-byte).
//   5. Multi-client (D11): the screen buffer is a SHARED pane property; every
//      attaching controller gets the same snapshot. Locked size rides the snapshot.
// ────────────────────────────────────────────────────────────────────────────

/// **D10 SCAFFOLD** — the JSON control frame a future host emits as the first
/// response to `subscribe-pane`, carrying the current rendered screen so a
/// late/reconnecting controller can repaint before consuming the live raw
/// stream. Defined now so the wire shape is fixed for S5; not yet emitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Wire-shape scaffold; emitted by S5's per-pane screen buffer.
pub struct PaneSnapshotFrame {
    /// Discriminator on the control channel: always `"pane-snapshot"`.
    #[serde(rename = "type")]
    pub frame_type: String,
    /// Pane this snapshot belongs to (UUID string).
    #[serde(rename = "paneId")]
    pub pane_id: String,
    /// Terminal repaint sequence (raw escape bytes, base64) reconstructing the
    /// current screen: cursor position, alt-screen state, scroll region, content.
    /// The controller feeds this to its wasm kernel before the raw `0x10` stream.
    pub screen: String,
    /// The pane's LOCKED render size (D11 shared property). `None` until a
    /// controller has claimed a size. Rides the snapshot so every attaching
    /// controller renders at the same grid.
    #[serde(rename = "lockedRows", skip_serializing_if = "Option::is_none")]
    pub locked_rows: Option<u16>,
    #[serde(rename = "lockedCols", skip_serializing_if = "Option::is_none")]
    pub locked_cols: Option<u16>,
}

#[cfg(test)]
mod jsonrpc_tests {
    //! Pure host-side wire-shape tests for the §S3 JSON-RPC leg. These cover the
    //! envelope builders and the D9 `$/hello` negotiation without needing a live
    //! `AppState` / Tauri runtime, so they compile + run under `cargo test -p
    //! ridge`. (On this machine the cdylib `cargo test` crashes 0xc0000139, so
    //! they are verified by `cargo check` here and runnable by the user post-
    //! rebuild; the S7 TS conformance suite covers the same negotiation E2E.)
    use super::*;

    #[test]
    fn jsonrpc_result_frame_shape() {
        let f = jsonrpc_result(&serde_json::json!(7), serde_json::json!({"ok": true}));
        assert_eq!(f["jsonrpc"], "2.0");
        assert_eq!(f["id"], serde_json::json!(7));
        assert_eq!(f["result"], serde_json::json!({"ok": true}));
        assert!(f.get("error").is_none());
    }

    #[test]
    fn jsonrpc_error_frame_carries_code_message_data() {
        let err =
            ridge_core::CoreError::CapabilityDenied("set_remote_enabled".into()).to_json_rpc();
        let f = jsonrpc_error(&serde_json::json!(3), err);
        assert_eq!(f["id"], serde_json::json!(3));
        assert_eq!(f["error"]["code"], serde_json::json!(1001));
        assert_eq!(f["error"]["data"]["kind"], "capability_denied");
        assert!(f["error"]["message"]
            .as_str()
            .unwrap()
            .contains("set_remote_enabled"));
    }

    #[test]
    fn hello_negotiates_capability_intersection() {
        let reply = negotiate_hello(&serde_json::json!({
            "protocolVersion": 1,
            "capabilities": ["pane", "invoke", "fs"],
        }));
        assert_eq!(reply["method"], "$/hello");
        assert_eq!(reply["params"]["protocolVersion"], serde_json::json!(1));
        let caps: Vec<&str> = reply["params"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // Only the host∩client subset is agreed.
        assert!(caps.contains(&"fs"));
        assert!(caps.contains(&"invoke"));
        assert!(!caps.contains(&"git")); // client did not request it
    }

    #[test]
    fn hello_empty_capabilities_means_all_host_caps() {
        // A peer that omits capabilities gets the host's full set (it can drive all).
        let reply = negotiate_hello(&serde_json::json!({ "protocolVersion": 1 }));
        let caps = reply["params"]["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), HOST_CAPABILITIES.len());
    }

    #[test]
    fn hello_version_mismatch_sends_bye() {
        let reply = negotiate_hello(&serde_json::json!({
            "protocolVersion": 0,
            "capabilities": ["pane"],
        }));
        assert_eq!(reply["method"], "$/bye");
        assert_eq!(reply["params"]["reason"], "protocol-version-mismatch");
    }

    #[test]
    fn pane_snapshot_frame_serializes_to_contract_shape() {
        let snap = PaneSnapshotFrame {
            frame_type: "pane-snapshot".into(),
            pane_id: "p1".into(),
            screen: "AAAA".into(),
            locked_rows: Some(24),
            locked_cols: Some(80),
        };
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["type"], "pane-snapshot");
        assert_eq!(v["paneId"], "p1");
        assert_eq!(v["lockedRows"], serde_json::json!(24));
        assert_eq!(v["lockedCols"], serde_json::json!(80));
    }
}
