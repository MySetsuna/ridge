//! Foreign views are graph references; no local shell or parser owns the remote PTY.
use crate::server::AppState;
use ridge_core::workspace::{graph::ForeignPaneBinding,pane_tree::SplitDirection};
use serde_json::{json,Value};
use uuid::Uuid;

pub(crate) fn mark_attached(state:&AppState,host:&str,sessions:&mut [ridge_core::remote::HostSessionMeta])->Result<(),String>{
    let graph=state.workspaces.lock().map_err(|_|"workspace unavailable")?;
    let attached:std::collections::HashSet<_>=graph.workspace_ids().flat_map(|id|graph.workspace(*id).unwrap().foreign_panes.values()).filter(|binding|binding.host_id==host).map(|binding|binding.session_id.to_string()).collect();
    for session in sessions {session.attached |= attached.contains(&session.id);}Ok(())
}

pub(crate) async fn dispatch(state:&AppState,method:&str,args:&Value,native:bool)->Option<Result<Value,String>>{
    if !matches!(method,"attach_host_session"|"detach_host_session"){return None;}
    Some(async {
        if !native{return Err("foreign attachments require native trust".into());}
        let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or("viewed workspace required")?).map_err(|_|"invalid workspaceId")?;
        if method=="detach_host_session" {
            let pane=Uuid::parse_str(args["paneId"].as_str().ok_or("paneId required")?).map_err(|_|"invalid paneId")?;
            let mut graph=state.workspaces.lock().map_err(|_|"workspace unavailable")?;
            let metadata=graph.workspace(workspace).ok_or("workspace unavailable")?;
            if !metadata.foreign_panes.contains_key(&pane){return Ok(Value::Null);}
            let mut next=graph.clone();next.close(workspace,pane).map_err(|error|error.to_string())?;
            crate::registry::save_workspace_graph_at(&state.workspaces_path,&next).map_err(|error|error.to_string())?;
            *graph=next;drop(graph);state.business_events.pane_tree_changed(workspace);return Ok(Value::Null);
        }
        let host_id=args["hostId"].as_str().ok_or("hostId required")?;
        let session=Uuid::parse_str(args["sessionId"].as_str().ok_or("sessionId required")?).map_err(|_|"invalid sessionId")?;
        let remote_workspace=Uuid::parse_str(args["remoteWorkspaceId"].as_str().ok_or("remoteWorkspaceId required")?).map_err(|_|"invalid remoteWorkspaceId")?;
        let host=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?.get(host_id).ok_or("unknown host")?;
        if host.status!=ridge_core::remote::HostStatus::Connected{return Err("host not connected".into());}
        let snapshot=if host.addr=="frontend-remote-link" {
            // Transitional Cloud transport reports through the trusted registration boundary.
            // The graph still owns the view; no local PTY is created for it.
            let session=host.sessions.iter().find(|item|item.id==session.to_string()).ok_or("unknown reported remote session")?;
            json!({"panes":[{"id":session.id,"title":session.title}]})
        }else{state.outbound_hosts.request(host_id,"get_workspace_snapshot",json!({"workspaceId":remote_workspace})).await?};
        let remote=snapshot["panes"].as_array().ok_or("invalid remote pane list")?.iter().find(|pane|pane["id"].as_str()==Some(session.to_string().as_str())).ok_or("remote pane outside workspace")?;
        let retired=state.retired_workspaces.lock().map_err(|_|"workspace lifecycle unavailable")?;
        if retired.contains(&workspace){return Err("workspace retired".into());}
        let mut graph=state.workspaces.lock().map_err(|_|"workspace unavailable")?;
        if graph.workspace(workspace).is_none(){return Err("viewed workspace unavailable".into());}
        for id in graph.workspace_ids(){
            if graph.workspace(*id).unwrap().foreign_panes.values().any(|binding|binding.host_id==host_id&&binding.workspace_id==remote_workspace&&binding.session_id==session){return Err("remote session already attached".into());}
        }
        let mut next=graph.clone();let target=next.leaves(workspace).map_err(|error|error.to_string())?.first().copied().ok_or("workspace layout empty")?;
        let pane=next.split(workspace,target,SplitDirection::Horizontal).map_err(|error|error.to_string())?;
        next.workspace_mut(workspace).unwrap().foreign_panes.insert(pane,ForeignPaneBinding{host_id:host_id.into(),host_label:host.label,kind:host.kind,workspace_id:remote_workspace,session_id:session,title:remote["title"].as_str().unwrap_or("terminal").into(),cwd:remote["cwd"].as_str().map(str::to_owned)});
        crate::registry::save_workspace_graph_at(&state.workspaces_path,&next).map_err(|error|error.to_string())?;
        *graph=next;drop(graph);drop(retired);state.business_events.pane_tree_changed(workspace);Ok(json!(pane))
    }.await)
}
