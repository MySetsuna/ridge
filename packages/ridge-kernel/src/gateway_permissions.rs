//! Runtime-owned legacy command-input approvals; distinct from ACP permissions.
use crate::{business::BusinessEvents, commune_permissions::HistoryLog, server::AppState};
use ridge_core::EventSink;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::{Arc,Mutex}, io::{BufRead,BufReader,Read}};
use tokio::sync::oneshot;
use uuid::Uuid;
struct Pending { request:Value, sender:oneshot::Sender<Value>, log:Arc<HistoryLog> }
#[derive(Default)]
pub(crate) struct Gateway { pending:Mutex<HashMap<String,Pending>>, log:Mutex<Option<Arc<HistoryLog>>> }
fn path()->std::path::PathBuf {crate::registry::ridge_data_dir().join("gateway-permissions.jsonl")}
impl Gateway {
 pub(crate) async fn submit(self:Arc<Self>,state:&AppState,args:&Value)->Result<Value,String>{
  let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or("missing workspaceId")?).map_err(|_|"invalid workspaceId")?;
  let pane=Uuid::parse_str(args["paneId"].as_str().ok_or("missing paneId")?).map_err(|_|"invalid paneId")?;
  let text=args["text"].as_str().ok_or("text must be a string")?;
  if text.len()>65536{return Err("gateway input too large".into());}
  let info=state.ptys.list().into_iter().find(|terminal|terminal.id==pane&&terminal.workspace_id==Some(workspace)).ok_or("gateway terminal binding missing")?;
  let binding=state.ptys.capture_input_binding(pane).map_err(|error|error.to_string())?;
  crate::workspace_memory::with_agent_input(workspace,pane,||Ok(()))?;
  let submitted=text.ends_with(['\r','\n']);let command=text.trim_end_matches(['\r','\n']);
  let payload=if submitted&&!command.is_empty(){
   let mut request=args.clone();request["action"]=json!(command);
   let decision=self.request(state,&request).await?;
   match decision["verdict"].as_str(){
    Some("approve")=>ridge_mcp::server::enter_terminated(command),
    Some("modify")=>ridge_mcp::server::enter_terminated(decision["replacement"].as_str().ok_or("gateway replacement missing")?),
    _=>return Err("Execution blocked by user authorization policy".into()),
   }
  }else if submitted{ridge_mcp::server::enter_terminated(text)}else{text.to_owned()};
  let state=state.clone();
  tokio::task::spawn_blocking(move||{
   let roster=state.roster.lock().map_err(|_|"roster unavailable")?;
   if roster.agent_identities().iter().any(|member|member.pane_id==pane.to_string()&&member.protocol_binding.is_some()){return Err("registered Agent messages require the existing protocol; use send_agent_message".into());}
   let live=state.ptys.list().into_iter().find(|terminal|terminal.id==pane).ok_or("gateway terminal binding missing")?;
   if live.workspace_id!=Some(workspace)||live.child_pid!=info.child_pid||live.child_start_instance!=info.child_start_instance{return Err("gateway process binding changed".into());}
   crate::workspace_memory::with_agent_input(workspace,pane,||state.ptys.write_bound_input(pane,&binding,payload.as_bytes()).map_err(|error|error.to_string()))?;
   Ok(json!({"terminalAccepted":true}))
  }).await.map_err(|error|error.to_string())?
 }

 pub(crate) async fn request(self:Arc<Self>,state:&AppState,args:&Value)->Result<Value,String> {
  let workspace=args["workspaceId"].as_str().ok_or("missing workspaceId")?;
  let pane=args["paneId"].as_str().ok_or("missing paneId")?;
  let wid=Uuid::parse_str(workspace).map_err(|_|"invalid workspaceId")?;
  let pid=Uuid::parse_str(pane).map_err(|_|"invalid paneId")?;
  let terminal=state.ptys.list().into_iter().find(|terminal|terminal.id==pid&&terminal.workspace_id==Some(wid)).ok_or("gateway terminal binding missing")?;
  if terminal.child_pid.is_none()||terminal.child_start_instance.is_none() {return Err("gateway process binding unavailable".into());}
  if state.roster.lock().map_err(|_|"roster unavailable")?.agent_identities().iter().any(|member|member.pane_id==pane&&member.protocol_binding.is_some()) {
   return Err("registered Agent messages require the existing protocol; use send_agent_message".into());
  }
  let action=args["action"].as_str().ok_or("missing action")?;
  if action.len()>65536 {return Err("gateway action too large".into());}
  let assessment=ridge_core::classify_shell_command(action);
  if !crate::settings::gateway_enabled()?||assessment.level!=ridge_core::RiskLevel::Dangerous {return Ok(json!({"verdict":"approve"}));}
  let initiator=args["initiator"].as_str().ok_or("missing initiator")?;
  if initiator.len()>1024 {return Err("gateway initiator too large".into());}
  let id=format!("gateway-{}",Uuid::new_v4());let nonce=Uuid::new_v4().to_string();let (sender,receiver)=oneshot::channel();
  {
   let mut pending=self.pending.lock().map_err(|_|"gateway unavailable")?;
   if pending.len()>=256 {return Err("gateway approval queue full".into());}
   let mut writer=self.log.lock().map_err(|_|"gateway ledger unavailable")?;
   if writer.is_none(){*writer=Some(Arc::new(HistoryLog::open(&path())?));}
   let log=writer.as_ref().unwrap().clone();
   let accepted=log.append(json!({"kind":"gateway_permission_requested","id":id,"workspaceId":workspace,"paneId":pane}))?;
   let request=json!({"id":id,"kind":"approval","workspaceId":workspace,"paneId":pane,"initiator":initiator,
    "action":action,"level":"Dangerous","reason":assessment.reason,"executor":"Ridge command input gateway","policySource":"Ridge HITL",
    "requestId":id,"createdAt":accepted["timestamp"],"resolutionNonce":nonce,"processId":terminal.child_pid,"processInstance":terminal.child_start_instance,
    "nextStep":"Approval releases the original input request; terminal acceptance does not prove command execution."});
   pending.insert(id.clone(),Pending{request,sender,log});
  }
  let _waiting=Waiting{gateway:self.clone(),id,events:state.business_events.clone()};
  state.business_events.broadcast("teammate://hitl-pending-changed",Value::Null);
  Ok(tokio::time::timeout(std::time::Duration::from_secs(120),receiver).await.ok().and_then(Result::ok).unwrap_or_else(||json!({"verdict":"reject"})))
 }
 pub(crate) fn snapshot(&self,local:bool)->Result<Vec<Value>,String> {
  let pending=self.pending.lock().map_err(|_|"gateway unavailable")?;
  let mut rows=pending.values().map(|entry|if local {entry.request.clone()} else {json!({"id":entry.request["id"],"workspaceId":entry.request["workspaceId"],"initiator":entry.request["paneId"],"level":"Dangerous","reason":"Ridge command input approval","createdAt":entry.request["createdAt"],"resolutionNonce":entry.request["resolutionNonce"]})}).collect::<Vec<_>>();
  rows.sort_by(|a,b|a["id"].as_str().cmp(&b["id"].as_str()));Ok(rows)
 }
 pub(crate) fn resolve(&self,state:&AppState,args:&Value,native:bool)->Result<Option<&'static str>,String> {
  let id=args["id"].as_str().ok_or("missing permission ID")?;
  let mut pending=self.pending.lock().map_err(|_|"gateway unavailable")?;
  let Some(entry)=pending.get(id) else{return Ok(if id.starts_with("gateway-"){Some("already-resolved")}else{None})};
  if !args["workspaceId"].is_null()&&args["workspaceId"]!=entry.request["workspaceId"] {return Err("gateway workspace mismatch".into());}
  if !args["optionId"].is_null() {return Ok(Some("bad-verdict"));}
  if !native {
   let nonce=args["resolutionNonce"].as_str().ok_or("missing remote permission nonce")?;
   let expected=entry.request["resolutionNonce"].as_str().ok_or("gateway nonce missing")?;
   if nonce.len()!=expected.len()||nonce.bytes().zip(expected.bytes()).fold(0u8,|difference,(a,b)|difference|(a^b))!=0 {return Ok(Some("nonce-mismatch"));}
  }
  let verdict=args["verdict"].as_str().ok_or("missing verdict")?;
  if !["approve","reject","modify"].contains(&verdict)||(!native&&verdict=="modify") {return Ok(Some("bad-verdict"));}
  let replacement=if verdict=="modify" {Some(args["replacement"].as_str().filter(|text|!text.trim().is_empty()&&text.len()<=65536).ok_or("invalid replacement")?)} else {None};
  if !args["replacement"].is_null()&&verdict!="modify" {return Ok(Some("bad-verdict"));}
  if verdict!="reject"&&!state.ptys.list().iter().any(|terminal|terminal.id.to_string()==entry.request["paneId"]&&terminal.workspace_id.map(|id|id.to_string())==entry.request["workspaceId"].as_str().map(str::to_owned)
   &&json!(terminal.child_pid)==entry.request["processId"]&&json!(terminal.child_start_instance)==entry.request["processInstance"]) {return Err("gateway terminal binding changed".into());}
  entry.log.append(json!({"kind":"gateway_permission_decided","id":id,"workspaceId":entry.request["workspaceId"],"paneId":entry.request["paneId"],"source":if native {"local"}else{"remote"},"verdict":verdict}))?;
  let entry=pending.remove(id).unwrap();
  Ok(Some(if entry.sender.send(json!({"verdict":verdict,"replacement":replacement})).is_ok(){"consumed"}else{"already-resolved"}))
 }
 pub(crate) fn audit(&self,workspace:Option<&str>)->Result<Vec<Value>,String> {
  let writer=self.log.lock().map_err(|_|"gateway ledger unavailable")?;
  let file=match std::fs::File::open(path()){Ok(file)=>file,Err(error)if error.kind()==std::io::ErrorKind::NotFound=>return Ok(Vec::new()),Err(error)=>return Err(error.to_string())};
  let length=if let Some(log)=writer.as_ref(){log.committed_length()?}else{file.metadata().map_err(|error|error.to_string())?.len()};
  let mut rows=std::collections::VecDeque::new();
  for line in BufReader::new(file.take(length)).lines(){let record:Value=serde_json::from_str(&line.map_err(|error|error.to_string())?).map_err(|error|error.to_string())?;
   if workspace.is_some_and(|workspace|record["workspaceId"]!=workspace)||!matches!(record["kind"].as_str(),Some("gateway_permission_decided"|"gateway_permission_cancelled")){continue;}
   rows.push_back(json!({"id":record["id"],"ts":record["timestamp"],"source":record["source"],"initiator":record["paneId"],"verdict":record["verdict"],"riskLevel":"Dangerous","reasonSummary":"Ridge command input approval","outcome":if record["kind"]=="gateway_permission_decided"{"decided"}else{"cancelled"}}));if rows.len()>50{rows.pop_front();}
  }Ok(rows.into_iter().collect())
 }
}
struct Waiting {gateway:Arc<Gateway>,id:String,events:BusinessEvents}
impl Drop for Waiting {fn drop(&mut self){if let Ok(mut pending)=self.gateway.pending.lock(){if let Some(entry)=pending.remove(&self.id){if let Err(error)=entry.log.append(json!({"kind":"gateway_permission_cancelled","id":self.id,"workspaceId":entry.request["workspaceId"],"paneId":entry.request["paneId"],"source":"cancellation","verdict":"reject"})){tracing::error!(%error,"gateway cancellation ledger unavailable");}}}self.events.broadcast("teammate://hitl-pending-changed",Value::Null);}}
