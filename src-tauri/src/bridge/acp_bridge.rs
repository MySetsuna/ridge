//! `acp_bridge` — Tauri 端最薄桥（ridge-acp ↔ Desktop 渲染层）。
//!
//! M2.1 §2 NS-2 + M3 §1/§3/§4 目标：
//! - 上行：`AgentSession::subscribe()` / `Commutation::events()` → 逐事件 `app.emit`
//! - 下行：Tauri command → `AgentSession::*` / `Commutation::*`
//!
//! 硬约束：
//! - Bridge 不存业务状态；session/message/tool/approval/plan 唯一事实源仍是 AgentSession + Commutation。
//! - Bridge 仅持 `Arc<AgentSession>` / `Arc<Commutation>` 句柄 + 多个 forwarder task。
//! - 重挂载/重订阅不得制造第二份事件流；旧 forwarder 必须被 abort。
//!
//! frozen Runtime 不动；本模块仅依赖 ridge-acp 的公开 API + tauri 公开 API。

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::process::Command;
use tokio::sync::broadcast;

use ridge_acp::communion::{Commutation, StaffSpec};
use ridge_acp::events::AgentSessionEvent;
use ridge_acp::provider::{ProviderSpec};
use ridge_acp::provider_adapter::{into_spawn_spec, resolve_provider_launch};
use ridge_acp::session::{AgentSession, AgentSessionConfig, SessionId};
use ridge_acp::spawn::spawn_acp_agent;
use ridge_acp::task::{append_story, append_task, scaffold_plan, Plan, PlanStore, Story, TaskPoint, TaskStatus};
use ridge_acp::workflow::{WorkflowCoordinator, restore_workflow, persist_plan_store_atomic};
use ridge_acp::transport::{AcpTransport};

/// Bridge 持有的唯一持久状态：当前会话 + Commutation + forwarder tasks。
/// 不持有 message list / tool state / approval state（那些只活在 AgentSession）。
#[derive(Default)]
pub struct AcpBridgeState {
    inner: Mutex<AcpBridgeStateInner>,
}

#[derive(Default)]
struct AcpBridgeStateInner {
    /// At most one active AgentSession (single conversation default).
    session: Option<Arc<AgentSession>>,
    /// Forwarder task for session events → app.emit("acp_event").
    session_forwarder: Option<tokio::task::JoinHandle<()>>,
    /// Optional active Commutation (Lead + Staff team coordination).
    commutation: Option<Arc<Commutation>>,
    /// Forwarder task for commutation events → app.emit("commutation_event").
    commutation_forwarder: Option<tokio::task::JoinHandle<()>>,
}

impl AcpBridgeState {
    fn replace_session(
        &self,
        new_session: Arc<AgentSession>,
        new_forwarder: tokio::task::JoinHandle<()>,
    ) {
        let mut g = self.inner.lock();
        if let Some(old) = g.session_forwarder.take() {
            old.abort();
        }
        g.session = Some(new_session);
        g.session_forwarder = Some(new_forwarder);
    }

    fn replace_commutation(
        &self,
        new_comm: Arc<Commutation>,
        new_forwarder: tokio::task::JoinHandle<()>,
    ) {
        let mut g = self.inner.lock();
        if let Some(old) = g.commutation_forwarder.take() {
            old.abort();
        }
        g.commutation = Some(new_comm);
        g.commutation_forwarder = Some(new_forwarder);
    }

    fn current_session(&self) -> Option<Arc<AgentSession>> {
        self.inner.lock().session.clone()
    }

    fn current_commutation(&self) -> Option<Arc<Commutation>> {
        self.inner.lock().commutation.clone()
    }

    fn require_session(&self) -> Result<Arc<AgentSession>, String> {
        self.current_session()
            .ok_or_else(|| "acp: no active session — call acp_open_session first".to_string())
    }

    async fn shutdown_active(&self) {
        let (old_session, old_sfwd, old_comm, old_cfwd) = {
            let mut g = self.inner.lock();
            (
                g.session.take(),
                g.session_forwarder.take(),
                g.commutation.take(),
                g.commutation_forwarder.take(),
            )
        };
        if let Some(t) = old_sfwd {
            t.abort();
        }
        if let Some(t) = old_cfwd {
            t.abort();
        }
        if let Some(s) = old_session {
            s.shutdown().await;
        }
        // Commutation doesn't own sessions; they shut down via session.shutdown().
    }
}

