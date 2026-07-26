//! 前端静态资源 serve（UA 分流：桌面完整 SPA vs 移动轻量 SPA）。
//!
//! 从桌面 `src-tauri/src/remote/server.rs` 下沉到共享 crate，供 LAN 远控服务端
//! （桌面 Tauri app / rdg CLI）复用。**零 Tauri 依赖**：serve 逻辑只依赖静态目录
//! + 一个 `remote_enabled` 开关（`Arc<AtomicBool>`）+ 是否 TLS（HSTS 头门控）。
//!
//! UA→UI 分叉判定复用 [`crate::ua::prefer_desktop_ui`]（SSOT），并额外校验桌面
//! 产物目录是否存在（缺失回退移动 SPA）。
//!
//! ## 装配方式
//!
//! [`ServeState`] 是这些 handler 的 axum State。调用方（桌面 `server.rs`）自身的
//! 路由 State 不必是 `ServeState`——只要为其实现 `FromRef<其State> for ServeState`，
//! 就能用 [`serve_router`] 把这批 serve 路由 `merge` 进主路由，并用
//! [`security_headers`] / [`compression_layer`] 装配中间件（行为与旧内联实现一致）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::{
    extract::{FromRef, Query, State},
    http::{HeaderMap, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use tower_http::compression::CompressionLayer;

/// UA 分流的静态 serve 配置：移动 SPA 目录（必有）+ 可选桌面 SPA 目录。
///
/// - `mobile_dir`：`pnpm build:remote` 产物（`static/remote/`），移动/触屏浏览器与
///   验证页（`/verify`）用它。
/// - `desktop_dir`：`pnpm build:desktop-web` 产物（`web-remote-dist/`），桌面浏览器
///   （UA 分流命中且产物存在时）用它；`None` 或产物缺失时桌面 UA 回退移动 SPA。
#[derive(Clone, Debug)]
pub struct UaServeConfig {
    pub mobile_dir: PathBuf,
    pub desktop_dir: Option<PathBuf>,
}

impl UaServeConfig {
    /// 运行时候选目录探测：泛化桌面 `server.rs` 里对 `static/remote` 与
    /// `web-remote-dist` 的候选路径探测。候选顺序见 [`probe_ui_dir`]：
    /// `RIDGE_REMOTE_UI_ROOT` 覆盖 → CWD → exe 目录逐级上溯（兼容桌面 exe 与更浅的
    /// `rdg` exe，无需按二进制标定级数）。
    ///
    /// 移动目录探测不到时回退到 `static/remote`（serve_index 会给出"未构建"提示页）；
    /// 桌面目录探测不到时为 `None`。
    pub fn resolve_ui_dirs() -> Self {
        let mobile_dir = probe_ui_dir(&PathBuf::from("static").join("remote"))
            .unwrap_or_else(|| PathBuf::from("static").join("remote"));
        let desktop_dir = probe_ui_dir(&PathBuf::from("web-remote-dist"));
        // §diagnostic: 记录 UI 目录解析结果
        tracing::info!(target: "ridge::remote::serve",
            mobile_dir = %mobile_dir.display(),
            desktop_dir = desktop_dir.as_ref().map(|d| d.display().to_string()).unwrap_or_else(|| "None".to_string()),
            "UI dirs resolved"
        );
        Self {
            mobile_dir,
            desktop_dir,
        }
    }

    /// 是否给该请求发桌面 SPA：先按 [`crate::ua::prefer_desktop_ui`] 判定（尊重
    /// `?ui=` 覆盖），再校验桌面产物 `index.html` 确实存在（缺失即回退移动 SPA）。
    pub fn wants_desktop_ui(&self, headers: &HeaderMap, ui_override: Option<&str>) -> bool {
        let ua = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        crate::ua::prefer_desktop_ui(ua, ui_override)
            && self
                .desktop_dir
                .as_ref()
                .map(|d| d.join("index.html").exists())
                .unwrap_or(false)
    }

    /// 该请求应命中的 UI 目录（桌面 vs 移动）。也是未知客户端路由回退 index.html
    /// 的 SPA 壳目录。
    pub fn ui_dir(&self, headers: &HeaderMap, ui_override: Option<&str>) -> &Path {
        if self.wants_desktop_ui(headers, ui_override) {
            // wants_desktop_ui 为真 ⇒ desktop_dir 必为 Some。
            self.desktop_dir.as_deref().unwrap_or(&self.mobile_dir)
        } else {
            &self.mobile_dir
        }
    }
}

/// 探测某个 UI 产物目录，返回首个含 `index.html` 的候选；都不存在则 `None`。候选顺序：
/// 0. `RIDGE_REMOTE_UI_ROOT/<rel>`——显式覆盖，真·无头部署（资产不在 exe 附近）时设它；
/// 1. `CWD/<rel>`——dev（从工程根 `cargo run`）；
/// 2..N. 从 exe 目录**逐级上溯**（最多 6 级）找 `<ancestor>/<rel>`。
///
/// 上溯替代旧的「固定 parent×4」：桌面 exe 在 `src-tauri/target/release`（工程根深 4 级），
/// 而 `rdg` 是 workspace 成员，exe 落 `target/release`（工程根深 2 级）——固定 4 级对 rdg
/// 会过冲到工程根之外。逐级上溯取**最靠近 exe 的命中**（最正确），一份代码兼容两种深度，
/// 且 `exe 旁`（深 1 级，NSIS/打包把资源拷到 exe 旁）也被覆盖。
fn probe_ui_dir(rel: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("RIDGE_REMOTE_UI_ROOT") {
        candidates.push(PathBuf::from(root).join(rel));
    }
    candidates.push(rel.to_path_buf());
    // 从 exe 目录逐级上溯（最多 6 级）：`ancestors()` 首项是 exe 自身，`skip(1)` 起于其
    // 所在目录，`take(6)` 封顶；顺序即「exe 旁 → 更上层」，取最靠近 exe 的命中。
    if let Ok(exe) = std::env::current_exe() {
        candidates.extend(exe.ancestors().skip(1).take(6).map(|d| d.join(rel)));
    }
    // §diagnostic: 记录探测路径便于调试
    for candidate in &candidates {
        if candidate.join("index.html").exists() {
            tracing::debug!(target: "ridge::remote::serve", path = %candidate.display(), "UI dir found");
            return Some(candidate.clone());
        }
    }
    tracing::warn!(target: "ridge::remote::serve", rel = %rel.display(), "UI dir not found in any candidate path");
    None
}

/// serve 这批 handler 的 axum State：UI 目录配置 + TLS 门（HSTS 头）+ `remote_enabled`
/// 开关（fallback 自门控，因为 `route_layer` 不包裹 fallback）。
///
/// 调用方的主路由 State 若非 `ServeState`，只需 `impl FromRef<主State> for ServeState`，
/// 即可让 [`serve_router`] 的 handler 通过子状态提取拿到本值。
#[derive(Clone)]
pub struct ServeState {
    pub cfg: UaServeConfig,
    /// 是否 TLS serve——门控 HSTS 响应头（纯 HTTP 下 HSTS 无意义/有害）。
    pub tls_enabled: bool,
    /// `remote_enabled` 总开关：关闭时 fallback 直接 503（route_layer 不包裹 fallback）。
    pub enabled: Arc<AtomicBool>,
}

/// 装配 serve 路由子树（`/`、`/assets/*`、CA 下载、SPA fallback），供调用方 `merge`
/// 进主路由。泛型于主 State `S`，只要 `ServeState: FromRef<S>`。
///
/// 注意：`security_headers` 与压缩层**不**在此装配——它们需覆盖调用方全部路由
/// （含 `/ws`、`/verify` 等非 serve 路由），故由调用方在主路由最外层统一 `.layer`
/// （见 [`security_headers`] / [`compression_layer`]），行为与旧内联实现一致。
pub fn serve_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
    ServeState: FromRef<S>,
{
    Router::new()
        .route("/", get(root_handler))
        .route("/assets/*path", get(assets_handler))
        // Local-CA download for the verification page's "trust this device"
        // flow — public (no token): the CA is public key material, and the
        // user needs it *before* authenticating to silence the warning.
        .route("/ridge-ca.crt", get(ca_crt_handler))
        .route("/ridge-ca.pem", get(ca_pem_handler))
        // PWA + SPA fallback: serve root-level static files emitted by the
        // remote build (sw.js, manifest.webmanifest, icons, …) and fall back to
        // index.html for client-side routes. Self-gates on `remote_enabled`
        // because `route_layer` middleware does not wrap the fallback.
        .fallback(spa_fallback_handler)
}

/// 压缩层（gzip/br，按 Accept-Encoding 协商）。装在主路由最外层，对内部所有
/// handler + fallback 的响应体压缩；默认谓词跳过已压缩类型与 <32B 小响应；
/// WS 101 升级无响应体不受影响。
pub fn compression_layer() -> CompressionLayer {
    CompressionLayer::new()
}

// ── Middleware ───────────────────────────────────────────────────────────────

/// SECURITY (audit M2): stamp baseline security headers on every response. The
/// remote UI was previously served with none, leaving it open to clickjacking
/// (no X-Frame-Options / CSP frame-ancestors) and MIME sniffing, and offering no
/// HSTS to pin the TLS upgrade. Applied as a plain `.layer` so it also covers the
/// SPA `.fallback`. Headers are only ADDED — a handler that already set one wins.
pub async fn security_headers(
    State(st): State<ServeState>,
    req: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    use axum::http::header::{HeaderName, HeaderValue};
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    let mut set = |name: HeaderName, value: &'static str| {
        if !headers.contains_key(&name) {
            headers.insert(name, HeaderValue::from_static(value));
        }
    };
    set(axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(axum::http::header::X_FRAME_OPTIONS, "DENY");
    // Modern clickjacking defence (supersedes X-Frame-Options where supported).
    set(
        axum::http::header::CONTENT_SECURITY_POLICY,
        "frame-ancestors 'none'",
    );
    set(
        HeaderName::from_static("referrer-policy"),
        "strict-origin-when-cross-origin",
    );
    // HSTS only over TLS (audit M2): meaningless on plain HTTP, and pinning HTTPS
    // when the server is intentionally running cleartext would brick access.
    if st.tls_enabled {
        set(
            axum::http::header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains",
        );
    }
    resp
}

// ── Handlers ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UiQuery {
    /// Manual override for the UA fork (`?ui=desktop` / `?ui=mobile`), for
    /// testing and edge browsers.
    ui: Option<String>,
}

