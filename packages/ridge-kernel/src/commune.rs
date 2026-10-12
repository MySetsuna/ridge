//! Generic ACP registration using OS-attested, already-running terminal endpoints.
use crate::{
    commune_permissions::ApprovalBinding,
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
use ridge_mcp::delivery::HubDeliveryAdapter;
use ridge_mcp::server::McpHost;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    net::{SocketAddr, SocketAddrV4},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub fn owns_method(method: &str) -> bool {
    matches!(
        method,
        "register_commune_member"
            | "get_teammate_topology"
            | "list_hitl_pending"
            | "list_hitl_pending_local"
            | "get_pending_hitl_count"
            | "resolve_hitl_request"
            | "resolve_hitl_remote"
            | "send_agent_message"
            | "get_agent_message_delivery_status"
            | "cancel_agent_message"
            | "read_agent_recent_replies"
            | "set_teammate_groups"
    )
}

pub async fn dispatch(
    state: &AppState,
    method: &str,
    args: &Value,
    native: bool,
) -> Result<Value, String> {
    match method {
        "set_teammate_groups" => {
            let state = state.clone();
            let args = args.clone();
            tokio::task::spawn_blocking(move || crate::commune_groups::replace(&state, &args, native))
                .await.map_err(|e| e.to_string())?
        }
        "read_agent_recent_replies" => {
            let state = state.clone();
            let args = args.clone();
            tokio::task::spawn_blocking(move || crate::commune_history::read(&state, &args, native))
                .await
                .map_err(|e| e.to_string())?
        }
        "register_commune_member" => {
            if !native {
                return Err("only a native trusted client may register an existing process".into());
            }
            attach(
                state,
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?,
            )
            .await
        }
        "get_teammate_topology" => {
            let mut profile = crate::kernel_mcp::KernelMcpHost::new(state.clone())
                .team_profile_for(args["workspaceId"].as_str())
                .map_err(|error| match error {
                    ridge_mcp::server::HostError::InvalidParams(message)
                    | ridge_mcp::server::HostError::Internal(message)
                    | ridge_mcp::server::HostError::Unsupported(message) => message,
                })?;
            let connections = state.commune.lock().await;
            let workspace = args["workspaceId"].as_str().ok_or("topology requires workspaceId")?;
            let groups = crate::commune_groups::projection(state, workspace)?;
            profile["groups"] = groups["groups"].clone();
            profile["groupsRevision"] = groups["groupsRevision"].clone();
            profile["groupTasks"] = crate::commune_tasks::list(state, workspace)?;
            if let Some(roster) = profile["roster"].as_array_mut() {
                roster.retain(|row| row["protocolBinding"].is_object());
                for row in roster {
                    let id = row["agentId"]
                        .as_str()
                        .ok_or("member has no ID")?
                        .to_owned();
                    let session = connections.get(&id);
                    let connected = row["online"] == true
                        && session.is_some_and(|session| session.is_connected());
                    let working = connected && session.is_some_and(|session| session.is_busy());
                    row["online"] = json!(connected);
                    row["canSend"] = json!(native);
                    row["canCancel"] = json!(native);
                    row["status"] = json!(if !connected {
                        "Disappeared"
                    } else if working {
                        "Working"
                    } else {
                        "Idle"
                    });
                    row["activity"] = json!(if working { "working" } else { "idle" });
                    if let Some(info) = session.and_then(|session| session.agent_info()) {
                        row["agentInfo"] = info.clone();
                        row["name"] = info["title"]
                            .as_str()
                            .or(info["name"].as_str())
                            .map(|name| json!(name))
                            .unwrap_or_else(|| json!(id));
                    }
                    row["currentDelivery"] = state
                        .mcp_state
                        .latest_runtime_receipt(
                            &id,
                            row["generation"]
                                .as_u64()
                                .ok_or("member generation missing")?,
                            row["lease"].as_str().ok_or("member lease missing")?,
                        )?
                        .unwrap_or(Value::Null);
                }
            }
            Ok(profile)
        }
        "list_hitl_pending" => state.permissions.snapshot(false),
        "list_hitl_pending_local" => {
            if !native {
                return Err("local permission details require native trust".into());
            }
            state.permissions.snapshot(true)
        }
        "get_pending_hitl_count" => Ok(json!(state
            .permissions
            .snapshot(false)?
            .as_array()
            .ok_or("invalid permission snapshot")?
            .len())),
        "resolve_hitl_request" | "resolve_hitl_remote" => {
            if method == "resolve_hitl_request" && !native {
                return Err("local permission resolution requires native trust".into());
            }
            if !args["replacement"].is_null() {
                return Err("ACP permission requests cannot be rewritten".into());
            }
            let id = args["id"].as_str().ok_or("missing permission ID")?;
            let Some(binding) = state.permissions.binding(id) else {
                return Ok(if native && method == "resolve_hitl_request" {
                    json!(false)
                } else {
                    json!({"outcome":"already-resolved"})
                });
            };
            for (key, expected) in [
                ("workspaceId", json!(binding.workspace_id)),
                ("agentId", json!(binding.agent_id)),
                ("generation", json!(binding.generation)),
                ("lease", json!(binding.lease)),
            ] {
                if !args[key].is_null() && args[key] != expected {
                    return Err("permission binding mismatch".into());
                }
            }
            if !args["optionId"].is_null() && !args["optionId"].is_string() {
                return Err("invalid permission option ID".into());
            }
            let session = state.commune.lock().await.get(&binding.agent_id).cloned();
            let roster = state.roster.lock().map_err(|_| "roster unavailable")?;
            let live = roster.agent_identities().into_iter().any(|member| {
                member.online
                    && member.agent_id == binding.agent_id
                    && member.workspace_id == binding.workspace_id
                    && member.session_id == binding.session_id
                    && member.generation == binding.generation
                    && member.lease == binding.lease
                    && member.protocol_binding.as_ref().is_some_and(|peer| {
                        process_identity::validate(&peer.process).is_ok()
                            && process_identity::validate(&peer.endpoint_owner).is_ok()
                    })
            });
            if !live || !session.is_some_and(|session| session.is_connected()) {
                return Err("permission binding no longer live".into());
            }
            let outcome = state.permissions.resolve(
                id,
                args["optionId"].as_str(),
                args["verdict"].as_str(),
                if method == "resolve_hitl_remote" {
                    Some(
                        args["nonce"]
                            .as_str()
                            .ok_or("missing remote permission nonce")?,
                    )
                } else {
                    None
                },
            )?;
            drop(roster);
            state
                .business_events
                .broadcast("teammate://hitl-pending-changed", Value::Null);
            if method == "resolve_hitl_request" {
                if outcome == "bad-verdict" {
                    return Err("select an option actually offered by the Agent".into());
                }
                Ok(json!(outcome == "consumed"))
            } else {
                Ok(json!({"outcome":outcome}))
            }
        }
        "send_agent_message" | "get_agent_message_delivery_status" | "cancel_agent_message" => {
            if args.get("groupAction").is_some() || args.get("groupTaskId").is_some() {
                let state = state.clone(); let args = args.clone(); let method = method.to_owned();
                return tokio::task::spawn_blocking(move || match method.as_str() {
                    "send_agent_message" => crate::commune_tasks::send(&state,&args),
                    "cancel_agent_message" => crate::commune_tasks::cancel(&state,&args),
                    _ => crate::commune_tasks::status(&state,&args),
                }).await.map_err(|e| e.to_string())?;
            }
            let grouped = method == "send_agent_message" && args.get("groupId").is_some();
            let tool = match method {
                "send_agent_message" if grouped => "ridge_create_task",
                "send_agent_message" => "ridge_send_message",
                "cancel_agent_message" => "ridge_cancel_delivery",
                _ => "ridge_delivery_status",
            };
            let mut request = json!({"workspace_id":args["workspaceId"],"agent_id":args["agentId"],"generation":args["generation"],"lease":args["lease"]});
            if args["agentId"]
                .as_str()
                .filter(|value| !value.is_empty())
                .is_none()
                || args["workspaceId"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .is_none()
            {
                return Err("message request requires exact member and workspace IDs".into());
            }
            if args["generation"]
                .as_u64()
                .filter(|value| *value > 0)
                .is_none()
                || args["lease"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .is_none()
            {
                return Err("message request requires an exact generation and lease".into());
            }
            if method == "send_agent_message" && !args["paneId"].is_null() {
                request["target_pane_id"] = args["paneId"].clone();
            }
            if method == "send_agent_message" {
                request[if grouped {"objective"} else {"message"}] = args["message"].clone();
                if grouped {
                    request["group_id"] = args["groupId"].clone();
                    request["group_revision"] = args["groupRevision"].clone();
                }
                request["idempotency_key"] = args["idempotency_key"].clone();
                request["from"] = json!("user-client");
            } else {
                request["delivery_id"] = args["deliveryId"].clone();
            }
            let state = state.clone();
            tokio::task::spawn_blocking(move || {
                let host = crate::kernel_mcp::KernelMcpHost::new(state.clone());
                let reply =
                    ridge_mcp::server::call_tool_rpc(tool, request, &host, &state.mcp_state);
                let content = reply["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or_else(|| format!("invalid Hub result: {}", reply["error"]))?;
                if reply["result"]["isError"] == true {
                    return Err(content.to_owned());
                }
                serde_json::from_str(content).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
        }
        _ => Err("unsupported Commune method".into()),
    }
}

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
    let history = state
        .permissions
        .history(&agent_id, &folder.join(format!("{agent_id}.jsonl")))?;
    let generation = state
        .roster
        .lock()
        .map_err(|_| "roster unavailable")?
        .next_agent_generation(&agent_id);
    let lease = Uuid::new_v4().to_string();
    let approval = ApprovalBinding {
        agent_id: agent_id.clone(),
        workspace_id: request.workspace_id.to_string(),
        pane_id: request.pane_id.to_string(),
        session_id: request.session_id.clone(),
        generation,
        lease: lease.clone(),
    };
    let permission_log = history.clone();
    let permissions = state.permissions.clone();
    let permission_roster = state.roster.clone();
    let permission_events = state.business_events.clone();
    let permission_hub = state.mcp_state.clone();
    let replay_id = Uuid::new_v4().to_string();
    history.append(json!({"kind":"replay_begin","agentId":agent_id,"sessionId":request.session_id,"replayId":replay_id,"generation":generation}))?;
    let replaying = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let update_phase = replaying.clone();
    let update_replay = replay_id.clone();
    let connection_history = history.clone();
    let (read, write) = stream.into_split();
    let events = state.business_events.clone();
    let event_agent = agent_id.clone();
    let session = ExistingConversation::load(
        AcpTransport::from_io(write, read),
        request.session_id.clone(),
        request.cwd.clone(),
        Arc::new(move |update| {
            let record = history
                .append(json!({"kind":"session_update","agentId":event_agent,"params":update,"generation":generation,
                    "phase":if update_phase.load(std::sync::atomic::Ordering::Acquire) {"replay"}else{"live"},"replayId":update_replay}))
                .map_err(ridge_acp::AcpErrorWrap::Io)?;
            events.broadcast("commune-session-update", record);
            Ok(())
        }),
        Arc::new(move |params| {
            let permissions = permissions.clone();
            let approval = approval.clone();
            let roster = permission_roster.clone();
            let log = permission_log.clone();
            let events = permission_events.clone();
            let hub = permission_hub.clone();
            Box::pin(async move {
                let live = roster.lock().is_ok_and(|roster| {
                    roster.agent_identities().into_iter().any(|member| {
                        member.online
                            && member.agent_id == approval.agent_id
                            && member.session_id == approval.session_id
                            && member.generation == approval.generation
                            && member.lease == approval.lease
                    })
                });
                if !live {
                    return None;
                }
                let context = match hub.latest_runtime_receipt(&approval.agent_id,approval.generation,&approval.lease) {
                    Ok(Some(receipt)) if receipt["execution"]["state"] == "running" => json!({"deliveryId":receipt["deliveryId"],"taskId":receipt["taskId"],"groupTaskId":receipt["payload"]["groupContext"]["groupTaskId"]}),
                    _ => Value::Null,
                };
                permissions.request(approval, params, log, events, context).await
            })
        }),
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
    if let Err(error) = connection_history.append(json!({"kind":if session.history_replayed(){"replay_committed"}else{"session_reconnected"},"agentId":agent_id,"sessionId":request.session_id,"replayId":replay_id,"generation":generation,"agentInfo":session.agent_info()})) {
        session.disconnect();
        return Err(error);
    }
    replaying.store(false, std::sync::atomic::Ordering::Release);
    let mut roster = state.roster.lock().map_err(|_| "roster unavailable")?;
    let mut next = roster.clone();
    if next.next_agent_generation(&agent_id) != generation {
        session.disconnect();
        return Err("Agent generation changed during registration".into());
    }
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
        capabilities: {
            let mut capabilities = vec![
                "messages".into(),
                "tasks".into(),
                "acp.prompt".into(),
                "acp.cancel".into(),
            ];
            if session.history_replayed() {
                capabilities.push("acp.history".into());
            }
            capabilities
        },
    })
    .map_err(|error| error.to_string())?;
    let receiver = state.mcp_state.register_delivery_endpoint(
        HubDeliveryAdapter::RuntimeApi,
        request.workspace_id.to_string(),
        agent_id.clone(),
        generation,
        lease.clone(),
    )?;
    if let Err(error) = save_roster_at(&state.roster_path, &next) {
        let _ = state.mcp_state.unregister_delivery_endpoint(
            HubDeliveryAdapter::RuntimeApi,
            &request.workspace_id.to_string(),
            &agent_id,
            generation,
            &lease,
        );
        session.disconnect();
        return Err(error.to_string());
    }
    *roster = next;
    connections.insert(agent_id.clone(), session.clone());
    drop(roster);
    tokio::spawn(consume_deliveries(
        state.clone(),
        session.clone(),
        receiver,
        agent_id.clone(),
        generation,
        lease.clone(),
        request.workspace_id.to_string(),
        connection_history.clone(),
    ));
    tokio::spawn({
        let state = state.clone();
        let agent_id = agent_id.clone();
        let lease = lease.clone();
        let process = binding.process.clone();
        let history = connection_history;
        async move {
            let mut disconnected = false;
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if disconnected {
                    let roster = state.roster.lock().expect("roster lock");
                    if !roster.agent_identities().into_iter().any(|member| {
                        member.agent_id == agent_id
                            && member.generation == generation
                            && member.lease == lease
                    }) {
                        break;
                    }
                }
                let observed = process_identity::instance(process.pid);
                let verified = observed.as_ref().is_ok_and(|current| current == &process);
                let exited = observed.as_ref().is_ok_and(|current| current != &process)
                    || observed
                        .as_ref()
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                if (session.is_connected() && verified) || (disconnected && !exited) {
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
                    if let Err(error) = history.append(json!({"kind":if exited {"process_exited"}else{"connection_lost"},"agentId":agent_id,"generation":generation})) {
                        tracing::error!(%agent_id,%error,"connection coverage gap could not be persisted");
                    }
                    state
                        .business_events
                        .broadcast("commune-members-changed", Value::Null);
                    disconnected = true;
                    if exited {
                        break;
                    }
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

/// One bounded Hub receiver per exact conversation; one prompt at a time.
/// No retry after a transport error: the Agent may already have executed it.
async fn consume_deliveries(
    state: AppState,
    session: Arc<ExistingConversation>,
    receiver: std::sync::mpsc::Receiver<Value>,
    agent_id: String,
    generation: u64,
    lease: String,
    workspace: String,
    history: Arc<crate::commune_permissions::HistoryLog>,
) {
    loop {
        let entry = match receiver.try_recv() {
            Ok(entry) => entry,
            Err(std::sync::mpsc::TryRecvError::Empty) if session.is_connected() => {
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
            _ => break,
        };
        let Some(id) = entry["deliveryId"].as_str() else {
            continue;
        };
        let publish = |execution: Value| -> Result<Value, String> {
            let receipt = state
                .mcp_state
                .record_runtime_execution(&agent_id, generation, &lease, id, execution)?;
            history.append(json!({"kind":"execution","agentId":agent_id,"sessionId":receipt["to"]["sessionId"],"deliveryId":id,
                "payload":receipt["payload"],"execution":receipt["execution"],"taskId":receipt["taskId"],"correlationId":receipt["correlationId"],"causationId":receipt["causationId"]}))?;
            state
                .business_events
                .broadcast("commune-task-changed", receipt.clone());
            Ok(receipt)
        };
        let live = {
            let roster = state.roster.lock().expect("roster lock");
            roster
                .agent_identities()
                .into_iter()
                .find(|member| member.agent_id == agent_id)
                .filter(|member| {
                    member.online && member.generation == generation && member.lease == lease
                })
                .and_then(|member| member.protocol_binding.clone())
        };
        let verified = live.as_ref().is_some_and(|binding| {
            binding.runtime_epoch == state.runtime_epoch
                && process_identity::validate(&binding.process).is_ok()
                && process_identity::validate(&binding.endpoint_owner).is_ok()
        });
        if !verified {
            if let Err(error) =
                publish(json!({"state":"rejected","reason":"Agent binding changed before prompt"}))
            {
                tracing::error!(%id, %error, "cannot persist rejected protocol delivery");
            }
            continue;
        }
        let receipt = match publish(json!({"state":"dispatching"})) {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::error!(%id, %error, "protocol prompt refused without durable execution receipt");
                session.disconnect();
                break;
            }
        };
        let cancelled =
            receipt["cancellationRequested"] == true || receipt["status"] == "cancelled";
        let expired = receipt["status"] == "expired"
            || receipt["deadlineUnixMs"].as_u64().is_some_and(|deadline| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(true, |now| now.as_millis() >= deadline as u128)
            });
        let text = receipt["payload"]["text"]
            .as_str()
            .or_else(|| receipt["payload"]["objective"].as_str());
        let execution = if cancelled {
            json!({"state":"cancelled","sent":false})
        } else if expired {
            json!({"state":"rejected","reason":"delivery deadline elapsed","sent":false})
        } else if let Some(text) = text {
            match session.prompt(text.to_owned()) {
                Err(error) => json!({"state":"rejected","reason":error.to_string(),"sent":false}),
                Ok(mut pending) => {
                    if let Err(error) = publish(json!({"state":"running"})) {
                        tracing::error!(%id, %error, "protocol dispatch outcome cannot be tracked durably");
                        session.disconnect();
                    }
                    let mut cancel_sent = false;
                    let response = loop {
                        tokio::select! {
                            response = &mut pending => break response,
                            _ = tokio::time::sleep(Duration::from_millis(20)), if !cancel_sent => {
                                match state.mcp_state.runtime_receipt(&agent_id, generation, &lease, id) {
                                    Ok(receipt) if receipt["cancellationRequested"] == true => {
                                        // A cancel notification is not a terminal response.
                                        if let Err(error) = session.cancel().await {
                                            tracing::error!(%id, %error, "protocol cancellation result unknown");
                                        }
                                        cancel_sent = true;
                                    }
                                    Err(error) => {
                                        tracing::error!(%id, %error, "protocol receipt lost during execution");
                                        session.disconnect();
                                    }
                                    _ => {},
                                }
                            }
                        }
                    };
                    match response {
                        Ok(Ok(result)) => {
                            json!({"state":if result["stopReason"] == "cancelled" {"cancelled"} else {"turn_ended"},"response":result})
                        }
                        Ok(Err(error)) => json!({"state":"unknown","reason":error.to_string()}),
                        Err(error) => json!({"state":"unknown","reason":error.to_string()}),
                    }
                }
            }
        } else {
            json!({"state":"rejected","reason":"ACP prompt requires text","sent":false})
        };
        if let Err(error) = publish(execution) {
            tracing::error!(%id, %error, "protocol result could not be persisted");
            session.disconnect();
            break;
        }
    }
    if let Err(error) = state.mcp_state.unregister_delivery_endpoint(
        HubDeliveryAdapter::RuntimeApi,
        &workspace,
        &agent_id,
        generation,
        &lease,
    ) {
        tracing::warn!(%agent_id, %error, "protocol delivery route already superseded");
    }
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