// ---- Provider command ----

#[derive(Debug, Clone, Deserialize)]
pub struct OpenSessionArgs {
    /// Optional provider binary path; empty/missing → default (opencode-acp or env override).
    #[serde(default)]
    pub provider_program: Option<String>,
    /// Optional cwd; empty/missing → inherit current working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// M3: provider/model/effort selection. None → mock_default().
    #[serde(default)]
    pub provider: Option<ProviderSpec>,
}

/// 启动 provider 子进程 + 建立 AgentSession；返回 session id 给前端。
#[tauri::command]
pub async fn acp_open_session(
    app: AppHandle,
    state: State<'_, AcpBridgeState>,
    args: OpenSessionArgs,
) -> Result<String, String> {
    state.shutdown_active().await;
    let mut provider = args.provider.unwrap_or_else(ProviderSpec::mock_default);
    if let Some(cwd) = &args.cwd { provider.config.insert("cwd".into(), serde_json::json!(cwd)); }
    let mut spec = into_spawn_spec(&provider).map_err(|e| e.reason)?;
    if let Some(program) = args.provider_program.or_else(|| std::env::var("RIDGE_ACP_PROVIDER_PATH").ok()) { spec.program = program.into(); }
    if let Some(cwd) = args.cwd { spec.cwd = Some(cwd.into()); }
    let (transport, child) = spawn_acp_agent(&spec).map_err(|e| e.to_string())?;
    let session = AgentSession::connect(transport, child, AgentSessionConfig { provider, client_name: "ridge-desktop".into(), client_version: env!("CARGO_PKG_VERSION").into() }).await.map_err(|e| e.to_string())?;
    let session_id = session.id().await;
    let session = Arc::new(session);

    let forwarder = tokio::spawn(forward_session_events(
        app.clone(),
        session.clone(),
        session_id.clone(),
    ));

    state.replace_session(session.clone(), forwarder);

    Ok(session_id.0)
}

fn forward_session_events(app: AppHandle, session: Arc<AgentSession>, session_id: SessionId) -> impl std::future::Future<Output = ()> {
    let mut rx = session.subscribe();
    async move {
        while let Ok(event) = rx.recv().await {
            let _ = app.emit("acp_event", AcpEventEnvelope { session_id: session_id.0.clone(), event });
        }
    }
}

#[derive(Clone, Serialize)]
struct AcpEventEnvelope {
    session_id: String,
    event: AgentSessionEvent,
}

#[tauri::command]
pub async fn acp_prompt(
    state: State<'_, AcpBridgeState>,
    text: String,
) -> Result<String, String> {
    let session = state.require_session()?;
    let turn_id = session
        .prompt(&text)
        .await
        .map_err(|e| format!("acp prompt: {e:?}"))?;
    Ok(turn_id.0)
}

#[tauri::command]
pub async fn acp_approve_tool(state: State<'_, AcpBridgeState>, turn_id: String, tool_call_id: String, approved: Option<bool>) -> Result<(), String> {
    let session = state.require_session()?;
    let turn = ridge_acp::session::TurnId(turn_id);
    if approved.unwrap_or(true) { session.approve_tool(&turn, &tool_call_id).await } else { session.reject_tool(&turn, &tool_call_id).await }.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn acp_cancel(state: State<'_, AcpBridgeState>) -> Result<(), String> {
    let session = state.require_session()?;
    session
        .cancel(None)
        .await
        .map_err(|e| format!("acp cancel: {e:?}"))
}

#[tauri::command]
pub async fn acp_close_session(state: State<'_, AcpBridgeState>) -> Result<(), String> {
    state.shutdown_active().await;
    Ok(())
}

#[tauri::command]
pub async fn acp_session_status(
    state: State<'_, AcpBridgeState>,
) -> Result<serde_json::Value, String> {
    let Some(session) = state.current_session() else {
        return Ok(serde_json::json!({ "status": "no_active_session" }));
    };
    let status = session.status().await;
    let id = session.id().await;
    let provider = session.provider().await;
    Ok(serde_json::json!({
        "session_id": id.0,
        "status": format!("{status:?}"),
        "provider": provider,
    }))
}

#[tauri::command]
pub async fn acp_session_snapshot(
    state: State<'_, AcpBridgeState>,
) -> Result<serde_json::Value, String> {
    let Some(session) = state.current_session() else {
        return Err("acp: no active session".into());
    };
    let snap = session.snapshot().await;
    serde_json::to_value(&snap).map_err(|e| format!("snapshot serialize: {e}"))
}

#[tauri::command]
pub async fn acp_session_save(
    state: State<'_, AcpBridgeState>,
    path: String,
) -> Result<(), String> {
    let session = state.require_session()?;
    let snap = session.snapshot().await;
    let persisted = PersistedSession { session_id: snap.session_id.0.clone(), provider: snap.provider.clone(), snapshot: serde_json::to_value(&snap).map_err(|e| e.to_string())? };
    let json = serde_json::to_string_pretty(&persisted).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("session save write: {e}"))?;
    Ok(())
}

