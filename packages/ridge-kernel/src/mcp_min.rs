//! 内核内嵌最小 MCP 面（REQ-RIDGE-MCP-AS-KERNEL-API-01）。
//! initialize + tools/list + tools/call（领域只读工具）；不依赖 Tauri。

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde_json::{json, Value};

use crate::domain::builtin_agent_profiles;
use crate::AppState;

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    if headers
        .get("x-ridge-token")
        .or_else(|| headers.get("x-ridge-kernel-token"))
        .and_then(|v| v.to_str().ok())
        == Some(token)
    {
        return true;
    }
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|v| v == token)
}

fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "ridge_kernel_list_agents",
                "description": "List agent launch profiles from ridge-kernel (domain SSOT slice).",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "ridge_kernel_fs_list",
                "description": "List directory children via ridge-kernel FS domain API.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "offset": { "type": "integer" },
                        "limit": { "type": "integer" }
                    },
                    "required": ["path"]
                }
            }
        ]
    })
}

fn tool_result_text(id: Value, text: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }]
        }
    })
}

fn tool_error(id: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": -32603, "message": message }
    })
}

/// 处理单条 JSON-RPC；通知返回 None。
pub fn handle_rpc(body: &str) -> Option<Value> {
    let req: Value = serde_json::from_str(body).ok()?;
    let method = req.get("method")?.as_str()?;
    let id = req.get("id").cloned();
    // 通知
    if id.is_none() {
        return None;
    }
    let id = id.unwrap();
    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "ridge-kernel", "version": env!("CARGO_PKG_VERSION") }
            }
        })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": tools_list()
        })),
        "tools/call" => {
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match name {
                "ridge_kernel_list_agents" => {
                    let text = serde_json::to_string_pretty(&json!({
                        "source": "ridge-kernel",
                        "profiles": builtin_agent_profiles(),
                    }))
                    .unwrap_or_else(|_| "{}".into());
                    Some(tool_result_text(id, text))
                }
                "ridge_kernel_fs_list" => {
                    let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                    if path.is_empty() {
                        return Some(tool_error(id, "path required"));
                    }
                    let offset = args.get("offset").and_then(|v| v.as_u64()).map(|n| n as usize);
                    let limit = args.get("limit").and_then(|v| v.as_u64()).map(|n| n as usize);
                    match ridge_core::fs::commands::get_directory_children(path, offset, limit) {
                        Ok(page) => {
                            let text = serde_json::to_string_pretty(&page)
                                .unwrap_or_else(|_| "{}".into());
                            Some(tool_result_text(id, text))
                        }
                        Err(e) => Some(tool_error(id, &e.to_string())),
                    }
                }
                other => Some(tool_error(id, &format!("unknown tool: {other}"))),
            }
        }
        "ping" => Some(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
        other => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("method not found: {other}") }
        })),
    }
}

pub async fn route_mcp(
    State(st): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    if !auth_ok(&headers, &st.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    match handle_rpc(&body) {
        None => (StatusCode::ACCEPTED, "").into_response(),
        Some(v) => (StatusCode::OK, Json(v)).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_tools_list() {
        let init = handle_rpc(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .expect("init");
        assert_eq!(init["result"]["serverInfo"]["name"], "ridge-kernel");
        let list = handle_rpc(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).expect("list");
        let tools = list["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "ridge_kernel_list_agents"));
    }

    #[test]
    fn call_list_agents() {
        let r = handle_rpc(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ridge_kernel_list_agents","arguments":{}}}"#,
        )
        .unwrap();
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("claude") || text.contains("profiles"));
    }
}
