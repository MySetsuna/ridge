//! `ridge-mcp` —— Ridge 桌面内置 MCP 的 stdio 兼容桥。
//!
//! Codex 的 streamable HTTP MCP URL 为静态配置，而 Ridge 的本机端点与 token 会随
//! 后端重启更新。此桥按 `--url/--token` → pane 环境变量 → endpoint sidecar 发现，
//! 每次连接失败重试一次发现；它是 Ridge 的独立 companion，不依赖或调用 `rdg`。

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

/// 扫临时目录最新的 endpoint sidecar。桌面后端重启时会刷新这个文件。
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
        if best.as_ref().is_some_and(|(time, _)| *time >= mtime) {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) else {
            continue;
        };
        let (Some(url), Some(token)) = (value["url"].as_str(), value["token"].as_str()) else {
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
    best.map(|(_, endpoint)| endpoint)
}

/// 按「显式参数 → Ridge pane 环境 → endpoint sidecar」发现当前端点。
pub fn discover(url: Option<String>, token: Option<String>) -> Result<Endpoint> {
    let clean = |value: Option<String>| value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty());
    let env = |key: &str| clean(std::env::var(key).ok());

    if let (Some(base_url), Some(token)) = (clean(url), clean(token)) {
        return Ok(Endpoint { base_url, token });
    }
    if let (Some(base_url), Some(token)) = (env("RIDGE_TEAMMATE_URL"), env("RIDGE_TEAMMATE_TOKEN")) {
        return Ok(Endpoint { base_url, token });
    }
    sidecar_endpoint().ok_or_else(|| {
        anyhow!(
            "找不到 Ridge MCP 端点：请在 Ridge pane 内启动 Codex，或先打开 Ridge 工作区。"
        )
    })
}

/// 逐行转发 stdio JSON-RPC 到 Ridge HTTP MCP。通知的 `202` 不写 stdout。
pub async fn run(url: Option<String>, token: Option<String>) -> Result<()> {
    let client = reqwest::Client::builder()
        // Ridge 端点恒为本机；开发机代理不应截获该请求。
        .no_proxy()
        .build()
        .context("创建 Ridge MCP HTTP client 失败")?;
    let mut endpoint = discover(url.clone(), token.clone())?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let request = line.trim().to_string();
        if request.is_empty() {
            continue;
        }
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            match client
                .post(endpoint.mcp_url())
                .header("x-ridge-token", &endpoint.token)
                .header("content-type", "application/json")
                .body(request.clone())
                .send()
                .await
            {
                Ok(reply) if reply.status() == reqwest::StatusCode::ACCEPTED => break None,
                Ok(reply) if reply.status().is_success() => break Some(reply.text().await.unwrap_or_default()),
                Ok(reply) => break Some(rpc_error(&request, &format!("Ridge 返回 HTTP {}", reply.status()))),
                Err(error) if attempt == 1 => match discover(url.clone(), token.clone()) {
                    Ok(fresh) if fresh != endpoint => {
                        endpoint = fresh;
                        continue;
                    }
                    _ => break Some(rpc_error(&request, &format!("连接 Ridge MCP 失败: {error}"))),
                },
                Err(error) => break Some(rpc_error(&request, &format!("连接 Ridge MCP 失败: {error}"))),
            }
        };
        if let Some(body) = response {
            output.write_all(body.replace(['\r', '\n'], " ").as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}

fn rpc_error(request: &str, message: &str) -> String {
    let id = serde_json::from_str::<serde_json::Value>(request)
        .ok()
        .and_then(|value| value.get("id").cloned())
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
    fn explicit_arguments_win_over_discovery() {
        let endpoint = discover(Some("http://127.0.0.1:9".into()), Some("token".into())).unwrap();
        assert_eq!(endpoint.mcp_url(), "http://127.0.0.1:9/api/v1/mcp");
    }

    #[test]
    fn rpc_error_keeps_request_id() {
        assert!(rpc_error(r#"{"jsonrpc":"2.0","id":42}"#, "boom").contains("\"id\":42"));
    }

    #[test]
    fn rpc_error_tolerates_bad_json() {
        assert!(rpc_error("not-json", "boom").contains("\"id\":null"));
    }
}
