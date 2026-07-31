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
    // panes 必须带 workspaceId：手机 MainApp / requestPaneSnapshot 丢弃无 workspaceId 的快照
    // （REQ-RDG-REMOTE-CONNECT-01：否则 rdg LAN 永远空白壳）。
    let hello = json!({ "type": "hello", "version": 1, "protocol": "ridge-remote-ws" });
    if ws_tx.send(Message::Text(hello.to_string())).await.is_err() {
        return;
    }
    let panes = build_pane_list(&workspace);
    if ws_tx
        .send(Message::Text(panes_snapshot(ws_id, panes).to_string()))
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

/// 手机/桌面 SPA 稳态 pane 快照：`workspaceId` 必填（MainApp 无则丢弃）。
fn panes_snapshot(ws_id: Uuid, panes: Vec<Value>) -> Value {
    json!({
        "type": "panes",
        "workspaceId": ws_id.to_string(),
        "panes": panes,
    })
}

/// 桌面 SPA `get_pane_layout` / `get_pane_layout_for` 的 leaf/split 树（对齐 `PaneNode`）。
fn pane_layout_from_workspace(workspace: &SharedWorkspace) -> Value {
    let w = workspace.lock().unwrap();
    if w.sessions.is_empty() {
        return json!({ "type": "leaf", "id": Uuid::nil().to_string() });
    }
    if w.sessions.len() == 1 {
        let s = &w.sessions[0];
        return json!({
            "type": "leaf",
            "id": s.id.to_string(),
            "title": s.title,
            "cwd": s.cwd,
        });
    }
    let children: Vec<Value> = w
        .sessions
        .iter()
        .map(|s| {
            json!({
                "type": "leaf",
                "id": s.id.to_string(),
                "title": s.title,
                "cwd": s.cwd,
            })
        })
        .collect();
    let n = children.len().max(1);
    let ratios: Vec<f64> = (0..n).map(|_| 1.0 / n as f64).collect();
    json!({
        "type": "split",
        "id": w.default_session_id().unwrap_or_else(Uuid::nil).to_string(),
        "direction": "horizontal",
        "children": children,
        "ratios": ratios,
    })
}

fn list_workspaces_value(ws_id: Uuid) -> Value {
    json!([{
        "id": ws_id.to_string(),
        "index": 0,
        "name": "rdg",
        "displaySeq": 1,
        "active": true,
    }])
}

