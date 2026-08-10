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
//! - `WorkspaceProvider` 面向单工作区：switch 只接受自身 id、create=在既有工作区再开
//!   pane、close=拒绝（最后一个工作区不可关）。保存文件接口返回当前工作区的可重开句柄。

use std::future::Future;
use std::path::{Path, PathBuf};
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

use crate::fs_reuse;
use crate::totp::RemoteTotp;

use super::workspace::SharedWorkspace;

/// rdg 没有桌面端 `.ridge` 持久化文件；为避免把“无保存文件”静默投影为空，
/// 对外暴露一个不可伪造为本地路径的当前工作区句柄。客户端可把它原样传回
/// `open_workspace_from_file`，该操作幂等地返回同一工作区 id。
const RDG_WORKSPACE_URI_PREFIX: &str = "rdg://workspace/";

fn rdg_workspace_uri(ws_id: Uuid) -> String {
    format!("{RDG_WORKSPACE_URI_PREFIX}{ws_id}")
}

fn current_workspace_file(ws_id: Uuid) -> Value {
    json!({
        "name": "rdg (current workspace)",
        "path": rdg_workspace_uri(ws_id),
        "mtime_secs": 0,
    })
}

/// Resolve the same serving roots for every LAN filesystem request.  Keeping
/// this at the host boundary makes the `RdgHost` trait implementation and the
/// JSON-RPC/invoke paths share one sandbox definition instead of silently
/// widening one of them to the whole process filesystem.
fn rdg_allowed_file_roots(workspace: &SharedWorkspace) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(w) = workspace.lock() {
        for session in &w.sessions {
            if let Some(cwd) = session.cwd.as_ref() {
                roots.push(PathBuf::from(cwd));
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    roots
}

fn ensure_current_workspace(requested: &str, ws_id: Uuid) -> Result<(), String> {
    if requested == ws_id.to_string() {
        Ok(())
    } else {
        Err(format!("workspace not found: {requested}"))
    }
}

fn create_rdg_pane(
    workspace: &SharedWorkspace,
    shell: Option<&str>,
    cwd: Option<&str>,
) -> Result<Uuid, String> {
    let mut w = workspace.lock().unwrap();
    let split_target = w.default_session_id();
    w.create_session(shell, cwd, split_target, SplitDirection::Horizontal)
        .map_err(|e| e.to_string())
}

fn create_pane_result(ws_id: Uuid, pane_id: Uuid) -> Value {
    json!({
        "success": true,
        "workspaceId": ws_id.to_string(),
        "paneId": pane_id.to_string(),
        "id": pane_id.to_string(),
        "operation": "create-pane",
        // rdg owns one workspace; create-workspace is an explicit pane
        // allocation rather than a second workspace hidden behind a no-op.
        "createdWorkspace": false,
    })
}

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
        // 单工作区仍须校验目标；任意 id 成功会让客户端把陈旧/跨 host
        // 工作区误当作已切换，后续 pane 请求才以更难诊断的方式失败。
        ensure_current_workspace(workspace_id, self.ws_id).map_err(HostError::NotFound)?;
        Ok(json!({ "success": true, "workspaceId": self.ws_id.to_string() }))
    }

    fn create_workspace(&self, _name: Option<String>) -> Result<Value, HostError> {
        // 单工作区语义：新建“工作区”退化为在既有工作区里再开一个 pane，
        // 并把退化结果明确返回，避免 UI 等待一个永远不会出现的新 workspace。
        let pane_id =
            create_rdg_pane(&self.workspace, None, None).map_err(HostError::BadRequest)?;
        Ok(create_pane_result(self.ws_id, pane_id))
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        ensure_current_workspace(workspace_id, self.ws_id).map_err(HostError::NotFound)?;
        Err(HostError::BadRequest(
            "cannot close the last workspace".to_string(),
        ))
    }

    fn allowed_file_roots(&self) -> Vec<PathBuf> {
        rdg_allowed_file_roots(&self.workspace)
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
    if ws_tx
        .send(Message::Text(
            ridge_core::commands::theme::active_theme_frame().to_string(),
        ))
        .await
        .is_err()
    {
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
            let sess = w
                .find(pane_id)
                .ok_or_else(|| format!("pane not found: {pane_id}"))?;
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
            let sess = w
                .find(pane_id)
                .ok_or_else(|| format!("pane not found: {pane_id}"))?;
            sess.resize(cols, rows).map_err(|e| e.to_string())?;
            let _ = out_tx.send(Message::Text(
                json!({
                    "type": "pty-resized",
                    "workspaceId": ws_id.to_string(),
                    "paneId": pane_id.to_string(),
                    "rows": rows,
                    "cols": cols,
                })
                .to_string(),
            ));
            Ok(Value::Null)
        }
        "get_active_workspace_id" => Ok(Value::String(ws_id.to_string())),
        "list_workspaces" => Ok(list_workspaces_value(ws_id)),
        "search" => {
            let root = args.get("root").and_then(Value::as_str).unwrap_or("");
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let use_regex = args
                .get("useRegex")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let case_sensitive = args
                .get("caseSensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let roots = rdg_allowed_file_roots(workspace);
            serde_json::to_value(fs_reuse::search(
                &roots,
                root,
                query,
                use_regex,
                case_sensitive,
            ))
            .map_err(|e| format!("cannot encode search result: {e}"))
        }
        "search_files" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            if query.trim().is_empty() {
                return Ok(json!([]));
            }
            let root = args
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|path| path.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            let roots = rdg_allowed_file_roots(workspace);
            let results = fs_reuse::search(&roots, &root, query, false, false)
                .into_iter()
                .map(|hit| {
                    json!({
                        "path": hit.file,
                        "line": hit.line,
                        "column": hit.column,
                        "snippet": hit.content,
                    })
                })
                .collect::<Vec<_>>();
            Ok(json!(results))
        }
        "get_directory_children" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let roots = rdg_allowed_file_roots(workspace);
            let entries = fs_reuse::list_dir(&roots, Path::new(path))
                .map_err(|e| format!("cannot list directory: {}", e.kind()))?;
            serde_json::to_value(entries)
                .map_err(|e| format!("cannot encode directory result: {e}"))
        }
        "get_file_tree" | "read_file" | "text_search" => {
            let roots = rdg_allowed_file_roots(workspace);
            let ctx = crate::core_host::headless_ctx(&roots);
            ridge_core::dispatch(cmd, args.clone(), &ctx).map_err(|e| e.to_command_string())
        }
        "switch_workspace" => {
            let requested = args
                .get("workspaceId")
                .and_then(Value::as_str)
                .unwrap_or("");
            ensure_current_workspace(requested, ws_id)?;
            Ok(json!({ "success": true, "workspaceId": ws_id.to_string() }))
        }
        "create_workspace" => {
            let pane_id = create_rdg_pane(workspace, None, None)?;
            Ok(create_pane_result(ws_id, pane_id))
        }
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
            start_pane_subscription(args, workspace, ws_id, out_tx);
            Ok(Value::Null)
        }
        // 桌面 SPA boot 可选能力：rdg 无对应实现时回空/成功，勿 error 打断「已接通」。
        "list_saved_workspaces" | "list_workspace_save_info" | "get_shell_history" => Ok(json!([])),
        "list_saved_workspace_files" => Ok(json!([current_workspace_file(ws_id)])),
        "open_workspace_from_file" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "path is required".to_string())?;
            let current = rdg_workspace_uri(ws_id);
            if path == current {
                return Ok(Value::String(ws_id.to_string()));
            }
            if path.starts_with(RDG_WORKSPACE_URI_PREFIX) {
                return Err(format!("workspace handle not found: {path}"));
            }
            Err("rdg LAN host exposes no .ridge files; open the current workspace handle returned by list_saved_workspace_files".to_string())
        }
        "get_theme_data" => Ok(json!({ "themes": [] })),
        "activate_pane_pty" | "set_pane_delta_mode" | "use_global_workspace" => Ok(Value::Null),
        "create_pane" => {
            let shell = args
                .get("shell")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            let cwd = args
                .get("cwd")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            if let Some(requested) = args.get("workspaceId").and_then(Value::as_str) {
                ensure_current_workspace(requested, ws_id)?;
            }
            let pane_id = create_rdg_pane(workspace, shell, cwd)?;
            Ok(create_pane_result(ws_id, pane_id))
        }
        other => Err(format!("method not supported on rdg LAN host: {other}")),
    }
}

