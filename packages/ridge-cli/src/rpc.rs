//! JSON-RPC 2.0 host 腿 + D9 `$/hello` 能力协商（统一远控 S3 / 契约 §7.3 / §11.1）。
//!
//! 本模块把桌面 host（`src/lib/remote/cloud/cloudHostBridge.ts`）的 controller↔host
//! 线协议在 cli 侧**逐字段复刻**：
//!   - JSON-RPC 2.0 信封：`{jsonrpc:"2.0", id?, method, params}` 请求 / 通知，
//!     `{jsonrpc:"2.0", id, result|error}` 响应。
//!   - D9 `$/hello`：controller 先发 `{protocolVersion, capabilities}`；host 回本端
//!     `$/hello`（能力取交集）或 `$/bye`（版本不兼容）。
//!
//! 与桌面 host 的关键差异在**能力集**：cli 是 terminal-only host，只公告它真能服务的
//! 能力（见 [`CLI_CAPABILITIES`]），controller 的 `hasCapability` 据此灰掉 IDE 面板
//! （git/workspace/theme/invoke），优雅降级——这正是契约 §11.1 「reduced-capability
//! host」的设计意图。
//!
//! 纯模块：只做帧解析 + 路由判定 + 响应构造，不碰 PTY / fs / E2EE（那些在 session.rs
//! 由本模块的判定结果驱动）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 本 host 实现的协议版本（与桌面 `cloudHostBridge.ts` 的 `HOST_PROTOCOL_VERSION`、
/// server.rs `REMOTE_PROTOCOL_VERSION` 对齐）。
pub const HOST_PROTOCOL_VERSION: u64 = 1;

/// cli host 公告的能力集（契约 §7.3 / §11.1：terminal/pty + fs 搜索/树）。
///
/// **刻意是桌面 `HOST_CAPABILITIES` 的子集**：cli 不是 IDE host，不服务
/// invoke（任意命令）/ git / workspace 管理 / theme。controller 取交集后灰掉那些面板，
/// 只保留终端 + 文件搜索/树，故同一 SPA controller 能驱动 cli 而不崩。
///
/// 注：cli **只读呈现**自己的固定单工作区（`list_workspaces`/`get_pane_layout`/
/// `get_active_workspace_id`），让 SPA 启动 `refreshWorkspaces` 能装配出唯一终端 pane；
/// 这不等于公告 `workspace` 管理能力（新建/重命名/关闭工作区仍不支持）。
///   - `pane`   —— 终端 pane 订阅 + PTY 字节流（write_to_pty / resize_pane / subscribe-pane）。
///   - `fs`     —— 文件树（get_directory_children）。
///   - `search` —— 文本搜索（search）。
pub const CLI_CAPABILITIES: &[&str] = &["pane", "fs", "search"];

/// D9 握手方法名（契约 §7.3）。
pub const HELLO_METHOD: &str = "$/hello";
/// D9 版本不匹配 teardown 方法名（契约 §7.3）。
pub const BYE_METHOD: &str = "$/bye";
/// 取消长任务方法名（契约 §7.0）。
pub const CANCEL_METHOD: &str = "$/cancel";

/// JSON-RPC 2.0 标准保留错误码（host 腿用到的子集）。
pub const JSON_RPC_INVALID_REQUEST: i64 = -32600;
pub const JSON_RPC_METHOD_NOT_FOUND: i64 = -32601;
pub const JSON_RPC_INTERNAL_ERROR: i64 = -32603;

/// 解析后的一帧 0x11 JSON-RPC 信封。host 只关心请求 / 通知（host 不向 controller
/// 发请求，故收到的 response 无意义 → [`Envelope::Ignore`]）。
#[derive(Debug, Clone, PartialEq)]
pub enum Envelope {
    /// 带 id 的请求：需要回 result/error。
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// 无 id 的通知：按语义处理，不回响应。
    Notification { method: String, params: Value },
    /// 非请求/通知（缺 method 的 response、坏帧等）→ 忽略。
    Ignore,
}

