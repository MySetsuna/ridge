//! Runtime owns native sessions and adopts their existing PTYs into the shared graph.
use crate::{server::AppState,registry::save_workspace_graph_at};
use ridge_core::{CoreError,CoreResult,Ctx};
use ridge_core::workspace::pane_tree::SplitDirection;
use ridge_tmux as native;
use serde_json::{json,Value};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) struct Gui(pub AppState);
fn session_label(id:Uuid,name:Option<&str>)->String {
    name.map(str::trim).filter(|name|!name.is_empty()).map(|name|name.replace([':', '\n', '\r'],"_")).unwrap_or_else(||format!("ws{}",&id.simple().to_string()[..8]))
}
impl native::http::GuiSessionSource for Gui {
    fn cwd_allowed(&self,cwd:&str)->bool {self.0.fs_scope.is_allowed(cwd)}
    fn sessions_for(&self,socket:&str)->Vec<native::GuiSession> {
        if socket!="default"{return Vec::new();}
        self.0.workspaces.lock().map(|graph|{
            let mut sessions:Vec<_>=graph.workspace_ids().map(|id|native::GuiSession{name:session_label(*id,graph.workspace(*id).and_then(|meta|meta.name.as_deref()))}).collect();
            if !sessions.iter().any(|session|session.name=="ridge"){sessions.push(native::GuiSession{name:"ridge".into()});}
            sessions
        }).unwrap_or_default()
    }
    fn session_lines(&self,format:Option<&str>)->Vec<String>{
        self.0.workspaces.lock().map(|graph|graph.workspace_ids().map(|id|{
            let label=session_label(*id,graph.workspace(*id).and_then(|meta|meta.name.as_deref()));
            let size=graph.leaves(*id).ok().and_then(|panes|panes.into_iter().find_map(|pane|self.0.ptys.info(pane).ok())).map(|pty|(pty.cols,pty.rows)).unwrap_or((80,24));
            let attached=graph.active()==Some(*id);
            match format {
                Some(format)=>format.replace("#{session_name}",&label).replace("#S",&label).replace("#{session_attached}",if attached{"1"}else{"0"}).replace("#{session_windows}","1").replace("#{session_width}",&size.0.to_string()).replace("#{session_height}",&size.1.to_string()),
                None=>format!("{label}: 1 windows [{}x{}]{}",size.0,size.1,if attached{" (attached)"}else{""}),
            }
        }).collect()).unwrap_or_default()
    }
}
pub(crate) fn dispatch(state:&AppState,method:&str,args:&Value,ctx:&Ctx)->Option<CoreResult<Value>> {
    if !matches!(method,"list_native_sessions"|"new_headless_session"|"terminate_native_session"|"summon_native_session"){return None;}
    Some((|| {
        let error=|message:String|CoreError::InvalidArgs(message);
        match method {
            "list_native_sessions"=>serde_json::to_value(native::list_all_sessions()).map_err(CoreError::internal),
            "new_headless_session"=>{
                let name=args["name"].as_str().map(str::trim).filter(|name|!name.is_empty()).map(str::to_owned).unwrap_or_else(||format!("term-{}",chrono::Utc::now().timestamp_millis()));
                let cwd=args["cwd"].as_str().map(str::to_owned);
                if cwd.as_ref().is_some_and(|cwd|!state.fs_scope.is_allowed(cwd)||!ctx.capabilities().root_scope().is_allowed(cwd)){return Err(CoreError::OutsideSandbox);}
                native::new_session(native::NewSessionReq {socket:"headless".into(),name:Some(name.clone()),creator_workspace_id:None,creator_pane_id:None,window_name:None,cwd,width:80,height:24,shell:std::env::var("SHELL").ok(),command:None,attach_or_create:false,print:None},&[]).map_err(|failure|error(failure.message()))?;
                Ok(json!(name))
            }
            "terminate_native_session"=>{
                native::kill_session(args["socket"].as_str().ok_or_else(||error("missing native socket".into()))?,args["target"].as_str().ok_or_else(||error("missing native target".into()))?,&[]).map_err(|failure|error(failure.message()))?;Ok(Value::Null)
            }
            _=>{
                let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or_else(||error("summon requires viewed workspace".into()))?).map_err(|_|error("invalid viewed workspace".into()))?;
                let socket=args["socket"].as_str().ok_or_else(||error("missing native socket".into()))?;
                let target=args["target"].as_str().ok_or_else(||error("missing native target".into()))?;
                let retired=state.retired_workspaces.lock().map_err(|_|error("workspace retirement unavailable".into()))?;
                if retired.contains(&workspace){return Err(error("workspace retired".into()));}
                let panes=native::summon(socket,target,&[]).map_err(|failure|error(failure.message()))?;
                let mut graph=state.workspaces.lock().map_err(|_|error("workspace unavailable".into()))?;
                if graph.workspace(workspace).is_none(){return Err(error("viewed workspace unavailable".into()));}
                let mut candidate=graph.clone();
                let mut installed=Vec::new();
                let mut moved=Vec::new();
                let prepared=(|| -> CoreResult<()> { for pane in panes {
                    if pane.cwd.as_ref().is_some_and(|cwd|!state.fs_scope.is_allowed(cwd)||!ctx.capabilities().root_scope().is_allowed(cwd)){return Err(CoreError::OutsideSandbox);}
                    if let Some((old,pid))=pane.prev_attachment {
                        if old==workspace&&state.ptys.contains(pid){continue;}
                        if state.ptys.contains(pid){candidate.close(old,pid)?;moved.push((old,pid));}
                    }
                    let target=candidate.leaves(workspace)?.first().copied().ok_or_else(||error("workspace layout empty".into()))?;
                    let id=candidate.split(workspace,target,SplitDirection::Horizontal)?;
                    if let Some(cwd)=&pane.cwd {candidate.workspace_mut(workspace).unwrap().pane_tree.panes.get_mut(&id).unwrap().cwd=Some(cwd.into());}
                    let global=pane.global_id;
                    state.ptys.adopt_native(id,workspace,socket.into(),pane).map_err(|failure|error(failure.to_string()))?;
                    installed.push((global,id));
                } save_workspace_graph_at(&state.workspaces_path,&candidate).map_err(|failure|error(failure.to_string())) })();
                if let Err(failure)=prepared {
                    for (_,id) in &installed {let _=state.ptys.destroy(*id);}return Err(failure);
                }
                *graph=candidate;
                for (global,id) in &installed {native::set_attachment(socket,*global,Some((workspace,*id)));}
                for (_,id) in &moved {state.ptys.destroy(*id).map_err(|failure|error(failure.to_string()))?;}
                drop(graph);drop(retired);
                if !installed.is_empty(){state.business_events.pane_tree_changed(workspace);}
                for (old,_) in moved {state.business_events.pane_tree_changed(old);}
                Ok(json!(installed.len()))
            }
        }
    })())
}
pub(crate) fn router(state:&AppState)->axum::Router {
    native::http::native_router(native::http::NativeHttpCtx {token:Arc::new(state.token.clone()),gui:Arc::new(Gui(state.clone())),mcp_state:state.mcp_state.clone()})
        .merge(axum::Router::new().route("/api/v1/tmux/summon",axum::routing::post(summon_http)).with_state(state.clone()))
}
async fn summon_http(axum::extract::State(state):axum::extract::State<AppState>,headers:axum::http::HeaderMap,axum::Json(mut args):axum::Json<Value>)->axum::response::Response {
    use axum::response::IntoResponse;
    let authorized=headers.get("x-ridge-token").or_else(||headers.get("x-ridge-kernel-token")).and_then(|value|value.to_str().ok())==Some(state.token.as_str());
    if !authorized{return (axum::http::StatusCode::UNAUTHORIZED,"unauthorized").into_response();}
    let Some(workspace)=headers.get("x-ridge-workspace").and_then(|value|value.to_str().ok()).filter(|workspace|Uuid::parse_str(workspace).is_ok()) else {return (axum::http::StatusCode::BAD_REQUEST,"viewed workspace header required").into_response();};
    args["workspaceId"]=json!(workspace);
    if args["socket"].is_null(){args["socket"]=json!("default");}
    let ctx=crate::host_core::runtime_ctx(state.fs_scope.roots(),true);
    match tokio::task::spawn_blocking(move||dispatch(&state,"summon_native_session",&args,&ctx).unwrap()).await {
        Ok(Ok(count))=>(axum::http::StatusCode::OK,format!("summoned {}",count.as_u64().unwrap_or(0))).into_response(),
        Ok(Err(error))=>(axum::http::StatusCode::BAD_REQUEST,error.to_command_string()).into_response(),
        Err(_)=>(axum::http::StatusCode::INTERNAL_SERVER_ERROR,"summon worker failed").into_response(),
    }
}