fn rdg_metadata_frame(
    workspace_id: Uuid,
    pane_id: Uuid,
    bytes: &[u8],
    utf8_pending: &mut Vec<u8>,
    osc_carryover: &mut ridge_core::pty::osc_stream::OscSignalCarryover,
) -> Option<Message> {
    let decoded = ridge_core::pty::decode::take_decoded_utf8(utf8_pending, bytes);
    if decoded.is_empty() {
        return None;
    }
    let complete = osc_carryover.push(decoded);
    if complete.is_empty() {
        return None;
    }
    let signals = ridge_core::pty::chunk::process(complete, 0, 0).emit?;
    if signals.title.is_none() && signals.cwd.is_none() {
        return None;
    }
    Some(Message::Text(
        json!({
            "type": "pty-meta",
            "workspaceId": workspace_id.to_string(),
            "paneId": pane_id.to_string(),
            "title": signals.title,
            "cwd": signals.cwd.map(|path| path.to_string_lossy().into_owned()),
        })
        .to_string(),
    ))
}

fn subscription_field<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    v.get(name)
        .or_else(|| v.get("params").and_then(|params| params.get(name)))
}

fn pane_subscription_request(v: &Value) -> Option<(Uuid, bool)> {
    let pane_id = subscription_field(v, "paneId")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())?;
    let resume = subscription_field(v, "resume")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some((pane_id, resume))
}

