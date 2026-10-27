//! Frozen parent/child relations; execution and receipts remain owned by the Hub.
use crate::{commune_groups::Group, server::AppState};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ridge_core::EventSink;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetRequest {
    agent_id: String,
    generation: u64,
    lease: String,
    #[serde(default)]
    objective: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group_scope: Option<String>,
    group_id: String,
    group_revision: u64,
    group_action: String,
    #[serde(default)]
    message: String,
    targets: Vec<TargetRequest>,
    #[serde(rename = "idempotency_key")]
    idempotency_key: String,
}
impl Request {
    fn scope(&self) -> &str { if self.group_scope.as_deref()==Some("host") {crate::commune_groups::HOST_GROUP_SCOPE} else {&self.workspace_id} }
}
fn scope(args:&Value)->Result<&str,String> {
    if args["groupScope"]=="host" {
        if !args["workspaceId"].is_null(){return Err("Host group task cannot have a workspace owner".into());}
        Ok(crate::commune_groups::HOST_GROUP_SCOPE)
    } else if !args["groupScope"].is_null(){Err("unsupported group scope".into())}
    else {let workspace=args["workspaceId"].as_str().ok_or("missing workspaceId or Host group scope")?;Uuid::parse_str(workspace).map_err(|_|"invalid workspaceId")?;Ok(workspace)}
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Child {
    pub key: String,
    pub binding: Value,
    pub objective: String,
    pub submission: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dispatch {
    pub id: String,
    pub request: Request,
    pub group: Group,
    pub runtime_epoch: String,
    pub created_at: u64,
    pub children: Vec<Child>,
    pub cancellation_requested: bool,
    pub coordinating: bool,
}
fn receipt(state: &AppState, child: &Child) -> Result<Option<Value>, String> {
    state
        .mcp_state
        .receipt_for_key(&child.binding, "user-client", &child.key)
}
fn child_state(
    state: &AppState,
    task: &Dispatch,
    child: &Child,
    receipt: Option<&Value>,
) -> Result<String, String> {
    let Some(receipt) = receipt else {
        return Ok(if child.submission == "prepared"
            && (task.runtime_epoch != state.runtime_epoch || !task.coordinating)
        {
            "unknown"
        } else {
            &child.submission
        }
        .into());
    };
    let execution = receipt["execution"]["state"].as_str();
    if execution == Some("running") && state.permissions.is_waiting(&child.binding)? {
        let latest = state.mcp_state.latest_runtime_receipt(
            child.binding["agentId"]
                .as_str()
                .ok_or("missing member ID")?,
            child.binding["generation"]
                .as_u64()
                .ok_or("missing generation")?,
            child.binding["lease"].as_str().ok_or("missing lease")?,
        )?;
        if latest
            .as_ref()
            .is_some_and(|latest| latest["deliveryId"] == receipt["deliveryId"])
        {
            return Ok("waiting_permission".into());
        }
    }
    Ok(match execution {
        Some("turn_ended") => "reply_ended",
        Some(other) => other,
        None => match receipt["status"].as_str() {
            Some("delivery_rejected") => "rejected",
            Some("delivery_failed") => "delivery_failed",
            Some("cancelled") => "cancelled",
            Some("expired") => "expired",
            Some("failed") => "reported_failed",
            Some("completed") => "reported_completed",
            _ => "queued",
        },
    }
    .into())
}
pub fn project(state: &AppState, task: &Dispatch, native: bool) -> Result<Value, String> {
    let mut children = Vec::new();
    let mut counts = serde_json::Map::new();
    for child in &task.children {
        let receipt = receipt(state, child)?;
        let status = child_state(state, task, child, receipt.as_ref())?;
        let count = counts.entry(status.clone()).or_insert(json!(0));
        *count = json!(count.as_u64().unwrap_or(0) + 1);
        let outputs = if let Some(delivery)=receipt.as_ref().and_then(|receipt|receipt["deliveryId"].as_str()) {
            state.mcp_state.protocol_outputs(delivery,native)?
        } else { json!({"items":[],"hasMore":false}) };
        children.push(json!({"member":child.binding,"objective":child.objective.chars().take(512).collect::<String>(),"objectiveTruncated":child.objective.chars().count()>512,"state":status,"submissionError":child.error,
            "deliveryError":receipt.as_ref().map(|receipt| &receipt["deliveryError"]),
            "deliveryId":receipt.as_ref().map(|receipt| &receipt["deliveryId"]),"taskId":receipt.as_ref().map(|receipt| &receipt["taskId"]),
            "execution":receipt.as_ref().map(|receipt| json!({"state":receipt["execution"]["state"],"stopReason":receipt["execution"]["response"]["stopReason"]})),"cancellationRequested":receipt.as_ref().map(|receipt| &receipt["cancellationRequested"]),
            "artifacts":outputs["items"],"artifactsHasMore":outputs["hasMore"],"artifactCoverage":"协议报告的内容；仅关联观察区间，未核验目标完成。无内容时覆盖未知。"}));
    }
    Ok(
        json!({"groupTaskId":task.id,"workspaceId":if task.request.workspace_id.is_empty(){Value::Null}else{json!(task.request.workspace_id)},"groupScope":task.request.group_scope,"groupId":task.group.id,
        "groupRevision":task.group.revision,"groupName":task.group.name,"action":task.request.group_action,
        "createdAt":task.created_at,"cancellationRequested":task.cancellation_requested,"children":children,"counts":counts,
        "objectiveVerified":false,"source":"Runtime frozen dispatch + actual Hub receipts"}),
    )
}
pub fn list(state: &AppState, workspace: &str, native: bool) -> Result<Value, String> {
    state
        .mcp_state
        .group_tasks(workspace, 10)?
        .into_iter()
        .map(|record| {
            let dispatch = serde_json::from_value::<Dispatch>(record).map_err(|e| e.to_string())?;
            project(state, &dispatch, native)
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|tasks| json!(tasks))
}
fn task(state: &AppState, workspace: &str, id: &str) -> Result<Dispatch, String> {
    serde_json::from_value(
        state
            .mcp_state
            .group_task(workspace, id, false)?
            .ok_or("group task not found in workspace")?,
    )
    .map_err(|e| e.to_string())
}
fn update(
    state: &AppState,
    workspace: &str,
    id: &str,
    mutate: impl FnOnce(&mut Dispatch),
) -> Result<Dispatch, String> {
    let record = state.mcp_state.update_group_task(workspace, id, |record| {
        let mut dispatch =
            serde_json::from_value::<Dispatch>(record.clone()).map_err(|e| e.to_string())?;
        mutate(&mut dispatch);
        *record = serde_json::to_value(dispatch).map_err(|e| e.to_string())?;
        Ok(())
    })?;
    serde_json::from_value(record).map_err(|e| e.to_string())
}
struct Coordinating<'a> {
    state: &'a AppState,
    workspace: String,
    id: String,
    finished: bool,
}
impl Coordinating<'_> {
    fn finish(mut self) -> Result<(), String> {
        update(self.state, &self.workspace, &self.id, |task| {
            task.coordinating = false
        })?;
        self.finished = true;
        Ok(())
    }
}
impl Drop for Coordinating<'_> {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = update(self.state, &self.workspace, &self.id, |task| {
                task.coordinating = false
            }) {
                tracing::error!(%error,task_id=%self.id,"group coordinator outcome unconfirmed; frozen relation retained");
            }
        }
    }
}
pub fn status(state: &AppState, args: &Value) -> Result<Value, String> {
    let task = task(
        state,
        scope(args)?,
        args["groupTaskId"].as_str().ok_or("missing groupTaskId")?,
    )?;
    let mut result = project(state, &task, true)?;
    let detail = if let Some(cursor) = args["objectiveCursor"].as_str() {
        if cursor.len() > 8192 {
            return Err("objective cursor too large".into());
        }
        let decoded = URL_SAFE_NO_PAD
            .decode(
                cursor
                    .strip_prefix("tasktext1:")
                    .ok_or("invalid objective cursor")?,
            )
            .map_err(|_| "invalid objective cursor")?;
        let cursor: Value =
            serde_json::from_slice(&decoded).map_err(|_| "invalid objective cursor")?;
        if cursor["workspaceId"] != task.request.scope() || cursor["groupTaskId"] != task.id {
            return Err("objective cursor scope mismatch".into());
        }
        Some((
            cursor["memberId"]
                .as_str()
                .ok_or("missing objective member")?
                .to_string(),
            cursor["offset"]
                .as_u64()
                .and_then(|offset| usize::try_from(offset).ok())
                .ok_or("invalid objective offset")?,
        ))
    } else {
        args["objectiveMemberId"]
            .as_str()
            .map(|member| (member.to_string(), 0))
    };
    if let Some((member, offset)) = detail {
        let child = task
            .children
            .iter()
            .find(|child| child.binding["agentId"] == member)
            .ok_or("objective member not in task")?;
        let length = child.objective.chars().count();
        if offset > length {
            return Err("invalid objective offset".into());
        }
        let cursor = |offset| {
            format!("tasktext1:{}",URL_SAFE_NO_PAD.encode(json!({"workspaceId":task.request.scope(),"groupTaskId":task.id,"memberId":member,"offset":offset}).to_string()))
        };
        result["objectiveDetail"] = json!({"memberId":member,"text":child.objective.chars().skip(offset).take(4096).collect::<String>(),"offset":offset,"length":length,
            "next":if offset+4096<length {Some(cursor(offset+4096))}else{None},"previous":if offset>0 {Some(cursor(offset.saturating_sub(4096)))}else{None}});
    }
    Ok(result)
}
struct SubmissionError { message: String, refused: bool }
fn call(state: &AppState, method: &str, args: Value) -> Result<Value, SubmissionError> {
    let unknown = |message| SubmissionError {message,refused:false};
    let host = crate::kernel_mcp::KernelMcpHost::new(state.clone());
    let reply = ridge_mcp::server::call_tool_rpc(method, args, &host, &state.mcp_state);
    if let Some(message) = reply["error"]["message"].as_str() {
        return Err(SubmissionError {message:message.into(),refused:reply["error"]["code"] == -32602});
    }
    let text = reply["result"]["content"][0]["text"].as_str().ok_or_else(|| unknown(format!("invalid Hub reply: {}",reply["error"])))?;
    if reply["result"]["isError"] == true { return Err(unknown(text.into())); }
    serde_json::from_str(text).map_err(|e| unknown(e.to_string()))
}
fn cancel_child(state: &AppState, child: &Child) -> Result<(), String> {
    let Some(receipt) = receipt(state, child)? else {
        return Ok(());
    };
    if matches!(
        receipt["execution"]["state"].as_str(),
        Some("turn_ended" | "cancelled" | "rejected" | "unknown")
    ) {
        return Ok(());
    }
    call(
        state,
        "ridge_cancel_delivery",
        json!({"workspace_id":child.binding["workspaceId"],"agent_id":child.binding["agentId"],
        "generation":child.binding["generation"],"lease":child.binding["lease"],"delivery_id":receipt["deliveryId"]}),
    ).map_err(|error| error.message)?;
    Ok(())
}
pub fn cancel(state: &AppState, args: &Value) -> Result<Value, String> {
    let workspace = scope(args)?;
    let id = args["groupTaskId"].as_str().ok_or("missing groupTaskId")?;
    let task = update(state, workspace, id, |task| {
        task.cancellation_requested = true
    })?;
    let errors = task
        .children
        .iter()
        .filter_map(|child| {
            cancel_child(state, child)
                .err()
                .map(|error| json!({"memberId":child.binding["agentId"],"error":error}))
        })
        .collect::<Vec<_>>();
    state.business_events.broadcast(
        "commune-task-changed",
        json!({"workspaceId":workspace,"groupTaskId":id}),
    );
    let mut projection = project(state, &task, true)?;
    projection["cancellationErrors"] = json!(errors);
    Ok(projection)
}
fn valid_objective(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 64 * 1024 {
        return Err("objective must contain 1–65536 bytes".into());
    }
    Ok(())
}
pub fn send(state: &AppState, args: &Value) -> Result<Value, String> {
    let request: Request = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    if !matches!(request.group_action.as_str(), "broadcast" | "assign")
        || request.targets.is_empty()
        || request.targets.len() > 32
    {
        return Err("select 1–32 members and an explicit broadcast/assign action".into());
    }
    if request.idempotency_key.is_empty() || request.idempotency_key.len() > 128 {
        return Err("invalid group idempotency key".into());
    }
    if request.group_action == "broadcast" {
        valid_objective(&request.message)?;
    }
    let host_scope=request.group_scope.as_deref()==Some("host");
    if request.group_scope.as_deref().is_some_and(|scope|scope!="host") || (host_scope && !request.workspace_id.is_empty()) {return Err("invalid group scope".into());}
    let id = if host_scope {None} else {Some(Uuid::parse_str(&request.workspace_id).map_err(|_| "invalid workspaceId")?)};
    let retired = state
        .retired_workspaces
        .lock()
        .map_err(|_| "workspace lifecycle unavailable")?;
    if let Some(id)=id {
        if retired.contains(&id) || state.workspaces.lock().map_err(|_|"workspace state unavailable")?.workspace(id).is_none() {return Err("workspace not found".into());}
    }
    let identities = state
        .roster
        .lock()
        .map_err(|_| "roster unavailable")?
        .agent_identities()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let store = state
        .groups
        .lock()
        .map_err(|_| "group authority unavailable")?;
    if let Some(record) =
        state
            .mcp_state
            .group_task(request.scope(), &request.idempotency_key, true)?
    {
        let previous = serde_json::from_value::<Dispatch>(record).map_err(|e| e.to_string())?;
        if previous.request != request {
            return Err(
                "group idempotency conflict; accepted targets/objectives are immutable".into(),
            );
        }
        drop(store);
        drop(retired);
        return project(state, &previous, true);
    }
    if !host_scope && store.host_authoritative() {return Err("legacy group scope cannot create new tasks; use Host-global groups".into());}
    let group = store
        .snapshot(request.scope())
        .groups
        .into_iter()
        .find(|group| group.id == request.group_id)
        .ok_or("group not found in workspace")?;
    if group.revision != request.group_revision {
        return Err("group revision changed; refresh before dispatch".into());
    }
    let mut seen = HashSet::new();
    let mut children = Vec::new();
    for requested in &request.targets {
        if !group.member_agent_ids.contains(&requested.agent_id)
            || !seen.insert(&requested.agent_id)
        {
            return Err("targets must be unique exact members of this group".into());
        }
        let member = identities
            .iter()
            .find(|member| {
                member.agent_id == requested.agent_id
                    && (host_scope || member.workspace_id == request.workspace_id)
                    && Uuid::parse_str(&member.workspace_id).is_ok_and(|id|!retired.contains(&id))
                    && member.protocol_binding.is_some()
            })
            .ok_or("registered member not found in workspace")?;
        if member.generation != requested.generation || member.lease != requested.lease {
            return Err("member binding changed; refresh before dispatch".into());
        }
        let objective = if request.group_action == "broadcast" {
            request.message.clone()
        } else {
            requested
                .objective
                .clone()
                .ok_or("each assigned member requires its approved objective")?
        };
        if request.group_action == "broadcast" && requested.objective.is_some() {
            return Err("broadcast cannot silently apply member-specific objectives".into());
        }
        valid_objective(&objective)?;
        children.push(Child {key:Uuid::new_v4().to_string(),binding:json!({"workspaceId":member.workspace_id,"agentId":member.agent_id,"paneId":member.pane_id,"sessionId":member.session_id,"generation":member.generation,"lease":member.lease}),objective,submission:"prepared".into(),error:None});
    }
    let dispatch = Dispatch {
        id: Uuid::new_v4().to_string(),
        group,
        children,
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as u64,
        runtime_epoch: state.runtime_epoch.clone(),
        request,
        cancellation_requested: false,
        coordinating: true,
    };
    let (record, created) = state.mcp_state.prepare_group_task(
        dispatch.request.scope(),
        &dispatch.request.idempotency_key,
        serde_json::to_value(&dispatch).map_err(|e| e.to_string())?,
    )?;
    drop(store);
    drop(retired);
    if !created {
        let previous = serde_json::from_value::<Dispatch>(record).map_err(|e| e.to_string())?;
        if previous.request != dispatch.request {
            return Err("group idempotency conflict".into());
        }
        return project(state, &previous, true);
    }
    let coordinator = Coordinating {
        state,
        workspace: dispatch.request.scope().to_owned(),
        id: dispatch.id.clone(),
        finished: false,
    };
    for child in &dispatch.children {
        let current = task(state, dispatch.request.scope(), &dispatch.id)?;
        let outcome = if current.cancellation_requested {
            Ok(None)
        } else {
            call(state,"ridge_create_task",json!({"workspace_id":child.binding["workspaceId"],"agent_id":child.binding["agentId"],"generation":child.binding["generation"],"lease":child.binding["lease"],
                "objective":child.objective,"idempotency_key":child.key,"from":"user-client","group_id":dispatch.group.id,"group_revision":dispatch.group.revision,"group_task_id":dispatch.id,"correlation_id":dispatch.id})).map(Some)
        };
        let (submission, error) = match outcome {
            Ok(Some(_)) => ("submitted", None),
            Ok(None) => ("cancelled_before_dispatch", None),
            Err(error) => (if error.refused {"rejected"} else {"unknown"},Some(error.message)),
        };
        let current = update(
            state,
            dispatch.request.scope(),
            &dispatch.id,
            |task| {
                if let Some(target) = task
                    .children
                    .iter_mut()
                    .find(|target| target.key == child.key)
                {
                    target.submission = submission.into();
                    target.error = error;
                }
            },
        )?;
        if current.cancellation_requested {
            if let Err(error) = cancel_child(state, child) {
                tracing::warn!(%error,task_id=%dispatch.id,"accepted group cancellation needs per-child inspection");
            }
        }
    }
    coordinator.finish()?;
    state.business_events.broadcast(
        "commune-task-changed",
        json!({"workspaceId":dispatch.request.workspace_id,"groupTaskId":dispatch.id}),
    );
    project(
        state,
        &task(state, dispatch.request.scope(), &dispatch.id)?,
        true,
    )
}

