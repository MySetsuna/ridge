//! Explicit terminal presentation mutations owned by the common Runtime.
use crate::server::AppState;
use ridge_core::{CoreError,CoreResult,Ctx,EventSink};
use serde_json::{json,Value};
use uuid::Uuid;
pub(crate) async fn dispatch(state:&AppState,method:&str,args:&Value,ctx:&Ctx)->Option<CoreResult<Value>>{
    if !matches!(method,"clear_pane_terminal"|"toggle_mode"){return None;}
    Some(async {
        let error=|message:String|CoreError::InvalidArgs(message);
        let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or_else(||error("workspace required".into()))?).map_err(|_|error("invalid workspace".into()))?;
        let pane=Uuid::parse_str(args["paneId"].as_str().ok_or_else(||error("pane required".into()))?).map_err(|_|error("invalid pane".into()))?;
        if method=="clear_pane_terminal"{
            if let Some((owner,binding))=crate::foreign_terminal::binding(state,pane).map_err(error)?{
                if owner!=workspace{return Err(error("pane outside workspace".into()));}
                state.outbound_hosts.request(&binding.host_id,method,json!({"workspaceId":binding.workspace_id,"paneId":binding.session_id})).await.map_err(error)?;
            }else{
                if state.ptys.info(pane).map_err(|failure|error(failure.to_string()))?.workspace_id!=Some(workspace){return Err(error("pane outside workspace".into()));}
                state.ptys.clear_terminal_preserving_prompt(pane).map_err(|failure|error(failure.to_string()))?;
            }
            return Ok(Value::Null);
        }
        let mode:ridge_core::workspace::mode::PaneMode=serde_json::from_value(args["mode"].clone()).map_err(|failure|error(failure.to_string()))?;
        if let ridge_core::workspace::mode::PaneMode::Editor{file_path:Some(path),..}=&mode {
            if ridge_core::dispatch::path_has_traversal(&path.to_string_lossy()){return Err(CoreError::PathTraversal);}
            if !state.fs_scope.is_allowed(&path.to_string_lossy())||!ctx.capabilities().root_scope().is_allowed(&path.to_string_lossy()){return Err(CoreError::OutsideSandbox);}
        }
        let mut graph=state.workspaces.lock().map_err(|_|error("workspace unavailable".into()))?;
        let mut next=graph.clone();
        let target=next.workspace_mut(workspace).and_then(|metadata|metadata.pane_tree.panes.get_mut(&pane)).ok_or_else(||error("pane outside workspace".into()))?;
        target.mode=mode.clone();
        crate::registry::save_workspace_graph_at(&state.workspaces_path,&next).map_err(|failure|CoreError::Io(failure.to_string()))?;
        *graph=next;drop(graph);
        state.business_events.broadcast("pane-mode-changed",json!({"workspaceId":workspace,"paneId":pane,"mode":mode}));
        state.business_events.pane_tree_changed(workspace);Ok(Value::Null)
    }.await)
}
