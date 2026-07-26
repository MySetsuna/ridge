//! `rdg mcp` —— stdio ↔ 本机 Ridge MCP 端点的桥。
//!
//! 为什么要它：MCP 客户端（Claude Code 等）只支持 stdio / sse / http，而 Ridge 的
//! 端点是**每次启动都换的** ephemeral 端口 + 随机 token（桌面 `127.0.0.1:<port>`，
//! 无头 `rdg tmux` 同理）。静态 URL 配置注定过期，用户就只能自己写桥。
//!
//! 有了本子命令，接入是一行、随安装即用、桌面与 rdg 通吃：
//!
//! ```text
//! claude mcp add ridge -- rdg mcp
//! ```
//!
//! 端点发现顺序：`--url/--token` → `RIDGE_TEAMMATE_URL/_TOKEN`（Ridge 注入进每个
//! pane 的 env）→ 临时目录里的 `ridge-teammate-endpoint-*.json` sidecar（后端重启
//! 换端口后由宿主刷新）。每条请求失败都会重新发现一次，端点漂移可自愈。

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub base_url: String,
    pub token: String,
}

impl Endpoint {
    fn mcp_url(&self) -> String {
        format!("{}/api/v1/mcp", self.base_url.trim_end_matches('/'))
    }
}

/// 扫临时目录里最新的 endpoint sidecar。宿主起 teammate 服务时写，重启换端口时刷新。
fn sidecar_endpoint() -> Option<Endpoint> {
    let dir: PathBuf = std::env::temp_dir();
    let mut best: Option<(std::time::SystemTime, Endpoint)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("ridge-teammate-endpoint-") || !name.ends_with(".json") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        if best.as_ref().is_some_and(|(t, _)| *t >= mtime) {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) else {
            continue;
        };
        let (Some(url), Some(token)) = (v["url"].as_str(), v["token"].as_str()) else {
            continue;
        };
        best = Some((
            mtime,
            Endpoint {
                base_url: url.trim().to_string(),
                token: token.trim().to_string(),
            },
        ));
    }
    best.map(|(_, e)| e)
}

/// 按「命令行 > env > sidecar」发现端点。
pub fn discover(url: Option<String>, token: Option<String>) -> Result<Endpoint> {
    let clean = |s: Option<String>| s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let env = |k: &str| clean(std::env::var(k).ok());

    if let (Some(u), Some(t)) = (clean(url), clean(token)) {
        return Ok(Endpoint {
            base_url: u,
            token: t,
        });
    }
    if let (Some(u), Some(t)) = (env("RIDGE_TEAMMATE_URL"), env("RIDGE_TEAMMATE_TOKEN")) {
        return Ok(Endpoint {
            base_url: u,
            token: t,
        });
    }
    sidecar_endpoint().ok_or_else(|| {
        anyhow!(
            "找不到 Ridge MCP 端点：请在 Ridge 分屏里运行（会注入 RIDGE_TEAMMATE_URL/_TOKEN），\
             或先起无头引擎 `rdg tmux`，或显式传 --url/--token。"
        )
    })
}

/// 逐行读 stdin 的 JSON-RPC，POST 到宿主，再把应答逐行写回 stdout。
/// 宿主对通知回 `202` 无体——此时不写 stdout（JSON-RPC 通知本就没有响应）。
pub async fn run(url: Option<String>, token: Option<String>) -> Result<()> {
    let mut ep = discover(url.clone(), token.clone())?;
    eprintln!("[rdg mcp] bridging stdio → {}", ep.mcp_url());

    // `.no_proxy()`：端点恒在 127.0.0.1，而开发机普遍设了 HTTP_PROXY/HTTPS_PROXY。
    // reqwest 默认吃 env 代理 → 本机请求被丢给代理，表现为**整条桥静默挂死**（实测）。
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .context("build http client")?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        let mut attempt = 0;
        let reply = loop {
            attempt += 1;
            let res = client
                .post(ep.mcp_url())
                .header("x-ridge-token", &ep.token)
                .header("content-type", "application/json")
                .body(line.clone())
                .send()
                .await;
            match res {
                Ok(resp) if resp.status() == reqwest::StatusCode::ACCEPTED => break None,
                Ok(resp) if resp.status().is_success() => {
                    break Some(resp.text().await.unwrap_or_default())
                }
                Ok(resp) => {
                    break Some(rpc_error(
                        &line,
                        &format!("宿主返回 {}", resp.status().as_u16()),
                    ))
                }
                // 端点漂移（宿主重启换端口/令牌）：重新发现一次再试，失败才回错。
                Err(e) if attempt == 1 => match discover(url.clone(), token.clone()) {
                    Ok(fresh) if fresh != ep => {
                        eprintln!("[rdg mcp] 端点已变，改连 {}", fresh.mcp_url());
                        ep = fresh;
                        continue;
                    }
                    _ => break Some(rpc_error(&line, &format!("连接失败: {e}"))),
                },
                Err(e) => break Some(rpc_error(&line, &format!("连接失败: {e}"))),
            }
        };
        if let Some(body) = reply {
            out.write_all(body.replace('\n', " ").as_bytes()).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }
    Ok(())
}

/// 用请求里的 id 拼一条 JSON-RPC 错误，客户端才不会一直等这条请求。
fn rpc_error(request_line: &str, message: &str) -> String {
    let id = serde_json::from_str::<serde_json::Value>(request_line)
        .ok()
        .and_then(|v| v.get("id").cloned())
        .unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32603, "message": message }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_args_win_over_env() {
        let ep = discover(Some("http://h:1".into()), Some("tok".into())).unwrap();
        assert_eq!(ep.base_url, "http://h:1");
        assert_eq!(ep.mcp_url(), "http://h:1/api/v1/mcp");
    }

    #[test]
    fn rpc_error_keeps_request_id() {
        let e = rpc_error(r#"{"jsonrpc":"2.0","id":42,"method":"tools/list"}"#, "boom");
        assert!(e.contains("\"id\":42"), "{e}");
    }

    #[test]
    fn rpc_error_tolerates_unparsable_line() {
        assert!(rpc_error("not json", "boom").contains("\"id\":null"));
    }
}
