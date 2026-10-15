//! One Runtime writer for the existing portable workspace-memory documents.
use serde_json::{json,Value};
use std::{path::PathBuf,sync::Mutex};
use uuid::Uuid;
static WRITES:Mutex<()>=Mutex::new(());
pub(crate) fn directory()->PathBuf {crate::workspace_history::directory().join("workspace-memory")}
fn path(id:Uuid)->PathBuf {directory().join(format!("{id}.json"))}
fn read(id:Uuid)->Result<Option<Value>,String>{
 match std::fs::read(path(id)) {
  Ok(bytes)=>{let value:Value=serde_json::from_slice(&bytes).map_err(|error|format!("invalid workspace memory; original preserved: {error}"))?;if !value.is_object(){return Err("invalid workspace memory; original preserved".into());}Ok(Some(value))},
  Err(error)if error.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(error)=>Err(error.to_string()),
 }
}
pub(crate) fn dispatch(method:&str,args:&Value,native:bool)->Result<Value,String>{
 if !native{return Err("workspace memory requires native trust".into());}
 if method=="get_workspace_memory_directory"{return Ok(json!(directory()));}
 if method=="get_automation_pause_count" {
  let _guard=WRITES.lock().map_err(|_|"workspace memory unavailable")?;
  let entries=match std::fs::read_dir(directory()){Ok(entries)=>entries,Err(error)if error.kind()==std::io::ErrorKind::NotFound=>return Ok(json!(0)),Err(error)=>return Err(error.to_string())};
  let mut count=0;
  for entry in entries {let path=entry.map_err(|error|error.to_string())?.path();if path.extension().and_then(|value|value.to_str())!=Some("json"){continue;}let id=Uuid::parse_str(path.file_stem().and_then(|value|value.to_str()).ok_or("invalid memory document name")?).map_err(|_|"invalid memory document name")?;if let Some(doc)=read(id)?{count+=pause_ids(&doc)?.len();}}
  return Ok(json!(count));
 }
 let id=Uuid::parse_str(args["workspaceId"].as_str().ok_or("missing workspaceId")?).map_err(|_|"invalid workspaceId")?;
 let _guard=WRITES.lock().map_err(|_|"workspace memory unavailable")?;
 if method=="retire_workspace_memory_document"{
  match std::fs::rename(path(id),path(id).with_extension(format!("retired-{}",Uuid::new_v4()))) {
   Ok(())=>{},Err(error)if error.kind()==std::io::ErrorKind::NotFound=>{},Err(error)=>return Err(error.to_string()),
  }return Ok(Value::Null);
 }
 if method=="set_workspace_memory"&&["goal","constraints","tasks"].iter().all(|field|args.get(*field).is_none_or(Value::is_null)){return Ok(Value::Null);}
 let previous=read(id)?;
 if method=="get_automation_input_paused" {
  let pane=Uuid::parse_str(args["paneId"].as_str().ok_or("missing paneId")?).map_err(|_|"invalid paneId")?;
  return Ok(json!(previous.as_ref().map(pause_ids).transpose()?.unwrap_or_default().contains(&pane)));
 }
 if method=="append_workspace_rollback_patch" {
  let patch=args["patch"].clone();serde_json::from_value::<crate::workspace_rollback::RollbackPatch>(patch.clone()).map_err(|error|error.to_string())?;
  let mut doc=previous.clone().unwrap_or_else(||json!({})).as_object().unwrap().clone();
  let rows=doc.entry("rollbackPatches").or_insert_with(||json!([])).as_array_mut().ok_or("invalid rollbackPatches; original preserved")?;
  rows.push(patch);if rows.len()>20{rows.drain(..rows.len()-20);}
  doc.insert("updatedAt".into(),json!(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error|error.to_string())?.as_millis()as u64));
  crate::workspace_history::write_at(&path(id),&serde_json::to_vec_pretty(&Value::Object(doc)).map_err(|error|error.to_string())?)?;return Ok(Value::Null);
 }
 if method=="read_workspace_memory_document"{return Ok(json!(previous));}
 if method=="get_workspace_memory"{return Ok(previous.unwrap_or_else(||json!({})));}
 let mut next=if method=="set_automation_input_paused"||method=="clear_automation_input_pauses" {
  let mut doc=previous.clone().unwrap_or_else(||json!({})).as_object().unwrap().clone();
  let mut ids=pause_ids(&Value::Object(doc.clone()))?;
  if method=="clear_automation_input_pauses" {ids.clear();}else{
   let pane=Uuid::parse_str(args["paneId"].as_str().ok_or("missing paneId")?).map_err(|_|"invalid paneId")?;
   let paused=args["paused"].as_bool().ok_or("paused must be a boolean")?;
   ids.retain(|id|*id!=pane);if paused{ids.push(pane);}
  }
  ids.sort_unstable();ids.dedup();if ids.is_empty(){doc.remove("suspendedPanes");}else{doc.insert("suspendedPanes".into(),json!(ids));}doc
 }else if method=="commit_workspace_memory_document" {
  if json!(previous)!=args["expected"]{return Err("workspace memory changed; reload before proposing another update".into());}
  if !args["value"].is_null()&&!args["value"].is_object(){return Err("workspace memory proposal must be an object or null".into());}
  args["value"].as_object().cloned().unwrap_or_default()
 }else{
  let mut doc=previous.clone().unwrap_or_else(||json!({})).as_object().unwrap().clone();
  for field in ["goal","constraints","tasks"] {
   if let Some(value)=args.get(field).filter(|value|!value.is_null()) {
    match field {"goal" if !value.is_string()=>return Err("goal must be a string".into()),"constraints" if !value.as_array().is_some_and(|rows|rows.iter().all(Value::is_string))=>return Err("constraints must be a string array".into()),"tasks" if !value.is_array()=>return Err("tasks must be an array".into()),_=>{}}
    if value.as_str().is_some_and(|text|text.trim().is_empty())||value.as_array().is_some_and(Vec::is_empty){doc.remove(field);}else{doc.insert(field.into(),value.clone());}
   }
  }doc
 };
 next.remove("updatedAt");
 if method=="set_automation_input_paused"||method=="clear_automation_input_pauses" {
  let mut before=previous.clone().unwrap_or_else(||json!({})).as_object().unwrap().clone();before.remove("updatedAt");
  if before==next{return Ok(Value::Null);}
 }

 if next.is_empty(){
  if previous.is_some(){std::fs::rename(path(id),path(id).with_extension(format!("retired-{}",Uuid::new_v4()))).map_err(|error|error.to_string())?;}
 }else{
  let timestamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|error|error.to_string())?.as_millis() as u64;
  next.insert("updatedAt".into(),json!(timestamp));
  crate::workspace_history::write_at(&path(id),&serde_json::to_vec_pretty(&Value::Object(next)).map_err(|error|error.to_string())?)?;
 }
 Ok(Value::Null)
}

