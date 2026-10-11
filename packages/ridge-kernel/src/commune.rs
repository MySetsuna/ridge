//! Generic ACP registration using OS-attested, already-running terminal endpoints.
use crate::{
    registry::{ridge_data_dir, save_roster_at},
    server::AppState,
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use ridge_acp::{existing::ExistingConversation, transport::AcpTransport};
use ridge_core::{
    process_identity::{self, ProcessInstance},
    teammate::communication::{AgentIdentity, AgentLifecycle, ProtocolBinding},
    EventSink,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    io::Write,
    net::{SocketAddr, SocketAddrV4},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisterRequest {
    workspace_id: Uuid,
    pane_id: Uuid,
    process: ProcessInstance,
    endpoint_owner: ProcessInstance,
    endpoint: SocketAddrV4,
    session_id: String,
    cwd: String,
}
fn pane_process(state: &AppState, request: &RegisterRequest) -> Result<ProcessInstance, String> {
    let pane = state
        .ptys
        .info(request.pane_id)
        .map_err(|error| error.to_string())?;
    if pane.workspace_id != Some(request.workspace_id) {
        return Err("pane/workspace binding mismatch".into());
    }
    let root = ProcessInstance {
        pid: pane.child_pid.ok_or("pane process unavailable")?,
        start_instance: pane
            .child_start_instance
            .ok_or("pane process instance unavailable")?,
    };
    if !process_identity::descendant(&root, &request.process).map_err(|error| error.to_string())?
        || !process_identity::descendant(&root, &request.endpoint_owner)
            .map_err(|error| error.to_string())?
    {
        return Err(
            "Agent and protocol endpoint must belong to the exact pane process tree".into(),
        );
    }
    if !process_identity::descendant(&request.endpoint_owner, &request.process)
        .map_err(|error| error.to_string())?
    {
        return Err("endpoint does not own the declared Agent process".into());
    }
    Ok(root)
}

pub(crate) async fn attach(state: &AppState, request: RegisterRequest) -> Result<Value, String> {
    if !request.endpoint.ip().is_loopback()
        || request.endpoint.port() == 0
        || request.session_id.trim().is_empty()
        || !std::path::Path::new(&request.cwd).is_absolute()
    {
        return Err("existing session, absolute cwd and local endpoint are required".into());
    }
    // One control connection per authenticated peer/session. No new Agent or session.
    let mut connections = state.commune.lock().await;
    let root = pane_process(state, &request)?;
    let peer_namespace = format!(
        "os-tcp:{}:{}:{}",
        request.endpoint_owner.pid, request.endpoint_owner.start_instance, request.endpoint
    );
    let existing = state
        .roster
        .lock()
        .map_err(|_| "roster unavailable")?
        .agent_identities()
        .into_iter()
        .find(|identity| {
            identity.session_id == request.session_id
                && identity
                    .protocol_binding
                    .as_ref()
                    .is_some_and(|binding| binding.peer_namespace == peer_namespace)
        })
        .cloned();
    if let Some(identity) = &existing {
        if identity.workspace_id != request.workspace_id.to_string()
            || identity.pane_id != request.pane_id.to_string()
            || identity.protocol_binding.as_ref().is_none_or(|binding| {
                binding.process != request.process
                    || binding.endpoint_owner != request.endpoint_owner
            })
        {
            return Err(
                "peer/session binding is already associated with a different terminal process"
                    .into(),
            );
        }
        if connections
            .get(&identity.agent_id)
            .is_some_and(|session| session.is_connected())
        {
            return Ok(
                json!({"agentId":identity.agent_id,"generation":identity.generation,"lease":identity.lease,"sessionId":identity.session_id,"binding":identity.protocol_binding}),
            );
        }
    }
    let stream = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::TcpStream::connect(request.endpoint),
    )
    .await
    .map_err(|_| "ACP endpoint connection timed out")?
    .map_err(|error| error.to_string())?;
    let SocketAddr::V4(client) = stream.local_addr().map_err(|error| error.to_string())? else {
        return Err("ACP connection address family mismatch".into());
    };
    process_identity::owns_connection(&request.endpoint_owner, request.endpoint, client)
        .map_err(|error| error.to_string())?;
    let agent_id = existing
        .as_ref()
        .map(|identity| identity.agent_id.clone())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let folder = ridge_data_dir().join("commune-history");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let history = Arc::new(std::sync::Mutex::new(
        options
            .open(folder.join(format!("{agent_id}.jsonl")))
            .map_err(|error| error.to_string())?,
    ));
    let (read, write) = stream.into_split();
    let events = state.business_events.clone();
    let event_agent = agent_id.clone();
    let session = ExistingConversation::load(
        AcpTransport::from_io(write, read),
        request.session_id.clone(),
        request.cwd.clone(),
        Arc::new(move |update| {
            let mut history = history
                .lock()
                .map_err(|_| ridge_acp::AcpErrorWrap::Io("history lock unavailable".into()))?;
            writeln!(
                history,
                "{}",
                json!({"agentId":event_agent,"params":update})
            )
            .map_err(|error| ridge_acp::AcpErrorWrap::Io(error.to_string()))?;
            history
                .sync_data()
                .map_err(|error| ridge_acp::AcpErrorWrap::Io(error.to_string()))?;
            events.broadcast(
                "commune-session-update",
                json!({"agentId":event_agent,"params":update}),
            );
            Ok(())
        }),
        Arc::new(|_| Box::pin(async { None })),
    )
    .await
    .map_err(|error| error.to_string())?;
    let session = Arc::new(session);
    let rechecked = pane_process(state, &request);
    if rechecked.as_ref() != Ok(&root)
        || process_identity::validate(&request.process).is_err()
        || process_identity::validate(&request.endpoint_owner).is_err()
    {
        session.disconnect();
        return Err("Agent/pane process instance changed during registration".into());
    }
    let mut roster = state.roster.lock().map_err(|_| "roster unavailable")?;
    let mut next = roster.clone();
    let generation = next.next_agent_generation(&agent_id);
    let lease = Uuid::new_v4().to_string();
    let binding = ProtocolBinding {
        protocol: "acp".into(),
        host_id: state.host_id.clone(),
        runtime_epoch: state.runtime_epoch.clone(),
        process: request.process,
        endpoint_owner: request.endpoint_owner,
        endpoint: request.endpoint.to_string(),
        peer_namespace,
    };
    next.commit_online_agent(AgentIdentity {
        protocol_binding: Some(binding.clone()),
        agent_id: agent_id.clone(),
        session_id: request.session_id.clone(),
        workspace_id: request.workspace_id.to_string(),
        pane_id: request.pane_id.to_string(),
        cwd: request.cwd,
        executable: String::new(),
        argv: Vec::new(),
        generation,
        lease: lease.clone(),
        lifecycle: AgentLifecycle::Online,
        online: true,
        last_seen_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_millis() as u64,
        capabilities: vec![
            "acp.prompt".into(),
            "acp.cancel".into(),
            "acp.history".into(),
        ],
    })
    .map_err(|error| error.to_string())?;
    if let Err(error) = save_roster_at(&state.roster_path, &next) {
        session.disconnect();
        return Err(error.to_string());
    }
    *roster = next;
    connections.insert(agent_id.clone(), session.clone());
    drop(roster);
    tokio::spawn({
        let state = state.clone();
        let agent_id = agent_id.clone();
        let lease = lease.clone();
        let process = binding.process.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let observed = process_identity::instance(process.pid);
                let verified = observed.as_ref().is_ok_and(|current| current == &process);
                let exited = observed.as_ref().is_ok_and(|current| current != &process)
                    || observed
                        .as_ref()
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                if session.is_connected() && verified {
                    continue;
                }
                session.disconnect();
                let changed = {
                    let mut roster = state.roster.lock().expect("roster lock");
                    let mut next = roster.clone();
                    if !next.disconnect_protocol_binding(&agent_id, generation, &lease, exited) {
                        break;
                    }
                    match save_roster_at(&state.roster_path, &next) {
                        Ok(()) => {
                            *roster = next;
                            true
                        }
                        Err(error) => {
                            tracing::error!(%agent_id,%error,"protocol disconnect could not be persisted");
                            false
                        }
                    }
                };
                if changed {
                    state
                        .business_events
                        .broadcast("commune-members-changed", Value::Null);
                    break;
                }
            }
        }
    });
    state
        .business_events
        .broadcast("commune-members-changed", Value::Null);
    Ok(
        json!({"agentId":agent_id,"generation":generation,"lease":lease,"sessionId":request.session_id,"binding":binding}),
    )
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<Value>, StatusCode> {
    if headers
        .get("x-ridge-kernel-token")
        .and_then(|value| value.to_str().ok())
        != Some(state.token.as_str())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(match attach(&state, request).await {
        Ok(value) => json!({"ok":true,"value":value}),
        Err(error) => json!({"ok":false,"error":error}),
    }))
}
