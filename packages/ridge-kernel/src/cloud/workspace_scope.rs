//! Verified workspace grant boundary, enforced against current Runtime resources.
use crate::server::AppState;
use serde::{Deserialize,Serialize};
use serde_json::{json,Value};
use std::{collections::HashSet,path::PathBuf};
use uuid::Uuid;

#[derive(Clone,Deserialize,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Scope {pub grant_id:String,pub grantee_user_id:String,pub owner_user_id:String,pub device_name:String,pub workspace_id:String,pub role:String,pub delegable:bool}
pub fn plan(state:&AppState,scope:&Scope,method:&str,args:&Value)->Result<(String,Value,Option<Value>),String> {
    if scope.role!="operator"||scope.delegable {return Err("unsupported workspace-share role".into());}
    if ridge_core::protocol_guard::is_desktop_privileged(method)||matches!(method,
        "create_workspace"|"close_workspace"|"reorder_workspaces"|"save_workspace"|"list_saved_workspaces"|"delete_saved_workspace"|"rename_saved_workspace"|
        "list_workspace_save_info"|"delete_workspace_file"|"get_default_workspace_save_dir"|"list_saved_workspace_files"|"save_workspace_to_file"|
        "open_workspace_from_file"|"get_restore_set"|"list_recent_workspaces"|"clear_recent_workspaces"|"get_last_opened_workspace_path"|"get_startup_context"|
        "browse_directory"|"get_remote_info"|"get_remote_totp"|"set_cloud_remote_active"|
        "list_native_sessions"|"new_headless_session"|"terminate_native_session"|"summon_native_session") {return Err("second hop or host-global method denied".into());}
    let id=Uuid::parse_str(&scope.workspace_id).map_err(|_|"invalid granted workspace")?;
    let panes:HashSet<String>={let graph=state.workspaces.lock().map_err(|_|"workspace unavailable")?;let metadata=graph.workspace(id).ok_or("granted workspace unavailable")?;
        metadata.pane_tree.get_all_leaves().into_iter().filter(|pane|!metadata.foreign_panes.contains_key(pane)).map(|id|id.to_string()).collect()};
    let mut roots=Vec::<PathBuf>::new();
    for terminal in state.ptys.list().into_iter().filter(|terminal|terminal.workspace_id==Some(id)) {
        if let Some(cwd)=terminal.cwd {roots.push(PathBuf::from(&cwd));if let Some(root)=ridge_core::commands::git::find_git_repo_root(cwd){roots.push(PathBuf::from(root));}}
    }
    let capabilities=ridge_core::CapabilitySet::remote_default().with_roots(roots.clone());
    fn resources(value:&Value,key:&str,scope:&Scope,panes:&HashSet<String>,roots:&ridge_core::sandbox::RootScope,has_roots:bool,repo:Option<&str>)->Result<(),String> {
        match value {
            Value::String(text)=>{
                if key=="workspaceId"&&text!=&scope.workspace_id {return Err("workspace mismatch".into());}
                if matches!(key,"paneId"|"targetPaneId"|"sourcePaneId")&&!panes.contains(text) {return Err("terminal outside workspace".into());}
                if matches!(key,"path"|"cwd"|"root"|"repoRoot"|"sourcePath"|"destinationPath"|"oldPath"|"newPath"|"filePath"|"paths") {
                    let candidate=if matches!(key,"path"|"paths")&&!std::path::Path::new(text).is_absolute() {
                        std::path::PathBuf::from(repo.ok_or("relative path requires a granted repository root")?).join(text)
                    } else {std::path::PathBuf::from(text)};
                    if !has_roots||!roots.is_allowed(&candidate.to_string_lossy()) {return Err("path outside granted workspace roots".into());}
                }
            }
            Value::Array(values)=>{for value in values {resources(value,key,scope,panes,roots,has_roots,repo)?;}},
            Value::Object(values)=>{for (key,value) in values {resources(value,key,scope,panes,roots,has_roots,repo)?;}},
            _=>{},
        }Ok(())
    }
    resources(args,"",scope,&panes,capabilities.root_scope(),!roots.is_empty(),args["repoRoot"].as_str())?;
    if method=="get_active_workspace_id" {return Ok((method.into(),args.clone(),Some(json!(scope.workspace_id))));}
    if method=="switch_workspace" {return if args["workspaceId"]==scope.workspace_id {Ok((method.into(),args.clone(),Some(Value::Null)))} else {Err("workspace mismatch".into())};}
    if method=="get_current_project" {return Ok((method.into(),args.clone(),Some(json!(roots.first().map(|path|path.to_string_lossy().into_owned())))));}
    let mut args=if args.is_object(){args.clone()}else{json!({})};
    if method=="get_pane_layout" {return Ok(("get_pane_layout_for".into(),json!({"workspaceId":scope.workspace_id}),None));}
    if method.contains("workspace")||method.starts_with("get_teammate_")||method.starts_with("list_hitl_")||matches!(method,"resolve_hitl_remote"|"get_orchestration_health"|"read_agent_recent_replies"|"set_teammate_groups"|"send_agent_message"|"get_agent_message_delivery_status"|"cancel_agent_message")
        ||args.get("paneId").is_some() {args["workspaceId"]=json!(scope.workspace_id);}
    Ok((method.into(),args,None))
}
