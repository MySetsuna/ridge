//! `ridge-mcp` —— Ridge 桌面内置 MCP 的 stdio 兼容桥。
//!
//! Codex 的 streamable HTTP MCP URL 为静态配置，而 Ridge 的本机端点与 token 会随
//! 后端重启更新。此桥按 `--url/--token` → pane 环境变量 → endpoint sidecar 发现，
//! 每次连接失败重试一次发现；它是 Ridge 的独立 companion，不依赖或调用 `rdg`。

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub base_url: String,
    pub token: String,
}

/// 可直接粘贴进常见 MCP 宿主的 stdio 配置；绝不展开瞬时端点或 token。
pub fn stdio_config(command: &Path) -> serde_json::Value {
    serde_json::json!({
        "mcpServers": {
            "ridge": {
                "command": command,
                "args": []
            }
        }
    })
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
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if !meta.file_type().is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                continue;
            }
        }
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
        let url = url.trim();
        let token = token.trim();
        if !is_loopback_endpoint(url) || token.is_empty() {
            continue;
        }
        best = Some((
            mtime,
            Endpoint {
                base_url: url.to_string(),
                token: token.to_string(),
            },
        ));
    }
    best.map(|(_, endpoint)| endpoint)
}

/// Discover the independent kernel MCP surface through the shared contract.
/// No Tauri sidecar, path convention, or duplicate JSON schema is involved.
fn kernel_endpoint() -> Option<Endpoint> {
    let endpoint = ridge_kernel::client::running_endpoint()?;
    let base_url = format!("http://127.0.0.1:{}", endpoint.port);
    Some(Endpoint {
        base_url,
        token: endpoint.token,
    })
}

