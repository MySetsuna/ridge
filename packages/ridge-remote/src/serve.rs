//! 前端统一产物 serve（UA 分流：桌面完整 SPA vs 移动轻量 SPA）。
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

pub use crate::embed_ui::UiKind;

/// UA 分流的静态 serve 配置。桌面与移动形态均位于同一 `remote-dist` 根。
#[derive(Clone, Debug)]
pub struct UaServeConfig {
    pub remote_dir: PathBuf,
}

impl UaServeConfig {
    /// `RIDGE_REMOTE_UI_ROOT` 可直接覆盖统一产物根；否则从 CWD / exe 上溯探测
    /// `remote-dist`，取最新命中，避免开发期旧 staging 盖住新构建。
    pub fn resolve_ui_dirs() -> Self {
        let remote_dir = probe_ui_root().unwrap_or_else(|| PathBuf::from("remote-dist"));
        tracing::info!(target: "ridge::remote::serve",
            remote_dir = %remote_dir.display(),
            "Remote UI root resolved"
        );
        Self { remote_dir }
    }

    /// 所选形态的磁盘目录；缺 index.html 即视为未构建，交给内嵌产物兜底。
    pub fn disk_dir(&self, kind: UiKind) -> Option<PathBuf> {
        let dir = self.remote_dir.join(kind.dir_name());
        dir.join("index.html").is_file().then_some(dir)
    }

    /// 是否给该请求发桌面 SPA：先按 [`crate::ua::prefer_desktop_ui`] 判定（尊重
    /// `?ui=` 覆盖），再校验桌面产物确实拿得到——**磁盘或内嵌**任一即可。
    ///
    /// 只看磁盘是 iter-62 的 bug：单文件 `rdg` 无外置桌面产物时，
    /// 电脑浏览器会被发手机 SPA。内嵌产物（`embed-ui`）同样
    /// 是「拿得到桌面 SPA」，必须计入。
    pub fn wants_desktop_ui(&self, headers: &HeaderMap, ui_override: Option<&str>) -> bool {
        let ua = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        crate::ua::prefer_desktop_ui(ua, ui_override) && self.desktop_ui_available()
    }

    /// 桌面 SPA 是否可服务（磁盘产物 > 内嵌产物）。
    pub fn desktop_ui_available(&self) -> bool {
        self.disk_dir(UiKind::Desktop).is_some() || crate::embed_ui::has_kind(UiKind::Desktop)
    }

    /// 该请求应命中的 UI 形态 + 其磁盘目录（无磁盘产物时 `None`，走内嵌）。
    /// 目录也是未知客户端路由回退 index.html 的 SPA 壳目录。
    ///
    /// **绝不**在桌面形态下回落到移动目录：那会让磁盘上的手机 index.html
    /// 冒充桌面壳——正是要修的串台。
    pub fn ui_target(&self, headers: &HeaderMap, ui_override: Option<&str>) -> UiTarget {
        if self.wants_desktop_ui(headers, ui_override) {
            UiTarget {
                kind: UiKind::Desktop,
                dir: self.disk_dir(UiKind::Desktop),
            }
        } else {
            UiTarget {
                kind: UiKind::Mobile,
                dir: self.disk_dir(UiKind::Mobile),
            }
        }
    }

    /// 与 `kind` **相反**的那套产物。仅供具体资产的跨形态回退，见 [`read_from_other_ui`]。
    pub fn other_ui_target(&self, kind: UiKind) -> UiTarget {
        match kind {
            UiKind::Desktop => UiTarget {
                kind: UiKind::Mobile,
                dir: self.disk_dir(UiKind::Mobile),
            },
            UiKind::Mobile => UiTarget {
                kind: UiKind::Desktop,
                dir: self.disk_dir(UiKind::Desktop),
            },
        }
    }
}

/// 从**另一形态**的产物里取同名资产（磁盘 > 内嵌）；取不到返回 `None`。
///
/// 为什么必须跨形态回退（iter-62 e2e 实证，页面白屏）：`?ui=` 覆盖只出现在**页面
/// 那一次**请求上；浏览器随后拉 `/assets/index-*.js`、`/_app/immutable/*` 时不带
/// 这个查询参数，于是又被 UA 判回另一套产物 → 整页资源 404、CSS 被当 text/plain
/// 拒收，`<div id="app">` 永远是空的。两套产物文件名带内容哈希、路径前缀也不相交
/// （`assets/` vs `_app/`），跨形态查不会张冠李戴。
///
/// **只对具体资产回退**：SPA 壳（index.html）绝不跨形态——那正是「电脑浏览器被发
/// 手机页」的串台本身，必须由 UA/显式覆盖单方决定。
async fn read_from_other_ui(cfg: &UaServeConfig, kind: UiKind, rel: &str) -> Option<Vec<u8>> {
    if rel == "index.html" || rel.ends_with("/index.html") {
        return None;
    }
    let other = cfg.other_ui_target(kind);
    // 调用方已过穿越守卫（rel 不含 `..` / `\` / `:`）。
    let disk = match other.dir.as_ref() {
        Some(dir) => tokio::fs::read(dir.join(rel)).await.ok(),
        None => None,
    };
    disk.or_else(|| crate::embed_ui::get_kind(other.kind, rel))
}

