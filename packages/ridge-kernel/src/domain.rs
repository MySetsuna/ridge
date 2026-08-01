//! Kernel domain endpoints (REQ-RIDGE-KERNEL-DOMAIN-01 first slices).
//! FS and workspace topology use ridge-core; Agent profiles are kernel-owned.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::AppState;

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("x-ridge-kernel-token")
        .or_else(|| headers.get("x-ridge-token"))
        .and_then(|v| v.to_str().ok())
        == Some(token)
}

/// Kernel-owned default agent profiles.
pub fn builtin_agent_profiles() -> Value {
    serde_json::to_value(ridge_kernel::agent_profiles::builtin_profiles())
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
    let workspace_id = graph.create_workspace();
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
    match graph.set_active(workspace_id) {
        Ok(()) => Ok(Json(json!({ "ok": true, "workspace_id": workspace_id }))),
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
    match graph.split(workspace_id, pane_id, direction) {
        Ok(new_pane_id) => Ok(Json(json!({ "ok": true, "pane_id": new_pane_id }))),
        Err(e) => Ok(bad_request(e.to_string())),
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
    match graph.close(workspace_id, pane_id) {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(e) => Ok(bad_request(e.to_string())),
    }
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
    match graph.set_locked_size(workspace_id, pane_id, request.cols, request.rows) {
        Ok(()) => Ok(Json(
            json!({ "ok": true, "cols": request.cols, "rows": request.rows }),
        )),
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
    })
}

pub async fn domain_agent_roster(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(roster_snapshot(&st.roster.lock().expect("roster lock"))))
}

#[derive(Deserialize)]
pub struct AgentRosterAddRequest {
    pub id: String,
    pub name: String,
    pub pane_id: u32,
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
    let teammate = ridge_core::teammate::model::Teammate::new(request.id, request.name, request.pane_id)
        .with_capability(capability);
    let agent_id = teammate.id.clone();
    st.roster
        .lock()
        .expect("roster lock")
        .add_teammate(teammate);
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
    st.roster
        .lock()
        .expect("roster lock")
        .remove_teammate(&agent_id);
    Ok(Json(json!({ "ok": true, "agent_id": agent_id })))
}

#[derive(Deserialize)]
pub struct PtyCreateRequest {
    pub shell: Option<String>,
    pub cwd: Option<String>,
}

pub async fn domain_pty_create(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PtyCreateRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    match st.ptys.spawn(request.shell.as_deref(), request.cwd.as_deref()) {
        Ok(pty_id) => Ok(Json(json!({ "ok": true, "pty_id": pty_id }))),
        Err(error) => Ok(bad_request(error.to_string())),
    }
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
    if !auth_ok(&headers, &st.token) { return Err(StatusCode::UNAUTHORIZED); }
    let pty_id = match parse_id(&pty_id, "pty") { Ok(id) => id, Err(body) => return Ok(body) };
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
pub struct PtyResizeRequest { pub cols: u16, pub rows: u16 }

pub async fn domain_pty_resize(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Json(request): Json<PtyResizeRequest>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) { return Err(StatusCode::UNAUTHORIZED); }
    let pty_id = match parse_id(&pty_id, "pty") { Ok(id) => id, Err(body) => return Ok(body) };
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
    if !auth_ok(&headers, &st.token) { return Err(StatusCode::UNAUTHORIZED); }
    let pty_id = match parse_id(&pty_id, "pty") { Ok(id) => id, Err(body) => return Ok(body) };
    match st.ptys.destroy(pty_id) {
        Ok(()) => Ok(Json(json!({ "ok": true }))),
        Err(error) => Ok(bad_request(error.to_string())),
    }
}

#[derive(Deserialize)]
pub struct PtyScrollbackQuery { pub max_bytes: Option<usize> }

pub async fn domain_pty_scrollback(
    State(st): State<AppState>,
    headers: HeaderMap,
    Path(pty_id): Path<String>,
    Query(query): Query<PtyScrollbackQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) { return Err(StatusCode::UNAUTHORIZED); }
    let pty_id = match parse_id(&pty_id, "pty") { Ok(id) => id, Err(body) => return Ok(body) };
    match st.ptys.scrollback(pty_id, query.max_bytes.unwrap_or(64 * 1024)) {
        Ok(bytes) => Ok(Json(json!({
            "ok": true,
            "pty_id": pty_id,
            "data_b64": base64::engine::general_purpose::STANDARD.encode(bytes),
        }))),
        Err(error) => Ok(bad_request(error.to_string())),
    }
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
}

pub async fn domain_git_status(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<GitQuery>,
) -> Result<Json<Value>, StatusCode> {
    if !auth_ok(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let is_repo = ridge_core::commands::git::is_git_repo(q.path.clone());
    if !is_repo {
        return Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": false,
            "path": q.path,
        })));
    }
    match ridge_core::commands::git::get_scm_status_sync(q.path.clone()) {
        Ok(status) => Ok(Json(json!({
            "ok": true,
            "source": "ridge-kernel",
            "is_repo": true,
            "path": q.path,
            "status": status,
        }))),
        Err(e) => Ok(Json(json!({
            "ok": false,
            "source": "ridge-kernel",
            "is_repo": true,
            "error": e,
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
            roster: Arc::new(std::sync::Mutex::new(
                ridge_core::teammate::topology::TopologyGraph::new(),
            )),
            ptys: Arc::new(ridge_kernel::pty::PtyRegistry::default()),
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

    #[test]
    fn agents_nonempty() {
        let v = builtin_agent_profiles();
        assert!(v.as_array().unwrap().len() >= 3);
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
        let listed = domain_workspaces(State(state), test_headers())
            .await
            .unwrap()
            .0;
        assert_eq!(listed["active"], workspace_id);
    }

    #[tokio::test]
    async fn roster_handlers_keep_agent_state_in_kernel() {
        let state = test_state();
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
    }
}
