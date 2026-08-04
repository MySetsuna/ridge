//! Remote host adapter backed by the long-lived `ridge-kernel` process.
//!
//! This module deliberately has no Tauri dependency.  The desktop process is
//! only a renderer/projection; LAN clients talk to this host after the desktop
//! is gone, while PTY input/output and workspace identity stay in kernel.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use ridge_kernel::client::{
    attach_domain_pty_output, create_domain_pty, list_domain_ptys, poll_domain_pty_output,
    read_domain_workspaces, request_json, resync_domain_pty_output, scrollback_domain_pty,
    write_domain_pty, resize_domain_pty, KernelPtyInfo, KernelPtyOutput,
};
use ridge_kernel::registry::KernelEndpoint;
use ridge_remote::auth::{RemoteAuth, SessionStore};
use ridge_remote::host::{HostAuth, HostError, HostMeta, RemoteHost, WorkspaceProvider, WsConn};
use ridge_remote::serve::UaServeConfig;

use crate::core_host;
use crate::fs_reuse;

const WORKSPACE_URI_PREFIX: &str = "ridge://kernel-workspace/";

pub struct KernelHost {
    pub endpoint: KernelEndpoint,
    pub totp: Arc<RemoteAuth>,
    pub sessions: SessionStore,
    pub port: u16,
    pub lan_ip: String,
    pub machine_name: String,
    pub serve_cfg: UaServeConfig,
    pub tls_enabled: bool,
    pub remote_enabled: Arc<AtomicBool>,
}

