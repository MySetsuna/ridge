//! Explicit human sidebar input to the same terminal. Not an Agent delivery adapter.
use crate::server::AppState;
use ridge_core::process_identity::{self,ProcessInstance};
use ridge_term::terminal_protocol::TerminalInput;
use serde::Deserialize;
use serde_json::{json,Value};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct ViewRequest {workspace_id:Uuid,pane_id:Uuid,process:ProcessInstance,terminal_epoch:String}
#[derive(Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct InputRequest {process:ProcessInstance,input:TerminalInput}

fn root(state:&AppState,workspace:Uuid,pane:Uuid,process:&ProcessInstance)->Result<ProcessInstance,String>{
    let graph=state.workspaces.lock().map_err(|_|"workspace unavailable")?;
    if !graph.workspace(workspace).is_some_and(|entry|entry.pane_tree.panes.contains_key(&pane)){return Err("Agent pane does not belong to workspace".into());}drop(graph);
    let info=state.ptys.info(pane).map_err(|error|error.to_string())?;
    if info.workspace_id!=Some(workspace){return Err("Agent terminal workspace changed".into());}
    let root=ProcessInstance{pid:info.child_pid.ok_or("terminal process unavailable")?,start_instance:info.child_start_instance.ok_or("terminal process instance unavailable")?};
    if !process_identity::descendant(&root,process).map_err(|error|error.to_string())?{return Err("Agent process is no longer in this terminal".into());}
    Ok(root)
}
pub(crate) fn dispatch(state:&AppState,method:&str,args:&Value,native:bool)->Result<Value,String>{
    if !native{return Err("manual terminal interaction requires native trust".into());}
    if method=="get_agent_terminal_view" {
        let request:ViewRequest=serde_json::from_value(args.clone()).map_err(|error|error.to_string())?;
        let before=root(state,request.workspace_id,request.pane_id,&request.process)?;
        let view=state.ptys.agent_terminal_view(request.pane_id,request.workspace_id,80).map_err(|error|error.to_string())?;
        if view["terminalEpoch"]!=request.terminal_epoch||root(state,request.workspace_id,request.pane_id,&request.process)?!=before{return Err("Agent terminal binding changed; refresh before using it".into());}
        Ok(view)
    }else{
        let request:InputRequest=serde_json::from_value(args.clone()).map_err(|error|error.to_string())?;
        let workspace=Uuid::parse_str(&request.input.pane.workspace_id).map_err(|_|"invalid workspaceId")?;
        let pane=Uuid::parse_str(&request.input.pane.pane_id).map_err(|_|"invalid paneId")?;
        let expected=root(state,workspace,pane,&request.process)?;
        let ack=state.ptys.terminal_input_checked(&request.input,||{
            // Re-check at the actual pipe write, after the shared input ledger
            // has fenced workspace, terminal epoch, sequence and retry bytes.
            if !process_identity::descendant(&expected,&request.process)?{anyhow::bail!("Agent process changed before manual input");}Ok(())
        }).map_err(|error|error.to_string())?;
        Ok(json!({"sequence":ack.sequence,"alreadyApplied":ack.already_applied,"terminalAccepted":true}))
    }
}