/// 按「显式参数 → pane 环境 → **ridge-kernel 登记** → teammate sidecar」发现。
pub fn discover(url: Option<String>, token: Option<String>) -> Result<Endpoint> {
    let clean = |value: Option<String>| {
        value
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    let env = |key: &str| clean(std::env::var(key).ok());

    if let (Some(base_url), Some(token)) = (clean(url), clean(token)) {
        return Ok(Endpoint { base_url, token });
    }
    if let (Some(base_url), Some(token)) = (env("RIDGE_TEAMMATE_URL"), env("RIDGE_TEAMMATE_TOKEN"))
    {
        return Ok(Endpoint { base_url, token });
    }
    // 优先独立内核（深根 / 无 Tauri）；sidecar 仍兼容「仅桌面 teammate 在跑」。
    if let Some(ep) = kernel_endpoint() {
        return Ok(ep);
    }
    sidecar_endpoint().ok_or_else(|| {
        anyhow!(
            "找不到 Ridge MCP 端点：请启动 ridge-kernel / Ridge 桌面，或在 pane 内设置 RIDGE_TEAMMATE_*。"
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
        let response = forward_request(&client, &request, &mut endpoint, || {
            discover(url.clone(), token.clone())
        })
        .await;
        if let Some(body) = response {
            output
                .write_all(body.replace(['\r', '\n'], " ").as_bytes())
                .await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}

fn is_loopback_endpoint(url: &str) -> bool {
    url.strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.trim_end_matches('/').parse::<u16>().ok())
        .is_some_and(|port| port != 0)
}

fn rediscovery_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
}

async fn forward_request(
    client: &reqwest::Client,
    request: &str,
    endpoint: &mut Endpoint,
    mut rediscover: impl FnMut() -> Result<Endpoint>,
) -> Option<String> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match client
            .post(endpoint.mcp_url())
            .header("x-ridge-token", &endpoint.token)
            .header("content-type", "application/json")
            .body(request.to_owned())
            .send()
            .await
        {
            Ok(reply) if reply.status() == reqwest::StatusCode::ACCEPTED => return None,
            Ok(reply) if reply.status().is_success() => {
                return Some(reply.text().await.unwrap_or_default())
            }
            Ok(reply) if attempt == 1 && rediscovery_status(reply.status()) => match rediscover() {
                Ok(fresh) if fresh != *endpoint => *endpoint = fresh,
                _ => {
                    return Some(rpc_error(
                        request,
                        &format!("Ridge 返回 HTTP {}", reply.status()),
                    ))
                }
            },
            Ok(reply) => {
                return Some(rpc_error(
                    request,
                    &format!("Ridge 返回 HTTP {}", reply.status()),
                ))
            }
            Err(error) if attempt == 1 => match rediscover() {
                Ok(fresh) if fresh != *endpoint => {
                    *endpoint = fresh;
                }
                _ => return Some(rpc_error(request, &format!("连接 Ridge MCP 失败: {error}"))),
            },
            Err(error) => {
                return Some(rpc_error(request, &format!("连接 Ridge MCP 失败: {error}")))
            }
        }
    }
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    #[test]
    fn printed_config_contains_only_companion_command() {
        let config = stdio_config(Path::new("/opt/ridge/ridge-mcp"));
        let text = config.to_string();
        assert!(text.contains("/opt/ridge/ridge-mcp"));
        assert!(!text.contains("token"));
        assert!(!text.contains("127.0.0.1"));
    }

    #[test]
    fn discovery_accepts_only_loopback_and_auth_failures_rotate() {
        assert!(is_loopback_endpoint("http://127.0.0.1:47615"));
        assert!(!is_loopback_endpoint("http://0.0.0.0:47615"));
        assert!(!is_loopback_endpoint("https://example.com:47615"));
        assert!(rediscovery_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(rediscovery_status(reqwest::StatusCode::FORBIDDEN));
        assert!(!rediscovery_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
    }

    #[tokio::test]
    async fn first_connection_failure_rediscovers_and_retries_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let fresh_url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 35\r\nconnection: close\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}",
                )
                .unwrap();
        });
        let stale_port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut endpoint = Endpoint {
            base_url: format!("http://127.0.0.1:{stale_port}"),
            token: "old".into(),
        };
        let rediscoveries = AtomicUsize::new(0);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = forward_request(
            &client,
            r#"{"jsonrpc":"2.0","id":7,"method":"initialize"}"#,
            &mut endpoint,
            || {
                rediscoveries.fetch_add(1, Ordering::SeqCst);
                Ok(Endpoint {
                    base_url: fresh_url.clone(),
                    token: "new".into(),
                })
            },
        )
        .await;
        server.join().unwrap();
        assert_eq!(rediscoveries.load(Ordering::SeqCst), 1);
        assert_eq!(endpoint.token, "new");
        assert!(response.unwrap().contains("\"id\":7"));
    }

    #[tokio::test]
    async fn unauthorized_rediscovery_retries_with_rotated_token() {
        let stale = TcpListener::bind("127.0.0.1:0").unwrap();
        let stale_url = format!("http://{}", stale.local_addr().unwrap());
        let stale_server = std::thread::spawn(move || {
            let (mut stream, _) = stale.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let fresh = TcpListener::bind("127.0.0.1:0").unwrap();
        let fresh_url = format!("http://{}", fresh.local_addr().unwrap());
        let fresh_server = std::thread::spawn(move || {
            let (mut stream, _) = fresh.accept().unwrap();
            let mut request = [0_u8; 2048];
            let read = stream.read(&mut request).unwrap();
            assert!(String::from_utf8_lossy(&request[..read]).contains("x-ridge-token: new"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 35\r\nconnection: close\r\n\r\n{\"jsonrpc\":\"2.0\",\"id\":8,\"result\":{}}",
                )
                .unwrap();
        });
        let mut endpoint = Endpoint {
            base_url: stale_url,
            token: "old".into(),
        };
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = forward_request(
            &client,
            r#"{"jsonrpc":"2.0","id":8,"method":"initialize"}"#,
            &mut endpoint,
            || {
                Ok(Endpoint {
                    base_url: fresh_url.clone(),
                    token: "new".into(),
                })
            },
        )
        .await;
        stale_server.join().unwrap();
        fresh_server.join().unwrap();
        assert_eq!(endpoint.token, "new");
        assert!(response.unwrap().contains("\"id\":8"));
    }
}
