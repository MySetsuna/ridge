//! Domain C4 —— 与宿主无关的 MCP 服务端内核：方法分发 + 工具路由 + 收件箱 + Stash。
//!
//! 为什么在这里：MCP 原先只长在桌面 `src-tauri/src/teammate/server.rs` 里，rdg
//! 无头 host 完全没有，且 `tools/list` 广告的工具有一半 `tools/call` 不认。本模块
//! 把「协议 + 工具语义」收成一份，宿主只需实现 [`McpHost`]（拿状态、动 pane），
//! 传输（WS / HTTP）各自挂 10 行路由。桌面与 rdg 由此对同一套能力平权。
//!
//! 跨 agent 协作是目标：接入方可能是 Claude Code、Cursor 或任何 MCP 客户端，
//! 彼此没有共同的内部协议——它们只共享**这个 server**：花名册发现同伴、注入消息、
//! 派活、抓对方屏幕、收件箱异步回话、Stash 传大块产物。

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::protocol as proto;
use crate::registry::ToolRegistry;
use crate::resource::{RidgeUri, StashStore};

// ─── 宿主接口 ────────────────────────────────────────────────────────────────

/// 工具执行失败的语义分类（映射到 JSON-RPC 错误码）。
#[derive(Debug)]
pub enum HostError {
    /// 寻址/参数问题 → `-32602`。
    InvalidParams(String),
    /// 宿主内部失败 → `-32603`。
    Internal(String),
    /// 本宿主不提供该能力（如无头 host 无前端编组）→ `-32601`。
    Unsupported(String),
}

impl HostError {
    fn code(&self) -> i64 {
        match self {
            HostError::InvalidParams(_) => proto::INVALID_PARAMS,
            HostError::Internal(_) => proto::INTERNAL_ERROR,
            HostError::Unsupported(_) => proto::METHOD_NOT_FOUND,
        }
    }
    fn message(&self) -> String {
        match self {
            HostError::InvalidParams(m) | HostError::Internal(m) => m.clone(),
            HostError::Unsupported(m) => m.clone(),
        }
    }
}

pub type HostResult<T> = Result<T, HostError>;

/// 宿主必须提供的最小动作面。所有方法同步：桌面与 rdg 的实现都只做加锁读写，
/// 不做网络/子进程等慢操作（慢操作会拖住 teammate HTTP 的单线程 runtime）。
pub trait McpHost: Send + Sync {
    /// 花名册快照（roster + leader + edges + groups）。
    fn team_profile(&self) -> Value;

    /// 把文本写进目标 pane 的 stdin；`mark_busy` 为真时同时把该 pane 标记「工作中」。
    fn send_text(&self, target: &Value, text: &str, mark_busy: bool) -> HostResult<()>;

    /// 抓目标 pane 的**渲染后**屏幕文本（末 `lines` 行）。监控队友干活用。
    fn capture_pane(&self, target: &Value, lines: usize) -> HostResult<String>;

    /// 分屏并返回 `{ "paneId": ..., "paneIndex": ... }`。
    fn split_pane(&self, direction: &str, role: &str, initial_cmd: Option<&str>)
        -> HostResult<Value>;

    /// 把成员加入按名字寻址的编组（前端 SSOT，fire-and-forget）。
    fn join_group(
        &self,
        _group_name: &str,
        _agent_id: Option<&str>,
        _target: Option<&Value>,
    ) -> HostResult<()> {
        Err(HostError::Unsupported(
            "本宿主不支持 ridge_join_group（编组是桌面前端能力）".into(),
        ))
    }

    /// 进度回流：worker 主动汇报，宿主自行落前端事件/日志。
    fn report_progress(&self, _from: &Value, _status: &str, _detail: &str) -> HostResult<()> {
        Err(HostError::Unsupported(
            "本宿主不支持 ridge_report_progress".into(),
        ))
    }

    /// 读非 cache 的 `ridge://` 资源，返回 `(mimeType, text)`；cache 由内核直接处理。
    fn read_resource(&self, uri: &RidgeUri) -> HostResult<(String, String)>;

    /// 把寻址参数归一为**稳定字符串键**（收件箱按它分桶）。宿主须校验目标存在。
    fn pane_key(&self, target: &Value) -> HostResult<String>;
}

// ─── 进程内共享状态：Stash + 收件箱 ──────────────────────────────────────────

fn stash() -> &'static Mutex<StashStore> {
    static STASH: std::sync::OnceLock<Mutex<StashStore>> = std::sync::OnceLock::new();
    STASH.get_or_init(|| Mutex::new(StashStore::with_defaults()))
}