/// 一次请求解析出的 UI 形态与其磁盘目录（`None` = 该形态只有内嵌产物）。
#[derive(Clone)]
pub struct UiTarget {
    pub kind: UiKind,
    pub dir: Option<PathBuf>,
}

/// 探测统一产物根；候选须至少含一套形态的 index.html。
/// 0. `RIDGE_REMOTE_UI_ROOT`；1. CWD/remote-dist；2..N. exe 上溯/remote-dist。
///
/// 上溯替代旧的「固定 parent×4」：桌面 exe 在 `src-tauri/target/release`（工程根深 4 级），
/// 而 `rdg` 是 workspace 成员，exe 落 `target/release`（工程根深 2 级）——固定 4 级对 rdg
/// 会过冲到工程根之外。逐级上溯取**最靠近 exe 的命中**（最正确），一份代码兼容两种深度，
/// 且 `exe 旁`（深 1 级，NSIS/打包把资源拷到 exe 旁）也被覆盖。
fn probe_ui_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(root) = std::env::var_os("RIDGE_REMOTE_UI_ROOT") {
        candidates.push(PathBuf::from(root));
    }
    candidates.push(PathBuf::from("remote-dist"));
    // 从 exe 目录逐级上溯（最多 6 级）：`ancestors()` 首项是 exe 自身，`skip(1)` 起于其
    // 所在目录，`take(6)` 封顶；顺序即「exe 旁 → 更上层」，取最靠近 exe 的命中。
    if let Ok(exe) = std::env::current_exe() {
        candidates.extend(
            exe.ancestors()
                .skip(1)
                .take(6)
                .map(|d| d.join("remote-dist")),
        );
    }
    let hits: Vec<(PathBuf, Option<std::time::SystemTime>)> = candidates
        .into_iter()
        .filter_map(|c| {
            [UiKind::Desktop, UiKind::Mobile]
                .into_iter()
                .filter_map(|kind| c.join(kind.dir_name()).join("index.html").metadata().ok())
                .filter(|m| m.is_file())
                .filter_map(|m| m.modified().ok())
                .max()
                .map(|mtime| (c, Some(mtime)))
        })
        .collect();
    match pick_freshest(&hits) {
        Some(dir) => {
            tracing::debug!(target: "ridge::remote::serve", path = %dir.display(), "UI dir found");
            Some(dir)
        }
        None => {
            tracing::warn!(target: "ridge::remote::serve", "Remote UI root not found");
            None
        }
    }
}

/// 在**已命中**的候选目录里挑一个：取 `index.html` 最新的那份；时间戳读不到或
/// 全部并列时退回候选顺序里的第一个（即原来的「最靠近 exe」语义）。
///
/// 为什么不能只按顺序取（iter-63 实测，排查耗了四轮）：exe 旁的那份是**构建时的
/// 拷贝**。开发里仓库根产物已更新，而 target staging 仍停在几小时前——顺序优先让陈旧拷贝一直盖住新产物，
/// 页面看着正常、跑的却是旧 bundle，改什么都「没生效」。装机升级留下的旧拷贝同理。
pub fn pick_freshest(hits: &[(PathBuf, Option<std::time::SystemTime>)]) -> Option<PathBuf> {
    let mut best: Option<&(PathBuf, Option<std::time::SystemTime>)> = None;
    for h in hits {
        best = match best {
            None => Some(h),
            // 严格大于：并列时保留先出现者，退化为原有顺序语义。
            Some(b) => match (h.1, b.1) {
                (Some(t), Some(bt)) if t > bt => Some(h),
                (Some(_), None) => Some(h),
                _ => Some(b),
            },
        };
    }
    best.map(|(p, _)| p.clone())
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
    serve_shell(st.cfg.ui_target(&headers, q.ui.as_deref())).await
}

/// Structured error token when Remote SPA assets are missing (V-B6A / Bug6a).
pub const REMOTE_UI_MISSING_CODE: &str = "REMOTE_UI_MISSING";