/// 桌面 WEB_REMOTE 经 LanWsAdapter 发 `invoke-request` / JSON-RPC；此前 rdg 静默忽略
/// → 桌面浏览器空白壳。终端接通最小方法集（pane + 只读 workspace 呈现）。
fn dispatch_lan_invoke(
    cmd: &str,
    args: &Value,
    workspace: &SharedWorkspace,
    ws_id: Uuid,
    out_tx: &mpsc::UnboundedSender<Message>,
) -> Result<Value, String> {
    match cmd {
        "write_to_pty" | "write_pty" => {
            let data = args.get("data").and_then(Value::as_str).unwrap_or("");
            if data.is_empty() {
                return Ok(Value::Null);
            }
            let pane_id = args
                .get("paneId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .and_then(|s| Uuid::parse_str(s).ok())
                .or_else(|| workspace.lock().unwrap().default_session_id())
                .ok_or_else(|| "no pane".to_string())?;
            let w = workspace.lock().unwrap();
            let sess = w.find(pane_id).ok_or_else(|| format!("pane not found: {pane_id}"))?;
            sess.send_input(data.as_bytes())
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "resize_pane" | "resize_pty" => {
            let rows = args.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16;
            let cols = args.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16;
            let pane_id = args
                .get("paneId")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
                .or_else(|| workspace.lock().unwrap().default_session_id())
                .ok_or_else(|| "no pane".to_string())?;
            let w = workspace.lock().unwrap();
            let sess = w.find(pane_id).ok_or_else(|| format!("pane not found: {pane_id}"))?;
            sess.resize(cols, rows).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "get_active_workspace_id" => Ok(Value::String(ws_id.to_string())),
        "list_workspaces" => Ok(list_workspaces_value(ws_id)),
        "get_pane_layout" | "get_pane_layout_for" | "get_window_pane_layout" => {
            Ok(pane_layout_from_workspace(workspace))
        }
        "get_workspace_snapshot" => {
            let panes = build_pane_list(workspace);
            Ok(json!({
                "workspaceId": ws_id.to_string(),
                "name": "rdg",
                "panes": panes,
                "layout": pane_layout_from_workspace(workspace),
            }))
        }
        // 桌面 bridge.subscribePane → JSON-RPC notification `subscribe-pane`；
        // 亦可能经 invoke-request 到达。复用 legacy 订阅语义。
        "subscribe-pane" | "subscribe_pane_raw" | "register_pane_delta_channel" => {
            start_pane_subscription(args, workspace, out_tx);
            Ok(Value::Null)
        }
        // 桌面 SPA boot 可选能力：rdg 无对应实现时回空/成功，勿 error 打断「已接通」。
        "list_saved_workspaces" | "list_workspace_save_info" | "get_shell_history" => {
            Ok(json!([]))
        }
        "get_theme_data" => Ok(json!({ "themes": [] })),
        "activate_pane_pty" | "set_pane_delta_mode" | "use_global_workspace" => Ok(Value::Null),
        "create_pane" => {
            let shell = args.get("shell").and_then(Value::as_str).filter(|s| !s.is_empty());
            let cwd = args.get("cwd").and_then(Value::as_str).filter(|s| !s.is_empty());
            let mut w = workspace.lock().unwrap();
            let split_target = w.default_session_id();
            match w.create_session(shell, cwd, split_target, SplitDirection::Horizontal) {
                Ok(id) => Ok(json!({ "paneId": id.to_string(), "id": id.to_string() })),
                Err(e) => Err(e.to_string()),
            }
        }
        other => Err(format!("method not supported on rdg LAN host: {other}")),
    }
}

fn start_pane_subscription(
    v: &Value,
    workspace: &SharedWorkspace,
    out_tx: &mpsc::UnboundedSender<Message>,
) {
    let pane_id = v
        .get("paneId")
        .and_then(Value::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .or_else(|| {
            // JSON-RPC params 可能嵌在 params 里；subscribe 亦可能只带顶层字段。
            v.get("params")
                .and_then(|p| p.get("paneId"))
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok())
        });
    let Some(pane_id) = pane_id else {
        return;
    };
    let resume = v
        .get("resume")
        .and_then(Value::as_bool)
        .or_else(|| v.get("params").and_then(|p| p.get("resume")).and_then(Value::as_bool))
        .unwrap_or(false);
    let sub = {
        let w = workspace.lock().unwrap();
        w.find(pane_id)
            .map(|s| (s.subscribe_with_backlog(), s.modes_snapshot()))
    };
    if let Some(((backlog, mut rx), (modes, alt))) = sub {
        let tx = out_tx.clone();
        tokio::spawn(async move {
            // 首订阅回放：RIS + 活动模式前导 + scrollback（共享 SSOT
            // `ridge_remote::pane::pane_resync_frame`，与桌面 LAN/cloud 逐字一份）。
            // resume 时跳过（内核已存活）。空 backlog 仍挂 live，避免永久黑屏。
            if !resume {
                let frame = ridge_remote::pane::pane_resync_frame(pane_id, &backlog, &modes, alt);
                if tx.send(Message::Binary(frame)).is_err() {
                    return;
                }
            }
            while let Ok(bytes) = rx.recv().await {
                let frame = ridge_remote::pane::pane_frame(pane_id, &bytes);
                if tx.send(Message::Binary(frame)).is_err() {
                    break;
                }
            }
        });
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
    // ── JSON-RPC 2.0（LanWsAdapter 协商后或 $/hello 直达）────────────────
    if v.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
        let method = v.get("method").and_then(Value::as_str).unwrap_or("");
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        let id = v.get("id").cloned();
        if method == "$/hello" {
            // 公告 terminal/fs/search 子集；桌面 SPA 灰掉未实现面板。
            let hello = crate::rpc::negotiate_hello(&params);
            if let Some(id) = id {
                // 带 id 的 hello：先推 $/hello 通知体，再 result null（对齐 session 路径语义简化：
                // 只回 result，能力体在 result 内亦可；桌面 adapter 认 hello reply 翻 native）。
                return Some(
                    crate::rpc::result_response(
                        &id,
                        hello.get("params").cloned().unwrap_or(hello),
                    )
                    .to_string(),
                );
            }
            return Some(hello.to_string());
        }
        if method == "$/cancel" {
            return id.map(|id| crate::rpc::result_response(&id, Value::Null).to_string());
        }
        // notification（无 id）：subscribe-pane 等
        if id.is_none() {
            if method == "subscribe-pane"
                || method == "subscribe_pane_raw"
                || method == "use-global-workspace"
            {
                if method != "use-global-workspace" {
                    start_pane_subscription(&params, workspace, out_tx);
                }
            }
            return None;
        }
        let id = id.unwrap();
        return Some(match dispatch_lan_invoke(method, &params, workspace, ws_id, out_tx) {
            Ok(result) => crate::rpc::result_response(&id, result).to_string(),
            Err(msg) => crate::rpc::error_response(
                &id,
                &crate::rpc::RpcError::new(crate::rpc::JSON_RPC_METHOD_NOT_FOUND, msg),
            )
            .to_string(),
        });
    }

    // ── 桌面 WEB_REMOTE 遗留 invoke-request 信封 ────────────────────────
    if v.get("type").and_then(Value::as_str) == Some("invoke-request") {
        let cmd = v.get("cmd").and_then(Value::as_str).unwrap_or("");
        let args = v.get("args").cloned().unwrap_or(Value::Null);
        let req_id = v.get("_reqId").cloned().unwrap_or(Value::Null);
        let mut reply = match dispatch_lan_invoke(cmd, &args, workspace, ws_id, out_tx) {
            Ok(result) => json!({
                "type": "invoke-result",
                "_reqId": req_id,
                "_result": result,
            }),
            Err(msg) => json!({
                "type": "invoke-result",
                "_reqId": req_id,
                "_error": msg,
            }),
        };
        // 保持对象形状稳定，便于 adapter 解析。
        if let Some(obj) = reply.as_object_mut() {
            obj.entry("type".to_string())
                .or_insert_with(|| json!("invoke-result"));
        }
        return Some(reply.to_string());
    }

    match v["type"].as_str().unwrap_or("") {
        "ping" => Some(json!({ "type": "pong" }).to_string()),

        "list-panes" | "list-workspace-panes" => {
            let panes = build_pane_list(workspace);
            Some(panes_snapshot(ws_id, panes).to_string())
        }

        "subscribe-pane" => {
            start_pane_subscription(v, workspace, out_tx);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn panes_snapshot_always_carries_workspace_id() {
        let ws = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let snap = panes_snapshot(ws, vec![json!({"id":"p1","title":"t","cwd":null})]);
        assert_eq!(snap["type"], "panes");
        assert_eq!(snap["workspaceId"], "11111111-1111-1111-1111-111111111111");
        assert_eq!(snap["panes"][0]["id"], "p1");
    }

    #[test]
    fn invoke_request_get_active_workspace_id() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let reply = handle_text(
            &json!({
                "type": "invoke-request",
                "cmd": "get_active_workspace_id",
                "args": {},
                "_reqId": 7,
            }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("reply");
        let v: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["type"], "invoke-result");
        assert_eq!(v["_reqId"], 7);
        assert_eq!(v["_result"], "22222222-2222-2222-2222-222222222222");
    }

    #[test]
    fn jsonrpc_list_workspaces_returns_array() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let reply = handle_text(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "list_workspaces",
                "params": {},
            }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("reply");
        let v: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"][0]["id"], "33333333-3333-3333-3333-333333333333");
    }
}