/// 每个 pane 一个内存收件箱（FIFO，上限 200 条）。
///
/// 为什么要它：stdin 注入是「打断式」的——对方正在跑命令时消息会被 shell 吃掉，
/// 且非同源 agent 没有回信通道。收件箱让任意 MCP 客户端**异步**收发：发送侧照旧
/// 注入 stdin（人也看得见），同时留一份可被 `ridge_inbox_read` 取走的副本。
fn inbox() -> &'static Mutex<HashMap<String, Vec<Value>>> {
    static INBOX: std::sync::OnceLock<Mutex<HashMap<String, Vec<Value>>>> =
        std::sync::OnceLock::new();
    INBOX.get_or_init(|| Mutex::new(HashMap::new()))
}

const INBOX_CAP: usize = 200;

fn inbox_push(key: &str, entry: Value) {
    let mut map = inbox().lock().unwrap();
    let q = map.entry(key.to_string()).or_default();
    q.push(entry);
    if q.len() > INBOX_CAP {
        let drop_n = q.len() - INBOX_CAP;
        q.drain(0..drop_n);
    }
}

/// 取走（默认）或窥视收件箱。取走后消息不再重复投递，避免 agent 反复处理旧消息。
fn inbox_take(key: &str, peek: bool) -> Vec<Value> {
    let mut map = inbox().lock().unwrap();
    match map.get_mut(key) {
        None => Vec::new(),
        Some(q) if peek => q.clone(),
        Some(q) => std::mem::take(q),
    }
}

// ─── 分发 ────────────────────────────────────────────────────────────────────

/// 处理一条 JSON-RPC 报文，返回应答字符串；**通知（无 id）返回 `None`**（按 JSON-RPC
/// 2.0：通知不得有响应；旧实现对 `notifications/initialized` 回 `-32601`，MCP 客户端
/// 握手时会看到一条伪错误）。
pub fn handle_message(text: &str, host: &dyn McpHost, version: &str) -> Option<String> {
    let req = match proto::parse_request(text.as_bytes()) {
        Ok(r) => r,
        Err(_) => {
            return Some(proto::mcp_error(Value::Null, proto::PARSE_ERROR, "parse error").to_string())
        }
    };
    if req.is_notification() {
        return None;
    }
    let id = req.id.clone();
    let resp = match req.method.as_str() {
        proto::METHOD_INITIALIZE => proto::mcp_result(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "agents-commune", "title": "Agent's Commune", "version": version },
                "capabilities": { "tools": {}, "resources": {} },
                "instructions": "Ridge 终端的多 agent 协作总线：先 ridge_get_team_profile 发现同伴，再 ridge_delegate_task 派活、ridge_capture_pane 看进展、ridge_inbox_read 收回话、ridge_stash_data 传大块产物。"
            }),
        ),
        proto::METHOD_PING => proto::mcp_result(id, json!({})),
        proto::METHOD_TOOLS_LIST => proto::mcp_result(id, ToolRegistry::default().tools_list_result()),
        proto::METHOD_TOOLS_CALL => tools_call(id, &req.params, host),
        proto::METHOD_RESOURCES_LIST => proto::mcp_result(id, resources_list()),
        proto::METHOD_RESOURCES_TEMPLATES_LIST => proto::mcp_result(
            id,
            json!({ "resourceTemplates": [ {
                "uriTemplate": "ridge://cache/{id}",
                "name": "stash blob",
                "description": "ridge_stash_data 暂存的中间产物",
                "mimeType": "text/plain"
            } ] }),
        ),
        proto::METHOD_RESOURCES_READ => resources_read(id, &req.params, host),
        other => proto::mcp_error(id, proto::METHOD_NOT_FOUND, &format!("method not found: {other}")),
    };
    Some(resp.to_string())
}

/// `resources/list`：静态列出可读资源（cache 条目不枚举，走模板）。
fn resources_list() -> Value {
    json!({ "resources": [
        {
            "uri": "ridge://workspace/active-panes",
            "name": "active-panes",
            "description": "当前工作区花名册：成员身份、paneId/paneIndex、状态、编组",
            "mimeType": "application/json"
        },
        {
            "uri": "ridge://workspace/git-status",
            "name": "git-status",
            "description": "工作区 Git 状态（分支 + 变更文件）",
            "mimeType": "application/json"
        },
        {
            "uri": "ridge://workspace/editor-context",
            "name": "editor-context",
            "description": "编辑器上下文：当前打开/活动的文件",
            "mimeType": "application/json"
        }
    ] })
}

