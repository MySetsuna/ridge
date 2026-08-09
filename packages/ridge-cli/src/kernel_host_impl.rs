//! Remote host adapter backed by the long-lived `ridge-kernel` process.
//!
//! This module deliberately has no Tauri dependency.  The desktop process is
//! only a renderer/projection; LAN clients talk to this host after the desktop
//! is gone, while PTY input/output and workspace identity stay in kernel.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use ridge_kernel::client::{
    attach_domain_pty_output, create_domain_pty, create_domain_pty_with_command,
    destroy_domain_pty, list_domain_ptys, poll_domain_pty_output, read_domain_workspaces,
    request_json, resize_domain_pty, resync_domain_pty_output, running_endpoint,
    scrollback_domain_pty, write_domain_pty, KernelPtyInfo, KernelPtyOutput,
};
use ridge_kernel::registry::KernelEndpoint;
use ridge_remote::auth::{RemoteAuth, SessionStore};
use ridge_remote::host::{HostAuth, HostError, HostMeta, RemoteHost, WorkspaceProvider, WsConn};
use ridge_remote::serve::UaServeConfig;

use crate::core_host;
use crate::fs_reuse;

const WORKSPACE_URI_PREFIX: &str = "ridge://kernel-workspace/";
/// Direct kernel hosts do not expose the desktop-only teammate controller.
/// The LAN client reads this hint from the workspace snapshot before mounting
/// background roster polling.
const KERNEL_HOST_CAPABILITIES: &[&str] = &["pane", "fs", "search", "workspace"];

