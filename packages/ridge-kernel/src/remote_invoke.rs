//! Transitional trusted transport adapter to the shared Host command executor.
use crate::server::AppState;
use serde_json::Value;
use std::{collections::HashMap,sync::{Arc,Mutex},time::{Duration,Instant}};
use uuid::Uuid;

struct Client { view:Arc<Mutex<Option<Uuid>>>, gate:Arc<tokio::sync::Mutex<()>>, connection:Uuid, subscriptions:crate::host::TerminalSubscriptions }
impl Default for Client {
    fn default()->Self {Self {view:Default::default(),gate:Default::default(),connection:Uuid::new_v4(),subscriptions:Default::default()}}
}
#[derive(Default)]
pub(crate) struct Clients(Mutex<HashMap<(String,String),(Arc<Client>,Instant)>>);
fn get_client(state:&AppState,connection:&str,client:&str)->Result<Arc<Client>,Value>{
    let error=|message:&str|ridge_core::CoreError::InvalidArgs(message.into()).to_json_rpc();
    if client.is_empty()||client.len()>128{return Err(error("invalid remote client identity"));}
    let mut clients=state.remote_invokes.0.lock().map_err(|_|error("remote clients unavailable"))?;
    clients.retain(|_,(_,updated)|updated.elapsed()<Duration::from_secs(24*3600));
    let key=(connection.to_owned(),client.to_owned());
    if !clients.contains_key(&key)&&clients.len()>=256{return Err(error("remote client capacity reached"));}
    let entry=clients.entry(key).or_insert_with(||(Default::default(),Instant::now()));entry.1=Instant::now();Ok(entry.0.clone())
}
pub(crate) async fn frame(state:AppState,connection:String,client:&str,frame:Value)->Result<Option<Value>,Value>{
    let client=get_client(&state,&connection,client)?;let guard=client.gate.clone().lock_owned().await;
    let host=crate::local_client::host(&state);let runtime=tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move||{let _guard=guard;crate::host::invoke_remote_frame(&host,&runtime,&client.view,&client.subscriptions,&client.connection.to_string(),&frame)})
        .await.map_err(|_|ridge_core::CoreError::InvalidArgs("remote frame worker failed".into()).to_json_rpc())?
}
pub(crate) fn stream_connection(state:&AppState,connection:&str,client:&str)->Result<String,String>{
    state.remote_invokes.0.lock().map_err(|_|"remote clients unavailable")?.get(&(connection.into(),client.into())).map(|(client,_)|client.connection.to_string()).ok_or("remote client binding unavailable".into())
}
pub(crate) async fn execute(state:AppState,args:Value,native:bool,connection:String,binding:bool)->Result<Value,Value> {
    let error=|message:String|ridge_core::CoreError::InvalidArgs(message).to_json_rpc();
    if !native {return Err(error("remote transport adapter requires native trust".into()));}
    let method=if binding {String::new()} else {args["method"].as_str().ok_or_else(||error("missing remote method".into()))?.to_owned()};
    let client=args["clientId"].as_str().unwrap_or("legacy");
    let client=get_client(&state,&connection,client)?;
    if binding {
        let _guard=client.gate.lock().await;
        let workspace=Uuid::parse_str(args["workspaceId"].as_str().ok_or_else(||error("missing workspace".into()))?).map_err(|e|error(e.to_string()))?;
        let pane=Uuid::parse_str(args["paneId"].as_str().ok_or_else(||error("missing pane".into()))?).map_err(|e|error(e.to_string()))?;
        let activation=args["activationId"].as_u64().filter(|id|*id>0).ok_or_else(||error("invalid terminal activation".into()))?;
        let active=args["active"].as_bool().ok_or_else(||error("missing terminal binding state".into()))?;
        let mut subscriptions=client.subscriptions.lock().map_err(|_|error("terminal bindings unavailable".into()))?;
        if active {
            if !state.ptys.list().iter().any(|pty|pty.id==pane&&pty.workspace_id==Some(workspace))&&crate::foreign_terminal::binding(&state,pane).map_err(error)?.is_none_or(|(id,_)|id!=workspace) {return Err(error("terminal outside workspace".into()));}
            if subscriptions.len()>=256&&!subscriptions.contains_key(&(workspace,pane)) {return Err(error("terminal binding capacity reached".into()));}
            if subscriptions.get(&(workspace,pane)).is_some_and(|current|current.activation>activation) {return Err(error("terminal activation changed".into()));}
            subscriptions.insert((workspace,pane),crate::host::TerminalSubscription{activation,lease:Uuid::new_v4()});
        } else if subscriptions.get(&(workspace,pane)).is_some_and(|current|current.activation==activation) {subscriptions.remove(&(workspace,pane));}
        return Ok(Value::Null);
    }
    let host=crate::local_client::host(&state);
    let runtime=tokio::runtime::Handle::current();
    let guard=client.gate.clone().lock_owned().await;
    tokio::task::spawn_blocking(move||{
        let _guard=guard;
        crate::host::invoke_remote_command(&host,&runtime,&client.view,&client.subscriptions,&client.connection.to_string(),&method,&args["args"])
    })
        .await.map_err(|_|error("remote command worker failed".into()))?
}
