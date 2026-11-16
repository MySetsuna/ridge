//! Shared control-plane health from Runtime-owned queues, documents and topology.
use crate::server::AppState;
use serde_json::{json,Value};
pub(crate) fn read(state:&AppState,workspace:Option<&str>)->Result<Value,String> {
    let pending=crate::commune::pending_snapshot(state,false,workspace)?.as_array().ok_or("invalid permission snapshot")?.len();
    let suspended=crate::workspace_memory::dispatch("get_automation_pause_count",&json!({"workspaceId":workspace}),true)?.as_u64().ok_or("invalid pause count")?;
    let enabled=pending>0||(workspace.is_none()&&crate::settings::gateway_enabled()?);
    let (foreign,outbound,generation)=if workspace.is_some() {
        (0,0,0)
    } else {
        let mut hosts=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?.snapshot();
        for host in &mut hosts {crate::foreign_panes::mark_attached(state,&host.id,&mut host.sessions)?;}
        (hosts.iter().flat_map(|host|host.sessions.iter()).filter(|session|session.attached).count(),
         hosts.iter().filter(|host|host.status==ridge_core::remote::HostStatus::Connected).count(),
         state.business_events.generation()?+1)
    };
    let degraded=suspended>0||(enabled&&pending>0);
    let level=if degraded {"degraded"} else if foreign>0||outbound>0 {"watch"} else {"ok"};
    let audit=crate::commune_history::audit(state,&json!({"limit":50,"workspaceId":workspace}))?;
    let rows=audit["items"].as_array().ok_or("invalid audit snapshot")?;
    let mut counts=json!({"approve":0,"reject":0,"timeout":0,"other":0,"total":rows.len(),"cap":50});
    for row in rows {
        let key=match row["verdict"].as_str(){Some("approve"|"approved")=>"approve",Some("reject"|"rejected")=>"reject",Some("timeout")=>"timeout",_=>"other"};
        counts[key]=json!(counts[key].as_u64().ok_or("invalid audit counter")?+1);
    }
    Ok(json!({"suspendedAgents":suspended,"pendingHitl":pending,"hitlEnabled":enabled,"degraded":degraded,
        "generation":generation,"foreignAttached":foreign,"outboundHostsConnected":outbound,"level":level,
        "foreignAttachedHint":"hosts foreign panes appear in roster when agent-tagged","hitlAudit":counts,
        "pollHintMs":match level {"degraded"=>1500,"watch"=>3000,_=>8000}}))
}
