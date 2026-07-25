//! rdg 的 `RemoteHost` 实现 —— 为 headless CLI 的单工作区 `SharedWorkspace`
//! 实现共享 `ridge_remote::host` 的 `HostMeta` / `HostAuth` / `WorkspaceProvider`
//! + `RemoteHost::serve_websocket`，让 rdg LAN 远控经共享 `server_app::run` 驱动，
//! 消除私有 `ridge-lan-ws` 协议与内联 HTML（P5：协议统一为 `ridge-remote-ws`）。
//!
//! 参照桌面 `src-tauri/src/remote_host_impl.rs::DesktopHost` 的模式，但陷阱不同：
//! - 鉴权用 `crate::totp::RemoteTotp`（非桌面 `RemoteAuth`），无节流/黑名单 ——
//!   `verify_code = totp.verify`，`is_blacklisted`/`pre_verify_gate`/
//!   `post_verify_record` 全走 trait 默认实现（宽松放行）；
//! - 会话令牌由 rdg 自持一个 `ridge_remote::auth::SessionStore`（零 Tauri）；
//! - `WorkspaceProvider` 面向单工作区：switch=no-op、create=在既有工作区再开 pane、
//!   close=拒绝（最后一个工作区不可关）。

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use ridge_core::workspace::pane_tree::SplitDirection;
use ridge_remote::auth::SessionStore;
use ridge_remote::host::{HostAuth, HostError, HostMeta, RemoteHost, WorkspaceProvider, WsConn};
use ridge_remote::serve::UaServeConfig;

use crate::totp::RemoteTotp;

use super::workspace::SharedWorkspace;

/// rdg LAN 远控宿主：包装单工作区 `SharedWorkspace` + TOTP + 会话令牌 + serve 配置，
/// 供共享 `server_app` 用一份代码驱动。
pub struct RdgHost {
    pub workspace: SharedWorkspace,
    pub totp: Arc<RemoteTotp>,
    /// rdg 自持的会话令牌仓（零 Tauri）：`/verify` 成功后签发、`/ws?token=` 复用。
    pub sessions: SessionStore,
    /// 单工作区的合成 id（rdg 无多工作区，仅用于满足 `/workspace/*` 契约返回体）。
    pub ws_id: Uuid,
    pub port: u16,
    pub lan_ip: String,
    pub machine_name: String,
    pub serve_cfg: UaServeConfig,
    pub tls_enabled: bool,
    /// rdg LAN host 无桌面式全局开关：服务运行期间恒为 true（fallback / remote_gate 复用）。
    pub remote_enabled: Arc<AtomicBool>,
}

impl HostMeta for RdgHost {
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
        self.remote_enabled.clone()
    }
    fn tls_enabled(&self) -> bool {
        self.tls_enabled
    }
    fn serve_cfg(&self) -> UaServeConfig {
        self.serve_cfg.clone()
    }
}

impl HostAuth for RdgHost {
    fn verify_code(&self, code: &str) -> bool {
        self.totp.verify(code)
    }
    // is_blacklisted / pre_verify_gate / post_verify_record：rdg 无节流/黑名单，
    // 全部沿用 trait 的宽松默认实现。
    fn create_session_token(&self, device_id: &str, ip: &str) -> String {
        self.sessions.create_session_bound(device_id, ip)
    }
    fn validate_token(&self, token: &str) -> bool {
        self.sessions.validate_token(token)
    }
    fn validate_token_bound(&self, token: &str, device_id: &str, ip: &str) -> bool {
        self.sessions.validate_token_bound(token, device_id, ip)
    }
    fn validate_token_device_strict(&self, token: &str, device_id: &str, ip: &str) -> bool {
        self.sessions
            .validate_token_device_strict(token, device_id, ip)
    }
}

impl WorkspaceProvider for RdgHost {
    fn list_workspaces_json(&self) -> Value {
        // 单工作区：返回唯一工作区 + 其 pane 列表（panes 字段对不消费它的客户端无害）。
        let panes = build_pane_list(&self.workspace);
        json!({
            "workspaces": [{
                "id": self.ws_id.to_string(),
                "name": "rdg",
                "displaySeq": 1,
                "active": true,
                "panes": panes,
            }]
        })
    }

    fn switch_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        // 单工作区：切换即恒停留在同一工作区（no-op 成功）。
        Ok(json!({ "success": true, "workspaceId": workspace_id }))
    }

    fn create_workspace(&self, _name: Option<String>) -> Result<Value, HostError> {
        // 单工作区语义：新建"工作区"退化为在既有工作区里再开一个 pane（create_session）。
        let mut w = self.workspace.lock().unwrap();
        let split_target = w.default_session_id();
        match w.create_session(None, None, split_target, SplitDirection::Horizontal) {
            Ok(_) => Ok(json!({ "success": true, "workspaceId": self.ws_id.to_string() })),
            Err(e) => Err(HostError::BadRequest(e.to_string())),
        }
    }

    fn close_workspace(&self, _workspace_id: &str) -> Result<Value, HostError> {
        Err(HostError::BadRequest(
            "cannot close the last workspace".to_string(),
        ))
    }

    fn allowed_file_roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(w) = self.workspace.lock() {
            for s in &w.sessions {
                if let Some(cwd) = s.cwd.as_ref() {
                    roots.push(PathBuf::from(cwd));
                }
            }
        }
        if let Ok(cd) = std::env::current_dir() {
            roots.push(cd);
        }
        roots
    }
}

