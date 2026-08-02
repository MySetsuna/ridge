//! 桌面 `RemoteHost` 实现 —— 为 `crate::state::AppState` 的包装器 `DesktopHost`
//! 实现共享 `ridge_remote::host` 的 `HostMeta` / `HostAuth` / `WorkspaceProvider`
//! + `RemoteHost::serve_websocket`，让桌面 LAN 远控经共享 `server_app::run` 驱动。
//!
//! 放在 src-tauri/src/ 顶层。`serve_websocket` 里封装整段每连接 WS 会话（原
//! `server.rs::handle_ws` + 各 dispatcher）；桌面保留 `crate::commands::*` 与
//! `ridge-core` 迁移双腿（D-GM-2）。core_bridge 依赖 `AppHandle`，与本文件平级
//! 放在 `crate::remote_bridge`。

use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use portable_pty::PtySize;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use ridge_remote::auth::{RemoteAuth, ThrottleDecision};
use ridge_remote::host::{HostAuth, HostError, HostMeta, RemoteHost, WorkspaceProvider, WsConn};
use ridge_remote::serve::UaServeConfig;

use crate::state::{AppState, RemotePaneSub, RemoteSubId};

type ScheduledPtyEvent = (bool, crate::types::RemotePtyEvent);

const MAX_IN_FLIGHT_DATA_REQUESTS: usize = 32;
const MAX_PRE_CANCELLED_DATA_REQUESTS: usize = 1024;
/// Invoke requests (the browser desktop bridge and the legacy mobile
/// `invoke-request` path) run independently from the reader loop so a slow
/// command cannot block `$/cancel`, PTY input, or health checks.
const MAX_IN_FLIGHT_INVOKE_REQUESTS: usize = 32;
const MAX_PRE_CANCELLED_INVOKE_REQUESTS: usize = 1024;

struct PendingDataRequest {
    handle: tokio::task::JoinHandle<()>,
    git_slot: String,
}

/// Per-WebSocket data-request lifecycle. A request is either in-flight,
/// pre-cancelled (cancel raced ahead of request), or completed exactly once.
/// Keeping the task handle here makes disconnect and pane teardown observable
/// cancellation points instead of merely dropping a frontend promise.
struct DataRequestRegistry {
    entries: std::collections::HashMap<u64, PendingDataRequest>,
    pre_cancelled: std::collections::HashSet<u64>,
}

/// Wire identity for an invoke request. Native JSON-RPC uses a serialized id
/// (normally a number); the legacy envelope has its own numeric namespace so
/// a late cancellation cannot accidentally cancel a request from the other
/// leg.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum InvokeRequestKey {
    Legacy(u64),
    JsonRpc(String),
}

impl InvokeRequestKey {
    fn jsonrpc(id: &serde_json::Value) -> Self {
        Self::JsonRpc(serde_json::to_string(id).unwrap_or_else(|_| "null".to_string()))
    }

    fn slot_name(&self) -> String {
        match self {
            Self::Legacy(id) => format!("legacy:{id}"),
            Self::JsonRpc(id) => format!("jsonrpc:{id}"),
        }
    }
}

enum InvokeResult {
    Legacy {
        key: InvokeRequestKey,
        reply: serde_json::Value,
    },
    JsonRpc {
        key: InvokeRequestKey,
        reply: serde_json::Value,
    },
}

struct PendingInvokeRequest {
    handle: tokio::task::JoinHandle<()>,
    git_slot: String,
}

/// Per-WebSocket invoke lifecycle. The request reader remains responsive while
/// a command runs; cancellation removes ownership before aborting the task, so
/// a result racing with cancellation is dropped exactly once.
struct InvokeRequestRegistry {
    entries: std::collections::HashMap<InvokeRequestKey, PendingInvokeRequest>,
    pre_cancelled: std::collections::HashSet<InvokeRequestKey>,
}

impl InvokeRequestRegistry {
    fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            pre_cancelled: std::collections::HashSet::new(),
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn contains(&self, key: &InvokeRequestKey) -> bool {
        self.entries.contains_key(key)
    }

    fn insert(&mut self, key: InvokeRequestKey, entry: PendingInvokeRequest) -> bool {
        if self.entries.contains_key(&key) {
            return false;
        }
        self.entries.insert(key, entry);
        true
    }

    fn take_pre_cancelled(&mut self, key: &InvokeRequestKey) -> bool {
        self.pre_cancelled.remove(key)
    }

    /// Abort an in-flight task, or remember a cancel that raced ahead of its
    /// request frame. Return the Git slot so the process guard can kill any
    /// already-spawned child tree before the task unwinds.
    fn cancel(&mut self, key: &InvokeRequestKey) -> Option<String> {
        self.cancel_keys(std::slice::from_ref(key))
    }

    /// Cancel across wire aliases without recording multiple tombstones. A
    /// browser request may have been translated to legacy `invoke-request`
    /// before the JSON-RPC handshake, while its `$/cancel` still carries the
    /// native JSON-RPC notification.
    fn cancel_keys(&mut self, keys: &[InvokeRequestKey]) -> Option<String> {
        for key in keys {
            if let Some(entry) = self.entries.remove(key) {
                entry.handle.abort();
                return Some(entry.git_slot);
            }
        }
        for key in keys {
            if self.pre_cancelled.len() >= MAX_PRE_CANCELLED_INVOKE_REQUESTS {
                break;
            }
            self.pre_cancelled.insert(key.clone());
        }
        None
    }

    fn complete(&mut self, key: &InvokeRequestKey) -> bool {
        self.entries.remove(key).is_some()
    }

    fn cancel_all(&mut self) -> Vec<String> {
        let mut slots = Vec::with_capacity(self.entries.len());
        for (_, entry) in self.entries.drain() {
            entry.handle.abort();
            slots.push(entry.git_slot);
        }
        self.pre_cancelled.clear();
        slots
    }
}

impl DataRequestRegistry {
    fn new() -> Self {
        Self {
            entries: std::collections::HashMap::new(),
            pre_cancelled: std::collections::HashSet::new(),
        }
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn insert(&mut self, id: u64, entry: PendingDataRequest) -> bool {
        if self.entries.contains_key(&id) {
            return false;
        }
        self.entries.insert(id, entry);
        true
    }

    fn contains(&self, id: u64) -> bool {
        self.entries.contains_key(&id)
    }

    fn take_pre_cancelled(&mut self, id: u64) -> bool {
        self.pre_cancelled.remove(&id)
    }

    /// Abort the task if present; otherwise remember the cancellation for a
    /// request frame that is still in flight on the WebSocket.
    fn cancel(&mut self, id: u64) -> Option<String> {
        if let Some(entry) = self.entries.remove(&id) {
            entry.handle.abort();
            return Some(entry.git_slot);
        }
        if self.pre_cancelled.len() < MAX_PRE_CANCELLED_DATA_REQUESTS {
            self.pre_cancelled.insert(id);
        }
        None
    }

    /// A result is sent only if its task still owns the request id. A cancel
    /// racing with a completed task therefore cannot produce a second reply.
    fn complete(&mut self, id: u64) -> bool {
        self.entries.remove(&id).is_some()
    }

    fn cancel_all(&mut self) -> Vec<String> {
        let mut slots = Vec::with_capacity(self.entries.len());
        for (_, entry) in self.entries.drain() {
            entry.handle.abort();
            slots.push(entry.git_slot);
        }
        self.pre_cancelled.clear();
        slots
    }
}

fn spawn_remote_lane_scheduler(
    cap: usize,
) -> (
    mpsc::Sender<crate::types::RemotePtyEvent>,
    mpsc::Sender<crate::types::RemotePtyEvent>,
    mpsc::Receiver<ScheduledPtyEvent>,
) {
    let (active_tx, mut active_rx) = mpsc::channel(cap);
    let (background_tx, mut background_rx) = mpsc::channel(cap);
    // One slot is the contract: at most one already-selected background frame
    // can precede active traffic; queued background backlog never can.
    let (scheduled_tx, scheduled_rx) = mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            // Reserve output capacity BEFORE selecting a lane. Otherwise a
            // second low frame can be selected and block behind the one-slot
            // queue, making a newly-arrived active frame wait behind two lows.
            let Ok(permit) = scheduled_tx.reserve().await else {
                return;
            };
            if let Ok(event) = active_rx.try_recv() {
                permit.send((true, event));
                continue;
            }
            let next = tokio::select! {
                biased;
                event = active_rx.recv() => event.map(|e| (true, e)),
                event = background_rx.recv() => event.map(|e| (false, e)),
            };
            let Some(event) = next else { return };
            permit.send(event);
        }
    });
    (active_tx, background_tx, scheduled_rx)
}

/// 桌面 LAN 远控宿主：包装 `AppState` + 鉴权 + 静态服务配置，供共享 `server_app` 驱动。
pub struct DesktopHost {
    pub state: AppState,
    pub auth: Arc<RemoteAuth>,
    pub port: u16,
    pub lan_ip: String,
    pub machine_name: String,
    pub serve_cfg: UaServeConfig,
    pub tls_enabled: bool,
}

impl HostMeta for DesktopHost {
    fn port(&self) -> u16 {
        self.port
    }
    fn lan_ip(&self) -> String {
        self.lan_ip.clone()
    }
    fn machine_name(&self) -> String {
        self.machine_name.clone()
    }
    fn remote_enabled(&self) -> Arc<AtomicBool> {
        self.state.remote_enabled.clone()
    }
    fn tls_enabled(&self) -> bool {
        self.tls_enabled
    }
    fn serve_cfg(&self) -> UaServeConfig {
        self.serve_cfg.clone()
    }
}

/// On a freshly-tripped hard-cap ban, add the offender to the persistent
/// blacklist so it stays barred across restarts and shows in the desktop panel.
/// （逐字搬自 server.rs::auto_blacklist_on_ban，改取 `&AppState`。）
fn auto_blacklist_on_ban(state: &AppState, ip: &str, device_id: &str) {
    let added_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let label = if !device_id.is_empty() {
        let short: String = device_id.chars().take(8).collect();
        format!("auto:{short}")
    } else {
        format!("auto:{ip}")
    };
    state.remote_blacklist.add(crate::state::BlacklistEntry {
        id: Uuid::new_v4().to_string(),
        device_id: (!device_id.is_empty()).then(|| device_id.to_string()),
        ip: Some(ip.to_string()),
        label,
        added_at,
    });
    tracing::warn!(
        target: "ridge::remote",
        ip, device = %device_id,
        "TOTP brute-force hard cap reached — device/IP auto-blacklisted"
    );
}

/// Apply the brute-force throttle + blacklist for one TOTP verify attempt.
/// （逐字搬自 server.rs::pre_verify_gate。）
fn pre_verify_gate(state: &AppState, ip: &str, device_id: &str) -> Result<(), ()> {
    if state.remote_blacklist.is_blocked(device_id, ip) {
        tracing::info!(target: "ridge::remote", ip, device = %device_id, "verify rejected: blacklisted");
        return Err(());
    }
    match state.remote_verify_throttle.check(ip, device_id) {
        ThrottleDecision::Allow => Ok(()),
        ThrottleDecision::Backoff { retry_after } => {
            tracing::info!(target: "ridge::remote", ip, device = %device_id, retry_s = retry_after.as_secs(), "verify rejected: backoff");
            Err(())
        }
        ThrottleDecision::Banned { retry_after, .. } => {
            tracing::warn!(target: "ridge::remote", ip, device = %device_id, retry_s = retry_after.as_secs(), "verify rejected: temp-banned");
            Err(())
        }
        ThrottleDecision::GlobalLimited => {
            tracing::warn!(target: "ridge::remote", ip, "verify rejected: global rate limit");
            Err(())
        }
    }
}

/// Record the outcome of a TOTP verify attempt against the throttle.
/// （逐字搬自 server.rs::post_verify_record。）
fn post_verify_record(state: &AppState, ip: &str, device_id: &str, valid: bool) {
    if valid {
        state.remote_verify_throttle.record_success(ip, device_id);
    } else {
        let fresh_ban = state.remote_verify_throttle.record_failure(ip, device_id);
        if fresh_ban {
            auto_blacklist_on_ban(state, ip, device_id);
        }
    }
}

impl HostAuth for DesktopHost {
    fn verify_code(&self, code: &str) -> bool {
        self.auth.verify(code)
    }
    fn is_blacklisted(&self, device_id: &str, ip: &str) -> bool {
        self.state.remote_blacklist.is_blocked(device_id, ip)
    }
    fn pre_verify_gate(&self, ip: &str, device_id: &str) -> Result<(), ()> {
        pre_verify_gate(&self.state, ip, device_id)
    }
    fn post_verify_record(&self, ip: &str, device_id: &str, valid: bool) {
        post_verify_record(&self.state, ip, device_id, valid)
    }
    fn create_session_token(&self, device_id: &str, ip: &str) -> String {
        self.state
            .remote_session_store
            .create_session_bound(device_id, ip)
    }
    fn validate_token(&self, token: &str) -> bool {
        self.state.remote_session_store.validate_token(token)
    }
    fn validate_token_bound(&self, token: &str, device_id: &str, ip: &str) -> bool {
        self.state
            .remote_session_store
            .validate_token_bound(token, device_id, ip)
    }
    fn validate_token_device_strict(&self, token: &str, device_id: &str, ip: &str) -> bool {
        self.state
            .remote_session_store
            .validate_token_device_strict(token, device_id, ip)
    }
}

impl WorkspaceProvider for DesktopHost {
    fn list_workspaces_json(&self) -> serde_json::Value {
        let state = &self.state;
        let order = state.workspace_order.read();
        let names = state.workspace_names.read();
        let map = state.workspaces.read();
        let active = *state.active_workspace.read();
        let workspaces: Vec<serde_json::Value> = order
            .iter()
            .map(|id| {
                let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
                serde_json::json!({
                    "id": id.to_string(),
                    "name": names.get(id).cloned().unwrap_or_else(|| format!("工作区 {}", display_seq)),
                    "displaySeq": display_seq,
                    "active": *id == active,
                })
            })
            .collect();
        serde_json::json!({ "workspaces": workspaces })
    }

    fn switch_workspace(&self, workspace_id: &str) -> Result<serde_json::Value, HostError> {
        let state = &self.state;
        let id = Uuid::parse_str(workspace_id)
            .map_err(|_| HostError::BadRequest("invalid workspace id".to_string()))?;
        let exists = state.workspaces.read().contains_key(&id);
        if !exists {
            return Err(HostError::NotFound("workspace not found".to_string()));
        }
        *state.active_workspace.write() = id;
        Ok(serde_json::json!({ "success": true, "workspaceId": id.to_string() }))
    }

    fn create_workspace(&self, name: Option<String>) -> Result<serde_json::Value, HostError> {
        use std::collections::HashMap;
        let state = &self.state;
        let id = Uuid::new_v4();
        let seq = state.allocate_workspace_seq();
        {
            let mut map = state.workspaces.write();
            map.insert(
                id,
                crate::state::Workspace {
                    pane_tree: crate::engine::pane_tree::PaneTree::new(),
                    terminals: HashMap::new(),
                    teammate_tmux_pane_cursor: 0,
                    teammate_pane_titles: HashMap::new(),
                    pane_sizes: HashMap::new(),
                    last_pane_index: None,
                    created_at: std::time::SystemTime::now(),
                    teammate_pane_states: HashMap::new(),
                    teammate_agent_pane_map: HashMap::new(),
                    teammate_owned_panes: std::collections::HashSet::new(),
                    associated_file_path: None,
                    pending_spawns: HashMap::new(),
                    pty_generation: HashMap::new(),
                    teammate_metrics: crate::state::TeammateMetrics::default(),
                    display_seq: seq,
                },
            );
        }
        state.workspace_order.write().push(id);
        *state.active_workspace.write() = id;
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            state.workspace_names.write().insert(id, name);
        }
        Ok(serde_json::json!({ "success": true, "workspaceId": id.to_string() }))
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<serde_json::Value, HostError> {
        // A1 同源化（iteration 10）：此前第三副本**漏发** WorkspacesChanged/
        // WorkspaceListChanged——LAN 端关区后他端列表不更新。委托唯一实现一并修复。
        let id = Uuid::parse_str(workspace_id)
            .map_err(|_| HostError::BadRequest("invalid workspace id".to_string()))?;
        crate::commands::workspace::close_workspace_core(&self.state, id)
            .map_err(HostError::BadRequest)?;
        Ok(serde_json::json!({ "success": true }))
    }

    fn allowed_file_roots(&self) -> Vec<PathBuf> {
        let state = &self.state;
        let mut roots: Vec<PathBuf> = Vec::new();
        {
            let map = state.workspaces.read();
            for ws in map.values() {
                for node in ws.pane_tree.panes.values() {
                    if let Some(cwd) = node.cwd.as_ref() {
                        roots.push(cwd.clone());
                    }
                }
            }
        }
        if let Some(proj) = state.current_project.read().clone() {
            roots.push(proj);
        }
        roots
    }
}

impl RemoteHost for DesktopHost {
    fn serve_websocket(
        self: Arc<Self>,
        socket: WebSocket,
        conn: WsConn,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            // WS 升级计时起点（原 ws_handler 透传，现于本钩子起点采集）。
            let upgrade_start = Instant::now();
            handle_ws(
                socket,
                self.state.clone(),
                conn.remote_addr,
                conn.device_id,
                conn.token,
                upgrade_start,
            )
            .await;
        })
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 以下为从 src-tauri/src/remote/server.rs 逐字搬入的每连接 WS 会话逻辑
// （handle_ws / apply_pane_resize / build_remote_pane_list / dispatch_* /
//  negotiate_hello / JSON-RPC 常量与辅助 / PaneSnapshotFrame）。
// 仅将 `ctx: RemoteCtx` 收窄为 `state: AppState`（本层不再需要 serve/auth 配置）。
// ════════════════════════════════════════════════════════════════════════════

fn build_remote_pane_list(ws: &crate::state::Workspace) -> Vec<serde_json::Value> {
    let mut list: Vec<serde_json::Value> = ws
        .pane_tree
        .get_all_leaves()
        .into_iter()
        .map(|pane_id| {
            let node = ws.pane_tree.panes.get(&pane_id);
            let title = ws
                .terminals
                .get(&pane_id)
                .and_then(|h| h.parser.lock().title())
                .filter(|t| !t.trim().is_empty())
                .or_else(|| ws.teammate_pane_titles.get(&pane_id).cloned())
                .or_else(|| node.and_then(|n| n.shell_kind.clone()))
                .or_else(|| {
                    ws.pending_spawns
                        .contains_key(&pane_id)
                        .then(|| "pending...".to_string())
                })
                .unwrap_or_else(|| "terminal".to_string());
            let agent_id = ws
                .teammate_agent_pane_map
                .iter()
                .find_map(|(agent, owner)| (*owner == pane_id).then(|| agent.clone()));
            let agent_state = ws.teammate_pane_states.get(&pane_id).map(|state| match state {
                crate::state::PaneState::Idle => "idle",
                crate::state::PaneState::Busy => "busy",
                crate::state::PaneState::Starting => "starting",
            });
            serde_json::json!({
                "id": pane_id.to_string(),
                "title": title,
                "cwd": node
                    .and_then(|n| n.cwd.as_ref().map(|p| p.to_string_lossy().to_string()))
                    .unwrap_or_default(),
                // iter-61：agent 标记态（远端工作区弹层的标记按钮据此高亮/切换）。
                "isAgent": matches!(
                    ws.teammate_pane_states.get(&pane_id),
                    Some(crate::state::PaneState::Busy)
                ),
                "agentState": agent_state,
                "agentId": agent_id,
            })
        })
        .collect();
    list.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    list
}