fn send_pane_metadata(
    tx: &mpsc::UnboundedSender<Message>,
    workspace_id: Uuid,
    pane_id: Uuid,
    bytes: &[u8],
    utf8_pending: &mut Vec<u8>,
    osc_carryover: &mut ridge_core::pty::osc_stream::OscSignalCarryover,
) -> bool {
    let Some(meta) = rdg_metadata_frame(workspace_id, pane_id, bytes, utf8_pending, osc_carryover)
    else {
        return true;
    };
    tx.send(meta).is_ok()
}

fn send_pane_bytes(
    tx: &mpsc::UnboundedSender<Message>,
    workspace_id: Uuid,
    pane_id: Uuid,
    bytes: &[u8],
    utf8_pending: &mut Vec<u8>,
    osc_carryover: &mut ridge_core::pty::osc_stream::OscSignalCarryover,
) -> bool {
    if tx
        .send(Message::Binary(ridge_remote::pane::pane_frame(
            pane_id, bytes,
        )))
        .is_err()
    {
        return false;
    }
    send_pane_metadata(
        tx,
        workspace_id,
        pane_id,
        bytes,
        utf8_pending,
        osc_carryover,
    )
}

fn start_pane_subscription(
    v: &Value,
    workspace: &SharedWorkspace,
    ws_id: Uuid,
    out_tx: &mpsc::UnboundedSender<Message>,
) {
    let Some((pane_id, resume)) = pane_subscription_request(v) else {
        return;
    };
    let sub = {
        let w = workspace.lock().unwrap();
        w.find(pane_id)
            .map(|s| (s.subscribe_with_backlog(), s.modes_snapshot()))
    };
    if let Some(((backlog, mut rx), (modes, alt))) = sub {
        let tx = out_tx.clone();
        tokio::spawn(async move {
            let mut utf8_pending = Vec::new();
            let mut osc_carryover = ridge_core::pty::osc_stream::OscSignalCarryover::default();
            // 首订阅回放：RIS + 活动模式前导 + scrollback（共享 SSOT
            // `ridge_remote::pane::pane_resync_frame`，与桌面 LAN/cloud 逐字一份）。
            // resume 时跳过（内核已存活）。空 backlog 仍挂 live，避免永久黑屏。
            if !resume {
                let frame = ridge_remote::pane::pane_resync_frame(pane_id, &backlog, &modes, alt);
                if tx.send(Message::Binary(frame)).is_err() {
                    return;
                }
            }
            if !send_pane_metadata(
                &tx,
                ws_id,
                pane_id,
                &backlog,
                &mut utf8_pending,
                &mut osc_carryover,
            ) {
                return;
            }
            while let Ok(bytes) = rx.recv().await {
                if !send_pane_bytes(
                    &tx,
                    ws_id,
                    pane_id,
                    &bytes,
                    &mut utf8_pending,
                    &mut osc_carryover,
                ) {
                    break;
                }
            }
        });
    }
}