impl RemoteHost for RdgHost {
    fn serve_websocket(
        self: Arc<Self>,
        socket: WebSocket,
        _conn: WsConn,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let workspace = self.workspace.clone();
        let ws_id = self.ws_id;
        Box::pin(async move {
            run_ws(socket, workspace, ws_id).await;
        })
    }
}

// ════════════════════════════════════════════════════════════════════════════
// 每连接 WS 会话（headless，讲 `ridge-remote-ws`）：把原 lan_host.rs::run_ws 的
// 私有 `ridge-lan-ws` 逻辑重写为面向 `SharedWorkspace` 的版本。pane 帧沿用桌面的
// 16 字节二进制 UUID 前缀（wsRemote.ts `uuidFromBytes` 从 offset 0 读 16 字节）。
// ════════════════════════════════════════════════════════════════════════════

/// 快照单工作区的 pane 列表 → `[{id,title,cwd}]`。
fn build_pane_list(workspace: &SharedWorkspace) -> Vec<Value> {
    let w = workspace.lock().unwrap();
    w.sessions
        .iter()
        .map(|s| {
            json!({
                "id": s.id.to_string(),
                "title": s.title,
                "cwd": s.cwd,
            })
        })
        .collect()
}

async fn run_ws(socket: WebSocket, workspace: SharedWorkspace, ws_id: Uuid) {
    use futures_util::{SinkExt, StreamExt};
    let (mut ws_tx, mut ws_rx) = socket.split();

    // 每连接输出通道：`subscribe-pane` 派生的转发任务把 16B-前缀帧推到这里，
    // 主循环再统一写回 WS（单写者，避免对 socket 的并发 send）。
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    // 握手：hello（协议已统一为 ridge-remote-ws）+ 初始 pane 列表。
    let hello = json!({ "type": "hello", "version": 1, "protocol": "ridge-remote-ws" });
    if ws_tx.send(Message::Text(hello.to_string())).await.is_err() {
        return;
    }
    let panes = build_pane_list(&workspace);
    if ws_tx
        .send(Message::Text(
            json!({ "type": "panes", "panes": panes }).to_string(),
        ))
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            frame = out_rx.recv() => {
                match frame {
                    Some(frame) => {
                        if ws_tx.send(frame).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            incoming = ws_rx.next() => {
                let Some(Ok(msg)) = incoming else { break; };
                match msg {
                    Message::Text(text) => {
                        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue; };
                        if let Some(reply) = handle_text(&v, &workspace, ws_id, &out_tx) {
                            if ws_tx.send(Message::Text(reply)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Message::Ping(p) => {
                        if ws_tx.send(Message::Pong(p)).await.is_err() {
                            break;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            else => break,
        }
    }
}

/// 处理一条文本控制帧。返回 `Some(text)` 需回送客户端的响应；`None` 表示无需响应
/// （或已通过 `out_tx` 异步转发）。内部对 `SharedWorkspace` 的加锁均不跨 await。
fn handle_text(
    v: &Value,
    workspace: &SharedWorkspace,
    ws_id: Uuid,
    out_tx: &mpsc::UnboundedSender<Message>,
) -> Option<String> {
    match v["type"].as_str().unwrap_or("") {
        "ping" => Some(json!({ "type": "pong" }).to_string()),

        "list-panes" | "list-workspace-panes" => {
            let panes = build_pane_list(workspace);
            Some(json!({ "type": "panes", "panes": panes }).to_string())
        }

        "subscribe-pane" => {
            let pane_id = v["paneId"].as_str().and_then(|s| Uuid::parse_str(s).ok())?;
            // §keep-alive resume（与桌面 remote_host_impl 同语义）：控制端切回一个保活了
            // 镜像内核的 pane 时带 `resume:true` → 跳过带 RIS 的 resync 回放（否则会把存活
            // 内核清回尾部）；仅续 live 流，内核保全量历史。首订阅/重载则正常发 resync。
            let resume = v["resume"].as_bool().unwrap_or(false);
            // 原子取 scrollback backlog + modes 快照 + 实时订阅（同锁，加锁范围极小，
            // 不跨 await）。modes 与 backlog 在写者临界区内同步推进 → 快照一致。
            let sub = {
                let w = workspace.lock().unwrap();
                w.find(pane_id)
                    .map(|s| (s.subscribe_with_backlog(), s.modes_snapshot()))
            };
            if let Some(((backlog, mut rx), (modes, alt))) = sub {
                let tx = out_tx.clone();
                tokio::spawn(async move {
                    // 首订阅回放：RIS + 活动模式前导 + scrollback（共享 SSOT
                    // `ridge_remote::pane::pane_resync_frame`，与桌面 LAN/cloud 逐字一份），
                    // 令控制端镜像内核重建鼠标上报/alt 屏等一次性开启态——修「手机控 rdg
                    // 里 TUI 丢鼠标」。resume 时跳过（内核已存活）。backlog 与 live 接缝由
                    // 发送侧同锁保证无重无漏（见 SessionHandle::subscribe_with_backlog）。
                    if !resume && !backlog.is_empty() {
                        let frame =
                            ridge_remote::pane::pane_resync_frame(pane_id, &backlog, &modes, alt);
                        if tx.send(Message::Binary(frame)).is_err() {
                            return;
                        }
                    }
                    while let Ok(bytes) = rx.recv().await {
                        // 16B pane-id 前缀 + 原始 PTY 字节（共享 SSOT，与桌面一致）。
                        let frame = ridge_remote::pane::pane_frame(pane_id, &bytes);
                        if tx.send(Message::Binary(frame)).is_err() {
                            break;
                        }
                    }
                });
            }
            None
        }

        "stdin" => {
            let data = v["data"].as_str().unwrap_or("");
            if data.is_empty() {
                return None;
            }
            // paneId 为空则回落到默认 session（passthrough 单终端场景）。
            let pane_id = v["paneId"]
                .as_str()
                .filter(|s| !s.is_empty())
                .and_then(|s| Uuid::parse_str(s).ok())
                .or_else(|| workspace.lock().unwrap().default_session_id());
            if let Some(pane_id) = pane_id {
                let w = workspace.lock().unwrap();
                if let Some(sess) = w.find(pane_id) {
                    let _ = sess.send_input(data.as_bytes());
                }
            }
            None
        }

        // 尺寸：viewport resize 与显式 claim/refresh 在 rdg 单终端下语义一致 ——
        // 都把该 pane 的 PTY 调整到客户端网格（rdg 无桌面式共享 canvas 争用）。
        "resize" | "claim-pane" | "refresh-pane" => {
            if let Some(pane_id) = v["paneId"].as_str().and_then(|s| Uuid::parse_str(s).ok()) {
                let rows = v["rows"].as_u64().unwrap_or(24) as u16;
                let cols = v["cols"].as_u64().unwrap_or(80) as u16;
                let w = workspace.lock().unwrap();
                if let Some(sess) = w.find(pane_id) {
                    let _ = sess.resize(cols, rows);
                }
            }
            None
        }

        "create-pane" => {
            let shell = v["shell"].as_str().filter(|s| !s.is_empty());
            let cwd = v["cwd"].as_str().filter(|s| !s.is_empty());
            let mut w = workspace.lock().unwrap();
            let split_target = w.default_session_id();
            let msg = match w.create_session(shell, cwd, split_target, SplitDirection::Horizontal) {
                Ok(id) => json!({
                    "type": "create-pane-result", "success": true, "paneId": id.to_string()
                }),
                Err(e) => json!({
                    "type": "create-pane-result", "success": false, "error": e.to_string()
                }),
            };
            Some(msg.to_string())
        }

        // rdg 单工作区暂不支持关闭单个 pane（workspace 无 remove 语义）。
        "close-pane" => Some(
            json!({
                "type": "close-pane-result", "success": false, "error": "unsupported on rdg"
            })
            .to_string(),
        ),

        "list-workspaces" => Some(
            json!({
                "type": "workspaces",
                "workspaces": [{
                    "id": ws_id.to_string(),
                    "name": "rdg",
                    "displaySeq": 1,
                    "active": true,
                }]
            })
            .to_string(),
        ),

        "switch-workspace" => {
            let id = v["workspaceId"].as_str().unwrap_or("");
            Some(
                json!({
                    "type": "switch-workspace-result", "success": true, "workspaceId": id
                })
                .to_string(),
            )
        }

        "create-workspace" => {
            // 单工作区：no-op 成功（保持在既有工作区）。
            Some(
                json!({
                    "type": "create-workspace-result", "success": true, "workspaceId": ws_id.to_string()
                })
                .to_string(),
            )
        }

        "close-workspace" => Some(
            json!({
                "type": "close-workspace-result", "success": false, "error": "cannot close the last workspace"
            })
            .to_string(),
        ),

        "current-project" => {
            let path = std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(String::from))
                .unwrap_or_default();
            Some(json!({ "type": "current-project", "path": path }).to_string())
        }

        // 其余帧（list-files / list-git-status / search-files / cycle-theme …）：
        // rdg headless 暂不实现，静默忽略（客户端相应面板留空，不影响终端主链路）。
        _ => None,
    }
}
