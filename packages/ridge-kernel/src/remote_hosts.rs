//! Runtime-owned host topology; only authenticated Runtime transports discover live sessions.
use crate::server::AppState;
use ridge_core::{remote::HostStatus, CoreError, CoreResult};
use serde_json::Value;

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
    if !matches!(method, "host_list_snapshot" | "list_host_sessions") { return None; }
    Some((|| {
        let invalid = |message: String| CoreError::InvalidArgs(message);
        if !native { return Err(invalid("host topology requires native trust".into())); }
        let hosts = state.remote_hosts.lock().map_err(|_| invalid("host topology unavailable".into()))?;
        if method == "host_list_snapshot" {
            let mut records=hosts.snapshot();drop(hosts);
            for record in &mut records {crate::foreign_panes::mark_attached(state,&record.id,&mut record.sessions).map_err(invalid)?;}
            return serde_json::to_value(records).map_err(|error| invalid(error.to_string()));
        }
        {
            let id = args["hostId"].as_str().filter(|id| !id.trim().is_empty()).ok_or_else(|| invalid("missing hostId".into()))?;
            let mut host = hosts.get(id).ok_or_else(|| invalid(format!("unknown host: {id}")))?;drop(hosts);
            crate::foreign_panes::mark_attached(state,id,&mut host.sessions).map_err(invalid)?;
            return serde_json::to_value(host.sessions).map_err(|error| invalid(error.to_string()));
        }
    })())
}