/// rdg 旧版 Remote flat 帧异步回送；返回 `Some(text)` 的控制帧仍由调用方直接回写。
/// 所有文件/Git/搜索操作均移入 blocking 池，避免阻塞 WebSocket 主循环。
fn rdg_workspace_base_dir(workspace: &SharedWorkspace) -> PathBuf {
    workspace
        .lock()
        .ok()
        .and_then(|w| {
            w.sessions
                .get(w.default_session_index)
                .and_then(|session| session.cwd.clone())
        })
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn rdg_resolve_legacy_path(workspace: &SharedWorkspace, raw: &str) -> PathBuf {
    let base = rdg_workspace_base_dir(workspace);
    if raw.is_empty() || raw == "/" {
        return base;
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn spawn_legacy_frame<F>(out_tx: &mpsc::UnboundedSender<Message>, operation: &'static str, work: F)
where
    F: FnOnce() -> Value + Send + 'static,
{
    let tx = out_tx.clone();
    tokio::spawn(async move {
        match tokio::task::spawn_blocking(work).await {
            Ok(payload) => {
                let _ = tx.send(Message::Text(payload.to_string()));
            }
            Err(error) => {
                tracing::warn!(target: "ridge_cli::remote", operation, %error, "legacy remote request failed");
            }
        }
    });
}

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
                    crate::rpc::result_response(&id, hello.get("params").cloned().unwrap_or(hello))
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
                    start_pane_subscription(&params, workspace, ws_id, out_tx);
                }
            }
            return None;
        }
        let id = id.unwrap();
        return Some(
            match dispatch_lan_invoke(method, &params, workspace, ws_id, out_tx) {
                Ok(result) => crate::rpc::result_response(&id, result).to_string(),
                Err(msg) => crate::rpc::error_response(
                    &id,
                    &crate::rpc::RpcError::new(crate::rpc::JSON_RPC_METHOD_NOT_FOUND, msg),
                )
                .to_string(),
            },
        );
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
            start_pane_subscription(v, workspace, ws_id, out_tx);
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
                    let _ = out_tx.send(Message::Text(
                        json!({
                            "type": "pty-resized",
                            "workspaceId": ws_id.to_string(),
                            "paneId": pane_id.to_string(),
                            "rows": rows,
                            "cols": cols,
                        })
                        .to_string(),
                    ));
                }
            }
            None
        }

        "create-pane" => {
            let shell = v["shell"].as_str().filter(|s| !s.is_empty());
            let cwd = v["cwd"].as_str().filter(|s| !s.is_empty());
            let msg = match create_rdg_pane(workspace, shell, cwd) {
                Ok(id) => {
                    let mut result = create_pane_result(ws_id, id);
                    result["type"] = json!("create-pane-result");
                    result
                }
                Err(error) => json!({
                    "type": "create-pane-result", "success": false, "error": error
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
            let msg = match ensure_current_workspace(id, ws_id) {
                Ok(()) => json!({
                    "type": "switch-workspace-result", "success": true, "workspaceId": ws_id.to_string()
                }),
                Err(error) => json!({
                    "type": "switch-workspace-result", "success": false, "workspaceId": id, "error": error
                }),
            };
            Some(msg.to_string())
        }

        "create-workspace" => {
            // 单工作区：明确退化为创建 pane，与 WorkspaceProvider/JSON-RPC
            // 路径保持同一语义，不再返回无法验证的 no-op 成功。
            let msg = match create_rdg_pane(workspace, None, None) {
                Ok(id) => {
                    let mut result = create_pane_result(ws_id, id);
                    result["type"] = json!("create-workspace-result");
                    result
                }
                Err(error) => json!({
                    "type": "create-workspace-result", "success": false, "error": error
                }),
            };
            Some(msg.to_string())
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

        // 旧版 Remote 的 sidebar flat 帧：保持协议兼容，但不在 WS 任务内同步跑磁盘/Git。
        "list-files" => {
            let target = rdg_resolve_legacy_path(
                workspace,
                v.get("path").and_then(Value::as_str).unwrap_or(""),
            );
            let roots = rdg_allowed_file_roots(workspace);
            spawn_legacy_frame(out_tx, "list-files", move || {
                let entries = fs_reuse::list_dir(&roots, &target).unwrap_or_default();
                let parent = target
                    .parent()
                    .map(|path| path.to_string_lossy().into_owned());
                json!({
                    "type": "files",
                    "path": target.to_string_lossy(),
                    "parent": parent,
                    "entries": entries,
                })
            });
            None
        }

        "list-git-status" => {
            let root = rdg_workspace_base_dir(workspace);
            spawn_legacy_frame(out_tx, "list-git-status", move || {
                let info = ridge_core::commands::git::git_info_for_path(&root);
                json!({
                    "type": "git-status",
                    "isGitRepo": info.is_git_repo,
                    "currentBranch": info.current_branch,
                    "branches": info.branches,
                    "files": info.diff.files,
                    "commits": info.commits,
                })
            });
            None
        }

        "search-files" => {
            let query = v
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            let root = rdg_workspace_base_dir(workspace);
            let roots = rdg_allowed_file_roots(workspace);
            spawn_legacy_frame(out_tx, "search-files", move || {
                let results = if query.trim().is_empty() {
                    Vec::new()
                } else {
                    fs_reuse::search(&roots, &root.to_string_lossy(), &query, false, false)
                        .into_iter()
                        .map(|hit| {
                            json!({
                                "path": hit.file,
                                "line": hit.line,
                                "column": hit.column,
                                "snippet": hit.content,
                            })
                        })
                        .collect::<Vec<_>>()
                };
                json!({ "type": "search-results", "query": query, "results": results })
            });
            None
        }

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
    fn theme_frame_matches_remote_wire_shape() {
        let colors = std::collections::HashMap::from([
            ("bg".to_string(), "#000000".to_string()),
            ("fg".to_string(), "#ffffff".to_string()),
        ]);
        let frame = ridge_core::commands::theme::theme_frame("dark", "dark", &colors);
        assert_eq!(frame["type"], "theme");
        assert_eq!(frame["id"], "dark");
        assert_eq!(frame["themeType"], "dark");
        assert_eq!(frame["colors"]["bg"], "#000000");
    }

    #[test]
    fn default_theme_frame_keeps_headless_clients_on_the_default_palette() {
        let frame = ridge_core::commands::theme::default_theme_frame();
        assert_eq!(frame["type"], "theme");
        assert_eq!(frame["id"], "default");
        assert_eq!(frame["themeType"], "dark");
        assert!(frame["colors"]
            .as_object()
            .is_some_and(|colors| colors.is_empty()));
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

    #[test]
    fn saved_workspace_handle_is_explicit_and_reopenable() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("44444444-4444-4444-4444-444444444444").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let files = dispatch_lan_invoke(
            "list_saved_workspace_files",
            &json!({}),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("rdg exposes its current workspace handle");
        assert_eq!(files.as_array().map(Vec::len), Some(1));
        let path = files[0]["path"].as_str().expect("handle path");
        assert_eq!(path, "rdg://workspace/44444444-4444-4444-4444-444444444444");

        let reopened = dispatch_lan_invoke(
            "open_workspace_from_file",
            &json!({ "path": path }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("current workspace handle is reopenable");
        assert_eq!(reopened, json!("44444444-4444-4444-4444-444444444444"));
    }

    #[test]
    fn saved_workspace_open_rejects_non_rdg_paths_with_actionable_error() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("55555555-5555-5555-5555-555555555555").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let error = dispatch_lan_invoke(
            "open_workspace_from_file",
            &json!({ "path": "C:/tmp/example.ridge" }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect_err("rdg must not read arbitrary host paths");
        assert!(error.contains("list_saved_workspace_files"));
    }

    #[test]
    fn workspace_switch_rejects_stale_or_foreign_ids() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("66666666-6666-6666-6666-666666666666").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let error = dispatch_lan_invoke(
            "switch_workspace",
            &json!({ "workspaceId": "77777777-7777-7777-7777-777777777777" }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect_err("foreign workspace must not report a successful switch");
        assert!(error.contains("workspace not found"));
    }

    #[test]
    fn fs_methods_are_served_through_json_rpc_and_sandboxed_core() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::current_dir().unwrap().join(format!(
            ".ridge-lan-fs-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("needle.txt"), "needle from lan host\n").unwrap();

        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("abababab-abab-abab-abab-abababababab").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();

        let tree_reply = handle_text(
            &json!({
                "jsonrpc": "2.0",
                "id": 21,
                "method": "get_file_tree",
                "params": { "path": root.to_string_lossy(), "depth": 1 },
            }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("file-tree reply");
        let tree: Value = serde_json::from_str(&tree_reply).unwrap();
        assert_eq!(tree["id"], 21);
        assert!(tree["result"]["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "needle.txt"));

        let children = dispatch_lan_invoke(
            "get_directory_children",
            &json!({ "path": root.to_string_lossy() }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("directory children");
        assert!(children
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "nested"));

        let search = dispatch_lan_invoke(
            "search",
            &json!({
                "root": root.to_string_lossy(),
                "query": "needle",
                "useRegex": false,
                "caseSensitive": false,
            }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("search result");
        assert_eq!(search.as_array().unwrap().len(), 1);

        let legacy_search = dispatch_lan_invoke(
            "search_files",
            &json!({ "path": root.to_string_lossy(), "query": "needle" }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("legacy search result");
        assert_eq!(
            legacy_search[0]["path"],
            root.join("needle.txt").to_string_lossy().to_string()
        );

        drop(workspace);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_list_files_is_async_and_returns_compatible_frame() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static N: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::current_dir().unwrap().join(format!(
            ".ridge-lan-legacy-files-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("needle.txt"), "needle\n").unwrap();

        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("cdcdcdcd-cdcd-cdcd-cdcd-cdcdcdcdcdcd").unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        assert!(handle_text(
            &json!({ "type": "list-files", "path": root.to_string_lossy() }),
            &workspace,
            ws_id,
            &tx,
        )
        .is_none());

        let frame = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("legacy file listing should not block the websocket loop")
            .expect("legacy file listing frame");
        let Message::Text(text) = frame else {
            panic!("expected text sidebar frame")
        };
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["type"], "files");
        assert_eq!(value["path"], root.to_string_lossy().to_string());
        assert!(value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "needle.txt"));

        drop(workspace);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_search_and_git_status_return_frames() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("dededede-dede-dede-dede-dededededede").unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();

        assert!(handle_text(
            &json!({ "type": "search-files", "query": "" }),
            &workspace,
            ws_id,
            &tx,
        )
        .is_none());
        let search_frame = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .expect("legacy search timeout")
            .expect("legacy search frame");
        let Message::Text(search_text) = search_frame else {
            panic!("expected search text frame")
        };
        let search: Value = serde_json::from_str(&search_text).unwrap();
        assert_eq!(search["type"], "search-results");
        assert_eq!(search["results"].as_array().unwrap().len(), 0);

        assert!(handle_text(
            &json!({ "type": "list-git-status" }),
            &workspace,
            ws_id,
            &tx,
        )
        .is_none());
        let git_frame = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("legacy git status timeout")
            .expect("legacy git status frame");
        let Message::Text(git_text) = git_frame else {
            panic!("expected git status text frame")
        };
        let git: Value = serde_json::from_str(&git_text).unwrap();
        assert_eq!(git["type"], "git-status");
        assert!(git["branches"].is_array());
        assert!(git["files"].is_array());
        assert!(git["commits"].is_array());
    }

    #[test]
    fn single_workspace_create_result_explicitly_reports_pane_fallback() {
        let ws_id = Uuid::parse_str("88888888-8888-8888-8888-888888888888").unwrap();
        let pane_id = Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap();
        let result = create_pane_result(ws_id, pane_id);
        assert_eq!(result["success"], true);
        assert_eq!(result["workspaceId"], ws_id.to_string());
        assert_eq!(result["paneId"], pane_id.to_string());
        assert_eq!(result["operation"], "create-pane");
        assert_eq!(result["createdWorkspace"], false);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn create_pane_invoke_adds_one_session_to_the_single_workspace() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let result = dispatch_lan_invoke(
            "create_pane",
            &json!({ "workspaceId": ws_id.to_string() }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("create_pane should allocate a PTY-backed pane");
        assert!(result["paneId"].as_str().is_some());
        assert_eq!(result["workspaceId"], ws_id.to_string());
        assert_eq!(workspace.lock().unwrap().sessions.len(), 1);
        // Dropping the shared workspace releases the PTY bridge and its child.
        drop(workspace);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resize_invoke_emits_actual_pane_dimensions() {
        let workspace = super::super::workspace::new_shared();
        let ws_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let created = dispatch_lan_invoke("create_pane", &json!({}), &workspace, ws_id, &tx)
            .expect("create pane");
        let pane_id = created["paneId"].as_str().expect("pane id");

        dispatch_lan_invoke(
            "resize_pane",
            &json!({ "paneId": pane_id, "rows": 42, "cols": 120 }),
            &workspace,
            ws_id,
            &tx,
        )
        .expect("resize pane");

        let frame = rx.try_recv().expect("resize event");
        let Message::Text(text) = frame else {
            panic!("expected text resize event")
        };
        let value: Value = serde_json::from_str(&text).expect("json resize event");
        assert_eq!(value["type"], "pty-resized");
        assert_eq!(value["workspaceId"], ws_id.to_string());
        assert_eq!(value["paneId"], pane_id);
        assert_eq!(value["rows"], 42);
        assert_eq!(value["cols"], 120);
        drop(workspace);
    }

    #[test]
    fn pane_subscription_request_accepts_top_level_and_nested_fields() {
        let pane_id = Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();

        assert_eq!(
            pane_subscription_request(&json!({
                "paneId": pane_id.to_string(),
                "resume": true,
            })),
            Some((pane_id, true))
        );
        assert_eq!(
            pane_subscription_request(&json!({
                "params": { "paneId": pane_id.to_string(), "resume": false },
            })),
            Some((pane_id, false))
        );
    }

    #[test]
    fn pane_subscription_request_rejects_missing_or_invalid_pane() {
        assert_eq!(pane_subscription_request(&json!({})), None);
        assert_eq!(
            pane_subscription_request(&json!({ "paneId": "not-a-uuid" })),
            None
        );
    }

    #[test]
    fn send_pane_bytes_preserves_binary_then_metadata_order() {
        let workspace_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let pane_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut utf8_pending = Vec::new();
        let mut osc_carryover = ridge_core::pty::osc_stream::OscSignalCarryover::default();

        assert!(send_pane_bytes(
            &tx,
            workspace_id,
            pane_id,
            b"output\x1b]2;title\x07",
            &mut utf8_pending,
            &mut osc_carryover,
        ));
        assert!(matches!(rx.try_recv().unwrap(), Message::Binary(_)));
        let Message::Text(metadata) = rx.try_recv().unwrap() else {
            panic!("expected pty metadata after binary frame");
        };
        assert!(metadata.contains("\"type\":\"pty-meta\""));
    }

    #[test]
    fn send_pane_paths_report_closed_output_channel() {
        let workspace_id = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let pane_id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        drop(rx);
        let mut utf8_pending = Vec::new();
        let mut osc_carryover = ridge_core::pty::osc_stream::OscSignalCarryover::default();

        assert!(!send_pane_metadata(
            &tx,
            workspace_id,
            pane_id,
            b"\x1b]2;title\x07",
            &mut utf8_pending,
            &mut osc_carryover,
        ));
    }

    #[test]
    fn rdg_metadata_frame_reassembles_split_osc() {
        let workspace_id = Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap();
        let pane_id = Uuid::parse_str("dddddddd-dddd-dddd-dddd-dddddddddddd").unwrap();
        let mut utf8_pending = Vec::new();
        let mut osc_carryover = ridge_core::pty::osc_stream::OscSignalCarryover::default();

        assert!(rdg_metadata_frame(
            workspace_id,
            pane_id,
            b"\x1b]2;split",
            &mut utf8_pending,
            &mut osc_carryover,
        )
        .is_none());

        let frame = rdg_metadata_frame(
            workspace_id,
            pane_id,
            b" title\x07\x1b]7;file:///C:/wind\x07",
            &mut utf8_pending,
            &mut osc_carryover,
        )
        .expect("completed OSC metadata");
        let Message::Text(text) = frame else {
            panic!("expected pty-meta text frame")
        };
        let value: Value = serde_json::from_str(&text).expect("valid pty-meta JSON");
        assert_eq!(value["type"], "pty-meta");
        assert_eq!(value["workspaceId"], workspace_id.to_string());
        assert_eq!(value["paneId"], pane_id.to_string());
        assert_eq!(value["title"], "split title");
        assert_eq!(value["cwd"], "C:/wind");
    }

    #[test]
    fn pane_subscription_request_accepts_top_level_and_params_fields() {
        let pane_id = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();

        assert_eq!(
            pane_subscription_request(&json!({
                "paneId": pane_id.to_string(),
                "resume": true,
            })),
            Some((pane_id, true))
        );
        assert_eq!(
            pane_subscription_request(&json!({
                "params": {
                    "paneId": pane_id.to_string(),
                    "resume": true,
                },
            })),
            Some((pane_id, true))
        );
        assert_eq!(
            pane_subscription_request(&json!({"paneId": "not-a-uuid"})),
            None
        );
    }
}
