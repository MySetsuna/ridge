//! Kernel domain endpoints (REQ-RIDGE-KERNEL-DOMAIN-01 first slices).
//! FS and workspace topology use ridge-core; Agent profiles are kernel-owned.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::pty::{PtyLaunch, PtyOutputLeaseError, PtyOutputRead};
use crate::registry::{save_remote_hosts_at, save_roster_at, save_workspace_graph_at};
use crate::server::{AppState, OutputLeaseSlot};

use ridge_core::teammate::communication::{AgentIdentity, CommunicationError};
use ridge_core::workspace::graph::WorkspaceMeta;
use ridge_core::workspace::pane_tree::PaneTree;
#[cfg(test)]
use ridge_mcp::delivery::HubDeliveryAdapter;

/// Bound HTTP lease handles even when clients disappear without DELETE. The
/// PTY replay bytes remain bounded separately by `PtyOutputHub`.
const MAX_OUTPUT_HTTP_LEASES: usize = 1024;

pub(crate) const KERNEL_FS_ROOT_ENV: &str = "RIDGE_KERNEL_FS_ROOT";
const DEFAULT_OUTPUT_POLL_TIMEOUT_MS: u64 = 15_000;
const MAX_OUTPUT_POLL_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_OUTPUT_POLL_FRAMES: usize = 32;
const MAX_OUTPUT_POLL_FRAMES: usize = 64;

/// Read the host-granted filesystem root once when the kernel starts. Empty or
/// unset keeps the existing desktop compatibility mode; a non-empty root makes
/// the kernel's FS domain endpoint the enforcing authority for that process.
pub(crate) fn fs_scope_from_env() -> ridge_core::sandbox::RootScope {
    std::env::var_os(KERNEL_FS_ROOT_ENV)
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(|value| ridge_core::sandbox::RootScope::from_roots([value]))
        .unwrap_or_else(ridge_core::sandbox::RootScope::unrestricted)
}

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("x-ridge-kernel-token")
        .or_else(|| headers.get("x-ridge-token"))
        .and_then(|v| v.to_str().ok())
        == Some(token)
}

/// Kernel-owned default agent profiles.
pub fn builtin_agent_profiles() -> Value {
    serde_json::to_value(crate::agent_profiles::builtin_profiles())
        .expect("AgentProfile serializes")
}

#[derive(Serialize)]
pub struct DomainMeta {
    pub ok: bool,
    pub capabilities: &'static [&'static str],
}

pub async fn domain_meta(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DomainMeta>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(DomainMeta {
        ok: true,
        capabilities: &[
            "fs.list",
            "agents.profiles",
            "agents.roster",
            "git.status",
            "ptys.lifecycle",
            "ptys.output-lease",
            "remote.hosts",
            "workspaces",
            "mcp",
        ],
    }))
}

fn bad_request(message: impl Into<String>) -> Json<Value> {
    Json(json!({ "ok": false, "error": message.into() }))
}

fn parse_id(raw: &str, kind: &str) -> Result<Uuid, Json<Value>> {
    Uuid::parse_str(raw).map_err(|_| bad_request(format!("invalid {kind} id")))
}

fn workspace_list(graph: &ridge_core::workspace::graph::WorkspaceGraph) -> Value {
    let mut ids: Vec<String> = graph.workspace_ids().map(ToString::to_string).collect();
    ids.sort();
    json!({
        "ok": true,
        "source": "ridge-kernel",
        "active": graph.active().map(|id| id.to_string()),
        "workspaces": ids,
    })
}

fn workspace_detail(
    graph: &ridge_core::workspace::graph::WorkspaceGraph,
    workspace_id: Uuid,
) -> Result<Value, Json<Value>> {
    let layout = graph
        .layout(workspace_id)
        .map_err(|e| bad_request(e.to_string()))?;
    let leaves = graph
        .leaves(workspace_id)
        .map_err(|e| bad_request(e.to_string()))?
        .into_iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "source": "ridge-kernel",
        "workspace_id": workspace_id,
        "active": graph.active() == Some(workspace_id),
        "layout": layout,
        "panes": leaves,
    }))
}