pub fn frozen_context(
    state: &AppState,
    group: &str,
    revision: u64,
    binding: &Value,
    args: &Value,
) -> Result<Value, String> {
    let task_id=args["group_task_id"].as_str().ok_or("missing group_task_id")?;
    let dispatch=task(state,crate::commune_groups::HOST_GROUP_SCOPE,task_id)
        .or_else(|_|task(state,binding["workspaceId"].as_str().ok_or("missing workspace")?,task_id))?;
    if dispatch.group.id != group || dispatch.group.revision != revision {
        return Err("group task snapshot mismatch".into());
    }
    let child = dispatch
        .children
        .iter()
        .find(|child| {
            child.binding == *binding
                && child.key == args["idempotency_key"].as_str().unwrap_or("")
                && child.objective == args["objective"].as_str().unwrap_or("")
        })
        .ok_or("objective/key/target not authorized by frozen group task")?;
    if args["from"] != "user-client" || dispatch.cancellation_requested {
        return Err("group dispatch source invalid or cancellation requested".into());
    }
    if !dispatch.coordinating || dispatch.runtime_epoch != state.runtime_epoch {
        return Err(
            "group dispatch coordinator no longer active; no replay of unresolved children".into(),
        );
    }
    Ok(
        json!({"groupId":dispatch.group.id,"groupRevision":dispatch.group.revision,"groupTaskId":dispatch.id,"name":dispatch.group.name,
        "members":dispatch.group.member_agent_ids,"action":dispatch.request.group_action,"targets":dispatch.children.iter().map(|child| &child.binding).collect::<Vec<_>>(),"childTarget":child.binding}),
    )
}