impl KernelHost {
    fn snapshot(&self) -> KernelSnapshot {
        let ptys = list_domain_ptys(&self.endpoint).unwrap_or_default();
        let mut ids = read_domain_workspaces(&self.endpoint)
            .map(|snapshot| {
                snapshot
                    .workspaces
                    .into_iter()
                    .filter_map(|id| Uuid::parse_str(&id).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for pty in &ptys {
            if let Some(id) = pty.workspace_id {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        if ids.is_empty() {
            if let Ok(value) = request_json(&self.endpoint, "POST", "/v1/domain/workspaces", None) {
                if let Some(id) = value
                    .get("workspace_id")
                    .and_then(Value::as_str)
                    .and_then(|id| Uuid::parse_str(id).ok())
                {
                    ids.push(id);
                }
            }
        }
        ids.sort();
        let active = read_domain_workspaces(&self.endpoint)
            .ok()
            .and_then(|snapshot| snapshot.active)
            .and_then(|id| Uuid::parse_str(&id).ok())
            .or_else(|| ids.first().copied());
        KernelSnapshot { ids, active, ptys }
    }

    fn workspace_id(&self, requested: Option<&str>) -> Result<Uuid, String> {
        let snapshot = self.snapshot();
        if let Some(raw) = requested.filter(|value| !value.trim().is_empty()) {
            let id = Uuid::parse_str(raw).map_err(|error| error.to_string())?;
            if snapshot.ids.contains(&id) {
                return Ok(id);
            }
            return Err(format!("workspace not found: {raw}"));
        }
        snapshot
            .active
            .or_else(|| snapshot.ids.first().copied())
            .ok_or_else(|| "kernel has no workspace".to_string())
    }

    fn panes(&self, workspace_id: Uuid, snapshot: &KernelSnapshot) -> Vec<Value> {
        snapshot
            .ptys
            .iter()
            .filter(|pty| pty.workspace_id == Some(workspace_id))
            .map(|pty| {
                json!({
                    "id": pty.pty_id.to_string(),
                    "title": pty.cwd.as_deref().unwrap_or(&pty.role),
                    "cwd": pty.cwd,
                    "status": pty.status,
                    "rows": pty.rows,
                    "cols": pty.cols,
                })
            })
            .collect()
    }

    fn layout(&self, workspace_id: Uuid, snapshot: &KernelSnapshot) -> Value {
        let raw = request_json(
            &self.endpoint,
            "GET",
            &format!("/v1/domain/workspaces/{workspace_id}"),
            None,
        )
        .ok()
        .and_then(|value| value.get("layout").cloned());
        raw.map(kernel_layout_to_ui).unwrap_or_else(|| {
            let panes = self.panes(workspace_id, snapshot);
            match panes.as_slice() {
                [pane] => json!({ "type": "leaf", "id": pane["id"], "title": pane["title"], "cwd": pane["cwd"] }),
                [] => json!({ "type": "leaf", "id": Uuid::nil().to_string() }),
                _ => json!({
                    "type": "split",
                    "id": panes[0]["id"],
                    "direction": "horizontal",
                    "children": panes.iter().map(|pane| json!({ "type": "leaf", "id": pane["id"], "title": pane["title"], "cwd": pane["cwd"] })).collect::<Vec<_>>(),
                    "ratios": panes.iter().map(|_| 1.0 / panes.len() as f64).collect::<Vec<_>>(),
                }),
            }
        })
    }

    fn roots(&self, snapshot: &KernelSnapshot) -> Vec<PathBuf> {
        let mut roots = snapshot
            .ptys
            .iter()
            .filter_map(|pty| pty.cwd.as_deref().map(PathBuf::from))
            .collect::<Vec<_>>();
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
        roots
    }

    fn pane_id(&self, args: &Value, snapshot: &KernelSnapshot) -> Result<Uuid, String> {
        args.get("paneId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .and_then(|value| Uuid::parse_str(value).ok())
            .or_else(|| snapshot.ptys.first().map(|pty| pty.pty_id))
            .ok_or_else(|| "no pane".to_string())
    }

    fn create_pane(&self, args: &Value, snapshot: &KernelSnapshot) -> Result<Uuid, String> {
        let workspace_id = self.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
        let target = snapshot
            .ptys
            .iter()
            .find(|pty| pty.workspace_id == Some(workspace_id))
            .map(|pty| pty.pty_id)
            .ok_or_else(|| "workspace has no pane to split".to_string())?;
        let direction = args
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("horizontal");
        let value = request_json(
            &self.endpoint,
            "POST",
            &format!("/v1/domain/workspaces/{workspace_id}/split"),
            Some(&json!({ "pane_id": target, "direction": direction })),
        )?;
        let pane_id = value
            .get("pane_id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| "kernel split response missing pane_id".to_string())?;
        let shell = args.get("shell").and_then(Value::as_str);
        let cwd = args.get("cwd").and_then(Value::as_str);
        create_domain_pty(
            &self.endpoint,
            pane_id,
            shell,
            cwd,
            Some(workspace_id),
            "shell",
            Some("remote-lan"),
        )?;
        Ok(pane_id)
    }
}

struct KernelSnapshot {
    ids: Vec<Uuid>,
    active: Option<Uuid>,
    ptys: Vec<KernelPtyInfo>,
}

impl HostMeta for KernelHost {
    fn port(&self) -> u16 { self.port }
    fn lan_ip(&self) -> String { self.lan_ip.clone() }
    fn machine_name(&self) -> String { self.machine_name.clone() }
    fn remote_enabled(&self) -> Arc<AtomicBool> { self.remote_enabled.clone() }
    fn tls_enabled(&self) -> bool { self.tls_enabled }
    fn serve_cfg(&self) -> UaServeConfig { self.serve_cfg.clone() }
}

impl HostAuth for KernelHost {
    fn verify_code(&self, code: &str) -> bool { self.totp.verify(code) }
    fn create_session_token(&self, device_id: &str, ip: &str) -> String {
        self.sessions.create_session_bound(device_id, ip)
    }
    fn validate_token(&self, token: &str) -> bool { self.sessions.validate_token(token) }
    fn validate_token_bound(&self, token: &str, device_id: &str, ip: &str) -> bool {
        self.sessions.validate_token_bound(token, device_id, ip)
    }
    fn validate_token_device_strict(&self, token: &str, device_id: &str, ip: &str) -> bool {
        self.sessions.validate_token_device_strict(token, device_id, ip)
    }
}

impl WorkspaceProvider for KernelHost {
    fn list_workspaces_json(&self) -> Value {
        let snapshot = self.snapshot();
        json!({
            "workspaces": snapshot.ids.iter().enumerate().map(|(index, id)| json!({
                "id": id.to_string(), "name": "Ridge", "displaySeq": index + 1,
                "active": snapshot.active == Some(*id), "panes": self.panes(*id, &snapshot),
            })).collect::<Vec<_>>()
        })
    }

    fn switch_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        let id = self.workspace_id(Some(workspace_id)).map_err(HostError::NotFound)?;
        request_json(&self.endpoint, "POST", &format!("/v1/domain/workspaces/{id}/activate"), None)
            .map_err(HostError::BadRequest)?;
        Ok(json!({ "success": true, "workspaceId": id.to_string() }))
    }

    fn create_workspace(&self, _name: Option<String>) -> Result<Value, HostError> {
        let value = request_json(&self.endpoint, "POST", "/v1/domain/workspaces", None)
            .map_err(HostError::BadRequest)?;
        Ok(json!({
            "success": true,
            "workspaceId": value.get("workspace_id").cloned().unwrap_or(Value::Null),
            "createdWorkspace": true,
        }))
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        let _ = self.workspace_id(Some(workspace_id)).map_err(HostError::NotFound)?;
        Err(HostError::BadRequest("kernel keeps workspace topology; close panes instead".into()))
    }

    fn allowed_file_roots(&self) -> Vec<PathBuf> { self.roots(&self.snapshot()) }
}

impl RemoteHost for KernelHost {
    fn serve_websocket(
        self: Arc<Self>,
        socket: WebSocket,
        _conn: WsConn,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move { run_ws(socket, self).await })
    }
}

fn kernel_layout_to_ui(value: Value) -> Value {
    if let Some(id) = value.get("Leaf").and_then(Value::as_str) {
        return json!({ "type": "leaf", "id": id });
    }
    if let Some(split) = value.get("Split") {
        let direction = match split.get("direction").and_then(Value::as_str) {
            Some("Vertical") => "vertical",
            _ => "horizontal",
        };
        let children = split
            .get("children")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(kernel_layout_to_ui)
            .collect::<Vec<_>>();
        return json!({
            "type": "split",
            "id": children.first().and_then(|child| child.get("id")).cloned().unwrap_or(Value::Null),
            "direction": direction,
            "children": children,
            "ratios": split.get("ratios").cloned().unwrap_or_else(|| json!([])),
        });
    }
    json!({ "type": "leaf", "id": Uuid::nil().to_string() })
}

async fn run_ws(socket: WebSocket, host: Arc<KernelHost>) {
    let (mut tx, mut rx) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let subscriptions = Arc::new(Mutex::new(std::collections::HashSet::<Uuid>::new()));
    if tx.send(Message::Text(json!({"type":"hello","version":1,"protocol":"ridge-remote-ws"}).to_string())).await.is_err() {
        return;
    }
    let _ = tx.send(Message::Text(panes_frame(&host).to_string())).await;
    loop {
        tokio::select! {
            Some(message) = out_rx.recv() => { if tx.send(message).await.is_err() { break; } }
            Some(Ok(message)) = rx.next() => match message {
                Message::Text(text) => {
                    let Ok(value) = serde_json::from_str::<Value>(&text) else { continue; };
                    if let Some(reply) = handle_text(&value, &host, &out_tx, &subscriptions) {
                        if tx.send(Message::Text(reply)).await.is_err() { break; }
                    }
                }
                Message::Ping(bytes) => { if tx.send(Message::Pong(bytes)).await.is_err() { break; } }
                Message::Close(_) => break,
                _ => {}
            },
            else => break,
        }
    }
}

fn handle_text(
    value: &Value,
    host: &Arc<KernelHost>,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<Uuid>>>,
) -> Option<String> {
    if value.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let id = value.get("id").cloned();
        if method == "$/hello" {
            let hello = crate::rpc::negotiate_hello(&params);
            return id.map(|id| crate::rpc::result_response(&id, hello.get("params").cloned().unwrap_or(hello)).to_string());
        }
        if id.is_none() {
            if matches!(method, "subscribe-pane" | "subscribe_pane_raw") { start_subscription(&params, host, out_tx, subscriptions); }
            return None;
        }
        let id = id.unwrap();
        return Some(jsonrpc_result(&id, dispatch(method, &params, host, out_tx, subscriptions)).to_string());
    }
    if value.get("type").and_then(Value::as_str) == Some("invoke-request") {
        let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or("");
        let args = value.get("args").cloned().unwrap_or(Value::Null);
        let result = dispatch(cmd, &args, host, out_tx, subscriptions);
        return Some(json!({"type":"invoke-result","_reqId":value.get("_reqId").cloned().unwrap_or(Value::Null),"_result":result}).to_string());
    }
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "ping" => Some(json!({"type":"pong"}).to_string()),
        "list-panes" | "list-workspace-panes" => Some(panes_frame(host).to_string()),
        "subscribe-pane" => { start_subscription(value, host, out_tx, subscriptions); None }
        "stdin" => { let _ = dispatch("write_to_pty", value, host, out_tx, subscriptions); None }
        "resize" | "claim-pane" | "refresh-pane" => { let _ = dispatch("resize_pane", value, host, out_tx, subscriptions); None }
        "create-pane" => Some(json!({"type":"create-pane-result","success":dispatch("create_pane", value, host, out_tx, subscriptions).is_ok()}).to_string()),
        "list-workspaces" => Some(json!({"type":"workspaces","workspaces":host.list_workspaces_json()["workspaces"]}).to_string()),
        _ => None,
    }
}

fn jsonrpc_result(id: &Value, result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => crate::rpc::result_response(id, value),
        Err(error) => crate::rpc::error_response(id, &crate::rpc::RpcError::new(crate::rpc::JSON_RPC_INTERNAL_ERROR, error)),
    }
}

fn dispatch(
    method: &str,
    args: &Value,
    host: &Arc<KernelHost>,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<Uuid>>>,
) -> Result<Value, String> {
    let snapshot = host.snapshot();
    match method {
        "get_active_workspace_id" => host.workspace_id(None).map(|id| Value::String(id.to_string())),
        "list_workspaces" => Ok(host.list_workspaces_json()["workspaces"].clone()),
        "get_pane_layout" | "get_pane_layout_for" | "get_window_pane_layout" => {
            let id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(host.layout(id, &snapshot))
        }
        "get_workspace_snapshot" => {
            let id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(json!({"workspaceId":id,"panes":host.panes(id,&snapshot),"layout":host.layout(id,&snapshot)}))
        }
        "write_to_pty" | "write_pty" => {
            let id = host.pane_id(args, &snapshot)?;
            let data = args.get("data").and_then(Value::as_str).unwrap_or("");
            write_domain_pty(&host.endpoint, id, data.as_bytes())?;
            Ok(Value::Null)
        }
        "resize_pane" | "resize_pty" => {
            let id = host.pane_id(args, &snapshot)?;
            let rows = args.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16;
            let cols = args.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16;
            resize_domain_pty(&host.endpoint, id, cols, rows)?;
            let _ = out_tx.send(Message::Text(json!({"type":"pty-resized","paneId":id,"rows":rows,"cols":cols}).to_string()));
            Ok(Value::Null)
        }
        "subscribe-pane" | "subscribe_pane_raw" | "register_pane_delta_channel" => {
            start_subscription(args, host, out_tx, subscriptions);
            Ok(Value::Null)
        }
        "create_pane" | "create-pane" => {
            let pane_id = host.create_pane(args, &snapshot)?;
            let workspace_id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(json!({"success":true,"workspaceId":workspace_id,"paneId":pane_id}))
        }
        "switch_workspace" => {
            let id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            request_json(&host.endpoint, "POST", &format!("/v1/domain/workspaces/{id}/activate"), None)?;
            Ok(json!({"success":true,"workspaceId":id}))
        }
        "search" | "search_files" => {
            let root = args.get("root").or_else(|| args.get("path")).and_then(Value::as_str).unwrap_or("");
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let hits = fs_reuse::search(&host.roots(&snapshot), root, query, args.get("useRegex").and_then(Value::as_bool).unwrap_or(false), args.get("caseSensitive").and_then(Value::as_bool).unwrap_or(false));
            serde_json::to_value(hits).map_err(|error| error.to_string())
        }
        "get_directory_children" | "list-files" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let entries = fs_reuse::list_dir(&host.roots(&snapshot), Path::new(path)).map_err(|error| error.to_string())?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "get_file_tree" | "read_file" | "text_search" => {
            let roots = host.roots(&snapshot);
            let ctx = core_host::headless_ctx(&roots);
            ridge_core::dispatch(method, args.clone(), &ctx).map_err(|error| error.to_command_string())
        }
        "list-git-status" => {
            let root = host.roots(&snapshot).first().cloned().unwrap_or_else(|| PathBuf::from("."));
            let info = ridge_core::commands::git::git_info_for_path(&root);
            Ok(json!({"type":"git-status","isGitRepo":info.is_git_repo,"currentBranch":info.current_branch,"branches":info.branches,"files":info.diff.files,"commits":info.commits}))
        }
        "list_saved_workspace_files" => Ok(json!([{"name":"kernel workspace","path":format!("{WORKSPACE_URI_PREFIX}{}",host.workspace_id(None)?) }])),
        "open_workspace_from_file" => Ok(Value::String(host.workspace_id(None)?.to_string())),
        "use_global_workspace" | "activate_pane_pty" | "set_pane_delta_mode" | "get_theme_data" => Ok(Value::Null),
        other => Err(format!("method not supported by kernel host: {other}")),
    }
}

fn panes_frame(host: &Arc<KernelHost>) -> Value {
    let snapshot = host.snapshot();
    let workspace_id = snapshot.active.or_else(|| snapshot.ids.first().copied()).map(|id| id.to_string());
    let panes = workspace_id.as_deref().and_then(|id| Uuid::parse_str(id).ok()).map(|id| host.panes(id, &snapshot)).unwrap_or_default();
    json!({"type":"panes","workspaceId":workspace_id,"panes":panes})
}

fn start_subscription(
    args: &Value,
    host: &Arc<KernelHost>,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<Uuid>>>,
) {
    let pane_id = args.get("paneId").and_then(Value::as_str).and_then(|id| Uuid::parse_str(id).ok()).or_else(|| args.get("params").and_then(|params| params.get("paneId")).and_then(Value::as_str).and_then(|id| Uuid::parse_str(id).ok()));
    let Some(pane_id) = pane_id else { return; };
    let Ok(mut active) = subscriptions.lock() else { return; };
    if !active.insert(pane_id) { return; }
    drop(active);
    let endpoint = host.endpoint.clone();
    let tx = out_tx.clone();
    tokio::spawn(async move {
        let scrollback = tokio::task::spawn_blocking({ let endpoint = endpoint.clone(); move || scrollback_domain_pty(&endpoint, pane_id, 262_144) }).await.ok().and_then(Result::ok).unwrap_or_default();
        let after_seq = tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || list_domain_ptys(&endpoint).ok().and_then(|ptys| ptys.into_iter().find(|pty| pty.pty_id == pane_id).map(|pty| pty.next_seq.saturating_sub(1)))
        }).await.ok().flatten();
        let lease = match tokio::task::spawn_blocking({ let endpoint = endpoint.clone(); move || attach_domain_pty_output(&endpoint, pane_id, after_seq) }).await {
            Ok(Ok(lease)) => lease,
            _ => return,
        };
        let frame = ridge_remote::pane::pane_resync_frame(pane_id, &scrollback, &ridge_term::term::modes::Modes::default(), false);
        if tx.send(Message::Binary(frame)).is_err() { return; }
        loop {
            let result = tokio::task::spawn_blocking({ let endpoint = endpoint.clone(); move || poll_domain_pty_output(&endpoint, pane_id, lease, 1000, 64) }).await;
            match result {
                Ok(Ok(KernelPtyOutput::Data(bytes))) if !bytes.is_empty() => { if tx.send(Message::Binary(ridge_remote::pane::pane_frame(pane_id, &bytes))).is_err() { break; } }
                Ok(Ok(KernelPtyOutput::Lagged)) => { let _ = resync_domain_pty_output(&endpoint, pane_id, lease); }
                Ok(Ok(KernelPtyOutput::Timeout)) => {}
                _ => break,
            }
        }
        let _ = ridge_kernel::client::detach_domain_pty_output(&endpoint, pane_id, lease);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_layout_is_converted_to_remote_shape() {
        let value = kernel_layout_to_ui(json!({"Split":{"direction":"Vertical","children":[{"Leaf":"a"},{"Leaf":"b"}],"ratios":[30.0,70.0]}}));
        assert_eq!(value["type"], "split");
        assert_eq!(value["direction"], "vertical");
        assert_eq!(value["children"][0]["id"], "a");
    }
}