fn resources_read(id: Value, params: &Value, host: &dyn McpHost) -> Value {
    let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
    match RidgeUri::parse(uri) {
        Ok(RidgeUri::Cache(cache_id)) => {
            let text = stash()
                .lock()
                .unwrap()
                .read(&cache_id)
                .map(|b| String::from_utf8_lossy(b).to_string());
            match text {
                Some(t) => proto::mcp_result(
                    id,
                    json!({ "contents": [ { "uri": uri, "mimeType": "text/plain", "text": t } ] }),
                ),
                None => proto::mcp_error(id, proto::INVALID_PARAMS, "cache 项不存在或已淘汰"),
            }
        }
        Ok(known) => match host.read_resource(&known) {
            Ok((mime, text)) => proto::mcp_result(
                id,
                json!({ "contents": [ { "uri": uri, "mimeType": mime, "text": text } ] }),
            ),
            Err(e) => proto::mcp_error(id, e.code(), &e.message()),
        },
        Err(_) => proto::mcp_error(id, proto::INVALID_PARAMS, "invalid ridge:// uri"),
    }
}

fn text_result(id: Value, text: impl Into<String>) -> Value {
    proto::mcp_result(id, json!({ "content": [ { "type": "text", "text": text.into() } ] }))
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
}

fn tools_call(id: Value, params: &Value, host: &dyn McpHost) -> Value {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let target = || args.get("target_pane_id").cloned().unwrap_or(Value::Null);

    let out: HostResult<String> = match name {
        "ridge_get_team_profile" => Ok(host.team_profile().to_string()),

        "ridge_send_to_teammate" | "ridge_delegate_task" => {
            let text = arg_str(&args, "message")
                .or_else(|| arg_str(&args, "objective"))
                .unwrap_or("");
            if text.is_empty() {
                Err(HostError::InvalidParams("message/objective 不能为空".into()))
            } else {
                let delegate = name == "ridge_delegate_task";
                let t = target();
                host.pane_key(&t).and_then(|key| {
                    host.send_text(&t, text, delegate).map(|()| {
                        inbox_push(
                            &key,
                            json!({
                                "from": arg_str(&args, "from").unwrap_or("mcp-client"),
                                "kind": if delegate { "task" } else { "message" },
                                "text": text,
                            }),
                        );
                        "delivered".to_string()
                    })
                })
            }
        }

        "ridge_capture_pane" => {
            let lines = args
                .get("lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(80)
                .clamp(1, 2000) as usize;
            host.capture_pane(&target(), lines)
        }

        "ridge_split_pane" => {
            let direction = arg_str(&args, "direction").unwrap_or("vertical");
            let role = arg_str(&args, "role").unwrap_or("worker");
            host.split_pane(direction, role, arg_str(&args, "initial_cmd"))
                .map(|v| v.to_string())
        }

        "ridge_join_group" => match arg_str(&args, "group_name") {
            None => Err(HostError::InvalidParams("group_name 不能为空".into())),
            Some(g) => {
                let t = target();
                let t_ref = (!t.is_null()).then_some(&t);
                host.join_group(g, arg_str(&args, "agent_id"), t_ref)
                    .map(|()| "dispatched".to_string())
            }
        },

        "ridge_report_progress" => {
            let status = arg_str(&args, "status").unwrap_or("update");
            let detail = arg_str(&args, "detail").unwrap_or("");
            let t = target();
            host.report_progress(&t, status, detail)
                .map(|()| "reported".to_string())
        }

        // 收发同一份内存队列：任何 MCP 客户端都能异步取走发给某 pane 的消息，
        // 不必依赖 stdin 注入被对方 shell 正确读到。
        "ridge_inbox_read" => {
            let peek = args.get("peek").and_then(|v| v.as_bool()).unwrap_or(false);
            host.pane_key(&target())
                .map(|key| Value::Array(inbox_take(&key, peek)).to_string())
        }

        "ridge_stash_data" => {
            // 规格历史写的是 content_base64、实现读的是 data —— 两个键都接，纯文本存。
            match arg_str(&args, "data").or_else(|| arg_str(&args, "content_base64")) {
                None => Err(HostError::InvalidParams("data 不能为空".into())),
                Some(d) => Ok(stash().lock().unwrap().stash_uri(d.as_bytes().to_vec())),
            }
        }

        // 未注册的工具名属**协议**错误（工具不存在）；宿主能力缺失走下面的 isError 结果。
        other => {
            return proto::mcp_error(
                id,
                proto::METHOD_NOT_FOUND,
                &format!("unknown tool: {other}"),
            )
        }
    };

    match out {
        Ok(text) => text_result(id, text),
        // MCP 约定：工具**执行**失败回 `isError` 结果而非 JSON-RPC 错误，客户端才好把
        // 原因交回模型自行改道（换宿主支持的工具），不至于当成协议崩坏。
        Err(HostError::Unsupported(m)) => proto::mcp_result(
            id,
            json!({ "content": [ { "type": "text", "text": m } ], "isError": true }),
        ),
        Err(e) => proto::mcp_error(id, e.code(), &e.message()),
    }
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost;

    impl McpHost for FakeHost {
        fn team_profile(&self) -> Value {
            json!({ "roster": [] })
        }
        fn send_text(&self, target: &Value, _text: &str, _busy: bool) -> HostResult<()> {
            if target.is_null() {
                return Err(HostError::InvalidParams("no target".into()));
            }
            Ok(())
        }
        fn capture_pane(&self, _t: &Value, lines: usize) -> HostResult<String> {
            Ok(format!("screen({lines})"))
        }
        fn split_pane(&self, d: &str, _r: &str, _c: Option<&str>) -> HostResult<Value> {
            Ok(json!({ "direction": d }))
        }
        fn read_resource(&self, _uri: &RidgeUri) -> HostResult<(String, String)> {
            Ok(("application/json".into(), "{}".into()))
        }
        fn pane_key(&self, target: &Value) -> HostResult<String> {
            if target.is_null() {
                Err(HostError::InvalidParams("no target".into()))
            } else {
                Ok(target.to_string())
            }
        }
    }

    fn call(msg: &str) -> Value {
        let out = handle_message(msg, &FakeHost, "test").expect("expected a response");
        serde_json::from_str(&out).unwrap()
    }

    #[test]
    fn notification_gets_no_response() {
        assert!(handle_message(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &FakeHost,
            "test"
        )
        .is_none());
    }

    #[test]
    fn resources_list_is_served() {
        let v = call(r#"{"jsonrpc":"2.0","id":1,"method":"resources/list"}"#);
        assert_eq!(v["result"]["resources"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn every_advertised_tool_is_routed() {
        // tools/list 广告什么，tools/call 就得认什么——历史缺陷正是广告了不认的工具。
        for spec in ToolRegistry::default().tools() {
            let msg = json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": { "name": spec.name, "arguments": {
                    "target_pane_id": 0, "message": "x", "objective": "x",
                    "group_name": "g", "data": "d", "role": "worker", "direction": "vertical"
                } }
            })
            .to_string();
            let v = call(&msg);
            assert!(
                v["error"]["code"].as_i64() != Some(proto::METHOD_NOT_FOUND),
                "工具 {} 被广告却未路由（tools/call 回 unknown tool）",
                spec.name
            );
        }
    }

    #[test]
    fn inbox_round_trip_take_once() {
        let send = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_to_teammate","arguments":{"target_pane_id":7,"message":"hi","from":"cursor"}}
        })
        .to_string();
        call(&send);
        let read = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_inbox_read","arguments":{"target_pane_id":7}}
        })
        .to_string();
        let v = call(&read);
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"from\":\"cursor\""), "收件箱应留副本: {text}");
        // 取走即清空
        let v2 = call(&read);
        assert_eq!(v2["result"]["content"][0]["text"].as_str().unwrap(), "[]");
    }

    #[test]
    fn stash_then_read_back() {
        let s = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_stash_data","arguments":{"data":"big blob"}}
        })
        .to_string();
        let uri = call(&s)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let r = json!({"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":uri}})
            .to_string();
        let v = call(&r);
        assert_eq!(v["result"]["contents"][0]["text"], "big blob");
    }

    #[test]
    fn unsupported_capability_is_tool_error_not_protocol_error() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_report_progress","arguments":{"target_pane_id":1,"status":"done"}}
        })
        .to_string();
        let v = call(&msg);
        assert!(v["error"].is_null(), "宿主能力缺失不该是协议错误");
        assert_eq!(v["result"]["isError"], true);
    }

    #[test]
    fn unknown_tool_is_method_not_found() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"no_such_tool","arguments":{}}
        })
        .to_string();
        assert_eq!(call(&msg)["error"]["code"], proto::METHOD_NOT_FOUND);
    }
}
