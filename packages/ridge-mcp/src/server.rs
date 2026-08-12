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
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::a2a::{A2aClientConfig, A2aEndpointRegistry};
use crate::delivery::{
    choose_delivery_adapter, DeliveryOutcome, DeliveryProbe, DeliveryRegistry, HubDeliveryAdapter,
    HubPtyRuntimeSnapshot,
};
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
    pub(crate) fn message(&self) -> String {
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

    /// Probe the target's real delivery surface. Hosts must override this for
    /// Runtime API/A2A/PTY; MCP pull is the only safe default because the Hub
    /// itself owns that queue.
    fn probe_delivery(&self, _target: &Value) -> HostResult<DeliveryProbe> {
        Ok(DeliveryProbe {
            mcp_pull: true,
            ..DeliveryProbe::default()
        })
    }

    /// Return one host-atomically sampled PTY runtime snapshot, if this host
    /// owns a verified sampler. `None` is fail-closed: the Hub never invents
    /// prompt/foreground/input state from a roster entry or pane title.
    fn pty_runtime_snapshot(&self, _target: &Value) -> HostResult<Option<HubPtyRuntimeSnapshot>> {
        Ok(None)
    }

    /// Authorize a cross-process delivery stream against the host's current
    /// roster before a route is registered. Legacy roster entries without a
    /// complete fenced identity fail closed instead of becoming impersonable.
    fn authorize_delivery_endpoint(
        &self,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> HostResult<()> {
        let profile = self.team_profile_for(None)?;
        let identity = ["agent_identities", "roster"]
            .into_iter()
            .filter_map(|key| profile.get(key).and_then(Value::as_array))
            .flatten()
            .find(|entry| {
                value_string(entry, &["agentId", "agent_id", "id"]).as_deref() == Some(agent_id)
            })
            .ok_or_else(|| {
                HostError::InvalidParams("delivery Agent identity is not registered".into())
            })?;
        let current_generation = identity.get("generation").and_then(Value::as_u64);
        let current_lease = value_string(identity, &["lease"]);
        let lifecycle = value_string(identity, &["lifecycle"]).unwrap_or_default();
        let online = identity
            .get("online")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if current_generation != Some(generation)
            || current_lease.as_deref() != Some(lease)
            || !online
            || !matches!(
                lifecycle.as_str(),
                "Online" | "Working" | "Waiting" | "Attention"
            )
        {
            return Err(HostError::InvalidParams(
                "delivery Agent identity is stale or offline".into(),
            ));
        }
        Ok(())
    }

    fn deliver_runtime_api(&self, _target: &Value, _entry: &Value) -> HostResult<DeliveryOutcome> {
        Err(HostError::Unsupported(
            "本宿主未提供经过探测的 Runtime API adapter".into(),
        ))
    }

    fn deliver_a2a(&self, _target: &Value, _entry: &Value) -> HostResult<DeliveryOutcome> {
        Err(HostError::Unsupported(
            "本宿主未提供经过探测的 A2A adapter".into(),
        ))
    }

    /// The default PTY adapter is deliberately behind the host's probe. A
    /// host that cannot prove all five conditions must leave the entry in Hub.
    fn deliver_pty_fallback(&self, target: &Value, entry: &Value) -> HostResult<DeliveryOutcome> {
        let payload = entry
            .get("payload")
            .and_then(|value| value.get("text").or_else(|| value.get("objective")))
            .and_then(Value::as_str)
            .ok_or_else(|| HostError::InvalidParams("PTY fallback payload must be text".into()))?;
        let submit = entry
            .get("payload")
            .and_then(|value| value.get("submitRequested"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let dispatch = self.send_text(target, payload, submit, true)?;
        Ok(DeliveryOutcome {
            adapter: HubDeliveryAdapter::PtyFallback,
            accepted: dispatch.terminal_accepted,
            remote_id: None,
            acknowledged: false,
        })
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
    idempotency: Mutex<IdempotencyStore>,
    /// Serialize the check → durable reservation → in-memory insert sequence.
    /// Separate mutexes protect each store, but cannot make that compound
    /// operation atomic; without this gate concurrent identical sends could
    /// each allocate a message before either one records its idempotency key.
    enqueue_lock: Mutex<()>,
    sequences: Mutex<HashMap<String, u64>>,
    delivery_registry: DeliveryRegistry,
    a2a_endpoints: A2aEndpointRegistry,
    persistence: Option<HubPersistence>,
}

impl Default for McpSessionState {
    fn default() -> Self {
        Self::empty()
    }
}

impl McpSessionState {
    fn empty() -> Self {
        Self {
            stash: Mutex::new(StashStore::with_defaults()),
            inbox: Mutex::new(HashMap::new()),
            receipts: Mutex::new(ReceiptStore {
                by_id: HashMap::new(),
                order: VecDeque::new(),
            }),
            idempotency: Mutex::new(IdempotencyStore {
                by_key: HashMap::new(),
                order: VecDeque::new(),
            }),
            enqueue_lock: Mutex::new(()),
            sequences: Mutex::new(HashMap::new()),
            delivery_registry: DeliveryRegistry::default(),
            a2a_endpoints: A2aEndpointRegistry::default(),
            persistence: None,
        }
    }

    /// Open the durable Hub used by production Kernel/Desktop hosts.
    ///
    /// Tests and explicitly ephemeral embedders keep using `Default`; a
    /// production caller must not silently fall back to volatile delivery
    /// state when SQLite cannot be opened.
    pub fn with_sqlite(path: impl AsRef<Path>) -> Result<Self, String> {
        let persistence = HubPersistence::open(path.as_ref())?;
        let mut state = Self {
            persistence: Some(persistence),
            ..Self::empty()
        };
        let loaded = state
            .persistence
            .as_ref()
            .expect("SQLite persistence installed")
            .load()?;
        state.apply_loaded(loaded);
        Ok(state)
    }

    fn apply_loaded(&mut self, loaded: PersistedHubState) {
        for (target_key, entry) in loaded.messages {
            let queue = self.inbox.get_mut().expect("inbox lock not poisoned");
            let entries = queue.entry(target_key).or_default();
            entries.push(entry);
            if entries.len() > INBOX_CAP {
                let drop_n = entries.len() - INBOX_CAP;
                entries.drain(0..drop_n);
            }
        }
        for (id, entry) in loaded.receipts {
            receipt_insert_memory(&self.receipts, id, entry);
        }
        for (key, entry) in loaded.idempotency {
            dedupe_insert_memory(&self.idempotency, key, entry);
        }
        *self
            .sequences
            .get_mut()
            .expect("sequence lock not poisoned") = loaded.sequences;
    }

    /// Drop all delivery state for a pane after its generation is destroyed.
    /// Stash remains host-scoped and is independently bounded/evicted.
    pub fn purge_pane(&self, key: &str) -> Result<(), String> {
        if let Some(persistence) = &self.persistence {
            persistence.purge(key)?;
        }
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
        let mut idempotency = self.idempotency.lock().unwrap();
        let expired_keys = idempotency
            .by_key
            .iter()
            .filter(|(_, value)| value.get("targetKey").and_then(Value::as_str) == Some(key))
            .map(|(id, _)| id.clone())
            .collect::<std::collections::HashSet<_>>();
        idempotency
            .by_key
            .retain(|id, _| !expired_keys.contains(id));
        idempotency.order.retain(|id| !expired_keys.contains(id));
        self.sequences.lock().unwrap().remove(key);
        Ok(())
    }

    /// Register a bounded in-process Runtime API/A2A receiver. The caller is
    /// responsible for tying the receiver lifetime to the current Agent
    /// session; stale generation/lease routes cannot be selected.
    pub fn register_delivery_endpoint(
        &self,
        adapter: HubDeliveryAdapter,
        agent_id: impl Into<String>,
        generation: u64,
        lease: impl Into<String>,
    ) -> Result<std::sync::mpsc::Receiver<Value>, String> {
        self.delivery_registry
            .register(adapter, agent_id, generation, lease)
    }

    pub fn unregister_delivery_endpoint(
        &self,
        adapter: HubDeliveryAdapter,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> Result<bool, String> {
        self.delivery_registry
            .unregister(adapter, agent_id, generation, lease)
    }

    /// Discover and register one standard A2A JSON-RPC endpoint for the
    /// current Agent generation. Credentials stay in the process-local route.
    pub fn register_a2a_endpoint(
        &self,
        agent_id: impl Into<String>,
        generation: u64,
        lease: impl Into<String>,
        config: A2aClientConfig,
    ) -> Result<crate::a2a::AgentCard, String> {
        self.a2a_endpoints
            .register(agent_id, generation, lease, config)
    }

    pub fn unregister_a2a_endpoint(
        &self,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> Result<bool, String> {
        self.a2a_endpoints.unregister(agent_id, generation, lease)
    }

    /// Publish one atomically sampled, generation/lease-fenced PTY runtime
    /// snapshot before the Hub may fall back to PTY delivery.
    pub fn register_pty_runtime_snapshot(
        &self,
        agent_id: impl Into<String>,
        generation: u64,
        lease: impl Into<String>,
        snapshot: HubPtyRuntimeSnapshot,
    ) -> Result<(), String> {
        self.delivery_registry
            .register_pty_runtime_snapshot(agent_id, generation, lease, snapshot)
    }

    /// Remove a PTY runtime snapshot only from its owning Agent generation.
    pub fn unregister_pty_runtime_snapshot(
        &self,
        agent_id: &str,
        generation: u64,
        lease: &str,
    ) -> Result<bool, String> {
        self.delivery_registry
            .unregister_pty_runtime_snapshot(agent_id, generation, lease)
    }

    pub fn delivery_probe(&self, target: &Value) -> DeliveryProbe {
        let mut probe = self.delivery_registry.probe(target);
        probe.a2a |= self.a2a_endpoints.probe(target);
        probe
    }

    pub fn deliver_registered_endpoint(
        &self,
        adapter: HubDeliveryAdapter,
        target: &Value,
        entry: &Value,
    ) -> Result<DeliveryOutcome, String> {
        self.delivery_registry.deliver(adapter, target, entry)
    }

    pub fn deliver_a2a_endpoint(
        &self,
        target: &Value,
        entry: &Value,
    ) -> Result<DeliveryOutcome, String> {
        if self.a2a_endpoints.probe(target) {
            let result = self.a2a_endpoints.deliver(target, entry)?;
            return Ok(DeliveryOutcome {
                adapter: HubDeliveryAdapter::A2a,
                accepted: true,
                remote_id: result.remote_id,
                acknowledged: true,
            });
        }
        self.delivery_registry
            .deliver(HubDeliveryAdapter::A2a, target, entry)
    }

    /// Acknowledge a delivery received through the cross-process stream.
    /// The stream derives the target key from the durable receipt and fences
    /// the acknowledgement against Agent identity, generation, and lease.
    pub fn acknowledge_delivery(
        &self,
        agent_id: &str,
        generation: u64,
        lease: &str,
        delivery_id: &str,
        status: &str,
        detail: Option<&str>,
    ) -> Result<Value, String> {
        let target_key = {
            let receipts = self
                .receipts
                .lock()
                .map_err(|_| "Hub receipt lock poisoned".to_string())?;
            let entry = receipts
                .by_id
                .get(delivery_id)
                .ok_or_else(|| "delivery receipt does not exist or expired".to_string())?;
            let target = entry
                .get("to")
                .ok_or_else(|| "delivery receipt has no target identity".to_string())?;
            let matches = target.get("agentId").and_then(Value::as_str) == Some(agent_id)
                && target.get("generation").and_then(Value::as_u64) == Some(generation)
                && target.get("lease").and_then(Value::as_str) == Some(lease);
            if !matches {
                return Err("delivery acknowledgement identity fence mismatch".into());
            }
            entry
                .get("targetKey")
                .and_then(Value::as_str)
                .ok_or_else(|| "delivery receipt has no target key".to_string())?
                .to_string()
        };
        receipt_ack(self, &target_key, delivery_id, status, detail).map_err(|error| error.message())
    }
}

fn default_state() -> &'static McpSessionState {
    static STATE: std::sync::OnceLock<McpSessionState> = std::sync::OnceLock::new();
    STATE.get_or_init(McpSessionState::default)
}

/// 每个 pane 一个有界收件箱（FIFO，上限 200 条；生产 Hub 由 SQLite 持久化）。
///
/// 为什么要它：stdin 注入是「打断式」的——对方正在跑命令时消息会被 shell 吃掉，
/// 且非同源 agent 没有回信通道。收件箱让任意 MCP 客户端**异步**收发：发送侧照旧
/// 注入 stdin（人也看得见），同时留一份可被 `ridge_inbox_read` 取走的副本。
const INBOX_CAP: usize = 200;

struct ReceiptStore {
    by_id: HashMap<String, Value>,
    order: VecDeque<String>,
}

struct IdempotencyStore {
    by_key: HashMap<String, Value>,
    order: VecDeque<String>,
}

struct HubPersistence {
    connection: Mutex<Connection>,
}

struct PersistedHubState {
    messages: Vec<(String, Value)>,
    receipts: Vec<(String, Value)>,
    idempotency: Vec<(String, Value)>,
    sequences: HashMap<String, u64>,
}

impl HubPersistence {
    fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create Hub directory {}: {error}", parent.display()))?;
        }
        let connection = Connection::open(path)
            .map_err(|error| format!("open Hub database {}: {error}", path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("configure Hub database timeout: {error}"))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 CREATE TABLE IF NOT EXISTS hub_messages (
                     delivery_id TEXT PRIMARY KEY,
                     target_key TEXT NOT NULL,
                     entry_json TEXT NOT NULL,
                     consumed INTEGER NOT NULL DEFAULT 0,
                     created_at INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS hub_messages_target_order
                     ON hub_messages(target_key, created_at);
                 CREATE TABLE IF NOT EXISTS hub_receipts (
                     delivery_id TEXT PRIMARY KEY,
                     target_key TEXT NOT NULL,
                     entry_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS hub_idempotency (
                     idempotency_key TEXT PRIMARY KEY,
                     target_key TEXT NOT NULL,
                     entry_json TEXT NOT NULL,
                     created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS hub_sequences (
                     target_key TEXT PRIMARY KEY,
                     sequence INTEGER NOT NULL
                 );",
            )
            .map_err(|error| format!("initialize Hub schema: {error}"))?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn load(&self) -> Result<PersistedHubState, String> {
        let connection = self.connection.lock().unwrap();
        let mut messages = Vec::new();
        let mut statement = connection
            .prepare(
                "SELECT target_key, entry_json
                 FROM hub_messages
                 WHERE consumed = 0
                 ORDER BY rowid ASC",
            )
            .map_err(|error| format!("prepare Hub messages: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("read Hub messages: {error}"))?;
        for row in rows {
            let (target_key, raw) =
                row.map_err(|error| format!("decode Hub message row: {error}"))?;
            let entry = serde_json::from_str(&raw)
                .map_err(|error| format!("parse Hub message JSON: {error}"))?;
            messages.push((target_key, entry));
        }

        let receipts = load_json_rows(&connection, "hub_receipts", "delivery_id")?;
        let idempotency = load_json_rows(&connection, "hub_idempotency", "idempotency_key")?;
        let mut sequences = HashMap::new();
        let mut statement = connection
            .prepare("SELECT target_key, sequence FROM hub_sequences")
            .map_err(|error| format!("prepare Hub sequences: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .map_err(|error| format!("read Hub sequences: {error}"))?;
        for row in rows {
            let (key, sequence) =
                row.map_err(|error| format!("decode Hub sequence row: {error}"))?;
            sequences.insert(key, sequence);
        }
        Ok(PersistedHubState {
            messages,
            receipts,
            idempotency,
            sequences,
        })
    }

    fn lookup_idempotency(&self, key: &str) -> Result<Option<Value>, String> {
        let connection = self.connection.lock().unwrap();
        let raw = connection
            .query_row(
                "SELECT entry_json FROM hub_idempotency WHERE idempotency_key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("read Hub idempotency: {error}"))?;
        raw.map(|raw| {
            serde_json::from_str(&raw).map_err(|error| format!("parse Hub idempotency: {error}"))
        })
        .transpose()
    }

    fn next_sequence(&self, target_key: &str) -> Result<u64, String> {
        let connection = self.connection.lock().unwrap();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("begin Hub sequence: {error}"))?;
        let current = transaction
            .query_row(
                "SELECT sequence FROM hub_sequences WHERE target_key = ?1",
                params![target_key],
                |row| row.get::<_, u64>(0),
            )
            .optional()
            .map_err(|error| format!("read Hub sequence: {error}"))?
            .unwrap_or_default();
        let next = current.saturating_add(1);
        transaction
            .execute(
                "INSERT INTO hub_sequences(target_key, sequence) VALUES (?1, ?2)
                 ON CONFLICT(target_key) DO UPDATE SET sequence = excluded.sequence",
                params![target_key, next],
            )
            .map_err(|error| format!("reserve Hub sequence: {error}"))?;
        transaction
            .commit()
            .map(|()| next)
            .map_err(|error| format!("commit Hub sequence: {error}"))
    }

    fn persist_enqueue(
        &self,
        target_key: &str,
        entry: &Value,
        idempotency_key: &str,
    ) -> Result<Option<Value>, String> {
        let delivery_id = entry
            .get("deliveryId")
            .and_then(Value::as_str)
            .ok_or_else(|| "Hub entry missing deliveryId".to_string())?;
        let raw =
            serde_json::to_string(entry).map_err(|error| format!("encode Hub entry: {error}"))?;
        let now = unix_millis();
        let connection = self.connection.lock().unwrap();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("begin Hub transaction: {error}"))?;
        let existing = transaction
            .query_row(
                "SELECT entry_json FROM hub_idempotency WHERE idempotency_key = ?1",
                params![idempotency_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("read Hub idempotency: {error}"))?;
        if let Some(raw) = existing {
            let entry = serde_json::from_str(&raw)
                .map_err(|error| format!("parse existing Hub idempotency: {error}"))?;
            return Ok(Some(entry));
        }
        transaction
            .execute(
                "INSERT OR REPLACE INTO hub_messages
                 (delivery_id, target_key, entry_json, consumed, created_at)
                 VALUES (?1, ?2, ?3, 0, ?4)",
                params![delivery_id, target_key, raw, now],
            )
            .map_err(|error| format!("persist Hub message: {error}"))?;
        transaction
            .execute(
                "DELETE FROM hub_messages
                 WHERE target_key = ?1
                   AND rowid NOT IN (
                       SELECT rowid FROM hub_messages
                       WHERE target_key = ?1 ORDER BY rowid DESC LIMIT ?2
                   )",
                params![target_key, INBOX_CAP as i64],
            )
            .map_err(|error| format!("trim Hub inbox: {error}"))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO hub_receipts
                 (delivery_id, target_key, entry_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![delivery_id, target_key, raw, now],
            )
            .map_err(|error| format!("persist Hub receipt: {error}"))?;
        transaction
            .execute(
                "DELETE FROM hub_receipts
                 WHERE rowid NOT IN (
                     SELECT rowid FROM hub_receipts ORDER BY rowid DESC LIMIT ?1
                 )",
                params![INBOX_CAP as i64],
            )
            .map_err(|error| format!("trim Hub receipts: {error}"))?;
        transaction
            .execute(
                "INSERT OR REPLACE INTO hub_idempotency
                 (idempotency_key, target_key, entry_json, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![idempotency_key, target_key, raw, now],
            )
            .map_err(|error| format!("persist Hub idempotency: {error}"))?;
        transaction
            .execute(
                "DELETE FROM hub_idempotency
                 WHERE rowid NOT IN (
                     SELECT rowid FROM hub_idempotency ORDER BY rowid DESC LIMIT ?1
                 )",
                params![INBOX_CAP as i64],
            )
            .map_err(|error| format!("trim Hub idempotency: {error}"))?;
        transaction
            .commit()
            .map(|()| None)
            .map_err(|error| format!("commit Hub enqueue: {error}"))
    }

    fn update_entry(&self, key: &str, delivery_id: &str, entry: &Value) -> Result<(), String> {
        let raw =
            serde_json::to_string(entry).map_err(|error| format!("encode Hub update: {error}"))?;
        let connection = self.connection.lock().unwrap();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("begin Hub update: {error}"))?;
        let changed = transaction
            .execute(
                "UPDATE hub_receipts SET entry_json = ?1
                 WHERE delivery_id = ?2 AND target_key = ?3",
                params![raw, delivery_id, key],
            )
            .map_err(|error| format!("update Hub receipt: {error}"))?;
        if changed == 0 {
            return Err("Hub receipt disappeared during update".into());
        }
        transaction
            .execute(
                "UPDATE hub_messages SET entry_json = ?1
                 WHERE delivery_id = ?2 AND target_key = ?3",
                params![raw, delivery_id, key],
            )
            .map_err(|error| format!("update Hub message: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub update: {error}"))
    }

    fn update_idempotency(&self, key: &str, entry: &Value) -> Result<(), String> {
        let raw = serde_json::to_string(entry)
            .map_err(|error| format!("encode Hub dedupe update: {error}"))?;
        let connection = self.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE hub_idempotency SET entry_json = ?1 WHERE idempotency_key = ?2",
                params![raw, key],
            )
            .map(|_| ())
            .map_err(|error| format!("update Hub idempotency: {error}"))
    }

    fn consume(&self, key: &str, delivery_ids: &[String]) -> Result<(), String> {
        if delivery_ids.is_empty() {
            return Ok(());
        }
        let connection = self.connection.lock().unwrap();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("begin Hub consume: {error}"))?;
        for delivery_id in delivery_ids {
            transaction
                .execute(
                    "UPDATE hub_messages SET consumed = 1
                     WHERE target_key = ?1 AND delivery_id = ?2",
                    params![key, delivery_id],
                )
                .map_err(|error| format!("consume Hub message: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("commit Hub consume: {error}"))
    }

    fn purge(&self, key: &str) -> Result<(), String> {
        let connection = self.connection.lock().unwrap();
        let transaction = connection
            .unchecked_transaction()
            .map_err(|error| format!("begin Hub purge: {error}"))?;
        for table in [
            "hub_messages",
            "hub_receipts",
            "hub_idempotency",
            "hub_sequences",
        ] {
            let sql = format!("DELETE FROM {table} WHERE target_key = ?1");
            transaction
                .execute(&sql, params![key])
                .map_err(|error| format!("purge Hub {table}: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("commit Hub purge: {error}"))
    }
}

fn load_json_rows(
    connection: &Connection,
    table: &str,
    key_column: &str,
) -> Result<Vec<(String, Value)>, String> {
    let sql = format!("SELECT {key_column}, entry_json FROM {table} ORDER BY rowid ASC");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("prepare Hub {table}: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| format!("read Hub {table}: {error}"))?;
    let mut values = Vec::new();
    for row in rows {
        let (key, raw) = row.map_err(|error| format!("decode Hub {table} row: {error}"))?;
        let value = serde_json::from_str(&raw)
            .map_err(|error| format!("parse Hub {table} JSON: {error}"))?;
        values.push((key, value));
    }
    Ok(values)
}

fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn deadline_expired(deadline_unix_ms: Option<u64>) -> bool {
    deadline_unix_ms.is_some_and(|deadline| deadline <= unix_millis().max(0) as u64)
}

fn receipt_insert_memory(store: &Mutex<ReceiptStore>, id: String, value: Value) {
    let mut store = store.lock().unwrap();
    store.order.push_back(id.clone());
    store.by_id.insert(id, value);
    while store.order.len() > INBOX_CAP {
        if let Some(expired) = store.order.pop_front() {
            store.by_id.remove(&expired);
        }
    }
}

fn dedupe_insert_memory(store: &Mutex<IdempotencyStore>, key: String, entry: Value) {
    let mut store = store.lock().unwrap();
    store.order.push_back(key.clone());
    store.by_key.insert(key, entry);
    while store.order.len() > INBOX_CAP {
        if let Some(expired) = store.order.pop_front() {
            store.by_key.remove(&expired);
        }
    }
}

fn receipt_insert(state: &McpSessionState, id: String, value: Value) {
    receipt_insert_memory(&state.receipts, id, value);
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
    if !matches!(
        status,
        "agent_received"
            | "agent_accepted"
            | "agent_acknowledged"
            | "agent_completed"
            | "agent_rejected"
    ) {
        return Err(HostError::InvalidParams(
            "status must be a supported Agent acknowledgement state".into(),
        ));
    }
    let value = {
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
        if matches!(
            value.get("status").and_then(Value::as_str),
            Some("cancelled" | "expired")
        ) {
            return Err(hub_error(
                "delivery_terminal",
                "cancelled or expired delivery cannot be acknowledged",
            ));
        }
        value["status"] = Value::String(status.to_string());
        value["agentAcknowledged"] = Value::Bool(status != "agent_received");
        value["ack"] = json!({
            "state": if status == "agent_rejected" { "nacked" } else { "acked" },
            "status": status,
            "detail": detail.filter(|item| !item.is_empty()),
        });
        if let Some(detail) = detail.filter(|v| !v.is_empty()) {
            value["detail"] = Value::String(detail.to_string());
        }
        value.clone()
    };
    sync_inbox_entry(
        state,
        key,
        value
            .get("deliveryId")
            .and_then(Value::as_str)
            .unwrap_or(id),
        &value,
    )
    .map_err(HostError::Internal)?;
    Ok(value)
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
fn inbox_take(state: &McpSessionState, key: &str, peek: bool) -> Result<Vec<Value>, String> {
    expire_queued_entries(state, key)?;
    let selected = {
        let mut map = state.inbox.lock().unwrap();
        match map.get_mut(key) {
            None => Vec::new(),
            Some(q) if peek => q.clone(),
            Some(q) => q.clone(),
        }
    };
    if !peek {
        let delivery_ids = selected_delivery_ids(&selected);
        if let Some(persistence) = &state.persistence {
            persistence.consume(key, &delivery_ids)?;
        }
        let mut map = state.inbox.lock().unwrap();
        if let Some(queue) = map.get_mut(key) {
            remove_selected_entries(queue, &selected, &delivery_ids);
        }
    }
    Ok(selected)
}

fn sync_inbox_entry(
    state: &McpSessionState,
    key: &str,
    delivery_id: &str,
    entry: &Value,
) -> Result<(), String> {
    if let Some(persistence) = &state.persistence {
        persistence.update_entry(key, delivery_id, entry)?;
    }
    let mut map = state.inbox.lock().unwrap();
    if let Some(queue) = map.get_mut(key) {
        if let Some(current) = queue
            .iter_mut()
            .find(|item| item.get("deliveryId").and_then(Value::as_str) == Some(delivery_id))
        {
            *current = entry.clone();
        }
    }
    drop(map);
    sync_idempotency_entry(state, entry)?;
    Ok(())
}

fn sync_idempotency_entry(state: &McpSessionState, entry: &Value) -> Result<(), String> {
    let Some(from) = entry.get("from").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(key) = entry.get("idempotencyKey").and_then(Value::as_str) else {
        return Ok(());
    };
    let key = idempotency_key(from, key);
    if let Some(persistence) = &state.persistence {
        persistence.update_idempotency(&key, entry)?;
    }
    if let Some(value) = state.idempotency.lock().unwrap().by_key.get_mut(&key) {
        *value = entry.clone();
    }
    Ok(())
}

fn inbox_fetch(
    state: &McpSessionState,
    key: &str,
    peek: bool,
    consume: bool,
    limit: usize,
    cursor: Option<&str>,
) -> Result<Vec<Value>, String> {
    expire_queued_entries(state, key)?;
    let selected = {
        let mut map = state.inbox.lock().unwrap();
        let Some(queue) = map.get_mut(key) else {
            return Ok(Vec::new());
        };
        let start = match cursor {
            None => 0,
            Some(value) => queue
                .iter()
                .position(|entry| entry.get("deliveryId").and_then(Value::as_str) == Some(value))
                .map(|index| index.saturating_add(1))
                .or_else(|| {
                    value.parse::<u64>().ok().map(|sequence| {
                        queue
                            .iter()
                            .position(|entry| {
                                entry.get("sequence").and_then(Value::as_u64) > Some(sequence)
                            })
                            .unwrap_or(queue.len())
                    })
                })
                .ok_or_else(|| "Hub cursor is not a delivery id or sequence".to_string())?
                .min(queue.len()),
        };
        let end = start.saturating_add(limit).min(queue.len());
        queue[start..end].to_vec()
    };
    if consume && !peek {
        let delivery_ids = selected_delivery_ids(&selected);
        if let Some(persistence) = &state.persistence {
            persistence.consume(key, &delivery_ids)?;
        }
        let mut map = state.inbox.lock().unwrap();
        if let Some(queue) = map.get_mut(key) {
            remove_selected_entries(queue, &selected, &delivery_ids);
        }
    }
    Ok(selected)
}

fn expire_queued_entries(state: &McpSessionState, key: &str) -> Result<(), String> {
    let expired = {
        let map = state.inbox.lock().unwrap();
        map.get(key)
            .into_iter()
            .flatten()
            .filter(|entry| {
                entry.get("status").and_then(Value::as_str) == Some("queued")
                    && deadline_expired(entry.get("deadlineUnixMs").and_then(Value::as_u64))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if expired.is_empty() {
        return Ok(());
    }
    let mut delivery_ids = Vec::with_capacity(expired.len());
    for entry in expired {
        let Some(delivery_id) = entry
            .get("deliveryId")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let mut updated = entry;
        updated["status"] = Value::String("expired".into());
        updated["expiredAtUnixMs"] = json!(unix_millis());
        if updated.get("kind").and_then(Value::as_str) == Some("task") {
            updated["taskStatus"] = Value::String("expired".into());
        }
        replace_receipt_entry(state, key, &delivery_id, &updated)?;
        delivery_ids.push(delivery_id);
    }
    if let Some(persistence) = &state.persistence {
        persistence.consume(key, &delivery_ids)?;
    }
    let mut map = state.inbox.lock().unwrap();
    if let Some(queue) = map.get_mut(key) {
        remove_selected_entries(queue, &[], &delivery_ids);
    }
    Ok(())
}

fn cancel_hub_entry(
    state: &McpSessionState,
    key: &str,
    delivery_id: Option<&str>,
    cancellation_id: Option<&str>,
    reason: Option<&str>,
) -> HostResult<Value> {
    let (receipt_id, mut entry) = {
        let receipts = state.receipts.lock().unwrap();
        receipts
            .by_id
            .iter()
            .find(|(_, value)| {
                value.get("targetKey").and_then(Value::as_str) == Some(key)
                    && delivery_id
                        .map(|id| value.get("deliveryId").and_then(Value::as_str) == Some(id))
                        .unwrap_or(true)
                    && cancellation_id
                        .map(|id| value.get("cancellationId").and_then(Value::as_str) == Some(id))
                        .unwrap_or(true)
            })
            .map(|(id, value)| (id.clone(), value.clone()))
            .ok_or_else(|| HostError::InvalidParams("delivery 不存在或已过期".into()))?
    };
    let status = entry
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("queued");
    if status == "cancelled" {
        return Ok(entry);
    }
    if matches!(
        status,
        "completed" | "expired" | "failed" | "delivery_rejected" | "adapter_accepted"
    ) {
        return Err(hub_error(
            "delivery_terminal",
            "delivery is already terminal and cannot be cancelled",
        ));
    }
    let detail = reason.filter(|value| !value.trim().is_empty());
    entry["status"] = Value::String("cancelled".into());
    entry["cancellationRequested"] = Value::Bool(true);
    entry["agentAcknowledged"] = Value::Bool(false);
    entry["ack"] = json!({
        "state": "nacked",
        "status": "cancelled",
        "detail": detail,
    });
    if entry.get("kind").and_then(Value::as_str) == Some("task") {
        entry["taskStatus"] = Value::String("cancelled".into());
    }
    replace_receipt_entry(state, key, &receipt_id, &entry).map_err(HostError::Internal)?;
    if let Some(persistence) = &state.persistence {
        persistence
            .consume(key, std::slice::from_ref(&receipt_id))
            .map_err(HostError::Internal)?;
    }
    let mut inbox = state.inbox.lock().unwrap();
    if let Some(queue) = inbox.get_mut(key) {
        remove_selected_entries(queue, &[], &[receipt_id]);
    }
    Ok(entry)
}

fn selected_delivery_ids(entries: &[Value]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| {
            entry
                .get("deliveryId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn remove_selected_entries(queue: &mut Vec<Value>, selected: &[Value], delivery_ids: &[String]) {
    let mut legacy_remaining = selected
        .iter()
        .filter(|entry| entry.get("deliveryId").and_then(Value::as_str).is_none())
        .count();
    queue.retain(|entry| {
        if let Some(id) = entry.get("deliveryId").and_then(Value::as_str) {
            return !delivery_ids.iter().any(|candidate| candidate == id);
        }
        if legacy_remaining > 0 {
            legacy_remaining -= 1;
            return false;
        }
        true
    });
}

fn next_sequence(state: &McpSessionState, target_key: &str) -> HostResult<u64> {
    if let Some(persistence) = &state.persistence {
        let sequence = persistence
            .next_sequence(target_key)
            .map_err(HostError::Internal)?;
        state
            .sequences
            .lock()
            .unwrap()
            .insert(target_key.to_string(), sequence);
        return Ok(sequence);
    }
    let mut sequences = state.sequences.lock().unwrap();
    let sequence = sequences.entry(target_key.to_string()).or_insert(0);
    *sequence = sequence.saturating_add(1);
    Ok(*sequence)
}

fn idempotency_key(from: &str, key: &str) -> String {
    format!("{from}:{key}")
}

fn dedupe_lookup(state: &McpSessionState, key: &str) -> Option<Value> {
    if let Some(value) = state.idempotency.lock().unwrap().by_key.get(key).cloned() {
        return Some(value);
    }
    let persistence = state.persistence.as_ref()?;
    let value = persistence.lookup_idempotency(key).ok().flatten();
    if let Some(entry) = value.clone() {
        dedupe_insert_memory(&state.idempotency, key.to_string(), entry);
    }
    value
}

fn dedupe_insert(state: &McpSessionState, key: String, entry: Value) {
    dedupe_insert_memory(&state.idempotency, key, entry);
}

struct HubEntryMeta<'a> {
    idempotency_key: &'a str,
    correlation_id: Option<&'a str>,
    causation_id: Option<&'a str>,
    conversation_id: Option<&'a str>,
    priority: &'a str,
    deadline_unix_ms: Option<u64>,
    cancellation_id: Option<&'a str>,
    topic: Option<&'a str>,
}

fn same_hub_entry(entry: &Value, target: &HubTarget, kind: &str, payload: &Value) -> bool {
    entry.get("targetKey").and_then(Value::as_str) == Some(target.target_key.as_str())
        && entry.get("kind").and_then(Value::as_str) == Some(kind)
        && entry.get("payload") == Some(payload)
}

fn deliver_hub_entry(
    host: &dyn McpHost,
    state: &McpSessionState,
    target: &HubTarget,
    target_value: &Value,
    adapter: HubDeliveryAdapter,
    delivery_id: &str,
    entry: &Value,
) -> HostResult<Value> {
    if adapter == HubDeliveryAdapter::McpPull {
        return Ok(entry.clone());
    }
    let mut attempted = entry.clone();
    attempted["deliveryAttempts"] = json!(1);
    attempted["deliveryLastAttemptAtUnixMs"] = json!(unix_millis());
    replace_receipt_entry(state, &target.target_key, delivery_id, &attempted)
        .map_err(HostError::Internal)?;
    let outcome = match adapter {
        HubDeliveryAdapter::RuntimeApi => host.deliver_runtime_api(target_value, &attempted),
        HubDeliveryAdapter::A2a if state.a2a_endpoints.probe(target_value) => state
            .deliver_a2a_endpoint(target_value, &attempted)
            .map_err(HostError::Internal),
        HubDeliveryAdapter::A2a => host.deliver_a2a(target_value, &attempted),
        HubDeliveryAdapter::PtyFallback => host.deliver_pty_fallback(target_value, &attempted),
        HubDeliveryAdapter::McpPull => unreachable!("MCP pull returned above"),
    };
    match outcome {
        Ok(outcome) => {
            if outcome.adapter != adapter {
                return Err(HostError::Internal(
                    "delivery adapter returned a mismatched outcome".into(),
                ));
            }
            let mut updated = attempted;
            updated["adapterAccepted"] = Value::Bool(outcome.accepted);
            updated["agentAcknowledged"] = Value::Bool(outcome.acknowledged);
            updated["status"] = Value::String(
                if outcome.accepted {
                    "adapter_accepted"
                } else {
                    "delivery_rejected"
                }
                .into(),
            );
            if let Some(remote_id) = outcome.remote_id {
                updated["adapterRemoteId"] = Value::String(remote_id);
            }
            replace_receipt_entry(state, &target.target_key, delivery_id, &updated)
                .map_err(HostError::Internal)?;
            Ok(updated)
        }
        Err(error) => {
            let mut failed = attempted;
            failed["status"] = Value::String("delivery_failed".into());
            failed["deliveryError"] = Value::String(error.message());
            replace_receipt_entry(state, &target.target_key, delivery_id, &failed)
                .map_err(HostError::Internal)?;
            Err(error)
        }
    }
}

fn enqueue_hub_entry(
    host: &dyn McpHost,
    state: &McpSessionState,
    target: &HubTarget,
    from: &str,
    kind: &str,
    payload: Value,
    meta: HubEntryMeta<'_>,
) -> HostResult<Value> {
    // Idempotency is a compound operation. Hold one short process-local gate
    // across lookup, SQLite reservation, and memory publication so concurrent
    // calls cannot allocate two logical messages for one key.
    let _enqueue_guard = state.enqueue_lock.lock().unwrap();
    expire_queued_entries(state, &target.target_key).map_err(HostError::Internal)?;
    if deadline_expired(meta.deadline_unix_ms) {
        return Err(hub_error(
            "deadline_exceeded",
            "delivery deadline has already elapsed",
        ));
    }
    let dedupe_key = idempotency_key(from, meta.idempotency_key);
    if let Some(existing) = dedupe_lookup(state, &dedupe_key) {
        if !same_hub_entry(&existing, target, kind, &payload) {
            return Err(hub_error(
                "idempotency_conflict",
                "key already belongs to another target, message kind, or payload",
            ));
        }
        let mut replay = existing;
        replay["deduplicated"] = Value::Bool(true);
        return Ok(replay);
    }
    let target_value = target.as_value();
    if let Some(snapshot) = host.pty_runtime_snapshot(&target_value)? {
        state
            .register_pty_runtime_snapshot(
                target.agent_id.clone(),
                target.generation,
                target.lease.clone(),
                snapshot,
            )
            .map_err(HostError::Internal)?;
    }
    let mut probe = host.probe_delivery(&target_value)?;
    probe.a2a |= state.a2a_endpoints.probe(&target_value);
    let adapter = choose_delivery_adapter(probe).ok_or_else(|| {
        hub_error(
            "delivery_unavailable",
            "target has no proven Runtime API, A2A, MCP pull, or safe PTY adapter",
        )
    })?;
    let message_id = Uuid::new_v4().to_string();
    let delivery_id = Uuid::new_v4().to_string();
    let task_id = (kind == "task").then(|| message_id.clone());
    let sequence = next_sequence(state, &target.target_key)?;
    let entry = json!({
        "messageId": message_id,
        "deliveryId": delivery_id,
        "taskId": task_id,
        "targetKey": target.target_key,
        "from": from,
        "to": {
            "agentId": target.agent_id,
            "sessionId": target.session_id,
            "workspaceId": target.workspace_id,
            "paneId": target.pane_id,
            "generation": target.generation,
            "lease": target.lease,
        },
        "kind": kind,
        "sequence": sequence,
        "idempotencyKey": meta.idempotency_key,
        "correlationId": meta.correlation_id,
        "causationId": meta.causation_id,
        "conversationId": meta.conversation_id,
        "priority": meta.priority,
        "deadlineUnixMs": meta.deadline_unix_ms,
        "cancellationId": meta.cancellation_id,
        "topic": meta.topic,
        "status": "queued",
        "taskStatus": (kind == "task").then_some("created"),
        "deliveryAdapter": adapter.as_str(),
        "deliveryReliability": adapter.reliability(),
        "deliveryAttempts": 0,
        "deliveryLastAttemptAtUnixMs": Value::Null,
        "terminalAccepted": false,
        "agentAcknowledged": false,
        "ack": { "state": "pending" },
        "workspaceId": target.workspace_id,
        "payload": payload,
    });
    let persistent_existing = state
        .persistence
        .as_ref()
        .map(|persistence| persistence.persist_enqueue(&target.target_key, &entry, &dedupe_key))
        .transpose()
        .map_err(HostError::Internal)?
        .flatten();
    if let Some(existing) = persistent_existing {
        if !same_hub_entry(&existing, target, kind, &payload) {
            return Err(hub_error(
                "idempotency_conflict",
                "key already belongs to another target, message kind, or payload",
            ));
        }
        dedupe_insert(state, dedupe_key, existing.clone());
        let mut replay = existing;
        replay["deduplicated"] = Value::Bool(true);
        return Ok(replay);
    }
    receipt_insert(state, delivery_id.clone(), entry.clone());
    inbox_push(state, &target.target_key, entry.clone());
    dedupe_insert(state, dedupe_key, entry.clone());
    deliver_hub_entry(
        host,
        state,
        target,
        &target_value,
        adapter,
        &delivery_id,
        &entry,
    )
}

fn replace_receipt_entry(
    state: &McpSessionState,
    key: &str,
    delivery_id: &str,
    entry: &Value,
) -> Result<(), String> {
    if let Some(persistence) = &state.persistence {
        persistence.update_entry(key, delivery_id, entry)?;
    }
    if let Some(value) = state.receipts.lock().unwrap().by_id.get_mut(delivery_id) {
        *value = entry.clone();
    }
    let mut inbox = state.inbox.lock().unwrap();
    if let Some(queue) = inbox.get_mut(key) {
        if let Some(value) = queue
            .iter_mut()
            .find(|value| value.get("deliveryId").and_then(Value::as_str) == Some(delivery_id))
        {
            *value = entry.clone();
        }
    }
    drop(inbox);
    sync_idempotency_entry(state, entry)?;
    Ok(())
}

fn valid_task_transition(from: &str, to: &str) -> bool {
    if from == to {
        return true;
    }
    match from {
        "created" => matches!(
            to,
            "assigned" | "accepted" | "running" | "cancelled" | "expired" | "failed"
        ),
        "assigned" => matches!(
            to,
            "accepted" | "running" | "cancelled" | "expired" | "failed"
        ),
        "accepted" => matches!(
            to,
            "running" | "waiting" | "blocked" | "cancelled" | "failed"
        ),
        "running" => matches!(
            to,
            "waiting" | "blocked" | "completed" | "cancelled" | "failed"
        ),
        "waiting" | "blocked" => matches!(to, "running" | "completed" | "cancelled" | "failed"),
        "completed" | "cancelled" | "failed" | "expired" => false,
        _ => false,
    }
}

fn task_update(
    state: &McpSessionState,
    target_key: &str,
    task_id: &str,
    status: &str,
    detail: Option<&str>,
) -> HostResult<Value> {
    if status.trim().is_empty() {
        return Err(HostError::InvalidParams("status must not be empty".into()));
    }
    let status = status.trim();
    let value = {
        let mut receipts = state.receipts.lock().unwrap();
        let receipt_id = receipts
            .by_id
            .iter()
            .find(|(_, value)| value.get("taskId").and_then(Value::as_str) == Some(task_id))
            .map(|(receipt_id, _)| receipt_id.clone())
            .ok_or_else(|| HostError::InvalidParams("task_id does not exist or expired".into()))?;
        let value = receipts
            .by_id
            .get_mut(&receipt_id)
            .expect("receipt found in the same locked store");
        if value.get("targetKey").and_then(Value::as_str) != Some(target_key)
            || value.get("kind").and_then(Value::as_str) != Some("task")
        {
            return Err(HostError::InvalidParams(
                "task_id does not belong to target task inbox".into(),
            ));
        }
        let current = value
            .get("taskStatus")
            .and_then(Value::as_str)
            .unwrap_or("created");
        if !valid_task_transition(current, status) {
            return Err(hub_error(
                "invalid_task_transition",
                format!("{current} -> {status}"),
            ));
        }
        value["taskStatus"] = Value::String(status.to_string());
        if matches!(status, "completed" | "failed" | "cancelled" | "expired") {
            value["status"] = Value::String(status.to_string());
        }
        if let Some(detail) = detail.filter(|item| !item.is_empty()) {
            value["detail"] = Value::String(detail.to_string());
        }
        value.clone()
    };
    sync_inbox_entry(
        state,
        target_key,
        value
            .get("deliveryId")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        &value,
    )
    .map_err(HostError::Internal)?;
    Ok(value)
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

fn resources_read(id: Value, params: &Value, host: &dyn McpHost, state: &McpSessionState) -> Value {
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

fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key)
        .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
}

fn value_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(*key) {
        Some(Value::String(item)) if !item.trim().is_empty() => Some(item.trim().to_string()),
        Some(Value::Number(item)) => Some(item.to_string()),
        _ => None,
    })
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(item) if !item.trim().is_empty() => Some(item.trim().to_string()),
        Value::Number(item) => Some(item.to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct HubTarget {
    target_key: String,
    agent_id: String,
    session_id: String,
    workspace_id: String,
    pane_id: String,
    generation: u64,
    lease: String,
}

impl HubTarget {
    fn as_value(&self) -> Value {
        json!({
            "agentId": self.agent_id,
            "sessionId": self.session_id,
            "workspaceId": self.workspace_id,
            "paneId": self.pane_id,
            "generation": self.generation,
            "lease": self.lease,
        })
    }
}

fn hub_error(code: &str, detail: impl Into<String>) -> HostError {
    HostError::InvalidParams(format!("{code}: {}", detail.into()))
}

/// Resolve one bounded roster snapshot and fence the target before a Hub
/// entry is created. Legacy roster entries without lease/generation fail
/// closed instead of silently degrading to PTY delivery.
fn resolve_hub_target(
    args: &Value,
    host: &dyn McpHost,
    required_capability: &str,
) -> HostResult<HubTarget> {
    let target = scoped_target(args, host)?;
    let target_pane = value_string(&target, &["paneId", "pane_id"])
        .or_else(|| args.get("target_pane_id").and_then(scalar_string))
        .ok_or_else(|| hub_error("target_missing", "target pane is not addressable"))?;
    let requested_workspace = arg_str(args, "workspace_id")
        .map(str::to_string)
        .or_else(|| value_string(&target, &["workspaceId", "workspace_id"]));
    let profile = host.team_profile_for(requested_workspace.as_deref())?;
    let requested_agent = arg_str(args, "agent_id");
    let mut entries = ["agent_identities", "roster"]
        .into_iter()
        .filter_map(|key| profile.get(key).and_then(Value::as_array))
        .flatten();
    let identity = entries
        .find(|entry| {
            let id_matches = requested_agent
                .map(|id| {
                    value_string(entry, &["agentId", "agent_id", "id"]).as_deref() == Some(id)
                })
                .unwrap_or(true);
            let pane_matches = value_string(entry, &["paneId", "pane_id"]).as_deref()
                == Some(target_pane.as_str());
            id_matches && pane_matches
        })
        .ok_or_else(|| {
            hub_error(
                "target_missing",
                "Agent identity is absent from Kernel roster",
            )
        })?;

    let agent_id = value_string(identity, &["agentId", "agent_id", "id"])
        .ok_or_else(|| hub_error("target_missing", "identity has no agent_id"))?;
    let session_id = value_string(identity, &["sessionId", "session_id"])
        .ok_or_else(|| hub_error("target_missing", "identity has no session_id"))?;
    let workspace_id = value_string(identity, &["workspaceId", "workspace_id"])
        .ok_or_else(|| hub_error("workspace_mismatch", "identity has no workspace_id"))?;
    if requested_workspace
        .as_deref()
        .is_some_and(|value| value != workspace_id)
    {
        return Err(hub_error(
            "workspace_mismatch",
            "target belongs to another workspace",
        ));
    }
    let pane_id = value_string(identity, &["paneId", "pane_id"])
        .ok_or_else(|| hub_error("target_missing", "identity has no pane_id"))?;
    let generation = identity
        .get("generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| hub_error("generation_mismatch", "identity has no generation"))?;
    if arg_u64(args, "generation").is_some_and(|value| value != generation) {
        return Err(hub_error(
            "generation_mismatch",
            "target generation is stale",
        ));
    }
    let lease = value_string(identity, &["lease"])
        .ok_or_else(|| hub_error("stale_lease", "identity has no lease"))?;
    if arg_str(args, "lease").is_some_and(|value| value != lease) {
        return Err(hub_error("stale_lease", "target lease is stale"));
    }
    let online = identity
        .get("online")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let lifecycle = value_string(identity, &["lifecycle"]).unwrap_or_default();
    if !online
        || !matches!(
            lifecycle.as_str(),
            "Online" | "Working" | "Waiting" | "Attention"
        )
    {
        return Err(hub_error(
            "target_offline",
            "target Agent is not receivable",
        ));
    }
    let capabilities = identity
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let capability_allowed = if required_capability == "delivery" {
        capabilities
            .iter()
            .any(|item| matches!(*item, "messages" | "tasks" | "events" | "control"))
    } else {
        capabilities.contains(&required_capability)
    };
    if !capability_allowed {
        return Err(hub_error(
            "capability_denied",
            format!("target lacks capability {required_capability}"),
        ));
    }
    Ok(HubTarget {
        target_key: host.pane_key(&target)?,
        agent_id,
        session_id,
        workspace_id,
        pane_id,
        generation,
        lease,
    })
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

fn tool_workspace(name: &str, args: &Value, host: &dyn McpHost) -> HostResult<String> {
    match name {
        "ridge_get_team_profile" | "ridge_list_agents" => host
            .team_profile_for(arg_str(args, "workspace_id"))
            .map(|profile| profile.to_string()),
        "ridge_list_workspaces" => host.list_workspaces().map(|items| items.to_string()),
        _ => host.launch_capabilities().and_then(|capabilities| {
            serde_json::to_string(&capabilities)
                .map_err(|error| HostError::Internal(error.to_string()))
        }),
    }
}

fn tool_hub_message(
    name: &str,
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let (kind, capability, payload, topic) = match name {
        "ridge_send_message" => (
            "message",
            "messages",
            arg_str(args, "message")
                .map(|text| {
                    let mut payload = json!({ "text": text });
                    if let Some(task_id) = arg_str(args, "a2a_task_id") {
                        payload["a2aTaskId"] = json!(task_id);
                    }
                    payload
                })
                .ok_or_else(|| HostError::InvalidParams("message must not be empty".into())),
            None,
        ),
        "ridge_create_task" => (
            "task",
            "tasks",
            arg_str(args, "objective")
                .map(|objective| json!({ "objective": objective }))
                .ok_or_else(|| HostError::InvalidParams("objective must not be empty".into())),
            None,
        ),
        _ => (
            "event",
            "events",
            arg_str(args, "topic")
                .map(|topic| json!({ "topic": topic, "payload": args.get("payload").cloned().unwrap_or(Value::Null) }))
                .ok_or_else(|| HostError::InvalidParams("topic must not be empty".into())),
            arg_str(args, "topic"),
        ),
    };
    let payload = payload?;
    let idempotency_key = arg_str(args, "idempotency_key")
        .ok_or_else(|| HostError::InvalidParams("idempotency_key must not be empty".into()))?;
    let priority = arg_str(args, "priority").unwrap_or(match kind {
        "task" => "task",
        "event" => "event",
        _ => "input",
    });
    if !matches!(priority, "control" | "input" | "task" | "event" | "history") {
        return Err(hub_error(
            "invalid_priority",
            "unsupported message priority",
        ));
    }
    let target = resolve_hub_target(args, host, capability)?;
    enqueue_hub_entry(
        host,
        state,
        &target,
        arg_str(args, "from").unwrap_or("mcp-client"),
        kind,
        payload,
        HubEntryMeta {
            idempotency_key,
            correlation_id: arg_str(args, "correlation_id"),
            causation_id: arg_str(args, "causation_id"),
            conversation_id: arg_str(args, "conversation_id"),
            priority,
            deadline_unix_ms: arg_u64(args, "deadline_unix_ms"),
            cancellation_id: arg_str(args, "cancellation_id"),
            topic,
        },
    )
    .map(|entry| entry.to_string())
}

fn tool_register_a2a_endpoint(
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let target = resolve_hub_target(args, host, "delivery")?;
    let agent_card_url = arg_str(args, "agent_card_url")
        .ok_or_else(|| HostError::InvalidParams("agent_card_url must not be empty".into()))?;
    let extensions = args
        .get("extensions")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_string).ok_or_else(|| {
                        HostError::InvalidParams("extensions must contain strings".into())
                    })
                })
                .collect::<HostResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let card = state
        .register_a2a_endpoint(
            target.agent_id.clone(),
            target.generation,
            target.lease.clone(),
            A2aClientConfig {
                agent_card_url: agent_card_url.to_string(),
                bearer_token: arg_str(args, "bearer_token").map(str::to_string),
                preferred_protocol_version: arg_str(args, "preferred_protocol_version")
                    .map(str::to_string),
                extensions,
                ..A2aClientConfig::default()
            },
        )
        .map_err(HostError::Internal)?;
    let protocol_version = card
        .supported_interfaces
        .iter()
        .find(|interface| {
            matches!(
                interface.protocol_binding.to_ascii_lowercase().as_str(),
                "jsonrpc" | "json-rpc"
            )
        })
        .map(|interface| interface.protocol_version.clone());
    Ok(json!({
        "registered": true,
        "adapter": "a2a",
        "agentId": target.agent_id,
        "generation": target.generation,
        "lease": target.lease,
        "agentCard": {
            "name": card.name,
            "version": card.version,
            "protocolVersion": protocol_version,
            "capabilities": card.capabilities,
        }
    })
    .to_string())
}

fn tool_unregister_a2a_endpoint(
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let target = resolve_hub_target(args, host, "delivery")?;
    let removed = state
        .unregister_a2a_endpoint(&target.agent_id, target.generation, &target.lease)
        .map_err(HostError::Internal)?;
    Ok(json!({
        "unregistered": removed,
        "adapter": "a2a",
        "agentId": target.agent_id,
        "generation": target.generation,
        "lease": target.lease,
    })
    .to_string())
}

fn tool_legacy_message(
    name: &str,
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let text = arg_str(args, "message")
        .or_else(|| arg_str(args, "objective"))
        .filter(|text| !text.is_empty())
        .ok_or_else(|| HostError::InvalidParams("message/objective 涓嶈兘涓虹┖".into()))?;
    let delegate = name == "ridge_delegate_task";
    let submit = name != "ridge_send_to_teammate"
        || args.get("submit").and_then(Value::as_bool).unwrap_or(true);
    let target = resolve_hub_target(args, host, if delegate { "tasks" } else { "messages" })?;
    let generated_key = format!("legacy:{name}:{}", Uuid::new_v4());
    let idempotency_key = arg_str(args, "idempotency_key").unwrap_or(&generated_key);
    let payload = if delegate {
        json!({ "objective": text, "submitRequested": submit })
    } else {
        json!({ "text": text, "submitRequested": submit })
    };
    let mut entry = enqueue_hub_entry(
        host,
        state,
        &target,
        arg_str(args, "from").unwrap_or("mcp-client"),
        if delegate { "task" } else { "message" },
        payload,
        HubEntryMeta {
            idempotency_key,
            correlation_id: arg_str(args, "correlation_id"),
            causation_id: arg_str(args, "causation_id"),
            conversation_id: arg_str(args, "conversation_id"),
            priority: if delegate { "task" } else { "input" },
            deadline_unix_ms: arg_u64(args, "deadline_unix_ms"),
            cancellation_id: arg_str(args, "cancellation_id"),
            topic: None,
        },
    )?;
    entry["legacyTool"] = Value::String(name.to_string());
    entry["submitRequested"] = Value::Bool(submit);
    Ok(entry.to_string())
}

fn tool_capture(args: &Value, host: &dyn McpHost) -> HostResult<String> {
    let lines = args
        .get("lines")
        .and_then(Value::as_u64)
        .unwrap_or(80)
        .clamp(1, 2000) as usize;
    let target = scoped_target(args, host)?;
    host.capture_pane(&target, lines)
}

fn tool_split(args: &Value, host: &dyn McpHost) -> HostResult<String> {
    let request = split_request(args)?;
    validate_launch_request(host, &request)?;
    host.split_pane_with(&request)
        .map(|value| split_result(value, &request).to_string())
}

fn tool_join(args: &Value, host: &dyn McpHost) -> HostResult<String> {
    let group = arg_str(args, "group_name")
        .ok_or_else(|| HostError::InvalidParams("group_name 涓嶈兘涓虹┖".into()))?;
    let agent = arg_str(args, "agent_id");
    let has_pane = args
        .get("target_pane_id")
        .is_some_and(|value| !value.is_null() && value != &Value::String(String::new()));
    if agent.is_none() && !has_pane {
        return Err(HostError::InvalidParams(
            "闇€鎻愪緵 agent_id 鎴?target_pane_id".into(),
        ));
    }
    let target = has_pane.then(|| scoped_target(args, host)).transpose()?;
    let workspace_target =
        (!has_pane).then(|| arg_str(args, "workspace_id").map(|id| json!({ "workspaceId": id })));
    let target = target.or_else(|| workspace_target.flatten());
    host.join_group(group, agent, target.as_ref())
        .map(|()| "dispatched".to_string())
}

fn tool_progress(args: &Value, host: &dyn McpHost) -> HostResult<String> {
    let target = scoped_target(args, host)?;
    host.report_progress(
        &target,
        arg_str(args, "status").unwrap_or("update"),
        arg_str(args, "detail").unwrap_or(""),
    )
    .map(|()| "reported".to_string())
}

fn tool_inbox_read(
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let target = scoped_target(args, host)?;
    let key = host.pane_key(&target)?;
    let peek = args.get("peek").and_then(Value::as_bool).unwrap_or(false);
    inbox_take(state, &key, peek)
        .map(|entries| Value::Array(entries).to_string())
        .map_err(HostError::Internal)
}

fn tool_inbox_fetch(
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let target = resolve_hub_target(args, host, "messages")?;
    let consume = args.get("consume").and_then(Value::as_bool).unwrap_or(true);
    let peek = args
        .get("peek")
        .and_then(Value::as_bool)
        .unwrap_or(!consume);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(INBOX_CAP as u64)
        .clamp(1, INBOX_CAP as u64) as usize;
    inbox_fetch(
        state,
        &target.target_key,
        peek,
        consume,
        limit,
        arg_str(args, "cursor"),
    )
    .map(|entries| Value::Array(entries).to_string())
    .map_err(HostError::Internal)
}

fn tool_cancel(args: &Value, host: &dyn McpHost, state: &McpSessionState) -> HostResult<String> {
    let target = resolve_hub_target(args, host, "delivery")?;
    let delivery_id = arg_str(args, "delivery_id");
    let cancellation_id = arg_str(args, "cancellation_id");
    if delivery_id.is_none() && cancellation_id.is_none() {
        return Err(HostError::InvalidParams(
            "delivery_id or cancellation_id must be provided".into(),
        ));
    }
    cancel_hub_entry(
        state,
        &target.target_key,
        delivery_id,
        cancellation_id,
        arg_str(args, "reason"),
    )
    .map(|entry| entry.to_string())
}

fn tool_delivery_status(
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let receipt_id = arg_str(args, "receipt_id")
        .ok_or_else(|| HostError::InvalidParams("receipt_id 涓嶈兘涓虹┖".into()))?;
    let target = scoped_target(args, host)?;
    let key = host.pane_key(&target)?;
    receipt_get(state, &key, receipt_id).map(|receipt| receipt.to_string())
}

fn tool_task_update(
    args: &Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> HostResult<String> {
    let task_id = arg_str(args, "task_id")
        .ok_or_else(|| HostError::InvalidParams("task_id must not be empty".into()))?;
    let status = arg_str(args, "status")
        .ok_or_else(|| HostError::InvalidParams("status must not be empty".into()))?;
    let target = resolve_hub_target(args, host, "tasks")?;
    task_update(
        state,
        &target.target_key,
        task_id,
        status,
        arg_str(args, "detail"),
    )
    .map(|value| value.to_string())
}

fn tool_ack(args: &Value, host: &dyn McpHost, state: &McpSessionState) -> HostResult<String> {
    let receipt_id = arg_str(args, "receipt_id")
        .ok_or_else(|| HostError::InvalidParams("receipt_id 涓嶈兘涓虹┖".into()))?;
    let status = arg_str(args, "status")
        .ok_or_else(|| HostError::InvalidParams("status 涓嶈兘涓虹┖".into()))?;
    let target = scoped_target(args, host)?;
    let key = host.pane_key(&target)?;
    receipt_ack(state, &key, receipt_id, status, arg_str(args, "detail"))
        .map(|receipt| receipt.to_string())
}

fn required_arg<'a>(args: &'a Value, key: &str) -> HostResult<&'a str> {
    arg_str(args, key).ok_or_else(|| HostError::InvalidParams(format!("{key} 涓嶈兘涓虹┖")))
}

fn tool_rejection(args: &Value, host: &dyn McpHost) -> HostResult<String> {
    let report = ExternalExecutionRejection {
        initiator: arg_str(args, "initiator")
            .unwrap_or("mcp-client")
            .to_string(),
        action: arg_str(args, "action").unwrap_or("").to_string(),
        executor: required_arg(args, "executor")?.to_string(),
        policy_source: required_arg(args, "policy_source")?.to_string(),
        request_id: required_arg(args, "request_id")?.to_string(),
        reason: required_arg(args, "reason")?.to_string(),
        next_step: required_arg(args, "next_step")?.to_string(),
    };
    host.report_execution_rejection(report).map(|id| {
        json!({
            "reportId": id,
            "status": "reported",
            "retry": "not_available_from_ridge",
        })
        .to_string()
    })
}

fn tool_stash(args: &Value, state: &McpSessionState) -> HostResult<String> {
    let data = arg_str(args, "data")
        .or_else(|| arg_str(args, "content_base64"))
        .ok_or_else(|| HostError::InvalidParams("data 涓嶈兘涓虹┖".into()))?;
    Ok(state
        .stash
        .lock()
        .unwrap()
        .stash_uri(data.as_bytes().to_vec()))
}

fn tools_call(id: Value, params: &Value, host: &dyn McpHost, state: &McpSessionState) -> Value {
    let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let out: HostResult<String> = match name {
        "ridge_get_team_profile"
        | "ridge_list_agents"
        | "ridge_list_workspaces"
        | "ridge_get_launch_capabilities" => tool_workspace(name, &args, host),

        "ridge_send_message" | "ridge_create_task" | "ridge_publish_event" => {
            tool_hub_message(name, &args, host, state)
        }

        "ridge_register_a2a_endpoint" => tool_register_a2a_endpoint(&args, host, state),
        "ridge_unregister_a2a_endpoint" => tool_unregister_a2a_endpoint(&args, host, state),

        "ridge_send_to_teammate" | "ridge_send_and_submit" | "ridge_delegate_task" => {
            tool_legacy_message(name, &args, host, state)
        }

        "ridge_capture_pane" => tool_capture(&args, host),
        "ridge_split_pane" => tool_split(&args, host),
        // agent_id  alone must not force pane resolve — historically scoped_target(null)
        // always failed on desktop (-32602) even when agent_id was valid.
        "ridge_join_group" => tool_join(&args, host),
        "ridge_report_progress" => tool_progress(&args, host),

        // 收发同一份有界收件箱：默认是内存态，生产宿主通过 SQLite 恢复同一份队列。
        // 任何 MCP 客户端都能异步取走发给某 pane 的消息，不必依赖 stdin 注入被对方 shell 正确读到。
        "ridge_inbox_read" => tool_inbox_read(&args, host, state),
        "ridge_fetch_inbox" => tool_inbox_fetch(&args, host, state),
        "ridge_cancel_delivery" => tool_cancel(&args, host, state),
        "ridge_delivery_status" => tool_delivery_status(&args, host, state),
        "ridge_task_update" => tool_task_update(&args, host, state),
        "ridge_acknowledge_receipt" => tool_ack(&args, host, state),
        "ridge_report_execution_rejection" => tool_rejection(&args, host),
        "ridge_stash_data" => tool_stash(&args, state),
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

/// Invoke one MCP tool through the same Hub/host path used by HTTP and WS
/// transports. Desktop IPC uses this small adapter so UI actions do not grow a
/// second message protocol.
pub fn call_tool_rpc(
    name: &str,
    arguments: Value,
    host: &dyn McpHost,
    state: &McpSessionState,
) -> Value {
    tools_call(
        json!(1),
        &json!({
            "name": name,
            "arguments": arguments,
        }),
        host,
        state,
    )
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delivery::HubPtySafety;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::thread;

    struct FakeHost;

    impl McpHost for FakeHost {
        fn team_profile(&self) -> Value {
            let roster = ["7", "8", "91"]
                .into_iter()
                .map(|pane_id| {
                    json!({
                        "id": format!("agent-{pane_id}"),
                        "agentId": format!("agent-{pane_id}"),
                        "sessionId": format!("session-{pane_id}"),
                        "workspaceId": "ws-test",
                        "paneId": pane_id,
                        "generation": 1,
                        "lease": format!("lease-{pane_id}"),
                        "lifecycle": "Online",
                        "online": true,
                        "capabilities": ["messages", "tasks", "events"]
                    })
                })
                .collect::<Vec<_>>();
            json!({ "workspaceId": "ws-test", "roster": roster })
        }
        fn send_text(
            &self,
            _target: &Value,
            _text: &str,
            _submit: bool,
            _busy: bool,
        ) -> HostResult<InputDispatch> {
            panic!("legacy compatibility route must not write PTY bytes")
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

    struct A2aHost {}

    impl McpHost for A2aHost {
        fn team_profile(&self) -> Value {
            json!({
                "workspaceId": "ws-test",
                "roster": [{
                    "id": "agent-a",
                    "agentId": "agent-a",
                    "sessionId": "session-a",
                    "workspaceId": "ws-test",
                    "paneId": "pane-a",
                    "generation": 1,
                    "lease": "lease-a",
                    "lifecycle": "Online",
                    "online": true,
                    "capabilities": ["messages", "tasks", "events"]
                }]
            })
        }

        fn send_text(
            &self,
            _target: &Value,
            _text: &str,
            _submit: bool,
            _busy: bool,
        ) -> HostResult<InputDispatch> {
            Ok(InputDispatch {
                terminal_accepted: false,
            })
        }

        fn capture_pane(&self, _target: &Value, _lines: usize) -> HostResult<String> {
            Ok(String::new())
        }

        fn split_pane(
            &self,
            _direction: &str,
            _role: &str,
            _initial_cmd: Option<&str>,
        ) -> HostResult<Value> {
            Ok(Value::Null)
        }

        fn read_resource(&self, _uri: &RidgeUri) -> HostResult<(String, String)> {
            Ok(("application/json".into(), "{}".into()))
        }

        fn pane_key(&self, target: &Value) -> HostResult<String> {
            Ok(target
                .get("paneId")
                .and_then(Value::as_str)
                .unwrap_or("pane-a")
                .into())
        }
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> (String, Value) {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            if let Some(header_end) = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| index + 4)
            {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end.saturating_add(content_length) {
                    break;
                }
            }
            let read = stream.read(&mut chunk).expect("read HTTP request");
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        let json = serde_json::from_str(body).unwrap_or(Value::Null);
        (raw, json)
    }

    #[test]
    fn hub_sends_through_registered_standard_a2a_endpoint_and_records_ack() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let fixture = thread::spawn(move || {
            let (mut card_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut card_stream);
            let card = json!({
                "name": "Ridge A2A fixture",
                "description": "A standard JSON-RPC fixture",
                "supportedInterfaces": [{
                    "url": format!("http://{address}/rpc"),
                    "protocolBinding": "JSONRPC",
                    "protocolVersion": "1.0"
                }],
                "version": "fixture-1",
                "capabilities": {},
                "defaultInputModes": ["text/plain"],
                "defaultOutputModes": ["text/plain"],
                "skills": []
            })
            .to_string();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                card.len()
            );
            card_stream.write_all(head.as_bytes()).unwrap();
            card_stream.write_all(card.as_bytes()).unwrap();

            let (mut rpc_stream, _) = listener.accept().unwrap();
            let (raw, request) = read_http_request(&mut rpc_stream);
            assert!(raw.to_ascii_lowercase().contains("a2a-version: 1.0"));
            assert_eq!(request["method"], "SendMessage");
            assert_eq!(request["params"]["message"]["parts"][0]["text"], "hello");
            let body = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {"task": {"id": "remote-task-1"}}
            })
            .to_string();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            rpc_stream.write_all(head.as_bytes()).unwrap();
            rpc_stream.write_all(body.as_bytes()).unwrap();
        });

        let state = Arc::new(McpSessionState::default());
        state
            .register_a2a_endpoint(
                "agent-a",
                1,
                "lease-a",
                A2aClientConfig {
                    agent_card_url: format!("http://{address}/card"),
                    ..A2aClientConfig::default()
                },
            )
            .unwrap();
        let host = A2aHost {};
        let response = call_tool_rpc(
            "ridge_send_message",
            json!({
                "target_pane_id": "pane-a",
                "message": "hello",
                "from": "sender",
                "idempotency_key": "a2a-1"
            }),
            &host,
            &state,
        );
        let entry: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("Hub result"),
        )
        .unwrap();
        assert_eq!(entry["deliveryAdapter"], "a2a");
        assert_eq!(entry["adapterAccepted"], true);
        assert_eq!(entry["agentAcknowledged"], true);
        assert_eq!(entry["adapterRemoteId"], "remote-task-1");
        fixture.join().unwrap();
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
            Ok(json!({
                "workspaceId": workspace_id,
                "roster": [{
                    "id": "capable-agent",
                    "agentId": "capable-agent",
                    "sessionId": "capable-session",
                    "workspaceId": workspace_id,
                    "paneId": 8,
                    "generation": 1,
                    "lease": "capable-lease",
                    "lifecycle": "Online",
                    "online": true,
                    "capabilities": ["messages", "tasks", "events"]
                }]
            }))
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
            _target: &Value,
            _text: &str,
            _submit: bool,
            _busy: bool,
        ) -> HostResult<InputDispatch> {
            panic!("legacy compatibility route must not write PTY bytes")
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

    struct AdapterHost {
        adapter: HubDeliveryAdapter,
        sent: std::sync::Arc<std::sync::Mutex<Vec<(String, bool, bool)>>>,
    }

    impl AdapterHost {
        fn new(adapter: HubDeliveryAdapter) -> Self {
            Self {
                adapter,
                sent: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    impl McpHost for AdapterHost {
        fn team_profile(&self) -> Value {
            Value::Null
        }

        fn send_text(
            &self,
            _target: &Value,
            _text: &str,
            _submit: bool,
            _busy: bool,
        ) -> HostResult<InputDispatch> {
            self.sent
                .lock()
                .unwrap()
                .push((_text.to_string(), _submit, _busy));
            Ok(InputDispatch {
                terminal_accepted: true,
            })
        }

        fn capture_pane(&self, _target: &Value, _lines: usize) -> HostResult<String> {
            Ok(String::new())
        }

        fn split_pane(
            &self,
            _direction: &str,
            _role: &str,
            _initial_cmd: Option<&str>,
        ) -> HostResult<Value> {
            Ok(Value::Null)
        }

        fn read_resource(&self, _uri: &RidgeUri) -> HostResult<(String, String)> {
            Ok(("text/plain".into(), String::new()))
        }

        fn pane_key(&self, target: &Value) -> HostResult<String> {
            Ok(target.to_string())
        }

        fn probe_delivery(&self, _target: &Value) -> HostResult<DeliveryProbe> {
            Ok(match self.adapter {
                HubDeliveryAdapter::RuntimeApi => DeliveryProbe {
                    runtime_api: true,
                    ..Default::default()
                },
                HubDeliveryAdapter::A2a => DeliveryProbe {
                    a2a: true,
                    ..Default::default()
                },
                HubDeliveryAdapter::PtyFallback => DeliveryProbe {
                    pty: crate::delivery::HubPtySafety {
                        agent_idle: true,
                        terminal_mode_agent_prompt: true,
                        foreground_is_target_agent: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                HubDeliveryAdapter::McpPull => DeliveryProbe {
                    mcp_pull: true,
                    ..Default::default()
                },
            })
        }

        fn deliver_runtime_api(
            &self,
            _target: &Value,
            _entry: &Value,
        ) -> HostResult<DeliveryOutcome> {
            Ok(DeliveryOutcome {
                adapter: HubDeliveryAdapter::RuntimeApi,
                accepted: true,
                remote_id: Some("runtime-1".into()),
                acknowledged: true,
            })
        }

        fn deliver_a2a(&self, _target: &Value, _entry: &Value) -> HostResult<DeliveryOutcome> {
            Ok(DeliveryOutcome {
                adapter: HubDeliveryAdapter::A2a,
                accepted: true,
                remote_id: Some("a2a-1".into()),
                acknowledged: false,
            })
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
        let receipt: Value =
            serde_json::from_str(sent["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        let read = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_inbox_read","arguments":{"target_pane_id":7}}
        })
        .to_string();
        assert_ne!(
            call_with_state(&read, &FakeHost, &first)["result"]["content"][0]["text"],
            "[]"
        );
        assert_eq!(
            call_with_state(&read, &FakeHost, &second)["result"]["content"][0]["text"],
            "[]"
        );
        first.purge_pane("7").expect("purge in-memory Hub state");
        let status = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_delivery_status","arguments":{"target_pane_id":7,"receipt_id":receipt["deliveryId"]}}
        })
        .to_string();
        assert!(
            call_with_state(&status, &FakeHost, &first)["error"]["message"]
                .as_str()
                .unwrap()
                .contains("不存在")
        );
    }

    #[test]
    fn typed_hub_tools_queue_ids_and_update_task_without_claiming_terminal_delivery() {
        let state = McpSessionState::default();
        let send = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_message","arguments":{
                "target_pane_id":7,"message":"hello","from":"agent-a",
                "idempotency_key":"message-1"
            }}
        })
        .to_string();
        let sent = call_with_state(&send, &FakeHost, &state);
        let message: Value = serde_json::from_str(
            sent["result"]["content"][0]["text"]
                .as_str()
                .expect("message result"),
        )
        .unwrap();
        assert_eq!(message["status"], "queued");
        assert_eq!(message["deliveryAdapter"], "mcp_pull");
        assert_eq!(message["deliveryReliability"], "at_least_once");
        assert_eq!(message["terminalAccepted"], false);
        assert!(message["messageId"].as_str().is_some());
        assert!(message["deliveryId"].as_str().is_some());

        let task = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_create_task","arguments":{
                "target_pane_id":7,"objective":"review","idempotency_key":"task-1"
            }}
        })
        .to_string();
        let task: Value = serde_json::from_str(
            call_with_state(&task, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let task_id = task["taskId"].as_str().unwrap();
        let update = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_task_update","arguments":{
                "target_pane_id":7,"task_id":task_id,"status":"accepted"
            }}
        })
        .to_string();
        let updated: Value = serde_json::from_str(
            call_with_state(&update, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(updated["taskStatus"], "accepted");

        let fetch = json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"ridge_fetch_inbox","arguments":{"target_pane_id":7,"consume":true}}
        })
        .to_string();
        let fetched: Value = serde_json::from_str(
            call_with_state(&fetch, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(fetched.as_array().unwrap().len(), 2);
    }

    #[test]
    fn hub_fences_target_and_deduplicates_idempotent_messages() {
        let state = McpSessionState::default();
        let request = |id: u64, key: &str, generation: Option<u64>| {
            let mut arguments = json!({
                "target_pane_id": 7,
                "message": "once",
                "from": "agent-a",
                "idempotency_key": key
            });
            if let Some(generation) = generation {
                arguments["generation"] = json!(generation);
            }
            json!({
                "jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"ridge_send_message","arguments":arguments}
            })
            .to_string()
        };

        let first: Value = serde_json::from_str(
            call_with_state(&request(1, "once-1", None), &FakeHost, &state)["result"]["content"][0]
                ["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let duplicate: Value = serde_json::from_str(
            call_with_state(&request(2, "once-1", None), &FakeHost, &state)["result"]["content"][0]
                ["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(duplicate["deduplicated"], true);
        assert_eq!(duplicate["messageId"], first["messageId"]);
        assert_eq!(duplicate["deliveryId"], first["deliveryId"]);

        let mut conflict = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_send_message","arguments":{
                "target_pane_id":7,"message":"changed","from":"agent-a",
                "idempotency_key":"once-1"
            }}
        });
        conflict["params"]["arguments"]["message"] = json!("changed");
        let conflict = call_with_state(&conflict.to_string(), &FakeHost, &state);
        assert!(conflict["error"]["message"]
            .as_str()
            .unwrap()
            .contains("idempotency_conflict"));

        let stale = call_with_state(&request(4, "once-2", Some(0)), &FakeHost, &state);
        assert!(stale["error"]["message"]
            .as_str()
            .unwrap()
            .contains("generation_mismatch"));
    }

    #[test]
    fn hub_rejects_expired_deadlines_and_cancellation_removes_pending_delivery() {
        let state = McpSessionState::default();
        let expired = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_send_message","arguments":{
                "target_pane_id":7,"message":"late","from":"agent-a",
                "idempotency_key":"expired-1","deadline_unix_ms":1
            }}
        })
        .to_string();
        let expired = call_with_state(&expired, &FakeHost, &state);
        assert!(expired["error"]["message"]
            .as_str()
            .unwrap()
            .contains("deadline_exceeded"));

        let send = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"ridge_send_message","arguments":{
                "target_pane_id":7,"message":"cancel me","from":"agent-a",
                "idempotency_key":"cancel-1","cancellation_id":"cancel-token-1"
            }}
        })
        .to_string();
        let sent: Value = serde_json::from_str(
            call_with_state(&send, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let cancel = json!({
            "jsonrpc":"2.0","id":3,"method":"tools/call",
            "params":{"name":"ridge_cancel_delivery","arguments":{
                "target_pane_id":7,"delivery_id":sent["deliveryId"],"reason":"user stopped"
            }}
        })
        .to_string();
        let cancelled: Value = serde_json::from_str(
            call_with_state(&cancel, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["cancellationRequested"], true);

        let ack_after_cancel = json!({
            "jsonrpc":"2.0","id":31,"method":"tools/call",
            "params":{"name":"ridge_acknowledge_receipt","arguments":{
                "target_pane_id":7,"receipt_id":sent["deliveryId"],"status":"agent_received"
            }}
        })
        .to_string();
        assert!(
            call_with_state(&ack_after_cancel, &FakeHost, &state)["error"]["message"]
                .as_str()
                .unwrap()
                .contains("delivery_terminal")
        );

        let fetch = json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"ridge_fetch_inbox","arguments":{"target_pane_id":7}}
        })
        .to_string();
        assert_eq!(
            call_with_state(&fetch, &FakeHost, &state)["result"]["content"][0]["text"],
            "[]"
        );

        let replay: Value = serde_json::from_str(
            call_with_state(&send, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(replay["deduplicated"], true);
        assert_eq!(replay["status"], "cancelled");
    }

    #[test]
    fn task_update_rejects_invalid_transition_and_syncs_consumed_receipt() {
        let state = McpSessionState::default();
        let create = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"ridge_create_task","arguments":{
                "target_pane_id":7,"objective":"review","idempotency_key":"task-transition"
            }}
        })
        .to_string();
        let task: Value = serde_json::from_str(
            call_with_state(&create, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        let task_id = task["taskId"].as_str().unwrap();
        let update = |id: u64, status: &str| {
            json!({
                "jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"ridge_task_update","arguments":{
                    "target_pane_id":7,"task_id":task_id,"status":status
                }}
            })
            .to_string()
        };
        let accepted = call_with_state(&update(2, "accepted"), &FakeHost, &state);
        assert!(accepted["result"]["content"][0]["text"].as_str().is_some());
        let invalid = call_with_state(&update(3, "created"), &FakeHost, &state);
        assert!(invalid["error"]["message"]
            .as_str()
            .unwrap()
            .contains("invalid_task_transition"));
        let fetched = json!({
            "jsonrpc":"2.0","id":4,"method":"tools/call",
            "params":{"name":"ridge_fetch_inbox","arguments":{"target_pane_id":7,"peek":true}}
        })
        .to_string();
        let inbox: Value = serde_json::from_str(
            call_with_state(&fetched, &FakeHost, &state)["result"]["content"][0]["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(inbox[0]["taskStatus"], "accepted");
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
        assert!(default_text.contains("\"status\":\"queued\""));
        assert!(draft_text.contains("\"submitRequested\":false"));
        assert!(submit_text.contains("\"submitRequested\":true"));
        assert!(submit_text.contains("\"deliveryAdapter\":\"mcp_pull\""));
        assert!(submit_text.contains("\"terminalAccepted\":false"));
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
        assert_eq!(receipt["status"], "queued");
        assert_eq!(receipt["deliveryAdapter"], "mcp_pull");
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
        let receipt_id = sent["deliveryId"]
            .as_str()
            .expect("delivery id")
            .to_string();

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
        assert_eq!(before["status"], "queued");
        assert_eq!(before["terminalAccepted"], false);
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

    fn adapter_test_target() -> HubTarget {
        HubTarget {
            target_key: "adapter-pane".into(),
            agent_id: "adapter-agent".into(),
            session_id: "adapter-session".into(),
            workspace_id: "adapter-workspace".into(),
            pane_id: "adapter-pane".into(),
            generation: 1,
            lease: "adapter-lease".into(),
        }
    }

    fn adapter_test_meta() -> HubEntryMeta<'static> {
        HubEntryMeta {
            idempotency_key: "adapter-key",
            correlation_id: None,
            causation_id: None,
            conversation_id: None,
            priority: "input",
            deadline_unix_ms: None,
            cancellation_id: None,
            topic: None,
        }
    }

    #[test]
    fn hub_dispatches_probed_runtime_a2a_and_guarded_pty_adapters() {
        for adapter in [
            HubDeliveryAdapter::RuntimeApi,
            HubDeliveryAdapter::A2a,
            HubDeliveryAdapter::PtyFallback,
        ] {
            let state = McpSessionState::default();
            let entry = enqueue_hub_entry(
                &AdapterHost::new(adapter),
                &state,
                &adapter_test_target(),
                "sender",
                "message",
                json!({ "text": "adapter" }),
                adapter_test_meta(),
            )
            .expect("adapter delivery");
            assert_eq!(entry["deliveryAdapter"], adapter.as_str());
            assert_eq!(entry["deliveryReliability"], adapter.reliability());
            assert_eq!(entry["status"], "adapter_accepted");
            assert_eq!(entry["adapterAccepted"], true);
            assert_eq!(entry["deliveryAttempts"], 1);
            assert!(entry["deliveryLastAttemptAtUnixMs"].as_i64().is_some());
        }
    }

    #[test]
    fn pty_fallback_accepts_objective_without_submit_and_rejects_non_text_payload() {
        let host = AdapterHost::new(HubDeliveryAdapter::PtyFallback);
        let target = json!({ "paneId": "adapter-pane" });
        let outcome = host
            .deliver_pty_fallback(
                &target,
                &json!({
                    "payload": { "objective": "inspect", "submitRequested": false }
                }),
            )
            .expect("objective should use the guarded PTY fallback");

        assert_eq!(outcome.adapter, HubDeliveryAdapter::PtyFallback);
        assert!(outcome.accepted);
        assert_eq!(
            host.sent.lock().unwrap().as_slice(),
            [("inspect".to_string(), false, true)]
        );

        let error = host
            .deliver_pty_fallback(&target, &json!({ "payload": { "text": 42 } }))
            .expect_err("non-text PTY payload must fail closed");
        assert!(error.message().contains("payload must be text"));
    }

    #[test]
    fn concurrent_identical_sends_create_one_logical_message() {
        let state = std::sync::Arc::new(McpSessionState::default());
        let target = adapter_test_target();
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let state = std::sync::Arc::clone(&state);
            let target = target.clone();
            let start = std::sync::Arc::clone(&start);
            workers.push(std::thread::spawn(move || {
                start.wait();
                enqueue_hub_entry(
                    &FakeHost,
                    &state,
                    &target,
                    "concurrent-sender",
                    "message",
                    json!({ "text": "once" }),
                    HubEntryMeta {
                        idempotency_key: "same-key",
                        correlation_id: None,
                        causation_id: None,
                        conversation_id: None,
                        priority: "input",
                        deadline_unix_ms: None,
                        cancellation_id: None,
                        topic: None,
                    },
                )
                .expect("concurrent enqueue")
            }));
        }

        let entries = workers
            .into_iter()
            .map(|worker| worker.join().expect("enqueue worker must finish"))
            .collect::<Vec<_>>();
        let message_ids = entries
            .iter()
            .filter_map(|entry| entry.get("messageId").and_then(Value::as_str))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(message_ids.len(), 1);
        assert_eq!(
            inbox_fetch(&state, "adapter-pane", true, false, 20, None)
                .expect("read concurrent inbox")
                .len(),
            1
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.get("deduplicated") == Some(&Value::Bool(true)))
                .count(),
            7
        );
    }

    #[test]
    fn inbox_cursor_can_resume_by_sequence_and_rejects_unknown_text() {
        let state = McpSessionState::default();
        let target = adapter_test_target();
        for key in ["cursor-1", "cursor-2", "cursor-3"] {
            enqueue_hub_entry(
                &FakeHost,
                &state,
                &target,
                "cursor-sender",
                "message",
                json!({ "text": key }),
                HubEntryMeta {
                    idempotency_key: key,
                    correlation_id: None,
                    causation_id: None,
                    conversation_id: None,
                    priority: "input",
                    deadline_unix_ms: None,
                    cancellation_id: None,
                    topic: None,
                },
            )
            .expect("enqueue cursor message");
        }

        let first = inbox_fetch(&state, "adapter-pane", true, false, 1, None)
            .expect("fetch first cursor page");
        let cursor = first[0]["sequence"].as_u64().unwrap().to_string();
        let resumed = inbox_fetch(&state, "adapter-pane", true, false, 10, Some(&cursor))
            .expect("resume by sequence cursor");
        assert_eq!(
            resumed
                .iter()
                .map(|entry| entry["sequence"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert!(
            inbox_fetch(&state, "adapter-pane", true, false, 10, Some("bad-cursor"))
                .expect_err("unknown cursor must fail closed")
                .contains("cursor")
        );
    }

    #[test]
    fn sqlite_hub_rehydrates_and_persists_ack_and_consume() {
        let path = std::env::temp_dir().join(format!("ridge-mcp-hub-{}.sqlite3", Uuid::new_v4()));
        let entry = json!({
            "deliveryId": "delivery-1",
            "messageId": "message-1",
            "targetKey": "pane-1",
            "kind": "message",
            "status": "queued",
            "agentAcknowledged": false,
            "payload": {"text": "durable"}
        });
        {
            let state = McpSessionState::with_sqlite(&path).expect("create SQLite Hub");
            state
                .persistence
                .as_ref()
                .expect("persistent backend")
                .persist_enqueue("pane-1", &entry, "sender:key-1")
                .expect("persist message");
            receipt_insert(&state, "delivery-1".into(), entry.clone());
            inbox_push(&state, "pane-1", entry.clone());
            dedupe_insert(&state, "sender:key-1".into(), entry);
        }

        {
            let state = McpSessionState::with_sqlite(&path).expect("reopen SQLite Hub");
            let fetched = inbox_fetch(&state, "pane-1", true, false, 10, None)
                .expect("fetch rehydrated message");
            assert_eq!(fetched.len(), 1);
            assert_eq!(fetched[0]["payload"]["text"], "durable");
            assert!(dedupe_lookup(&state, "sender:key-1").is_some());

            let acknowledged = receipt_ack(
                &state,
                "pane-1",
                "delivery-1",
                "agent_acknowledged",
                Some("received"),
            )
            .expect("ack durable message");
            assert_eq!(acknowledged["status"], "agent_acknowledged");
        }

        {
            let state = McpSessionState::with_sqlite(&path).expect("reopen acknowledged Hub");
            let acknowledged =
                receipt_get(&state, "pane-1", "delivery-1").expect("read acknowledged receipt");
            assert_eq!(acknowledged["status"], "agent_acknowledged");
            let consumed = inbox_fetch(&state, "pane-1", false, true, 10, None)
                .expect("consume durable message");
            assert_eq!(consumed.len(), 1);
        }

        {
            let state = McpSessionState::with_sqlite(&path).expect("reopen consumed Hub");
            assert!(inbox_fetch(&state, "pane-1", true, false, 10, None)
                .expect("read consumed inbox")
                .is_empty());
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[test]
    fn sqlite_hub_idempotency_is_shared_between_state_instances() {
        let path = std::env::temp_dir().join(format!("ridge-mcp-hub-{}.sqlite3", Uuid::new_v4()));
        let target = HubTarget {
            target_key: "pane-2".into(),
            agent_id: "agent-2".into(),
            session_id: "session-2".into(),
            workspace_id: "workspace-2".into(),
            pane_id: "pane-2".into(),
            generation: 2,
            lease: "lease-2".into(),
        };
        let first = {
            let state = McpSessionState::with_sqlite(&path).expect("create SQLite Hub");
            enqueue_hub_entry(
                &FakeHost,
                &state,
                &target,
                "sender-2",
                "message",
                json!({"text":"once"}),
                HubEntryMeta {
                    idempotency_key: "same-key",
                    correlation_id: None,
                    causation_id: None,
                    conversation_id: None,
                    priority: "normal",
                    deadline_unix_ms: None,
                    cancellation_id: None,
                    topic: None,
                },
            )
            .expect("enqueue first message")
        };
        let second = {
            let state = McpSessionState::with_sqlite(&path).expect("reopen SQLite Hub");
            enqueue_hub_entry(
                &FakeHost,
                &state,
                &target,
                "sender-2",
                "message",
                json!({"text":"once"}),
                HubEntryMeta {
                    idempotency_key: "same-key",
                    correlation_id: None,
                    causation_id: None,
                    conversation_id: None,
                    priority: "normal",
                    deadline_unix_ms: None,
                    cancellation_id: None,
                    topic: None,
                },
            )
            .expect("deduplicate second message")
        };
        assert_eq!(first["messageId"], second["messageId"]);
        assert_eq!(second["deduplicated"], true);

        let third = {
            let state = McpSessionState::with_sqlite(&path).expect("reopen sequenced Hub");
            enqueue_hub_entry(
                &AdapterHost::new(HubDeliveryAdapter::McpPull),
                &state,
                &target,
                "sender-2",
                "message",
                json!({"text":"twice"}),
                HubEntryMeta {
                    idempotency_key: "different-key",
                    ..adapter_test_meta()
                },
            )
            .expect("allocate next sequence")
        };
        assert_eq!(first["sequence"], 1);
        assert_eq!(third["sequence"], 2);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[test]
    fn sqlite_hub_persists_cancellation_and_removes_pending_message() {
        let path = std::env::temp_dir().join(format!("ridge-mcp-hub-{}.sqlite3", Uuid::new_v4()));
        let target = adapter_test_target();
        let entry = {
            let state = McpSessionState::with_sqlite(&path).expect("create SQLite Hub");
            enqueue_hub_entry(
                &FakeHost,
                &state,
                &target,
                "persistent-sender",
                "message",
                json!({"text":"cancel me"}),
                HubEntryMeta {
                    idempotency_key: "persistent-cancel",
                    cancellation_id: Some("persistent-cancel-token"),
                    ..adapter_test_meta()
                },
            )
            .expect("enqueue persistent message")
        };
        {
            let state = McpSessionState::with_sqlite(&path).expect("reopen SQLite Hub");
            let cancelled = cancel_hub_entry(
                &state,
                &target.target_key,
                entry["deliveryId"].as_str(),
                None,
                Some("shutdown"),
            )
            .expect("cancel persistent message");
            assert_eq!(cancelled["status"], "cancelled");
        }
        {
            let state = McpSessionState::with_sqlite(&path).expect("reopen cancelled Hub");
            assert_eq!(
                dedupe_lookup(&state, "persistent-sender:persistent-cancel").unwrap()["status"],
                "cancelled"
            );
            assert_eq!(
                receipt_get(
                    &state,
                    &target.target_key,
                    entry["deliveryId"].as_str().unwrap()
                )
                .unwrap()["status"],
                "cancelled"
            );
            assert!(
                inbox_fetch(&state, &target.target_key, true, false, 10, None)
                    .unwrap()
                    .is_empty()
            );
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[test]
    fn session_state_routes_registered_runtime_without_blocking() {
        let state = McpSessionState::default();
        let receiver = state
            .register_delivery_endpoint(HubDeliveryAdapter::RuntimeApi, "agent-a", 2, "lease-2")
            .expect("register runtime route");
        let target = json!({
            "agentId": "agent-a",
            "generation": 2,
            "lease": "lease-2"
        });
        assert!(state.delivery_probe(&target).runtime_api);
        let safe = HubPtySafety {
            agent_idle: true,
            terminal_mode_agent_prompt: true,
            foreground_is_target_agent: true,
            ..Default::default()
        };
        state
            .register_pty_runtime_snapshot(
                "agent-a",
                2,
                "lease-2",
                HubPtyRuntimeSnapshot::new(safe, 1, 1),
            )
            .expect("register PTY safety proof");
        assert_eq!(state.delivery_probe(&target).pty, safe);
        let entry = json!({"messageId":"message-1"});
        let outcome = state
            .deliver_registered_endpoint(HubDeliveryAdapter::RuntimeApi, &target, &entry)
            .expect("deliver runtime route");
        assert!(outcome.accepted);
        assert_eq!(receiver.recv().unwrap(), entry);
        assert!(state
            .unregister_delivery_endpoint(HubDeliveryAdapter::RuntimeApi, "agent-a", 2, "lease-2")
            .unwrap());
        assert!(state
            .unregister_pty_runtime_snapshot("agent-a", 2, "lease-2")
            .unwrap());
        assert!(!state.delivery_probe(&target).runtime_api);
        assert_eq!(state.delivery_probe(&target).pty, HubPtySafety::default());
    }

    #[test]
    fn stream_ack_is_fenced_by_receipt_identity_generation_and_lease() {
        let state = McpSessionState::default();
        let target = adapter_test_target();
        let entry = enqueue_hub_entry(
            &AdapterHost::new(HubDeliveryAdapter::RuntimeApi),
            &state,
            &target,
            "sender",
            "message",
            json!({"text":"cross-process"}),
            adapter_test_meta(),
        )
        .expect("enqueue delivery");
        let delivery_id = entry["deliveryId"].as_str().expect("delivery id");
        assert!(state
            .acknowledge_delivery(
                "adapter-agent",
                0,
                "adapter-lease",
                delivery_id,
                "agent_received",
                None,
            )
            .is_err());
        let acknowledged = state
            .acknowledge_delivery(
                "adapter-agent",
                1,
                "adapter-lease",
                delivery_id,
                "agent_acknowledged",
                Some("received over stream"),
            )
            .expect("acknowledge delivery");
        assert_eq!(acknowledged["agentAcknowledged"], true);
        assert_eq!(acknowledged["ack"]["state"], "acked");
    }
}
