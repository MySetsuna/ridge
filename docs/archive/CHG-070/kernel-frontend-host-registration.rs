//! Runtime authority for registered host topology; transports report discovered sessions.
use crate::server::AppState;
use ridge_core::{remote::{HostKind, HostRecord, HostSessionMeta, HostStatus}, CoreError, CoreResult, EventSink};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Registration { host_id: String, kind: HostKind, label: String, sessions: Vec<Session> }
#[derive(Deserialize)]
struct Session { id: String, title: String }

pub(crate) fn restore_runtime_topology()->anyhow::Result<ridge_core::remote::RemoteHostTopology>{
    let mut records=crate::registry::load_remote_hosts()?;
    let mut changed=false;
    for host in records.values_mut(){
        if matches!(host.status,HostStatus::Connected|HostStatus::Connecting){host.status=HostStatus::Disconnected;host.detail="Runtime 已重启，需重新认证连接".into();changed=true;}
        for session in &mut host.sessions {if session.attached{session.attached=false;changed=true;}}
    }
    if changed {crate::registry::save_remote_hosts(&records)?;}
    Ok(ridge_core::remote::RemoteHostTopology::from_records(records))
}

pub(crate) fn dispatch(state: &AppState, method: &str, args: &Value, native: bool) -> Option<CoreResult<Value>> {
    if !matches!(method, "host_list_snapshot" | "list_host_sessions" | "register_frontend_host") { return None; }
    Some((|| {
        let invalid = |message: String| CoreError::InvalidArgs(message);
        if !native { return Err(invalid("host topology requires native trust".into())); }
        let mut hosts = state.remote_hosts.lock().map_err(|_| invalid("host topology unavailable".into()))?;
        if method == "host_list_snapshot" {
            let mut records=hosts.snapshot();drop(hosts);
            for record in &mut records {crate::foreign_panes::mark_attached(state,&record.id,&mut record.sessions).map_err(invalid)?;}
            return serde_json::to_value(records).map_err(|error| invalid(error.to_string()));
        }
        if method == "list_host_sessions" {
            let id = args["hostId"].as_str().filter(|id| !id.trim().is_empty()).ok_or_else(|| invalid("missing hostId".into()))?;
            let mut host = hosts.get(id).ok_or_else(|| invalid(format!("unknown host: {id}")))?;drop(hosts);
            crate::foreign_panes::mark_attached(state,id,&mut host.sessions).map_err(invalid)?;
            return serde_json::to_value(host.sessions).map_err(|error| invalid(error.to_string()));
        }
        let registration: Registration = serde_json::from_value(args.clone()).map_err(|error| invalid(error.to_string()))?;
        if registration.host_id.trim().is_empty() || registration.host_id.len() > 4096 || registration.label.len() > 4096 || registration.sessions.len() > 4096 {
            return Err(invalid("invalid host registration bounds".into()));
        }
        let previous = hosts.get(&registration.host_id);
        let mut ids = HashSet::new();
        let mut sessions = Vec::with_capacity(registration.sessions.len());
        for session in registration.sessions {
            if session.id.trim().is_empty() || session.id.len() > 4096 || session.title.len() > 65536 || !ids.insert(session.id.clone()) {
                return Err(invalid("invalid or duplicate host session".into()));
            }
            let attached = previous.as_ref().and_then(|host| host.sessions.iter().find(|old| old.id == session.id)).is_some_and(|old| old.attached);
            sessions.push(HostSessionMeta { id: session.id, title: session.title, attached });
        }
        let mut next = hosts.clone();
        next.upsert(HostRecord { id: registration.host_id, kind: registration.kind, label: registration.label,
            addr: "frontend-remote-link".into(), status: HostStatus::Connected, detail: "RemoteLink topology".into(), sessions });
        let changed = next.records() != hosts.records();
        crate::registry::save_remote_hosts_at(&state.remote_hosts_path, next.records()).map_err(|error| invalid(format!("host registration failed; previous topology retained: {error}")))?;
        *hosts = next;
        if changed { state.business_events.broadcast("host-list-changed", Value::Null); }
        Ok(json!(null))
    })())
}

