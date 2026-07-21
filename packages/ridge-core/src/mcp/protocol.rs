//! Domain C1 —— JSON-RPC 2.0 / MCP 报文类型。
//!
//! 纯协议层：报文结构 + 构造/解析辅助。传输（axum WS/SSE 挂载、`tools/call`
//! 路由到拓扑总线）在 `src-tauri` 接线，复用 `remote/server.rs` 的 WS+JSON-RPC
//! 模式。构造辅助返回 `serde_json::Value`，与既有 `jsonrpc_result`/`jsonrpc_error`
//! 习惯一致。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ── MCP 标准方法名 ──
pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_TOOLS_LIST: &str = "tools/list";
pub const METHOD_TOOLS_CALL: &str = "tools/call";
pub const METHOD_RESOURCES_READ: &str = "resources/read";
pub const METHOD_NOTIFY_PROGRESS: &str = "notifications/progress";

// ── JSON-RPC 2.0 标准错误码 ──
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// 一个 JSON-RPC 2.0 请求（从线上解析得到）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC 错误对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpErrorObj {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// 一个 JSON-RPC 2.0 响应。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorObj>,
}

/// 构造一个成功响应 `{"jsonrpc":"2.0","id":id,"result":result}`。
pub fn mcp_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 构造一个错误响应 `{"jsonrpc":"2.0","id":id,"error":{code,message}}`。
pub fn mcp_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// 构造一个通知 `{"jsonrpc":"2.0","method":method,"params":params}`（无 id）。
pub fn mcp_notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "method": method, "params": params })
}

/// C3 —— 异步 Fire-and-Forget 任务回执。Leader 派活后立即拿到，终端焦点不被锁死。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTicket {
    pub task_id: String,
    pub assigned_pane: u32,
    /// 固定 "dispatched"。
    pub status: String,
    pub message: String,
}

impl TaskTicket {
    pub fn dispatched(task_id: impl Into<String>, assigned_pane: u32) -> Self {
        Self {
            task_id: task_id.into(),
            assigned_pane,
            status: "dispatched".to_string(),
            message: "Task is running in background. You will receive an MCP notification upon completion."
                .to_string(),
        }
    }
}

/// C3 —— `notifications/progress` 的 params（`progressToken` 走 camelCase）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressParams {
    #[serde(rename = "progressToken")]
    pub progress_token: String,
    pub data: Value,
}

/// 构造一条完整的 `notifications/progress` 通知。
pub fn progress_notification(token: &str, data: Value) -> Value {
    mcp_notification(
        METHOD_NOTIFY_PROGRESS,
        json!({ "progressToken": token, "data": data }),
    )
}

/// MCP 报文解析错误。
#[derive(Debug, thiserror::Error)]
pub enum McpProtocolError {
    #[error("JSON 解析失败: {0}")]
    BadJson(#[from] serde_json::Error),
    #[error("不是 JSON-RPC 2.0 报文")]
    NotJsonRpc2,
}

/// 解析一个 JSON-RPC 2.0 请求字节串，校验 `jsonrpc == "2.0"`。
pub fn parse_request(bytes: &[u8]) -> Result<McpRequest, McpProtocolError> {
    let req: McpRequest = serde_json::from_slice(bytes)?;
    if req.jsonrpc != "2.0" {
        return Err(McpProtocolError::NotJsonRpc2);
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tools_call_request() {
        let raw = br#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"ridge_split_pane"}}"#;
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, METHOD_TOOLS_CALL);
        assert_eq!(req.id, json!(7));
        assert_eq!(req.params["name"], "ridge_split_pane");
    }

    #[test]
    fn parse_request_rejects_non_2_0() {
        let raw = br#"{"jsonrpc":"1.0","id":1,"method":"x"}"#;
        assert!(matches!(
            parse_request(raw),
            Err(McpProtocolError::NotJsonRpc2)
        ));
    }

    #[test]
    fn parse_request_defaults_missing_params() {
        let raw = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let req = parse_request(raw).unwrap();
        assert!(req.params.is_null());
    }

    #[test]
    fn mcp_result_shape() {
        let v = mcp_result(json!(1), json!({"ok": true}));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["ok"], true);
        assert!(v.get("error").is_none());
    }

    #[test]
    fn mcp_error_shape() {
        let v = mcp_error(json!("abc"), METHOD_NOT_FOUND, "no such method");
        assert_eq!(v["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(v["error"]["message"], "no such method");
        assert!(v.get("result").is_none());
    }

    #[test]
    fn notification_has_no_id() {
        let v = mcp_notification("notifications/progress", json!({"x": 1}));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["method"], "notifications/progress");
        assert!(v.get("id").is_none());
    }

    #[test]
    fn task_ticket_serializes() {
        let t = TaskTicket::dispatched("tsk_9988", 2);
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["task_id"], "tsk_9988");
        assert_eq!(v["assigned_pane"], 2);
        assert_eq!(v["status"], "dispatched");
    }

    #[test]
    fn progress_notification_uses_camel_case_token() {
        let v = progress_notification("tsk_9988", json!({"status": "completed"}));
        assert_eq!(v["method"], METHOD_NOTIFY_PROGRESS);
        assert_eq!(v["params"]["progressToken"], "tsk_9988");
        assert_eq!(v["params"]["data"]["status"], "completed");
    }

    #[test]
    fn progress_params_roundtrip_camel_case() {
        let p = ProgressParams {
            progress_token: "t1".into(),
            data: json!({"n": 1}),
        };
        let v = serde_json::to_value(&p).unwrap();
        assert!(v.as_object().unwrap().contains_key("progressToken"));
        let back: ProgressParams = serde_json::from_value(v).unwrap();
        assert_eq!(back.progress_token, "t1");
    }
}
