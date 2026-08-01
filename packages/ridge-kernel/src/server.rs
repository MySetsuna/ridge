//! Ridge 内核进程（独立于桌面/rdg 外壳）。
//!
//! - 控制面：`/v1/health|status|shutdown`
//! - 领域只读：`/v1/domain/*`（FS list、Agent profiles）
//! - 共享 MCP：`POST /api/v1/mcp`、`GET /api/v1/mcp/ws`（无 Tauri 可联）
//!
//! 发现：`%LOCALAPPDATA%/ridge/kernel.pid` + `kernel.json`

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::client::{running_endpoint, wait_for_running, KERNEL_PROTOCOL_VERSION};
use crate::registry::{
    clear_registry, load_remote_hosts, load_roster, load_workspace_graph, remote_hosts_path,
    roster_path, workspace_graph_path, write_registry, KernelEndpoint, KernelInstanceGuard,
};
use crate::{domain, kernel_mcp::KernelMcpHost, pty::PtyRegistry};

#[derive(Clone)]
pub struct AppState {
    pub token: String,
    pub pid: u32,
    pub port: u16,
    pub started_at_unix: u64,
    pub shutdown_tx: Arc<std::sync::Mutex<Option<oneshot::Sender<()>>>>,
    pub shutting_down: Arc<AtomicBool>,
    /// Shell-neutral workspace topology for the kernel API. Shell migration is
    /// deliberately separate, so this does not claim existing shells migrated.
    pub workspaces: Arc<std::sync::Mutex<ridge_core::workspace::graph::WorkspaceGraph>>,
    pub workspaces_path: std::path::PathBuf,
    /// Kernel-owned Agent roster/topology; process binding remains a host adapter.
    pub roster: Arc<std::sync::Mutex<ridge_core::teammate::topology::TopologyGraph>>,
    pub roster_path: std::path::PathBuf,
    pub groups: Arc<std::sync::Mutex<HashMap<String, std::collections::HashSet<Uuid>>>>,
    pub mcp_state: Arc<ridge_mcp::server::McpSessionState>,
    /// Kernel-owned remote host topology; shells only project or transport it.
    pub remote_hosts: Arc<std::sync::Mutex<ridge_core::remote::RemoteHostTopology>>,
    pub remote_hosts_path: std::path::PathBuf,
    /// PTY process lifetime belongs to the kernel, not an API shell.
    pub ptys: Arc<PtyRegistry>,
}

#[derive(Serialize)]
struct HealthBody {
    ok: bool,
    role: &'static str,
    #[serde(rename = "protocolVersion")]
    protocol_version: u64,
    pid: u32,
    started_at_unix: u64,
    domain: &'static [&'static str],
}

#[derive(Serialize)]
struct StatusBody {
    ok: bool,
    role: &'static str,
    pid: u32,
    port: u16,
    started_at_unix: u64,
    mcp: &'static str,
    domain: &'static [&'static str],
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("x-ridge-kernel-token")
        .or_else(|| headers.get("x-ridge-token"))
        .and_then(|v| v.to_str().ok())
        == Some(token)
}

async fn health(State(st): State<AppState>) -> Json<HealthBody> {
    Json(HealthBody {
        ok: !st.shutting_down.load(Ordering::Acquire),
        role: "ridge-kernel",
        protocol_version: KERNEL_PROTOCOL_VERSION,
        pid: st.pid,
        started_at_unix: st.started_at_unix,
        domain: &[
            "fs.list",
            "agents.profiles",
            "agents.roster",
            "git.status",
            "ptys.lifecycle",
            "remote.hosts",
            "workspaces",
            "mcp",
        ],
    })
}

async fn status(
    State(st): State<AppState>,
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<SocketAddr>,
) -> Result<Json<StatusBody>, StatusCode> {
    let _ = addr;
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(StatusBody {
        ok: true,
        role: "ridge-kernel",
        pid: st.pid,
        port: st.port,
        started_at_unix: st.started_at_unix,
        mcp: "/api/v1/mcp",
        domain: &[
            "fs.list",
            "agents.profiles",
            "agents.roster",
            "git.status",
            "ptys.lifecycle",
            "remote.hosts",
            "workspaces",
            "mcp",
        ],
    }))
}