/// Resize a pane's PTY and canonical parser, broadcast the resulting
/// delta frame to the desktop (via the pane delta channel), and send
/// a PtyResized event to every remote subscriber so they can resize
/// their own wasm kernel. This is the shared path for the remote
/// "refresh-pane" / "claim-pane" commands.
fn apply_pane_resize(
    state: &AppState,
    ws_id: Uuid,
    pane_id: Uuid,
    rows: u16,
    cols: u16,
    pixel_width: u16,
    pixel_height: u16,
) {
    let rows = rows.max(1).min(500);
    let cols = cols.max(1).min(500);
    let frame_bytes = {
        let map = state.workspaces.read();
        let Some(ws) = map.get(&ws_id) else { return };
        let Some(handle) = ws.terminals.get(&pane_id) else {
            return;
        };
        let _ = handle.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
        });
        handle.delta_mode.store(true, Ordering::Release);
        let frame = {
            let mut p = handle.parser.lock();
            p.resize(rows, cols)
        };
        ridge_term::term::delta::encode_frame(&frame).ok()
    };
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&ws_id) {
            ws.pane_sizes.insert(pane_id, (rows, cols));
        }
    }
    let Some(bytes) = frame_bytes else { return };
    // Desktop viewer (if attached via a delta channel).
    if let Some(sender) = state.get_pane_delta_channel(ws_id, pane_id) {
        sender(bytes.clone());
    }
    // All remote viewers receive a PtyResized event so their wasm
    // kernel can call kernel.resize() for reflow.
    state.broadcast_remote_event(
        ws_id,
        pane_id,
        crate::types::RemotePtyEvent::PtyResized {
            workspace_id: ws_id,
            pane_id,
            rows,
            cols,
        },
    );
}

/// Encode the resize acknowledgement sent to every subscribed remote viewer.
/// Keep `workspaceId` on the wire: the browser transport rejects a resize event
/// without it to prevent a stale pane from another workspace mutating its grid.
fn pty_resized_message(workspace_id: Uuid, pane_id: Uuid, rows: u16, cols: u16) -> String {
    serde_json::json!({
        "type": "pty-resized",
        "workspaceId": workspace_id.to_string(),
        "paneId": pane_id.to_string(),
        "rows": rows,
        "cols": cols,
    })
    .to_string()
}

/// Notify remote viewers after the invoke-request resize path. Legacy
/// `claim-pane`/`refresh-pane` already goes through `apply_pane_resize`, while
/// the shared RPC scheduler uses `resize_pane`; both paths must produce the
/// same `pty-resized` contract.
fn broadcast_invoke_resize(
    state: &AppState,
    workspace_id: &str,
    pane_id: &str,
    rows: u16,
    cols: u16,
) {
    let Ok(workspace_id) = Uuid::parse_str(workspace_id) else {
        return;
    };
    let Ok(pane_id) = Uuid::parse_str(pane_id) else {
        return;
    };
    let exists = state
        .workspaces
        .read()
        .get(&workspace_id)
        .is_some_and(|workspace| workspace.terminals.contains_key(&pane_id));
    if !exists {
        return;
    }
    state.broadcast_remote_event(
        workspace_id,
        pane_id,
        crate::types::RemotePtyEvent::PtyResized {
            workspace_id,
            pane_id,
            rows: rows.max(1).min(500),
            cols: cols.max(1).min(500),
        },
    );
}