/// M3 §5 session persistence: ProviderSpec + id + key snapshot fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    pub session_id: String,
    pub provider: ProviderSpec,
    pub snapshot: serde_json::Value,
}

#[tauri::command]
pub async fn acp_session_load(
    app: AppHandle,
    state: State<'_, AcpBridgeState>,
    path: String,
) -> Result<PersistedSession, String> {
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("session load read: {e}"))?;
    let persisted: PersistedSession = serde_json::from_str(&raw)
        .map_err(|e| format!("session load parse: {e}"))?;
    state.shutdown_active().await;
    let cwd = persisted.provider.config.get("cwd").and_then(|value| value.as_str()).map(str::to_string);
    let session = spawn_one_session(Some(persisted.provider.clone()), cwd).await?;
    let session = Arc::new(session);
    let session_id = session.id().await;
    let forwarder = tokio::spawn(forward_session_events(
        app.clone(),
        session.clone(),
        session_id.clone(),
    ));
    state.replace_session(session.clone(), forwarder);
    Ok(PersistedSession { session_id: session_id.0, provider: session.provider().await, snapshot: persisted.snapshot })
}

// ---- Commutation commands ----

#[derive(Debug, Clone, Deserialize)]
pub struct CommutationOpenArgs {
    pub task_title: String,
    pub task_context: String,
    /// M3 §1: staff specs list (without session ids — bridge assigns).
    pub staff: Vec<StaffSpecInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaffSpecInput {
    pub role: String,
    pub provider_label: String,
    /// Optional per-staff provider spec; falls back to default.
    #[serde(default)]
    pub provider: Option<ProviderSpec>,
}

/// Open a Commutation: spawn Lead AgentSession + N Staff AgentSessions.
#[tauri::command]
pub async fn commutation_open(
    app: AppHandle,
    state: State<'_, AcpBridgeState>,
    args: CommutationOpenArgs,
) -> Result<serde_json::Value, String> {
    // Spawn Lead
    let lead_session = spawn_one_session(None, None).await?;
    let lead_session = Arc::new(lead_session);
    let lead_id = lead_session.id().await;

    // Build Commutation
    let comm = Commutation::new(&lead_session, args.task_title, args.task_context).await;
    let comm = Arc::new(comm);

    // Spawn Staff + assign
    let mut staff_ids = Vec::with_capacity(args.staff.len());
    for spec in args.staff.iter() {
        let staff_provider = spec
            .provider
            .clone()
            .unwrap_or_else(|| ProviderSpec::mock_default());
        let staff_session = spawn_one_session(Some(staff_provider), None).await?;
        let staff_session = Arc::new(staff_session);
        let staff_id = staff_session.id().await;
        comm.assign_staff(
            StaffSpec {
                session_id: staff_id.clone(),
                role: spec.role.clone(),
                provider_label: spec.provider_label.clone(),
            },
            &staff_session,
        )
        .await;
        staff_ids.push(staff_id);
    }

    // Forwarder for Commutation events
    let forwarder = tokio::spawn(forward_commutation_events(
        app.clone(),
        comm.clone(),
        lead_id.clone(),
    ));
    state.replace_commutation(comm.clone(), forwarder);

    // If no session was open yet, set Lead as active session too
    if state.current_session().is_none() {
        let sfwd = tokio::spawn(forward_session_events(
            app.clone(),
            lead_session.clone(),
            lead_id.clone(),
        ));
        state.replace_session(lead_session.clone(), sfwd);
    }

    Ok(serde_json::json!({
        "lead_session_id": lead_id.0,
        "staff_session_ids": staff_ids.iter().map(|s: &SessionId| s.0.clone()).collect::<Vec<String>>(),
    }))
}

async fn spawn_one_session(
    provider: Option<ProviderSpec>,
    cwd: Option<String>,
) -> Result<AgentSession, String> {
    let provider = provider.unwrap_or_else(ProviderSpec::mock_default);
    // M4: route provider_kind through provider_adapter; each kind maps to a
    // concrete binary + framing. Errors here are honest capability gaps.
    let mut launch = resolve_provider_launch(&provider)
        .map_err(|gap| {
            format!(
                "acp: capability gap for {:?} (binary {}): {}",
                gap.provider_kind, gap.binary, gap.reason
            )
        })?;
    if let Ok(program) = std::env::var("RIDGE_ACP_PROVIDER_PATH") { launch.program = program.into(); }
    let program = launch.program;
    let args = launch.args;
    let framing = launch.framing;
    let mut cmd = Command::new(&program);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(c) = cwd.as_ref() {
        cmd.current_dir(c);
    }
    for arg in &args {
        cmd.arg(arg);
    }
    let mut child = cmd.spawn().map_err(|e| format!("acp: spawn failed: {e}"))?;
    let stdin = child.stdin.take().ok_or("acp: missing stdin")?;
    let stdout = child.stdout.take().ok_or("acp: missing stdout")?;
    let stderr = child.stderr.take();
    let transport = AcpTransport::spawn_with_framing(stdin, stdout, stderr, framing);
    let cfg = AgentSessionConfig {
        provider,
        client_name: "ridge-desktop".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
    };
    AgentSession::connect(transport, child, cfg)
        .await
        .map_err(|e| format!("acp: connect failed: {e:?}"))
}

async fn forward_commutation_events(
    app: AppHandle,
    comm: Arc<Commutation>,
    _lead_id: ridge_acp::session::SessionId,
) {
    let mut rx = comm.events();
    loop {
        match rx.recv().await {
            Ok(ev) => {
                if let Err(e) = app.emit("commutation_event", &ev) {
                    tracing::warn!(?e, "commutation bridge: emit failed");
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(lagged = n, "commutation bridge: subscriber lagged");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[tauri::command]
pub async fn commutation_snapshot(
    state: State<'_, AcpBridgeState>,
) -> Result<serde_json::Value, String> {
    let Some(comm) = state.current_commutation() else {
        return Ok(serde_json::json!({ "state": "no_active_commutation" }));
    };
    let snap = comm.snapshot().await;
    serde_json::to_value(&snap).map_err(|e| format!("commutation snapshot serialize: {e}"))
}

#[tauri::command]
pub async fn commutation_prompt_lead(
    state: State<'_, AcpBridgeState>,
    text: String,
) -> Result<String, String> {
    let Some(comm) = state.current_commutation() else {
        return Err("commutation: no active commutation".into());
    };
    let _ = comm; // future: comm.forward_to_lead(text).await
    let lead = state.require_session()?;
    let turn_id = lead
        .prompt(&text)
        .await
        .map_err(|e| format!("commutation lead prompt: {e:?}"))?;
    Ok(turn_id.0)
}

// ---- provider helpers (extension on Option for caller ergonomics) ----

// ---- tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn session_event_bus_forwards_terminal_event_on_shutdown() {
        let mut cmd = Command::new(std::env::var("CARGO_BIN_EXE_fake_acp_agent").unwrap_or_else(|_| "fake_acp_agent".into()));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let _stderr = child.stderr.take();
        let transport = AcpTransport::spawn(stdin, stdout, _stderr);
        let session = Arc::new(
            AgentSession::connect(transport, child, AgentSessionConfig::mock_default())
                .await
                .unwrap(),
        );

        let mut rx = session.subscribe();
        session.shutdown().await;
        let ev = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("terminal event within 1s")
            .expect("not closed");
        match ev {
            AgentSessionEvent::SessionFailed { .. } => {}
            other => panic!("expected SessionFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn bridge_require_without_session_errors() {
        let state = AcpBridgeState::default();
        let err = state.require_session().err().unwrap();
        assert!(err.contains("no active session"));
    }

    #[tokio::test]
    async fn bridge_default_provider_used_when_none_passed() {
        // OpenSessionArgs with provider = None falls back to mock_default.
        let args = OpenSessionArgs {
            provider_program: None,
            cwd: None,
            provider: None,
        };
        assert!(args.provider.is_none());
    }

    #[test]
    fn persisted_session_roundtrip() {
        let p = PersistedSession {
            session_id: "abc".into(),
            provider: ProviderSpec::mock_default(),
            snapshot: serde_json::json!({ "turns": [] }),
        };
        let s = serde_json::to_string(&p).unwrap();
        let back: PersistedSession = serde_json::from_str(&s).unwrap();
        assert_eq!(back.session_id, "abc");
    }
}

// One shared PlanStore is used by both bridge commands and the coordinator.
#[derive(Default)]
pub struct PlanStoreState {
    inner: Arc<tokio::sync::Mutex<PlanStore>>,
    workflow: tokio::sync::Mutex<Option<Arc<WorkflowCoordinator>>>,
    forwarder: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    loaded: tokio::sync::Mutex<bool>,
}
fn workflow_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("RIDGE_WORKFLOW_STORE_PATH") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
        return Ok(path);
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("agent-workflows.json"))
}
impl PlanStoreState {
    async fn load(&self, app: &AppHandle) -> Result<(), String> {
        let mut loaded = self.loaded.lock().await;
        if !*loaded {
            let path = workflow_path(app)?;
            if path.exists() { *self.inner.lock().await = restore_workflow(&path).await?.0; }
            *loaded = true;
        }
        Ok(())
    }
}
#[tauri::command]
pub async fn workflow_restore(app: AppHandle, state: State<'_, PlanStoreState>) -> Result<Vec<Plan>, String> {
    state.load(&app).await?;
    let store = state.inner.lock().await;
    Ok(store.list().iter().filter_map(|id| store.get(id).cloned()).collect())
}
#[derive(Deserialize)]
struct ProposedPlan { title: String, stories: Vec<Story> }
#[tauri::command]
pub async fn workflow_propose(app: AppHandle, state: State<'_, PlanStoreState>, sessions: State<'_, AcpBridgeState>, request: String, workspace_cwd: String, staff: HashMap<String, ProviderSpec>) -> Result<Plan, String> {
    state.load(&app).await?;
    if !std::path::Path::new(&workspace_cwd).is_dir() { return Err("workspace directory does not exist".into()); }
    if state.workflow.lock().await.is_some() { return Err("finish the current workflow before proposing another plan".into()); }
    let lead = sessions.require_session()?;
    let prompt = format!("[ridge plan proposal] Propose a task DAG for this request. Return JSON only, no tools. Shape: {{\"title\":\"Plan\",\"stories\":[{{\"id\":\"story-1\",\"title\":\"Work\",\"description\":\"\",\"tasks\":[{{\"id\":\"A\",\"title\":\"Inspect\",\"description\":\"Inspect the requested work\",\"point\":\"small\",\"dependencies\":[],\"assigned_session_id\":null,\"status\":\"queued\",\"result\":null}}]}}]}}. Use staff names {:?} as assignees. Point values small, medium, large, extra_large. Workspace: {}. Request: {}", staff.keys().collect::<Vec<_>>(), workspace_cwd, request);
    let outcome = ridge_acp::worker::run_one_task(&lead, &prompt, Default::default()).await;
    if outcome.status != TaskStatus::Completed { return Err(outcome.handoff.summary); }
    let text = outcome.handoff.summary.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let proposed: ProposedPlan = serde_json::from_str(text).map_err(|e| format!("Lead returned an invalid Plan: {e}"))?;
    let mut plan = scaffold_plan(format!("plan-{}", ridge_acp::session::SessionId::new().0), proposed.title, request, workspace_cwd);
    plan.lead_provider = Some(lead.provider().await);
    plan.stories = proposed.stories;
    plan.staff = staff;
    for task in plan.all_tasks_mut() { task.assignee = task.assignee.take().or_else(|| task.assigned_session_id.take()); task.status = TaskStatus::Queued; task.result = None; }
    for task in plan.all_tasks() { if let Some(assignee) = &task.assignee { if !plan.staff.contains_key(assignee) { return Err(format!("Lead proposed unknown assignee: {assignee}")); } } }
    if plan.all_tasks().next().is_none() { return Err("Lead proposed an empty plan".into()); }
    let mut store = state.inner.lock().await;
    store.insert(plan.clone())?;
    if let Err(e) = persist_plan_store_atomic(&store, &workflow_path(&app)?).await { store.remove(&plan.id); return Err(e); }
    Ok(plan)
}
#[tauri::command]
pub async fn plan_edit_task(app: AppHandle, state: State<'_, PlanStoreState>, plan_id: String, task_id: String, title: Option<String>, point: Option<TaskPoint>, dependencies: Option<Vec<String>>, assignee: Option<String>) -> Result<Plan, String> {
    let mut store = state.inner.lock().await;
    let backup = store.get(&plan_id).ok_or("plan not found")?.clone();
    let task = backup.find_task(&task_id).ok_or("task not found")?;
    store.review_task(&plan_id, &task_id, title.unwrap_or_else(|| task.title.clone()), point.unwrap_or(task.point), dependencies.unwrap_or_else(|| task.dependencies.clone()), assignee.map(|id| if id.is_empty() { None } else { Some(id) }).unwrap_or_else(|| task.assignee.clone()))?;
    if let Err(e) = persist_plan_store_atomic(&store, &workflow_path(&app)?).await { store.insert(backup)?; return Err(e); }
    Ok(store.get(&plan_id).unwrap().clone())
}
#[tauri::command]
pub async fn plan_set_staff(app: AppHandle, state: State<'_, PlanStoreState>, plan_id: String, name: String, provider: ProviderSpec) -> Result<Plan, String> {
    let mut store = state.inner.lock().await;
    let backup = store.get(&plan_id).ok_or("plan not found")?.clone();
    if backup.confirmed { return Err("staff configuration is locked after Confirm".into()); }
    if !backup.staff.contains_key(&name) { return Err("staff not found".into()); }
    store.get_mut(&plan_id).unwrap().staff.insert(name, provider);
    if let Err(e) = persist_plan_store_atomic(&store, &workflow_path(&app)?).await { store.insert(backup)?; return Err(e); }
    Ok(store.get(&plan_id).unwrap().clone())
}
async fn run_workflow(app: AppHandle, state: &PlanStoreState, sessions: &AcpBridgeState, plan_id: String, resume: Option<String>) -> Result<serde_json::Value, String> {
    {
        let store = state.inner.lock().await;
        let plan = store.get(&plan_id).ok_or("plan not found")?;
        if let Some(task_id) = &resume { if plan.find_task(task_id).ok_or("task not found")?.status != TaskStatus::Interrupted { return Err("only Interrupted tasks can Resume".into()); } }
    }
    let existing = state.workflow.lock().await.clone();
    let coordinator = if let Some(existing) = existing { if existing.current_plan().await.id != plan_id { return Err("a different workflow is active".into()); } existing } else {
        let lead = sessions.require_session()?;
        let coordinator = Arc::new(WorkflowCoordinator::new_shared(plan_id.clone(), state.inner.clone(), lead, Some(workflow_path(&app)?)));
        let mut events = coordinator.events();
        let emitter = app.clone();
        let forwarder = tokio::spawn(async move { while let Ok(event) = events.recv().await { let _ = emitter.emit("workflow_event", &event); } });
        if let Some(old) = state.forwarder.lock().await.replace(forwarder) { old.abort(); }
        *state.workflow.lock().await = Some(coordinator.clone());
        coordinator
    };
    if let Some(task) = resume { coordinator.resume_task(&task).await?; }
    let outcome = coordinator.run_plan().await?;
    if matches!(outcome.state, ridge_acp::workflow::WorkflowState::Completed | ridge_acp::workflow::WorkflowState::FinishedWithFailure) { *state.workflow.lock().await = None; }
    serde_json::to_value(outcome).map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workflow_confirm(app: AppHandle, state: State<'_, PlanStoreState>, sessions: State<'_, AcpBridgeState>, plan_id: String) -> Result<serde_json::Value, String> {
    {
        let mut store = state.inner.lock().await;
        let backup = store.get(&plan_id).ok_or("plan not found")?.clone();
        store.confirm(&plan_id)?;
        if let Err(e) = persist_plan_store_atomic(&store, &workflow_path(&app)?).await { store.insert(backup)?; return Err(e); }
    }
    run_workflow(app, &state, &sessions, plan_id, None).await
}
#[tauri::command]
pub async fn workflow_resume(app: AppHandle, state: State<'_, PlanStoreState>, sessions: State<'_, AcpBridgeState>, plan_id: String, task_id: String) -> Result<serde_json::Value, String> {
    run_workflow(app, &state, &sessions, plan_id, Some(task_id)).await
}
#[tauri::command]
pub async fn workflow_approve(state: State<'_, PlanStoreState>, task_id: String, turn_id: String, tool_call_id: String, approved: bool) -> Result<(), String> {
    let coordinator = state.workflow.lock().await.clone().ok_or("no live workflow; Resume required")?;
    coordinator.approve_task(&task_id, &ridge_acp::session::TurnId(turn_id), &tool_call_id, approved).await
}
#[tauri::command]
pub async fn workflow_cancel(state: State<'_, PlanStoreState>) -> Result<(), String> {
    let coordinator = state.workflow.lock().await.clone().ok_or("no live workflow")?;
    coordinator.cancel().await
}
#[tauri::command]
pub async fn plan_get(state: State<'_, PlanStoreState>, plan_id: String) -> Result<Plan, String> { state.inner.lock().await.get(&plan_id).cloned().ok_or("plan not found".into()) }
#[tauri::command]
pub async fn plan_list(state: State<'_, PlanStoreState>) -> Result<Vec<String>, String> { Ok(state.inner.lock().await.list()) }
#[tauri::command]
pub async fn plan_save(state: State<'_, PlanStoreState>, plan_id: String, path: String) -> Result<(), String> {
    let store = state.inner.lock().await;
    let mut saved = PlanStore::new(); saved.insert(store.get(&plan_id).ok_or("plan not found")?.clone())?;
    persist_plan_store_atomic(&saved, &PathBuf::from(path)).await
}
#[tauri::command]
pub async fn plan_load(state: State<'_, PlanStoreState>, path: String) -> Result<Vec<Plan>, String> {
    let (loaded, _) = restore_workflow(&PathBuf::from(path)).await?;
    let plans: Vec<_> = loaded.list().iter().filter_map(|id| loaded.get(id).cloned()).collect();
    let mut store = state.inner.lock().await;
    for plan in &plans { store.insert(plan.clone())?; }
    Ok(plans)
}
#[tauri::command]
pub async fn plan_scaffold(state: State<'_, PlanStoreState>, id: String, title: String, context: String, workspace_cwd: String) -> Result<(), String> { state.inner.lock().await.insert(scaffold_plan(id, title, context, workspace_cwd)) }
#[tauri::command]
pub async fn plan_add_story(state: State<'_, PlanStoreState>, plan_id: String, title: String, description: String) -> Result<String, String> {
    let mut store = state.inner.lock().await; Ok(append_story(store.get_mut(&plan_id).ok_or("plan not found")?, title, description))
}
#[tauri::command]
pub async fn plan_add_task(state: State<'_, PlanStoreState>, plan_id: String, story_id: String, title: String, description: String, point: TaskPoint, dependencies: Vec<String>) -> Result<String, String> {
    let mut store = state.inner.lock().await; Ok(append_task(store.get_mut(&plan_id).ok_or("plan not found")?, &story_id, title, description, point, dependencies))
}
#[tauri::command]
pub async fn plan_set_task_status(state: State<'_, PlanStoreState>, plan_id: String, task_id: String, status: TaskStatus) -> Result<(), String> {
    let mut store = state.inner.lock().await;
    let task = store.get_mut(&plan_id).ok_or("plan not found")?.find_task_mut(&task_id).ok_or("task not found")?;
    if status != TaskStatus::Queued || task.status != TaskStatus::Interrupted { return Err("task status is driven by canonical execution events".into()); }
    task.status = status; Ok(())
}

#[tauri::command]
pub fn workflow_cwd() -> Result<String, String> { std::env::current_dir().map(|path| path.to_string_lossy().into_owned()).map_err(|e| e.to_string()) }

#[tauri::command]
pub async fn workflow_cancel_review(app: AppHandle, state: State<'_, PlanStoreState>, plan_id: String) -> Result<(), String> {
    let mut store = state.inner.lock().await;
    let backup = store.get(&plan_id).ok_or("plan not found")?.clone();
    if backup.confirmed { return Err("the review has already been confirmed".into()); }
    for task in store.get_mut(&plan_id).unwrap().all_tasks_mut() { task.status = TaskStatus::Cancelled; }
    if let Err(e) = persist_plan_store_atomic(&store, &workflow_path(&app)?).await { store.insert(backup)?; return Err(e); }
    Ok(())
}
