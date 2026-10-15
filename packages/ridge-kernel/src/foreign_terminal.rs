//! Foreign semantic frames and input stay on the original remote terminal owner.
use crate::server::AppState;
use axum::{extract::{State,Path,Query,WebSocketUpgrade,ws::{WebSocket,Message}},http::{HeaderMap,StatusCode},response::Response};
use base64::Engine;
use ridge_core::workspace::graph::ForeignPaneBinding;
use ridge_term::terminal_protocol::{PaneRef,decode_frame,encode_frame};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::HashMap,sync::Arc,time::Duration};
use tokio::sync::{Mutex,mpsc,oneshot,watch};
use uuid::Uuid;
type Key=(String,Uuid,u64);
struct Request{method:String,args:Value,reply:oneshot::Sender<Result<Value,String>>}
struct Lease{id:Uuid,requests:mpsc::Sender<Request>,stop:watch::Sender<bool>}
impl Drop for Lease{fn drop(&mut self){let _=self.stop.send(true);}}
#[derive(Default)]
pub(crate) struct Manager(Mutex<HashMap<Key,Arc<Lease>>>);
pub(crate) fn binding(state:&AppState,pane:Uuid)->Result<Option<(Uuid,ForeignPaneBinding)>,String>{
    let graph=state.workspaces.lock().map_err(|_|"workspace unavailable")?;
    let found=graph.workspace_ids().find_map(|id|graph.workspace(*id)?.foreign_panes.get(&pane).cloned().map(|binding|(*id,binding)));Ok(found)
}
fn rewrite(bytes:&[u8],binding:&ForeignPaneBinding,local:&PaneRef,activation:u64)->Result<Vec<u8>,String>{
    let mut frame=decode_frame(bytes).map_err(|error|error.to_string())?;
    if frame.pane.workspace_id!=binding.workspace_id.to_string()||frame.pane.pane_id!=binding.session_id.to_string()||frame.activation_id!=activation{return Err("foreign terminal identity mismatch".into());}
    frame.pane=local.clone();encode_frame(&frame).map_err(|error|error.to_string())
}
impl Manager {
    pub(crate) async fn stop(&self){let mut leases=self.0.lock().await;for lease in leases.values(){let _=lease.stop.send(true);}leases.clear();}
    pub(crate) async fn request(&self,state:&AppState,pane:Uuid,args:&Value)->Result<Value,String>{
        let (workspace,bound)=binding(state,pane)?.ok_or("foreign view unavailable")?;
        let method=args["method"].as_str().ok_or("missing terminal method")?;
        let mut params=args["params"].clone();
        let nested=match method{"write_terminal_input"=>Some("input"),"get_terminal_history"=>Some("request"),"get_terminal_snapshot"=>None,_=>return Err("unsupported foreign terminal method".into())};
        let request=if let Some(key)=nested{&params[key]}else{&params};
        let activation=request["activationId"].as_u64().filter(|id|*id>0&&*id<=9_007_199_254_740_991).ok_or("invalid activation")?;
        let reference=if nested.is_some(){&request["pane"]}else{request};
        if reference["workspaceId"]!=workspace.to_string()||reference["paneId"]!=pane.to_string(){return Err("foreign terminal binding mismatch".into());}
        let connection=args["connectionId"].as_str().ok_or("terminal connection required")?;
        let key=(connection.into(),pane,activation);
        let sender=self.0.lock().await.get(&key).map(|lease|lease.requests.clone()).ok_or("foreign terminal subscription unavailable")?;
        if let Some(key)=nested{params[key]["pane"]=json!({"workspaceId":bound.workspace_id,"paneId":bound.session_id});}else{params["workspaceId"]=json!(bound.workspace_id);params["paneId"]=json!(bound.session_id);}
        let (reply,result)=oneshot::channel();sender.try_send(Request{method:method.into(),args:params,reply}).map_err(|_|"foreign terminal queue unavailable")?;
        let result=tokio::time::timeout(Duration::from_secs(10),result).await.map_err(|_|"foreign terminal request timed out")?.map_err(|_|"foreign terminal connection closed")??;
        if method=="write_terminal_input"{return Ok(result);}
        if binding(state,pane)?.is_none_or(|(current,current_binding)|current!=workspace||current_binding!=bound){return Err("foreign terminal binding changed".into());}
        let bytes=base64::engine::general_purpose::STANDARD.decode(result.as_str().ok_or("invalid foreign terminal frame")?).map_err(|_|"invalid foreign terminal frame")?;
        Ok(json!(base64::engine::general_purpose::STANDARD.encode(rewrite(&bytes,&bound,&PaneRef::new(workspace.to_string(),pane.to_string()),activation)?)))
    }
}
#[derive(Deserialize)]
pub(crate) struct StreamQuery{pub activation:u64,pub connection:String,pub workspace:Option<Uuid>}
pub(crate) async fn stream(State(state):State<AppState>,headers:HeaderMap,Path(pane):Path<String>,Query(query):Query<StreamQuery>,upgrade:WebSocketUpgrade)->Result<Response,StatusCode>{
    if headers.get("x-ridge-kernel-token").and_then(|value|value.to_str().ok())!=Some(state.token.as_str()){return Err(StatusCode::UNAUTHORIZED);}
    if query.connection.is_empty()||query.connection.len()>128||query.activation==0||query.activation>9_007_199_254_740_991{return Err(StatusCode::BAD_REQUEST);}
    let pane=Uuid::parse_str(&pane).map_err(|_|StatusCode::BAD_REQUEST)?;
    let (workspace,bound)=binding(&state,pane).map_err(|_|StatusCode::CONFLICT)?.ok_or(StatusCode::NOT_FOUND)?;
    if query.workspace.is_some_and(|expected|expected!=workspace){return Err(StatusCode::CONFLICT);}
    let (mut remote,host_stop)=state.outbound_hosts.terminal_socket(&bound.host_id).await.map_err(|_|StatusCode::CONFLICT)?;
    remote.send(cloud_ws::tungstenite::Message::Text(json!({"type":"subscribe-pane","workspaceId":bound.workspace_id,"paneId":bound.session_id,"activationId":query.activation,"active":true}).to_string())).await.map_err(|_|StatusCode::BAD_GATEWAY)?;
    let key=(query.connection,pane,query.activation);let (tx,rx)=mpsc::channel(64);let (stop,stop_rx)=watch::channel(false);
    let lease=Arc::new(Lease{id:Uuid::new_v4(),requests:tx,stop});
    let mut leases=state.foreign_terminals.0.lock().await;
    if leases.len()>=256&&!leases.contains_key(&key){return Err(StatusCode::TOO_MANY_REQUESTS);}
    if let Some(previous)=leases.insert(key.clone(),lease.clone()){let _=previous.stop.send(true);}drop(leases);
    Ok(upgrade.max_message_size(ridge_term::terminal_protocol::MAX_FRAME_BYTES).on_upgrade(move|socket|run(socket,remote,state,bound,workspace,pane,key,lease,rx,stop_rx,host_stop)))
}
async fn run(mut local:WebSocket,mut remote:crate::outbound_hosts::Socket,state:AppState,bound:ForeignPaneBinding,workspace:Uuid,pane:Uuid,key:Key,lease:Arc<Lease>,mut requests:mpsc::Receiver<Request>,mut stop:watch::Receiver<bool>,mut host_stop:watch::Receiver<bool>){
    use cloud_ws::tungstenite::Message as Remote;
    let local_ref=PaneRef::new(workspace.to_string(),pane.to_string());
    let mut pending=HashMap::<u64,oneshot::Sender<Result<Value,String>>>::new();let mut sequence=0u64;
    let mut tick=tokio::time::interval(Duration::from_millis(100));
    while !*stop.borrow()&&!*host_stop.borrow(){tokio::select!{
        _=stop.changed()=>break,_=host_stop.changed()=>break,
        _=tick.tick()=>{if binding(&state,pane).ok().flatten().is_none_or(|(id,binding)|id!=workspace||binding!=bound){break;}pending.retain(|_,reply|!reply.is_closed());},
        incoming=local.recv()=>match incoming{Some(Ok(Message::Ping(bytes)))=>{if local.send(Message::Pong(bytes)).await.is_err(){break;}},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,_=>{}},
        request=requests.recv()=>{let Some(request)=request else{break;};if pending.len()>=64{let _=request.reply.send(Err("foreign terminal in-flight bound reached".into()));continue;}if binding(&state,pane).ok().flatten().is_none_or(|(current,binding)|current!=workspace||binding!=bound){break;}sequence+=1;pending.insert(sequence,request.reply);if remote.send(Remote::Text(json!({"jsonrpc":"2.0","id":sequence,"method":request.method,"params":request.args}).to_string())).await.is_err(){break;}},
        incoming=remote.next()=>match incoming{
            Some(Ok(Remote::Binary(bytes))) if bytes.first()==Some(&0x13)=>{if binding(&state,pane).ok().flatten().is_none_or(|(current,binding)|current!=workspace||binding!=bound){break;}let Ok(bytes)=rewrite(&bytes,&bound,&local_ref,key.2)else{break;};if local.send(Message::Binary(bytes)).await.is_err(){break;}},
            Some(Ok(Remote::Text(text)))=>{if let Ok(mut value)=serde_json::from_str::<Value>(&text){
                if value["type"]=="event"&&value["name"]=="pane-meta-changed"&&value["payload"]["workspaceId"]==bound.workspace_id.to_string()
                    &&value["payload"]["paneId"]==bound.session_id.to_string()&&value["payload"]["activationId"]==key.2 {
                    if binding(&state,pane).ok().flatten().is_none_or(|(current,binding)|current!=workspace||binding!=bound){break;}
                    value["payload"]["workspaceId"]=json!(workspace);value["payload"]["paneId"]=json!(pane);
                    if local.send(Message::Text(value.to_string())).await.is_err(){break;}
                }else if let Some(reply)=value["id"].as_u64().and_then(|id|pending.remove(&id)){let _=reply.send(if value.get("error").is_some(){Err("remote terminal request rejected".into())}else{Ok(value["result"].clone())});}
            }},
            Some(Ok(Remote::Ping(bytes)))=>{if remote.send(Remote::Pong(bytes)).await.is_err(){break;}},Some(Ok(Remote::Close(_)))|None|Some(Err(_))=>break,_=>{},
        },
    }}
    {let mut leases=state.foreign_terminals.0.lock().await;if leases.get(&key).is_some_and(|current|current.id==lease.id){leases.remove(&key);}}
    let _=remote.close(None).await;let _=local.close().await;
}