async fn handle_ws(
    socket: WebSocket,
    state: AppState,
    remote_addr: String,
    device_id: String,
    token: Option<String>,
    // §perf (B方案 三段埋点): WS 升级握手开始的时刻，由 ws_handler move 进 on_upgrade
    // 闭包后透传进来，用于在本任务开头打印 upgrade 段耗时。
    upgrade_start: Instant,
) {
    use futures::{SinkExt, StreamExt};
    let (mut socket_tx, mut ws_rx) = socket.split();
    // The reader never owns the WebSocket sink. A slow client therefore cannot
    // suspend stdin/control handling while a scrollback frame is in flight.
    // High is control + active raw; low is background raw + scrollback. The
    // writer re-checks high before every low frame.
    let writer_cap = ridge_remote::pane::RAW_CHAN_CAP;
    let (ws_tx, mut high_tx_rx) = mpsc::channel::<Message>(writer_cap);
    let (low_tx, mut low_tx_rx) = mpsc::channel::<Message>(writer_cap);
    tokio::spawn(async move {
        loop {
            let next = tokio::select! {
                biased;
                message = high_tx_rx.recv() => message,
                message = low_tx_rx.recv() => message,
            };
            let Some(message) = next else { return };
            if socket_tx.send(message).await.is_err() { return; }
        }
    });

    // Register this client in the remote client registry so the desktop
    // RemotePanel can list, disconnect, or blacklist it.
    // §M-2: snapshot the auth identity BEFORE `register` consumes it, so the
    // periodic health check can re-validate token TTL + blacklist mid-session.
    let recheck_addr = remote_addr.clone();
    let recheck_device = device_id.clone();
    let recheck_token = token.clone();
    let (client_id, kill_flag) = state.remote_client_registry.register(
        remote_addr,
        String::new(), // user-agent not available from axum WS directly
        device_id,
        token,
    );
    tracing::info!(target: "ridge::remote", client_id, "WebSocket client connected");
    // §perf (B方案): upgrade 段 = WS 升级握手到本任务真正开始执行的耗时。
    tracing::info!(target: "ridge::remote::perf", client_id, elapsed_ms = upgrade_start.elapsed().as_millis() as u64, "ws_upgrade");

    // Per-client active/background lanes. The one-slot merge means a low frame
    // can be in flight, but a low backlog can never sit ahead of a new active frame.
    let cap = ridge_remote::pane::RAW_CHAN_CAP;
    let (active_raw_tx, background_raw_tx, mut raw_rx) =
        spawn_remote_lane_scheduler(cap);
    let sub_id = RemoteSubId::next();

    // Gap and throttle state is per pane; one noisy background pane must never
    // dirty or throttle the foreground pane.
    let mut desync_by_pane:
        std::collections::HashMap<(Uuid, Uuid), Arc<AtomicBool>> =
        std::collections::HashMap::new();
    // §resync-throttle: a resync replays up to 64 KiB of scrollback, so under a
    // sustained-overload feedback loop (slow client → drops → resync → slower)
    // we cap it to at most once per interval. The desync flag is only CONSUMED
    // when we actually resync — if throttled, it stays set so a later frame
    // (after the interval) performs the recovery instead of losing the signal.
    let mut last_resync_by_pane:
        std::collections::HashMap<(Uuid, Uuid), Instant> =
        std::collections::HashMap::new();
    const RESYNC_MIN_INTERVAL: Duration = ridge_remote::pane::RESYNC_MIN_INTERVAL;

    // §rate-limit: per-connection token bucket for `data-request`. An
    // authenticated remote already has shell access, so this is an anti-abuse
    // / anti-DoS guard (scripted bulk FS/git calls), not an authz boundary.
    let mut dr_window_start = Instant::now();
    let mut dr_count: u32 = 0;
    const DR_WINDOW: Duration = Duration::from_secs(5);
    const DR_MAX_PER_WINDOW: u32 = 120;

    // Data requests are independent tasks so one slow Git/file operation
    // cannot block PTY/control traffic. The registry supplies cancellation and
    // a hard in-flight cap, preventing an unbounded remote queue.
    let mut data_requests = DataRequestRegistry::new();
    let (data_result_tx, mut data_result_rx) =
        mpsc::channel::<(u64, serde_json::Value)>(MAX_IN_FLIGHT_DATA_REQUESTS);

    // Invoke requests (legacy and native JSON-RPC) use the same bounded,
    // cancellable task ownership. Results return through this channel so the
    // reader loop can continue receiving cancellation and control frames while
    // a command is running.
    let mut invoke_requests = InvokeRequestRegistry::new();
    let (invoke_result_tx, mut invoke_result_rx) =
        mpsc::channel::<InvokeResult>(MAX_IN_FLIGHT_INVOKE_REQUESTS);

    // current_pane owns Files/Git/Search cwd and active QoS only. Every visited
    // pane remains in subscribed_panes across ordinary pane/workspace switches.
    let mut current_pane: Option<(Uuid, Uuid)> = None;
    let mut subscribed_panes: std::collections::HashSet<(Uuid, Uuid)> =
        std::collections::HashSet::new();

    // Client-reported viewport grid dimensions, updated by the `resize` WS
    // message. Used for the first-connect auto-claim and the refresh button.
    let mut mobile_rows: u16 = 24;
    let mut mobile_cols: u16 = 80;

    // Subscribe to structural change broadcasts (pane/workspace add/close/rename)
    // so this client can push updated lists to the remote frontend without polling.
    let mut structural_rx = state.remote_structural_tx.subscribe();
    // §web-remote: generic host → desktop-browser event relay (fs-changed, …).
    // The mobile SPA ignores `{type:'event'}` frames, so this is harmless to it.
    let mut ui_event_rx = state.remote_ui_event_tx.subscribe();
    // §own-active: there is NO connect-time auto-claim. A remote endpoint resizes
    // the shared PTY only when it becomes the active owner — i.e. on a genuine
    // user interaction (`claim-pane`) or the explicit refresh button
    // (`refresh-pane`). Merely connecting / changing viewport records the size
    // here but never stomps the PTY the desktop is using.

    // Initial handshake.
    let ws_connect_start = std::time::Instant::now();
    // §perf (B方案): first-byte 段日志的一次性 once-guard（仅用其 None/Some 状态打一次）。
    let mut first_pty_bytes_at: Option<Instant> = None;
    let welcome = serde_json::json!({"type": "hello","version": 1,"protocol": "ridge-remote-ws"});
    if ws_tx
        .send(Message::Text(welcome.to_string()))
        .await
        .is_err()
    {
        state.remote_client_registry.unregister(client_id);
        return;
    }

    // §theme: push the desktop's active theme so the remote chrome and the
    // terminal kernel follow it (passive — a snapshot taken at connect). Best
    // effort: on any failure the client keeps its own CSS-variable fallbacks.
    if let Some(entry) = crate::commands::theme::active_theme_entry_no_handle() {
        let theme_msg = serde_json::json!({
            "type": "theme",
            "id": entry.id,
            "themeType": entry.theme_type,
            "colors": entry.colors,
        });
        let _ = ws_tx.send(Message::Text(theme_msg.to_string())).await;
    }
    // §perf (B方案): connect 段 = hello + theme 两帧都发完的"首帧 ready"时刻（theme 为
    // best-effort，缺省时此处即 hello 之后，天然取到最迟者）。
    tracing::info!(target: "ridge::remote::perf", client_id, elapsed_ms = ws_connect_start.elapsed().as_millis() as u64, "ws_connected_first_frame");

    // §state-sep: per-client active workspace. Seeded once from the global
    // active workspace at connect, then owned by THIS client. Switching /
    // creating / closing workspaces from a remote no longer rewrites the
    // shared `active_workspace` (which would drag the desktop and every other
    // client along) — only this `active_ws_id` moves. All readers below
    // (list-panes / subscribe / stdin / resize / output+delta filters) use it.
    let mut active_ws_id = state.active_workspace_id();

    // §web-remote global-workspace mode. The desktop-UI-in-browser client is a
    // second *peer desktop*: it switches workspaces through the real global
    // `switch_workspace` command (invoke-request), not the per-client WS
    // `switch-workspace` message. So when this flag is set (the client sends
    // `use-global-workspace` right after connect), `active_ws_id` is kept in sync
    // with the GLOBAL active workspace at the top of every loop iteration — which
    // runs before any incoming message/event is handled (incl. the subscribe-pane
    // that follows a switch), so all readers below see the right workspace.
    // Mobile clients never set it and keep their independent per-client view.
    let mut use_global_ws = false;

    // §S3 $/cancel registry (JSON-RPC leg only). Invokes on a single connection
    // are processed serially inside this loop, so there is no concurrent in-flight
    // request to abort mid-flight on the same socket. This set records ids the
    // client asked to cancel; a request whose id was pre-cancelled (a client that
    // pipelines `$/cancel` ahead of, or racing, its request) is short-circuited
    // with a "cancelled" error and never runs the backing command. Long tasks that
    // cannot be interrupted simply run to completion — the guard guarantees we
    // never crash and never send a stale result for a cancelled id. Bounded so a
    // hostile client cannot grow it without limit.
    // Periodic health check (1s): tears down an ALREADY-OPEN connection when
    // remote control is toggled off, this client is force-disconnected
    // (kill_flag), its device/IP gets blacklisted, or — for token sessions — the
    // session token expires (TTL) or is revoked. SECURITY (audit M-2): the
    // handshake check alone never expires a live session, so without this an
    // issued token would outlive its TTL on an open socket and a fresh blacklist
    // entry wouldn't kick an already-connected client.
    let mut health_interval = tokio::time::interval(std::time::Duration::from_secs(1));
    health_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Message loop: forward PTY output to WS client, relay keystrokes back.
    loop {
        // §web-remote: keep `active_ws_id` mirrored to the global active workspace
        // for desktop-browser peers. Runs whenever the loop iterates (i.e. before
        // handling each message/event), so a global switch via invoke-request is
        // reflected before the browser's following subscribe-pane is processed.
        if use_global_ws {
            let g = state.active_workspace_id();
            if g != active_ws_id {
                // Drop the stale subscriptions; the browser re-subscribes the new
                // workspace's panes on its own re-render. Their bytes are filtered
                // by `workspace_id == active_ws_id`, so nothing leaks across.
                for (ws, p) in subscribed_panes.drain() {
                    state.unregister_remote_sub(ws, p, sub_id);
                }
                desync_by_pane.clear();
                last_resync_by_pane.clear();
                current_pane = None;
                active_ws_id = g;
            }
        }
        tokio::select! {
                    msg = ws_rx.next() => {
                        let Some(Ok(Message::Text(text))) = msg else {
                            break;
                        };
                        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };

                        // §S3 JSON-RPC 2.0 leg (additive). A frame is JSON-RPC iff
                        // `jsonrpc == "2.0"`; otherwise it is a legacy control frame and
                        // falls straight through to the `match parsed["type"]` below
                        // (old clients are byte-for-byte unchanged). JSON-RPC requests and
                        // `$/`-methods are fully handled here and `continue`; JSON-RPC
                        // notifications for ordinary control methods are normalised into
                        // the legacy `{type, …params}` shape and DELIBERATELY fall through
                        // so they reuse the exact same handlers (no duplicated logic).
                        let parsed = if parsed.get("jsonrpc").and_then(|v| v.as_str()) == Some("2.0") {
                            let method = parsed.get("method").and_then(|m| m.as_str()).map(String::from);
                            let has_id = parsed.get("id").map(|i| !i.is_null()).unwrap_or(false);
                            let empty = serde_json::json!({});
                            let params = parsed.get("params").unwrap_or(&empty).clone();

                            match (method.as_deref(), has_id) {
                                // ── D9 version + capability handshake ──
                                (Some("$/hello"), _) => {
                                    let reply = negotiate_hello(&params);
                                    let _ = ws_tx.send(Message::Text(reply.to_string())).await;
                                    continue;
                                }
                                // ── $/cancel: register the target id (notification, no id) ──
                                (Some("$/cancel"), _) => {
                                    if let Some(target) = params.get("id") {
                                        let key = InvokeRequestKey::jsonrpc(target);
                                        let legacy_key = target
                                            .as_u64()
                                            .map(InvokeRequestKey::Legacy);
                                        let keys = legacy_key
                                            .as_ref()
                                            .map(|legacy| vec![key.clone(), legacy.clone()])
                                            .unwrap_or_else(|| vec![key.clone()]);
                                        if let Some(slot) = invoke_requests.cancel_keys(&keys) {
                                            ridge_core::commands::git::cancel_git_slot(&slot);
                                        }
                                        tracing::debug!(target: "ridge::remote", id = %target, "received $/cancel");
                                    }
                                    continue;
                                }
                                // ── JSON-RPC request (has id) → dispatch + JSON-RPC reply ──
                                (Some(m), true) => {
                                    let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                    let key = InvokeRequestKey::jsonrpc(&id);

                                    // §rate-limit: shared token bucket with the legacy leg.
                                    if dr_window_start.elapsed() >= DR_WINDOW {
                                        dr_window_start = Instant::now();
                                        dr_count = 0;
                                    }
                                    dr_count += 1;
                                    if dr_count > DR_MAX_PER_WINDOW {
                                        tracing::warn!(target: "ridge::remote", client_id, cmd = %m, "invoke (json-rpc) rate limit exceeded; rejecting");
                                        let err = serde_json::json!({
                                            "code": JSON_RPC_INTERNAL_ERROR,
                                            "message": "rate limited: too many invoke requests",
                                            "data": { "kind": "rate_limited" },
                                        });
                                        let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                        continue;
                                    }

                                    // §cancel: a request whose id was already cancelled never runs.
                                    if invoke_requests.take_pre_cancelled(&key) {
                                        let err = serde_json::json!({
                                            "code": JSON_RPC_INTERNAL_ERROR,
                                            "message": "request cancelled",
                                            "data": { "kind": "cancelled" },
                                        });
                                        let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                        continue;
                                    }

                                    if invoke_requests.len() >= MAX_IN_FLIGHT_INVOKE_REQUESTS
                                        || invoke_requests.contains(&key)
                                    {
                                        let err = serde_json::json!({
                                            "code": JSON_RPC_INTERNAL_ERROR,
                                            "message": "too many invoke requests",
                                            "data": { "kind": "queue_full" },
                                        });
                                        let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                        continue;
                                    }

                                    let result_tx = invoke_result_tx.clone();
                                    let task_method = m.to_string();
                                    let task_params = params.clone();
                                    let task_state = state.clone();
                                    let task_id = id.clone();
                                    let task_key = key.clone();
                                    let git_slot = format!("remote:{client_id}:invoke:{}", key.slot_name());
                                    let task_slot = git_slot.clone();
                                    let handle = tokio::spawn(async move {
                                        let reply = ridge_core::commands::git::with_git_request_slot(
                                            task_slot,
                                            dispatch_invoke_jsonrpc(
                                                &task_method,
                                                &task_params,
                                                &task_state,
                                            ),
                                        )
                                        .await
                                        .map_or_else(
                                            |error| jsonrpc_error(&task_id, error),
                                            |result| jsonrpc_result(&task_id, result),
                                        );
                                        let _ = result_tx
                                            .send(InvokeResult::JsonRpc {
                                                key: task_key,
                                                reply,
                                            })
                                            .await;
                                    });
                                    let _ = invoke_requests.insert(
                                        key,
                                        PendingInvokeRequest { handle, git_slot },
                                    );
                                    // §cancel: if the client cancelled while the (serial) dispatch
                                    // was running, drop the stale result instead of sending it.
                                    continue;
                                }
                                // ── JSON-RPC notification (no id) → reuse legacy handlers ──
                                (Some(m), false) => {
                                    // Normalise `{jsonrpc, method, params}` → `{type: method,
                                    // …params}` so the legacy `match` below handles it once.
                                    let mut flat = match params {
                                        serde_json::Value::Object(map) => map,
                                        _ => serde_json::Map::new(),
                                    };
                                    flat.insert("type".to_string(), serde_json::json!(m));
                                    serde_json::Value::Object(flat)
                                }
                                // Malformed JSON-RPC (no method): reply error if it had an id.
                                (None, _) => {
                                    if has_id {
                                        let id = parsed.get("id").cloned().unwrap_or(serde_json::Value::Null);
                                        let err = serde_json::json!({
                                            "code": JSON_RPC_INVALID_REQUEST,
                                            "message": "missing method",
                                            "data": { "kind": "invalid_request" },
                                        });
                                        let _ = ws_tx.send(Message::Text(jsonrpc_error(&id, err).to_string())).await;
                                    }
                                    continue;
                                }
                            }
                        } else {
                            parsed
                        };

                        let _ = match parsed["type"].as_str() {
                            Some("ping") => {
                                ws_tx.send(Message::Text(serde_json::json!({"type":"pong"}).to_string())).await
                            }
                            Some("use-global-workspace") => {
                                // §web-remote: desktop-browser peer opts into global
                                // workspace semantics (see use_global_ws above). Seed
                                // immediately so the first list/subscribe is correct.
                                use_global_ws = true;
                                active_ws_id = state.active_workspace_id();
                                Ok(())
                            }
                            Some("list-panes") => {
                                // Self-heal: drop PTYs/pending no longer in the tree before listing.
                                crate::commands::terminal::reap_orphan_panes_all(&state).await;
                                let pane_list = {
                                    let workspaces = state.workspaces.read();
                                    let Some(ws) = workspaces.get(&active_ws_id) else {
                                        drop(workspaces);
                                        continue;
                                    };
                                    build_remote_pane_list(ws)
                                };
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type":"panes",
                                    "workspaceId": active_ws_id.to_string(),
                                    "panes":pane_list
                                }).to_string())).await
                            }
                            Some("subscribe-pane") => {
                                let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                                if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                                    let Some(target_ws) = parsed["workspaceId"].as_str()
                                        .and_then(|s| Uuid::parse_str(s).ok()) else {
                                        let _ = ws_tx.send(Message::Text(serde_json::json!({
                                            "type": "error", "code": "MISSING_WORKSPACE",
                                            "message": "subscribe-pane requires workspaceId"
                                        }).to_string())).await;
                                        continue;
                                    };
                                    let pane_exists = state.workspaces.read()
                                        .get(&target_ws)
                                        .is_some_and(|ws| ws.terminals.contains_key(&pane_id));
                                    if !pane_exists { continue; }
                                    let new_key = (target_ws, pane_id);
                                    let wants_active = parsed["active"].as_bool().unwrap_or(!use_global_ws);
                                    if wants_active {
                                        if let Some(old_key) = current_pane {
                                            if old_key != new_key && subscribed_panes.contains(&old_key) {
                                                state.unregister_remote_sub(old_key.0, old_key.1, sub_id);
                                                if let Some(flag) = desync_by_pane.get(&old_key) {
                                                    state.register_remote_sub(
                                                        old_key.0,
                                                        old_key.1,
                                                        RemotePaneSub {
                                                            id: sub_id,
                                                            raw_tx: background_raw_tx.clone(),
                                                            desync: Arc::clone(flag),
                                                        },
                                                    );
                                                }
                                            }
                                        }
                                        current_pane = Some(new_key);
                                    }
                                    let do_register = subscribed_panes.insert(new_key);
                                    if !do_register {
                                        // Promotion only swaps this pane onto the active lane;
                                        // its kernel/subscription/history remain intact.
                                        if wants_active {
                                            state.unregister_remote_sub(target_ws, pane_id, sub_id);
                                            if let Some(flag) = desync_by_pane.get(&new_key) {
                                                state.register_remote_sub(
                                                    target_ws,
                                                    pane_id,
                                                    RemotePaneSub {
                                                        id: sub_id,
                                                        raw_tx: active_raw_tx.clone(),
                                                        desync: Arc::clone(flag),
                                                    },
                                                );
                                            }
                                        }
                                        continue;
                                    }

                                    // Ensure the canonical parser is in delta mode so
                                    // the desktop frontend continues receiving deltas.
                                    {
                                        let workspaces = state.workspaces.read();
                                        if let Some(h) = workspaces
                                            .get(&target_ws)
                                            .and_then(|ws| ws.terminals.get(&pane_id))
                                        {
                                            h.delta_mode.store(true, Ordering::Release);
                                        }
                                    }

                                    let desync = Arc::new(AtomicBool::new(false));
                                    desync_by_pane.insert(new_key, Arc::clone(&desync));
                                    state.register_remote_sub(
                                        target_ws, pane_id,
                                        RemotePaneSub {
                                            id: sub_id,
                                            raw_tx: if wants_active {
                                                active_raw_tx.clone()
                                            } else {
                                                background_raw_tx.clone()
                                            },
                                            desync: desync.clone(),
                                        },
                                    );

                                    // §D10 接入点 (integration point) — S5 per-pane screen
                                    // buffer: emit a `PaneSnapshotFrame` (rendered screen +
                                    // locked size) HERE, before the raw scrollback below, so a
                                    // late/reconnecting controller repaints exact terminal state
                                    // (cursor/alt-screen/scroll-region) ahead of the live raw
                                    // stream. See the D10 SCAFFOLD block near `PaneSnapshotFrame`.
                                    // Today we ship raw scrollback only (a working precursor).
                                    //
                                    // §history-pull（2026-07-02，LAN 对齐 cloud）: push a
                                    // BOUNDED tail (via the seq-cursor scrollback store), not the
                                    // legacy full replay, so the client paints ~1.5 screens
                                    // immediately and pages OLDER history lazily on scroll-up
                                    // (`scrollback-before`). The tail rides the SAME 16-byte
                                    // pane-UUID-prefixed binary frame; a following
                                    // `scrollback-meta` JSON frame seeds the client's paging
                                    // cursor at the oldest byte currently shown.
                                    //
                                    // Ordering note: we register BEFORE snapshotting the
                                    // scrollback on purpose. This guarantees no GAP — every
                                    // chunk is either in this snapshot or delivered live (or,
                                    // in a sub-microsecond window, both → a harmless duplicate
                                    // that a vte repaint absorbs). The reverse order would
                                    // trade the benign dup for a dropped chunk, which is worse
                                    // for a mirror. True dedup would require coupling the PTY
                                    // reader's scrollback-append + fan-out under one lock — not
                                    // worth the hot-path cost.
                                    // §keep-alive resume vs full resync (fixes the RIS-vs-keep-alive
                                    // conflict). A controller that kept its mirror kernel alive across a
                                    // pane switch (mobile P4 keep-alive) resubscribes with `resume:true`
                                    // (and optionally `sinceSeq`), so the host must NOT send a
                                    // RIS-bearing resync — that would wipe the surviving kernel down to
                                    // the tail. Three cases:
                                    //   • sinceSeq present  → incremental replay of the gap since the
                                    //     controller's cursor (NO RIS) when contiguous; a gap/eviction
                                    //     falls back to a full RIS resync. (frontend activation pending)
                                    //   • resume, no cursor → live-only: send NO frame; the alive kernel
                                    //     keeps its full history and just resumes the live stream.
                                    //   • else (fresh / reload / desktop) → full RIS resync + preamble
                                    //     + scrollback (reattaches modes, repaints an empty mirror).
                                    let resume = parsed["resume"].as_bool().unwrap_or(false);
                                    let since_seq = parsed["sinceSeq"].as_u64();
                                    let cap = ridge_remote::pane::RESYNC_SCROLLBACK_LAN;
                                    let (chunk, incremental) = if let Some(cursor) = since_seq {
                                        let c = state.get_pty_scrollback_since(
                                            target_ws, pane_id, cursor, cap,
                                        );
                                        let contiguous = c.start_seq == cursor;
                                        (c, contiguous)
                                    } else {
                                        (state.get_pty_scrollback_tail(target_ws, pane_id, cap), false)
                                    };

                                    if incremental {
                                        // Resume gap replay: append only the bytes since the cursor,
                                        // NO RIS. Empty when the controller is already current.
                                        if !chunk.bytes.is_empty() {
                                            let frame = ridge_remote::pane::pane_frame(
                                                pane_id, chunk.bytes.as_bytes(),
                                            );
                                            let _ = low_tx.try_send(Message::Binary(frame.into()));
                                        }
                                    } else if resume {
                                        // Live-only resume: alive kernel keeps its history; send nothing
                                        // here and let the live stream continue. (No RIS, no scrollback.)
                                    } else if !chunk.bytes.is_empty() {
                                        // Fresh / gapped: RIS + active-mode preamble + scrollback.
                                        let (modes, alt) = state.get_pane_modes(target_ws, pane_id);
                                        let resync = ridge_remote::pane::pane_resync_frame(
                                            pane_id, chunk.bytes.as_bytes(), &modes, alt,
                                        );
                                        let _ = low_tx.try_send(Message::Binary(resync.into()));
                                    }
                                    // Seed/refresh the client's paging + resume cursor. `headSeq` is the
                                    // controller's next resume cursor; `incremental` tells it whether the
                                    // mirror was reset (repaint + reset paging anchor) or resumed (append,
                                    // keep anchor). Sent even for an empty tail so a fresh pane learns
                                    // it's at-oldest. A live-only resume skips it (nothing changed here).
                                    if !resume || since_seq.is_some() {
                                        let meta = serde_json::json!({
                                            "type": "scrollback-meta",
                                            "workspaceId": target_ws.to_string(),
                                            "paneId": pane_id.to_string(),
                                            "startSeq": chunk.start_seq,
                                            "atOldest": chunk.at_oldest,
                                            "headSeq": chunk.head_seq,
                                            "incremental": incremental,
                                        });
                                        let _ = ws_tx
                                            .send(Message::Text(meta.to_string()))
                                            .await;
                                    }
                                }
                                Ok(())
                            }
                            Some("scrollback-before") => {
                                // §history-pull lazy paging: the client scrolled near the top and
                                // wants the batch OLDER than what it currently shows (seq <
                                // beforeSeq). Reply with the raw bytes + the new oldest seq /
                                // at-oldest so the client advances its cursor and knows when to
                                // stop. `_reqId` is opaque — echo it back for the client's
                                // request/response matching. Always reply (never leave the client
                                // hanging): a bad pane id → empty bytes + atOldest:true so paging
                                // stops.
                                let req_id = parsed["_reqId"].clone();
                                let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                                let before_seq = parsed["beforeSeq"].as_u64().unwrap_or(0);
                                let max_bytes =
                                    parsed["maxBytes"].as_u64().unwrap_or(65536) as usize;
                                let target_ws = parsed["workspaceId"].as_str()
                                    .and_then(|s| Uuid::parse_str(s).ok());
                                let result = if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                                    if let Some(target_ws) = target_ws {
                                        let chunk = state.get_pty_scrollback_before(
                                            target_ws, pane_id, before_seq, max_bytes,
                                        );
                                        serde_json::json!({
                                            "type": "scrollback-before-result",
                                            "_reqId": req_id,
                                            "bytes": chunk.bytes,
                                            "startSeq": chunk.start_seq,
                                            "endSeq": chunk.end_seq,
                                            "atOldest": chunk.at_oldest,
                                        })
                                    } else {
                                        serde_json::json!({
                                            "type": "scrollback-before-result", "_reqId": req_id,
                                            "bytes": "", "startSeq": before_seq, "endSeq": before_seq,
                                            "atOldest": true, "error": "MISSING_WORKSPACE"
                                        })
                                    }
                                } else {
                                    serde_json::json!({
                                        "type": "scrollback-before-result",
                                        "_reqId": req_id,
                                        "bytes": "",
                                        "startSeq": before_seq,
                                        "endSeq": before_seq,
                                        "atOldest": true,
                                    })
                                };
                                ws_tx.send(Message::Text(result.to_string())).await
                            }
                            Some("current-project") => {
                                let path = state.current_project.read().clone()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "current-project",
                                    "path": path,
                                }).to_string())).await
                            }
                            Some("list-workspaces") => {
                                let ws_list = {
                                    let order = state.workspace_order.read();
                                    let names = state.workspace_names.read();
                                    let map = state.workspaces.read();
                                    // §state-sep: the `active` flag reflects THIS client's
                                    // selection, not the global one.
                                    let active = active_ws_id;
                                    order.iter().map(|id| {
                                        // §unify: unnamed workspaces fall back to "工作区 {display_seq}",
                                        // matching the desktop WorkspaceSidebar label.
                                        let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
                                        serde_json::json!({
                                            "id": id.to_string(),
                                            "name": names.get(id).cloned()
                                                .unwrap_or_else(|| format!("工作区 {}", display_seq)),
                                            "displaySeq": display_seq,
                                            "active": *id == active,
                                        })
                                    }).collect::<Vec<_>>()
                                };
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "workspaces",
                                    "workspaces": ws_list,
                                }).to_string())).await
                            }
                            Some("switch-workspace") => {
                                let id_str = parsed["workspaceId"].as_str().unwrap_or("");
                                let result = if let Ok(id) = Uuid::parse_str(id_str) {
                                    let exists = state.workspaces.read().contains_key(&id);
                                    if exists {
                                        // §state-sep: switch THIS client only — do not touch
                                        // the global active_workspace. Drop the current pane
                                        // subscription so the old workspace's stream stops; the
                                        // client re-subscribes after its next list-panes.
                                        active_ws_id = id;
                                        serde_json::json!({ "type": "switch-workspace-result", "success": true, "workspaceId": id.to_string() })
                                    } else {
                                        serde_json::json!({ "type": "switch-workspace-result", "success": false, "error": "workspace not found" })
                                    }
                                } else {
                                    serde_json::json!({ "type": "switch-workspace-result", "success": false, "error": "invalid workspace id" })
                                };
                                ws_tx.send(Message::Text(result.to_string())).await
                            }
                            Some("create-workspace") => {
                                let name = parsed["name"].as_str().and_then(|n| if n.is_empty() { None } else { Some(n.to_string()) });
                                let id = Uuid::new_v4();
                                let seq = state.allocate_workspace_seq();
                                {
                                    use std::collections::HashMap;
                                    let mut map = state.workspaces.write();
                                    map.insert(id, crate::state::Workspace {
                                        pane_tree: crate::engine::pane_tree::PaneTree::new(),
                                        terminals: HashMap::new(),
                                        teammate_tmux_pane_cursor: 0,
                                        teammate_pane_titles: HashMap::new(),
                                        pane_sizes: HashMap::new(),
                                        last_pane_index: None,
                                        created_at: std::time::SystemTime::now(),
                                        teammate_pane_states: HashMap::new(),
                                        teammate_agent_pane_map: HashMap::new(),
                                        teammate_owned_panes: std::collections::HashSet::new(),
                                        associated_file_path: None,
                                        pending_spawns: HashMap::new(),
                                        pty_generation: HashMap::new(),
                                        teammate_metrics: crate::state::TeammateMetrics::default(),
                                        display_seq: seq,
                                    });
                                }
                                state.workspace_order.write().push(id);
                                // §state-sep: the new workspace is shared data (visible to all),
                                // but only THIS client jumps to it. Other clients / the desktop
                                // stay on their own selection.
                                active_ws_id = id;
                                if let Some(ref n) = name {
                                    state.workspace_names.write().insert(id, n.clone());
                                }
                                let send = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "create-workspace-result",
                                    "success": true,
                                    "workspaceId": id.to_string(),
                                }).to_string())).await;
                                // Broadcast structural change to all remote clients and desktop.
                                let _ = state.remote_structural_tx.send(
                                    crate::types::RemoteStructuralEvent::WorkspacesChanged
                                );
                                let _ = state.event_tx.try_send(
                                    crate::types::GlobalEvent::WorkspaceListChanged
                                );
                                send
                            }
                            Some("close-workspace") => {
                                let id_str = parsed["workspaceId"].as_str().unwrap_or("");
                                let mut success = false;
                                let result = if let Ok(id) = Uuid::parse_str(id_str) {
                                    {
                                        let order = state.workspace_order.read();
                                        if order.len() <= 1 {
                                            serde_json::json!({ "type": "close-workspace-result", "success": false, "error": "cannot close last workspace" })
                                        } else {
                                            drop(order);
                                            state.workspaces.write().remove(&id);
                                            state.workspace_order.write().retain(|w| *w != id);
                                            state.workspace_names.write().remove(&id);
                                            // Closing a workspace destroys shared data: if the
                                            // DESKTOP (global active) was viewing it, move the
                                            // global off so the desktop doesn't point at a dead
                                            // workspace. This is unavoidable — you can't view a
                                            // workspace that no longer exists.
                                            if *state.active_workspace.read() == id {
                                                let first = state.workspace_order.read().first().cloned();
                                                if let Some(first_id) = first {
                                                    *state.active_workspace.write() = first_id;
                                                }
                                            }
                                            // §state-sep: if THIS client was on the closed
                                            // workspace, fall back to the first remaining one
                                            // (independently of the desktop / other clients).
                                            let closed: Vec<_> = subscribed_panes
                                                .iter()
                                                .copied()
                                                .filter(|(ws, _)| *ws == id)
                                                .collect();
                                            for (ws, pane) in closed {
                                                subscribed_panes.remove(&(ws, pane));
                                                desync_by_pane.remove(&(ws, pane));
                                                last_resync_by_pane.remove(&(ws, pane));
                                                state.unregister_remote_sub(ws, pane, sub_id);
                                            }
                                            if current_pane.is_some_and(|(ws, _)| ws == id) {
                                                current_pane = None;
                                            }
                                            if active_ws_id == id {
                                                if let Some(first_id) = state.workspace_order.read().first().cloned() {
                                                    active_ws_id = first_id;
                                                }
                                            }
                                            success = true;
                                            serde_json::json!({ "type": "close-workspace-result", "success": true })
                                        }
                                    }
                                } else {
                                    serde_json::json!({ "type": "close-workspace-result", "success": false, "error": "invalid workspace id" })
                                };
                                let send = ws_tx.send(Message::Text(result.to_string())).await;
                                if success {
                                    let _ = state.remote_structural_tx.send(
                                        crate::types::RemoteStructuralEvent::WorkspacesChanged
                                    );
                                    let _ = state.event_tx.try_send(
                                        crate::types::GlobalEvent::WorkspaceListChanged
                                    );
                                }
                                send
                            }
                            Some("stdin") => {
                                let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                                let data_str = parsed["data"].as_str().unwrap_or("");
                                if let (Ok(pane_id), false) =
                                    (Uuid::parse_str(pane_id_str), data_str.is_empty())
                                {
                                    let data = data_str.to_string();
                                    let writer = {
                                        let workspaces = state.workspaces.read();
                                        workspaces
                                            .get(&active_ws_id)
                                            .and_then(|ws| ws.terminals.get(&pane_id))
                                            .map(|handle| handle.writer.clone())
                                    };
                                    // Offload blocking ConPTY WriteFile to a
                                    // blocking task so it cannot freeze the WS
                                    // event loop (which would cascade into
                                    // RPC timeouts + reconnect storms).
                                    if let Some(writer) = writer {
                                        tokio::task::spawn_blocking(move || {
                                            let mut w = writer.lock();
                                            let _ = w.write_all(data.as_bytes());
                                            let _ = w.flush();
                                        });
                                    }
                                }
                                // no response needed
                                Ok(())
                            }
                            Some("resize") => {
                                // The client renders at the canonical PTY grid (driven by
                                // `pty-resized` from claim/refresh), so a viewport-only resize
                                // doesn't touch the shared PTY or the client kernel. We just
                                // record the clamped size as the fallback used by the next
                                // claim/refresh. The `.min(500)` is a defensive bound against a
                                // malformed viewport, not the anti-OOM guard it was when each
                                // sub owned a `rows × cols` parser.
                                let _pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                                let rows = parsed["rows"].as_u64().unwrap_or(mobile_rows as u64) as u16;
                                let cols = parsed["cols"].as_u64().unwrap_or(mobile_cols as u64) as u16;
                                mobile_rows = rows.max(1).min(500);
                                mobile_cols = cols.max(1).min(500);
                                Ok(())
                            }
                            // §own-active: this client becomes the active size owner. Both
                            // the implicit "I just interacted" claim (`claim-pane`) and the
                            // explicit refresh button (`refresh-pane`) resize the shared PTY +
                            // canonical parser to this client's viewport and broadcast a full
                            // repaint to every viewer (desktop included). Last interaction wins.
                            Some("refresh-pane") | Some("claim-pane") => {
                                let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                                let rows = parsed["rows"].as_u64().unwrap_or(mobile_rows as u64) as u16;
                                let cols = parsed["cols"].as_u64().unwrap_or(mobile_cols as u64) as u16;
                                let pixel_width = parsed["pixelWidth"].as_u64().unwrap_or(cols as u64 * 8) as u16;
                                let pixel_height = parsed["pixelHeight"].as_u64().unwrap_or(rows as u64 * 16) as u16;
                                mobile_rows = rows;
                                mobile_cols = cols;
                                if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                                    apply_pane_resize(&state, active_ws_id, pane_id, rows, cols, pixel_width, pixel_height);
                                }
                                Ok(())
                            }
        Some("create-pane") => {
                                // §6: create a terminal in THIS client's active workspace
                                // using the balanced-split chooser, then immediately activate
                                // it (Phase 2) at this client's viewport size. Remote clients
                                // can't call the front-end `activate_pane_pty` Tauri command,
                                // so without this the pane would sit in `pending_spawns`
                                // forever ("pending..."). Once live it streams to every viewer
                                // via the PTY fan-out on subscribe.
                                let shell = parsed["shell"].as_str()
                                    .and_then(|s| if s.is_empty() { None } else { Some(s.to_string()) });
                                let mut success = false;
                                let result = match crate::commands::pane::remote_create_pane(&state, active_ws_id, shell, None, None) {
                                    Ok(new_id) => match crate::commands::terminal::activate_pane_pty_state(
                                        &state, None, active_ws_id, new_id,
                                        Some(mobile_rows), Some(mobile_cols),
                                    ) {
                                        Ok(()) => {
                                            success = true;
                                            serde_json::json!({
                                                "type": "create-pane-result", "success": true, "paneId": new_id.to_string()
                                            })
                                        }
                                        Err(e) => serde_json::json!({
                                            "type": "create-pane-result", "success": false, "error": e.to_string()
                                        }),
                                    },
                                    Err(e) => serde_json::json!({
                                        "type": "create-pane-result", "success": false, "error": e.to_string()
                                    }),
                                };
                                let send = ws_tx.send(Message::Text(result.to_string())).await;
                                if success {
                                    let _ = state.remote_structural_tx.send(
                                        crate::types::RemoteStructuralEvent::PanesChanged { workspace_id: active_ws_id }
                                    );
                                    let _ = state.event_tx.try_send(
                                        crate::types::GlobalEvent::PaneTreeChanged { workspace_id: active_ws_id }
                                    );
                                }
                                send
                            }
                            Some("close-pane") => {
                                let pane_id_str = parsed["paneId"].as_str().unwrap_or("");
                                let mut success = false;
                                let result = match Uuid::parse_str(pane_id_str) {
                                    Ok(pane_id) => match crate::commands::pane::remote_close_pane(&state, active_ws_id, pane_id).await {
                                        Ok(()) => {
                                            success = true;
                                            serde_json::json!({ "type": "close-pane-result", "success": true })
                                        }
                                        Err(e) => serde_json::json!({ "type": "close-pane-result", "success": false, "error": e.to_string() }),
                                    },
                                    Err(_) => serde_json::json!({ "type": "close-pane-result", "success": false, "error": "invalid pane id" }),
                                };
                                let send = ws_tx.send(Message::Text(result.to_string())).await;
                                if success {
                                    if let Ok(pane_id) = Uuid::parse_str(pane_id_str) {
                                        subscribed_panes.remove(&(active_ws_id, pane_id));
                                        desync_by_pane.remove(&(active_ws_id, pane_id));
                                        last_resync_by_pane.remove(&(active_ws_id, pane_id));
                                        state.unregister_remote_sub(active_ws_id, pane_id, sub_id);
                                        if current_pane == Some((active_ws_id, pane_id)) {
                                            current_pane = None;
                                        }
                                    }
                                    let _ = state.remote_structural_tx.send(
                                        crate::types::RemoteStructuralEvent::PanesChanged { workspace_id: active_ws_id }
                                    );
                                    let _ = state.event_tx.try_send(
                                        crate::types::GlobalEvent::PaneTreeChanged { workspace_id: active_ws_id }
                                    );
                                }
        send
                            }
                            Some("list-files") => {
                                let path_str = parsed["path"].as_str().unwrap_or("").to_string();
                                // §unify: base dir follows the subscribed pane's cwd (same as the
                                // desktop file tree), falling back to the active project, then home.
                                let base_dir = current_pane
                                    .and_then(|(ws_id, pane_id)| {
                                        let map = state.workspaces.read();
                                        map.get(&ws_id)
                                            .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
                                            .and_then(|n| n.cwd.clone())
                                    })
                                    .or_else(|| state.current_project.read().clone())
                                    .or_else(dirs::home_dir)
                                    .unwrap_or_else(|| PathBuf::from("."));
                                let result = tokio::task::spawn_blocking(move || {
                                    // Empty/"/" → base dir; absolute → as-is; else relative to base.
                                    let target = if path_str.is_empty() || path_str == "/" {
                                        base_dir.clone()
                                    } else {
                                        let p = PathBuf::from(&path_str);
                                        if p.is_absolute() { p } else { base_dir.join(&path_str) }
                                    };
                                    // §unify: reuse the desktop file-tree pager so gitignore marking,
                                    // OS-junk filtering and dir-first sorting match the desktop UI.
                                    let page = crate::fs::tree::FileTree::page_children(&target, 0, 5000)
                                        .unwrap_or(crate::fs::tree::DirectoryPage {
                                            entries: Vec::new(),
                                            total: 0,
                                            offset: 0,
                                            has_more: false,
                                        });
                                    let parent = target.parent().map(|p| p.to_string_lossy().to_string());
                                    serde_json::json!({
                                        "type": "files",
                                        "path": target.to_string_lossy().to_string(),
                                        "parent": parent,
                                        "entries": page.entries,
                                    })
                                }).await;
                                match result {
                                    Ok(msg) => ws_tx.send(Message::Text(msg.to_string())).await,
                                    Err(e) => {
                                        tracing::warn!(target: "ridge::remote", error = %e, "list-files blocking task failed");
                                        Ok(())
                                    }
                                }
                            }
                            Some("list-remote-clients") => {
                                let clients = state.remote_client_registry.list();
                                let list: Vec<serde_json::Value> = clients.iter().map(|c| {
                                    let elapsed = c.connected_at.elapsed()
                                        .map(|d| d.as_secs())
                                        .unwrap_or(0);
                                    serde_json::json!({
                                        "id": c.id,
                                        "connectedAt": elapsed,
                                        "remoteAddr": c.remote_addr,
                                        "userAgent": c.user_agent,
                                    })
                                }).collect();
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "remote-clients",
                                    "clients": list,
                                }).to_string())).await
                            }
                            Some("kick-remote-client") => {
                                let target_id = parsed["id"].as_u64().unwrap_or(0);
                                let kicked = state.remote_client_registry.kick(target_id);
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "kick-remote-client-result",
                                    "success": kicked,
                                    "clientId": target_id,
                                }).to_string())).await
                            }
                            Some("list-git-status") => {
                                // §unify: same cwd resolution as list-files (subscribed pane → project → home).
                                let base_dir = current_pane
                                    .and_then(|(ws_id, pane_id)| {
                                        let map = state.workspaces.read();
                                        map.get(&ws_id)
                                            .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
                                            .and_then(|n| n.cwd.clone())
                                    })
                                    .or_else(|| state.current_project.read().clone())
                                    .or_else(dirs::home_dir)
                                    .unwrap_or_else(|| PathBuf::from("."));
                                let result = tokio::task::spawn_blocking(move || {
                                    // §unify: reuse the desktop git module so branch / commits / diff
                                    // come from the exact same source as the desktop Git panel.
                                    let info = crate::commands::git::git_info_for_path(&base_dir);
                                    serde_json::json!({
                                        "type": "git-status",
                                        "isGitRepo": info.is_git_repo,
                                        "currentBranch": info.current_branch,
                                        "branches": info.branches,
                                        "files": info.diff.files,
                                        "commits": info.commits,
                                    })
                                }).await;
                                match result {
                                    Ok(msg) => ws_tx.send(Message::Text(msg.to_string())).await,
                                    Err(e) => {
                                        tracing::warn!(target: "ridge::remote", error = %e, "list-git-status blocking task failed");
                                        Ok(())
                                    }
                                }
                            }
                            Some("search-files") => {
                                let query = parsed["query"].as_str().unwrap_or("").to_string();
                                // §unify: search root follows the subscribed pane's cwd (same as desktop).
                                let root = current_pane
                                    .and_then(|(ws_id, pane_id)| {
                                        let map = state.workspaces.read();
                                        map.get(&ws_id)
                                            .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
                                            .and_then(|n| n.cwd.clone())
                                    })
                                    .or_else(|| state.current_project.read().clone())
                                    .or_else(dirs::home_dir)
                                    .unwrap_or_else(|| PathBuf::from("."));
                                let results = if query.trim().is_empty() {
                                    Vec::new()
                                } else {
                                    // §unify: reuse the desktop text_search engine (gitignore-aware ripgrep walk).
                                    crate::commands::project::text_search(
                                        root.to_string_lossy().to_string(),
                                        query.clone(),
                                        None, None, None, Some(200), None, None,
                                    )
                                    .await
                                    .unwrap_or_default()
                                };
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "search-results",
                                    "query": query,
                                    "results": results,
                                }).to_string())).await
                            }
                            Some("cycle-theme") => {
                                // §theme-cycle: a control end taps "theme" → push the theme
                                // AFTER the one it currently shows. Stateless: we never write
                                // the active theme to disk nor clobber peers (§theme-isolation
                                // — the control end owns its own appearance). The client tracks
                                // its current id (seeded from the connect `theme` push) and
                                // sends it as `current`; an unknown/empty id starts at index 0.
                                let current = parsed["current"].as_str().unwrap_or("");
                                let tf = ridge_core::commands::theme::get_theme_data();
                                if tf.themes.is_empty() {
                                    Ok(())
                                } else {
                                    let n = tf.themes.len();
                                    let next = match tf.themes.iter().position(|t| t.id == current) {
                                        Some(idx) => (idx + 1) % n,
                                        None => 0,
                                    };
                                    let entry = &tf.themes[next];
                                    let msg = serde_json::json!({
                                        "type": "theme",
                                        "id": entry.id,
                                        "themeType": entry.theme_type,
                                        "colors": entry.colors,
                                    });
                                    ws_tx.send(Message::Text(msg.to_string())).await
                                }
                            }
                            Some("list-workspace-panes") => {
                                // §peek-panes: list an arbitrary workspace's panes WITHOUT
                                // switching this client's active workspace — backs the tree's
                                // "expand a non-active workspace to peek at its terminals".
                                // Read-only; never touches `active_ws_id`.
                                let id_str = parsed["workspaceId"].as_str().unwrap_or("");
                                let pane_list = if let Ok(id) = Uuid::parse_str(id_str) {
                                    let workspaces = state.workspaces.read();
                                    workspaces.get(&id).map(build_remote_pane_list).unwrap_or_default()
                                } else {
                                    Vec::new()
                                };
                                ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "workspace-panes",
                                    "workspaceId": id_str,
                                    "panes": pane_list,
                                }).to_string())).await
                            }
                            Some("set-host-clipboard") => {
                                // §copy-to-host: a control end's copy ALSO lands on the DESKTOP
                                // host's system clipboard, so the host's own native paste (Ctrl+V)
                                // picks it up. An authenticated remote already has shell stdin, so
                                // writing the clipboard is strictly less powerful — best-effort,
                                // failures are non-fatal (e.g. a headless host with no AppHandle).
                                if let Some(text) = parsed["text"].as_str() {
                                    if let Some(app) = state.app_handle.get() {
                                        use tauri_plugin_clipboard_manager::ClipboardExt;
                                        let _ = app.clipboard().write_text(text.to_string());
                                    }
                                }
                                Ok(())
                            }
                            Some("data-request") => {
                                // Backs the remote `WsDataProvider` (src/lib/transport/ws.ts).
                                // An authenticated remote already has shell stdin, so this
                                // mirrors the desktop `TauriDataProvider` 1:1 within the SAME
                                // trust boundary. Guards layered on top: a per-connection rate
                                // limit (below), a read-only toggle + path-traversal rejection
                                // + audit log of mutations (in `dispatch_data_request`). The
                                // reply carries `_reqId` plus `_result` (ok) or `_error` (fail).
                                let req_id = parsed["_reqId"].as_u64().unwrap_or(0);
                                let method = parsed["method"].as_str().unwrap_or("").to_string();

                                if req_id == 0 {
                                    ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "data-result",
                                        "_reqId": req_id,
                                        "_error": "invalid data request id",
                                    }).to_string())).await
                                } else if data_requests.take_pre_cancelled(req_id) {
                                    ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "data-result",
                                        "_reqId": req_id,
                                        "_error": "data request cancelled",
                                    }).to_string())).await
                                } else if data_requests.len() >= MAX_IN_FLIGHT_DATA_REQUESTS {
                                    tracing::warn!(target: "ridge::remote", client_id, req_id, "data-request in-flight cap exceeded; rejecting");
                                    ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "data-result",
                                        "_reqId": req_id,
                                        "_error": "too many in-flight data requests",
                                    }).to_string())).await
                                } else if data_requests.contains(req_id) {
                                    ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "data-result",
                                        "_reqId": req_id,
                                        "_error": "duplicate data request id",
                                    }).to_string())).await
                                } else {

                                // §rate-limit: refill the window, then count this request.
                                if dr_window_start.elapsed() >= DR_WINDOW {
                                    dr_window_start = Instant::now();
                                    dr_count = 0;
                                }
                                dr_count += 1;
                                if dr_count > DR_MAX_PER_WINDOW {
                                    tracing::warn!(
                                        target: "ridge::remote",
                                        client_id, method = %method,
                                        "data-request rate limit exceeded; rejecting"
                                    );
                                    let reply = serde_json::json!({
                                        "_reqId": req_id,
                                        "type": "data-result",
                                        "_error": "rate limited: too many data requests",
                                    });
                                    ws_tx.send(Message::Text(reply.to_string())).await
                                } else {
                                    let git_slot = format!("remote:{client_id}:{req_id}");
                                    let result_tx = data_result_tx.clone();
                                    let task_method = method.clone();
                                    let task_params = parsed.clone();
                                    let task_state = state.clone();
                                    let task_slot = git_slot.clone();
                                    let handle = tokio::spawn(async move {
                                        let slot_for_scope = task_slot.clone();
                                        let reply = ridge_core::commands::git::with_git_request_slot(
                                            slot_for_scope,
                                            dispatch_data_request(
                                                &task_method,
                                                &task_params,
                                                &task_state,
                                                Some(task_slot),
                                            ),
                                        )
                                        .await;
                                        let _ = result_tx.send((req_id, reply)).await;
                                    });
                                    // IDs are monotonic per connection; a duplicate was rejected
                                    // above and therefore cannot overwrite a live task.
                                    let _ = data_requests.insert(req_id, PendingDataRequest { handle, git_slot });
                                    Ok(())
                                }
                                }
                            }
                            Some("data-cancel") => {
                                let req_id = parsed["_reqId"].as_u64().unwrap_or(0);
                                if req_id != 0 {
                                    if let Some(slot) = data_requests.cancel(req_id) {
                                        ridge_core::commands::git::cancel_git_slot(&slot);
                                    }
                                    tracing::debug!(target: "ridge::remote", client_id, req_id, "data request cancelled");
                                }
                                Ok(())
                            }
                            Some("invoke-cancel") => {
                                let req_id = parsed["_reqId"].as_u64().unwrap_or(0);
                                if req_id != 0 {
                                    let key = InvokeRequestKey::Legacy(req_id);
                                    if let Some(slot) = invoke_requests.cancel(&key) {
                                        ridge_core::commands::git::cancel_git_slot(&slot);
                                    }
                                    tracing::debug!(target: "ridge::remote", client_id, req_id, "invoke request cancelled");
                                }
                                Ok(())
                            }
                            Some("invoke-request") => {
                                // Backs the browser-side Tauri `invoke()` shim
                                // (src/lib/transport/tauriShim/core.ts) used when the FULL
                                // desktop UI is served to a desktop browser. Same trust
                                // boundary, rate limit, read-only gate, traversal guard and
                                // audit as `data-request`; the explicit allowlist in
                                // `dispatch_invoke_request` is the security boundary (unknown
                                // cmd → error), so host-privileged / remote-admin commands
                                // (get_remote_info, set_remote_enabled, deep-root, …) stay
                                // unreachable. The reply carries `type:'invoke-result'` so it
                                // survives RemoteConnection's onmessage routing.
                                let req_id = parsed["_reqId"].as_u64().unwrap_or(0);
                                let cmd = parsed["cmd"].as_str().unwrap_or("").to_string();
                                if dr_window_start.elapsed() >= DR_WINDOW {
                                    dr_window_start = Instant::now();
                                    dr_count = 0;
                                }
                                dr_count += 1;
                                if dr_count > DR_MAX_PER_WINDOW {
                                    tracing::warn!(target: "ridge::remote", client_id, cmd = %cmd, "invoke-request rate limit exceeded; rejecting");
                                    let reply = serde_json::json!({
                                        "type": "invoke-result", "_reqId": req_id,
                                        "_error": "rate limited: too many invoke requests",
                                    });
                                    ws_tx.send(Message::Text(reply.to_string())).await
                                } else {
                                    let key = InvokeRequestKey::Legacy(req_id);
                                    if invoke_requests.take_pre_cancelled(&key) {
                                        let reply = serde_json::json!({
                                            "type": "invoke-result", "_reqId": req_id,
                                            "_error": "request cancelled",
                                        });
                                        ws_tx.send(Message::Text(reply.to_string())).await
                                    } else if req_id == 0
                                        || invoke_requests.len() >= MAX_IN_FLIGHT_INVOKE_REQUESTS
                                        || invoke_requests.contains(&key)
                                    {
                                        let reply = serde_json::json!({
                                            "type": "invoke-result", "_reqId": req_id,
                                            "_error": if req_id == 0 {
                                                "invalid invoke request id"
                                            } else {
                                                "too many invoke requests"
                                            },
                                        });
                                        ws_tx.send(Message::Text(reply.to_string())).await
                                    } else {
                                        let empty = serde_json::json!({});
                                        let args = parsed.get("args").unwrap_or(&empty).clone();
                                        let result_tx = invoke_result_tx.clone();
                                        let task_cmd = cmd.clone();
                                        let task_state = state.clone();
                                        let task_key = key.clone();
                                        let git_slot = format!("remote:{client_id}:invoke:{}", key.slot_name());
                                        let task_slot = git_slot.clone();
                                        let handle = tokio::spawn(async move {
                                            let mut reply = ridge_core::commands::git::with_git_request_slot(
                                                task_slot,
                                                dispatch_invoke_request(&task_cmd, &args, &task_state),
                                            )
                                            .await;
                                            if let Some(obj) = reply.as_object_mut() {
                                                obj.insert("_reqId".to_string(), serde_json::json!(req_id));
                                                obj.insert("type".to_string(), serde_json::json!("invoke-result"));
                                            }
                                            let _ = result_tx
                                                .send(InvokeResult::Legacy { key: task_key, reply })
                                                .await;
                                        });
                                        let _ = invoke_requests.insert(
                                            key,
                                            PendingInvokeRequest { handle, git_slot },
                                        );
                                        Ok(())
                                    }
                                }
                            }
                            _ => {
                                ws_tx.send(Message::Text(serde_json::json!({"type":"error","message":"unknown message type"}).to_string())).await
                            }
                        };
                    }
                    invoke_result = invoke_result_rx.recv() => {
                        let Some(result) = invoke_result else { break; };
                        match result {
                            InvokeResult::Legacy { key, reply } => {
                                if invoke_requests.complete(&key) {
                                    if ws_tx.send(Message::Text(reply.to_string())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            InvokeResult::JsonRpc { key, reply } => {
                                if invoke_requests.complete(&key) {
                                    if ws_tx.send(Message::Text(reply.to_string())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    data_result = data_result_rx.recv() => {
                        let Some((req_id, mut reply)) = data_result else { break; };
                        // A cancelled task may already have queued a result. The
                        // registry ownership check suppresses that stale reply.
                        if data_requests.complete(req_id) {
                            if let Some(obj) = reply.as_object_mut() {
                                obj.insert("_reqId".to_string(), serde_json::json!(req_id));
                                obj.insert("type".to_string(), serde_json::json!("data-result"));
                            }
                            if ws_tx.send(Message::Text(reply.to_string())).await.is_err() {
                                break;
                            }
                        }
                    }
                    event = raw_rx.recv() => {
                        match event {
                            Some((foreground, crate::types::RemotePtyEvent::RawBytes { workspace_id, pane_id, bytes })) => {
                                // §perf (B方案): first-byte 段 = 从 raw_rx 收到的第一帧 PTY
                                // 输出，用 Option<Instant> 守卫只打一次。
                                if first_pty_bytes_at.is_none() {
                                    first_pty_bytes_at = Some(Instant::now());
                                    tracing::info!(target: "ridge::remote::perf", client_id, elapsed_ms = ws_connect_start.elapsed().as_millis() as u64, "ws_first_pty_bytes");
                                }
                                let key = (workspace_id, pane_id);
                                if subscribed_panes.contains(&key) {
                                    let Some(desync) = desync_by_pane.get(&key) else { continue; };
                                    // Stale frames already queued on the prior lane are never
                                    // allowed to cross a focus barrier.
                                    if (foreground && current_pane != Some(key))
                                        || (!foreground && current_pane == Some(key))
                                    {
                                        desync.store(true, Ordering::Release);
                                        continue;
                                    }
                                    // Once a background pane has a hole, later bytes would only
                                    // corrupt its parser. Keep it subscribed but stale until it
                                    // is promoted and receives one bounded resync.
                                    if desync.load(Ordering::Acquire) && current_pane != Some(key) {
                                        continue;
                                    }
                                    // §resync: if the fan-out dropped frames for this sub, the
                                    // client's vte stream has a hole that would corrupt every
                                    // subsequent parse. Reset the terminal (RIS) and replay
                                    // fresh scrollback before the current bytes so the parser
                                    // re-synchronises — but throttle it (see RESYNC_MIN_INTERVAL)
                                    // so a sustained-overload loop can't amplify congestion. We
                                    // only CONSUME the desync flag when we actually resync; if
                                    // throttled it stays set for a later frame to handle.
                                    if desync.load(Ordering::Acquire) {
                                        let now = Instant::now();
                                        let throttled = last_resync_by_pane
                                            .get(&key)
                                            .is_some_and(|t| now.duration_since(*t) < RESYNC_MIN_INTERVAL);
                                        if !throttled {
                                            desync.store(false, Ordering::Release);
                                            last_resync_by_pane.insert(key, now);
                                            let history = state.get_recent_scrollback_for(
                                                workspace_id, pane_id, ridge_remote::pane::RESYNC_SCROLLBACK_LAN,
                                            );
                                            // §mode-reattach: 16B pane-id 前缀 + (RIS + 活动模式前导 +
                                            // scrollback)，整帧走共享 SSOT `ridge_remote::pane`（与 cloud /
                                            // rdg 一份），前导让控制端重建鼠标上报/alt 等一次性态。
                                            let (modes, alt) = state.get_pane_modes(workspace_id, pane_id);
                                            let resync = ridge_remote::pane::pane_resync_frame(
                                                pane_id, &history, &modes, alt,
                                            );
                                            if (if foreground { &ws_tx } else { &low_tx })
                                                .try_send(Message::Binary(resync.into()))
                                                .is_err()
                                            {
                                                desync.store(true, Ordering::Release);
                                                continue;
                                            }
                                        }
                                    }
                                    let frame = ridge_remote::pane::pane_frame(pane_id, &bytes);
                                    if (if foreground { &ws_tx } else { &low_tx })
                                        .try_send(Message::Binary(frame.into()))
                                        .is_err()
                                    {
                                        desync.store(true, Ordering::Release);
                                    }
                                }
                            }
                            Some((_, crate::types::RemotePtyEvent::Metadata { workspace_id, pane_id, title, cwd })) => {
                                if subscribed_panes.contains(&(workspace_id, pane_id)) {
                                    let _ = ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "pty-meta",
                                        "workspaceId": workspace_id.to_string(),
                                        "paneId": pane_id.to_string(),
                                        "title": title,
                                        "cwd": cwd.clone(),
                                    }).to_string())).await;
                                    // §web-remote: desktop UI listens to pane-cwd-changed-{ws}-{pane}.
                                    // Title-only Metadata events carry cwd=None (PaneTitleChanged fires
                                    // on every prompt redraw). Forwarding them with a null cwd made the
                                    // controller call setPaneCwd(null) → normalizeCwd(null).replace →
                                    // TypeError spam. Only emit a cwd-changed event when there IS a cwd.
                                    if let Some(cwd) = &cwd {
                                        let _ = ws_tx.send(Message::Text(serde_json::json!({
                                            "type": "event",
                                            "name": format!("pane-cwd-changed-{}-{}", workspace_id, pane_id),
                                            "payload": { "cwd": cwd },
                                        }).to_string())).await;
                                    }
                                }
                            }
                            Some((_, crate::types::RemotePtyEvent::PtyResized { workspace_id, pane_id, rows, cols })) => {
                                if subscribed_panes.contains(&(workspace_id, pane_id)) {
                                    let _ = ws_tx
                                        .send(Message::Text(pty_resized_message(
                                            workspace_id, pane_id, rows, cols,
                                        )))
                                        .await;
                                }
                            }
                            None => break,
                        }
                    }
                    structural = structural_rx.recv() => {
                        match structural {
                            Ok(crate::types::RemoteStructuralEvent::PanesChanged { workspace_id }) => {
                                if subscribed_panes.iter().any(|(ws, _)| *ws == workspace_id) {
                                    // Self-heal orphaned PTYs/pending before re-enumerating.
                                    crate::commands::terminal::reap_orphan_panes_all(&state).await;
                                    // Re-enumerate panes for this workspace and push to client.
                                    let pane_list = {
                                        let workspaces = state.workspaces.read();
                                        if let Some(ws) = workspaces.get(&workspace_id) {
                                            build_remote_pane_list(ws)
                                        } else {
                                            Vec::new()
                                        }
                                    };
                                    let _ = ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "panes",
                                        "workspaceId": workspace_id.to_string(),
                                        "panes": pane_list
                                    }).to_string())).await;
                                    // §web-remote: desktop UI re-syncs layout on pane-tree-changed.
                                    let _ = ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "event",
                                        "name": "pane-tree-changed",
                                        "payload": { "workspaceId": workspace_id.to_string() },
                                    }).to_string())).await;
                                }
                            }
                            Ok(crate::types::RemoteStructuralEvent::WorkspacesChanged) => {
                                // Push updated workspace list.
                                let ws_list = {
                                    let order = state.workspace_order.read();
                                    let names = state.workspace_names.read();
                                    let map = state.workspaces.read();
                                    let active = active_ws_id;
                                    order.iter().map(|id| {
                                        let display_seq = map.get(id).map(|w| w.display_seq).unwrap_or(0);
                                        serde_json::json!({
                                            "id": id.to_string(),
                                            "name": names.get(id).cloned()
                                                .unwrap_or_else(|| format!("工作区 {}", display_seq)),
                                            "displaySeq": display_seq,
                                            "active": *id == active,
                                        })
                                    }).collect::<Vec<_>>()
                                };
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "workspaces", "workspaces": ws_list
                                }).to_string())).await;
                                // §web-remote: desktop UI refreshes on workspace-list-changed.
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "event", "name": "workspace-list-changed", "payload": {},
                                }).to_string())).await;
                            }
                            Ok(crate::types::RemoteStructuralEvent::WorkspaceRenamed { workspace_id, name }) => {
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "workspace-renamed",
                                    "workspaceId": workspace_id.to_string(),
                                    "name": name,
                                }).to_string())).await;
                                let _ = ws_tx.send(Message::Text(serde_json::json!({
                                    "type": "event", "name": "workspace-list-changed", "payload": {},
                                }).to_string())).await;
                            }
                            Err(_) => {
                                // Lagged — skip; the next request-response cycle will fix it.
                            }
                        }
                    }
                    ui_evt = ui_event_rx.recv() => {
                        // §web-remote: relay a host Tauri event to the desktop browser's
                        // `listen()` shim. Broadcast to every client; the mobile SPA drops it.
                        //
                        // §S3 backpressure + coalesce (§5.2 / R8): `git checkout`,
                        // dependency installs etc. produce event STORMS (many
                        // `fs-changed`/scm-refresh in a tick). The broadcast bus is already
                        // bounded (capacity 256, drop-oldest on lag — the Lagged arm), but
                        // forwarding each event 1:1 to a slow WS client can still stall the
                        // socket. So after the first event we DRAIN everything currently
                        // queued without awaiting and COALESCE by event name (latest payload
                        // wins, insertion order preserved), collapsing a burst into one send
                        // per distinct event name. A slow client thus sees the *final* state,
                        // never an unbounded backlog.
                        if let Ok(first) = ui_evt {
                            // Insertion-ordered de-dup: name → payload, with a parallel order vec.
                            let mut order: Vec<String> = Vec::new();
                            let mut latest: std::collections::HashMap<String, serde_json::Value> =
                                std::collections::HashMap::new();
                            let mut push = |name: String, payload: serde_json::Value| {
                                if !latest.contains_key(&name) {
                                    order.push(name.clone());
                                }
                                latest.insert(name, payload);
                            };
                            push(first.name, first.payload);
                            // Drain everything already buffered (non-blocking); stop on empty.
                            // Bounded by the broadcast capacity (256), so this cannot spin.
                            loop {
                                match ui_event_rx.try_recv() {
                                    Ok(ev) => push(ev.name, ev.payload),
                                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {
                                        // Skipped some — the coalesced latest-state below still
                                        // converges the client; keep draining what remains.
                                        continue;
                                    }
                                    Err(_) => break, // Empty or Closed → done draining.
                                }
                            }
                            let mut send_failed = false;
                            for name in order {
                                if let Some(payload) = latest.remove(&name) {
                                    if ws_tx.send(Message::Text(serde_json::json!({
                                        "type": "event",
                                        "name": name,
                                        "payload": payload,
                                    }).to_string())).await.is_err() {
                                        send_failed = true;
                                        break;
                                    }
                                }
                            }
                            if send_failed { break; }
                        }
                    }
                    _ = health_interval.tick() => {
                        // §M-2: a token session whose token has expired (TTL) or
                        // been revoked must not keep streaming. `?code=` (TOTP)
                        // connections carry no token, so this only applies when one
                        // was presented. `validate_token_bound` also reaps the
                        // expired entry as a side effect.
                        let token_revoked = match recheck_token.as_deref() {
                            Some(t) => !state
                                .remote_session_store
                                .validate_token_bound(t, &recheck_device, &recheck_addr),
                            None => false,
                        };
                        let close_reason = if !state.remote_enabled.load(Ordering::Relaxed) {
                            Some("Remote control disabled")
                        } else if kill_flag.load(Ordering::Relaxed) {
                            Some("Disconnected by admin")
                        } else if state
                            .remote_blacklist
                            .is_blocked(&recheck_device, &recheck_addr)
                        {
                            Some("Device blacklisted")
                        } else if token_revoked {
                            Some("Session expired")
                        } else {
                            None
                        };
                        if let Some(reason) = close_reason {
                            tracing::info!(target: "ridge::remote", client_id, reason, "WS closed by health check");
                            let _ = ws_tx.send(Message::Close(Some(
                                axum::extract::ws::CloseFrame {
                                    code: 1000,
                                    reason: std::borrow::Cow::Borrowed(reason),
                                }
                            ))).await;
                            break;
                        }
                    }
                }
    }

    // Cancel all detached data work before dropping the socket. Slotted Git
    // requests also invalidate the process-guard generation, killing a live
    // child tree instead of waiting for the normal timeout.
    for slot in data_requests.cancel_all() {
        ridge_core::commands::git::cancel_git_slot(&slot);
    }
    for slot in invoke_requests.cancel_all() {
        ridge_core::commands::git::cancel_git_slot(&slot);
    }

    // Clean up: unregister from all subscribed panes (single-pane mobile slot +
    // the multi-pane desktop set).
    for (ws, pane) in subscribed_panes.drain() {
        state.unregister_remote_sub(ws, pane, sub_id);
    }
    state.remote_client_registry.unregister(client_id);

    tracing::info!(target: "ridge::remote", client_id, "WebSocket client disconnected");
    // §perf (B方案): 连接收尾汇总 = 本连接从 hello 起到关闭的总时长。
    tracing::info!(target: "ridge::remote::perf", client_id, total_ms = ws_connect_start.elapsed().as_millis() as u64, "ws_closed");
}