fn persist_workspaces(
    st: &AppState,
    graph: &ridge_core::workspace::graph::WorkspaceGraph,
) -> Result<(), StatusCode> {
    save_workspace_graph_at(&st.workspaces_path, graph)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn persist_roster(
    st: &AppState,
    roster: &ridge_core::teammate::topology::TopologyGraph,
) -> Result<(), StatusCode> {
    save_roster_at(&st.roster_path, roster).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Commit the stable Agent identity only after the PTY process has spawned.
/// Ordinary shell panes remain diagnostic/runtime state, not Agent entries.
pub(crate) fn commit_agent_identity_for_pty(
    st: &AppState,
    pane_id: Uuid,
    role: &str,
    executable: &str,
    argv: Vec<String>,
) -> Result<bool, String> {
    if role.trim().is_empty() || role.eq_ignore_ascii_case("shell") {
        return Ok(false);
    }
    let info = st.ptys.info(pane_id).map_err(|error| error.to_string())?;
    let workspace_id = info
        .workspace_id
        .ok_or_else(|| "Agent PTY must belong to a workspace".to_string())?;
    let agent_id = format!("kernel:{pane_id}");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0);
    let mut roster = st.roster.lock().expect("roster lock");
    let mut next = roster.clone();
    let generation = next.next_agent_generation(&agent_id);
    next.commit_online_agent(ridge_core::teammate::communication::AgentIdentity {
        agent_id: agent_id.clone(),
        session_id: format!("session:{}", Uuid::new_v4()),
        workspace_id: workspace_id.to_string(),
        pane_id: pane_id.to_string(),
        cwd: info.cwd.unwrap_or_default(),
        executable: executable.to_string(),
        argv,
        generation,
        lease: Uuid::new_v4().to_string(),
        lifecycle: ridge_core::teammate::communication::AgentLifecycle::Online,
        online: true,
        last_seen_unix_ms: timestamp,
        capabilities: vec!["messages".into(), "tasks".into(), "events".into()],
    })
    .map_err(|error| error.to_string())?;
    save_roster_at(&st.roster_path, &next).map_err(|error| error.to_string())?;
    *roster = next;
    Ok(true)
}

/// Remove a live identity only after the corresponding PTY destroy succeeded.
pub(crate) fn remove_agent_identity_for_pty(st: &AppState, pane_id: Uuid) -> Result<bool, String> {
    let mut roster = st.roster.lock().expect("roster lock");
    let mut next = roster.clone();
    let Some(identity) = next.remove_agent_identity_by_pane(&pane_id.to_string()) else {
        return Ok(false);
    };
    save_roster_at(&st.roster_path, &next).map_err(|error| error.to_string())?;
    *roster = next;
    st.mcp_state.purge_identity(
        &identity.workspace_id,
        &identity.agent_id,
        identity.generation,
        &identity.lease,
    )?;
    Ok(true)
}

/// Kernel-owned remote host topology. Transport connection remains a shell adapter.
pub async fn domain_remote_hosts(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let hosts = st.remote_hosts.lock().expect("remote host lock").snapshot();
    Ok(Json(
        json!({ "ok": true, "source": "ridge-kernel", "hosts": hosts }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct RemoteHostSessionMutation {
    pub host_id: String,
    pub session_id: String,
}

/// Apply one remote-session attachment transition to a topology clone.
///
/// The caller persists and swaps the clone only after this validation succeeds,
/// so a rejected duplicate/unknown transition cannot partially mutate the
/// kernel-owned record. Keeping this operation separate from the HTTP handlers
/// also makes the state-machine contract deterministic to test.
fn mutate_remote_host_session(
    hosts: &mut ridge_core::remote::RemoteHostTopology,
    host_id: &str,
    session_id: &str,
    attached: bool,
) -> Result<(), String> {
    if host_id.trim().is_empty() || session_id.trim().is_empty() {
        return Err("host_id and session_id are required".into());
    }
    let mut host = hosts
        .get(host_id)
        .ok_or_else(|| format!("unknown remote host: {host_id}"))?;
    let session = host
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .ok_or_else(|| format!("unknown remote session: {session_id}"))?;
    if session.attached == attached {
        return Err(if attached {
            format!("remote session already attached: {session_id}")
        } else {
            format!("remote session already detached: {session_id}")
        });
    }
    session.attached = attached;
    hosts.upsert(host);
    Ok(())
}

async fn mutate_remote_host_session_endpoint(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RemoteHostSessionMutation>,
    attached: bool,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut hosts = st.remote_hosts.lock().expect("remote host lock");
    let mut next = hosts.clone();
    if let Err(error) =
        mutate_remote_host_session(&mut next, &request.host_id, &request.session_id, attached)
    {
        return Ok(bad_request(error));
    }
    save_remote_hosts_at(&st.remote_hosts_path, next.records())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    *hosts = next;
    Ok(Json(json!({
        "ok": true,
        "source": "ridge-kernel",
        "host_id": request.host_id,
        "session_id": request.session_id,
        "attached": attached,
    })))
}

pub async fn domain_remote_host_session_attach(
    state: State<AppState>,
    headers: HeaderMap,
    request: Json<RemoteHostSessionMutation>,
) -> Result<Json<Value>, StatusCode> {
    mutate_remote_host_session_endpoint(state, headers, request, true).await
}

pub async fn domain_remote_host_session_detach(
    state: State<AppState>,
    headers: HeaderMap,
    request: Json<RemoteHostSessionMutation>,
) -> Result<Json<Value>, StatusCode> {
    mutate_remote_host_session_endpoint(state, headers, request, false).await
}

pub async fn domain_remote_host_upsert(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(host): Json<ridge_core::remote::HostRecord>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if host.id.trim().is_empty() {
        return Ok(bad_request("remote host id is required"));
    }
    let id = host.id.clone();
    let mut hosts = st.remote_hosts.lock().expect("remote host lock");
    let mut next = hosts.clone();
    next.upsert(host);
    save_remote_hosts_at(&st.remote_hosts_path, next.records())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    *hosts = next;
    Ok(Json(json!({ "ok": true, "host_id": id })))
}

pub async fn domain_remote_host_remove(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(host_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut hosts = st.remote_hosts.lock().expect("remote host lock");
    let mut next = hosts.clone();
    let removed = next.remove(&host_id);
    save_remote_hosts_at(&st.remote_hosts_path, next.records())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    *hosts = next;
    Ok(Json(
        json!({ "ok": true, "removed": removed, "host_id": host_id }),
    ))
}

/// Kernel-owned workspace topology. Existing shell migration remains explicit.
pub async fn domain_workspaces(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(workspace_list(
        &st.workspaces.lock().expect("workspace graph lock"),
    )))
}

pub async fn domain_workspace_create(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    let mut next = graph.clone();
    let workspace_id = next.create_workspace();
    persist_workspaces(&st, &next)?;
    *graph = next;
    Ok(Json(json!({
        "ok": true,
        "source": "ridge-kernel",
        "workspace_id": workspace_id,
        "active": true,
    })))
}

pub async fn domain_workspace_get(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let workspace_id = match parse_id(&workspace_id, "workspace") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let graph = st.workspaces.lock().expect("workspace graph lock");
    match workspace_detail(&graph, workspace_id) {
        Ok(body) => Ok(Json(body)),
        Err(body) => Ok(body),
    }
}

pub async fn domain_workspace_activate(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let workspace_id = match parse_id(&workspace_id, "workspace") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    let mut next = graph.clone();
    match next.set_active(workspace_id) {
        Ok(()) => {
            persist_workspaces(&st, &next)?;
            *graph = next;
            Ok(Json(json!({ "ok": true, "workspace_id": workspace_id })))
        }
        Err(e) => Ok(bad_request(e.to_string())),
    }
}

#[derive(Deserialize)]
pub struct WorkspaceSplitRequest {
    pub pane_id: String,
    pub direction: String,
}

fn split_direction(
    raw: &str,
) -> Result<ridge_core::workspace::pane_tree::SplitDirection, Json<Value>> {
    use ridge_core::workspace::pane_tree::SplitDirection;
    match raw {
        "horizontal" => Ok(SplitDirection::Horizontal),
        "vertical" => Ok(SplitDirection::Vertical),
        _ => Err(bad_request("direction must be horizontal or vertical")),
    }
}

pub async fn domain_workspace_split(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(request): Json<WorkspaceSplitRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let workspace_id = match parse_id(&workspace_id, "workspace") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let pane_id = match parse_id(&request.pane_id, "pane") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let direction = match split_direction(&request.direction) {
        Ok(value) => value,
        Err(body) => return Ok(body),
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    let mut next = graph.clone();
    match next.split(workspace_id, pane_id, direction) {
        Ok(new_pane_id) => {
            persist_workspaces(&st, &next)?;
            *graph = next;
            Ok(Json(json!({ "ok": true, "pane_id": new_pane_id })))
        }
        Err(e) => Ok(bad_request(e.to_string())),
    }
}

fn close_pane_resources(
    st: &AppState,
    pane_id: Uuid,
    previous: &ridge_core::workspace::graph::WorkspaceGraph,
    next: &ridge_core::workspace::graph::WorkspaceGraph,
) -> Result<Option<String>, StatusCode> {
    let pty = if st.ptys.contains(pane_id) {
        Some(
            st.ptys
                .begin_destroy(pane_id)
                .map_err(|_| StatusCode::CONFLICT)?,
        )
    } else {
        None
    };
    if pty.is_some() {
        remove_output_leases_for_pty(st, pane_id);
    }
    if let Err(error) = persist_workspaces(st, next) {
        if pty.is_some() {
            st.ptys.cancel_destroy(pane_id);
        }
        return Err(error);
    }
    let Some(bridge) = pty else {
        return Ok(None);
    };
    if bridge.destroy().is_err() {
        st.ptys.cancel_destroy(pane_id);
        let _ = persist_workspaces(st, previous);
        return Ok(Some(
            "pane PTY did not terminate; close rolled back".to_string(),
        ));
    }
    st.ptys
        .finish_destroy(pane_id)
        .map_err(|_| StatusCode::CONFLICT)?;
    match st.mcp_state.purge_pane(&pane_id.to_string()) {
        Ok(()) => Ok(None),
        Err(error) => Ok(Some(format!(
            "pane closed but Hub delivery state could not be purged: {error}"
        ))),
    }
}

pub async fn domain_workspace_pane_close(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, pane_id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let workspace_id = match parse_id(&workspace_id, "workspace") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let pane_id = match parse_id(&pane_id, "pane") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    let previous = graph.clone();
    let mut next = graph.clone();
    if let Err(error) = next.close(workspace_id, pane_id) {
        return Ok(bad_request(error.to_string()));
    }
    if let Some(message) = close_pane_resources(&st, pane_id, &previous, &next)? {
        return Ok(bad_request(message));
    }
    *graph = next;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct LockedSizeRequest {
    pub cols: u16,
    pub rows: u16,
}

pub async fn domain_workspace_pane_locked_size(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, pane_id)): Path<(String, String)>,
    Json(request): Json<LockedSizeRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let workspace_id = match parse_id(&workspace_id, "workspace") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let pane_id = match parse_id(&pane_id, "pane") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    let mut next = graph.clone();
    match next.set_locked_size(workspace_id, pane_id, request.cols, request.rows) {
        Ok(()) => {
            persist_workspaces(&st, &next)?;
            *graph = next;
            Ok(Json(
                json!({ "ok": true, "cols": request.cols, "rows": request.rows }),
            ))
        }
        Err(e) => Ok(bad_request(e.to_string())),
    }
}

pub async fn domain_agents(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({
        "ok": true,
        "source": "ridge-kernel",
        "profiles": builtin_agent_profiles(),
    })))
}

fn roster_snapshot(graph: &ridge_core::teammate::topology::TopologyGraph) -> Value {
    let mut roster = graph
        .roster()
        .into_iter()
        .cloned()
        .collect::<Vec<ridge_core::teammate::model::Teammate>>();
    roster.sort_by(|left, right| left.id.cmp(&right.id));
    json!({
        "ok": true,
        "source": "ridge-kernel",
        "leader_id": graph.leader_id(),
        "roster": roster,
        "agent_identities": graph.agent_identities(),
    })
}

pub async fn domain_agent_roster(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(roster_snapshot(
        &st.roster.lock().expect("roster lock"),
    )))
}

#[derive(Deserialize)]
pub struct AgentRosterAddRequest {
    pub id: String,
    pub name: String,
    pub pane_id: u32,
}

#[derive(Deserialize)]
pub struct AgentIdentityCommitRequest {
    pub agent_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub cwd: String,
    pub executable: String,
    #[serde(default)]
    pub argv: Vec<String>,
    pub generation: u64,
    pub lease: String,
    pub lifecycle: ridge_core::teammate::communication::AgentLifecycle,
    pub online: bool,
    pub last_seen_unix_ms: u64,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

/// Commit one identity only after the caller has completed spawn/attach and
/// proved the Agent is online. Teammate metadata remains a compatibility API.
pub async fn domain_agent_identity_commit(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AgentIdentityCommitRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let identity = AgentIdentity {
        agent_id: request.agent_id,
        session_id: request.session_id,
        workspace_id: request.workspace_id,
        pane_id: request.pane_id,
        cwd: request.cwd,
        executable: request.executable,
        argv: request.argv,
        generation: request.generation,
        lease: request.lease,
        lifecycle: request.lifecycle,
        online: request.online,
        last_seen_unix_ms: request.last_seen_unix_ms,
        capabilities: request.capabilities,
    };
    let agent_id = identity.agent_id.clone();
    let mut roster = st.roster.lock().expect("roster lock");
    let mut next = roster.clone();
    match next.commit_online_agent(identity) {
        Ok(()) => {
            persist_roster(&st, &next)?;
            *roster = next;
            Ok(Json(json!({ "ok": true, "agent_id": agent_id })))
        }
        Err(error @ CommunicationError::InvalidEnvelope(_))
        | Err(error @ CommunicationError::TargetOffline(_))
        | Err(error @ CommunicationError::GenerationMismatch { .. })
        | Err(error @ CommunicationError::StaleLease) => Ok(Json(json!({
            "ok": false,
            "error": error,
        }))),
        Err(error) => Ok(Json(json!({ "ok": false, "error": error }))),
    }
}

pub async fn domain_agent_roster_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AgentRosterAddRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if request.id.trim().is_empty() || request.name.trim().is_empty() {
        return Ok(bad_request("agent id and name are required"));
    }
    let capability = ridge_core::teammate::model::recognize_capability(&request.name, None);
    let teammate =
        ridge_core::teammate::model::Teammate::new(request.id, request.name, request.pane_id)
            .with_capability(capability);
    let agent_id = teammate.id.clone();
    let mut roster = st.roster.lock().expect("roster lock");
    let mut next = roster.clone();
    next.add_teammate(teammate);
    persist_roster(&st, &next)?;
    *roster = next;
    Ok(Json(json!({ "ok": true, "agent_id": agent_id })))
}

pub async fn domain_agent_roster_remove(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if agent_id.trim().is_empty() {
        return Ok(bad_request("agent id is required"));
    }
    let mut roster = st.roster.lock().expect("roster lock");
    let mut next = roster.clone();
    next.remove_teammate(&agent_id);
    persist_roster(&st, &next)?;
    *roster = next;
    Ok(Json(json!({ "ok": true, "agent_id": agent_id })))
}

#[derive(Deserialize)]
pub struct PtyCreateRequest {
    /// Stable pane identity supplied by a shell. When omitted, the kernel
    /// generates a UUID for backwards-compatible callers.
    pub pty_id: Option<Uuid>,
    /// Optional explicit executable. `shell` remains the compatibility field
    /// for ordinary interactive PTYs.
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub workspace_id: Option<Uuid>,
    pub role: Option<String>,
    pub launch_profile: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Deserialize)]
pub struct WorkspaceTopologySyncRequest {
    pub pane_tree: PaneTree,
}

/// Synchronize a desktop-owned pane tree into the kernel projection. Missing
/// workspace ids are inserted so a fresh desktop workspace can become the
/// detached Remote source of truth before its first PTY attaches.
pub async fn domain_workspace_topology(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<String>,
    Json(request): Json<WorkspaceTopologySyncRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let workspace_id = match parse_id(&workspace_id, "workspace") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    let mut next = graph.clone();
    if let Some(meta) = next.workspace_mut(workspace_id) {
        meta.pane_tree = request.pane_tree;
    } else {
        next.insert_workspace(
            workspace_id,
            WorkspaceMeta {
                pane_tree: request.pane_tree,
                name: None,
                locked_sizes: HashMap::new(),
            },
        );
    }
    persist_workspaces(&st, &next)?;
    *graph = next;
    Ok(Json(json!({
        "ok": true,
        "source": "ridge-kernel",
        "workspace_id": workspace_id,
    })))
}

fn validate_pty_create(request: &PtyCreateRequest) -> Option<&'static str> {
    if request.args.len() > 128 {
        return Some("too many PTY launch arguments");
    }
    if request.env.len() > 256 {
        return Some("too many PTY launch environment entries");
    }
    let env_bytes = request
        .env
        .iter()
        .map(|(key, value)| key.len().saturating_add(value.len()))
        .sum::<usize>();
    (env_bytes > 64 * 1024).then_some("PTY launch environment is too large")
}