/// User-facing repair hint + error code for missing `index.html` (unit-testable).
pub fn remote_ui_missing_message() -> String {
    format!(
        "{code}: Remote UI not built yet. Run: pnpm build:remote (or set RIDGE_REMOTE_UI_ROOT to remote-dist).",
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
///
/// 旧签名（只吃一个目录）保留给外部调用方；形态感知的入口是 [`serve_shell`]。
pub async fn serve_index(dir: &Path) -> Response {
    serve_shell(UiTarget {
        kind: UiKind::Mobile,
        dir: Some(dir.to_path_buf()),
    })
    .await
}

/// 按 UI 形态发 SPA 壳：磁盘 > 内嵌，两者皆无才给 `REMOTE_UI_MISSING` 提示页。
pub async fn serve_shell(target: UiTarget) -> Response {
    let disk = match target.dir.as_ref() {
        Some(dir) => tokio::fs::read(dir.join("index.html")).await.ok(),
        None => None,
    };
    // 磁盘 > 内嵌：本地开发改前端立即生效；单文件分发（rdg，`embed-ui`）磁盘无产物
    // 时回落到编进二进制的那份，杜绝 REMOTE_UI_MISSING / 桌面被发手机页。
    let bytes = match disk {
        Some(b) => Some(b),
        None => crate::embed_ui::get_kind(target.kind, "index.html"),
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
    // mobile shape. The chosen target is also the SPA shell for unknown client routes.
    let target = st.cfg.ui_target(&headers, q.ui.as_deref());

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
        return serve_shell(target).await;
    }

    // …then a canonical-path containment check is the authoritative guard: the
    // resolved target (symlinks + `.` segments collapsed) must live inside the
    // chosen UI dir. `canonicalize` fails for non-existent paths, which naturally
    // routes unknown SPA client-side routes to the shell.
    let within = match target.dir.as_ref() {
        Some(base) => match (
            tokio::fs::canonicalize(base.join(rel)).await,
            tokio::fs::canonicalize(base).await,
        ) {
            (Ok(real), Ok(root)) => real.starts_with(&root).then_some(real),
            _ => None,
        },
        None => None,
    };
    // 磁盘未命中 → 同形态的内嵌产物（单文件 rdg 的 sw.js / manifest / icons /
    // 桌面 `_app/immutable/*` 都走这条），仍未命中才回落 SPA 壳。
    // `rel` 已过上面的穿越守卫。
    let disk = match &within {
        Some(real) => tokio::fs::read(real).await.ok(),
        None => None,
    };
    let bytes = match disk {
        Some(b) => Some(b),
        None => crate::embed_ui::get_kind(target.kind, rel),
    };
    // 仍未命中 → 另一形态的同名资产（`?ui=` 覆盖后续请求不带参数，见
    // read_from_other_ui；壳 index.html 被该函数拒绝，不会串台）。都没有才回落 SPA 壳。
    let bytes = match bytes {
        Some(b) => Some(b),
        None => read_from_other_ui(&st.cfg, target.kind, rel).await,
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
        None => serve_shell(target).await,
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

/// Serve static assets (JS, CSS, WASM) from the built output directory of the
/// UI the request resolved to（此前恒取移动目录——桌面 SPA 一旦也用 `/assets/*`
/// 就会串到手机产物上）。
async fn assets_handler(
    State(st): State<ServeState>,
    headers: HeaderMap,
    Query(q): Query<UiQuery>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl IntoResponse {
    let target = st.cfg.ui_target(&headers, q.ui.as_deref());
    // 穿越守卫（与 spa_fallback_handler 同款）：axum 先百分号解码，`%2e%2e` 到手即 `..`。
    // 通配段此前直接 join 进产物目录，等于把 `/assets/../..` 交给文件系统。
    if path.contains("..") || path.contains('\\') || path.contains(':') || path.starts_with('/') {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let rel = format!("assets/{path}");
    let disk = match target.dir.as_ref() {
        Some(dir) => tokio::fs::read(dir.join(&rel)).await.ok(),
        None => None,
    };
    // 磁盘 > 内嵌（同 serve_shell：单文件 rdg 靠内嵌供资产）> 另一形态（`?ui=` 覆盖
    // 后续的资产请求不带该参数，见 read_from_other_ui）。
    let read = match disk {
        Some(b) => Some(b),
        None => crate::embed_ui::get_kind(target.kind, &rel),
    };
    let read = match read {
        Some(b) => Some(b),
        None => read_from_other_ui(&st.cfg, target.kind, &rel).await,
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

    /// iter-63：exe 旁的构建拷贝会长期盖住仓库里刚构建出来的新产物。
    /// 命中多个候选时必须取**最新**的那份。
    #[test]
    fn freshest_ui_dir_wins_over_an_older_copy_next_to_the_exe() {
        use std::time::{Duration, SystemTime};
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let new = SystemTime::UNIX_EPOCH + Duration::from_secs(9_000);
        let hits = vec![
            (PathBuf::from("exe-side"), Some(old)),
            (PathBuf::from("repo-root"), Some(new)),
        ];
        assert_eq!(pick_freshest(&hits), Some(PathBuf::from("repo-root")));
    }

    /// 时间戳读不到 / 完全并列 → 退回原有的「候选顺序第一个」语义，行为不变。
    #[test]
    fn pick_freshest_falls_back_to_candidate_order() {
        use std::time::{Duration, SystemTime};
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(42);
        assert_eq!(
            pick_freshest(&[(PathBuf::from("a"), None), (PathBuf::from("b"), None)]),
            Some(PathBuf::from("a"))
        );
        assert_eq!(
            pick_freshest(&[(PathBuf::from("a"), Some(t)), (PathBuf::from("b"), Some(t))]),
            Some(PathBuf::from("a"))
        );
        // 有时间戳者胜过读不到时间戳者（后者无从比较，保守让位）。
        assert_eq!(
            pick_freshest(&[(PathBuf::from("a"), None), (PathBuf::from("b"), Some(t))]),
            Some(PathBuf::from("b"))
        );
        assert_eq!(pick_freshest(&[]), None);
    }

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

    /// 单文件分发（rdg）回归钉：一次 `pnpm build:remote` 必须产出并内嵌两套 UI。
    #[cfg(feature = "embed-ui")]
    #[test]
    fn embedded_ui_is_present_when_feature_on() {
        assert!(
            crate::embed_ui::has_ui(),
            "embed-ui 已开启但手机内嵌产物为空——构建前须先跑 `pnpm build:remote`"
        );
        assert!(crate::embed_ui::get("index.html").is_some_and(|b| !b.is_empty()));
        assert!(
            crate::embed_ui::has_kind(UiKind::Desktop),
            "embed-ui 已开启但桌面内嵌产物为空——构建前须先跑 `pnpm build:remote`"
        );
    }

    /// iter-62 串台钉：两套内嵌产物必须**真的不同**且各归其位。
    /// 桌面壳是 SvelteKit 全量 SPA（`_app/immutable` 入口），手机壳是 vite 轻量 SPA
    /// （`/assets` 入口）；一旦 `get_kind` 取错，电脑浏览器拿到的就是手机页。
    #[cfg(feature = "embed-ui")]
    #[test]
    fn embedded_desktop_and_mobile_shells_are_distinct() {
        let desktop = crate::embed_ui::get_kind(UiKind::Desktop, "index.html").expect("desktop");
        let mobile = crate::embed_ui::get_kind(UiKind::Mobile, "index.html").expect("mobile");
        assert_ne!(desktop, mobile, "桌面/手机内嵌壳不得是同一份");
        let d = String::from_utf8_lossy(&desktop);
        let m = String::from_utf8_lossy(&mobile);
        // 各自的**入口脚本**互斥：SvelteKit 走 `_app/immutable/entry/`，vite 轻量端走 `/assets/`。
        assert!(
            d.contains("_app/immutable/entry/"),
            "桌面壳应以 SvelteKit 入口起步: {}",
            &d[..d.len().min(400)]
        );
        assert!(m.contains("/assets/"), "手机壳应以 vite /assets 入口起步");
        assert!(!d.contains("src=\"/assets/"), "桌面壳不该是手机产物");
        // 桌面包的 SvelteKit 入口脚本必须也取得到，否则壳能开、页面白屏。
        assert!(crate::embed_ui::get_kind(UiKind::Desktop, "_app/version.json").is_some());
    }

    /// iter-62 回归钉：桌面 UA 的分流不得只看磁盘产物。
    #[cfg(feature = "embed-ui")]
    #[test]
    fn desktop_ua_uses_embedded_desktop_when_no_disk_dir() {
        let cfg = UaServeConfig {
            remote_dir: PathBuf::from("__no_such_remote_dist__"),
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::USER_AGENT,
            axum::http::HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64)"),
        );
        assert!(cfg.wants_desktop_ui(&headers, None));
        let target = cfg.ui_target(&headers, None);
        assert_eq!(target.kind, UiKind::Desktop);
        assert!(target.dir.is_none(), "桌面形态无磁盘产物时不得回落手机目录");

        let mut phone = HeaderMap::new();
        phone.insert(
            axum::http::header::USER_AGENT,
            axum::http::HeaderValue::from_static("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)"),
        );
        assert_eq!(cfg.ui_target(&phone, None).kind, UiKind::Mobile);
    }
}