/// Dispatches one remote `data-request` `method` to the same backend command
/// the desktop `TauriDataProvider` (src/lib/transport/tauri.ts) invokes, with
/// the same arguments, and returns `{"_result": ...}` on success or
/// `{"_error": ...}` on failure. The caller stamps `_reqId`.
///
/// Paths arrive absolute (identical to the desktop IPC contract) and are passed
/// through unchanged — desktop and remote therefore behave identically. Most
/// backing commands are `async` and offload their own blocking work; the two
/// shape-mismatched methods (`git_status`, `search_files`) delegate to the
/// dedicated mappers below.
/// Methods that mutate the filesystem or git repository state. Gated by the
/// read-only toggle and audit-logged.
fn is_mutating_method(method: &str) -> bool {
    matches!(
        method,
        "write_file"
            | "rename_path"
            | "delete_path"
            | "create_file"
            | "create_directory"
            | "copy_path"
            | "move_path"
            | "git_stage"
            | "git_unstage"
            | "git_commit"
            | "git_pull"
            | "git_push"
            | "git_sync"
            | "git_checkout"
            | "git_revert"
            | "git_cherry_pick"
            | "git_reset"
            | "git_create_tag"
            | "git_discard"
            | "git_clean_untracked"
    )
}

/// Rejects a path that contains a `..` component (post-split, both separators).
/// Absolute paths still pass — this only blocks traversal tricks, not the
/// already-trusted absolute-path contract shared with the desktop.
fn path_has_traversal(p: &str) -> bool {
    !p.is_empty() && p.split(['/', '\\']).any(|c| c == "..")
}