fn seed_workspace_for_pty(
    st: &AppState,
    request: &PtyCreateRequest,
    pty_id: Uuid,
) -> Result<Option<Uuid>, StatusCode> {
    let Some(workspace_id) = request.workspace_id else {
        return Ok(None);
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    if graph.workspace(workspace_id).is_some() {
        return Ok(None);
    }
    let mut panes = HashMap::new();
    panes.insert(
        pty_id,
        ridge_core::workspace::pane_tree::Pane {
            id: pty_id,
            mode: ridge_core::workspace::mode::PaneMode::Terminal,
            cwd: request.cwd.clone().map(std::path::PathBuf::from),
            shell_kind: request.shell.clone().or(request.program.clone()),
        },
    );
    graph.insert_workspace(
        workspace_id,
        ridge_core::workspace::graph::WorkspaceMeta {
            pane_tree: ridge_core::workspace::pane_tree::PaneTree {
                root: ridge_core::workspace::pane_tree::PaneNode::Leaf(pty_id),
                panes,
            },
            name: None,
            locked_sizes: HashMap::new(),
        },
    );
    persist_workspaces(st, &graph)?;
    Ok(Some(workspace_id))
}

fn rollback_seeded_workspace(st: &AppState, workspace_id: Option<Uuid>) {
    let Some(workspace_id) = workspace_id else {
        return;
    };
    let mut graph = st.workspaces.lock().expect("workspace graph lock");
    graph.remove_workspace(workspace_id);
    let _ = persist_workspaces(st, &graph);
}

pub async fn domain_pty_create(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PtyCreateRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = request.pty_id.unwrap_or_else(Uuid::new_v4);
    let role = request.role.as_deref().unwrap_or("shell");
    let program = request.program.as_deref().or(request.shell.as_deref());
    if let Some(error) = validate_pty_create(&request) {
        return Ok(bad_request(error));
    }
    // A desktop shell may create the first PTY before it has persisted the
    // corresponding graph entry.  Seed that stable workspace identity here so
    // a detached LAN/WebRTC host can expose a real layout after a hard kill.
    let inserted_workspace = seed_workspace_for_pty(&st, &request, pty_id)?;
    match st.ptys.spawn_command_for_with_env(PtyLaunch {
        id: pty_id,
        program,
        args: &request.args,
        cwd: request.cwd.as_deref(),
        workspace_id: request.workspace_id,
        role,
        launch_profile: request.launch_profile.as_deref(),
        env: Some(&request.env),
    }) {
        Ok(pty_id) => {
            if let (Some(cols), Some(rows)) = (request.cols, request.rows) {
                if let Err(error) = st.ptys.resize(pty_id, cols, rows) {
                    let _ = st.ptys.destroy(pty_id);
                    rollback_seeded_workspace(&st, inserted_workspace);
                    return Ok(bad_request(error.to_string()));
                }
            }
            let executable = request
                .program
                .as_deref()
                .or(request.shell.as_deref())
                .unwrap_or("shell");
            if let Err(error) =
                commit_agent_identity_for_pty(&st, pty_id, role, executable, request.args.clone())
            {
                let _ = st.ptys.destroy(pty_id);
                rollback_seeded_workspace(&st, inserted_workspace);
                return Ok(bad_request(error));
            }
            Ok(Json(
                json!({ "ok": true, "source": "ridge-kernel", "pty_id": pty_id }),
            ))
        }
        Err(error) => {
            rollback_seeded_workspace(&st, inserted_workspace);
            Ok(bad_request(error.to_string()))
        }
    }
}

/// Discover PTYs that outlived a desktop shell. The kernel is the lifecycle
/// owner, so this endpoint intentionally exposes stable pane/workspace
/// identity and bounded output sequence metadata to an authenticated local
/// shell during restart reattachment.
pub async fn domain_pty_list(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let ptys = st
        .ptys
        .list()
        .into_iter()
        .map(|info| {
            let (oldest_seq, next_seq) = st.ptys.output_bounds(info.id).unwrap_or((0, 0));
            json!({
                "id": info.id,
                "pty_id": info.id,
                "pane_index": info.pane_index,
                "workspace_id": info.workspace_id,
                "role": info.role,
                "program": info.program,
                "launch_profile": info.launch_profile,
                "cwd": info.cwd,
                "status": info.status,
                "child_pid": info.child_pid,
                "cols": info.cols,
                "rows": info.rows,
                "oldest_seq": oldest_seq,
                "next_seq": next_seq,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "ok": true,
        "source": "ridge-kernel",
        "ptys": ptys,
    })))
}

#[derive(Deserialize)]
pub struct PtyWriteRequest {
    pub data_b64: String,
}

pub async fn domain_pty_write(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(request): Json<PtyWriteRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let data = match base64::engine::general_purpose::STANDARD.decode(request.data_b64) {
        Ok(data) => data,
        Err(_) => return Ok(bad_request("data_b64 must be valid base64")),
    };
    match st.ptys.write(pty_id, &data) {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

#[derive(Deserialize)]
pub struct PtyResizeRequest {
    pub cols: u16,
    pub rows: u16,
}

pub async fn domain_pty_resize(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(request): Json<PtyResizeRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    match st.ptys.resize(pty_id, request.cols, request.rows) {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

pub async fn domain_pty_destroy(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let result = st.ptys.destroy(pty_id);
    // `destroy` closes the hub and wakes pending polls. Remove idle HTTP
    // handles as well; their Drop detaches from the bounded replay state.
    remove_output_leases_for_pty(&st, pty_id);
    match result {
        Ok(()) => {
            if let Err(error) = remove_agent_identity_for_pty(&st, pty_id) {
                return Ok(bad_request(format!(
                    "PTY destroyed but Agent identity teardown could not be persisted: {error}"
                )));
            }
            if let Err(error) = st.mcp_state.purge_pane(&pty_id.to_string()) {
                return Ok(bad_request(format!(
                    "PTY destroyed but Hub delivery state could not be purged: {error}"
                )));
            }
            Ok(Json(json!({ "ok": true })))
        }
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

pub async fn domain_pty_clear(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    st.ptys
        .clear_scrollback(pty_id)
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(
        json!({ "ok": true, "pty_id": pty_id, "scrollback_cleared": true }),
    ))
}

#[derive(Deserialize)]
pub struct PtyScrollbackQuery {
    pub max_bytes: Option<usize>,
}

pub async fn domain_pty_scrollback(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Query(query): Query<PtyScrollbackQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    match st
        .ptys
        .scrollback(pty_id, query.max_bytes.unwrap_or(64 * 1024))
    {
        Ok(bytes) => Ok(Json(json!({
            "ok": true,
            "pty_id": pty_id,
            "data_b64": base64::engine::general_purpose::STANDARD.encode(bytes),
        }))),
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct PtyOutputAttachQuery {
    /// Resume after this sequence number. `None` starts at the oldest retained
    /// frame in the bounded replay window.
    pub after_seq: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct PtyOutputPollQuery {
    /// Long-poll wait bound. The kernel rejects values above the hard cap so a
    /// stalled client cannot hold a lease forever.
    pub timeout_ms: Option<u64>,
    /// Maximum frames returned in one response.
    pub max_frames: Option<usize>,
}

fn remove_output_leases_for_pty(st: &AppState, pty_id: Uuid) {
    let mut leases = st.output_leases.lock().expect("output lease lock");
    leases.retain(|_, slot| slot.pty_id != pty_id);
}

fn output_lease_slot(
    st: &AppState,
    pty_id: Uuid,
    lease_id: Uuid,
) -> Result<std::sync::Arc<OutputLeaseSlot>, Json<Value>> {
    let leases = st.output_leases.lock().expect("output lease lock");
    let slot = leases
        .get(&lease_id)
        .cloned()
        .ok_or_else(|| bad_request("PTY output lease not found"))?;
    if slot.pty_id != pty_id {
        return Err(bad_request("PTY output lease belongs to another PTY"));
    }
    Ok(slot)
}

fn remove_output_lease(st: &AppState, pty_id: Uuid, lease_id: Uuid) -> bool {
    let mut leases = st.output_leases.lock().expect("output lease lock");
    if leases
        .get(&lease_id)
        .is_some_and(|slot| slot.pty_id == pty_id)
    {
        leases.remove(&lease_id);
        true
    } else {
        false
    }
}

fn output_read_json(pty_id: Uuid, lease_id: Uuid, read: PtyOutputRead) -> Value {
    match read {
        PtyOutputRead::Data(frames) => json!({
            "ok": true,
            "kind": "data",
            "pty_id": pty_id,
            "lease_id": lease_id,
            "frames": frames.into_iter().map(|frame| json!({
                "seq": frame.seq,
                "data_b64": base64::engine::general_purpose::STANDARD.encode(frame.data),
            })).collect::<Vec<_>>(),
        }),
        PtyOutputRead::Lagged {
            requested_seq,
            oldest_seq,
            latest_seq,
        } => json!({
            "ok": true,
            "kind": "lagged",
            "pty_id": pty_id,
            "lease_id": lease_id,
            "requested_seq": requested_seq,
            "oldest_seq": oldest_seq,
            "latest_seq": latest_seq,
            "resync_required": true,
        }),
    }
}

/// Create a bounded, replayable PTY output lease for HTTP clients.
pub async fn domain_pty_output_attach(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Query(query): Query<PtyOutputAttachQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    {
        let leases = st.output_leases.lock().expect("output lease lock");
        if leases.len() >= MAX_OUTPUT_HTTP_LEASES {
            return Ok(bad_request("PTY output lease capacity exhausted"));
        }
    }
    match st.ptys.attach_output(pty_id, query.after_seq) {
        Ok(lease) => {
            let lease_id = lease.id();
            let slot = std::sync::Arc::new(OutputLeaseSlot {
                pty_id,
                lease,
                poll_lock: tokio::sync::Mutex::new(()),
            });
            let mut leases = st.output_leases.lock().expect("output lease lock");
            if leases.len() >= MAX_OUTPUT_HTTP_LEASES {
                drop(leases);
                // The preflight can race another attach. Drop detaches the
                // bounded PTY lease and releases its cursor before returning.
                return Ok(bad_request("PTY output lease capacity exhausted"));
            }
            leases.insert(lease_id, slot);
            Ok(Json(json!({
                "ok": true,
                "source": "ridge-kernel",
                "pty_id": pty_id,
                "lease_id": lease_id,
                "protocol": "bounded-seq-v1",
            })))
        }
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

/// Long-poll one output lease. A timeout is a normal empty event, not a
/// transport error; lifecycle closure removes the lease and returns a stable
/// protocol error so clients stop retrying a destroyed pane.
pub async fn domain_pty_output_poll(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((pty_id, lease_id)): Path<(String, String)>,
    Query(query): Query<PtyOutputPollQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let lease_id = match parse_id(&lease_id, "lease") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let timeout_ms = query.timeout_ms.unwrap_or(DEFAULT_OUTPUT_POLL_TIMEOUT_MS);
    if timeout_ms > MAX_OUTPUT_POLL_TIMEOUT_MS {
        return Ok(bad_request(format!(
            "timeout_ms must be <= {MAX_OUTPUT_POLL_TIMEOUT_MS}"
        )));
    }
    let max_frames = query.max_frames.unwrap_or(DEFAULT_OUTPUT_POLL_FRAMES);
    if max_frames == 0 || max_frames > MAX_OUTPUT_POLL_FRAMES {
        return Ok(bad_request(format!(
            "max_frames must be between 1 and {MAX_OUTPUT_POLL_FRAMES}"
        )));
    }
    let slot = match output_lease_slot(&st, pty_id, lease_id) {
        Ok(slot) => slot,
        Err(body) => return Ok(body),
    };
    let _poll_guard = slot.poll_lock.lock().await;
    match slot
        .lease
        .next(Duration::from_millis(timeout_ms), max_frames)
        .await
    {
        Ok(read) => Ok(Json(output_read_json(pty_id, lease_id, read))),
        Err(PtyOutputLeaseError::TimedOut) => Ok(Json(json!({
            "ok": true,
            "kind": "timeout",
            "pty_id": pty_id,
            "lease_id": lease_id,
            "frames": [],
        }))),
        Err(
            error @ (PtyOutputLeaseError::Detached
            | PtyOutputLeaseError::Closing
            | PtyOutputLeaseError::Closed),
        ) => {
            remove_output_lease(&st, pty_id, lease_id);
            Ok(bad_request(error.to_string()))
        }
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

/// Explicitly move a lagged lease back to the oldest retained sequence.
pub async fn domain_pty_output_resync(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((pty_id, lease_id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let lease_id = match parse_id(&lease_id, "lease") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let slot = match output_lease_slot(&st, pty_id, lease_id) {
        Ok(slot) => slot,
        Err(body) => return Ok(body),
    };
    let _poll_guard = slot.poll_lock.lock().await;
    match slot.lease.resync() {
        Ok(cursor_seq) => Ok(Json(json!({
            "ok": true,
            "kind": "resynced",
            "pty_id": pty_id,
            "lease_id": lease_id,
            "cursor_seq": cursor_seq,
        }))),
        Err(error) => {
            remove_output_lease(&st, pty_id, lease_id);
            Ok(bad_request(error.to_string()))
        }
    }
}

/// Detach an HTTP output lease. The operation is idempotent from the client's
/// perspective: a missing lease is reported as a normal protocol error rather
/// than leaving a retrying client attached forever.
pub async fn domain_pty_output_detach(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path((pty_id, lease_id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let pty_id = match parse_id(&pty_id, "pty") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let lease_id = match parse_id(&lease_id, "lease") {
        Ok(id) => id,
        Err(body) => return Ok(body),
    };
    let slot = {
        let mut leases = st.output_leases.lock().expect("output lease lock");
        match leases.get(&lease_id) {
            Some(slot) if slot.pty_id == pty_id => leases.remove(&lease_id),
            Some(_) => return Ok(bad_request("PTY output lease belongs to another PTY")),
            None => return Ok(bad_request("PTY output lease not found")),
        }
    };
    if let Some(slot) = slot {
        let _ = slot.lease.detach();
    }
    Ok(Json(json!({
        "ok": true,
        "kind": "detached",
        "pty_id": pty_id,
        "lease_id": lease_id,
    })))
}

#[derive(Deserialize)]
pub struct FsListQuery {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

pub async fn domain_fs_list(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<FsListQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !st.fs_scope.is_allowed(&q.path) {
        return Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "path": q.path,
            "error": "path outside kernel filesystem root",
        })));
    }
    match ridge_core::fs::commands::get_directory_children(&q.path, q.offset, q.limit) {
        Ok(page) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "path": q.path,
            "page": page,
        }))),
        Err(e) => Ok(Json(json!({
            "ok": false,
            "error": e.to_string(),
        }))),
    }
}

#[derive(Deserialize)]
pub struct GitQuery {
    pub path: String,
    #[serde(default)]
    pub fast: bool,
}

/// Explicit Git write operations owned by the kernel. The tagged request keeps
/// the mutation surface finite and typed; arbitrary Git argv never crosses the
/// domain boundary.
#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum GitMutationRequest {
    Stage {
        repo_root: String,
        paths: Vec<String>,
    },
    Unstage {
        repo_root: String,
        paths: Vec<String>,
    },
    Discard {
        repo_root: String,
        paths: Vec<String>,
    },
    CleanUntracked {
        repo_root: String,
        paths: Vec<String>,
    },
    Commit {
        repo_root: String,
        message: String,
        amend: Option<bool>,
    },
    Checkout {
        repo_root: String,
        branch: String,
        create: Option<bool>,
        base: Option<String>,
    },
    Push {
        repo_root: String,
        set_upstream: Option<bool>,
    },
    PushBranch {
        repo_root: String,
        branch: String,
    },
    MergeBranch {
        repo_root: String,
        branch: String,
    },
    DeleteBranch {
        repo_root: String,
        branch: String,
        force: Option<bool>,
    },
    RenameBranch {
        repo_root: String,
        old_name: String,
        new_name: String,
    },
    Rebase {
        repo_root: String,
        onto: String,
    },
    DeleteTag {
        repo_root: String,
        name: String,
    },
    PushTag {
        repo_root: String,
        name: String,
    },
    StashPush {
        repo_root: String,
        message: Option<String>,
        include_untracked: Option<bool>,
    },
    StashApply {
        repo_root: String,
        reference: String,
    },
    StashPop {
        repo_root: String,
        reference: String,
    },
    StashDrop {
        repo_root: String,
        reference: String,
    },
    StashBranch {
        repo_root: String,
        branch: String,
        reference: String,
    },
    Fetch {
        repo_root: String,
    },
    Pull {
        repo_root: String,
    },
    Sync {
        repo_root: String,
    },
    CherryPickAbort {
        repo_root: String,
    },
    RevertAbort {
        repo_root: String,
    },
    CherryPick {
        repo_root: String,
        hash: String,
    },
    Revert {
        repo_root: String,
        hash: String,
    },
    CreateTag {
        repo_root: String,
        name: String,
        hash: Option<String>,
        message: Option<String>,
    },
    Reset {
        repo_root: String,
        hash: String,
        mode: String,
    },
}

impl GitMutationRequest {
    fn repo_root(&self) -> &str {
        match self {
            Self::Stage { repo_root, .. }
            | Self::Unstage { repo_root, .. }
            | Self::Discard { repo_root, .. }
            | Self::CleanUntracked { repo_root, .. }
            | Self::Commit { repo_root, .. }
            | Self::Checkout { repo_root, .. }
            | Self::Push { repo_root, .. }
            | Self::PushBranch { repo_root, .. }
            | Self::MergeBranch { repo_root, .. }
            | Self::DeleteBranch { repo_root, .. }
            | Self::RenameBranch { repo_root, .. }
            | Self::Rebase { repo_root, .. }
            | Self::DeleteTag { repo_root, .. }
            | Self::PushTag { repo_root, .. }
            | Self::StashPush { repo_root, .. }
            | Self::StashApply { repo_root, .. }
            | Self::StashPop { repo_root, .. }
            | Self::StashDrop { repo_root, .. }
            | Self::StashBranch { repo_root, .. }
            | Self::Fetch { repo_root }
            | Self::Pull { repo_root }
            | Self::Sync { repo_root }
            | Self::CherryPickAbort { repo_root }
            | Self::RevertAbort { repo_root }
            | Self::CherryPick { repo_root, .. }
            | Self::Revert { repo_root, .. }
            | Self::CreateTag { repo_root, .. }
            | Self::Reset { repo_root, .. } => repo_root,
        }
    }
}

/// Typed Git reads owned by the kernel. Keeping graph/history arguments in a
/// tagged request prevents the Tauri shell from reintroducing direct Git
/// subprocess calls as new UI features are added.
#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "kebab-case")]
pub enum GitReadRequest {
    Info {
        repo_root: String,
    },
    Commits {
        repo_root: String,
        offset: u32,
        limit: u32,
    },
    FileVersions {
        repo_root: String,
        path: String,
        cached: Option<bool>,
    },
    CommitFiles {
        repo_root: String,
        hash: String,
    },
    FileVersionsAtCommit {
        repo_root: String,
        path: String,
        hash: String,
    },
    FileVersionsBetween {
        repo_root: String,
        path: String,
        from: String,
        to: String,
    },
    CompareCommits {
        repo_root: String,
        from: String,
        to: String,
    },
    DiffFile {
        repo_root: String,
        path: String,
        cached: Option<bool>,
    },
    Blame {
        repo_root: String,
        path: String,
    },
    FileLog {
        repo_root: String,
        path: String,
        limit: Option<u32>,
    },
}

impl GitReadRequest {
    fn repo_root(&self) -> &str {
        match self {
            Self::Info { repo_root }
            | Self::Commits { repo_root, .. }
            | Self::FileVersions { repo_root, .. }
            | Self::CommitFiles { repo_root, .. }
            | Self::FileVersionsAtCommit { repo_root, .. }
            | Self::FileVersionsBetween { repo_root, .. }
            | Self::CompareCommits { repo_root, .. }
            | Self::DiffFile { repo_root, .. }
            | Self::Blame { repo_root, .. }
            | Self::FileLog { repo_root, .. } => repo_root,
        }
    }
}

pub async fn domain_git_mutate(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GitMutationRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let repo_root = request.repo_root().to_string();
    if ridge_core::commands::git::find_git_repo_root(repo_root.clone()).is_none() {
        return Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": false,
            "repo_root": repo_root,
            "error": format!("Not a git repo: {repo_root}"),
        })));
    }

    let result: Result<Option<String>, String> = match request {
        GitMutationRequest::Stage { repo_root, paths } => {
            ridge_core::commands::git::git_stage(repo_root, paths)
                .await
                .map(|_| None)
        }
        GitMutationRequest::Unstage { repo_root, paths } => {
            ridge_core::commands::git::git_unstage(repo_root, paths)
                .await
                .map(|_| None)
        }
        GitMutationRequest::Discard { repo_root, paths } => {
            ridge_core::commands::git::git_discard(repo_root, paths)
                .await
                .map(|_| None)
        }
        GitMutationRequest::CleanUntracked { repo_root, paths } => {
            ridge_core::commands::git::git_clean_untracked(repo_root, paths)
                .await
                .map(|_| None)
        }
        GitMutationRequest::Commit {
            repo_root,
            message,
            amend,
        } => ridge_core::commands::git::git_commit(repo_root, message, amend)
            .await
            .map(|_| None),
        GitMutationRequest::Checkout {
            repo_root,
            branch,
            create,
            base,
        } => ridge_core::commands::git::git_checkout(repo_root, branch, create, base)
            .await
            .map(|_| None),
        GitMutationRequest::Push {
            repo_root,
            set_upstream,
        } => ridge_core::commands::git::git_push(repo_root, set_upstream)
            .await
            .map(|_| None),
        GitMutationRequest::PushBranch { repo_root, branch } => {
            ridge_core::commands::git::git_push_branch(repo_root, branch)
                .await
                .map(Some)
        }
        GitMutationRequest::MergeBranch { repo_root, branch } => {
            ridge_core::commands::git::git_merge_branch(repo_root, branch)
                .await
                .map(Some)
        }
        GitMutationRequest::DeleteBranch {
            repo_root,
            branch,
            force,
        } => ridge_core::commands::git::git_delete_branch(repo_root, branch, force)
            .await
            .map(Some),
        GitMutationRequest::RenameBranch {
            repo_root,
            old_name,
            new_name,
        } => ridge_core::commands::git::git_rename_branch(repo_root, old_name, new_name)
            .await
            .map(Some),
        GitMutationRequest::Rebase { repo_root, onto } => {
            ridge_core::commands::git::git_rebase(repo_root, onto)
                .await
                .map(Some)
        }
        GitMutationRequest::DeleteTag { repo_root, name } => {
            ridge_core::commands::git::git_delete_tag(repo_root, name)
                .await
                .map(Some)
        }
        GitMutationRequest::PushTag { repo_root, name } => {
            ridge_core::commands::git::git_push_tag(repo_root, name)
                .await
                .map(Some)
        }
        GitMutationRequest::StashPush {
            repo_root,
            message,
            include_untracked,
        } => ridge_core::commands::git::git_stash_push(repo_root, message, include_untracked)
            .await
            .map(Some),
        GitMutationRequest::StashApply {
            repo_root,
            reference,
        } => ridge_core::commands::git::git_stash_apply(repo_root, reference)
            .await
            .map(Some),
        GitMutationRequest::StashPop {
            repo_root,
            reference,
        } => ridge_core::commands::git::git_stash_pop(repo_root, reference)
            .await
            .map(Some),
        GitMutationRequest::StashDrop {
            repo_root,
            reference,
        } => ridge_core::commands::git::git_stash_drop(repo_root, reference)
            .await
            .map(Some),
        GitMutationRequest::StashBranch {
            repo_root,
            branch,
            reference,
        } => ridge_core::commands::git::git_stash_branch(repo_root, branch, reference)
            .await
            .map(Some),
        GitMutationRequest::Fetch { repo_root } => ridge_core::commands::git::git_fetch(repo_root)
            .await
            .map(|_| None),
        GitMutationRequest::Pull { repo_root } => ridge_core::commands::git::git_pull(repo_root)
            .await
            .map(|_| None),
        GitMutationRequest::Sync { repo_root } => ridge_core::commands::git::git_sync(repo_root)
            .await
            .map(|_| None),
        GitMutationRequest::CherryPickAbort { repo_root } => {
            ridge_core::commands::git::git_cherry_pick_abort(repo_root)
                .await
                .map(|_| None)
        }
        GitMutationRequest::RevertAbort { repo_root } => {
            ridge_core::commands::git::git_revert_abort(repo_root)
                .await
                .map(|_| None)
        }
        GitMutationRequest::CherryPick { repo_root, hash } => {
            ridge_core::commands::git::git_cherry_pick(repo_root, hash)
                .await
                .map(|_| None)
        }
        GitMutationRequest::Revert { repo_root, hash } => {
            ridge_core::commands::git::git_revert(repo_root, hash)
                .await
                .map(|_| None)
        }
        GitMutationRequest::CreateTag {
            repo_root,
            name,
            hash,
            message,
        } => ridge_core::commands::git::git_create_tag(repo_root, name, hash, message)
            .await
            .map(|_| None),
        GitMutationRequest::Reset {
            repo_root,
            hash,
            mode,
        } => ridge_core::commands::git::git_reset(repo_root, hash, mode)
            .await
            .map(|_| None),
    };

    match result {
        Ok(output) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "repo_root": repo_root,
            "output": output,
        }))),
        Err(error) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "repo_root": repo_root,
            "error": error,
        }))),
    }
}