async fn root_handler(
    State(st): State<ServeState>,
    headers: HeaderMap,
    Query(q): Query<UiQuery>,
) -> impl IntoResponse {
    serve_index(st.cfg.ui_dir(&headers, q.ui.as_deref())).await
}

/// Structured error token when Remote SPA assets are missing (V-B6A / Bug6a).
pub const REMOTE_UI_MISSING_CODE: &str = "REMOTE_UI_MISSING";

/// User-facing repair hint + error code for missing `index.html` (unit-testable).
pub fn remote_ui_missing_message() -> String {
    format!(
        "{code}: Remote UI not built yet. Run: pnpm build:remote (or RIDGE_REMOTE_UI_ROOT to a built static/remote).",
        code = REMOTE_UI_MISSING_CODE
    )
}

fn remote_ui_missing_html() -> String {
    format!(
        r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Ridge Remote</title></head><body style="background:#0d1117;color:#e6edf3;font-family:sans-serif;display:flex;flex-direction:column;align-items:center;justify-content:center;height:100vh;margin:0"><h1>Ridge Remote</h1><p data-ridge-error="{code}">{code}: Remote UI not built yet.</p><p>Run: <code>pnpm build:remote</code></p><p style="opacity:.7;font-size:12px">Or set RIDGE_REMOTE_UI_ROOT to a built asset dir.</p></body></html>"#,
        code = REMOTE_UI_MISSING_CODE
    )
}