async fn dispatch_data_request(
    method: &str,
    params: &serde_json::Value,
    state: &AppState,
    git_slot: Option<String>,
) -> serde_json::Value {
    use crate::commands::{git, project};

    // §audit: record every remote mutation so a trust-but-verify operator has a
    // trail. (Remote sessions are always read-write — there is no read-only mode.)
    if is_mutating_method(method) {
        tracing::info!(target: "ridge::remote::fs", method, "remote mutating data-request");
    }

    // §traversal guard: reject `..` in any path-bearing field before it reaches
    // the filesystem layer.
    for key in ["path", "from", "to", "repoRoot"] {
        if let Some(v) = params.get(key).and_then(|x| x.as_str()) {
            if path_has_traversal(v) {
                tracing::warn!(
                    target: "ridge::remote::fs", method, key,
                    "rejected data-request: path traversal"
                );
                return serde_json::json!({ "_error": "path traversal rejected" });
            }
        }
    }
    if let Some(arr) = params.get("paths").and_then(|x| x.as_array()) {
        if arr
            .iter()
            .filter_map(|x| x.as_str())
            .any(path_has_traversal)
        {
            return serde_json::json!({ "_error": "path traversal rejected" });
        }
    }

    // Field extractors — keep each arm to a single readable line.
    fn s(v: &serde_json::Value, k: &str) -> String {
        v[k].as_str().unwrap_or("").to_string()
    }
    fn usize_opt(v: &serde_json::Value, k: &str) -> Option<usize> {
        v[k].as_u64().map(|n| n as usize)
    }
    fn path_list(v: &serde_json::Value) -> Vec<String> {
        v["paths"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
    // Result envelopes.
    fn unit(r: Result<(), String>) -> serde_json::Value {
        match r {
            Ok(()) => serde_json::json!({ "_result": null }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }
    fn val<T: Serialize>(r: Result<T, String>) -> serde_json::Value {
        match r {
            Ok(v) => serde_json::json!({ "_result": v }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }

    match method {
        // ── Filesystem ──
        "get_file_tree" => {
            val(project::get_file_tree(s(params, "path"), usize_opt(params, "depth")).await)
        }
        "get_directory_children" => val(project::get_directory_children(
            s(params, "path"),
            usize_opt(params, "offset"),
            usize_opt(params, "limit"),
        )
        .await),
        "path_exists" => val(project::path_exists(s(params, "path")).await),
        "read_file" => val(project::read_file(s(params, "path"))),
        "write_file" => unit(project::write_file(s(params, "path"), s(params, "content")).await),
        "rename_path" => unit(project::rename_path(s(params, "from"), s(params, "to"))),
        "delete_path" => unit(project::delete_path(s(params, "path")).await),
        "create_file" => unit(project::create_file(s(params, "path"))),
        "create_directory" => unit(project::create_directory(s(params, "path"))),
        "copy_path" => unit(project::copy_path(s(params, "from"), s(params, "to"), None).await),
        "move_path" => unit(project::move_path(s(params, "from"), s(params, "to")).await),

        // ── Git ── (all async; offload internally)
        "git_status" => git_status_result(s(params, "repoRoot"), git_slot).await,
        "git_stage" => unit(git::git_stage(s(params, "repoRoot"), path_list(params)).await),
        "git_unstage" => unit(git::git_unstage(s(params, "repoRoot"), path_list(params)).await),
        "git_commit" => unit(
            git::git_commit(
                s(params, "repoRoot"),
                s(params, "message"),
                params["amend"].as_bool(),
            )
            .await,
        ),
        "git_pull" => unit(git::git_pull(s(params, "repoRoot")).await),
        "git_push" => {
            unit(git::git_push(s(params, "repoRoot"), params["setUpstream"].as_bool()).await)
        }
        "git_sync" => unit(git::git_sync(s(params, "repoRoot")).await),
        "git_checkout" => unit(
            git::git_checkout(
                s(params, "repoRoot"),
                s(params, "branch"),
                params["create"].as_bool(),
                None,
            )
            .await,
        ),
        "git_revert" => unit(git::git_revert(s(params, "repoRoot"), s(params, "hash")).await),
        "git_cherry_pick" => {
            unit(git::git_cherry_pick(s(params, "repoRoot"), s(params, "hash")).await)
        }
        // Frontend sends { mode, commit }; git_reset takes (repo_root, hash, mode).
        "git_reset" => unit(
            git::git_reset(
                s(params, "repoRoot"),
                s(params, "commit"),
                s(params, "mode"),
            )
            .await,
        ),
        "git_create_tag" => unit(
            git::git_create_tag(
                s(params, "repoRoot"),
                s(params, "name"),
                None,
                params["message"].as_str().map(String::from),
            )
            .await,
        ),
        "git_discard" => unit(git::git_discard(s(params, "repoRoot"), path_list(params)).await),
        "git_clean_untracked" => {
            unit(git::git_clean_untracked(s(params, "repoRoot"), Vec::new()).await)
        }
        // Read-only: unified diff of one file vs HEAD (or the index when cached).
        "git_diff_file" => val(
            git::git_diff_file(
                s(params, "repoRoot"),
                s(params, "path"),
                params["cached"].as_bool(),
            )
            .await,
        ),
        "git_stash_list" => val(git::git_stash_list(s(params, "repoRoot")).await),

        // ── Search ──
        "search_files" => search_files_result(state, s(params, "query"), s(params, "path")).await,

        other => serde_json::json!({ "_error": format!("unknown data-request method: {}", other) }),
    }
}

/// Maps `ScmRepoStatus` (+ recent commit log) into the frontend `GitStatusResult`
/// shape: `{ staged, unstaged, untracked, commits }`. Commits aren't part of
/// `ScmRepoStatus`, so they're pulled separately via `git_info_for_path` on a
/// blocking thread (it shells out to `git log`).
async fn git_status_result(repo_root: String, git_slot: Option<String>) -> serde_json::Value {
    let scm = match crate::commands::git::get_scm_status(repo_root.clone(), git_slot).await {
        Ok(status) => status,
        Err(e) => return serde_json::json!({ "_error": e }),
    };
    // Do not call the synchronous `git_info_for_path` from an ad-hoc
    // `spawn_blocking`: that path bypassed the shared git semaphore and could
    // recreate the remote git.exe pile-up under repeated status polling.
    // The paginated log command uses the same bounded admission + timeout path
    // as `get_scm_status`, and avoids re-running branch/diff probes already
    // covered by the status request.
    let commits = crate::commands::git::get_git_commits_paginated(repo_root.clone(), 0, 50)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| serde_json::json!({
            "hash": c.hash,
            "msg": c.subject,
            "time": c.date,
            "author": c.author,
            "parents": c.parents,
            "refs": c.refs,
        }))
        .collect::<Vec<_>>();
    // Branch listing is part of the same Git snapshot consumed by the Remote
    // Graph tab. A failed optional branch probe must not hide a valid status.
    let branches = crate::commands::git::git_list_branches(repo_root.clone())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|branch| branch.name)
        .collect::<Vec<_>>();

    let map_files = |files: Vec<crate::commands::git::ScmFile>| {
        files
            .into_iter()
            .map(|f| serde_json::json!({ "name": f.path, "status": f.status }))
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "_result": {
            "is_git_repo": scm.is_git_repo,
            "current_branch": scm.current_branch,
            "has_upstream": scm.has_upstream,
            "branches": branches,
            "staged": map_files(scm.staged),
            "unstaged": map_files(scm.changes),
            "untracked": scm.untracked.into_iter().map(|f| f.path).collect::<Vec<_>>(),
            "commits": commits,
        }
    })
}

/// Runs the desktop text-search engine and remaps `fs::search::SearchResult`
/// (`{ file, content }`) onto the frontend `SearchResult` (`{ path, snippet }`).
/// An empty `path` falls back to the active project, then the home dir.
async fn search_files_result(state: &AppState, query: String, path: String) -> serde_json::Value {
    if query.trim().is_empty() {
        return serde_json::json!({ "_result": [] });
    }
    let root = if path.trim().is_empty() {
        state
            .current_project
            .read()
            .clone()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .to_string_lossy()
            .to_string()
    } else {
        path
    };
    match crate::commands::project::text_search(
        root,
        query,
        None,
        None,
        None,
        Some(500),
        None,
        None,
    )
    .await
    {
        Ok(results) => {
            let mapped = results
                .into_iter()
                .map(|r| serde_json::json!({ "path": r.file, "line": r.line, "column": r.column, "snippet": r.content }))
                .collect::<Vec<_>>();
            serde_json::json!({ "_result": mapped })
        }
        Err(e) => serde_json::json!({ "_error": e }),
    }
}

/// FS/git-write commands, used only to tag remote mutations for the audit log.
/// Remote sessions are always read-write (there is no read-only session mode),
/// so this is purely a mutation-audit predicate, not an access gate.
fn is_mutating_invoke(cmd: &str) -> bool {
    is_mutating_method(cmd) || matches!(cmd, "replace_in_files" | "apply_file_edits")
}

/// Dispatches one browser `invoke-request` to the matching desktop command. The
/// real `#[tauri::command]` functions are called directly with a `State` /
/// `AppHandle` derived from the process-stashed handle (`AppState.app_handle`),
/// so behaviour is identical to the desktop IPC path. This is an ALLOWLIST:
/// unknown commands — including deliberately-excluded host-privileged ones
/// (`get_remote_info` exposes the live TOTP; `set_remote_enabled` /
/// `disconnect_session` / blacklist are remote-admin; `enter_deep_root_mode` /
/// `set_cloud_remote_active` is host-only) — return
/// an error and never reach a handler.
async fn dispatch_invoke_request(
    cmd: &str,
    args: &serde_json::Value,
    state: &AppState,
) -> serde_json::Value {
    use crate::commands::{fs_watch, git, pane, project, ridge_file, terminal, watch, workspace};
    use tauri::Manager;

    // §audit: record every remote mutation so a trust-but-verify operator has a
    // trail. (Remote sessions are always read-write — there is no read-only mode.)
    if is_mutating_invoke(cmd) {
        tracing::info!(target: "ridge::remote::fs", cmd, "remote mutating invoke");
    }
    // §traversal guard: reject `..` in any path-bearing field.
    for key in ["path", "from", "to", "repoRoot", "root", "cwd"] {
        if let Some(v) = args.get(key).and_then(|x| x.as_str()) {
            if path_has_traversal(v) {
                return serde_json::json!({ "_error": "path traversal rejected" });
            }
        }
    }
    if let Some(arr) = args.get("paths").and_then(|x| x.as_array()) {
        if arr
            .iter()
            .filter_map(|x| x.as_str())
            .any(path_has_traversal)
        {
            return serde_json::json!({ "_error": "path traversal rejected" });
        }
    }

    // ── arg extractors (frontend sends Tauri-style camelCase keys) ──
    fn s(v: &serde_json::Value, k: &str) -> String {
        v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
    }
    fn opt_s(v: &serde_json::Value, k: &str) -> Option<String> {
        v.get(k).and_then(|x| x.as_str()).map(String::from)
    }
    fn usize_opt(v: &serde_json::Value, k: &str) -> Option<usize> {
        v.get(k).and_then(|x| x.as_u64()).map(|n| n as usize)
    }
    fn usize_arg(v: &serde_json::Value, k: &str) -> usize {
        v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as usize
    }
    fn u16_opt(v: &serde_json::Value, k: &str) -> Option<u16> {
        v.get(k).and_then(|x| x.as_u64()).map(|n| n as u16)
    }
    fn u16_arg(v: &serde_json::Value, k: &str) -> u16 {
        v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u16
    }
    fn u32_arg(v: &serde_json::Value, k: &str) -> u32 {
        v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32
    }
    fn bool_opt(v: &serde_json::Value, k: &str) -> Option<bool> {
        v.get(k).and_then(|x| x.as_bool())
    }
    fn vec_s(v: &serde_json::Value, k: &str) -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
    fn from_arg<T: serde::de::DeserializeOwned>(
        v: &serde_json::Value,
        k: &str,
    ) -> Result<T, String> {
        serde_json::from_value(v.get(k).cloned().unwrap_or(serde_json::Value::Null))
            .map_err(|e| e.to_string())
    }
    fn val<T: Serialize>(r: Result<T, String>) -> serde_json::Value {
        match r {
            Ok(v) => serde_json::json!({ "_result": v }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }
    fn unit(r: Result<(), String>) -> serde_json::Value {
        match r {
            Ok(()) => serde_json::json!({ "_result": null }),
            Err(e) => serde_json::json!({ "_error": e }),
        }
    }
    fn plain<T: Serialize>(v: T) -> serde_json::Value {
        serde_json::json!({ "_result": v })
    }
    // S1: map a `ridge_core::dispatch` result onto the legacy WS envelope.
    // `Ok(value)` → `{ "_result": value }`; `Err(core_err)` → `{ "_error":
    // message }` (the same human string the legacy handlers produced). The
    // structured JSON-RPC `{code,message,data}` object is reserved for the
    // JSON-RPC leg.
    //
    // §S3 (D-GM-2 resolved): the JSON-RPC leg now transmits the FULL structured
    // error. `dispatch_invoke_jsonrpc` calls `CoreError::to_json_rpc()` for
    // migrated core methods, so capability_denied=1001 / read_only=1002 /
    // path_traversal=1003 / … reach the client intact (asserted by the S7
    // conformance suite). This LEGACY leg stays message-only on purpose: old
    // browser Remote clients consume the bare `_error` string and
    // must not change. Paired anchor: `lanWsAdapter.handleInbound`.
    fn core_result_to_envelope(
        r: Result<serde_json::Value, ridge_core::CoreError>,
    ) -> serde_json::Value {
        match r {
            Ok(v) => serde_json::json!({ "_result": v }),
            Err(e) => serde_json::json!({ "_error": e.to_command_string() }),
        }
    }

    // Most commands need a real Tauri context. The stashed handle gives us both
    // a managed `State<AppState>` (same Arcs as `state`) and an `AppHandle`.
    let handle = match state.app_handle.get() {
        Some(h) => h.clone(),
        None => return serde_json::json!({ "_error": "host application handle not ready" }),
    };
    // `handle.state::<AppState>()` panics if AppState isn't managed; guard once so
    // a misconfigured host degrades to an error instead of aborting the WS task.
    if handle.try_state::<AppState>().is_none() {
        return serde_json::json!({ "_error": "host application state unavailable" });
    }

    match cmd {
        // ── Filesystem (read-only: S5 migrated into ridge-core) ──
        // `get_file_tree` / `get_directory_children` / `path_exists` /
        // `read_file` / `read_file_for_editor` now live in `ridge_core::fs::
        // commands`; route them through the unified `ridge_core::dispatch` so the
        // LAN host shares the exact same implementation + capability gate (D8)
        // the headless host uses. The core error maps onto the legacy
        // `{_result|_error}` envelope below — wire behaviour is unchanged.
        "get_file_tree"
        | "get_directory_children"
        | "path_exists"
        | "read_file"
        | "read_file_for_editor" => {
            let ctx = crate::remote_bridge::remote_ctx(&handle, state, "remote");
            core_result_to_envelope(ridge_core::dispatch(cmd, args.clone(), &ctx))
        }
        "write_file" => unit(project::write_file(s(args, "path"), s(args, "content")).await),
        "apply_file_edits" => match from_arg::<Vec<project::TextEdit>>(args, "edits") {
            Ok(edits) => unit(project::apply_file_edits(s(args, "path"), edits).await),
            Err(e) => serde_json::json!({ "_error": format!("invalid edits: {e}") }),
        },
        "rename_path" => unit(project::rename_path(s(args, "from"), s(args, "to"))),
        "delete_path" => unit(project::delete_path(s(args, "path")).await),
        "create_file" => unit(project::create_file(s(args, "path"))),
        "create_directory" => unit(project::create_directory(s(args, "path"))),
        "copy_path" => unit(
            project::copy_path(s(args, "from"), s(args, "to"), bool_opt(args, "overwrite")).await,
        ),
        "move_path" => unit(project::move_path(s(args, "from"), s(args, "to")).await),
        "reveal_in_file_manager" => unit(project::reveal_in_file_manager(s(args, "path"))),
        // `read_file_for_editor` is handled by the read-only ridge-core arm above.
        "get_current_project" => val(project::get_current_project(handle.state())),

        // ── Filesystem / git watchers (live fs-changed / scm refresh) ──
        "start_watching_paths" => match from_arg::<Vec<fs_watch::WatchSpec>>(args, "roots") {
            Ok(roots) => {
                unit(fs_watch::start_watching_paths(roots, handle.clone(), handle.state()).await)
            }
            Err(e) => serde_json::json!({ "_error": format!("invalid roots: {e}") }),
        },
        "start_watching_repos" => unit(
            watch::start_watching_repos(vec_s(args, "roots"), handle.clone(), handle.state()).await,
        ),

        // ── Pane / terminal ──
        "get_pane_layout" => val(pane::get_pane_layout(handle.state())),
        "get_pane_layout_for" => val(pane::get_pane_layout_for(
            handle.state(),
            s(args, "workspaceId"),
        )),
        "split_pane" => {
            val(pane::split_pane(handle.state(), s(args, "paneId"), s(args, "direction")).await)
        }
        "dock_pane" => unit(
            pane::dock_pane(
                handle.state(),
                s(args, "sourcePaneId"),
                s(args, "targetPaneId"),
                s(args, "region"),
            )
            .await,
        ),
        "close_pane" => unit(pane::close_pane(handle.state(), s(args, "paneId")).await),
        "toggle_mode" => match from_arg(args, "mode") {
            Ok(mode) => unit(pane::toggle_mode(handle.state(), s(args, "paneId"), mode).await),
            Err(e) => serde_json::json!({ "_error": format!("invalid mode: {e}") }),
        },
        "set_split_ratios_at_path" => match (
            from_arg::<Vec<usize>>(args, "path"),
            from_arg::<Vec<f32>>(args, "ratios"),
        ) {
            (Ok(p), Ok(r)) => unit(pane::set_split_ratios_at_path(handle.state(), p, r).await),
            _ => serde_json::json!({ "_error": "invalid split-ratio args" }),
        },
        "set_split_ratios_batch" => match from_arg(args, "updates") {
            Ok(updates) => unit(pane::set_split_ratios_batch(handle.state(), updates).await),
            Err(e) => serde_json::json!({ "_error": format!("invalid updates: {e}") }),
        },
        "create_pane" => unit(
            terminal::create_pane(handle.state(), s(args, "paneId"), opt_s(args, "shell")).await,
        ),
        "activate_pane_pty" => unit(
            terminal::activate_pane_pty(
                handle.state(),
                handle.clone(),
                s(args, "workspaceId"),
                s(args, "paneId"),
                u16_opt(args, "rows"),
                u16_opt(args, "cols"),
            )
            .await,
        ),
        "change_pane_shell" => unit(
            terminal::change_pane_shell(
                handle.state(),
                s(args, "paneId"),
                s(args, "shell"),
                args.get("args")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            )
            .await,
        ),
        "write_to_pty" => {
            unit(
                terminal::write_to_pty(
                handle.state(),
                s(args, "paneId"),
                s(args, "data"),
                opt_s(args, "workspaceId"),
                opt_s(args, "inputSourceId"),
                args.get("inputSequence").and_then(|value| value.as_u64()),
                )
                .await,
            )
        }
        "resize_pane" => {
            let workspace_id = s(args, "workspaceId");
            let pane_id = s(args, "paneId");
            let rows = u16_arg(args, "rows");
            let cols = u16_arg(args, "cols");
            let result = terminal::resize_pane_remote(
                handle.state::<AppState>().inner(),
                handle.clone(),
                workspace_id.clone(),
                pane_id.clone(),
                rows,
                cols,
                bool_opt(args, "isAlt"),
                bool_opt(args, "isInlineTui"),
            )
            .await;
            if result.is_ok() {
                broadcast_invoke_resize(state, &workspace_id, &pane_id, rows, cols);
            }
            unit(result)
        }
        "detect_available_shells" => plain(terminal::detect_available_shells()),
        "get_shell_history" => val(terminal::get_shell_history(s(args, "shellKind")).await),

        // ── Native (headless) tmux session discovery ──
        // `list` is read-only; `summon` adopts a headless session into the
        // caller's viewed workspace (`workspaceId` from the remote client; the
        // desktop omits it → active workspace).
        "list_native_sessions" => plain(terminal::list_native_sessions()),
        "summon_native_session" => val(terminal::summon_native_session(
            handle.state(),
            handle.clone(),
            s(args, "socket"),
            s(args, "target"),
            opt_s(args, "workspaceId"),
        )
        .await),
        // `new_headless_session` 起一个新无头会话（mutating）；`terminate_native_session`
        // 真正杀掉会话（mutating，destructive）——两者经 MUTATING_METHODS 只读门控，
        // 只读会话会被后端 pre-check 挡掉。
        "new_headless_session" => val(terminal::new_headless_session(
            opt_s(args, "name"),
            opt_s(args, "cwd"),
        )),
        "terminate_native_session" => val(terminal::terminate_native_session(
            s(args, "socket"),
            s(args, "target"),
        )),

        // ── Workspace (live) ──
        // `list_workspaces` is read-only and required by the desktop SPA
        // controller's boot (`refreshWorkspaces`): without it the web-remote
        // controller's `invoke('list_workspaces')` throws "command not available
        // remotely", aborting workspace init and stranding the controller on
        // "请先选择一个工作区". Mirrors `get_active_workspace_id` (val + State).
        "list_workspaces" => val(workspace::list_workspaces(handle.state())),
        "get_active_workspace_id" => val(workspace::get_active_workspace_id(handle.state())),
        "switch_workspace" => unit(workspace::switch_workspace(
            handle.state(),
            s(args, "workspaceId"),
        )),
        "create_workspace" => val(workspace::create_workspace(
            handle.state(),
            opt_s(args, "name"),
        )),
        "close_workspace" => unit(workspace::close_workspace(
            handle.state(),
            s(args, "workspaceId"),
        )),
        "rename_workspace" => unit(workspace::rename_workspace(
            handle.state(),
            s(args, "workspaceId"),
            s(args, "name"),
        )),
        "reorder_workspaces" => unit(workspace::reorder_workspaces(
            handle.state(),
            usize_arg(args, "fromIndex"),
            usize_arg(args, "toIndex"),
        )),

        // ── Workspace (persistence / .ridge) ──
        "save_workspace" => val(workspace::save_workspace(
            handle.clone(),
            handle.state(),
            opt_s(args, "name"),
        )),
        "list_saved_workspaces" => val(workspace::list_saved_workspaces(handle.clone())),
        "delete_saved_workspace" => unit(workspace::delete_saved_workspace(
            handle.clone(),
            s(args, "id"),
        )),
        "rename_saved_workspace" => unit(workspace::rename_saved_workspace(
            handle.clone(),
            s(args, "id"),
            s(args, "name"),
        )),
        "list_workspace_save_info" => val(ridge_file::list_workspace_save_info(handle.state())),
        "delete_workspace_file" => unit(ridge_file::delete_workspace_file(
            handle.clone(),
            handle.state(),
            s(args, "workspaceId"),
        )),
        "get_default_workspace_save_dir" => val(ridge_file::get_default_workspace_save_dir()),
        "list_saved_workspace_files" => val(ridge_file::list_saved_workspace_files()),
        "save_workspace_to_file" => val(ridge_file::save_workspace_to_file(
            handle.clone(),
            handle.state(),
            s(args, "workspaceId"),
            s(args, "name"),
            opt_s(args, "path"),
        )),
        "open_workspace_from_file" => val(ridge_file::open_workspace_from_file(
            handle.clone(),
            handle.state(),
            s(args, "path"),
        )),
        "get_restore_set" => val(ridge_file::get_restore_set(handle.clone())),
        "list_recent_workspaces" => val(ridge_file::list_recent_workspaces(handle.clone())),
        "clear_recent_workspaces" => unit(ridge_file::clear_recent_workspaces(handle.clone())),
        "get_last_opened_workspace_path" => {
            val(ridge_file::get_last_opened_workspace_path(handle.clone()))
        }
        "get_startup_context" => val(ridge_file::get_startup_context(handle.state())),
        "browse_directory" => val(ridge_file::browse_directory(opt_s(args, "path"))),

        // ── Teammate（P1 控制台 MVP）──
        // 只读 roster 快照，与桌面 Agent Center 同一投影（无 MCP endpoint/token）。
        // HITL 裁决与 Agent 配置写路径刻意不路由（P2 前不入 allowlist）。
        // AC4-C10 / C55: admit_remote_method (canonicalize + desktop-privileged deny).
        cmd if ridge_core::protocol_guard::admit_remote_method(cmd).is_err() => {
            let err = ridge_core::protocol_guard::admit_remote_method(cmd)
                .err()
                .unwrap_or_else(|| format!("remote denied: {cmd}"));
            val::<()>(Err(err))
        }
        "get_teammate_topology" => val(
            crate::commands::teammate::get_teammate_topology(
                opt_s(args, "workspaceId"),
                handle.state(),
            )
            .await,
        ),
        // P2 阶段 1：脱敏待审批快照（无 action 全文）。
        "list_hitl_pending" => val(crate::commands::teammate::list_hitl_pending()),
        "list_hitl_audit_remote" => {
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            val(Ok(crate::commands::teammate::list_hitl_audit_remote(limit)))
        }
        // P2 阶段 2：远端裁决（nonce 单次消费；桌面版 resolve_hitl_request 仍不路由）。
        "resolve_hitl_remote" => val(crate::commands::teammate::resolve_hitl_remote(
            s(args, "id"),
            s(args, "nonce"),
            s(args, "verdict"),
        )),
        // R19：只读编排健康（suspended / pending）——与桌面 Agent Center badge 同源。
        "get_orchestration_health" => val(Ok(crate::commands::teammate::get_orchestration_health())),
        // Resume a recorded Agent session without shell interpolation. The
        // host resolves the registered profile, validates CWD, creates the
        // pane, and activates the structured PTY before returning its id.
        "resume_agent_session" => {
            let result = Uuid::parse_str(&s(args, "workspaceId"))
                .map_err(|_| "invalid workspaceId".to_string())
                .and_then(|workspace_id| {
                    pane::remote_resume_agent_pane(
                        state,
                        workspace_id,
                        s(args, "agent"),
                        s(args, "sessionId"),
                        s(args, "cwd"),
                    )
                    .and_then(|pane_id| {
                        terminal::activate_pane_pty_state(
                            state,
                            None,
                            workspace_id,
                            pane_id,
                            None,
                            None,
                        )
                        .map(|()| (workspace_id, pane_id))
                    })
                    .map_err(|e| e.to_string())
                });
            match result {
                Ok((workspace_id, pane_id)) => {
                    let _ = state.remote_structural_tx.send(
                        crate::types::RemoteStructuralEvent::PanesChanged { workspace_id },
                    );
                    let _ = state.event_tx.try_send(
                        crate::types::GlobalEvent::PaneTreeChanged { workspace_id },
                    );
                    val(Ok(serde_json::json!({ "paneId": pane_id.to_string() })))
                }
                Err(e) => val::<serde_json::Value>(Err(e)),
            }
        }
        "read_agent_recent_replies" => plain(
            project::read_agent_recent_replies(
                vec_s(args, "projectPaths"),
                Some(usize_opt(args, "limit").unwrap_or(40).clamp(1, 100)),
            )
            .await,
        ),
        "set_teammate_groups" => match from_arg::<serde_json::Value>(args, "groups") {
            Ok(groups) => val(crate::commands::teammate::set_teammate_groups(
                s(args, "workspaceId"),
                groups,
            )),
            Err(e) => val::<()>(Err(format!("invalid groups: {e}"))),
        },
        // iter-61：远端把某 pane 标记/取消标记为 agent（工作区弹层「标记」按钮）。
        // 与桌面 SplitContainer 同一对命令；只改 teammate 侧表，不 spawn 进程。
        "register_teammate_agent" => unit(
            crate::commands::pane::register_teammate_agent(
                handle.state(),
                handle.clone(),
                s(args, "workspaceId"),
                s(args, "paneId"),
                s(args, "agentId"),
            )
            .await,
        ),
        "release_teammate_agent" => unit(
            crate::commands::pane::release_teammate_agent(
                handle.state(),
                handle.clone(),
                s(args, "workspaceId"),
                s(args, "paneId"),
            )
            .await,
        ),

        // ── Theme / settings (S1: migrated into ridge-core) ──
        // These three handlers now live in `ridge_core`; route them through
        // the unified `ridge_core::dispatch` so the LAN host shares the exact
        // same implementation + capability gate (D8) the headless host will.
        // The core's `{code,message,data}` error maps onto the legacy
        // `{_result|_error}` WS envelope below — wire behaviour is unchanged.
        "get_theme_data" | "set_active_theme" | "set_user_default_cwd" => {
            let ctx = crate::remote_bridge::remote_ctx(&handle, state, "remote");
            core_result_to_envelope(ridge_core::dispatch(cmd, args.clone(), &ctx))
        }

        // ── Search ── (S5: `text_search` migrated into ridge-core)
        // Routes through the unified dispatch (the `search` alias shares the
        // same handler). camelCase arg keys are read by the core directly.
        "text_search" => {
            let ctx = crate::remote_bridge::remote_ctx(&handle, state, "remote");
            core_result_to_envelope(ridge_core::dispatch(cmd, args.clone(), &ctx))
        }
        "filename_search" => {
            val(project::filename_search(s(args, "root"), s(args, "pattern")).await)
        }
        "text_search_diagnostics" => plain(project::text_search_diagnostics(
            Some(vec_s(args, "includeGlobs")),
            Some(vec_s(args, "excludeGlobs")),
        )),
        "replace_in_files" => val(project::replace_in_files(
            s(args, "root"),
            s(args, "search"),
            s(args, "replace"),
            vec_s(args, "files"),
            bool_opt(args, "caseSensitive"),
            bool_opt(args, "useRegex"),
        )
        .await),

        // ── Git (read) ──
        "find_git_repo_root" => plain(git::find_git_repo_root(s(args, "path"))),
        "find_git_repos_below" => {
            plain(git::find_git_repos_below(s(args, "path"), usize_opt(args, "maxDepth")).await)
        }
        "get_scm_status" => val(git::get_scm_status(s(args, "repoRoot"), opt_s(args, "slot")).await),
        "get_git_info_with_cwd" => val(git::get_git_info_with_cwd(s(args, "cwd")).await),
        "get_git_commits_paginated" => val(git::get_git_commits_paginated(
            s(args, "repoRoot"),
            u32_arg(args, "offset"),
            u32_arg(args, "limit"),
        )
        .await),
        "git_list_branches" => val(git::git_list_branches(s(args, "repoRoot")).await),
        "git_diff_summary" => val(git::git_diff_summary(s(args, "repoRoot"), opt_s(args, "slot")).await),
        "git_stash_list" => val(git::git_stash_list(s(args, "repoRoot")).await),
        "git_get_file_versions" => val(git::git_get_file_versions(
            s(args, "repoRoot"),
            s(args, "path"),
            bool_opt(args, "cached"),
        )
        .await),
        // §web-remote: commit-diff tabs (FileEditor `loadDiff` with a commit hash)
        // were missing from the allowlist → "command not available remotely".
        "git_get_file_versions_at_commit" => val(git::git_get_file_versions_at_commit(
            s(args, "repoRoot"),
            s(args, "path"),
            s(args, "hash"),
        )
        .await),
        "git_op_in_progress" => plain(git::git_op_in_progress(s(args, "repoRoot"))),
        "git_fetch" => unit(git::git_fetch(s(args, "repoRoot")).await),

        // ── Git (mutating; mirrors dispatch_data_request) ──
        "git_stage" => unit(git::git_stage(s(args, "repoRoot"), vec_s(args, "paths")).await),
        "git_unstage" => unit(git::git_unstage(s(args, "repoRoot"), vec_s(args, "paths")).await),
        "git_commit" => unit(
            git::git_commit(
                s(args, "repoRoot"),
                s(args, "message"),
                bool_opt(args, "amend"),
            )
            .await,
        ),
        "git_pull" => unit(git::git_pull(s(args, "repoRoot")).await),
        "git_push" => unit(git::git_push(s(args, "repoRoot"), bool_opt(args, "setUpstream")).await),
        "git_sync" => unit(git::git_sync(s(args, "repoRoot")).await),
        "git_checkout" => unit(
            git::git_checkout(
                s(args, "repoRoot"),
                s(args, "branch"),
                bool_opt(args, "create"),
                None,
            )
            .await,
        ),
        "git_revert" => unit(git::git_revert(s(args, "repoRoot"), s(args, "hash")).await),
        "git_cherry_pick" => unit(git::git_cherry_pick(s(args, "repoRoot"), s(args, "hash")).await),
        "git_reset" => {
            unit(git::git_reset(s(args, "repoRoot"), s(args, "commit"), s(args, "mode")).await)
        }
        "git_create_tag" => unit(
            git::git_create_tag(
                s(args, "repoRoot"),
                s(args, "name"),
                None,
                opt_s(args, "message"),
            )
            .await,
        ),
        "git_discard" => unit(git::git_discard(s(args, "repoRoot"), vec_s(args, "paths")).await),
        "git_clean_untracked" => {
            unit(git::git_clean_untracked(s(args, "repoRoot"), Vec::new()).await)
        }

        other => {
            tracing::warn!(target: "ridge::remote", cmd = %other, "invoke-request: command not in allowlist");
            serde_json::json!({ "_error": format!("command not available remotely: {}", other) })
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §S3 unified line protocol — JSON-RPC 2.0 leg (additive, backward-compatible).
//
// The LAN host historically spoke a bespoke envelope: invoke as
// `{type:'invoke-request', cmd, args, _reqId}` → `{type:'invoke-result',
// _reqId, _result|_error}`, control as flat `{type:'…', …}` frames. That LEGACY
// leg is left byte-for-byte unchanged (both browser Remote shapes depend
// on it). This section adds a *parallel* JSON-RPC 2.0 leg per the S0 contract
// (`docs/contracts/ridge-cloud-protocol.md` §7.0/§7.3/§7.4):
//
//   request       { "jsonrpc":"2.0", "id":…, "method":…, "params":… }
//   success resp  { "jsonrpc":"2.0", "id":…, "result":… }
//   error resp    { "jsonrpc":"2.0", "id":…, "error":{ code, message, data } }
//   notification  { "jsonrpc":"2.0", "method":…, "params":… }   (no id)
//   $/hello       D9 version + capability handshake
//   $/cancel      cancel an in-flight request by id
//
// The host replies in the SAME shape it received: a JSON-RPC request gets a
// JSON-RPC response; a legacy invoke-request gets the legacy result. A frame is
// treated as JSON-RPC iff `parsed["jsonrpc"] == "2.0"`.
// ════════════════════════════════════════════════════════════════════════════

/// Protocol version this host implements (D9). The controller SPA negotiates
/// the highest common version; v1 is the only version today.
const REMOTE_PROTOCOL_VERSION: u64 = 1;

/// Capabilities this host advertises in the `$/hello` handshake (D9). The
/// controller intersects this with its own set and greys out missing panels.
/// `pane`/`invoke` are transport-level; the rest mirror the command families in
/// `REMOTE_ALLOWLIST` (the capability *gate* for execution is still D8 — these
/// only drive which controller panels are shown, per S0 contract §7.3).
const HOST_CAPABILITIES: &[&str] = &[
    "pane",
    "invoke",
    "fs",
    "git",
    "search",
    "workspace",
    "theme",
    "teammate",
];

/// Methods already migrated into `ridge-core` (mirrors the dedicated arm in
/// `dispatch_invoke_request`). For these the JSON-RPC leg passes the FULL
/// `CoreError::to_json_rpc()` `{code,message,data}` object through — resolving
/// the legacy "message-only" error-code loss documented at decision **D-GM-2**.
const CORE_MIGRATED_METHODS: &[&str] = &[
    // S1
    "get_theme_data",
    "set_active_theme",
    "set_user_default_cwd",
    // S5 — read-only filesystem + search
    "get_file_tree",
    "get_directory_children",
    "path_exists",
    "read_file",
    "read_file_for_editor",
    "text_search",
    "search",
    // ── #19 phase-2 activation（2026-07-11，用户授权 + 真机验收）──────────────
    // 本会话已把这些命令迁入 ridge-core（core handler + dispatch arm + 端口，ridge-core
    // 单测过）。加入本表 → 远端 JSON-RPC 路由改经 ridge_core::dispatch（统一实现），
    // 不再走 legacy 直连 arm。dispatch 内部仍强制 allowlist（能力门不变）。
    // **批 1：只读**（幂等、低后果，先上供真机验证等价；写批随后）。
    // 搜索/shell 只读
    "filename_search",
    "text_search_diagnostics",
    "detect_available_shells",
    "get_shell_history",
    "browse_directory",
    // 工作区/pane/terminal 只读
    "get_workspace_snapshot",
    "get_active_workspace_id",
    "list_workspaces",
    "get_pane_layout",
    "get_pane_layout_for",
    "get_pane_scrollback_tail",
    "get_pane_scrollback_before",
    "list_native_sessions",
    // git 只读
    "git_op_in_progress",
    "get_git_info_with_cwd",
    "get_scm_status",
    "git_list_branches",
    "find_git_repos_below",
    "find_git_repo_root",
    "git_blame",
    "git_file_log",
    "git_diff_file",
    "git_diff_summary",
    "git_get_file_versions",
    "get_git_commits_paginated",
    // ── #19 phase-2 批2：写命令（sync 迁入 core；已列 MUTATING_METHODS，只读会话被
    // dispatch 门拒）。远端 JSON-RPC 改经 dispatch → 我方 handler（逐字对齐桌面逻辑）。
    // 破坏性（close/delete）与 spawn（create/split）尤须**真机验收**远端行为等价 +
    // PTY 生命周期/文件删除正确、无泄漏。close_pane/write_to_pty 属 async，仍走 legacy
    // （见 async-dispatch 设计，待 async dispatch 后并入）。──
    "switch_workspace",
    "reorder_workspaces",
    "rename_workspace",
    "create_workspace",
    "close_workspace",
    "save_workspace",
    "save_workspace_to_file",
    "delete_workspace_file",
    "resize_pane",
    "create_pane",
    "split_pane",
];

/// `ridge_core::dispatch` is synchronous by contract. Git read methods in
/// this list can wait on the shared semaphore and spawn/collect git children;
/// never run them on the remote WebSocket executor.
const CORE_GIT_DISPATCH_METHODS: &[&str] = &[
    "git_op_in_progress",
    "get_git_info_with_cwd",
    "get_scm_status",
    "git_list_branches",
    "find_git_repos_below",
    "find_git_repo_root",
    "git_blame",
    "git_file_log",
    "git_diff_file",
    "git_diff_summary",
    "git_get_file_versions",
    "get_git_commits_paginated",
];

fn is_core_git_dispatch_method(method: &str) -> bool {
    CORE_GIT_DISPATCH_METHODS.contains(&method)
}

async fn dispatch_core_git_offloaded(
    method: String,
    args: serde_json::Value,
    ctx: ridge_core::Ctx,
    git_slot: Option<(String, u64)>,
) -> Result<Result<serde_json::Value, ridge_core::CoreError>, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || {
        match git_slot {
            Some((slot, generation)) => {
                ridge_core::commands::git::with_git_sync_request_generation(
                    slot,
                    generation,
                    || ridge_core::dispatch(&method, args, &ctx),
                )
            }
            None => ridge_core::dispatch(&method, args, &ctx),
        }
    })
    .await
}

/// Dispatch one **JSON-RPC** invoke. Returns `Ok(result_value)` or
/// `Err(json_rpc_error_object)` where the error object is `{code,message,data}`.
///
/// §D-GM-2: for `ridge-core`-migrated methods the error is produced by
/// `CoreError::to_json_rpc()`, so the structured `code` (capability_denied=1001,
/// read_only=1002, path_traversal=1003, …) and `data.kind` survive end-to-end —
/// no longer collapsed to a bare message. For not-yet-migrated legacy methods
/// the backing handler only produces a `String`, so its error maps to the
/// JSON-RPC `INTERNAL_ERROR` (-32603) code with that message preserved; the
/// `code`/`data` fidelity improves automatically as each handler migrates into
/// `ridge-core` (S1 ledger).
async fn dispatch_invoke_jsonrpc(
    cmd: &str,
    args: &serde_json::Value,
    state: &AppState,
) -> Result<serde_json::Value, serde_json::Value> {
    // JSON-RPC-native path for migrated core commands: pass `to_json_rpc()`
    // through verbatim (the D-GM-2 fix).
    if CORE_MIGRATED_METHODS.contains(&cmd) {
        let handle = match state.app_handle.get() {
            Some(h) => h.clone(),
            None => {
                return Err(serde_json::json!({
                    "code": ridge_core::error::CODE_HOST_UNAVAILABLE,
                    "message": "host application handle not ready",
                    "data": { "kind": "host_unavailable" },
                }))
            }
        };
        let ctx = crate::remote_bridge::remote_ctx(&handle, state, "remote");
        if is_core_git_dispatch_method(cmd) {
            return match dispatch_core_git_offloaded(
                cmd.to_string(),
                args.clone(),
                ctx,
                ridge_core::commands::git::current_git_request_slot(),
            )
            .await
            {
                Ok(result) => result.map_err(|e| e.to_json_rpc()),
                Err(e) => Err(serde_json::json!({
                    "code": JSON_RPC_INTERNAL_ERROR,
                    "message": format!("core git dispatch task failed: {e}"),
                    "data": { "kind": "internal" },
                })),
            };
        }
        return ridge_core::dispatch(cmd, args.clone(), &ctx).map_err(|e| e.to_json_rpc());
    }

    // Legacy methods: reuse the single source of command routing
    // (`dispatch_invoke_request`) and translate its `{_result|_error}` envelope
    // into the JSON-RPC result/error shape. Un-migrated handlers only carry a
    // message, so the error code is the generic INTERNAL_ERROR (-32603).
    let envelope = dispatch_invoke_request(cmd, args, state).await;
    if let Some(err) = envelope.get("_error") {
        let message = err.as_str().unwrap_or("command failed").to_string();
        Err(serde_json::json!({
            "code": JSON_RPC_INTERNAL_ERROR,
            "message": message,
            "data": { "kind": "internal" },
        }))
    } else {
        Ok(envelope
            .get("_result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }
}

/// Standard JSON-RPC 2.0 reserved error codes used by the host leg.
const JSON_RPC_INVALID_REQUEST: i64 = -32600;
const JSON_RPC_INTERNAL_ERROR: i64 = -32603;

/// Build a JSON-RPC success response frame.
fn jsonrpc_result(id: &serde_json::Value, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC error response frame from a `{code,message,data}` object.
fn jsonrpc_error(id: &serde_json::Value, error: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// Negotiate the `$/hello` handshake (D9). Given the controller's announced
/// `protocolVersion` + `capabilities`, return either the host's reply
/// `$/hello` notification (on a compatible version) or a `$/bye` notification
/// (no common version) for the caller to send. Capabilities are intersected so
/// the controller greys out panels this host does not serve.
fn negotiate_hello(params: &serde_json::Value) -> serde_json::Value {
    let peer_version = params
        .get("protocolVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // Highest common version. v1 host supports exactly {1}; the common max with
    // any peer that also supports v1 is 1, otherwise there is no overlap.
    if peer_version < REMOTE_PROTOCOL_VERSION {
        return serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/bye",
            "params": { "reason": "protocol-version-mismatch" },
        });
    }
    let peer_caps: std::collections::HashSet<String> = params
        .get("capabilities")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let agreed: Vec<&str> = HOST_CAPABILITIES
        .iter()
        .copied()
        .filter(|c| peer_caps.is_empty() || peer_caps.contains(*c))
        .collect();
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "$/hello",
        "params": {
            "protocolVersion": REMOTE_PROTOCOL_VERSION,
            "capabilities": agreed,
        },
    })
}

// ────────────────────────────────────────────────────────────────────────────
// §S3 · D10 attach screen-snapshot — SCAFFOLD ONLY (full per-pane screen buffer
// is deferred to S5; see contract §7.4). raw-byte (`0x10`) PTY output is NOT
// replayable, so a controller that attaches late or reconnects cannot recover
// history from the live stream. D10's terminal answer: the host keeps a
// per-pane SCREEN BUFFER and, on `subscribe-pane`, sends a SNAPSHOT first, then
// resumes the raw stream.
//
// CURRENT STATE (partial precursor, already shipping): `subscribe-pane` replays
// up to 64 KiB of recent scrollback as raw bytes before the live stream (see the
// `subscribe-pane` arm + the desync-resync path). That gives the kernel enough
// to repaint, which is why the LAN leg works today. It is byte-scrollback, NOT a
// rendered screen snapshot, so it does not capture alt-screen state / cursor /
// scroll-region precisely.
//
// SCAFFOLD this section defines (no behaviour change yet):
//   • [`PaneSnapshotFrame`] — the JSON control-frame shape a future host will
//     emit as the FIRST response to `subscribe-pane`, before raw `0x10` bytes.
//   • The接入点 (integration point) is marked inline in the `subscribe-pane`
//     handler with `// §D10 接入点`.
//
// FOLLOW-UP IMPLEMENTATION NOTES (S5 / per-pane screen buffer):
//   1. State: add a per-pane rendered-screen buffer on the PTY handle (reuse the
//      existing `parser` / vte `Terminal` — `terminals[pane].parser` already
//      tracks screen state for `title()`; expose a `screen_snapshot()` that emits
//      a repaint sequence incl. cursor pos, alt-screen flag, scroll region).
//   2. Emit: in the `subscribe-pane` arm, BEFORE the scrollback send, push a
//      `PaneSnapshotFrame` carrying that repaint sequence + the pane's LOCKED
//      render size (D11 shared property — see `lockedRows`/`lockedCols`).
//   3. Reconnect: the L2 client (rpcClient.onReconnected) re-sends
//      `subscribe-pane` per previously-subscribed pane; the host replies snapshot
//      → raw, exactly as a fresh attach (already wired client-side, bridge.ts).
//   4. Bound: the screen buffer is O(rows×cols), naturally bounded — unlike the
//      abandoned per-sub 11 MB delta PaneParser (the OOM that motivated raw-byte).
//   5. Multi-client (D11): the screen buffer is a SHARED pane property; every
//      attaching controller gets the same snapshot. Locked size rides the snapshot.
// ────────────────────────────────────────────────────────────────────────────

/// **D10 SCAFFOLD** — the JSON control frame a future host emits as the first
/// response to `subscribe-pane`, carrying the current rendered screen so a
/// late/reconnecting controller can repaint before consuming the live raw
/// stream. Defined now so the wire shape is fixed for S5; not yet emitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Wire-shape scaffold; emitted by S5's per-pane screen buffer.
pub struct PaneSnapshotFrame {
    /// Discriminator on the control channel: always `"pane-snapshot"`.
    #[serde(rename = "type")]
    pub frame_type: String,
    /// Pane this snapshot belongs to (UUID string).
    #[serde(rename = "paneId")]
    pub pane_id: String,
    /// Terminal repaint sequence (raw escape bytes, base64) reconstructing the
    /// current screen: cursor position, alt-screen state, scroll region, content.
    /// The controller feeds this to its wasm kernel before the raw `0x10` stream.
    pub screen: String,
    /// The pane's LOCKED render size (D11 shared property). `None` until a
    /// controller has claimed a size. Rides the snapshot so every attaching
    /// controller renders at the same grid.
    #[serde(rename = "lockedRows", skip_serializing_if = "Option::is_none")]
    pub locked_rows: Option<u16>,
    #[serde(rename = "lockedCols", skip_serializing_if = "Option::is_none")]
    pub locked_cols: Option<u16>,
}

#[cfg(test)]
mod jsonrpc_tests {
    //! Pure host-side wire-shape tests for the §S3 JSON-RPC leg. These cover the
    //! envelope builders and the D9 `$/hello` negotiation without needing a live
    //! `AppState` / Tauri runtime, so they compile + run under `cargo test -p
    //! ridge`. (On this machine the cdylib `cargo test` crashes 0xc0000139, so
    //! they are verified by `cargo check` here and runnable by the user post-
    //! rebuild; the S7 TS conformance suite covers the same negotiation E2E.)
    use super::*;

    fn raw(pane_id: Uuid) -> crate::types::RemotePtyEvent {
        crate::types::RemotePtyEvent::RawBytes {
            workspace_id: Uuid::nil(),
            pane_id,
            bytes: Arc::new(vec![1]),
        }
    }

    fn pane_id(event: ScheduledPtyEvent) -> (bool, Uuid) {
        match event {
            (
                foreground,
                crate::types::RemotePtyEvent::RawBytes { pane_id, .. },
            ) => (foreground, pane_id),
            _ => panic!("expected raw bytes"),
        }
    }

    #[tokio::test]
    async fn active_lane_overtakes_background_backlog_after_one_in_flight_frame() {
        let (active, background, mut scheduled) = spawn_remote_lane_scheduler(8);
        let low_in_flight = Uuid::new_v4();
        let low_backlog = Uuid::new_v4();
        let high = Uuid::new_v4();

        background.send(raw(low_in_flight)).await.unwrap();
        tokio::task::yield_now().await;
        background.send(raw(low_backlog)).await.unwrap();
        active.send(raw(high)).await.unwrap();

        assert_eq!(pane_id(scheduled.recv().await.unwrap()), (false, low_in_flight));
        assert_eq!(pane_id(scheduled.recv().await.unwrap()), (true, high));
        assert_eq!(pane_id(scheduled.recv().await.unwrap()), (false, low_backlog));
    }

    #[tokio::test]
    async fn data_request_registry_cancels_once_and_suppresses_stale_completion() {
        let mut registry = DataRequestRegistry::new();
        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        assert!(registry.insert(
            7,
            PendingDataRequest {
                handle,
                git_slot: "remote:test:7".into(),
            },
        ));
        assert_eq!(registry.cancel(7).as_deref(), Some("remote:test:7"));
        assert!(!registry.complete(7), "cancelled request cannot complete twice");

        // Cancellation may arrive before the request frame; it is consumed
        // exactly once when that frame eventually arrives.
        assert!(registry.cancel(8).is_none());
        assert!(registry.take_pre_cancelled(8));
        assert!(!registry.take_pre_cancelled(8));
    }

    #[tokio::test]
    async fn invoke_request_registry_cancels_once_and_bounds_duplicates() {
        let mut registry = InvokeRequestRegistry::new();
        let key = InvokeRequestKey::Legacy(7);
        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
        });
        assert!(registry.insert(
            key.clone(),
            PendingInvokeRequest {
                handle,
                git_slot: "remote:test:invoke:7".into(),
            },
        ));
        assert!(!registry.insert(
            key.clone(),
            PendingInvokeRequest {
                handle: tokio::spawn(async {}),
                git_slot: "duplicate".into(),
            },
        ));
        assert_eq!(
            registry.cancel(&key).as_deref(),
            Some("remote:test:invoke:7")
        );
        assert!(!registry.complete(&key), "cancelled invoke cannot complete twice");

        let raced = InvokeRequestKey::JsonRpc("8".into());
        assert!(registry.cancel(&raced).is_none());
        assert!(registry.take_pre_cancelled(&raced));
        assert!(!registry.take_pre_cancelled(&raced));
    }

    #[test]
    fn jsonrpc_result_frame_shape() {
        let f = jsonrpc_result(&serde_json::json!(7), serde_json::json!({"ok": true}));
        assert_eq!(f["jsonrpc"], "2.0");
        assert_eq!(f["id"], serde_json::json!(7));
        assert_eq!(f["result"], serde_json::json!({"ok": true}));
        assert!(f.get("error").is_none());
    }

    #[test]
    fn jsonrpc_error_frame_carries_code_message_data() {
        let err =
            ridge_core::CoreError::CapabilityDenied("set_remote_enabled".into()).to_json_rpc();
        let f = jsonrpc_error(&serde_json::json!(3), err);
        assert_eq!(f["id"], serde_json::json!(3));
        assert_eq!(f["error"]["code"], serde_json::json!(1001));
        assert_eq!(f["error"]["data"]["kind"], "capability_denied");
        assert!(f["error"]["message"]
            .as_str()
            .unwrap()
            .contains("set_remote_enabled"));
    }

    #[test]
    fn hello_negotiates_capability_intersection() {
        let reply = negotiate_hello(&serde_json::json!({
            "protocolVersion": 1,
            "capabilities": ["pane", "invoke", "fs"],
        }));
        assert_eq!(reply["method"], "$/hello");
        assert_eq!(reply["params"]["protocolVersion"], serde_json::json!(1));
        let caps: Vec<&str> = reply["params"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        // Only the host∩client subset is agreed.
        assert!(caps.contains(&"fs"));
        assert!(caps.contains(&"invoke"));
        assert!(!caps.contains(&"git")); // client did not request it
    }

    #[test]
    fn hello_empty_capabilities_means_all_host_caps() {
        // A peer that omits capabilities gets the host's full set (it can drive all).
        let reply = negotiate_hello(&serde_json::json!({ "protocolVersion": 1 }));
        let caps = reply["params"]["capabilities"].as_array().unwrap();
        assert_eq!(caps.len(), HOST_CAPABILITIES.len());
    }

    #[test]
    fn hello_version_mismatch_sends_bye() {
        let reply = negotiate_hello(&serde_json::json!({
            "protocolVersion": 0,
            "capabilities": ["pane"],
        }));
        assert_eq!(reply["method"], "$/bye");
        assert_eq!(reply["params"]["reason"], "protocol-version-mismatch");
    }

    struct EmptyCoreState;

    impl ridge_core::CoreState for EmptyCoreState {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct EmptyEventSink;

    impl ridge_core::EventSink for EmptyEventSink {
        fn emit(
            &self,
            _scope: ridge_core::EventScope,
            _connection: &ridge_core::ConnectionId,
            _name: &str,
            _payload: serde_json::Value,
        ) {
        }
    }

    #[test]
    fn core_git_dispatch_method_set_covers_sync_git_reads() {
        for method in [
            "get_scm_status",
            "git_list_branches",
            "git_diff_summary",
            "get_git_info_with_cwd",
            "get_git_commits_paginated",
        ] {
            assert!(is_core_git_dispatch_method(method), "{method} must be offloaded");
        }
        assert!(!is_core_git_dispatch_method("get_theme_data"));
    }

    #[tokio::test]
    async fn offloaded_git_dispatch_preserves_non_git_result() {
        let root = std::env::temp_dir().join(format!(
            "ridge-remote-git-dispatch-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ctx = ridge_core::Ctx::new(
            Arc::new(EmptyCoreState),
            Arc::new(EmptyEventSink),
            Arc::new(ridge_core::TokioSpawner),
            ridge_core::CapabilitySet::remote_default(),
        );
        let result = dispatch_core_git_offloaded(
            "get_scm_status".to_string(),
            serde_json::json!({ "repoRoot": root.to_string_lossy() }),
            ctx,
            None,
        )
        .await
        .expect("blocking task should join")
        .expect_err("non-Git path must return a core error");
        assert!(result.to_command_string().to_ascii_lowercase().contains("git"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pane_snapshot_frame_serializes_to_contract_shape() {
        let snap = PaneSnapshotFrame {
            frame_type: "pane-snapshot".into(),
            pane_id: "p1".into(),
            screen: "AAAA".into(),
            locked_rows: Some(24),
            locked_cols: Some(80),
        };
        let v = serde_json::to_value(&snap).unwrap();
        assert_eq!(v["type"], "pane-snapshot");
        assert_eq!(v["paneId"], "p1");
        assert_eq!(v["lockedRows"], serde_json::json!(24));
        assert_eq!(v["lockedCols"], serde_json::json!(80));
    }

    #[test]
    fn pty_resize_frame_carries_workspace_identity() {
        let workspace_id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let pane_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&pty_resized_message(workspace_id, pane_id, 40, 120)).unwrap();
        assert_eq!(value["type"], "pty-resized");
        assert_eq!(value["workspaceId"], workspace_id.to_string());
        assert_eq!(value["paneId"], pane_id.to_string());
        assert_eq!(value["rows"], 40);
        assert_eq!(value["cols"], 120);
    }
}