fn select_endpoint(initial: KernelEndpoint, refreshed: Option<KernelEndpoint>) -> KernelEndpoint {
    refreshed.unwrap_or(initial)
}

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
    fn current_endpoint(&self) -> KernelEndpoint {
        select_endpoint(self.endpoint.clone(), running_endpoint())
    }

    fn snapshot(&self) -> KernelSnapshot {
        let mut errors = Vec::new();
        let endpoint = self.current_endpoint();
        let ptys =
            capture_kernel_result(&mut errors, "list kernel PTYs", list_domain_ptys(&endpoint))
                .unwrap_or_default();
        let workspace_snapshot = capture_kernel_result(
            &mut errors,
            "list kernel workspaces",
            read_domain_workspaces(&endpoint),
        );
        let mut ids = workspace_snapshot
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .workspaces
                    .iter()
                    .filter_map(|id| Uuid::parse_str(id).ok())
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
            if let Ok(value) = request_json(&endpoint, "POST", "/v1/domain/workspaces", None) {
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
        let active = workspace_snapshot
            .and_then(|snapshot| snapshot.active)
            .and_then(|id| Uuid::parse_str(&id).ok())
            .or_else(|| ids.first().copied());
        KernelSnapshot {
            ids,
            active,
            ptys,
            errors,
        }
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
                    "shell_kind": pty.program,
                    "status": pty.status,
                    "rows": pty.rows,
                    "cols": pty.cols,
                })
            })
            .collect()
    }

    fn layout(&self, workspace_id: Uuid, snapshot: &KernelSnapshot) -> Value {
        let raw = request_json(
            &self.current_endpoint(),
            "GET",
            &format!("/v1/domain/workspaces/{workspace_id}"),
            None,
        )
        .ok()
        .and_then(|value| value.get("layout").cloned());
        raw.map(|value| kernel_layout_to_ui(value, snapshot))
            .unwrap_or_else(|| {
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
        Self::resolve_pane_id(args, snapshot)
    }

    fn resolve_pane_id(args: &Value, snapshot: &KernelSnapshot) -> Result<Uuid, String> {
        let requested_workspace = args
            .get("workspaceId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                Uuid::parse_str(value)
                    .map_err(|error| format!("invalid workspace id: {value}: {error}"))
            })
            .transpose()?;

        let pane_id = match args.get("paneId").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => Uuid::parse_str(value)
                .map_err(|error| format!("invalid pane id: {value}: {error}"))?,
            _ => {
                let fallback_workspace = requested_workspace.or(snapshot.active);
                snapshot
                    .ptys
                    .iter()
                    .find(|pty| {
                        fallback_workspace
                            .map(|workspace_id| pty.workspace_id == Some(workspace_id))
                            .unwrap_or(true)
                    })
                    .map(|pty| pty.pty_id)
                    .ok_or_else(|| "no pane".to_string())?
            }
        };

        if let Some(workspace_id) = requested_workspace {
            let belongs_to_workspace = snapshot
                .ptys
                .iter()
                .any(|pty| pty.pty_id == pane_id && pty.workspace_id == Some(workspace_id));
            if !belongs_to_workspace {
                return Err(format!(
                    "pane {pane_id} does not belong to workspace {workspace_id}"
                ));
            }
        }
        Ok(pane_id)
    }

    fn create_pane(&self, args: &Value, snapshot: &KernelSnapshot) -> Result<Uuid, String> {
        let workspace_id = self.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
        let target = snapshot
            .ptys
            .iter()
            .find(|pty| pty.workspace_id == Some(workspace_id))
            .map(|pty| pty.pty_id)
            .or_else(|| {
                request_json(
                    &self.current_endpoint(),
                    "GET",
                    &format!("/v1/domain/workspaces/{workspace_id}"),
                    None,
                )
                .ok()
                .and_then(|value| workspace_detail_pane_target(&value))
            })
            .ok_or_else(|| "workspace has no pane to split".to_string())?;
        let direction = args
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("horizontal");
        let value = request_json(
            &self.current_endpoint(),
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
            &self.current_endpoint(),
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

fn capture_kernel_result<T>(
    errors: &mut Vec<String>,
    label: &str,
    result: Result<T, String>,
) -> Option<T> {
    result.map_or_else(
        |error| {
            errors.push(format!("{label}: {error}"));
            None
        },
        Some,
    )
}

struct KernelSnapshot {
    ids: Vec<Uuid>,
    active: Option<Uuid>,
    ptys: Vec<KernelPtyInfo>,
    errors: Vec<String>,
}

impl HostMeta for KernelHost {
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

impl HostAuth for KernelHost {
    fn verify_code(&self, code: &str) -> bool {
        self.totp.verify(code)
    }
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

impl WorkspaceProvider for KernelHost {
    fn list_workspaces_json(&self) -> Value {
        let snapshot = self.snapshot();
        json!({
            "workspaces": snapshot.ids.iter().enumerate().map(|(index, id)| json!({
                "id": id.to_string(), "name": "Ridge", "displaySeq": index + 1,
                "active": snapshot.active == Some(*id), "panes": self.panes(*id, &snapshot),
                "capabilities": KERNEL_HOST_CAPABILITIES,
            })).collect::<Vec<_>>()
        })
    }

    fn switch_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        let id = self
            .workspace_id(Some(workspace_id))
            .map_err(HostError::NotFound)?;
        request_json(
            &self.current_endpoint(),
            "POST",
            &format!("/v1/domain/workspaces/{id}/activate"),
            None,
        )
        .map_err(HostError::BadRequest)?;
        Ok(json!({ "success": true, "workspaceId": id.to_string() }))
    }

    fn create_workspace(&self, _name: Option<String>) -> Result<Value, HostError> {
        let value = request_json(
            &self.current_endpoint(),
            "POST",
            "/v1/domain/workspaces",
            None,
        )
        .map_err(HostError::BadRequest)?;
        Ok(json!({
            "success": true,
            "workspaceId": value.get("workspace_id").cloned().unwrap_or(Value::Null),
            "createdWorkspace": true,
        }))
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        let _ = self
            .workspace_id(Some(workspace_id))
            .map_err(HostError::NotFound)?;
        Err(HostError::BadRequest(
            "kernel keeps workspace topology; close panes instead".into(),
        ))
    }

    fn allowed_file_roots(&self) -> Vec<PathBuf> {
        self.roots(&self.snapshot())
    }
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

fn kernel_layout_to_ui(value: Value, snapshot: &KernelSnapshot) -> Value {
    if let Some(id) = value.get("Leaf").and_then(Value::as_str) {
        let mut result = json!({ "type": "leaf", "id": id });
        if let Some(pty) = snapshot
            .ptys
            .iter()
            .find(|pty| pty.pty_id.to_string() == id)
        {
            result["cwd"] = pty.cwd.clone().map(Value::String).unwrap_or(Value::Null);
            result["shell_kind"] = pty
                .program
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null);
        }
        return result;
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
            .map(|child| kernel_layout_to_ui(child, snapshot))
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
    let subscriptions = Arc::new(Mutex::new(std::collections::HashSet::<(Uuid, Uuid)>::new()));
    let mut last_panes_frame = panes_frame(&host);
    let mut topology_poll = tokio::time::interval(Duration::from_millis(250));
    topology_poll.tick().await;
    if tx
        .send(Message::Text(
            json!({
                "type":"hello",
                "version":1,
                "protocol":"ridge-remote-ws",
                "capabilities": KERNEL_HOST_CAPABILITIES,
            })
            .to_string(),
        ))
        .await
        .is_err()
    {
        return;
    }
    if tx
        .send(Message::Text(
            ridge_core::commands::theme::active_theme_frame().to_string(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let _ = tx.send(Message::Text(last_panes_frame.to_string())).await;
    loop {
        tokio::select! {
            Some(message) = out_rx.recv() => { if tx.send(message).await.is_err() { break; } }
            _ = topology_poll.tick() => {
                let next_panes_frame = tokio::task::spawn_blocking({
                    let host = Arc::clone(&host);
                    move || panes_frame(&host)
                }).await;
                let Ok(next_panes_frame) = next_panes_frame else { continue; };
                if panes_frame_changed(&last_panes_frame, &next_panes_frame) {
                    if tx.send(Message::Text(next_panes_frame.to_string())).await.is_err() { break; }
                    last_panes_frame = next_panes_frame;
                }
            }
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
    subscriptions: &Arc<Mutex<std::collections::HashSet<(Uuid, Uuid)>>>,
) -> Option<String> {
    let snapshot = host.snapshot();
    if value.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let id = value.get("id").cloned();
        if method == "$/hello" {
            let hello = crate::rpc::negotiate_hello(&params);
            return id.map(|id| {
                crate::rpc::result_response(&id, hello.get("params").cloned().unwrap_or(hello))
                    .to_string()
            });
        }
        if id.is_none() {
            if matches!(method, "subscribe-pane" | "subscribe_pane_raw") {
                start_subscription(&params, host, &snapshot, out_tx, subscriptions);
            }
            return None;
        }
        let id = id.unwrap();
        return Some(
            jsonrpc_result(&id, dispatch(method, &params, host, out_tx, subscriptions)).to_string(),
        );
    }
    if value.get("type").and_then(Value::as_str) == Some("invoke-request") {
        let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or("");
        let args = value.get("args").cloned().unwrap_or(Value::Null);
        let result = dispatch(cmd, &args, host, out_tx, subscriptions);
        return Some(invoke_result_wire(
            value.get("_reqId").cloned().unwrap_or(Value::Null),
            result,
        ));
    }
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "ping" => Some(json!({"type":"pong"}).to_string()),
        "list-panes" | "list-workspace-panes" => Some(panes_frame(host).to_string()),
        "subscribe-pane" => {
            start_subscription(value, host, &snapshot, out_tx, subscriptions);
            None
        }
        "stdin" => {
            let _ = dispatch("write_to_pty", value, host, out_tx, subscriptions);
            None
        }
        "resize" | "claim-pane" | "refresh-pane" => {
            let _ = dispatch("resize_pane", value, host, out_tx, subscriptions);
            None
        }
        "create-pane" => Some(
            create_pane_wire_result(dispatch("create_pane", value, host, out_tx, subscriptions))
                .to_string(),
        ),
        "create-workspace" => Some(
            create_workspace_wire_result(
                host.create_workspace(value.get("name").and_then(Value::as_str).map(str::to_owned)),
            )
            .to_string(),
        ),
        "switch-workspace" => Some(
            workspace_wire_result(
                "switch-workspace-result",
                host.switch_workspace(
                    value
                        .get("workspaceId")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                ),
            )
            .to_string(),
        ),
        "close-workspace" => Some(
            workspace_wire_result(
                "close-workspace-result",
                host.close_workspace(
                    value
                        .get("workspaceId")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                ),
            )
            .to_string(),
        ),
        "close-pane" => Some(
            close_pane_wire_result(dispatch("close_pane", value, host, out_tx, subscriptions))
                .to_string(),
        ),
        "list-workspaces" => Some(
            json!({"type":"workspaces","workspaces":host.list_workspaces_json()["workspaces"]})
                .to_string(),
        ),
        _ => None,
    }
}

fn invoke_result_wire(request_id: Value, result: Result<Value, String>) -> String {
    match result {
        Ok(result) => json!({
            "type": "invoke-result",
            "_reqId": request_id,
            "_result": result,
        })
        .to_string(),
        Err(error) => json!({
            "type": "invoke-result",
            "_reqId": request_id,
            "_error": error,
        })
        .to_string(),
    }
}

fn jsonrpc_result(id: &Value, result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => crate::rpc::result_response(id, value),
        Err(error) => crate::rpc::error_response(
            id,
            &crate::rpc::RpcError::new(crate::rpc::JSON_RPC_INTERNAL_ERROR, error),
        ),
    }
}

fn dispatch(
    method: &str,
    args: &Value,
    host: &Arc<KernelHost>,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<(Uuid, Uuid)>>>,
) -> Result<Value, String> {
    let snapshot = host.snapshot();
    match method {
        "get_active_workspace_id" => host
            .workspace_id(None)
            .map(|id| Value::String(id.to_string())),
        "list_workspaces" => Ok(host.list_workspaces_json()["workspaces"].clone()),
        "get_pane_layout" | "get_pane_layout_for" | "get_window_pane_layout" => {
            let id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(host.layout(id, &snapshot))
        }
        "get_workspace_snapshot" => {
            let id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(
                json!({"workspaceId":id,"panes":host.panes(id,&snapshot),"layout":host.layout(id,&snapshot)}),
            )
        }
        "write_to_pty" | "write_pty" => {
            let id = host.pane_id(args, &snapshot)?;
            let data = args.get("data").and_then(Value::as_str).unwrap_or("");
            write_domain_pty(&host.current_endpoint(), id, data.as_bytes())?;
            Ok(Value::Null)
        }
        "resize_pane" | "resize_pty" => {
            let id = host.pane_id(args, &snapshot)?;
            let rows = args.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16;
            let cols = args.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16;
            resize_domain_pty(&host.current_endpoint(), id, cols, rows)?;
            let _ = out_tx.send(Message::Text(
                json!({"type":"pty-resized","paneId":id,"rows":rows,"cols":cols}).to_string(),
            ));
            Ok(Value::Null)
        }
        "subscribe-pane" | "subscribe_pane_raw" | "register_pane_delta_channel" => {
            start_subscription(args, host, &snapshot, out_tx, subscriptions);
            Ok(Value::Null)
        }
        "create_pane" | "create-pane" => {
            let pane_id = host.create_pane(args, &snapshot)?;
            let workspace_id =
                host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(json!({"success":true,"workspaceId":workspace_id,"paneId":pane_id}))
        }
        "close_pane" | "close-pane" => {
            let pane_id = host.pane_id(args, &snapshot)?;
            destroy_domain_pty(&host.current_endpoint(), pane_id)?;
            Ok(json!({"success":true,"paneId":pane_id}))
        }
        "switch_workspace" => {
            let id = host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            request_json(
                &host.current_endpoint(),
                "POST",
                &format!("/v1/domain/workspaces/{id}/activate"),
                None,
            )?;
            Ok(json!({"success":true,"workspaceId":id}))
        }
        "search" | "search_files" => {
            let root = args
                .get("root")
                .or_else(|| args.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let hits = fs_reuse::search(
                &host.roots(&snapshot),
                root,
                query,
                args.get("useRegex")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                args.get("caseSensitive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            );
            serde_json::to_value(hits).map_err(|error| error.to_string())
        }
        "get_directory_children" | "list-files" => {
            let path = args.get("path").and_then(Value::as_str).unwrap_or("");
            let entries = fs_reuse::list_dir(&host.roots(&snapshot), Path::new(path))
                .map_err(|error| error.to_string())?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "get_file_tree" | "read_file" | "text_search" => {
            let roots = host.roots(&snapshot);
            let ctx = core_host::headless_ctx(&roots);
            ridge_core::dispatch(method, args.clone(), &ctx)
                .map_err(|error| error.to_command_string())
        }
        "detect_available_shells" => {
            serde_json::to_value(ridge_core::commands::shell::detect_available_shells())
                .map_err(|error| error.to_string())
        }
        "change_pane_shell" => {
            let pane_id = host.pane_id(args, &snapshot)?;
            let workspace_id =
                host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            let shell = args
                .get("shell")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            let shell_args = args
                .get("args")
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let cwd = snapshot
                .ptys
                .iter()
                .find(|pty| pty.pty_id == pane_id)
                .and_then(|pty| pty.cwd.as_deref());
            destroy_domain_pty(&host.current_endpoint(), pane_id)?;
            create_domain_pty_with_command(
                &host.current_endpoint(),
                pane_id,
                shell,
                &shell_args,
                cwd,
                Some(workspace_id),
                "shell",
                Some("remote-lan"),
                &HashMap::new(),
            )?;
            Ok(Value::Null)
        }
        "list-git-status" => {
            let root = host
                .roots(&snapshot)
                .first()
                .cloned()
                .unwrap_or_else(|| PathBuf::from("."));
            let info = ridge_core::commands::git::git_info_for_path(&root);
            Ok(
                json!({"type":"git-status","isGitRepo":info.is_git_repo,"currentBranch":info.current_branch,"branches":info.branches,"files":info.diff.files,"commits":info.commits}),
            )
        }
        "list_saved_workspace_files" => Ok(
            json!([{"name":"kernel workspace","path":format!("{WORKSPACE_URI_PREFIX}{}",host.workspace_id(None)?) }]),
        ),
        "open_workspace_from_file" => Ok(Value::String(host.workspace_id(None)?.to_string())),
        "use_global_workspace" | "activate_pane_pty" | "set_pane_delta_mode" | "get_theme_data" => {
            Ok(Value::Null)
        }
        other => Err(format!("method not supported by kernel host: {other}")),
    }
}

fn panes_frame(host: &Arc<KernelHost>) -> Value {
    let snapshot = host.snapshot();
    let workspace_id = snapshot
        .active
        .or_else(|| snapshot.ids.first().copied())
        .map(|id| id.to_string());
    let workspace_uuid = workspace_id
        .as_deref()
        .and_then(|id| Uuid::parse_str(id).ok());
    let panes = workspace_uuid
        .map(|id| host.panes(id, &snapshot))
        .unwrap_or_default();
    let layout = workspace_uuid
        .map(|id| host.layout(id, &snapshot))
        .unwrap_or_else(|| json!({ "type": "leaf", "id": Uuid::nil() }));
    json!({"type":"panes","workspaceId":workspace_id,"panes":panes,"layout":layout,"errors":snapshot.errors})
}

fn panes_frame_changed(previous: &Value, next: &Value) -> bool {
    previous != next
}

fn workspace_detail_pane_target(value: &Value) -> Option<Uuid> {
    value
        .get("panes")
        .and_then(Value::as_array)
        .and_then(|panes| {
            panes
                .iter()
                .find_map(|pane| pane.as_str().and_then(|id| Uuid::parse_str(id).ok()))
        })
}

fn create_pane_wire_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(result) => json!({
            "type": "create-pane-result",
            "success": true,
            "workspaceId": result.get("workspaceId"),
            "paneId": result.get("paneId"),
        }),
        Err(error) => json!({
            "type": "create-pane-result",
            "success": false,
            "error": error,
        }),
    }
}

fn create_workspace_wire_result(result: Result<Value, HostError>) -> Value {
    match result {
        Ok(result) => json!({
            "type": "create-workspace-result",
            "success": result.get("success").cloned().unwrap_or(Value::Bool(true)),
            "workspaceId": result.get("workspaceId"),
        }),
        Err(error) => {
            let message = match error {
                HostError::BadRequest(message) | HostError::NotFound(message) => message,
            };
            json!({
                "type": "create-workspace-result",
                "success": false,
                "error": message,
            })
        }
    }
}

fn workspace_wire_result(result_type: &str, result: Result<Value, HostError>) -> Value {
    match result {
        Ok(result) => {
            let mut wire = json!({
                "type": result_type,
                "success": result.get("success").cloned().unwrap_or(Value::Bool(true)),
            });
            if let Some(workspace_id) = result.get("workspaceId") {
                wire["workspaceId"] = workspace_id.clone();
            }
            wire
        }
        Err(error) => json!({
            "type": result_type,
            "success": false,
            "error": host_error_message(error),
        }),
    }
}

fn host_error_message(error: HostError) -> String {
    match error {
        HostError::BadRequest(message) | HostError::NotFound(message) => message,
    }
}

fn close_pane_wire_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(result) => json!({
            "type": "close-pane-result",
            "success": true,
            "paneId": result.get("paneId"),
        }),
        Err(error) => json!({
            "type": "close-pane-result",
            "success": false,
            "error": error,
        }),
    }
}

fn start_subscription(
    args: &Value,
    host: &Arc<KernelHost>,
    snapshot: &KernelSnapshot,
    out_tx: &mpsc::UnboundedSender<Message>,
    subscriptions: &Arc<Mutex<std::collections::HashSet<(Uuid, Uuid)>>>,
) {
    let Some((workspace_id, pane_id)) = subscription_target(args, snapshot) else {
        return;
    };
    let Ok(mut active) = subscriptions.lock() else {
        return;
    };
    if !active.insert((workspace_id, pane_id)) {
        return;
    }
    drop(active);
    let endpoint = host.current_endpoint();
    let tx = out_tx.clone();
    tokio::spawn(async move {
        let scrollback = tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || scrollback_domain_pty(&endpoint, pane_id, 262_144)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
        let after_seq = tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || {
                list_domain_ptys(&endpoint).ok().and_then(|ptys| {
                    ptys.into_iter()
                        .find(|pty| pty.pty_id == pane_id)
                        .map(|pty| pty.next_seq.saturating_sub(1))
                })
            }
        })
        .await
        .ok()
        .flatten();
        let lease = match tokio::task::spawn_blocking({
            let endpoint = endpoint.clone();
            move || attach_domain_pty_output(&endpoint, pane_id, after_seq)
        })
        .await
        {
            Ok(Ok(lease)) => lease,
            _ => return,
        };
        let frame = ridge_remote::pane::pane_resync_frame(
            pane_id,
            &scrollback,
            &ridge_term::term::modes::Modes::default(),
            false,
        );
        if tx.send(Message::Binary(frame)).is_err() {
            return;
        }
        let mut metadata_buffer = Vec::new();
        loop {
            let result = tokio::task::spawn_blocking({
                let endpoint = endpoint.clone();
                move || poll_domain_pty_output(&endpoint, pane_id, lease, 1000, 64)
            })
            .await;
            match result {
                Ok(Ok(KernelPtyOutput::Data(bytes))) => {
                    if !send_subscription_data(
                        workspace_id,
                        pane_id,
                        &bytes,
                        &mut metadata_buffer,
                        &tx,
                    ) {
                        break;
                    }
                }
                Ok(Ok(KernelPtyOutput::Lagged)) => {
                    let _ = resync_domain_pty_output(&endpoint, pane_id, lease);
                }
                Ok(Ok(KernelPtyOutput::Timeout)) => {}
                _ => break,
            }
        }
        let _ = ridge_kernel::client::detach_domain_pty_output(&endpoint, pane_id, lease);
    });
}

fn send_subscription_data(
    workspace_id: Uuid,
    pane_id: Uuid,
    bytes: &[u8],
    metadata_buffer: &mut Vec<u8>,
    tx: &mpsc::UnboundedSender<Message>,
) -> bool {
    if bytes.is_empty() {
        return true;
    }
    metadata_buffer.extend_from_slice(bytes);
    if metadata_buffer.len() > 8192 {
        let keep_from = metadata_buffer.len() - 8192;
        metadata_buffer.drain(..keep_from);
    }
    if let Some(metadata) = pty_metadata_frame(workspace_id, pane_id, metadata_buffer) {
        if tx.send(Message::Text(metadata.to_string())).is_err() {
            return false;
        }
    }
    tx.send(Message::Binary(ridge_remote::pane::pane_frame(
        pane_id, bytes,
    )))
    .is_ok()
}

fn subscription_target(args: &Value, snapshot: &KernelSnapshot) -> Option<(Uuid, Uuid)> {
    let target = args
        .get("params")
        .filter(|value| value.is_object())
        .unwrap_or(args);
    let Some(pane_id) = target
        .get("paneId")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
    else {
        return None;
    };
    let Some(pane) = snapshot.ptys.iter().find(|pty| pty.pty_id == pane_id) else {
        return None;
    };
    let workspace_id = target
        .get("workspaceId")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .or(pane.workspace_id)
        .or(snapshot.active);
    workspace_id
        .filter(|workspace_id| pane.workspace_id == Some(*workspace_id))
        .map(|workspace_id| (workspace_id, pane_id))
}

fn pty_metadata_frame(workspace_id: Uuid, pane_id: Uuid, bytes: &[u8]) -> Option<Value> {
    let title = last_osc_sequence(bytes, &[b'\x1b', b']', b'0', b';'])
        .or_else(|| last_osc_sequence(bytes, &[b'\x1b', b']', b'1', b';']))
        .or_else(|| last_osc_sequence(bytes, &[b'\x1b', b']', b'2', b';']))
        .and_then(ridge_core::pty::title::parse_title_from_output);
    let cwd = last_osc_sequence(bytes, &[b'\x1b', b']', b'7', b';'])
        .and_then(|sequence| {
            ridge_core::pty::cwd::parse_cwd_from_output(&String::from_utf8_lossy(sequence))
        })
        .map(|path| path.to_string_lossy().into_owned());
    if title.is_none() && cwd.is_none() {
        return None;
    }
    Some(json!({
        "type": "pty-meta",
        "workspaceId": workspace_id,
        "paneId": pane_id,
        "title": title,
        "cwd": cwd,
    }))
}

fn last_osc_sequence<'a>(bytes: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    let start = bytes
        .windows(prefix.len())
        .rposition(|window| window == prefix)?;
    let rest = &bytes[start + prefix.len()..];
    let bel = rest
        .iter()
        .position(|byte| *byte == 0x07)
        .map(|offset| offset + 1);
    let st = rest
        .windows(2)
        .position(|window| window == [0x1b, b'\\'])
        .map(|offset| offset + 2);
    let end = bel.or(st)?;
    Some(&bytes[start..start + prefix.len() + end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_endpoint_prefers_live_registry_generation() {
        let initial = KernelEndpoint {
            pid: 11,
            port: 1111,
            token: "old".into(),
            started_at_unix: 1,
        };
        let refreshed = KernelEndpoint {
            pid: 22,
            port: 2222,
            token: "new".into(),
            started_at_unix: 2,
        };
        let selected = select_endpoint(initial, Some(refreshed));
        assert_eq!(selected.pid, 22);
        assert_eq!(selected.port, 2222);
        assert_eq!(selected.token, "new");
    }

    #[test]
    fn current_endpoint_falls_back_to_boot_generation_when_registry_unavailable() {
        let initial = KernelEndpoint {
            pid: 11,
            port: 1111,
            token: "old".into(),
            started_at_unix: 1,
        };
        let selected = select_endpoint(initial, None);
        assert_eq!(selected.pid, 11);
        assert_eq!(selected.port, 1111);
        assert_eq!(selected.token, "old");
    }

    fn test_pty(pane_id: Uuid, workspace_id: Uuid) -> KernelPtyInfo {
        KernelPtyInfo {
            id: pane_id,
            pty_id: pane_id,
            workspace_id: Some(workspace_id),
            role: "shell".into(),
            program: Some("powershell.exe".into()),
            launch_profile: None,
            cwd: None,
            status: "running".into(),
            cols: 80,
            rows: 24,
            oldest_seq: 0,
            next_seq: 0,
        }
    }

    #[test]
    fn invoke_result_wire_keeps_dispatch_errors_out_of_result_payload() {
        let success: Value =
            serde_json::from_str(&invoke_result_wire(json!(7), Ok(json!({"roster": []})))).unwrap();
        assert_eq!(success["_result"]["roster"], json!([]));
        assert!(success.get("_error").is_none());

        let failure: Value = serde_json::from_str(&invoke_result_wire(
            json!(8),
            Err("method not supported by kernel host: get_teammate_topology".into()),
        ))
        .unwrap();
        assert_eq!(
            failure["_error"],
            "method not supported by kernel host: get_teammate_topology"
        );
        assert!(failure.get("_result").is_none());
    }

    #[test]
    fn kernel_layout_is_converted_to_remote_shape() {
        let snapshot = KernelSnapshot {
            ids: Vec::new(),
            active: None,
            ptys: vec![KernelPtyInfo {
                id: Uuid::from_u128(1),
                pty_id: Uuid::from_u128(1),
                workspace_id: None,
                role: "shell".into(),
                program: Some("powershell.exe".into()),
                launch_profile: None,
                cwd: Some("C:\\work".into()),
                status: "running".into(),
                cols: 80,
                rows: 24,
                oldest_seq: 0,
                next_seq: 1,
            }],
            errors: Vec::new(),
        };
        let value = kernel_layout_to_ui(
            json!({"Split":{"direction":"Vertical","children":[{"Leaf":"a"},{"Leaf":"b"}],"ratios":[30.0,70.0]}}),
            &snapshot,
        );
        assert_eq!(value["type"], "split");
        assert_eq!(value["direction"], "vertical");
        assert_eq!(value["children"][0]["id"], "a");
    }

    #[test]
    fn kernel_layout_includes_live_shell_metadata() {
        let pane_id = Uuid::from_u128(1);
        let snapshot = KernelSnapshot {
            ids: vec![],
            active: None,
            ptys: vec![KernelPtyInfo {
                id: pane_id,
                pty_id: pane_id,
                workspace_id: None,
                role: "shell".into(),
                program: Some("wsl.exe".into()),
                launch_profile: None,
                cwd: Some("C:\\work".into()),
                status: "running".into(),
                cols: 80,
                rows: 24,
                oldest_seq: 0,
                next_seq: 1,
            }],
            errors: vec![],
        };
        let value = kernel_layout_to_ui(json!({"Leaf": pane_id.to_string()}), &snapshot);
        assert_eq!(value["shell_kind"], "wsl.exe");
        assert_eq!(value["cwd"], "C:\\work");
    }

    #[test]
    fn pane_id_fallback_stays_in_requested_workspace() {
        let workspace_a = Uuid::new_v4();
        let workspace_b = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let pane_b = Uuid::new_v4();
        let snapshot = KernelSnapshot {
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_a),
            ptys: vec![test_pty(pane_a, workspace_a), test_pty(pane_b, workspace_b)],
            errors: Vec::new(),
        };

        assert_eq!(
            KernelHost::resolve_pane_id(&json!({"workspaceId": workspace_b}), &snapshot),
            Ok(pane_b)
        );
    }

    #[test]
    fn pane_id_rejects_cross_workspace_and_invalid_ids() {
        let workspace_a = Uuid::new_v4();
        let workspace_b = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let snapshot = KernelSnapshot {
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_a),
            ptys: vec![test_pty(pane_a, workspace_a)],
            errors: Vec::new(),
        };

        let error = KernelHost::resolve_pane_id(
            &json!({"paneId": pane_a, "workspaceId": workspace_b}),
            &snapshot,
        )
        .unwrap_err();
        assert!(error.contains("does not belong"));
        assert!(
            KernelHost::resolve_pane_id(&json!({"paneId": "not-a-uuid"}), &snapshot)
                .unwrap_err()
                .contains("invalid pane id")
        );
    }

    #[test]
    fn subscription_target_requires_matching_workspace_and_accepts_nested_params() {
        let workspace_a = Uuid::new_v4();
        let workspace_b = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let snapshot = KernelSnapshot {
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_a),
            ptys: vec![test_pty(pane_a, workspace_a)],
            errors: Vec::new(),
        };

        assert_eq!(
            subscription_target(&json!({"params": {"paneId": pane_a}}), &snapshot),
            Some((workspace_a, pane_a))
        );
        assert_eq!(
            subscription_target(
                &json!({"paneId": pane_a, "workspaceId": workspace_b}),
                &snapshot,
            ),
            None
        );
        assert_eq!(
            subscription_target(&json!({"paneId": "not-a-uuid"}), &snapshot),
            None
        );
    }

    #[test]
    fn capture_kernel_result_records_transport_errors_without_faking_success() {
        let mut errors = Vec::new();
        assert_eq!(
            capture_kernel_result(
                &mut errors,
                "list kernel PTYs",
                Err::<(), _>("denied".into())
            ),
            None
        );
        assert_eq!(errors, vec!["list kernel PTYs: denied"]);

        assert_eq!(
            capture_kernel_result(&mut errors, "list kernel PTYs", Ok(3)),
            Some(3)
        );
        assert_eq!(errors, vec!["list kernel PTYs: denied"]);
    }

    #[test]
    fn create_pane_wire_result_preserves_success_and_error_details() {
        let success = create_pane_wire_result(Ok(json!({
            "workspaceId": "workspace",
            "paneId": "pane",
        })));
        assert_eq!(success["success"], true);
        assert_eq!(success["paneId"], "pane");

        let failure = create_pane_wire_result(Err("workspace has no pane to split".into()));
        assert_eq!(failure["success"], false);
        assert_eq!(failure["error"], "workspace has no pane to split");
    }

    #[test]
    fn create_workspace_wire_result_preserves_legacy_remote_contract() {
        let workspace_id = Uuid::new_v4();
        let success = create_workspace_wire_result(Ok(json!({
            "success": true,
            "workspaceId": workspace_id,
            "createdWorkspace": true,
        })));
        assert_eq!(success["type"], "create-workspace-result");
        assert_eq!(success["success"], true);
        assert_eq!(success["workspaceId"], workspace_id.to_string());

        let failure =
            create_workspace_wire_result(Err(HostError::BadRequest("kernel unavailable".into())));
        assert_eq!(failure["type"], "create-workspace-result");
        assert_eq!(failure["success"], false);
        assert_eq!(failure["error"], "kernel unavailable");
    }

    #[test]
    fn default_theme_frame_keeps_kernel_host_handshake_complete() {
        let frame = ridge_core::commands::theme::default_theme_frame();
        assert_eq!(frame["type"], "theme");
        assert_eq!(frame["id"], "default");
        assert_eq!(frame["themeType"], "dark");
        assert!(frame["colors"]
            .as_object()
            .is_some_and(|colors| colors.is_empty()));
    }

    #[test]
    fn legacy_workspace_and_pane_results_preserve_errors_instead_of_timing_out() {
        let workspace_id = Uuid::new_v4();
        let switched = workspace_wire_result(
            "switch-workspace-result",
            Ok(json!({ "success": true, "workspaceId": workspace_id })),
        );
        assert_eq!(switched["success"], true);
        assert_eq!(switched["workspaceId"], workspace_id.to_string());

        let closed = workspace_wire_result(
            "close-workspace-result",
            Err(HostError::BadRequest(
                "kernel keeps workspace topology".into(),
            )),
        );
        assert_eq!(closed["success"], false);
        assert_eq!(closed["error"], "kernel keeps workspace topology");

        let pane = close_pane_wire_result(Ok(json!({ "paneId": "pane" })));
        assert_eq!(pane["type"], "close-pane-result");
        assert_eq!(pane["success"], true);
        assert_eq!(pane["paneId"], "pane");
    }

    #[test]
    fn topology_poll_only_emits_changed_panes_frames() {
        let frame = json!({"type":"panes","workspaceId":"workspace","panes":[],"errors":[]});
        assert!(!panes_frame_changed(&frame, &frame));
        assert!(panes_frame_changed(
            &frame,
            &json!({"type":"panes","workspaceId":"workspace","panes":[{"id":"pane"}],"errors":[]})
        ));
        assert!(panes_frame_changed(
            &frame,
            &json!({"type":"panes","workspaceId":"workspace","panes":[],"errors":["kernel unavailable"]})
        ));
    }

    #[test]
    fn empty_workspace_uses_kernel_graph_leaf_as_split_target() {
        let pane_id = Uuid::new_v4();
        assert_eq!(
            workspace_detail_pane_target(&json!({ "panes": [pane_id.to_string()] })),
            Some(pane_id)
        );
        assert_eq!(workspace_detail_pane_target(&json!({ "panes": [] })), None);
        assert_eq!(
            workspace_detail_pane_target(&json!({ "panes": ["bad"] })),
            None
        );
    }

    #[test]
    fn pty_metadata_frame_surfaces_osc_title_and_cwd() {
        let workspace_id = Uuid::new_v4();
        let pane_id = Uuid::new_v4();
        let frame = pty_metadata_frame(
            workspace_id,
            pane_id,
            b"\x1b]2;title\x07\x1b]7;file:///C:/workspace\x07",
        )
        .expect("OSC metadata should produce a frame");
        assert_eq!(frame["type"], "pty-meta");
        assert_eq!(frame["workspaceId"], workspace_id.to_string());
        assert_eq!(frame["paneId"], pane_id.to_string());
        assert_eq!(frame["title"], "title");
        assert_eq!(frame["cwd"], "C:/workspace");
        assert!(pty_metadata_frame(workspace_id, pane_id, b"plain output").is_none());
    }
}