/// 解析一段 0x11 JSON 字节为 JSON-RPC 信封。坏 JSON / 非对象 / 缺 jsonrpc 字段 →
/// [`Envelope::Ignore`]（与桌面 host「记录并丢弃」一致，永不 panic）。
pub fn parse_envelope(body: &[u8]) -> Envelope {
    let value: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Envelope::Ignore,
    };
    let obj = match value.as_object() {
        Some(o) => o,
        None => return Envelope::Ignore,
    };
    // 桌面 host 要求 jsonrpc:"2.0"，缺失即忽略。
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Envelope::Ignore;
    }
    let method = match obj.get("method").and_then(Value::as_str) {
        Some(m) => m.to_string(),
        None => return Envelope::Ignore, // 无 method 的 response → 忽略
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    match obj.get("id") {
        Some(id) if !id.is_null() => Envelope::Request {
            id: id.clone(),
            method,
            params,
        },
        _ => Envelope::Notification { method, params },
    }
}

/// 构造 JSON-RPC 成功响应 `{jsonrpc:"2.0", id, result}`。
pub fn result_response(id: &Value, result: Value) -> Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// JSON-RPC 错误对象 `{code, message, data?}`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// 构造 JSON-RPC 错误响应 `{jsonrpc:"2.0", id, error}`。
pub fn error_response(id: &Value, error: &RpcError) -> Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// 根据 controller 的 `$/hello` params 计算 host 回复（与桌面 `negotiateHello`
/// 逐字段对齐）：返回 `$/hello` 通知（兼容版本，能力取交集）或 `$/bye` 通知
/// （无公共版本）。
pub fn negotiate_hello(params: &Value) -> Value {
    let peer_version = params
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if peer_version < HOST_PROTOCOL_VERSION {
        return serde_json::json!({
            "jsonrpc": "2.0",
            "method": BYE_METHOD,
            "params": { "reason": "protocol-version-mismatch" },
        });
    }
    // peerCaps 为空 ⇒ 不约束（与桌面 `peerCaps.size === 0` 分支一致）。
    let peer_caps: Vec<&str> = params
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let agreed: Vec<&str> = CLI_CAPABILITIES
        .iter()
        .copied()
        .filter(|c| peer_caps.is_empty() || peer_caps.contains(c))
        .collect();
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": HELLO_METHOD,
        "params": { "protocolVersion": HOST_PROTOCOL_VERSION, "capabilities": agreed },
    })
}

/// 业务方法路由判定（纯）：把 controller 的 invoke 方法名映射成 cli 能服务的动作。
/// session.rs 据此驱动 PTY / fs，再用本模块的 `result_response`/`error_response` 回帧。
#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    /// `write_to_pty { paneId, data }` → 写 PTY 输入。
    WritePty { data: String },
    /// `resize_pane { rows, cols, … }` → resize PTY。
    ResizePane { cols: u16, rows: u16 },
    /// `get_active_workspace_id` → 返回 cli 的固定 workspace id。
    GetActiveWorkspaceId,
    /// `list_workspaces` → cli 单工作区列表（只读呈现，非工作区管理能力）。
    /// 桌面 SPA 启动 `refreshWorkspaces` 会调它；cli 是固定单工作区 host，回一条即可，
    /// 否则 refreshWorkspaces 抛错令整个 boot IIFE 中断、终端永不渲染。
    ListWorkspaces,
    /// `get_pane_layout` → cli 单 pane 布局（leaf=CLI_PANE_ID）。同上由 boot 调，
    /// 回单 leaf 让 SPA 渲染唯一终端 pane 并据此订阅 PTY 流。
    GetPaneLayout,
    /// `search { root, query, useRegex, caseSensitive }` → fs 搜索。
    Search {
        root: String,
        query: String,
        use_regex: bool,
        case_sensitive: bool,
    },
    /// `get_directory_children { path, … }` → 列目录。
    DirectoryChildren { path: String },
    /// cli 不服务的方法（IDE 命令等）→ 回 METHOD_NOT_FOUND。
    Unsupported(String),
}

