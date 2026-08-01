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

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

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

/// What the host can prove synchronously after writing a pane input buffer.
/// It deliberately stops at the transport boundary: an interactive program or
/// agent still needs to acknowledge consumption separately.
#[derive(Debug, Clone, Copy)]
pub struct InputDispatch {
    pub terminal_accepted: bool,
}

/// Host-advertised launch profile. Empty model/effort lists mean callers may
/// select the profile but may not override that dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfile {
    pub id: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub reasoning_efforts: Vec<String>,
    #[serde(default)]
    pub supports_checkpoint: bool,
}

/// Capability discovery is host-owned; the MCP core never invents model names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCapabilities {
    #[serde(default)]
    pub profiles: Vec<LaunchProfile>,
}

/// Typed split request passed to capable desktop/headless hosts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitPaneRequest {
    pub workspace_id: Option<String>,
    pub direction: String,
    pub role: String,
    pub initial_cmd: Option<String>,
    pub launch_profile: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub checkpoint: Option<String>,
    pub replace_target: Option<Value>,
}

/// A rejection observed at another execution layer (for example Codex's
/// execution gateway). Ridge only records and displays it: no report may imply
/// that Ridge caused the rejection or can retry the original operation.
#[derive(Debug, Clone)]
pub struct ExternalExecutionRejection {
    pub initiator: String,
    pub action: String,
    pub executor: String,
    pub policy_source: String,
    pub request_id: String,
    pub reason: String,
    pub next_step: String,
}

/// 宿主必须提供的最小动作面。所有方法同步：桌面与 rdg 的实现都只做加锁读写，
/// 不做网络/子进程等慢操作（慢操作会拖住 teammate HTTP 的单线程 runtime）。
pub trait McpHost: Send + Sync {
    /// 花名册快照（roster + leader + edges + groups）。
    fn team_profile(&self) -> Value;

    /// Enumerate workspaces. Legacy hosts compile unchanged and report the
    /// capability as unsupported instead of fabricating a one-workspace list.
    fn list_workspaces(&self) -> HostResult<Value> {
        Err(HostError::Unsupported("本宿主不支持跨工作区枚举".into()))
    }

    /// Read a workspace-scoped roster. Omitting workspace keeps the historical
    /// current-workspace behavior; an explicit workspace fails closed by default.
    fn team_profile_for(&self, workspace_id: Option<&str>) -> HostResult<Value> {
        if workspace_id.is_some() {
            return Err(HostError::InvalidParams(
                "本宿主未实现显式 workspace 寻址".into(),
            ));
        }
        Ok(self.team_profile())
    }

    /// Discover launch profiles/models/efforts. Empty lists prohibit overrides.
    fn launch_capabilities(&self) -> HostResult<LaunchCapabilities> {
        Ok(LaunchCapabilities::default())
    }

    /// Validate and normalize a pane target. Legacy hosts receive the original
    /// target only when workspace is omitted; explicit workspace never degrades
    /// silently to their current workspace.
    fn resolve_pane_target(&self, workspace_id: Option<&str>, target: &Value) -> HostResult<Value> {
        if workspace_id.is_some() {
            return Err(HostError::InvalidParams(
                "本宿主未实现显式 workspace 寻址".into(),
            ));
        }
        Ok(target.clone())
    }

    /// Writes text into a pane input buffer. `submit` explicitly dispatches
    /// Enter. `terminal_accepted` only reports a successful host→terminal
    /// transport write; it never implies agent consumption.
    fn send_text(
        &self,
        target: &Value,
        text: &str,
        submit: bool,
        mark_busy: bool,
    ) -> HostResult<InputDispatch>;

    /// Surface an externally rejected execution to the desktop user. The
    /// default keeps non-desktop hosts honest: they cannot claim a visible
    /// approval/retry flow they do not own.
    fn report_execution_rejection(
        &self,
        _report: ExternalExecutionRejection,
    ) -> HostResult<String> {
        Err(HostError::Unsupported(
            "本宿主无法展示外部执行拒绝；请在拥有该执行网关的界面查看原因和重试入口".into(),
        ))
    }

    /// 抓目标 pane 的**渲染后**屏幕文本（末 `lines` 行）。监控队友干活用。
    fn capture_pane(&self, target: &Value, lines: usize) -> HostResult<String>;

    /// 分屏并返回 `{ "paneId": ..., "paneIndex": ... }`。
    fn split_pane(
        &self,
        direction: &str,
        role: &str,
        initial_cmd: Option<&str>,
    ) -> HostResult<Value>;

