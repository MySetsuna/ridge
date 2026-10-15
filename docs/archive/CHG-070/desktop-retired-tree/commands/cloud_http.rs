//! 桌面侧 ridge-cloud HTTP 代理命令。
//!
//! 为什么存在：桌面 WebView（Windows 上是 WebView2，源 `http://tauri.localhost`）
//! 对 ridge-cloud 主域（`https://9527127.xyz`）的 `fetch` 是**跨域**请求，受浏览器
//! CORS 管控。云端 CORS allowlist 只放行 `https://tauri.localhost`，不放行 Windows
//! 默认的 `http://tauri.localhost`，于是登录等所有 cloud API 的 WebView fetch 都被
//! CORS 拦截 → `apiClient` 把 fetch 抛错映射成 `NETWORK`（「网络连接失败，请检查网络」）。
//!
//! 修复：桌面把 cloud HTTP 经本命令走 Rust（复用已有 `reqwest`）发出——Rust 不是浏览器、
//! 不受 CORS/CSP 约束，请求恒可达。web-remote（浏览器，同源访问云子域）仍走原生 fetch。
//! 见 `src/lib/remote/cloud/apiClient.ts` 的 `isTauri()` 分支。
//!
//! 安全：本命令可由前端发起任意 HTTP，故用**主机白名单**限定只能打 ridge-cloud
//! 主域 / 其子域（及本机回环，供 dev 指向本地 cloud），防被滥用为通用 SSRF 代理。

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;

/// ridge-cloud 主域（与前端 `apiClient.ts` 的 `BASE_DOMAIN` 默认值一致）。
const CLOUD_BASE_DOMAIN: &str = "9527127.xyz";

/// `cloud_http` 的返回：HTTP 状态码 + 原始响应体文本（前端按 §2 信封解析）。
#[derive(Serialize)]
pub struct CloudHttpResponse {
    pub status: u16,
    pub body: String,
}

/// 判定目标 host 是否允许：ridge-cloud 主域、其子域，或本机回环（dev 指向本地 cloud）。
fn is_allowed_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    if host == CLOUD_BASE_DOMAIN || host.ends_with(&format!(".{CLOUD_BASE_DOMAIN}")) {
        return true;
    }
    // dev：RIDGE_CLOUD_BASE_DOMAIN=localhost:xxxx 指向本地 cloud（含 *.localhost 子域信令）。
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if host == "127.0.0.1" || host == "::1" {
        return true;
    }
    false
}

/// 经 Rust 发一次 cloud HTTP 请求，绕过 WebView 跨域限制。
///
/// 仅允许打 ridge-cloud 主域/子域（+ 本机回环）。任何传输层失败统一回 `Err(String)`，
/// 前端据此抛 `ApiError('NETWORK', ...)`，与原 fetch 失败路径语义一致。
#[tauri::command]
pub async fn cloud_http(
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Option<String>,
) -> Result<CloudHttpResponse, String> {
    // 解析 + 校验目标 host（白名单），拒绝非 cloud 目标。
    // 用 reqwest 重导出的 Url，免新增直接依赖。
    let parsed = reqwest::Url::parse(&url).map_err(|e| format!("invalid url: {e}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("unsupported scheme: {other}")),
    }
    let host = parsed.host_str().unwrap_or_default();
    if !is_allowed_host(host) {
        return Err(format!("host not allowed: {host}"));
    }

    // reqwest::blocking 在阻塞线程池执行，避免阻塞 Tauri 主线程/异步运行时。
    tauri::async_runtime::spawn_blocking(move || {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?;

        let verb = reqwest::Method::from_bytes(method.to_ascii_uppercase().as_bytes())
            .map_err(|e| format!("invalid method: {e}"))?;
        let mut req = client.request(verb, &url);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        if let Some(b) = body {
            req = req.body(b);
        }

        let resp = req.send().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp.text().map_err(|e| e.to_string())?;
        Ok(CloudHttpResponse { status, body: text })
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}