pub async fn domain_git_read(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GitReadRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let repo_root = request.repo_root().to_string();
    let detect_path = repo_root.clone();
    let is_repo = tokio::task::spawn_blocking(move || {
        ridge_core::commands::git::find_git_repo_root(detect_path).is_some()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "repo_root": repo_root,
            "value": null,
        })));
    }

    let result: Result<Value, String> = match request {
        GitReadRequest::Info { repo_root } => {
            ridge_core::commands::git::get_git_info_with_cwd(repo_root)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        GitReadRequest::Commits {
            repo_root,
            offset,
            limit,
        } => ridge_core::commands::git::get_git_commits_paginated(repo_root, offset, limit)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        GitReadRequest::FileVersions {
            repo_root,
            path,
            cached,
        } => ridge_core::commands::git::git_get_file_versions(repo_root, path, cached)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        GitReadRequest::CommitFiles { repo_root, hash } => {
            ridge_core::commands::git::git_get_commit_files(repo_root, hash)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        GitReadRequest::FileVersionsAtCommit {
            repo_root,
            path,
            hash,
        } => ridge_core::commands::git::git_get_file_versions_at_commit(repo_root, path, hash)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        GitReadRequest::FileVersionsBetween {
            repo_root,
            path,
            from,
            to,
        } => ridge_core::commands::git::git_get_file_versions_between(repo_root, path, from, to)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        GitReadRequest::CompareCommits {
            repo_root,
            from,
            to,
        } => ridge_core::commands::git::git_compare_commits(repo_root, from, to)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        GitReadRequest::DiffFile {
            repo_root,
            path,
            cached,
        } => ridge_core::commands::git::git_diff_file(repo_root, path, cached)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
        GitReadRequest::Blame { repo_root, path } => {
            ridge_core::commands::git::git_blame(repo_root, path)
                .await
                .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        }
        GitReadRequest::FileLog {
            repo_root,
            path,
            limit,
        } => ridge_core::commands::git::git_file_log(repo_root, path, limit)
            .await
            .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string())),
    };

    match result {
        Ok(value) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "repo_root": repo_root,
            "value": value,
        }))),
        Err(error) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "repo_root": repo_root,
            "error": error,
        }))),
    }
}