/// Serve `index.html` with `Cache-Control: no-cache` so a freshly deployed
/// build (new hashed asset names, new service worker) is always picked up on
/// the next visit instead of being pinned by a stale cached shell.
pub async fn serve_index(dir: &Path) -> Response {
    let index_path = dir.join("index.html");
    // 磁盘 > 内嵌：本地开发改前端立即生效；单文件分发（rdg，`embed-ui`）磁盘无产物
    // 时回落到编进二进制的那份，杜绝 REMOTE_UI_MISSING。
    let bytes = match tokio::fs::read(&index_path).await {
        Ok(b) => Some(b),
        Err(_) => crate::embed_ui::get("index.html"),
    };
    match bytes {
        Some(bytes) => axum::response::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(axum::http::header::CACHE_CONTROL, "no-cache")
            .body(axum::body::Body::from(bytes))
            .unwrap(),
        None => {
            // Fallback: structured error code + repair hint (not silent empty 200 without guidance)
            axum::response::Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header("X-Ridge-Error", REMOTE_UI_MISSING_CODE)
                .header(axum::http::header::CACHE_CONTROL, "no-store")
                .body(axum::body::Body::from(remote_ui_missing_html()))
                .unwrap()
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
    match crate::tls::ca_cert_der() {
        Some(der) => axum::response::Response::builder()
            .header(axum::http::header::CONTENT_TYPE, "application/x-x509-ca-cert")
            .header(axum::http::header::CACHE_CONTROL, "no-store")
            .body(axum::body::Body::from(der))
            .unwrap(),
        None => (StatusCode::NOT_FOUND, "no CA certificate available").into_response(),
    }
}

/// Serve the local root CA as PEM — for desktop trust-store import.
async fn ca_pem_handler() -> impl IntoResponse {
    match crate::tls::ca_cert_pem() {
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
    State(st): State<ServeState>,
    headers: HeaderMap,
    Query(q): Query<UiQuery>,
    uri: Uri,
) -> Response {
    if !st.enabled.load(Ordering::Relaxed) {
        return (StatusCode::SERVICE_UNAVAILABLE, "Remote control disabled").into_response();
    }

    // §UA fork: a desktop browser resolves root-level files (and the SvelteKit
    // `_app/*` bundle) against the desktop build; the mobile SPA against
    // mobile_dir. The chosen dir is also the SPA shell for unknown client routes.
    let base = st.cfg.ui_dir(&headers, q.ui.as_deref());

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
    // 磁盘未命中 → 内嵌（单文件 rdg 的 sw.js / manifest / icons 走这条），
    // 仍未命中才回落 SPA 壳。`rel` 已过上面的穿越守卫。
    let disk = match &within {
        Some(real) => tokio::fs::read(real).await.ok(),
        None => None,
    };
    let bytes = match disk {
        Some(b) => Some(b),
        None => crate::embed_ui::get(rel),
    };
    match bytes {
        Some(bytes) => {
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
        None => serve_index(base).await,
    }
}

/// Content-type + cache policy for root-level build artifacts. The service
/// worker and manifest must revalidate (`no-cache`) so new versions are
/// detected; immutable hashed bundles live under `/assets` (see `assets_handler`).
pub fn root_asset_headers(path: &str) -> (&'static str, &'static str) {
    if path == "sw.js" || path.ends_with("/sw.js") {
        ("application/javascript", "no-cache")
    } else if path.ends_with(".html") {
        // Without this branch a directly-requested `.html` (e.g. `/index.html`)
        // fell through to `application/octet-stream`, which the browser offers as
        // a DOWNLOAD instead of rendering. `no-cache` so a new build's shell is
        // always revalidated (matches serve_index).
        ("text/html; charset=utf-8", "no-cache")
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
    State(st): State<ServeState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    let file_path = st.cfg.mobile_dir.join("assets").join(&path);
    // 磁盘 > 内嵌（同 serve_index：单文件 rdg 靠内嵌供资产）。
    let read = match tokio::fs::read(&file_path).await {
        Ok(b) => Some(b),
        Err(_) => crate::embed_ui::get(&format!("assets/{path}")),
    };
    match read {
        Some(bytes) => {
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
        None => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_ui_missing_message_carries_code_and_repair_hint() {
        let m = remote_ui_missing_message();
        assert!(m.contains(REMOTE_UI_MISSING_CODE), "{m}");
        assert!(m.contains("pnpm build:remote"), "{m}");
    }

    #[test]
    fn remote_ui_missing_html_embeds_code() {
        let h = remote_ui_missing_html();
        assert!(h.contains(REMOTE_UI_MISSING_CODE));
        assert!(h.contains("data-ridge-error"));
        assert!(h.contains("pnpm build:remote"));
    }

    /// 单文件分发（rdg）回归钉：开 `embed-ui` 编出来的库必须真带 UI。
    /// 若构建机漏跑 `pnpm build:remote`，内嵌为空 → LAN 远控又会 REMOTE_UI_MISSING，
    /// 这条测试在发布前就把它抓住（feature 关闭时不涉及，故 cfg 门控）。
    #[cfg(feature = "embed-ui")]
    #[test]
    fn embedded_ui_is_present_when_feature_on() {
        assert!(
            crate::embed_ui::has_ui(),
            "embed-ui 已开启但内嵌产物为空——构建前须先跑 `pnpm build:remote`"
        );
        assert!(crate::embed_ui::get("index.html").is_some_and(|b| !b.is_empty()));
    }
}