    /// Typed, workspace-aware split. The default preserves the old host method
    /// only for its exact legacy surface and rejects every new selector.
    fn split_pane_with(&self, request: &SplitPaneRequest) -> HostResult<Value> {
        if request.workspace_id.is_some() {
            return Err(HostError::InvalidParams(
                "本宿主未实现显式 workspace 创建".into(),
            ));
        }
        if request.launch_profile.is_some()
            || request.model.is_some()
            || request.reasoning_effort.is_some()
        {
            return Err(HostError::InvalidParams(
                "本宿主未实现 launch profile".into(),
            ));
        }
        self.split_pane(
            &request.direction,
            &request.role,
            request.initial_cmd.as_deref(),
        )
    }

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

/// 给注入文本补一个 **CR**（0x0D）作 Enter，并吃掉调用方自带的尾部换行。
///
/// 为什么不是 LF：Claude Code / Cursor 这类 raw-mode TUI 把 LF 当「插入换行」，
/// 消息会**停在对端输入框永不提交**（实测：整段进了 composer，agent 根本没收到）；
/// 终端上 Enter 的线上字节本就是 CR。shell 在 canonical 模式下同样接受 CR，
/// 故两类目标统一用 CR。
pub fn enter_terminated(text: &str) -> String {
    format!("{}\r", text.trim_end_matches(['\r', '\n']))
}

// ─── MCP 会话状态：每个宿主实例隔离，兼容旧的进程级入口 ─────────────────────

pub struct McpSessionState {
    stash: Mutex<StashStore>,
    inbox: Mutex<HashMap<String, Vec<Value>>>,
    receipts: Mutex<ReceiptStore>,
}

impl Default for McpSessionState {
    fn default() -> Self {
        Self {
            stash: Mutex::new(StashStore::with_defaults()),
            inbox: Mutex::new(HashMap::new()),
            receipts: Mutex::new(ReceiptStore {
                by_id: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }
}

impl McpSessionState {
    /// Drop all delivery state for a pane after its generation is destroyed.
    /// Stash remains host-scoped and is independently bounded/evicted.
    pub fn purge_pane(&self, key: &str) {
        self.inbox.lock().unwrap().remove(key);
        let mut receipts = self.receipts.lock().unwrap();
        let expired = receipts
            .by_id
            .iter()
            .filter(|(_, value)| value.get("targetKey").and_then(Value::as_str) == Some(key))
            .map(|(id, _)| id.clone())
            .collect::<std::collections::HashSet<_>>();
        receipts.by_id.retain(|id, _| !expired.contains(id));
        receipts.order.retain(|id| !expired.contains(id));
    }
}

fn default_state() -> &'static McpSessionState {
    static STATE: std::sync::OnceLock<McpSessionState> = std::sync::OnceLock::new();
    STATE.get_or_init(McpSessionState::default)
}

/// 每个 pane 一个内存收件箱（FIFO，上限 200 条）。
///
/// 为什么要它：stdin 注入是「打断式」的——对方正在跑命令时消息会被 shell 吃掉，
/// 且非同源 agent 没有回信通道。收件箱让任意 MCP 客户端**异步**收发：发送侧照旧
/// 注入 stdin（人也看得见），同时留一份可被 `ridge_inbox_read` 取走的副本。
const INBOX_CAP: usize = 200;

struct ReceiptStore {
    by_id: HashMap<String, Value>,
    order: VecDeque<String>,
}

fn receipt_insert(state: &McpSessionState, id: String, value: Value) {
    let mut store = state.receipts.lock().unwrap();
    store.order.push_back(id.clone());
    store.by_id.insert(id, value);
    while store.order.len() > INBOX_CAP {
        if let Some(expired) = store.order.pop_front() {
            store.by_id.remove(&expired);
        }
    }
}

fn receipt_get(state: &McpSessionState, key: &str, id: &str) -> HostResult<Value> {
    let store = state.receipts.lock().unwrap();
    let value = store
        .by_id
        .get(id)
        .ok_or_else(|| HostError::InvalidParams("receipt 不存在或已过期".into()))?;
    if value.get("targetKey").and_then(Value::as_str) != Some(key) {
        return Err(HostError::InvalidParams(
            "receipt 不属于该 target pane".into(),
        ));
    }
    Ok(value.clone())
}

fn receipt_ack(
    state: &McpSessionState,
    key: &str,
    id: &str,
    status: &str,
    detail: Option<&str>,
) -> HostResult<Value> {
    if !matches!(status, "agent_acknowledged" | "agent_rejected") {
        return Err(HostError::InvalidParams(
            "status 只能是 agent_acknowledged 或 agent_rejected".into(),
        ));
    }
    let mut store = state.receipts.lock().unwrap();
    let value = store
        .by_id
        .get_mut(id)
        .ok_or_else(|| HostError::InvalidParams("receipt 不存在或已过期".into()))?;
    if value.get("targetKey").and_then(Value::as_str) != Some(key) {
        return Err(HostError::InvalidParams(
            "receipt 不属于该 target pane".into(),
        ));
    }
    value["status"] = Value::String(status.to_string());
    value["agentAcknowledged"] = Value::Bool(status == "agent_acknowledged");
    if let Some(detail) = detail.filter(|v| !v.is_empty()) {
        value["detail"] = Value::String(detail.to_string());
    }
    Ok(value.clone())
}

fn inbox_push(state: &McpSessionState, key: &str, entry: Value) {
    let mut map = state.inbox.lock().unwrap();
    let q = map.entry(key.to_string()).or_default();
    q.push(entry);
    if q.len() > INBOX_CAP {
        let drop_n = q.len() - INBOX_CAP;
        q.drain(0..drop_n);
    }
}

/// 取走（默认）或窥视收件箱。取走后消息不再重复投递，避免 agent 反复处理旧消息。
fn inbox_take(state: &McpSessionState, key: &str, peek: bool) -> Vec<Value> {
    let mut map = state.inbox.lock().unwrap();
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
    handle_message_with_state(text, host, version, default_state())
}

/// Same dispatcher with host-owned state. Kernel/desktop transports each pass
/// one instance so inbox, receipts and stash die with that host lifecycle.
pub fn handle_message_with_state(
    text: &str,
    host: &dyn McpHost,
    version: &str,
    state: &McpSessionState,
) -> Option<String> {
    let req = match proto::parse_request(text.as_bytes()) {
        Ok(r) => r,
        Err(_) => {
            return Some(
                proto::mcp_error(Value::Null, proto::PARSE_ERROR, "parse error").to_string(),
            )
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
        proto::METHOD_TOOLS_LIST => {
            proto::mcp_result(id, ToolRegistry::default().tools_list_result())
        }
        proto::METHOD_TOOLS_CALL => tools_call(id, &req.params, host, state),
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
        proto::METHOD_RESOURCES_READ => resources_read(id, &req.params, host, state),
        other => proto::mcp_error(
            id,
            proto::METHOD_NOT_FOUND,
            &format!("method not found: {other}"),
        ),
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

fn resources_read(
    id: Value,
    params: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> Value {
    let uri = params.get("uri").and_then(|v| v.as_str()).unwrap_or("");
    match RidgeUri::parse(uri) {
        Ok(RidgeUri::Cache(cache_id)) => {
            let text = state
                .stash
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
    proto::mcp_result(
        id,
        json!({ "content": [ { "type": "text", "text": text.into() } ] }),
    )
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn scoped_target(args: &Value, host: &dyn McpHost) -> HostResult<Value> {
    host.resolve_pane_target(
        arg_str(args, "workspace_id"),
        args.get("target_pane_id").unwrap_or(&Value::Null),
    )
}

fn split_request(args: &Value) -> HostResult<SplitPaneRequest> {
    let initial_cmd = arg_str(args, "initial_cmd").map(str::to_string);
    let launch_profile = arg_str(args, "launch_profile").map(str::to_string);
    let model = arg_str(args, "model").map(str::to_string);
    let reasoning_effort = arg_str(args, "reasoning_effort").map(str::to_string);
    let checkpoint = arg_str(args, "checkpoint").map(str::to_string);
    let replace_target = args.get("replace_target_pane_id").cloned();
    if initial_cmd.is_some() && launch_profile.is_some() {
        return Err(HostError::InvalidParams(
            "initial_cmd 与 launch_profile 互斥".into(),
        ));
    }
    if launch_profile.is_none()
        && (model.is_some()
            || reasoning_effort.is_some()
            || checkpoint.is_some()
            || replace_target.is_some())
    {
        return Err(HostError::InvalidParams(
            "model/reasoning_effort/checkpoint/replace_target_pane_id 必须随 launch_profile 使用"
                .into(),
        ));
    }
    if replace_target.is_some() && checkpoint.is_none() {
        return Err(HostError::InvalidParams(
            "replace_target_pane_id 必须随 checkpoint 使用".into(),
        ));
    }
    Ok(SplitPaneRequest {
        workspace_id: arg_str(args, "workspace_id").map(str::to_string),
        direction: arg_str(args, "direction").unwrap_or("vertical").to_string(),
        role: arg_str(args, "role").unwrap_or("worker").to_string(),
        initial_cmd,
        launch_profile,
        model,
        reasoning_effort,
        checkpoint,
        replace_target,
    })
}

fn validate_launch_request(host: &dyn McpHost, request: &SplitPaneRequest) -> HostResult<()> {
    let Some(profile_id) = request.launch_profile.as_deref() else {
        return Ok(());
    };
    let capabilities = host.launch_capabilities()?;
    let profile = capabilities
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| HostError::InvalidParams(format!("未知 launch_profile: {profile_id}")))?;
    if let Some(model) = request.model.as_deref() {
        if !profile.models.iter().any(|allowed| allowed == model) {
            return Err(HostError::InvalidParams(format!(
                "model 不在 launch_profile {profile_id} 的许可集合"
            )));
        }
    }
    if let Some(effort) = request.reasoning_effort.as_deref() {
        if !profile
            .reasoning_efforts
            .iter()
            .any(|allowed| allowed == effort)
        {
            return Err(HostError::InvalidParams(format!(
                "reasoning_effort 不在 launch_profile {profile_id} 的许可集合"
            )));
        }
    }
    if request.checkpoint.is_some() && !profile.supports_checkpoint {
        return Err(HostError::InvalidParams(format!(
            "launch_profile {profile_id} 不支持 checkpoint"
        )));
    }
    Ok(())
}

fn split_result(mut value: Value, request: &SplitPaneRequest) -> Value {
    let object = match value.as_object_mut() {
        Some(object) => object,
        None => return json!({ "status": "pane_created", "hostResult": value }),
    };
    object
        .entry("status")
        .or_insert_with(|| Value::String("pane_created".into()));
    object.entry("workspaceId").or_insert_with(|| {
        request
            .workspace_id
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null)
    });
    object.entry("launchProfile").or_insert_with(|| {
        request
            .launch_profile
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null)
    });
    object.entry("model").or_insert_with(|| {
        request
            .model
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null)
    });
    object.entry("reasoningEffort").or_insert_with(|| {
        request
            .reasoning_effort
            .clone()
            .map(Value::String)
            .unwrap_or(Value::Null)
    });
    object
        .entry("checkpointTransferred")
        .or_insert_with(|| Value::Bool(request.checkpoint.is_some()));
    object
        .entry("replacementRequested")
        .or_insert_with(|| Value::Bool(request.replace_target.is_some()));
    object.entry("commandSummary").or_insert_with(|| {
        Value::String(
            if request.initial_cmd.is_some() {
                "custom initial command (redacted)"
            } else if let Some(profile) = request.launch_profile.as_deref() {
                return Value::String(format!("launch profile {profile}"));
            } else {
                "default shell"
            }
            .into(),
        )
    });
    value
}

fn tools_call(id: Value, params: &Value, host: &dyn McpHost, state: &McpSessionState) -> Value {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let target = || scoped_target(&args, host);

    let out: HostResult<String> = match name {
        "ridge_get_team_profile" => host
            .team_profile_for(arg_str(&args, "workspace_id"))
            .map(|profile| profile.to_string()),
        "ridge_list_workspaces" => host.list_workspaces().map(|items| items.to_string()),
        "ridge_get_launch_capabilities" => host.launch_capabilities().and_then(|capabilities| {
            serde_json::to_string(&capabilities)
                .map_err(|error| HostError::Internal(error.to_string()))
        }),

        "ridge_send_to_teammate" | "ridge_send_and_submit" | "ridge_delegate_task" => {
            let text = arg_str(&args, "message")
                .or_else(|| arg_str(&args, "objective"))
                .unwrap_or("");
            if text.is_empty() {
                Err(HostError::InvalidParams(
                    "message/objective 不能为空".into(),
                ))
            } else {
                let delegate = name == "ridge_delegate_task";
                let submit = if name == "ridge_send_to_teammate" {
                    args.get("submit").and_then(Value::as_bool).unwrap_or(true)
                } else {
                    true
                };
                target().and_then(|t| host.pane_key(&t).map(|key| (t, key))).and_then(|(t, key)| {
                    let workspace_id = t
                        .get("workspaceId")
                        .or_else(|| t.get("workspace_id"))
                        .cloned()
                        .or_else(|| arg_str(&args, "workspace_id").map(|id| json!(id)))
                        .unwrap_or(Value::Null);
                    host.send_text(&t, text, submit, delegate).map(|dispatch| {
                        let status = if submit { "submit_dispatched" } else { "draft_injected" };
                        let receipt_id = Uuid::new_v4().to_string();
                        let receipt = json!({
                            "receiptId": receipt_id,
                            "targetKey": key,
                            "from": arg_str(&args, "from").unwrap_or("mcp-client"),
                            "kind": if delegate { "task" } else { "message" },
                            "status": status,
                            "terminalAccepted": dispatch.terminal_accepted,
                            "agentAcknowledged": false,
                            "workspaceId": workspace_id.clone(),
                            "text": text,
                        });
                        receipt_insert(state, receipt_id.clone(), receipt.clone());
                        inbox_push(
                            state,
                            &key,
                            receipt,
                        );
                        json!({
                            "receiptId": receipt_id,
                            "status": status,
                            "terminalAccepted": dispatch.terminal_accepted,
                            "agentAcknowledged": false,
                            "workspaceId": workspace_id,
                            "next": if submit { "call ridge_delivery_status; target agent may call ridge_acknowledge_receipt" } else { "call ridge_send_and_submit to dispatch Enter" },
                        }).to_string()
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
            target().and_then(|target| host.capture_pane(&target, lines))
        }

        "ridge_split_pane" => split_request(&args).and_then(|request| {
            validate_launch_request(host, &request)?;
            host.split_pane_with(&request)
                .map(|value| split_result(value, &request).to_string())
        }),

        // agent_id  alone must not force pane resolve — historically scoped_target(null)
        // always failed on desktop (-32602) even when agent_id was valid.
        "ridge_join_group" => match arg_str(&args, "group_name") {
            None => Err(HostError::InvalidParams("group_name 不能为空".into())),
            Some(g) => {
                let agent = arg_str(&args, "agent_id");
                let has_pane = args
                    .get("target_pane_id")
                    .is_some_and(|v| !v.is_null() && v != &Value::String(String::new()));
                match (agent, has_pane) {
                    (None, false) => Err(HostError::InvalidParams(
                        "需提供 agent_id 或 target_pane_id".into(),
                    )),
                    (agent, true) => target().and_then(|t| {
                        host.join_group(g, agent, Some(&t))
                            .map(|()| "dispatched".to_string())
                    }),
                    (Some(agent), false) => {
                        // workspace-only optional target for multi-ws; no pane resolve.
                        let synthetic = arg_str(&args, "workspace_id").map(|ws| {
                            json!({ "workspaceId": ws })
                        });
                        host.join_group(g, Some(agent), synthetic.as_ref())
                            .map(|()| "dispatched".to_string())
                    }
                }
            }
        },

        "ridge_report_progress" => {
            let status = arg_str(&args, "status").unwrap_or("update");
            let detail = arg_str(&args, "detail").unwrap_or("");
            target().and_then(|t| {
                host.report_progress(&t, status, detail)
                    .map(|()| "reported".to_string())
            })
        }

        // 收发同一份内存队列：任何 MCP 客户端都能异步取走发给某 pane 的消息，
        // 不必依赖 stdin 注入被对方 shell 正确读到。
        "ridge_inbox_read" => {
            let peek = args.get("peek").and_then(|v| v.as_bool()).unwrap_or(false);
            target().and_then(|target| {
                host.pane_key(&target)
                        .map(|key| Value::Array(inbox_take(state, &key, peek)).to_string())
            })
        }

        "ridge_delivery_status" => {
            let receipt_id = arg_str(&args, "receipt_id")
                .ok_or_else(|| HostError::InvalidParams("receipt_id 不能为空".into()));
            receipt_id.and_then(|receipt_id| {
                target().and_then(|target| {
                    host.pane_key(&target)
                        .and_then(|key| receipt_get(state, &key, receipt_id))
                        .map(|receipt| receipt.to_string())
                })
            })
        }

        "ridge_acknowledge_receipt" => {
            let receipt_id = arg_str(&args, "receipt_id")
                .ok_or_else(|| HostError::InvalidParams("receipt_id 不能为空".into()));
            let status = arg_str(&args, "status")
                .ok_or_else(|| HostError::InvalidParams("status 不能为空".into()));
            match (receipt_id, status) {
                (Ok(receipt_id), Ok(status)) => target().and_then(|target| {
                    host.pane_key(&target)
                        .and_then(|key| {
                            receipt_ack(state, &key, receipt_id, status, arg_str(&args, "detail"))
                        })
                        .map(|receipt| receipt.to_string())
                }),
                (Err(e), _) | (_, Err(e)) => Err(e),
            }
        }

        "ridge_report_execution_rejection" => {
            let required = |key| {
                arg_str(&args, key)
                    .ok_or_else(|| HostError::InvalidParams(format!("{key} 不能为空")))
            };
            match (
                required("executor"),
                required("policy_source"),
                required("request_id"),
                required("reason"),
                required("next_step"),
            ) {
                (Ok(executor), Ok(policy_source), Ok(request_id), Ok(reason), Ok(next_step)) => {
                    host.report_execution_rejection(ExternalExecutionRejection {
                        initiator: arg_str(&args, "initiator")
                            .unwrap_or("mcp-client")
                            .to_string(),
                        action: arg_str(&args, "action").unwrap_or("").to_string(),
                        executor: executor.to_string(),
                        policy_source: policy_source.to_string(),
                        request_id: request_id.to_string(),
                        reason: reason.to_string(),
                        next_step: next_step.to_string(),
                    })
                    .map(|id| {
                        json!({
                            "reportId": id,
                            "status": "reported",
                            "retry": "not_available_from_ridge",
                        })
                        .to_string()
                    })
                }
                (Err(error), _, _, _, _)
                | (_, Err(error), _, _, _)
                | (_, _, Err(error), _, _)
                | (_, _, _, Err(error), _)
                | (_, _, _, _, Err(error)) => Err(error),
            }
        }

        "ridge_stash_data" => {
            // 规格历史写的是 content_base64、实现读的是 data —— 两个键都接，纯文本存。
            match arg_str(&args, "data").or_else(|| arg_str(&args, "content_base64")) {
                None => Err(HostError::InvalidParams("data 不能为空".into())),
                Some(d) => Ok(state.stash.lock().unwrap().stash_uri(d.as_bytes().to_vec())),
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
        fn send_text(
            &self,
            target: &Value,
            _text: &str,
            _submit: bool,
            _busy: bool,
        ) -> HostResult<InputDispatch> {
            if target.is_null() {
                return Err(HostError::InvalidParams("no target".into()));
            }
            Ok(InputDispatch {
                terminal_accepted: true,
            })
        }
        fn report_execution_rejection(
            &self,
            report: ExternalExecutionRejection,
        ) -> HostResult<String> {
            Ok(format!("report:{}:{}", report.executor, report.request_id))
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

    struct CapableHost;

    impl McpHost for CapableHost {
        fn team_profile(&self) -> Value {
            json!({ "workspaceId": "current", "roster": [] })
        }
        fn list_workspaces(&self) -> HostResult<Value> {
            Ok(json!([{ "workspaceId": "ws-a" }, { "workspaceId": "ws-b" }]))
        }
        fn team_profile_for(&self, workspace_id: Option<&str>) -> HostResult<Value> {
            let workspace_id = workspace_id.unwrap_or("current");
            if !matches!(workspace_id, "current" | "ws-a" | "ws-b") {
                return Err(HostError::InvalidParams("workspace 不存在".into()));
            }
            Ok(json!({ "workspaceId": workspace_id, "roster": [] }))
        }
        fn launch_capabilities(&self) -> HostResult<LaunchCapabilities> {
            Ok(LaunchCapabilities {
                profiles: vec![LaunchProfile {
                    id: "codex".into(),
                    models: vec!["gpt-5".into()],
                    reasoning_efforts: vec!["medium".into(), "high".into()],
                    supports_checkpoint: true,
                }],
            })
        }
        fn resolve_pane_target(
            &self,
            workspace_id: Option<&str>,
            target: &Value,
        ) -> HostResult<Value> {
            let workspace_id = workspace_id.unwrap_or("current");
            if !matches!(workspace_id, "current" | "ws-a" | "ws-b") {
                return Err(HostError::InvalidParams("workspace 不存在".into()));
            }
            Ok(json!({ "workspaceId": workspace_id, "paneId": target }))
        }
        fn send_text(
            &self,
            target: &Value,
            _text: &str,
            _submit: bool,
            _busy: bool,
        ) -> HostResult<InputDispatch> {
            if target.get("workspaceId").is_none() || target.get("paneId").is_none() {
                return Err(HostError::InvalidParams("target 未复合寻址".into()));
            }
            Ok(InputDispatch {
                terminal_accepted: true,
            })
        }
        fn capture_pane(&self, target: &Value, _lines: usize) -> HostResult<String> {
            Ok(target.to_string())
        }
        fn split_pane(&self, _d: &str, _r: &str, _c: Option<&str>) -> HostResult<Value> {
            unreachable!("typed host uses split_pane_with")
        }
        fn split_pane_with(&self, request: &SplitPaneRequest) -> HostResult<Value> {
            if !matches!(
                request.workspace_id.as_deref(),
                None | Some("ws-a") | Some("ws-b")
            ) {
                return Err(HostError::InvalidParams("workspace 不存在".into()));
            }
            Ok(json!({
                "paneId": "pane-new",
                "workspaceId": request.workspace_id,
                "launchProfile": request.launch_profile,
                "model": request.model,
                "reasoningEffort": request.reasoning_effort,
                "checkpointTransferred": request.checkpoint.is_some(),
                "replacementRequested": request.replace_target.is_some(),
            }))
        }
        fn join_group(
            &self,
            group_name: &str,
            agent_id: Option<&str>,
            target: Option<&Value>,
        ) -> HostResult<()> {
            if group_name.trim().is_empty() {
                return Err(HostError::InvalidParams("group_name 不能为空".into()));
            }
            if agent_id.is_none() && target.is_none() {
                return Err(HostError::InvalidParams(
                    "需提供 agent_id 或 target_pane_id".into(),
                ));
            }
            if agent_id == Some("missing-agent") {
                return Err(HostError::InvalidParams(
                    "agent_id missing-agent 不在花名册".into(),
                ));
            }
            Ok(())
        }
        fn read_resource(&self, _uri: &RidgeUri) -> HostResult<(String, String)> {
            Ok(("application/json".into(), "{}".into()))
        }
        fn pane_key(&self, target: &Value) -> HostResult<String> {
            Ok(target.to_string())
        }
    }

    fn call_with(msg: &str, host: &dyn McpHost) -> Value {
        let out = handle_message(msg, host, "test").expect("expected a response");
        serde_json::from_str(&out).unwrap()
    }

    fn call_with_state(msg: &str, host: &dyn McpHost, state: &McpSessionState) -> Value {
        let out = handle_message_with_state(msg, host, "test", state).expect("expected response");
        serde_json::from_str(&out).unwrap()
    }

    fn call(msg: &str) -> Value {
        call_with(msg, &FakeHost)
    }

    #[test]
    fn enter_is_cr_not_lf() {
        // 回归守卫：LF 会让消息卡在对端 TUI 输入框（实测），Enter 必须是 CR。
        assert_eq!(enter_terminated("hi"), "hi\r");
        assert_eq!(enter_terminated("hi\n"), "hi\r");
        assert_eq!(enter_terminated("hi\r\n"), "hi\r");
        assert!(!enter_terminated("hi").contains('\n'));
    }

    #[test]
    fn host_state_isolated_and_purge_removes_delivery_records() {
        let first = McpSessionState::default();
        let second = McpSessionState::default();
        let send = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_to_teammate","arguments":{"target_pane_id":7,"message":"isolated"}}
        })
        .to_string();
        let sent = call_with_state(&send, &FakeHost, &first);
        let receipt: Value = serde_json::from_str(
            sent["result"]["content"][0]["text"].as_str().unwrap(),
        )
        .unwrap();
        let read = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_inbox_read","arguments":{"target_pane_id":7}}
        })
        .to_string();
        assert_ne!(call_with_state(&read, &FakeHost, &first)["result"]["content"][0]["text"], "[]");
        assert_eq!(call_with_state(&read, &FakeHost, &second)["result"]["content"][0]["text"], "[]");
        first.purge_pane("7");
        let status = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_delivery_status","arguments":{"target_pane_id":7,"receipt_id":receipt["receiptId"]}}
        })
        .to_string();
        assert!(call_with_state(&status, &FakeHost, &first)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("不存在"));
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
        assert!(
            text.contains("\"from\":\"cursor\""),
            "收件箱应留副本: {text}"
        );
        // 取走即清空
        let v2 = call(&read);
        assert_eq!(v2["result"]["content"][0]["text"].as_str().unwrap(), "[]");
    }

    #[test]
    fn send_defaults_to_submit_and_preserves_explicit_draft() {
        let default_send = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_to_teammate","arguments":{"target_pane_id":8,"message":"hi"}}
        })
        .to_string();
        let draft = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_send_to_teammate","arguments":{"target_pane_id":8,"message":"hi","submit":false}}
        })
        .to_string();
        let forced_submit = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_send_and_submit","arguments":{"target_pane_id":8,"message":"hi"}}
        })
        .to_string();
        let default_text = call(&default_send)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let draft_text = call(&draft)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        let submit_text = call(&forced_submit)["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(default_text.contains("submit_dispatched"));
        assert!(draft_text.contains("draft_injected"));
        assert!(submit_text.contains("submit_dispatched"));
        assert!(submit_text.contains("\"terminalAccepted\":true"));
        assert!(submit_text.contains("\"agentAcknowledged\":false"));
    }

    #[test]
    fn legacy_host_rejects_explicit_workspace_instead_of_misrouting() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_and_submit","arguments":{
                "workspace_id":"ws-b","target_pane_id":8,"message":"hi"
            }}
        })
        .to_string();
        let response = call(&msg);
        assert_eq!(response["error"]["code"], proto::INVALID_PARAMS);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("显式 workspace"));
    }

    #[test]
    fn explicit_workspace_routes_as_composite_identity() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_and_submit","arguments":{
                "workspace_id":"ws-b","target_pane_id":8,"message":"hi"
            }}
        })
        .to_string();
        let response = call_with(&msg, &CapableHost);
        let receipt: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(receipt["workspaceId"], "ws-b");
        assert_eq!(receipt["status"], "submit_dispatched");
        assert_eq!(receipt["agentAcknowledged"], false);
    }

    #[test]
    fn launch_capabilities_are_host_owned_and_typed_split_is_validated() {
        let discover = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_get_launch_capabilities","arguments":{}}
        })
        .to_string();
        let capabilities: Value = serde_json::from_str(
            call_with(&discover, &CapableHost)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(capabilities["profiles"][0]["id"], "codex");
        assert_eq!(capabilities["profiles"][0]["models"], json!(["gpt-5"]));

        let split = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_split_pane","arguments":{
                "workspace_id":"ws-b","direction":"vertical","role":"worker",
                "launch_profile":"codex","model":"gpt-5","reasoning_effort":"high",
                "checkpoint":"session-1","replace_target_pane_id":"pane-old"
            }}
        })
        .to_string();
        let created: Value = serde_json::from_str(
            call_with(&split, &CapableHost)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(created["workspaceId"], "ws-b");
        assert_eq!(created["launchProfile"], "codex");
        assert_eq!(created["model"], "gpt-5");
        assert_eq!(created["reasoningEffort"], "high");
        assert_eq!(created["checkpointTransferred"], true);
        assert_eq!(created["replacementRequested"], true);
        assert_eq!(created["status"], "pane_created");
    }

    #[test]
    fn join_group_agent_id_only_does_not_force_pane_resolve() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_join_group","arguments":{
                "group_name":"alpha","agent_id":"auto:grok:deadbeef"
            }}
        })
        .to_string();
        let response = call_with(&msg, &CapableHost);
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(
            response["result"]["content"][0]["text"].as_str().unwrap(),
            "dispatched"
        );
    }

    #[test]
    fn join_group_missing_member_selector_is_invalid_params() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_join_group","arguments":{"group_name":"alpha"}}
        })
        .to_string();
        let response = call_with(&msg, &CapableHost);
        assert_eq!(response["error"]["code"], proto::INVALID_PARAMS);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("agent_id"));
    }

    #[test]
    fn join_group_unknown_agent_is_invalid_params_not_silent_ok() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_join_group","arguments":{
                "group_name":"alpha","agent_id":"missing-agent"
            }}
        })
        .to_string();
        let response = call_with(&msg, &CapableHost);
        assert_eq!(response["error"]["code"], proto::INVALID_PARAMS);
        assert!(response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("花名册"));
    }

    #[test]
    fn join_group_unsupported_host_returns_is_error_not_silent_ok() {
        let msg = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_join_group","arguments":{
                "group_name":"alpha","agent_id":"worker-1"
            }}
        })
        .to_string();
        let response = call(&msg); // FakeHost default Unsupported
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("不支持 ridge_join_group"));
    }

    #[test]
    fn split_rejects_mutual_exclusion_and_unadvertised_overrides() {
        for arguments in [
            json!({
                "direction":"vertical","role":"worker",
                "initial_cmd":"pwsh","launch_profile":"codex"
            }),
            json!({
                "direction":"vertical","role":"worker",
                "launch_profile":"unknown"
            }),
            json!({
                "direction":"vertical","role":"worker",
                "launch_profile":"codex","model":"invented"
            }),
            json!({
                "direction":"vertical","role":"worker",
                "launch_profile":"codex","reasoning_effort":"ultra"
            }),
            json!({
                "direction":"vertical","role":"worker","model":"gpt-5"
            }),
            json!({
                "direction":"vertical","role":"worker",
                "launch_profile":"codex","replace_target_pane_id":"pane-old"
            }),
            json!({
                "workspace_id":"forged","direction":"vertical","role":"worker",
                "launch_profile":"codex"
            }),
        ] {
            let msg = json!({
                "jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"ridge_split_pane","arguments":arguments}
            })
            .to_string();
            assert_eq!(
                call_with(&msg, &CapableHost)["error"]["code"],
                proto::INVALID_PARAMS
            );
        }
    }

    #[test]
    fn receipt_tracks_explicit_agent_acknowledgement() {
        let send = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_and_submit","arguments":{"target_pane_id":91,"message":"inspect"}}
        })
        .to_string();
        let sent: Value = serde_json::from_str(
            call(&send)["result"]["content"][0]["text"]
                .as_str()
                .expect("send receipt"),
        )
        .expect("receipt JSON");
        let receipt_id = sent["receiptId"].as_str().expect("receipt id").to_string();

        let status = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_delivery_status","arguments":{"target_pane_id":91,"receipt_id":receipt_id}}
        })
        .to_string();
        let before: Value = serde_json::from_str(
            call(&status)["result"]["content"][0]["text"]
                .as_str()
                .expect("status JSON"),
        )
        .expect("status receipt");
        assert_eq!(before["status"], "submit_dispatched");
        assert_eq!(before["terminalAccepted"], true);
        assert_eq!(before["agentAcknowledged"], false);

        let ack = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_acknowledge_receipt","arguments":{
                "target_pane_id":91,"receipt_id":receipt_id,"status":"agent_acknowledged","detail":"received"
            }}
        })
        .to_string();
        let after: Value = serde_json::from_str(
            call(&ack)["result"]["content"][0]["text"]
                .as_str()
                .expect("ack JSON"),
        )
        .expect("ack receipt");
        assert_eq!(after["status"], "agent_acknowledged");
        assert_eq!(after["agentAcknowledged"], true);
        assert_eq!(after["detail"], "received");
    }

    #[test]
    fn external_rejection_keeps_executor_attribution_and_never_claims_retry() {
        let message = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_report_execution_rejection","arguments":{
                "executor":"Codex execution gateway",
                "policy_source":"organization execution policy",
                "request_id":"request-42",
                "reason":"rejected: blocked by policy",
                "next_step":"use the gateway approval flow, then retry there"
            }}
        })
        .to_string();
        let response = call(&message);
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("report response");
        assert!(text.contains("report:Codex execution gateway:request-42"));
        assert!(text.contains("not_available_from_ridge"));
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