pub async fn domain_git_status(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let GitQuery { path, fast } = q;
    // Git discovery/status spawn external processes and may wait on the shared
    // guard. Keep that work off the async executor so a slow repository cannot
    // stall health, MCP, or unrelated domain requests.
    let status_path = path.clone();
    let (is_repo, status) = tokio::task::spawn_blocking(move || {
        let is_repo = ridge_core::commands::git::find_git_repo_root(status_path.clone()).is_some();
        let status = is_repo.then(|| {
            if fast {
                ridge_core::commands::git::get_scm_status_fast_sync(status_path)
            } else {
                ridge_core::commands::git::get_scm_status_sync(status_path)
            }
        });
        (is_repo, status)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "path": path,
        })));
    }
    let status = status.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    match status {
        Ok(status) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "status": status,
        }))),
        Err(e) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "error": e,
        }))),
    }
}

/// Read the kernel-owned local/remote branch projection. Repository discovery
/// happens before `git branch` so confirmed non-Git roots never emit repeated
/// subprocess errors from the SCM sidebar.
pub async fn domain_git_branches(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let path = q.path;
    let branch_path = path.clone();
    let (is_repo, branches) = tokio::task::spawn_blocking(move || {
        let is_repo = ridge_core::commands::git::find_git_repo_root(branch_path.clone()).is_some();
        let branches =
            is_repo.then(|| ridge_core::commands::git::git_list_branches_sync(branch_path));
        (is_repo, branches)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "path": path,
            "branches": [],
        })));
    }
    match branches.ok_or(StatusCode::INTERNAL_SERVER_ERROR)? {
        Ok(branches) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "branches": branches,
        }))),
        Err(error) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "error": error,
        }))),
    }
}