async fn shutdown(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if st.shutting_down.swap(true, Ordering::AcqRel) {
        return Ok(Json(serde_json::json!({ "ok": true, "already": true })));
    }
    if let Some(tx) = st.shutdown_tx.lock().ok().and_then(|mut g| g.take()) {
        let _ = tx.send(());
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn run(host: &str, requested_port: u16) -> Result<()> {
    if let Some(endpoint) = running_endpoint() {
        tracing::info!(
            pid = endpoint.pid,
            port = endpoint.port,
            "ridge-kernel already running"
        );
        return Ok(());
    }
    let _instance_guard = match KernelInstanceGuard::try_acquire()? {
        Some(guard) => guard,
        None => {
            if let Some(endpoint) = wait_for_running(Duration::from_secs(8)) {
                tracing::info!(
                    pid = endpoint.pid,
                    port = endpoint.port,
                    "attached to concurrently starting ridge-kernel"
                );
                return Ok(());
            }
            anyhow::bail!("another ridge-kernel owns the instance lock but did not become healthy");
        }
    };
    let token = Uuid::new_v4().to_string();
    let pid = std::process::id();
    let started_at_unix = now_unix();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let bind_addr: SocketAddr = format!("{host}:{requested_port}")
        .parse()
        .context("parse bind addr")?;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .context("bind control plane")?;
    let local = listener.local_addr().context("local_addr")?;
    let port = local.port();

    let state = AppState {
        token: token.clone(),
        pid,
        port,
        started_at_unix,
        shutdown_tx: Arc::new(std::sync::Mutex::new(Some(shutdown_tx))),
        shutting_down: Arc::new(AtomicBool::new(false)),
        workspaces: Arc::new(std::sync::Mutex::new(
            load_workspace_graph().unwrap_or_else(|error| {
                tracing::warn!(%error, "workspace topology restore failed; starting empty");
                ridge_core::workspace::graph::WorkspaceGraph::new()
            }),
        )),
        workspaces_path: workspace_graph_path(),
        roster: Arc::new(std::sync::Mutex::new(load_roster().unwrap_or_else(
            |error| {
                tracing::warn!(%error, "agent roster restore failed; starting empty");
                ridge_core::teammate::topology::TopologyGraph::new()
            },
        ))),
        roster_path: roster_path(),
        groups: Arc::new(std::sync::Mutex::new(HashMap::new())),
        mcp_state: Arc::new(ridge_mcp::server::McpSessionState::default()),
        remote_hosts: Arc::new(std::sync::Mutex::new(
            ridge_core::remote::RemoteHostTopology::from_records(
                load_remote_hosts().unwrap_or_else(|error| {
                    tracing::warn!(%error, "remote host topology restore failed; starting empty");
                    HashMap::new()
                }),
            ),
        )),
        remote_hosts_path: remote_hosts_path(),
        ptys: Arc::new(PtyRegistry::default()),
    };

    let mcp = KernelMcpHost::router(state.clone(), Arc::new(token.clone()));
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/shutdown", post(shutdown))
        .route("/v1/domain", get(domain::domain_meta))
        .route("/v1/domain/agents", get(domain::domain_agents))
        .route(
            "/v1/domain/agents/roster",
            get(domain::domain_agent_roster).post(domain::domain_agent_roster_add),
        )
        .route(
            "/v1/domain/agents/roster/:agent_id",
            delete(domain::domain_agent_roster_remove),
        )
        .route("/v1/domain/fs/list", get(domain::domain_fs_list))
        .route("/v1/domain/git/status", get(domain::domain_git_status))
        .route(
            "/v1/domain/remote-hosts",
            get(domain::domain_remote_hosts).post(domain::domain_remote_host_upsert),
        )
        .route(
            "/v1/domain/remote-hosts/:host_id",
            delete(domain::domain_remote_host_remove),
        )
        .route("/v1/domain/ptys", post(domain::domain_pty_create))
        .route(
            "/v1/domain/ptys/:pty_id/write",
            post(domain::domain_pty_write),
        )
        .route(
            "/v1/domain/ptys/:pty_id/resize",
            post(domain::domain_pty_resize),
        )
        .route(
            "/v1/domain/ptys/:pty_id/clear",
            post(domain::domain_pty_clear),
        )
        .route(
            "/v1/domain/ptys/:pty_id",
            get(domain::domain_pty_scrollback).delete(domain::domain_pty_destroy),
        )
        .route(
            "/v1/domain/workspaces",
            get(domain::domain_workspaces).post(domain::domain_workspace_create),
        )
        .route(
            "/v1/domain/workspaces/:workspace_id",
            get(domain::domain_workspace_get),
        )
        .route(
            "/v1/domain/workspaces/:workspace_id/activate",
            post(domain::domain_workspace_activate),
        )
        .route(
            "/v1/domain/workspaces/:workspace_id/split",
            post(domain::domain_workspace_split),
        )
        .route(
            "/v1/domain/workspaces/:workspace_id/panes/:pane_id/close",
            post(domain::domain_workspace_pane_close),
        )
        .route(
            "/v1/domain/workspaces/:workspace_id/panes/:pane_id/locked-size",
            post(domain::domain_workspace_pane_locked_size),
        )
        .with_state(state)
        .merge(mcp);

    write_registry(&KernelEndpoint {
        pid,
        port,
        token: token.clone(),
        started_at_unix,
    })
    .context("write kernel registry")?;

    tracing::info!(
        target: "ridge_kernel",
        %local,
        pid,
        "ridge-kernel listening (control + domain + mcp)"
    );

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );
    tokio::select! {
        r = server => {
            clear_registry(pid);
            r.context("serve")?;
        }
        _ = shutdown_rx => {
            tracing::info!(target: "ridge_kernel", "shutdown requested");
            clear_registry(pid);
        }
        _ = tokio::signal::ctrl_c() => {
            tracing::info!(target: "ridge_kernel", "ctrl-c");
            clear_registry(pid);
        }
    }
    Ok(())
}
