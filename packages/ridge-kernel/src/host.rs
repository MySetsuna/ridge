//! Remote host adapter backed by the long-lived `ridge-kernel` process.
//!
//! This module deliberately has no Tauri dependency.  The desktop process is
//! only a renderer/projection; LAN clients talk to this host after the desktop
//! is gone, while PTY input/output and workspace identity stay in kernel.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::client::{
    create_domain_pty, create_domain_pty_with_command, destroy_domain_pty, list_domain_ptys,
    read_domain_workspaces, request_json, resize_domain_pty, running_endpoint, write_domain_pty,
    DomainPtyLaunch, KernelPtyInfo,
};
use crate::registry::{read_endpoint, KernelEndpoint};
use ridge_remote::auth::{RemoteAuth, SessionStore, VerifyThrottle};
use ridge_remote::host::{HostAuth, HostError, HostMeta, RemoteHost, WorkspaceProvider, WsConn};
use ridge_remote::serve::UaServeConfig;

use crate::host_core as core_host;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const WORKSPACE_URI_PREFIX: &str = "ridge://kernel-workspace/";
/// Direct kernel hosts do not expose the desktop-only teammate controller.
/// The LAN client reads this hint from the workspace snapshot before mounting
/// background roster polling.
const KERNEL_HOST_CAPABILITIES: &[&str] = &["pane", "fs", "search", "workspace", "theme"];

pub fn implemented_capabilities() -> &'static [&'static str] {
    KERNEL_HOST_CAPABILITIES
}

/// Report the actual shared terminal stream used by independent LAN hosts.
pub fn log_kernel_transport_mode() {
    tracing::info!(target: "ridge_kernel::host", "kernel transport: canonical terminal frames over authenticated WebSocket");
}

fn select_endpoint(initial: KernelEndpoint, refreshed: Option<KernelEndpoint>) -> KernelEndpoint {
    refreshed.unwrap_or(initial)
}

/// CHG-050 §2.2 toggleable host trace. Off by default — when off,
/// every caller of `host_trace_*!` reduces to a single OnceLock read
/// plus a branch (sub-microsecond on hot paths). Enable with
/// `RIDGE_HOST_TRACE=1|true|yes|on` to emit `tracing::info!` lines
/// under target `ridge_cli::trace`.
pub fn host_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        matches!(
            std::env::var("RIDGE_HOST_TRACE").ok().as_deref(),
            Some("1" | "true" | "yes" | "on")
        )
    })
}

fn trace_input_source(args: &Value) -> &str {
    args.get("inputSourceId")
        .or_else(|| args.get("input_source_id"))
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
        })
        .unwrap_or("unavailable")
}

#[macro_export]
macro_rules! host_trace {
    ($($arg:tt)+) => {{
        if $crate::host::host_trace_enabled() {
            tracing::info!(target: "ridge_cli::trace", $($arg)+);
        }
    }};
}