/// Read the kernel-owned stash projection. Repository detection happens before
/// `git stash list`, so confirmed non-Git roots never spawn a stash process.
pub async fn domain_git_stashes(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let path = q.path;
    let detect_path = path.clone();
    let is_repo = tokio::task::spawn_blocking(move || {
        ridge_core::commands::git::find_git_repo_root(detect_path).is_some()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "path": path,
            "stashes": [],
        })));
    }
    match ridge_core::commands::git::git_stash_list(path.clone()).await {
        Ok(stashes) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "stashes": stashes,
        }))),
        Err(error) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "error": error,
        }))),
    }
}

/// Read the aggregated diff counts used by desktop PaneGitStatus. Repository
/// discovery happens before `git diff`, so confirmed non-Git roots return a
/// healthy negative result and do not emit repeated Git errors.
pub async fn domain_git_diff_summary(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let path = q.path;
    let diff_path = path.clone();
    let (is_repo, summary) = tokio::task::spawn_blocking(move || {
        let is_repo = ridge_core::commands::git::find_git_repo_root(diff_path.clone()).is_some();
        let summary = is_repo.then(|| ridge_core::commands::git::git_diff_summary_sync(diff_path));
        (is_repo, summary)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "path": path,
            "summary": {"added": 0, "removed": 0},
        })));
    }
    match summary.ok_or(StatusCode::INTERNAL_SERVER_ERROR)? {
        Ok(summary) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "summary": summary,
        }))),
        Err(error) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": path,
            "error": error,
        }))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use tokio::sync::oneshot;

    fn test_state() -> AppState {
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        AppState {
            token: "test-token".to_string(),
            pid: 1,
            port: 0,
            started_at_unix: 0,
            shutdown_tx: Arc::new(std::sync::Mutex::new(Some(shutdown_tx))),
            shutting_down: Arc::new(AtomicBool::new(false)),
            workspaces: Arc::new(std::sync::Mutex::new(
                ridge_core::workspace::graph::WorkspaceGraph::new(),
            )),
            workspaces_path: std::env::temp_dir()
                .join(format!("ridge-kernel-workspaces-{}.json", Uuid::new_v4())),
            roster: Arc::new(std::sync::Mutex::new(
                ridge_core::teammate::topology::TopologyGraph::new(),
            )),
            roster_path: std::env::temp_dir()
                .join(format!("ridge-kernel-roster-{}.json", Uuid::new_v4())),
            groups: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            mcp_state: Arc::new(ridge_mcp::server::McpSessionState::default()),
            remote_hosts: Arc::new(std::sync::Mutex::new(
                ridge_core::remote::RemoteHostTopology::default(),
            )),
            remote_hosts_path: std::env::temp_dir()
                .join(format!("ridge-kernel-test-{}.json", Uuid::new_v4())),
            ptys: Arc::new(crate::pty::PtyRegistry::default()),
            output_leases: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            fs_scope: ridge_core::sandbox::RootScope::unrestricted(),
        }
    }

    fn test_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-ridge-kernel-token",
            HeaderValue::from_static("test-token"),
        );
        headers
    }

    #[tokio::test]
    async fn pty_create_preserves_stable_pane_identity_for_reconnect() {
        let state = test_state();
        let pane_id = Uuid::new_v4();
        let response = domain_pty_create(
            State(state.clone()),
            test_headers(),
            Json(PtyCreateRequest {
                pty_id: Some(pane_id),
                program: None,
                args: Vec::new(),
                env: HashMap::new(),
                shell: None,
                cwd: None,
                workspace_id: Some(Uuid::new_v4()),
                role: Some("shell".into()),
                launch_profile: None,
                cols: Some(80),
                rows: Some(24),
            }),
        )
        .await
        .expect("stable PTY create")
        .0;
        assert_eq!(response["ok"], true);
        assert_eq!(response["pty_id"], pane_id.to_string());

        let listed = domain_pty_list(State(state.clone()), test_headers())
            .await
            .expect("PTY list")
            .0;
        assert_eq!(listed["source"], "ridge-kernel");
        assert_eq!(listed["ptys"][0]["pty_id"], pane_id.to_string());
        assert_eq!(listed["ptys"][0]["program"], Value::Null);
        assert!(listed["ptys"][0]["child_pid"].as_u64().is_some());
        state.ptys.destroy(pane_id).expect("destroy test PTY");
    }

    #[tokio::test]
    async fn agent_pty_lifecycle_commits_and_tears_down_kernel_identity() {
        let state = test_state();
        let pane_id = Uuid::new_v4();
        let workspace_id = Uuid::new_v4();
        let response = domain_pty_create(
            State(state.clone()),
            test_headers(),
            Json(PtyCreateRequest {
                pty_id: Some(pane_id),
                program: None,
                args: Vec::new(),
                env: HashMap::new(),
                shell: None,
                cwd: None,
                workspace_id: Some(workspace_id),
                role: Some("worker".into()),
                launch_profile: Some("codex".into()),
                cols: None,
                rows: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        let target = {
            let roster = state.roster.lock().unwrap();
            let identity = roster
                .agent_identity(&format!("kernel:{pane_id}"))
                .expect("spawned Agent identity");
            assert_eq!(identity.workspace_id, workspace_id.to_string());
            assert_eq!(
                identity.lifecycle,
                ridge_core::teammate::communication::AgentLifecycle::Online
            );
            json!({
                "workspaceId": identity.workspace_id,
                "agentId": identity.agent_id,
                "generation": identity.generation,
                "lease": identity.lease,
            })
        };
        state
            .mcp_state
            .register_delivery_endpoint(
                HubDeliveryAdapter::RuntimeApi,
                target["workspaceId"].as_str().unwrap(),
                target["agentId"].as_str().unwrap(),
                target["generation"].as_u64().unwrap(),
                target["lease"].as_str().unwrap(),
            )
            .expect("register runtime route");
        assert!(state.mcp_state.delivery_probe(&target).runtime_api);
        let destroyed = domain_pty_destroy(
            State(state.clone()),
            test_headers(),
            Path(pane_id.to_string()),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(destroyed["ok"], true);
        assert!(state
            .roster
            .lock()
            .unwrap()
            .agent_identity(&format!("kernel:{pane_id}"))
            .is_none());
        assert!(!state.mcp_state.delivery_probe(&target).runtime_api);
    }

    #[tokio::test]
    async fn pty_create_rejects_unbounded_launch_payload_before_spawn() {
        let state = test_state();
        let response = domain_pty_create(
            State(state.clone()),
            test_headers(),
            Json(PtyCreateRequest {
                pty_id: Some(Uuid::new_v4()),
                program: Some("cmd.exe".into()),
                args: (0..129).map(|n| n.to_string()).collect(),
                env: HashMap::new(),
                shell: None,
                cwd: None,
                workspace_id: None,
                role: Some("agent".into()),
                launch_profile: None,
                cols: None,
                rows: None,
            }),
        )
        .await
        .expect("bounded launch response")
        .0;
        assert_eq!(response["ok"], false);
        assert_eq!(state.ptys.len(), 0);
    }

    #[test]
    fn agents_nonempty() {
        let v = builtin_agent_profiles();
        assert!(v.as_array().unwrap().len() >= 3);
    }

    #[tokio::test]
    async fn git_status_recognizes_repository_ancestor_off_executor() {
        let root = std::env::temp_dir().join(format!("ridge-kernel-git-{}", Uuid::new_v4()));
        let child = root.join("nested");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        let response = domain_git_status(
            State(test_state()),
            test_headers(),
            Query(GitQuery {
                path: child.to_string_lossy().into_owned(),
                fast: false,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["is_repo"], true);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn fs_list_rejects_paths_outside_kernel_scope() {
        let root = std::env::temp_dir().join(format!("ridge-kernel-fs-root-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let outside = root
            .parent()
            .expect("temp root has parent")
            .join(format!("ridge-kernel-fs-outside-{}", Uuid::new_v4()));
        let mut state = test_state();
        state.fs_scope = ridge_core::sandbox::RootScope::from_roots([root.clone()]);

        let response = domain_fs_list(
            State(state),
            test_headers(),
            Query(FsListQuery {
                path: outside.to_string_lossy().into_owned(),
                offset: None,
                limit: None,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], false);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["error"], "path outside kernel filesystem root");
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_status_fast_confirms_non_repository_without_numstat() {
        let root =
            std::env::temp_dir().join(format!("ridge-kernel-non-git-fast-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let response = domain_git_status(
            State(test_state()),
            test_headers(),
            Query(GitQuery {
                path: root.to_string_lossy().into_owned(),
                fast: true,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["is_repo"], false);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_branches_confirms_non_repository_without_spawning_branch() {
        let root = std::env::temp_dir().join(format!("ridge-kernel-non-git-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let response = domain_git_branches(
            State(test_state()),
            test_headers(),
            Query(GitQuery {
                path: root.to_string_lossy().into_owned(),
                fast: false,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["is_repo"], false);
        assert_eq!(response["branches"], json!([]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_stashes_confirms_non_repository_without_spawning_stash() {
        let root =
            std::env::temp_dir().join(format!("ridge-kernel-non-git-stash-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let response = domain_git_stashes(
            State(test_state()),
            test_headers(),
            Query(GitQuery {
                path: root.to_string_lossy().into_owned(),
                fast: false,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["is_repo"], false);
        assert_eq!(response["stashes"], json!([]));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_diff_summary_confirms_non_repository_without_spawning_diff() {
        let root =
            std::env::temp_dir().join(format!("ridge-kernel-non-git-diff-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let response = domain_git_diff_summary(
            State(test_state()),
            test_headers(),
            Query(GitQuery {
                path: root.to_string_lossy().into_owned(),
                fast: false,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["is_repo"], false);
        assert_eq!(response["summary"], json!({"added": 0, "removed": 0}));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_mutation_rejects_non_repository_before_write() {
        let root =
            std::env::temp_dir().join(format!("ridge-kernel-non-git-mutate-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let response = domain_git_mutate(
            State(test_state()),
            test_headers(),
            Json(GitMutationRequest::Stage {
                repo_root: root.to_string_lossy().into_owned(),
                paths: Vec::new(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], false);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["is_repo"], false);
        assert!(response["error"]
            .as_str()
            .unwrap()
            .contains("Not a git repo"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn git_read_rejects_non_repository_before_spawning_history_command() {
        let root =
            std::env::temp_dir().join(format!("ridge-kernel-non-git-read-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let response = domain_git_read(
            State(test_state()),
            test_headers(),
            Json(GitReadRequest::Commits {
                repo_root: root.to_string_lossy().into_owned(),
                offset: 0,
                limit: 50,
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], true);
        assert_eq!(response["source"], "ridge-kernel");
        assert_eq!(response["is_repo"], false);
        assert!(response["value"].is_null());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn pty_output_http_requires_auth_and_bounds_long_poll() {
        let state = test_state();
        let pty_id = Uuid::new_v4().to_string();
        let unauthorized = domain_pty_output_attach(
            State(state.clone()),
            HeaderMap::new(),
            Path(pty_id.clone()),
            Query(PtyOutputAttachQuery::default()),
        )
        .await;
        assert!(matches!(unauthorized, Err(StatusCode::UNAUTHORIZED)));

        let too_long = domain_pty_output_poll(
            State(state.clone()),
            test_headers(),
            Path((pty_id.clone(), Uuid::new_v4().to_string())),
            Query(PtyOutputPollQuery {
                timeout_ms: Some(MAX_OUTPUT_POLL_TIMEOUT_MS + 1),
                max_frames: Some(1),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(too_long["ok"], false);
        assert!(too_long["error"].as_str().unwrap().contains("timeout_ms"));

        let too_many = domain_pty_output_poll(
            State(state),
            test_headers(),
            Path((pty_id, Uuid::new_v4().to_string())),
            Query(PtyOutputPollQuery {
                timeout_ms: Some(0),
                max_frames: Some(MAX_OUTPUT_POLL_FRAMES + 1),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(too_many["ok"], false);
        assert!(too_many["error"].as_str().unwrap().contains("max_frames"));
    }

    #[tokio::test]
    async fn pty_output_http_unknown_lease_does_not_retry_or_allocate_state() {
        let state = test_state();
        let pty_id = Uuid::new_v4().to_string();
        let lease_id = Uuid::new_v4().to_string();
        let response = domain_pty_output_detach(
            State(state.clone()),
            test_headers(),
            Path((pty_id.clone(), lease_id)),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(response["ok"], false);
        assert_eq!(state.output_leases.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn pty_output_http_poll_resync_and_detach_share_one_cursor() {
        let state = test_state();
        let pty_id = Uuid::new_v4();
        let lease = crate::pty::test_output_lease();
        let lease_id = lease.id();
        state.output_leases.lock().unwrap().insert(
            lease_id,
            std::sync::Arc::new(OutputLeaseSlot {
                pty_id,
                lease,
                poll_lock: tokio::sync::Mutex::new(()),
            }),
        );

        let timeout = domain_pty_output_poll(
            State(state.clone()),
            test_headers(),
            Path((pty_id.to_string(), lease_id.to_string())),
            Query(PtyOutputPollQuery {
                timeout_ms: Some(0),
                max_frames: Some(1),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(timeout["kind"], "timeout");

        let resynced = domain_pty_output_resync(
            State(state.clone()),
            test_headers(),
            Path((pty_id.to_string(), lease_id.to_string())),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(resynced["kind"], "resynced");
        assert_eq!(resynced["cursor_seq"], 1);

        let detached = domain_pty_output_detach(
            State(state.clone()),
            test_headers(),
            Path((pty_id.to_string(), lease_id.to_string())),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(detached["kind"], "detached");
        assert!(state.output_leases.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn remote_host_handlers_keep_topology_in_kernel() {
        let state = test_state();
        let persisted = state.remote_hosts_path.clone();
        let host = ridge_core::remote::HostRecord {
            id: "remote-a".into(),
            kind: ridge_core::remote::HostKind::Remote,
            label: "A".into(),
            addr: "127.0.0.1:9900".into(),
            status: ridge_core::remote::HostStatus::Connected,
            detail: "live".into(),
            sessions: vec![],
        };
        let _ = domain_remote_host_upsert(State(state.clone()), test_headers(), Json(host))
            .await
            .unwrap();
        let listed = domain_remote_hosts(State(state.clone()), test_headers())
            .await
            .unwrap()
            .0;
        assert_eq!(listed["hosts"][0]["id"], "remote-a");
        let restored: std::collections::HashMap<String, ridge_core::remote::HostRecord> =
            serde_json::from_slice(&std::fs::read(&persisted).unwrap()).unwrap();
        assert_eq!(restored["remote-a"].label, "A");
        let removed =
            domain_remote_host_remove(State(state), test_headers(), Path("remote-a".into()))
                .await
                .unwrap()
                .0;
        assert_eq!(removed["removed"], true);
        let _ = std::fs::remove_file(persisted);
    }

    #[tokio::test]
    async fn remote_host_session_attach_detach_is_atomic_and_idempotence_fails_closed() {
        let state = test_state();
        let persisted = state.remote_hosts_path.clone();
        let host = ridge_core::remote::HostRecord {
            id: "remote-session".into(),
            kind: ridge_core::remote::HostKind::Remote,
            label: "Session host".into(),
            addr: "127.0.0.1:9901".into(),
            status: ridge_core::remote::HostStatus::Connected,
            detail: "live".into(),
            sessions: vec![ridge_core::remote::HostSessionMeta {
                id: "session-a".into(),
                title: "shell".into(),
                attached: false,
            }],
        };
        let _ = domain_remote_host_upsert(State(state.clone()), test_headers(), Json(host))
            .await
            .unwrap();

        let request = || {
            Json(RemoteHostSessionMutation {
                host_id: "remote-session".into(),
                session_id: "session-a".into(),
            })
        };
        let attached =
            domain_remote_host_session_attach(State(state.clone()), test_headers(), request())
                .await
                .unwrap()
                .0;
        assert_eq!(attached["ok"], true);
        assert_eq!(attached["attached"], true);

        let duplicate =
            domain_remote_host_session_attach(State(state.clone()), test_headers(), request())
                .await
                .unwrap()
                .0;
        assert_eq!(duplicate["ok"], false);

        let detached =
            domain_remote_host_session_detach(State(state.clone()), test_headers(), request())
                .await
                .unwrap()
                .0;
        assert_eq!(detached["ok"], true);
        assert_eq!(detached["attached"], false);

        let listed = domain_remote_hosts(State(state), test_headers())
            .await
            .unwrap()
            .0;
        assert_eq!(listed["hosts"][0]["sessions"][0]["attached"], false);
        let _ = std::fs::remove_file(persisted);
    }

    #[test]
    fn workspace_detail_tracks_kernel_graph() {
        let mut graph = ridge_core::workspace::graph::WorkspaceGraph::new();
        let id = graph.create_workspace();
        let body = workspace_detail(&graph, id).unwrap();
        assert_eq!(body["workspace_id"], id.to_string());
        assert_eq!(body["panes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn workspace_list_reports_active_kernel_workspace() {
        let mut graph = ridge_core::workspace::graph::WorkspaceGraph::new();
        let id = graph.create_workspace();
        let body = workspace_list(&graph);
        assert_eq!(body["active"], id.to_string());
        assert_eq!(body["workspaces"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn workspace_handlers_mutate_only_kernel_state() {
        let state = test_state();
        let persisted = state.workspaces_path.clone();
        let created = domain_workspace_create(State(state.clone()), test_headers())
            .await
            .unwrap()
            .0;
        let workspace_id = created["workspace_id"].as_str().unwrap().to_string();
        let detail = domain_workspace_get(
            State(state.clone()),
            test_headers(),
            Path(workspace_id.clone()),
        )
        .await
        .unwrap()
        .0;
        let pane_id = detail["panes"][0].as_str().unwrap().to_string();
        let split = domain_workspace_split(
            State(state.clone()),
            test_headers(),
            Path(workspace_id.clone()),
            Json(WorkspaceSplitRequest {
                pane_id,
                direction: "horizontal".to_string(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert!(split["pane_id"].is_string());
        let restored: ridge_core::workspace::graph::WorkspaceGraph =
            serde_json::from_slice(&std::fs::read(&persisted).unwrap()).unwrap();
        assert_eq!(restored.active().unwrap().to_string(), workspace_id);
        assert_eq!(
            restored.leaves(restored.active().unwrap()).unwrap().len(),
            2
        );
        let listed = domain_workspaces(State(state), test_headers())
            .await
            .unwrap()
            .0;
        assert_eq!(listed["active"], workspace_id);
        let _ = std::fs::remove_file(persisted);
    }

    #[tokio::test]
    async fn roster_handlers_keep_agent_state_in_kernel() {
        let state = test_state();
        let persisted = state.roster_path.clone();
        let _ = domain_agent_roster_add(
            State(state.clone()),
            test_headers(),
            Json(AgentRosterAddRequest {
                id: "codex-1".to_string(),
                name: "Codex".to_string(),
                pane_id: 7,
            }),
        )
        .await
        .unwrap();
        let listed = domain_agent_roster(State(state.clone()), test_headers())
            .await
            .unwrap()
            .0;
        assert_eq!(listed["roster"][0]["capability"], "Skilled");
        let restored: ridge_core::teammate::topology::TopologyGraph =
            serde_json::from_slice(&std::fs::read(&persisted).unwrap()).unwrap();
        assert!(restored.get("codex-1").is_some());
        let _ = domain_agent_roster_remove(
            State(state.clone()),
            test_headers(),
            Path("codex-1".to_string()),
        )
        .await
        .unwrap();
        let listed = domain_agent_roster(State(state), test_headers())
            .await
            .unwrap()
            .0;
        assert!(listed["roster"].as_array().unwrap().is_empty());
        let _ = std::fs::remove_file(persisted);
    }

    #[tokio::test]
    async fn identity_commit_requires_online_and_fences_generation_and_lease() {
        let state = test_state();
        let request =
            |generation: u64, lease: &str, online: bool, lifecycle| AgentIdentityCommitRequest {
                agent_id: "agent-1".into(),
                session_id: "session-1".into(),
                workspace_id: "workspace-1".into(),
                pane_id: "pane-1".into(),
                cwd: "C:\\code\\wind".into(),
                executable: "codex".into(),
                argv: vec!["--full-auto".into()],
                generation,
                lease: lease.into(),
                lifecycle,
                online,
                last_seen_unix_ms: 1,
                capabilities: vec!["messages".into()],
            };

        let rejected = domain_agent_identity_commit(
            State(state.clone()),
            test_headers(),
            Json(request(
                1,
                "lease-1",
                false,
                ridge_core::teammate::communication::AgentLifecycle::Failed,
            )),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(rejected["ok"], false);

        let committed = domain_agent_identity_commit(
            State(state.clone()),
            test_headers(),
            Json(request(
                1,
                "lease-1",
                true,
                ridge_core::teammate::communication::AgentLifecycle::Online,
            )),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(committed["ok"], true);

        let stale = domain_agent_identity_commit(
            State(state.clone()),
            test_headers(),
            Json(request(
                1,
                "lease-2",
                true,
                ridge_core::teammate::communication::AgentLifecycle::Online,
            )),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(stale["ok"], false);
        let roster = state.roster.lock().unwrap();
        assert_eq!(roster.agent_identity("agent-1").unwrap().lease, "lease-1");
    }
}
