//! Kernel-owned authenticated outbound connections. Credentials never enter topology storage.
use crate::server::AppState;
use cloud_ws::{tungstenite::{Message,client::IntoClientRequest},MaybeTlsStream,WebSocketStream};
use futures::{SinkExt,StreamExt};
use ridge_core::{remote::{HostKind,HostRecord,HostSessionMeta,HostStatus},EventSink};
use serde_json::{json,Value};
use std::{collections::HashMap,sync::Arc,time::Duration};
use tokio::{net::TcpStream,sync::{mpsc,oneshot,Mutex}};
use uuid::Uuid;
pub(crate) enum Socket {Lan(WebSocketStream<MaybeTlsStream<TcpStream>>),Cloud(crate::cloud::controller::Socket)}
impl Socket {
    fn identity_warning(&self)->Option<&str>{match self{Self::Lan(_)=>None,Self::Cloud(socket)=>socket.identity_warning()}}
    pub(crate) async fn send(&mut self,message:Message)->Result<(),String>{match self{Self::Lan(socket)=>socket.send(message).await.map_err(|_|"outbound write failed".into()),Self::Cloud(socket)=>socket.send(message).await}}
    pub(crate) async fn next(&mut self)->Option<Result<Message,String>>{match self{Self::Lan(socket)=>socket.next().await.map(|frame|frame.map_err(|_|"outbound read failed".into())),Self::Cloud(socket)=>socket.next().await}}
    pub(crate) async fn close(&mut self,_:Option<()>)->Result<(),String>{match self{Self::Lan(socket)=>socket.close(None).await.map_err(|_|"outbound close failed".into()),Self::Cloud(socket)=>{socket.close().await;Ok(())}}}
}
struct Request {method:String,args:Value,reply:oneshot::Sender<Result<Value,String>>}
#[derive(Clone)]
enum Connection {Lan{url:String,token:String,tls:Arc<rustls::ClientConfig>},Cloud(crate::cloud::controller::Config,Arc<ridge_core::DeviceIdentity>)}
impl Connection {
    async fn open(&self)->Result<Socket,String>{
        let Self::Lan{url,token,tls}=self else {let Self::Cloud(config,identity)=self else{unreachable!()};return crate::cloud::controller::open(config.clone(),identity.clone()).await.map(Socket::Cloud);};
        let mut request=url.as_str().into_client_request().map_err(|_|"invalid outbound request")?;
        request.headers_mut().insert("authorization",format!("Bearer {}",token).parse().map_err(|_|"invalid session credential")?);
        let config=cloud_ws::tungstenite::protocol::WebSocketConfig{max_message_size:Some(ridge_term::terminal_protocol::MAX_FRAME_BYTES),max_frame_size:Some(ridge_term::terminal_protocol::MAX_FRAME_BYTES),max_write_buffer_size:2*ridge_term::terminal_protocol::MAX_FRAME_BYTES,..Default::default()};
        tokio::time::timeout(Duration::from_secs(10),cloud_ws::connect_async_tls_with_config(request,Some(config),false,Some(cloud_ws::Connector::Rustls(tls.clone())))).await.map_err(|_|"outbound handshake timed out")?.map(|value|Socket::Lan(value.0)).map_err(|_|"outbound TLS/WebSocket handshake failed".into())
    }
}
struct Peer {requests:mpsc::Sender<Request>,task:tokio::task::JoinHandle<()>,connection:Connection,stop:tokio::sync::watch::Sender<bool>}
impl Drop for Peer {fn drop(&mut self){self.stop.send_replace(true);self.task.abort();}}
#[derive(Default)]
pub struct Manager {control:Mutex<()>,peers:Mutex<HashMap<String,Peer>>}
async fn rpc(socket:&mut Socket,id:u64,method:&str,args:Value)->Result<Value,String> {
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":id,"method":method,"params":args}).to_string())).await.map_err(|_|"outbound write failed")?;
    tokio::time::timeout(Duration::from_secs(10),async {
        while let Some(frame)=socket.next().await {
            match frame.map_err(|_|"outbound read failed")? {
                Message::Text(text)=>{let Ok(value)=serde_json::from_str::<Value>(&text) else {continue;};
                    if value["id"]==id {return if value.get("error").is_some(){Err(format!("remote command rejected: {method} ({})",value["error"]["code"]))}else{Ok(value["result"].clone())};}},
                Message::Ping(bytes)=>socket.send(Message::Pong(bytes)).await.map_err(|_|"outbound pong failed")?,
                Message::Close(_)=>break,
                _=>{},
            }
        }Err("outbound connection closed".into())
    }).await.map_err(|_|"outbound response timed out".to_string())?
}
fn persist(state:&AppState,record:HostRecord)->Result<(),String> {
    let mut hosts=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?;
    let mut next=hosts.clone();next.upsert(record);
    crate::registry::save_remote_hosts_at(&state.remote_hosts_path,next.records()).map_err(|_|"host topology persistence failed")?;
    *hosts=next;drop(hosts);state.business_events.broadcast("host-list-changed",Value::Null);Ok(())
}
impl Manager {
    pub(crate) async fn terminal_socket(&self,id:&str)->Result<(Socket,tokio::sync::watch::Receiver<bool>),String>{
        let (connection,stop)={let peers=self.peers.lock().await;let peer=peers.get(id).filter(|peer|!peer.task.is_finished()).ok_or("host connection unavailable")?;(peer.connection.clone(),peer.stop.subscribe())};
        let socket=connection.open().await?;if *stop.borrow(){return Err("host disconnected".into());}Ok((socket,stop))
    }
    pub async fn stop(&self){self.peers.lock().await.clear();}
    async fn close(&self,id:&str){
        let previous=self.peers.lock().await.remove(id);
        if let Some(mut previous)=previous {previous.task.abort();let _=(&mut previous.task).await;}
    }
    pub async fn dispatch(&self,state:&AppState,method:&str,args:&Value,native:bool)->Option<Result<Value,String>> {
        if !matches!(method,"host_list_snapshot"|"connect_host"|"disconnect_host"|"forget_host"|"list_host_sessions"){return None;}
        if method=="list_host_sessions"&&!self.peers.lock().await.contains_key(args["hostId"].as_str().unwrap_or("")){return None;}
        Some(async {
            if !native{return Err("host connections require native trust".into());}
            if method=="host_list_snapshot" {
                let mut records=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?.snapshot();
                let live:std::collections::HashSet<_>=self.peers.lock().await.iter().filter(|(_,peer)|!peer.task.is_finished()).map(|(id,_)|id.clone()).collect();
                let mut result=Vec::new();
                for record in &mut records {
                    crate::foreign_panes::mark_attached(state,&record.id,&mut record.sessions)?;
                    let mut value=serde_json::to_value(&record).map_err(|_|"invalid host projection")?;
                    if live.contains(&record.id){value["runtimeManaged"]=json!(true);}
                    result.push(value);
                }return Ok(json!(result));
            }
            let _control=self.control.lock().await;
            if method=="connect_host" {return self.connect(state,args).await;}
            let id=args["hostId"].as_str().ok_or("missing hostId")?;
            if method=="list_host_sessions" {
                let mut record=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?.get(id).ok_or("unknown host")?;
                let workspaces=self.request(id,"list_workspaces",json!({})).await?;
                let workspaces=workspaces.as_array().filter(|rows|rows.len()<=4096).ok_or("invalid remote workspace list")?;
                let mut sessions=Vec::new();
                for workspace in workspaces {
                    let snapshot=self.request(id,"get_workspace_snapshot",json!({"workspaceId":workspace["id"]})).await?;
                    append_sessions(&mut sessions,&snapshot)?;
                }
                for session in &mut sessions {session.attached=record.sessions.iter().any(|old|old.id==session.id&&old.attached);}
                record.sessions=sessions;let result=serde_json::to_value(&record.sessions).map_err(|_|"invalid host session list")?;
                persist(state,record)?;
                let mut result:Vec<HostSessionMeta>=serde_json::from_value(result).map_err(|_|"invalid host session list")?;
                crate::foreign_panes::mark_attached(state,id,&mut result)?;
                return serde_json::to_value(result).map_err(|_|"invalid host session list".into());
            }
            {
            let mut hosts=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?;
            let mut next=hosts.clone();let mut record=next.get(id).ok_or("unknown host")?;
            if method=="forget_host" {next.remove(id);}else{record.status=HostStatus::Disconnected;record.detail="已断开".into();for session in &mut record.sessions{session.attached=false;}next.upsert(record);}
            crate::registry::save_remote_hosts_at(&state.remote_hosts_path,next.records()).map_err(|_|"host topology persistence failed")?;
            *hosts=next;
            }
            self.close(id).await;
            state.business_events.broadcast("host-list-changed",Value::Null);Ok(Value::Null)
        }.await)
    }
    async fn connect(&self,state:&AppState,args:&Value)->Result<Value,String> {
        let kind=match args["kind"].as_str(){Some("rdg")=>HostKind::Rdg,Some("remote")=>HostKind::Remote,_=>return Err("invalid host kind".into())};
        let addr=args["addr"].as_str().ok_or("missing host address")?.trim();
        if addr.is_empty()||addr.len()>4096||args.get("label").is_some_and(|label|!label.is_null()&&(!label.is_string()||label.as_str().is_some_and(|label|label.len()>4096))){return Err("invalid host registration bounds".into());}
        let cloud=args.get("cloud").filter(|value|!value.is_null()).map(|value|serde_json::from_value::<crate::cloud::controller::Config>(value.clone()).map_err(|_|"invalid Cloud controller configuration")).transpose()?;
        if let Some(config)=&cloud{config.validate()?;if config.device_name!=addr{return Err("Cloud device identity does not match host address".into());}}
        let controller_identity = if cloud.is_some() { Some(state.controller_identity.get()?) } else { None };
        let mut url=url::Url::parse(&if cloud.is_some(){"https://localhost".into()}else if addr.contains("://"){addr.to_owned()}else{format!("https://{addr}")}).map_err(|_|"invalid host address")?;
        if !url.username().is_empty()||url.password().is_some()||url.query().is_some()||url.fragment().is_some(){return Err("host address must not contain credentials, query or fragment".into());}
        let local=url.host_str().is_some_and(|host|host=="localhost"||host=="127.0.0.1"||host=="[::1]");
        if !matches!(url.scheme(),"http"|"https"|"ws"|"wss")||matches!(url.scheme(),"http"|"ws")&&!local {return Err("outbound host requires TLS".into());}
        let secure=matches!(url.scheme(),"https"|"wss");
        url.set_scheme(if secure{"https"}else{"http"}).map_err(|_|"invalid host scheme")?;url.set_path("/");
        let credential=cloud.as_ref().map(|config|config.code.as_str()).or_else(||args["token"].as_str()).filter(|token|!token.trim().is_empty()).ok_or("host TOTP required")?.trim().to_owned();
        if credential.len()>4096{return Err("invalid host credential bounds".into());}
        let id=format!("{}:{}",if cloud.is_some(){"cloud"}else if kind==HostKind::Rdg{"rdg"}else{"lan"},addr);
        let label=args["label"].as_str().map(str::trim).filter(|label|!label.is_empty()).unwrap_or(addr).to_owned();
        if self.peers.lock().await.get(&id).is_some_and(|peer|!peer.task.is_finished()&&match (&peer.connection,&cloud){(Connection::Lan{..},None)=>!*peer.stop.borrow(),(Connection::Cloud(old,_),Some(new))=>old.username==new.username&&old.base_domain==new.base_domain&&old.plaintext==new.plaintext,_=>false}) {
            let mut record=state.remote_hosts.lock().map_err(|_|"host topology unavailable")?.get(&id).ok_or("connected host topology unavailable")?;
            if record.label!=label {record.label=label;persist(state,record)?;}
            if let Some(config)=cloud {if let Some(peer)=self.peers.lock().await.get_mut(&id){peer.connection=Connection::Cloud(config,controller_identity.clone().ok_or("controller identity unavailable")?);}}
            return Ok(json!(id));
        }
        let mut record=HostRecord{id:id.clone(),kind,label,addr:addr.into(),status:HostStatus::Connecting,detail:"认证与协议协商".into(),sessions:Vec::new()};
        self.close(&id).await;
        persist(state,record.clone())?;
        let connected=async {
            let connection=if let Some(config)=cloud {Connection::Cloud(config,controller_identity.ok_or("controller identity unavailable")?)}else{
            let device=Uuid::new_v4().to_string();
            let tls=crate::client::lan_tls_config()?;
            // Authentication and WebSocket use one strict trust snapshot.
            let builder=reqwest::Client::builder().use_preconfigured_tls((*tls).clone()).no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(10));
            let http=builder.build().map_err(|_|"outbound HTTP initialization failed")?;
            let verify:Value=http.post(url.join("verify").map_err(|_|"invalid verify endpoint")?).form(&[("code",credential.as_str()),("device",device.as_str())]).send().await.map_err(|_|"outbound authentication transport failed")?.error_for_status().map_err(|_|"outbound authentication rejected")?.json().await.map_err(|_|"invalid authentication response")?;
            let token=verify["token"].as_str().filter(|token|!token.is_empty()).ok_or("outbound authentication rejected")?;
            url.set_scheme(if secure{"wss"}else{"ws"}).map_err(|_|"invalid WebSocket scheme")?;url.set_path("/ws");url.query_pairs_mut().append_pair("device",&device);
            Connection::Lan{url:url.to_string(),token:token.into(),tls}
            };
            let mut socket=connection.open().await?;
            let hello=rpc(&mut socket,1,"$/hello",json!({"protocolVersion":ridge_remote::rpc::HOST_PROTOCOL_VERSION,"terminalProtocolVersion":ridge_term::terminal_protocol::PROTOCOL_VERSION,"capabilities":["pane","invoke","workspace"]})).await?;
            if hello["protocolVersion"]!=ridge_remote::rpc::HOST_PROTOCOL_VERSION{return Err("outbound protocol version mismatch".to_string());}
            let rows=rpc(&mut socket,2,"list_workspaces",json!({})).await?;
            let rows=rows.as_array().ok_or("invalid remote workspace list")?;
            if rows.len()>4096{return Err("remote workspace list exceeds bound".into());}
            let mut sessions=Vec::new();
            for (index,row) in rows.iter().enumerate(){
                let wid=row["id"].as_str().ok_or("invalid remote workspace identity")?;
                let snapshot=rpc(&mut socket,3+index as u64,"get_workspace_snapshot",json!({"workspaceId":wid})).await?;
                append_sessions(&mut sessions,&snapshot)?;
            }
            Ok::<_,String>((socket,sessions,connection))
        }.await;
        let (mut socket,sessions,connection)=match connected {Ok(value)=>value,Err(error)=>{record.status=HostStatus::Error;record.detail=error;persist(state,record)?;return Ok(json!(id));}};
        record.status=HostStatus::Connected;record.detail=format!("已认证 · 共同 Host 协议{}",socket.identity_warning().map(|warning|format!(" · {warning}")).unwrap_or_default());record.sessions=sessions;persist(state,record.clone())?;
        let (tx,mut rx)=mpsc::channel::<Request>(64);
        let (stop,_)=tokio::sync::watch::channel(false);let ended=stop.clone();
        let owner=Arc::downgrade(&state.outbound_hosts);
        let topology=state.remote_hosts.clone();let path=state.remote_hosts_path.clone();let events=state.business_events.clone();
        let task=tokio::spawn(async move {
            let mut sequence=10000;let mut ended=ended;
            let update=|status:HostStatus,detail:&str|->Result<(),String>{
                let mut hosts=topology.lock().map_err(|_|"host topology unavailable")?;let mut next=hosts.clone();let mut current=next.get(&record.id).ok_or("host forgotten")?;
                current.status=status;current.detail=detail.into();next.upsert(current);crate::registry::save_remote_hosts_at(&path,next.records()).map_err(|_|"host topology persistence failed")?;*hosts=next;events.broadcast("host-list-changed",Value::Null);Ok(())
            };
            'service: loop {
                let dropped=loop {tokio::select! {
                    request=rx.recv()=>{let Some(request)=request else{break false;};if request.reply.is_closed(){continue;}sequence+=1;let result=rpc(&mut socket,sequence,&request.method,request.args).await;let failed=result.as_ref().is_err_and(|error|!error.starts_with("remote command rejected:"));let _=request.reply.send(result);if failed{break true;}},
                    frame=socket.next()=>match frame {Some(Ok(Message::Ping(bytes)))=>{if socket.send(Message::Pong(bytes)).await.is_err(){break true;}},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break true,_=>{}},
                }};
                let _=socket.close(None).await;ended.send_replace(true);
                if !dropped{break;}
                if update(HostStatus::Connecting,"Host 连接中断，Runtime 正在恢复").is_err(){break;}
                for attempt in 0..5 {
                    tokio::time::sleep(Duration::from_millis(250u64<<attempt)).await;
                    let latest=if let Some(owner)=owner.upgrade(){owner.peers.lock().await.get(&record.id).map(|peer|peer.connection.clone())}else{None};
                    let Some(latest)=latest else{break 'service;};
                    let Ok(mut resumed)=latest.open().await else{continue;};
                    let hello=rpc(&mut resumed,1,"$/hello",json!({"protocolVersion":ridge_remote::rpc::HOST_PROTOCOL_VERSION,"terminalProtocolVersion":ridge_term::terminal_protocol::PROTOCOL_VERSION,"capabilities":["pane","invoke","workspace"]})).await;
                    if !hello.is_ok_and(|hello|hello["protocolVersion"]==ridge_remote::rpc::HOST_PROTOCOL_VERSION){let _=resumed.close(None).await;continue;}
                    let detail=format!("已认证 · 共同 Host 协议{}",resumed.identity_warning().map(|warning|format!(" · {warning}")).unwrap_or_default());
                    if update(HostStatus::Connected,&detail).is_err(){let _=resumed.close(None).await;break 'service;}
                    let (fresh,_)=tokio::sync::watch::channel(false);
                    if let Some(owner)=owner.upgrade(){if let Some(peer)=owner.peers.lock().await.get_mut(&record.id){peer.stop=fresh.clone();}else{break 'service;}}else{break 'service;}
                    ended=fresh;socket=resumed;continue 'service;
                }
                break;
            }
            let _=socket.close(None).await;
            ended.send_replace(true);
            let _=update(HostStatus::Disconnected,"远端连接已关闭");
        });
        self.peers.lock().await.insert(id.clone(),Peer{requests:tx,task,connection,stop});Ok(json!(id))
    }
    pub(crate) async fn request(&self,id:&str,method:&str,args:Value)->Result<Value,String>{
        let sender=self.peers.lock().await.get(id).map(|peer|peer.requests.clone()).ok_or("host connection unavailable")?;
        let (reply,rx)=oneshot::channel();sender.try_send(Request{method:method.into(),args,reply}).map_err(|_|"host request queue unavailable")?;
        tokio::time::timeout(Duration::from_secs(12),rx).await.map_err(|_|"host request timed out")?.map_err(|_|"host connection closed")?
    }
}
pub(crate) async fn websocket(
    ws:axum::extract::WebSocketUpgrade,
    axum::extract::State(state):axum::extract::State<AppState>,
    axum::extract::ConnectInfo(peer):axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::Path(id):axum::extract::Path<String>,
    headers:axum::http::HeaderMap,
    axum::extract::Query(query):axum::extract::Query<crate::local_client::ClientQuery>,
)->Result<axum::response::Response,axum::http::StatusCode>{
    let (host,binding,origin)=crate::local_client::authorize(&state,peer,&headers,&query.device_id)?;
    let peers=state.outbound_hosts.peers.lock().await;
    let peer=peers.get(&id).filter(|peer|!peer.task.is_finished()).ok_or(axum::http::StatusCode::CONFLICT)?;
    let connected=peer.connection.clone();let stop=peer.stop.subscribe();drop(peers);
    let remote=connected.open().await.map_err(|_|axum::http::StatusCode::BAD_GATEWAY)?;
    if *stop.borrow(){return Err(axum::http::StatusCode::CONFLICT);}
    Ok(ws.protocols(["ridge-client"]).max_message_size(ridge_term::terminal_protocol::MAX_FRAME_BYTES).on_upgrade(move|socket|relay(socket,remote,host,binding,origin,stop)))
}
async fn relay(mut local:axum::extract::ws::WebSocket,mut remote:Socket,host:Arc<crate::host::KernelHost>,binding:ridge_remote::host::WsConn,origin:String,mut stop:tokio::sync::watch::Receiver<bool>){
    use axum::extract::ws::Message as Local;
    let mut expiry=tokio::time::interval(Duration::from_secs(1));
    let authorized=||host.sessions.validate_origin_bound_token(binding.token.as_deref().unwrap_or(""),&binding.device_id,&binding.remote_addr,&origin);
    while !*stop.borrow() {tokio::select!{
        _=stop.changed()=>break,
        _=expiry.tick()=>{if !host.sessions.validate_origin_bound_token(binding.token.as_deref().unwrap_or(""),&binding.device_id,&binding.remote_addr,&origin){break;}},
        incoming=local.recv()=>{if !authorized(){break;}let Some(Ok(frame))=incoming else{break;};let frame=match frame{Local::Text(text)=>Message::Text(text),Local::Binary(bytes)=>Message::Binary(bytes),Local::Ping(bytes)=>Message::Ping(bytes),Local::Pong(bytes)=>Message::Pong(bytes),Local::Close(_)=>break};if remote.send(frame).await.is_err(){break;}},
        incoming=remote.next()=>{if !authorized(){break;}let Some(Ok(frame))=incoming else{break;};let frame=match frame{Message::Text(text)=>Local::Text(text),Message::Binary(bytes)=>Local::Binary(bytes),Message::Ping(bytes)=>Local::Ping(bytes),Message::Pong(bytes)=>Local::Pong(bytes),Message::Close(_)=>break,_=>continue};if local.send(frame).await.is_err(){break;}},
    }}
    let _=remote.close(None).await;let _=local.close().await;
}
fn append_sessions(sessions:&mut Vec<HostSessionMeta>,snapshot:&Value)->Result<(),String>{
    for pane in snapshot["panes"].as_array().ok_or("invalid remote pane list")? {
        if sessions.len()>=4096{return Err("remote session list exceeds bound".into());}
        let id=pane["id"].as_str().ok_or("invalid remote pane identity")?;
        if Uuid::parse_str(id).is_err()||sessions.iter().any(|session|session.id==id){return Err("invalid or duplicate remote pane identity".into());}
        sessions.push(HostSessionMeta{id:id.into(),title:pane["title"].as_str().unwrap_or("terminal").into(),attached:false});
    }Ok(())
}