pub struct KernelHost {
    pub endpoint: KernelEndpoint,
    pub totp: Arc<RemoteAuth>,
    /// §7.4 trust-grant + 暴力破解节流；`HostAuth` 5 方法委托到本字段
    /// （`pre_verify_gate` / `post_verify_record` / `is_blacklisted` /
    /// `totp_trust_check` / `totp_trust_record`）。
    pub totp_throttle: Arc<VerifyThrottle>,
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
        // Registry reads are local and cheap. Avoid probing the kernel health
        // endpoint on every projection request when the host is still bound
        // to the same boot generation; those probes use a bounded socket read
        // timeout and can serialize startup RPCs behind the topology poll.
        let Some(refreshed) = read_endpoint() else {
            return self.endpoint.clone();
        };
        if refreshed.pid == self.endpoint.pid
            && refreshed.port == self.endpoint.port
            && refreshed.token == self.endpoint.token
        {
            self.endpoint.clone()
        } else {
            select_endpoint(self.endpoint.clone(), running_endpoint())
        }
    }

    fn seed_workspace_pane(endpoint: &KernelEndpoint, workspace_id: Uuid) -> Result<Uuid, String> {
        let pane_id = Uuid::new_v4();
        request_json(
            endpoint,
            "PUT",
            &format!("/v1/domain/workspaces/{workspace_id}/topology"),
            Some(&json!({
                "pane_tree": {
                    "root": { "Leaf": pane_id },
                    "panes": {
                        pane_id.to_string(): {
                            "id": pane_id,
                            "mode": "Terminal"
                        }
                    }
                }
            })),
        )?;
        create_domain_pty(
            endpoint,
            pane_id,
            None,
            None,
            Some(workspace_id),
            "shell",
            Some("remote-lan"),
        )?;
        Ok(pane_id)
    }

    fn create_workspace_json(&self, name: Option<&str>) -> Result<Value, String> {
        let endpoint = self.current_endpoint();
        let body = name.map(|value| json!({ "name": value }));
        let value = request_json(&endpoint, "POST", "/v1/domain/workspaces", body.as_ref())?;
        let workspace_id = value
            .get("workspace_id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or_else(|| "kernel create-workspace response missing workspace_id".to_string())?;
        Self::seed_workspace_pane(&endpoint, workspace_id)?;
        Ok(json!({
            "success": true,
            "workspaceId": workspace_id.to_string(),
            "createdWorkspace": true,
        }))
    }

    fn set_split_ratios(&self, args: &Value) -> Result<Value, String> {
        let workspace_id = self.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
        let mut detail = request_json(
            &self.current_endpoint(),
            "GET",
            &format!("/v1/domain/workspaces/{workspace_id}"),
            None,
        )?;
        let mut layout = detail
            .get_mut("layout")
            .map(Value::take)
            .ok_or_else(|| "kernel workspace response missing layout".to_string())?;
        let updates = if let Some(updates) = args.get("updates").and_then(Value::as_array) {
            updates
                .iter()
                .map(|update| {
                    let path = update
                        .get("path")
                        .and_then(Value::as_array)
                        .ok_or_else(|| "split ratio update missing path".to_string())?
                        .iter()
                        .map(|value| {
                            value
                                .as_u64()
                                .map(|index| index as usize)
                                .ok_or_else(|| "split ratio path must contain integers".to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let ratios = update
                        .get("ratios")
                        .and_then(Value::as_array)
                        .ok_or_else(|| "split ratio update missing ratios".to_string())?
                        .iter()
                        .map(|value| {
                            value
                                .as_f64()
                                .ok_or_else(|| "split ratios must contain numbers".to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok((path, ratios))
                })
                .collect::<Result<Vec<_>, String>>()?
        } else {
            vec![(
                args.get("path")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "split ratio update missing path".to_string())?
                    .iter()
                    .map(|value| {
                        value
                            .as_u64()
                            .map(|index| index as usize)
                            .ok_or_else(|| "split ratio path must contain integers".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                args.get("ratios")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "split ratio update missing ratios".to_string())?
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .ok_or_else(|| "split ratios must contain numbers".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )]
        };
        for (path, ratios) in updates {
            apply_split_ratios(&mut layout, &path, &ratios)?;
        }
        let panes = pane_metadata_for_layout(&layout)?;
        let topology = request_json(
            &self.current_endpoint(),
            "PUT",
            &format!("/v1/domain/workspaces/{workspace_id}/topology"),
            Some(&json!({ "pane_tree": { "root": layout, "panes": panes } })),
        )?;
        if topology.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err("kernel rejected split ratio topology update".to_string());
        }
        Ok(Value::Null)
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
                    if let Err(error) = Self::seed_workspace_pane(&endpoint, id) {
                        errors.push(format!("seed initial workspace pane: {error}"));
                    }
                    ids.push(id);
                }
            }
        }
        let entries = workspace_snapshot.as_ref().map(|snapshot|snapshot.entries.clone()).unwrap_or_default();
        let active = workspace_snapshot
            .and_then(|snapshot| snapshot.active)
            .and_then(|id| Uuid::parse_str(&id).ok())
            .or_else(|| ids.first().copied());
        KernelSnapshot {
            entries,
            ids,
            active,
            ptys,
            errors,
        }
    }

    fn workspace_id(&self, requested: Option<&str>) -> Result<Uuid, String> {
        let snapshot = self.snapshot();
        Self::workspace_id_from_snapshot(&snapshot, requested)
    }

    fn workspace_id_from_snapshot(
        snapshot: &KernelSnapshot,
        requested: Option<&str>,
    ) -> Result<Uuid, String> {
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
        let mut panes = snapshot
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
            .collect::<Vec<_>>();

        // The kernel graph can contain a stable pane before its PTY is
        // attached (fresh remote workspace, or a shell being rebuilt). Keep
        // that identity visible to the browser so the real pane component can
        // perform its normal create/attach path instead of rendering an empty
        // workspace and waiting for a manual "new terminal" action.
        if let Ok(detail) = request_json(
            &self.current_endpoint(),
            "GET",
            &format!("/v1/domain/workspaces/{workspace_id}"),
            None,
        ) {
            if let Some(graph_panes) = detail.get("panes").and_then(Value::as_array) {
                for pane_id in graph_panes.iter().filter_map(Value::as_str) {
                    if panes.iter().any(|pane| pane["id"] == pane_id) {
                        continue;
                    }
                    panes.push(json!({
                        "id": pane_id,
                        "title": "shell",
                        "cwd": Value::Null,
                        "shell_kind": Value::Null,
                        "status": "detached",
                        "rows": 24,
                        "cols": 80,
                    }));
                }
            }
        }
        panes
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
        Self::resolve_pane_id_with_refresh(args, snapshot, || self.snapshot())
    }

    /// Resolve pane_id from the dispatcher-captured snapshot. On failure, if a
    /// workspace was explicitly requested, re-query the kernel once (stale
    /// snapshot race right after `create_pane`). The cross-workspace check
    /// stays strict in both passes.
    fn resolve_pane_id_with_refresh<F>(
        args: &Value,
        snapshot: &KernelSnapshot,
        refresh: F,
    ) -> Result<Uuid, String>
    where
        F: FnOnce() -> KernelSnapshot,
    {
        match Self::resolve_pane_id(args, snapshot) {
            Ok(id) => Ok(id),
            Err(initial) => {
                let has_workspace = args
                    .get("workspaceId")
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                if !has_workspace {
                    return Err(initial);
                }
                Self::resolve_pane_id(args, &refresh())
            }
        }
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
        if let Some(requested) = args
            .get("paneId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            let pane_id = Uuid::parse_str(requested)
                .map_err(|error| format!("invalid pane id: {requested}: {error}"))?;
            if snapshot
                .ptys
                .iter()
                .any(|pty| pty.pty_id == pane_id && pty.workspace_id == Some(workspace_id))
            {
                return Ok(pane_id);
            }

            let detail = request_json(
                &self.current_endpoint(),
                "GET",
                &format!("/v1/domain/workspaces/{workspace_id}"),
                None,
            )?;
            if !workspace_detail_contains_pane(&detail, pane_id) {
                return Err(format!(
                    "pane {pane_id} does not belong to workspace {workspace_id}"
                ));
            }
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
            return Ok(pane_id);
        }
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

fn apply_split_ratios(layout: &mut Value, path: &[usize], ratios: &[f64]) -> Result<(), String> {
    let mut node = layout;
    for index in path {
        let children = node
            .get_mut("Split")
            .and_then(Value::as_object_mut)
            .and_then(|split| split.get_mut("children"))
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "split ratio path does not reach a split".to_string())?;
        node = children
            .get_mut(*index)
            .ok_or_else(|| format!("split ratio path index out of range: {index}"))?;
    }
    let split = node
        .get_mut("Split")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "split ratio target is not a split".to_string())?;
    let child_count = split
        .get("children")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if child_count != ratios.len() {
        return Err(format!(
            "split ratios len {} != children {}",
            ratios.len(),
            child_count
        ));
    }
    let sum: f64 = ratios.iter().sum();
    if !sum.is_finite() || sum <= f64::EPSILON {
        return Err("split ratios sum is zero or non-finite".to_string());
    }
    split.insert(
        "ratios".to_string(),
        json!(ratios
            .iter()
            .map(|ratio| ratio / sum * 100.0)
            .collect::<Vec<_>>()),
    );
    Ok(())
}

fn pane_metadata_for_layout(layout: &Value) -> Result<serde_json::Map<String, Value>, String> {
    let mut panes = serde_json::Map::new();
    collect_pane_metadata(layout, &mut panes)?;
    Ok(panes)
}

fn collect_pane_metadata(
    node: &Value,
    panes: &mut serde_json::Map<String, Value>,
) -> Result<(), String> {
    if let Some(id) = node.get("Leaf").and_then(Value::as_str) {
        let pane_id =
            Uuid::parse_str(id).map_err(|error| format!("invalid pane id {id}: {error}"))?;
        panes.insert(id.to_string(), json!({ "id": pane_id, "mode": "Terminal" }));
        return Ok(());
    }
    let children = node
        .get("Split")
        .and_then(Value::as_object)
        .and_then(|split| split.get("children"))
        .and_then(Value::as_array)
        .ok_or_else(|| "kernel workspace layout contains an invalid pane node".to_string())?;
    for child in children {
        collect_pane_metadata(child, panes)?;
    }
    Ok(())
}

#[derive(Clone)]
struct KernelSnapshot {
    entries: Vec<ridge_core::commands::workspace::WorkspaceEntry>,
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

    fn is_blacklisted(&self, device_id: &str, ip: &str) -> bool {
        // 当前实现：throttle 的临时 ban 已经在 pre_verify_gate 拦截，持久黑名单
        // 留待桌面共享服务侧补齐；本处只把 throttle 命中一并视为「黑名单态」。
        let _ = (device_id, ip);
        false
    }

    fn pre_verify_gate(&self, ip: &str, device_id: &str) -> Result<(), ()> {
        match self.totp_throttle.check(ip, device_id) {
            ridge_remote::auth::ThrottleDecision::Allow => Ok(()),
            _ => Err(()),
        }
    }

    fn post_verify_record(&self, ip: &str, device_id: &str, valid: bool) {
        if valid {
            self.totp_throttle.record_success(ip, device_id);
        } else {
            self.totp_throttle.record_failure(ip, device_id);
        }
    }

    fn totp_trust_check(&self, ctrl_pub: &[u8]) -> bool {
        // §7.4 trust-grant：复用 CLI 共享 grant_store（与 cloud 腿 session_control 同源）。
        let identity = self.totp.current_identity();
        ridge_core::grant_store::check(&identity, ctrl_pub)
    }

    fn totp_trust_record(&self, ctrl_pub: &[u8]) {
        let identity = self.totp.current_identity();
        ridge_core::grant_store::record(&identity, ctrl_pub);
    }
}

impl WorkspaceProvider for KernelHost {
    fn list_workspaces_json(&self) -> Value {
        let snapshot = self.snapshot();
        json!({
            "workspaces": snapshot.ids.iter().enumerate().map(|(index, id)| json!({
                "id": id.to_string(), "name": snapshot.entries.iter().find(|entry|entry.id==id.to_string()).and_then(|entry|entry.name.as_ref()), "displaySeq": snapshot.entries.iter().find(|entry|entry.id==id.to_string()).map(|entry|entry.display_seq).unwrap_or(index as u64 + 1),
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

    fn create_workspace(&self, name: Option<String>) -> Result<Value, HostError> {
        self.create_workspace_json(name.as_deref())
            .map_err(HostError::BadRequest)
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<Value, HostError> {
        let _ = self
            .workspace_id(Some(workspace_id))
            .map_err(HostError::NotFound)?;
        dispatch_shared_core(self, "close_workspace", &json!({"workspaceId":workspace_id}), false, &self.roots(&self.snapshot()), &Uuid::new_v4().to_string())
            .map_err(|error|HostError::BadRequest(error.message))?;
        Ok(json!({"success":true,"workspaceId":workspace_id}))
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
        Box::pin(async move { run_ws(socket, self, None).await })
    }
}

impl KernelHost {
    pub async fn serve_local_websocket(
        self: Arc<Self>,
        socket: WebSocket,
        connection: WsConn,
        origin: String,
    ) {
        run_ws(socket, self, Some((connection, origin))).await;
    }
}

fn presentation_authorized(host: &KernelHost, binding: Option<&(WsConn, String)>) -> bool {
    binding.is_none_or(|(connection, origin)| {
        connection.token.as_deref().is_some_and(|token| {
            host.sessions.validate_origin_bound_token(
                token,
                &connection.device_id,
                &connection.remote_addr,
                origin,
            )
        })
    })
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

struct DispatchCounters {
    bytes: AtomicUsize,
    enqueued: AtomicUsize,
    dequeued: AtomicUsize,
    started: Instant,
}
struct DispatchTrace {
    at: Instant,
    bytes: usize,
    counters: Arc<DispatchCounters>,
}
impl DispatchCounters {
    fn new() -> Self {
        Self {
            bytes: AtomicUsize::new(0),
            enqueued: AtomicUsize::new(0),
            dequeued: AtomicUsize::new(0),
            started: Instant::now(),
        }
    }
}
impl DispatchTrace {
    fn admit(counters: &Arc<DispatchCounters>, bytes: usize) -> Self {
        counters.bytes.fetch_add(bytes, Ordering::Relaxed);
        counters.enqueued.fetch_add(1, Ordering::Relaxed);
        Self {
            at: Instant::now(),
            bytes,
            counters: counters.clone(),
        }
    }
    fn rollback(self) {
        self.counters.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        self.counters.enqueued.fetch_sub(1, Ordering::Relaxed);
    }
    fn dequeue(self, lane: &'static str, depth: usize) {
        self.counters.bytes.fetch_sub(self.bytes, Ordering::Relaxed);
        let dequeued = self.counters.dequeued.fetch_add(1, Ordering::Relaxed) + 1;
        crate::host_trace!(
            ev = "dispatch_dequeue",
            lane,
            bytes = self.bytes,
            queue_depth = depth,
            queue_bytes = self.counters.bytes.load(Ordering::Relaxed),
            enqueued = self.counters.enqueued.load(Ordering::Relaxed),
            dequeued,
            queue_wait_us = self.at.elapsed().as_micros() as u64,
            queue_lifetime_us = self.counters.started.elapsed().as_micros() as u64
        );
    }
}

async fn run_ws(socket: WebSocket, host: Arc<KernelHost>, binding: Option<(WsConn, String)>) {
    // P2-B8: surface the active kernel transport once per WS upgrade.
    log_kernel_transport_mode();
    let (mut tx, mut rx) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Message>(64);
    let connection_id = Uuid::new_v4().to_string();
    let endpoint = host.current_endpoint();
    let Ok(mut events_request) = format!("ws://127.0.0.1:{}/v1/domain/events?connectionId={connection_id}&native={}", endpoint.port, binding.is_some()).into_client_request() else { return; };
    let Ok(token) = endpoint.token.parse() else { return; };
    events_request.headers_mut().insert("x-ridge-kernel-token", token);
    let Ok(Ok((mut events, _))) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::connect_async(events_request)).await else { return; };

    let subscriptions = Arc::new(Mutex::new(HashMap::<(Uuid, Uuid), u64>::new()));
    let runtime = tokio::runtime::Handle::current();
    let Ok(initial) = tokio::task::spawn_blocking({
        let host = host.clone();
        move || host.snapshot()
    })
    .await
    else {
        return;
    };
    let restored = binding
        .as_ref()
        .and_then(|(connection, _)| connection.token.as_deref())
        .and_then(|token| host.sessions.origin_bound_workspace(token))
        .and_then(|workspace| Uuid::parse_str(&workspace).ok())
        .filter(|workspace|initial.ids.contains(workspace));
    let view = Arc::new(Mutex::new(
        restored
            .or(initial.active)
            .or_else(|| initial.ids.first().copied()),
    ));
    let dispatch_counters = host_trace_enabled().then(|| Arc::new(DispatchCounters::new()));
    let input_counters = host_trace_enabled().then(|| Arc::new(DispatchCounters::new()));
    let (dispatch_tx, mut dispatch_rx) = mpsc::channel::<(Value, Option<DispatchTrace>)>(64);
    let dispatch_out_tx = out_tx.clone();
    let dispatch_host = Arc::clone(&host);
    let dispatch_subscriptions = Arc::clone(&subscriptions);
    let dispatch_runtime = runtime.clone();
    let dispatch_binding = binding.clone();
    let dispatch_connection = connection_id.clone();
    let dispatch_view = view.clone();
    let _dispatch_worker = tokio::task::spawn_blocking(move || {
        while let Some((value, trace)) = dispatch_rx.blocking_recv() {
            if !presentation_authorized(&dispatch_host, dispatch_binding.as_ref()) {
                break;
            }
            if let Some(trace) = trace {
                trace.dequeue("control", dispatch_rx.len());
            }
            let Some(reply) = handle_client_text(
                &value,
                &dispatch_host,
                &dispatch_runtime,
                &dispatch_out_tx,
                &dispatch_subscriptions,
                &dispatch_view,
                dispatch_binding.as_ref(),
                &dispatch_connection,
            ) else {
                continue;
            };
            if dispatch_out_tx.blocking_send(Message::Text(reply)).is_err() {
                break;
            }
        }
    });
    let (input_tx, mut input_rx) = mpsc::channel::<(Value, Option<DispatchTrace>)>(64);
    let input_out_tx = out_tx.clone();
    let input_host = Arc::clone(&host);
    let input_subscriptions = Arc::clone(&subscriptions);
    let input_runtime = runtime;
    let input_binding = binding.clone();
    let input_connection = connection_id.clone();
    let input_view = view.clone();
    let _input_worker = tokio::task::spawn_blocking(move || {
        while let Some((value, trace)) = input_rx.blocking_recv() {
            if !presentation_authorized(&input_host, input_binding.as_ref()) {
                break;
            }
            if let Some(trace) = trace {
                trace.dequeue("input", input_rx.len());
            }
            let Some(reply) = handle_client_text(
                &value,
                &input_host,
                &input_runtime,
                &input_out_tx,
                &input_subscriptions,
                &input_view,
                input_binding.as_ref(),
                &input_connection,
            ) else {
                continue;
            };
            if input_out_tx.blocking_send(Message::Text(reply)).is_err() {
                break;
            }
        }
    });
    let Ok(mut last_panes_frame) = tokio::task::spawn_blocking({
        let host = host.clone();
        let view = view.clone();
        move || client_panes_frame(&host, &view)
    })
    .await
    else {
        return;
    };
    let mut topology_poll = tokio::time::interval(Duration::from_millis(250));
    topology_poll.tick().await;
    if tx
        .send(Message::Text(
            json!({
                "type":"hello",
                "version":1,
                "protocol":"ridge-remote-ws",
                // One generated terminal contract, also used by the TUI adapter.
                "terminalProtocolVersion": ridge_term::terminal_protocol::PROTOCOL_VERSION,
                "terminalMaxFrameBytes": ridge_term::terminal_protocol::MAX_FRAME_BYTES,
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
        if !presentation_authorized(&host, binding.as_ref()) {
            break;
        }
        tokio::select! {
            Some(message) = out_rx.recv() => { if tx.send(message).await.is_err() { break; } }
            event = events.next() => match event {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    if serde_json::from_str::<Value>(&text).is_ok_and(|event|event["name"]=="workspace-list-changed") {
                        let Ok(frame) = tokio::task::spawn_blocking({let host=host.clone();let view=view.clone();move||client_workspaces_frame(&host,&view)}).await else {break;};
                        if tx.send(Message::Text(frame.to_string())).await.is_err() {break;}
                    }
                    if tx.send(Message::Text(text)).await.is_err() { break; }
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(bytes))) => {
                    if events.send(tokio_tungstenite::tungstenite::Message::Pong(bytes)).await.is_err() { break; }
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {},
            },
            _ = topology_poll.tick() => {
                let next_panes_frame = tokio::task::spawn_blocking({
                    let host = Arc::clone(&host);
                    let view = view.clone();
                    move || client_panes_frame(&host, &view)
                }).await;
                let Ok(next_panes_frame) = next_panes_frame else { continue; };
                if panes_frame_changed(&last_panes_frame, &next_panes_frame) {
                    if tx.send(Message::Text(next_panes_frame.to_string())).await.is_err() { break; }
                    last_panes_frame = next_panes_frame;
                }
            }
            message = rx.next() => match message {
                Some(Ok(message)) => match message {
                Message::Text(text) => {
                    let Ok(value) = serde_json::from_str::<Value>(&text) else { continue; };
                    let (target, counters) = if is_latency_sensitive_pty_message(&value) {
                        (&input_tx, &input_counters)
                    } else {
                        (&dispatch_tx, &dispatch_counters)
                    };
                    let trace = counters.as_ref().map(|counters| DispatchTrace::admit(counters, text.len()));
                    if let Err(error) = target.try_send((value, trace)) {
                        let (_, trace) = error.into_inner();
                        if let Some(trace) = trace { trace.rollback(); }
                        break;
                    }
                }
                Message::Ping(bytes) => { if tx.send(Message::Pong(bytes)).await.is_err() { break; } }
                Message::Close(_) => break,
                _ => {}
            },
                _ => break,
            },
            else => break,
        }
    }
}

fn is_latency_sensitive_pty_message(value: &Value) -> bool {
    if matches!(
        value.get("type").and_then(Value::as_str),
        Some("stdin" | "subscribe-pane")
    ) {
        return true;
    }
    let method = value
        .get("method")
        .or_else(|| value.get("cmd"))
        .and_then(Value::as_str);
    matches!(
        method,
        Some(
            "write_terminal_input"
                | "write_to_pty"
                | "write_pty"
                | "subscribe-pane"
                | "subscribe_pane_raw"
        )
    )
}

fn dispatch_shared_core(host: &KernelHost, method: &str, args: &Value, native: bool, roots: &[PathBuf], connection_id: &str) -> Result<Value, ridge_remote::rpc::RpcError> {
    let internal = |message: String| ridge_remote::rpc::RpcError::new(ridge_remote::rpc::JSON_RPC_INTERNAL_ERROR, message);
    let response = request_json(&host.current_endpoint(), "POST", "/v1/domain/commands",
        Some(&json!({"method":method,"args":args,"native":native,"roots":roots,"connectionId":connection_id}))).map_err(internal)?;
    if response["ok"] == true { return Ok(response["value"].clone()); }
    if response["error"]["code"] == -32601 && matches!(method,"git_stage"|"git_unstage"|"git_commit") {
        let mut request = json!({"operation":method.trim_start_matches("git_"),"repo_root":args["repoRoot"]});
        if method=="git_commit" {request["message"]=args["message"].clone();request["amend"]=args["amend"].clone();}
        else {request["paths"]=args["paths"].clone();}
        serde_json::from_value::<crate::domain::GitMutationRequest>(request.clone()).map_err(|error|internal(format!("invalid Git mutation: {error}")))?;
        return crate::client::mutate_domain_git(&host.current_endpoint(),&request).map(|_|Value::Null).map_err(internal);
    }
    Err(serde_json::from_value(response["error"].clone()).unwrap_or_else(|_|internal("invalid shared Runtime command error".into())))
}

fn handle_client_text(
    value: &Value,
    host: &Arc<KernelHost>,
    runtime: &tokio::runtime::Handle,
    out: &mpsc::Sender<Message>,
    subscriptions: &Arc<Mutex<HashMap<(Uuid, Uuid), u64>>>,
    view: &Arc<Mutex<Option<Uuid>>>,
    binding: Option<&(WsConn, String)>,
    connection_id: &str,
) -> Option<String> {
    let jsonrpc = value["jsonrpc"] == "2.0";
    let invoke = value["type"] == "invoke-request";
    let method = value
        .get(if jsonrpc {
            "method"
        } else if invoke {
            "cmd"
        } else {
            "type"
        })
        .and_then(Value::as_str)
        .unwrap_or("");
    let args = if jsonrpc {
        &value["params"]
    } else if invoke {
        &value["args"]
    } else {
        value
    };
    let reply = |result: Result<Value, String>| {
        if jsonrpc {
            value
                .get("id")
                .map(|id| jsonrpc_result(id, result).to_string())
        } else if invoke {
            Some(invoke_result_wire(
                value.get("_reqId").cloned().unwrap_or(Value::Null),
                result,
            ))
        } else {
            Some(
                workspace_wire_result(
                    "switch-workspace-result",
                    result.map_err(HostError::BadRequest),
                )
                .to_string(),
            )
        }
    };
    if (jsonrpc || invoke)
        && matches!(
            method,
            "save_workspace_to_file"
                | "open_workspace_from_file"
                | "get_workspace_save_info"
                | "list_workspace_save_info"
                | "get_default_workspace_save_dir"
                | "list_saved_workspace_files"
        )
    {
        let result = (|| {
            let native = binding.is_some();
            let roots = if native {
                Vec::new()
            } else {
                host.roots(&host.snapshot())
            };
            ridge_core::dispatch::validate_request(
                method,
                args,
                &core_host::runtime_ctx(&roots, native),
            )
            .map_err(|error| error.to_command_string())?;
            let operation = match method {
                "save_workspace_to_file" => "save",
                "open_workspace_from_file" => "open",
                "get_workspace_save_info" => "info",
                "list_workspace_save_info" => "associations",
                "get_default_workspace_save_dir" => "default-directory",
                _ => "list",
            };
            let selected = *view.lock().map_err(|_| "client selection unavailable")?;
            let response = request_json(
                &host.current_endpoint(),
                "POST",
                "/v1/domain/workspace-files",
                Some(&json!({"operation":operation,
                "workspaceId":args.get("workspaceId").cloned().or_else(||selected.map(|id|json!(id))),"path":args.get("path"),"name":args.get("name")})),
            )?;
            if response["ok"] != true {
                return Err(response["error"]
                    .as_str()
                    .unwrap_or("workspace file operation failed")
                    .into());
            }
            let value = response["value"].clone();
            if method == "open_workspace_from_file" {
                let id = value["workspaceId"]
                    .as_str()
                    .and_then(|id| Uuid::parse_str(id).ok())
                    .ok_or("invalid imported workspace identity")?;
                *view.lock().map_err(|_| "client selection unavailable")? = Some(id);
                if let Some(token) = binding.and_then(|(connection, _)| connection.token.as_deref())
                {
                    host.sessions
                        .set_origin_bound_workspace(token, &id.to_string());
                }
                return Ok(json!(id));
            }
            if method == "save_workspace_to_file" {
                Ok(value["path"].clone())
            } else {
                Ok(value)
            }
        })();
        return reply(result);
    }
    if (jsonrpc || invoke)
        && (!is_kernel_host_method(method)
            || matches!(
                    method,
                    "get_theme_data" | "set_active_theme" | "detect_available_shells" | "get_file_tree"
                        | "read_file"
                        | "text_search"
                        | "load_terminal_font_faces"
                        | "read_terminal_font_face_chunk"
                ))
    {
        let native = binding.is_some();
        let roots = if native { Vec::new() } else { host.roots(&host.snapshot()) };
        let result = dispatch_shared_core(host, method, args, native, &roots, connection_id);
        if jsonrpc {
            return value.get("id").map(|id| match result {
                Ok(value) => ridge_remote::rpc::result_response(id, value),
                Err(error) => ridge_remote::rpc::error_response(id, &error),
            }.to_string());
        }
        return reply(result.map_err(|error|error.message));
    }
    if matches!(method, "switch_workspace" | "switch-workspace") {
        let snapshot = host.snapshot();
        let result = KernelHost::workspace_id_from_snapshot(
            &snapshot,
            args.get("workspaceId").and_then(Value::as_str),
        )
        .and_then(|id| {
            *view.lock().map_err(|_| "client selection unavailable")? = Some(id);
            if let Some(token) = binding.and_then(|(connection, _)| connection.token.as_deref()) {
                if !host
                    .sessions
                    .set_origin_bound_workspace(token, &id.to_string())
                {
                    return Err("client session expired".into());
                }
            }
            subscriptions
                .lock()
                .map_err(|_| "client subscriptions unavailable")?
                .retain(|(workspace, _), _| *workspace == id);
            Ok(json!({"success":true,"workspaceId":id}))
        });
        return reply(result);
    }
    let selected = *view.lock().ok()?;
    if method == "get_active_workspace_id" {
        let snapshot = host.snapshot();
        let selected = selected
            .filter(|id|snapshot.ids.contains(id))
            .or(snapshot.active)
            .or_else(|| snapshot.ids.first().copied());
        let result = selected
            .filter(|id| snapshot.ids.contains(id))
            .ok_or_else(|| "client workspace unavailable".to_string())
            .map(|id| {
                if let Ok(mut active) = view.lock() {
                    *active = Some(id);
                }
                if let Some(token) = binding.and_then(|(connection, _)| connection.token.as_deref())
                {
                    host.sessions
                        .set_origin_bound_workspace(token, &id.to_string());
                }
                Value::String(id.to_string())
            });
        return reply(result);
    }
    if matches!(method, "list-panes" | "list-workspace-panes") {
        return Some(client_panes_frame(host, view).to_string());
    }
    let mut scoped = value.clone();
    let params = if jsonrpc {
        &mut scoped["params"]
    } else if invoke {
        &mut scoped["args"]
    } else {
        &mut scoped
    };
    if params.is_null() {
        *params = json!({});
    }
    if let (Some(id), Some(args)) = (selected, params.as_object_mut()) {
        args.entry("workspaceId").or_insert_with(|| json!(id));
    }
    let answer = handle_text(&scoped, host, runtime, out, subscriptions)?;
    // Creating a workspace selects it only for the creating presentation.
    if matches!(method, "create_workspace" | "create-workspace") {
        if let Ok(response) = serde_json::from_str::<Value>(&answer) {
            let result = if jsonrpc {
                &response["result"]
            } else if invoke {
                &response["_result"]
            } else {
                &response
            };
            if let Some(id) = result.as_str().or_else(||result["workspaceId"].as_str())
                .and_then(|id| Uuid::parse_str(id).ok())
            {
                if let Ok(mut active) = view.lock() {
                    *active = Some(id);
                }
                if let Some(token) = binding.and_then(|(connection, _)| connection.token.as_deref())
                {
                    host.sessions
                        .set_origin_bound_workspace(token, &id.to_string());
                }
            }
        }
    }
    if method == "list_workspaces" {
        if let Ok(mut response) = serde_json::from_str::<Value>(&answer) {
            let rows = if jsonrpc {
                &mut response["result"]
            } else {
                &mut response["_result"]
            };
            if let Some(rows) = rows.as_array_mut() {
                for row in rows {
                    row["active"] =
                        json!(row["id"].as_str() == selected.map(|id| id.to_string()).as_deref());
                }
            }
            return Some(response.to_string());
        }
    }
    Some(answer)
}

fn handle_text(
    value: &Value,
    host: &Arc<KernelHost>,
    runtime: &tokio::runtime::Handle,
    out_tx: &mpsc::Sender<Message>,
    subscriptions: &Arc<Mutex<HashMap<(Uuid, Uuid), u64>>>,
) -> Option<String> {
    if value.get("jsonrpc").and_then(Value::as_str) == Some("2.0") {
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let id = value.get("id").cloned();
        if method == "$/hello" {
            let hello = ridge_remote::rpc::negotiate_hello_with_capabilities(
                &params,
                KERNEL_HOST_CAPABILITIES,
            );
            return id.map(|id| {
                ridge_remote::rpc::result_response(
                    &id,
                    hello.get("params").cloned().unwrap_or(hello),
                )
                .to_string()
            });
        }
        if id.is_none() {
            if matches!(method, "subscribe-pane" | "subscribe_pane_raw") {
                let snapshot = host.snapshot();
                start_subscription(&params, host, &snapshot, runtime, out_tx, subscriptions);
            }
            return None;
        }
        let id = id.unwrap();
        return Some(
            jsonrpc_result(
                &id,
                dispatch(method, &params, host, runtime, out_tx, subscriptions),
            )
            .to_string(),
        );
    }
    if value.get("type").and_then(Value::as_str) == Some("invoke-request") {
        let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or("");
        let args = value.get("args").cloned().unwrap_or(Value::Null);
        let result = dispatch(cmd, &args, host, runtime, out_tx, subscriptions);
        return Some(invoke_result_wire(
            value.get("_reqId").cloned().unwrap_or(Value::Null),
            result,
        ));
    }
    match value.get("type").and_then(Value::as_str).unwrap_or("") {
        "ping" => Some(json!({"type":"pong"}).to_string()),
        "list-panes" | "list-workspace-panes" => Some(panes_frame(host).to_string()),
        "subscribe-pane" => {
            let snapshot = host.snapshot();
            start_subscription(value, host, &snapshot, runtime, out_tx, subscriptions);
            None
        }
        "unsubscribe-pane" => {
            if let Some(target) = subscription_target(value, &host.snapshot()) {
                if let Ok(mut active) = subscriptions.lock() {
                    active.remove(&target);
                }
            }
            None
        }
        "stdin" => {
            let _ = dispatch("write_to_pty", value, host, runtime, out_tx, subscriptions);
            None
        }
        "resize" | "claim-pane" | "refresh-pane" => {
            let _ = dispatch("resize_pane", value, host, runtime, out_tx, subscriptions);
            None
        }
        "create-pane" => Some(
            create_pane_wire_result(dispatch(
                "create_pane",
                value,
                host,
                runtime,
                out_tx,
                subscriptions,
            ))
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
            close_pane_wire_result(dispatch(
                "close_pane",
                value,
                host,
                runtime,
                out_tx,
                subscriptions,
            ))
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
        Ok(value) => ridge_remote::rpc::result_response(id, value),
        Err(error) => ridge_remote::rpc::error_response(
            id,
            &ridge_remote::rpc::RpcError::new(ridge_remote::rpc::JSON_RPC_INTERNAL_ERROR, error),
        ),
    }
}

fn pane_resize_owner(value: &Value) -> &'static str {
    match value.get("owner").and_then(Value::as_str) {
        Some("remote") => "remote",
        Some("host") => "host",
        _ if matches!(
            value.get("type").and_then(Value::as_str),
            Some("claim-pane" | "refresh-pane")
        ) =>
        {
            "remote"
        }
        _ => "host",
    }
}

fn dispatch(
    method: &str,
    args: &Value,
    host: &Arc<KernelHost>,
    runtime: &tokio::runtime::Handle,
    out_tx: &mpsc::Sender<Message>,
    subscriptions: &Arc<Mutex<HashMap<(Uuid, Uuid), u64>>>,
) -> Result<Value, String> {
    // Unsupported SPA probes must fail before the snapshot projection. The
    // projection performs multiple kernel HTTP calls; keeping it ahead of the
    // final `other` arm lets a burst of optional probes delay the first pane
    // subscription and makes real keyboard input appear to lose output.
    if !is_kernel_host_method(method) {
        return Err(format!("method not supported by kernel host: {method}"));
    }
    if matches!(
        method,
        "write_terminal_input" | "get_terminal_history" | "get_terminal_snapshot"
    ) {
        let (pane, activation) = if method == "get_terminal_snapshot" {
            (
                json!({"workspaceId":args["workspaceId"],"paneId":args["paneId"]}),
                args["activationId"].as_u64(),
            )
        } else {
            let request = if method == "write_terminal_input" {
                &args["input"]
            } else {
                &args["request"]
            };
            (request["pane"].clone(), request["activationId"].as_u64())
        };
        let workspace = Uuid::parse_str(pane["workspaceId"].as_str().ok_or("missing workspace")?)
            .map_err(|_| "invalid workspace")?;
        let id = Uuid::parse_str(pane["paneId"].as_str().ok_or("missing pane")?)
            .map_err(|_| "invalid pane")?;
        let activation = activation
            .filter(|id| *id > 0)
            .ok_or("invalid activation")?;
        if subscriptions
            .lock()
            .map_err(|_| "subscription lock")?
            .get(&(workspace, id))
            != Some(&activation)
        {
            return Err("terminal activation changed".into());
        }
        let reply = request_json(
            &host.current_endpoint(),
            "POST",
            &format!("/v1/domain/ptys/{id}/terminal"),
            Some(&json!({"method":method,"params":args})),
        )?;
        if reply["ok"] != true {
            return Err(reply["error"]
                .as_str()
                .unwrap_or("terminal request rejected")
                .to_owned());
        }
        return Ok(reply["value"].clone());
    }
    // PTY input already carries its stable pane identity. Let the kernel's
    // authenticated PTY endpoint validate that identity directly; the broad
    // projection below remains only for fallback pane resolution and methods
    // that genuinely need the complete snapshot.
    if matches!(method, "write_to_pty" | "write_pty") {
        if let Some(raw_pane_id) = args
            .get("paneId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            let pane_id = Uuid::parse_str(raw_pane_id)
                .map_err(|error| format!("invalid pane id: {raw_pane_id}: {error}"))?;
            let data = args.get("data").and_then(Value::as_str).unwrap_or("");
            let endpoint = host.current_endpoint();
            // CHG-050 §2.2: fast-path write; mirror the same recv /
            // after instrumentation as the snapshot-projection path so
            // the trace correlates both branches.
            let t0 = host_trace_enabled().then(Instant::now);
            crate::host_trace!(
                ev = "recv_input",
                route = "direct",
                pane_id = %pane_id,
                source_id = trace_input_source(&args),
                input_sequence = args.get("inputSequence").or_else(|| args.get("input_sequence")).and_then(serde_json::Value::as_u64).unwrap_or(0),
                bytes = data.len()
            );
            let result = write_domain_pty(&endpoint, pane_id, data.as_bytes());
            crate::host_trace!(
                ev = "after_write",
                pane_id = %pane_id,
                bytes = data.len(),
                ok = result.is_ok(),
                dt_us = t0.map(|start| start.elapsed().as_micros() as u64).unwrap_or(0)
            );
            result.map_err(|e| e)?;
            return Ok(Value::Null);
        }
    }
    // Workspace creation and optional desktop probes do not need a topology
    // snapshot. Keep them ahead of the synchronous projection so a burst of
    // startup calls cannot delay the first real workspace action.
    if method == "create_workspace" {
        let name = args.get("name").and_then(Value::as_str);
        return host.create_workspace_json(name).map(|value|value["workspaceId"].clone());
    }
    if matches!(
        method,
        "set_split_ratios_at_path" | "set_split_ratios_batch"
    ) {
        return host.set_split_ratios(args);
    }
    if matches!(
        method,
        "detect_available_shells"
            | "get_shell_history"
            | "list_saved_workspaces"
            | "list_workspace_save_info"
            | "list_native_sessions"
            | "set_user_default_cwd"
            | "start_watching_paths"
            | "get_pane_foreground_process"
            | "get_pty_runtime_identity"
            | "get_workspace_memory"
            | "get_orchestration_health"
            | "read_agent_recent_replies"
    ) {
        return match method {
            "detect_available_shells" => {
                serde_json::to_value(ridge_core::commands::shell::detect_available_shells())
                    .map_err(|error| error.to_string())
            }
            "get_shell_history" => ridge_core::commands::shell::get_shell_history()
                .map(|history| json!(history))
                .map_err(|error| error.to_string()),
            _ => Err(format!("runtime capability not implemented: {method}")),
        };
    }
    if method == "list_workspaces" {
        return Ok(host.list_workspaces_json()["workspaces"].clone());
    }
    if method == "get_active_workspace_id" {
        let snapshot = host.snapshot();
        return snapshot
            .active
            .or_else(|| snapshot.ids.first().copied())
            .map(|id| Value::String(id.to_string()))
            .ok_or_else(|| "kernel has no workspace".to_string());
    }
    let snapshot = host.snapshot();
    match method {
        "get_pane_layout" | "get_pane_layout_for" | "get_window_pane_layout" => {
            let id = KernelHost::workspace_id_from_snapshot(
                &snapshot,
                args.get("workspaceId").and_then(Value::as_str),
            )?;
            Ok(host.layout(id, &snapshot))
        }
        "get_workspace_snapshot" => {
            let id = KernelHost::workspace_id_from_snapshot(
                &snapshot,
                args.get("workspaceId").and_then(Value::as_str),
            )?;
            Ok(
                json!({"workspaceId":id,"panes":host.panes(id,&snapshot),"layout":host.layout(id,&snapshot)}),
            )
        }
        "write_to_pty" | "write_pty" => {
            let id = host.pane_id(args, &snapshot)?;
            let data = args.get("data").and_then(Value::as_str).unwrap_or("");
            let endpoint = host.current_endpoint();
            // CHG-050 §2.2: host-side `recv_input` trace records when
            // the host first sees a write request; `after_write` fires
            // after `write_domain_pty` returns so the dt_us covers the
            // snapshot projection + the kernel HTTP POST roundtrip.
            let t0 = host_trace_enabled().then(Instant::now);
            crate::host_trace!(
                ev = "recv_input",
                route = "snapshot",
                pane_id = %id,
                source_id = trace_input_source(&args),
                input_sequence = args.get("inputSequence").or_else(|| args.get("input_sequence")).and_then(serde_json::Value::as_u64).unwrap_or(0),
                bytes = data.len()
            );
            let result = write_domain_pty(&endpoint, id, data.as_bytes());
            crate::host_trace!(
                ev = "after_write",
                pane_id = %id,
                bytes = data.len(),
                ok = result.is_ok(),
                dt_us = t0.map(|start| start.elapsed().as_micros() as u64).unwrap_or(0)
            );
            result.map_err(|e| e)?;
            Ok(Value::Null)
        }
        "resize_pane" | "resize_pty" => {
            let workspace_id =
                host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            let id = host.pane_id(args, &snapshot)?;
            let rows = args.get("rows").and_then(Value::as_u64).unwrap_or(24) as u16;
            let cols = args.get("cols").and_then(Value::as_u64).unwrap_or(80) as u16;
            let owner = pane_resize_owner(args);
            resize_domain_pty(&host.current_endpoint(), id, cols, rows)?;
            let _ = out_tx.try_send(Message::Text(
                json!({
                    "type": "pty-resized",
                    "workspaceId": workspace_id,
                    "paneId": id,
                    "rows": rows,
                    "cols": cols,
                    "owner": owner,
                })
                .to_string(),
            ));
            Ok(Value::Null)
        }
        "subscribe-pane" | "subscribe_pane_raw" | "register_pane_delta_channel" => {
            start_subscription(args, host, &snapshot, runtime, out_tx, subscriptions);
            Ok(Value::Null)
        }
        "create_pane" | "create-pane" => {
            let pane_id = host.create_pane(args, &snapshot)?;
            let workspace_id =
                host.workspace_id(args.get("workspaceId").and_then(Value::as_str))?;
            Ok(json!({"success":true,"workspaceId":workspace_id,"paneId":pane_id}))
        }
        "split_pane" => {
            let mut split_args = args.clone();
            if let Some(object) = split_args.as_object_mut() {
                object.remove("paneId");
            }
            let pane_id = host.create_pane(&split_args, &snapshot)?;
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
        "search" | "search_files" | "get_directory_children" | "list-files" | "get_file_tree" | "read_file" | "text_search" | "load_terminal_font_faces" | "read_terminal_font_face_chunk" | "detect_available_shells" => {
            let method = match method { "list-files" => "get_directory_children", "search_files" => "text_search", other => other };
            let mut params = args.clone();
            if matches!(method, "search" | "text_search") && params.get("root").is_none() {
                params["root"] = args.get("path").cloned().unwrap_or(Value::String(String::new()));
            }
            dispatch_shared_core(host, method, &params, false, &host.roots(&snapshot), &Uuid::new_v4().to_string()).map_err(|error|error.message)
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
                DomainPtyLaunch {
                    pty_id: pane_id,
                    program: shell,
                    args: &shell_args,
                    cwd,
                    workspace_id: Some(workspace_id),
                    role: "shell",
                    launch_profile: Some("remote-lan"),
                    env: Some(&HashMap::new()),
                    cols: None,
                    rows: None,
                },
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
        "use_global_workspace" | "activate_pane_pty" | "set_pane_delta_mode" => Ok(Value::Null),
        other => Err(format!("method not supported by kernel host: {other}")),
    }
}

fn is_kernel_host_method(method: &str) -> bool {
    matches!(
        method,
        "write_terminal_input"
            | "get_terminal_history"
            | "get_terminal_snapshot"
            | "get_active_workspace_id"
            | "list_workspaces"
            | "get_pane_layout"
            | "get_pane_layout_for"
            | "get_window_pane_layout"
            | "get_workspace_snapshot"
            | "write_to_pty"
            | "write_pty"
            | "resize_pane"
            | "resize_pty"
            | "subscribe-pane"
            | "subscribe_pane_raw"
            | "register_pane_delta_channel"
            | "create_pane"
            | "create-pane"
            | "split_pane"
            | "close_pane"
            | "close-pane"
            | "switch_workspace"
            | "create_workspace"
            | "set_split_ratios_at_path"
            | "set_split_ratios_batch"
            | "search"
            | "search_files"
            | "get_directory_children"
            | "list-files"
            | "get_file_tree"
            | "read_file"
            | "text_search"
            | "load_terminal_font_faces"
            | "read_terminal_font_face_chunk"
            | "detect_available_shells"
            | "get_shell_history"
            | "change_pane_shell"
            | "list-git-status"
            | "list_saved_workspace_files"
            | "list_saved_workspaces"
            | "list_workspace_save_info"
            | "list_native_sessions"
            | "set_user_default_cwd"
            | "start_watching_paths"
            | "get_pane_foreground_process"
            | "get_pty_runtime_identity"
            | "get_workspace_memory"
            | "get_orchestration_health"
            | "read_agent_recent_replies"
            | "open_workspace_from_file"
            | "use_global_workspace"
            | "activate_pane_pty"
            | "set_pane_delta_mode"
            | "get_theme_data"
            | "set_active_theme"
    )
}

fn panes_frame(host: &Arc<KernelHost>) -> Value {
    let snapshot = host.snapshot();
    panes_frame_for_snapshot(host, &snapshot)
}

fn client_panes_frame(host: &Arc<KernelHost>, view: &Arc<Mutex<Option<Uuid>>>) -> Value {
    let mut snapshot = host.snapshot();
    if let Ok(mut active) = view.lock() {
        *active = active.filter(|id|snapshot.ids.contains(id)).or(snapshot.active).or_else(||snapshot.ids.first().copied());
        snapshot.active = *active;
    }
    panes_frame_for_snapshot(host, &snapshot)
}

fn client_workspaces_frame(host: &Arc<KernelHost>, view: &Arc<Mutex<Option<Uuid>>>) -> Value {
    let mut frame = host.list_workspaces_json();
    let valid: Vec<_> = frame["workspaces"].as_array().into_iter().flatten().filter_map(|row|row["id"].as_str().and_then(|id|Uuid::parse_str(id).ok())).collect();
    if let Ok(mut active) = view.lock() {
        *active = active.filter(|id|valid.contains(id)).or_else(||valid.first().copied());
        if let Some(rows) = frame["workspaces"].as_array_mut() {
            for row in rows {row["active"]=json!(row["id"].as_str()==active.map(|id|id.to_string()).as_deref());}
        }
    }
    frame["type"] = json!("workspaces");
    frame
}

fn panes_frame_for_snapshot(host: &Arc<KernelHost>, snapshot: &KernelSnapshot) -> Value {
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

fn workspace_detail_contains_pane(value: &Value, pane_id: Uuid) -> bool {
    value
        .get("panes")
        .and_then(Value::as_array)
        .is_some_and(|panes| {
            panes
                .iter()
                .filter_map(Value::as_str)
                .any(|id| id == pane_id.to_string())
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
    runtime: &tokio::runtime::Handle,
    out_tx: &mpsc::Sender<Message>,
    subscriptions: &Arc<Mutex<HashMap<(Uuid, Uuid), u64>>>,
) {
    let Some((workspace_id, pane_id)) = subscription_target(args, snapshot) else {
        return;
    };
    let Some(activation) = args
        .get("activationId")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0 && *id <= 9_007_199_254_740_991)
    else {
        return;
    };
    if let Ok(mut active) = subscriptions.lock() {
        if active.get(&(workspace_id, pane_id)) == Some(&activation) {
            return;
        }
        active.insert((workspace_id, pane_id), activation);
    } else {
        return;
    }
    let endpoint = host.current_endpoint();
    let tx = out_tx.clone();
    let subscriptions = subscriptions.clone();
    runtime.spawn(async move {
        let run = async {
            let mut request = format!("ws://127.0.0.1:{}/v1/domain/ptys/{}/terminal/stream?activation={}", endpoint.port, pane_id, activation).into_client_request().map_err(|error| error.to_string())?;
            request.headers_mut().insert("x-ridge-kernel-token", endpoint.token.parse().map_err(|_| "invalid kernel token")?);
            let (mut socket, _) = tokio_tungstenite::connect_async(request).await.map_err(|error| error.to_string())?;
            loop {
                if tx.is_closed() || subscriptions.lock().map(|active| active.get(&(workspace_id, pane_id)) != Some(&activation)).unwrap_or(true) { break; }
                tokio::select! {
                    frame = socket.next() => match frame {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) if bytes.first() == Some(&0x13) && bytes.len() <= ridge_term::terminal_protocol::MAX_FRAME_BYTES => {
                            if tx.send(Message::Binary(bytes)).await.is_err() { break; }
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(bytes))) => { socket.send(tokio_tungstenite::tungstenite::Message::Pong(bytes)).await.map_err(|error| error.to_string())?; }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => break,
                        Some(Err(error)) => return Err(error.to_string()),
                        _ => {}
                    },
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
            }
            let _ = socket.close(None).await;
            Ok::<(), String>(())
        }.await;
        if let Ok(mut active) = subscriptions.lock() { if active.get(&(workspace_id, pane_id)) == Some(&activation) { active.remove(&(workspace_id, pane_id)); } }
        if let Err(error) = run { tracing::warn!(%pane_id, %error, "canonical terminal subscription failed"); }
    });
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

#[cfg(test)]
fn pty_metadata_frame(workspace_id: Uuid, pane_id: Uuid, bytes: &[u8]) -> Option<Value> {
    let title = ridge_core::pty::title::parse_title_from_output(bytes);
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

#[cfg(test)]
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
    #[tokio::test]
    async fn dispatch_trace_preserves_fifo_payload_and_rolls_back_failed_admission() {
        use super::{DispatchCounters, DispatchTrace};
        let counters = std::sync::Arc::new(DispatchCounters::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for id in [1, 2] {
            assert!(tx
                .send((
                    serde_json::json!({"id":id,"data":"unchanged"}),
                    Some(DispatchTrace::admit(&counters, 20))
                ))
                .is_ok());
        }
        for id in [1, 2] {
            let (value, trace) = rx.recv().await.unwrap();
            assert_eq!(value, serde_json::json!({"id":id,"data":"unchanged"}));
            trace.unwrap().dequeue("input", rx.len());
        }
        assert_eq!(counters.bytes.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(
            counters.dequeued.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
        drop(rx);
        if let Err(tokio::sync::mpsc::error::SendError((_, Some(trace)))) = tx.send((
            serde_json::json!({}),
            Some(DispatchTrace::admit(&counters, 9)),
        )) {
            trace.rollback();
        }
        assert_eq!(counters.bytes.load(std::sync::atomic::Ordering::Relaxed), 0);
        assert_eq!(
            counters.enqueued.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }
    #[test]
    fn input_trace_keeps_bounded_identifiers_only() {
        assert_eq!(
            super::trace_input_source(&serde_json::json!({"inputSourceId":"source_1:lane-2"})),
            "source_1:lane-2"
        );
        assert_eq!(
            super::trace_input_source(&serde_json::json!({"input_source_id":"source_2"})),
            "source_2"
        );
        for value in [
            "",
            "terminal command data",
            "https://example.invalid",
            &"x".repeat(65),
        ] {
            assert_eq!(
                super::trace_input_source(&serde_json::json!({"inputSourceId":value})),
                "unavailable"
            );
        }
    }
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
            child_pid: None,
            child_start_instance: None,
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
    fn unsupported_kernel_host_method_is_classified_before_snapshot() {
        assert!(is_kernel_host_method("subscribe_pane_raw"));
        assert!(is_kernel_host_method("write_to_pty"));
        assert!(is_kernel_host_method("get_pane_foreground_process"));
        assert!(is_kernel_host_method("start_watching_paths"));
        assert!(is_kernel_host_method("get_shell_history"));
    }

    #[test]
    fn pty_subscription_shares_the_latency_sensitive_queue_with_input() {
        assert!(is_latency_sensitive_pty_message(
            &json!({ "type": "stdin" })
        ));
        assert!(is_latency_sensitive_pty_message(
            &json!({ "type": "subscribe-pane" })
        ));
        assert!(is_latency_sensitive_pty_message(
            &json!({ "type": "invoke-request", "cmd": "subscribe_pane_raw" })
        ));
        assert!(!is_latency_sensitive_pty_message(
            &json!({ "type": "invoke-request", "cmd": "get_workspace_snapshot" })
        ));
    }

    #[test]
    fn kernel_layout_is_converted_to_remote_shape() {
        let snapshot = KernelSnapshot {
            entries: Vec::new(),
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
                child_pid: None,
            child_start_instance: None,
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
            entries: Vec::new(),
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
                child_pid: None,
            child_start_instance: None,
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
            entries: Vec::new(),
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
            entries: Vec::new(),
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
    fn pane_id_refresh_resolves_when_kernel_registered_pane_after_snapshot_capture() {
        let workspace = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let stale = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace],
            active: Some(workspace),
            ptys: Vec::new(),
            errors: Vec::new(),
        };
        let fresh = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace],
            active: Some(workspace),
            ptys: vec![test_pty(pane, workspace)],
            errors: Vec::new(),
        };

        assert_eq!(
            KernelHost::resolve_pane_id_with_refresh(
                &json!({"paneId": pane, "workspaceId": workspace}),
                &stale,
                || fresh.clone(),
            ),
            Ok(pane)
        );
    }

    #[test]
    fn pane_id_refresh_still_rejects_cross_workspace_even_after_snapshot_update() {
        let workspace_a = Uuid::new_v4();
        let workspace_b = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let stale = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_a),
            ptys: Vec::new(),
            errors: Vec::new(),
        };
        let fresh = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_a),
            ptys: vec![test_pty(pane, workspace_a)],
            errors: Vec::new(),
        };

        let error = KernelHost::resolve_pane_id_with_refresh(
            &json!({"paneId": pane, "workspaceId": workspace_b}),
            &stale,
            || fresh.clone(),
        )
        .unwrap_err();
        assert!(error.contains("does not belong"));
    }

    #[test]
    fn pane_id_refresh_skipped_when_no_workspace_requested() {
        let workspace = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let stale = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace],
            active: Some(workspace),
            ptys: vec![test_pty(pane, workspace)],
            errors: Vec::new(),
        };
        let mut refresh_calls = 0;
        let result =
            KernelHost::resolve_pane_id_with_refresh(&json!({"paneId": pane}), &stale, || {
                refresh_calls += 1;
                stale.clone()
            });
        assert!(result.is_ok());
        assert_eq!(refresh_calls, 0, "no workspace requested → no refresh");
    }

    #[test]
    fn pane_id_refresh_then_ab_workspace_replay_keeps_correct_owner() {
        let workspace_a = Uuid::new_v4();
        let workspace_b = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let pane_b = Uuid::new_v4();
        let stale = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_a),
            ptys: vec![test_pty(pane_a, workspace_a)],
            errors: Vec::new(),
        };
        let fresh = KernelSnapshot {
            entries: Vec::new(),
            ids: vec![workspace_a, workspace_b],
            active: Some(workspace_b),
            ptys: vec![test_pty(pane_a, workspace_a), test_pty(pane_b, workspace_b)],
            errors: Vec::new(),
        };

        // First refresh resolves A; second refresh resolves B; third refresh
        // resolves A again — none cross-resolves.
        let r1 = KernelHost::resolve_pane_id_with_refresh(
            &json!({"paneId": pane_a, "workspaceId": workspace_a}),
            &stale,
            || fresh.clone(),
        );
        let r2 = KernelHost::resolve_pane_id_with_refresh(
            &json!({"paneId": pane_b, "workspaceId": workspace_b}),
            &stale,
            || fresh.clone(),
        );
        let r3 = KernelHost::resolve_pane_id_with_refresh(
            &json!({"paneId": pane_a, "workspaceId": workspace_a}),
            &stale,
            || fresh.clone(),
        );
        let cross = KernelHost::resolve_pane_id_with_refresh(
            &json!({"paneId": pane_a, "workspaceId": workspace_b}),
            &stale,
            || fresh.clone(),
        );
        assert_eq!(r1, Ok(pane_a));
        assert_eq!(r2, Ok(pane_b));
        assert_eq!(r3, Ok(pane_a));
        assert!(cross.unwrap_err().contains("does not belong"));
    }

    #[test]
    fn subscription_target_requires_matching_workspace_and_accepts_nested_params() {
        let workspace_a = Uuid::new_v4();
        let workspace_b = Uuid::new_v4();
        let pane_a = Uuid::new_v4();
        let snapshot = KernelSnapshot {
            entries: Vec::new(),
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
    fn workspace_detail_contains_requested_pane_only() {
        let pane_id = Uuid::from_u128(1);
        assert!(workspace_detail_contains_pane(
            &json!({ "panes": [pane_id.to_string()] }),
            pane_id
        ));
        assert!(!workspace_detail_contains_pane(
            &json!({ "panes": [Uuid::from_u128(2).to_string()] }),
            pane_id
        ));
        assert!(!workspace_detail_contains_pane(
            &json!({ "panes": [] }),
            pane_id
        ));
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

    #[test]
    fn pty_metadata_frame_keeps_latest_osc2_after_osc0() {
        let workspace_id = Uuid::new_v4();
        let pane_id = Uuid::new_v4();
        let frame = pty_metadata_frame(
            workspace_id,
            pane_id,
            b"\x1b]0;prompt\x07\x1b]2;Codex working\x07",
        )
        .expect("OSC title should produce a frame");
        assert_eq!(frame["title"], "Codex working");
    }

    #[test]
    fn resize_owner_defaults_closed_and_preserves_known_endpoints() {
        assert_eq!(pane_resize_owner(&json!({})), "host");
        assert_eq!(pane_resize_owner(&json!({ "owner": "unknown" })), "host");
        assert_eq!(pane_resize_owner(&json!({ "owner": "remote" })), "remote");
        assert_eq!(
            pane_resize_owner(&json!({ "type": "refresh-pane" })),
            "remote"
        );
    }

    // ── §7.4 HostAuth 5 方法委托（throttle + grant_store）────────────────────

    /// 构造一个最小可测的 KernelHost（不需真 kernel endpoint）。
    fn build_host_for_throttle_test() -> KernelHost {
        KernelHost {
            endpoint: KernelEndpoint {
                pid: 1,
                port: 1111,
                token: "t".into(),
                started_at_unix: 0,
            },
            totp: Arc::new(ridge_remote::auth::RemoteAuth::new()),
            totp_throttle: Arc::new(VerifyThrottle::new()),
            sessions: SessionStore::new(),
            port: 9527,
            lan_ip: "127.0.0.1".into(),
            machine_name: "t".into(),
            serve_cfg: UaServeConfig {
                remote_dir: std::path::PathBuf::from("remote-dist"),
            },
            tls_enabled: false,
            remote_enabled: Arc::new(AtomicBool::new(true)),
        }
    }

    #[test]
    fn pre_verify_gate_delegates_to_throttle() {
        let h = build_host_for_throttle_test();
        // 新键必放行。
        assert!(h.pre_verify_gate("1.2.3.4", "devA").is_ok());
        // 11 次连败（越过 THROTTLE_HARD_LIMIT=10）触发硬 ban。
        for _ in 0..11 {
            h.post_verify_record("1.2.3.4", "devA", false);
        }
        assert!(
            h.pre_verify_gate("1.2.3.4", "devA").is_err(),
            "11 连败后预检闸门必拒"
        );
    }

    #[test]
    fn post_verify_record_success_clears_backoff() {
        let h = build_host_for_throttle_test();
        // 几次失败推进到 backoff。
        for _ in 0..3 {
            h.post_verify_record("5.6.7.8", "devB", false);
        }
        // 成功清空 → 下一发仍允许。
        h.post_verify_record("5.6.7.8", "devB", true);
        assert!(h.pre_verify_gate("5.6.7.8", "devB").is_ok());
    }
}