/// 把 (method, params) 解析为 [`Method`]。参数缺失/类型不符时尽量取默认（与桌面
/// host 的 `normalizeParams` 宽容策略一致），无法服务的方法落到 `Unsupported`。
pub fn route_method(method: &str, params: &Value) -> Method {
    match method {
        // 终端输入：桌面 SPA 经 shim `write_to_pty { paneId, data }`（RidgePane.svelte）。
        "write_to_pty" | "write_pty" => Method::WritePty {
            data: params
                .get("data")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
        // 终端 resize：桌面 SPA 经 `resize_pane { workspaceId, paneId, rows, cols, … }`。
        "resize_pane" | "resize_pty" => Method::ResizePane {
            cols: params.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16,
            rows: params.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16,
        },
        // cloudPaneSource 解析活动 ws 以拼 `pty-output-{ws}-{pane}` event 名。
        "get_active_workspace_id" => Method::GetActiveWorkspaceId,
        // 桌面 SPA `refreshWorkspaces` 启动调用：cli 单工作区/单 pane host 据实回桩，
        // 否则 boot 中断（详见各 Method 变体注释 + session.rs dispatch）。
        "list_workspaces" => Method::ListWorkspaces,
        "get_pane_layout" => Method::GetPaneLayout,
        // 文本搜索（契约 §9）：字段名与桌面 fs::search 入参一致（camelCase）。
        "search" => Method::Search {
            root: params
                .get("root")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            query: params
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            use_regex: params
                .get("useRegex")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            case_sensitive: params
                .get("caseSensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        // 文件树（契约 §9）。
        "get_directory_children" => Method::DirectoryChildren {
            path: params
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        },
        other => Method::Unsupported(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_request_with_id() {
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": 7, "method": "write_to_pty",
            "params": { "paneId": "p", "data": "ls\n" }
        }))
        .unwrap();
        match parse_envelope(&body) {
            Envelope::Request { id, method, params } => {
                assert_eq!(id, json!(7));
                assert_eq!(method, "write_to_pty");
                assert_eq!(params["data"], "ls\n");
            }
            other => panic!("expected Request, got {other:?}"),
        }
    }

    #[test]
    fn parses_notification_without_id() {
        let body = serde_json::to_vec(
            &json!({ "jsonrpc": "2.0", "method": "subscribe-pane", "params": { "paneId": "p" } }),
        )
        .unwrap();
        match parse_envelope(&body) {
            Envelope::Notification { method, params } => {
                assert_eq!(method, "subscribe-pane");
                assert_eq!(params["paneId"], "p");
            }
            other => panic!("expected Notification, got {other:?}"),
        }
    }

    #[test]
    fn ignores_missing_jsonrpc_version() {
        let body = serde_json::to_vec(&json!({ "method": "x" })).unwrap();
        assert_eq!(parse_envelope(&body), Envelope::Ignore);
    }

    #[test]
    fn ignores_response_without_method() {
        let body =
            serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": 1, "result": null })).unwrap();
        assert_eq!(parse_envelope(&body), Envelope::Ignore);
    }

    #[test]
    fn ignores_garbage() {
        assert_eq!(parse_envelope(b"not json"), Envelope::Ignore);
        assert_eq!(parse_envelope(b"[1,2,3]"), Envelope::Ignore);
    }

    #[test]
    fn hello_negotiates_capability_intersection() {
        // controller 公告全 IDE 能力集；cli 只回它支持的子集（pane/fs/search）。
        let reply = negotiate_hello(&json!({
            "protocolVersion": 1,
            "capabilities": ["pane", "invoke", "fs", "git", "search", "workspace", "theme"]
        }));
        assert_eq!(reply["method"], HELLO_METHOD);
        assert_eq!(reply["params"]["protocolVersion"], 1);
        let caps: Vec<String> =
            serde_json::from_value(reply["params"]["capabilities"].clone()).unwrap();
        assert_eq!(caps, vec!["pane", "fs", "search"]);
        // 明确不包含 IDE-only 能力。
        assert!(!caps.contains(&"invoke".to_string()));
        assert!(!caps.contains(&"git".to_string()));
        assert!(!caps.contains(&"workspace".to_string()));
        assert!(!caps.contains(&"theme".to_string()));
    }

    #[test]
    fn hello_empty_peer_caps_returns_full_cli_set() {
        // peerCaps 为空 ⇒ 不约束（桌面同款分支）：回 cli 全能力集。
        let reply = negotiate_hello(&json!({ "protocolVersion": 1 }));
        let caps: Vec<String> =
            serde_json::from_value(reply["params"]["capabilities"].clone()).unwrap();
        assert_eq!(caps, vec!["pane", "fs", "search"]);
    }

    #[test]
    fn hello_version_mismatch_returns_bye() {
        let reply = negotiate_hello(&json!({ "protocolVersion": 0, "capabilities": [] }));
        assert_eq!(reply["method"], BYE_METHOD);
        assert_eq!(reply["params"]["reason"], "protocol-version-mismatch");
    }

    #[test]
    fn routes_write_to_pty() {
        let m = route_method("write_to_pty", &json!({ "paneId": "p", "data": "abc" }));
        assert_eq!(m, Method::WritePty { data: "abc".into() });
    }

    #[test]
    fn routes_resize_pane() {
        let m = route_method(
            "resize_pane",
            &json!({ "workspaceId": "w", "paneId": "p", "rows": 40, "cols": 120 }),
        );
        assert_eq!(
            m,
            Method::ResizePane {
                cols: 120,
                rows: 40
            }
        );
    }

    #[test]
    fn routes_search_with_camel_case_params() {
        let m = route_method(
            "search",
            &json!({ "root": "/r", "query": "foo", "useRegex": true, "caseSensitive": false }),
        );
        assert_eq!(
            m,
            Method::Search {
                root: "/r".into(),
                query: "foo".into(),
                use_regex: true,
                case_sensitive: false
            }
        );
    }

    #[test]
    fn routes_directory_children() {
        let m = route_method(
            "get_directory_children",
            &json!({ "path": "/tmp", "offset": 0, "limit": 100 }),
        );
        assert_eq!(
            m,
            Method::DirectoryChildren {
                path: "/tmp".into()
            }
        );
    }

    #[test]
    fn routes_workspace_presentation_methods() {
        // cli 单工作区/单 pane host：boot 的 refreshWorkspaces 调这两个，须命中专用变体
        // （而非 Unsupported），否则 SPA 启动中断、终端不渲染。
        assert_eq!(
            route_method("list_workspaces", &json!({})),
            Method::ListWorkspaces
        );
        assert_eq!(
            route_method("get_pane_layout", &json!({})),
            Method::GetPaneLayout
        );
    }

    #[test]
    fn unsupported_method_falls_through() {
        let m = route_method("git_status", &json!({}));
        assert_eq!(m, Method::Unsupported("git_status".into()));
    }

    #[test]
    fn result_and_error_response_shapes() {
        let id = json!(3);
        let ok = result_response(&id, json!({ "x": 1 }));
        assert_eq!(ok["jsonrpc"], "2.0");
        assert_eq!(ok["id"], 3);
        assert_eq!(ok["result"]["x"], 1);

        let err = error_response(&id, &RpcError::new(JSON_RPC_METHOD_NOT_FOUND, "no"));
        assert_eq!(err["error"]["code"], JSON_RPC_METHOD_NOT_FOUND);
        assert_eq!(err["error"]["message"], "no");
        // data 缺省时不应出现在线上。
        assert!(err["error"].get("data").is_none());
    }
}