/// A committed pause and automation input share one linearization boundary.
pub(crate) fn with_agent_input<T>(workspace:Uuid,pane:Uuid,write:impl FnOnce()->Result<T,String>)->Result<T,String>{
 let _guard=WRITES.lock().map_err(|_|"workspace memory unavailable; input rejected")?;
 if let Some(doc)=read(workspace)? {
  if let Some(panes)=doc.get("suspendedPanes") {
   let panes=panes.as_array().ok_or("invalid suspension state; input rejected")?;
   let ids=panes.iter().map(|value|value.as_str().and_then(|value|Uuid::parse_str(value).ok()).ok_or("invalid suspension state; input rejected")).collect::<Result<Vec<_>,_>>()?;
   if ids.contains(&pane){return Err("agent suspended: Runtime input gate active".into());}
  }
 }
 write()
}

fn pause_ids(doc:&Value)->Result<Vec<Uuid>,String>{
 let Some(panes)=doc.get("suspendedPanes")else{return Ok(Vec::new())};
 panes.as_array().ok_or("invalid suspension state; input rejected")?.iter().map(|value|value.as_str().and_then(|value|Uuid::parse_str(value).ok()).ok_or_else(||"invalid suspension state; input rejected".into())).collect()
}

pub(crate) fn append_rollback_patch(workspace:Uuid,patch:Value)->Result<(),String>{
 dispatch("append_workspace_rollback_patch",&json!({"workspaceId":workspace,"patch":patch}),true).map(|_|())
}
