//! Kernel owns the Cloud listener, reconnection loop and controller peers.
use super::{connection,ice,signaling::{Signaling,SignalMsg,Role},workspace_scope::Scope};
use crate::server::AppState;
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::{HashMap,HashSet},sync::Mutex,time::Duration};
use tokio::sync::{mpsc,oneshot};

#[derive(Clone,Deserialize,PartialEq,Eq)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
pub struct Config {#[serde(alias="token")]pub device_token:String,pub device_name:String,pub username:String,#[serde(default="default_domain")]pub base_domain:String,#[serde(default)]pub root:Option<std::path::PathBuf>,#[serde(default)]pub plaintext:bool}
fn default_domain()->String {std::env::var("RIDGE_BASE_DOMAIN").ok().filter(|domain|!domain.is_empty()).unwrap_or_else(||"9527127.xyz".into())}
impl Config {
    pub(crate) fn local(&self)->bool {url::Url::parse(&format!("https://{}",self.base_domain)).ok().and_then(|url|url.host_str().map(str::to_owned)).is_some_and(|host|host=="localhost"||host.ends_with(".localhost")||host=="[::1]"||host.parse::<std::net::IpAddr>().is_ok_and(|ip|ip.is_loopback()))}
    pub fn api_base(&self)->String {format!("{}://{}/api/v1",if self.local()&&self.plaintext{"http"}else{"https"},self.base_domain)}
    pub(crate) fn signal_url(&self)->Result<String,String> {
        if self.plaintext&&!self.local(){return Err("public Cloud endpoints require TLS".into());}
        if self.root.as_ref().is_some_and(|root|!root.is_absolute()||ridge_core::dispatch::path_has_traversal(&root.to_string_lossy())) {return Err("Cloud serving root must be absolute without traversal".into());}
        for name in [&self.device_name,&self.username] {if name.is_empty()||name.len()>100||!name.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'-'|b'_')) {return Err("invalid Cloud host identity".into());}}
        if self.device_token.is_empty()||self.device_token.len()>8192 {return Err("invalid Cloud device credential".into());}
        let base=url::Url::parse(&format!("https://{}",self.base_domain)).map_err(|_|"invalid Cloud base domain")?;
        if !base.username().is_empty()||base.password().is_some()||base.path()!="/"||base.query().is_some()||base.fragment().is_some(){return Err("invalid Cloud base domain".into());}
        let mut url=url::Url::parse(&format!("{}://{}-{}.{}/ws",if self.local()&&self.plaintext{"ws"}else{"wss"},self.device_name,self.username,self.base_domain)).map_err(|_|"invalid Cloud signaling endpoint")?;
        url.query_pairs_mut().append_pair("token",&self.device_token).append_pair("role","host");Ok(url.into())
    }
}
enum Action {Kick(String,bool)}
struct Worker {config:Config,stop:oneshot::Sender<()>,actions:mpsc::Sender<Action>,task:tokio::task::JoinHandle<()>}
#[derive(Default)]
pub struct Manager {worker:tokio::sync::Mutex<Option<Worker>>,control:tokio::sync::Mutex<()>,snapshot:Mutex<Value>}
struct Peer {stop:Option<oneshot::Sender<()>>,input:mpsc::Sender<SignalMsg>,task:tokio::task::JoinHandle<()>,status:String,created:u64}
impl Drop for Peer {fn drop(&mut self){if let Some(stop)=self.stop.take(){let _=stop.send(());}self.task.abort();}}
impl Manager {
    fn publish(&self,state:&AppState,status:&str,error:&str,sessions:Vec<Value>) {
        let snapshot=json!({"state":status,"error":error,"sessions":sessions});
        if let Ok(mut current)=self.snapshot.lock(){*current=snapshot.clone();}
        use ridge_core::EventSink;state.business_events.broadcast("cloud-host-changed",snapshot);
    }
    pub async fn stop(&self) {
        let _guard=self.control.lock().await;
        self.stop_inner().await;
    }
    async fn stop_inner(&self) {
        if let Some(worker)=self.worker.lock().await.take() {
            let _=worker.stop.send(());let mut task=worker.task;
            if tokio::time::timeout(Duration::from_secs(5),&mut task).await.is_err(){task.abort();let _=task.await;}
        }
    }
    pub async fn dispatch(self:&std::sync::Arc<Self>,state:&AppState,method:&str,args:&Value,native:bool)->Option<Result<Value,String>> {
        if !matches!(method,"cloud_host_online"|"cloud_host_offline"|"get_cloud_host_state"|"cloud_host_kick"|"cloud_host_blacklist") {return None;}
        if !native {return Some(Err("Cloud host control requires native trust".into()));}
        Some(async {
            let _guard=self.control.lock().await;
            match method {
                "get_cloud_host_state"=>{
                    let snapshot=self.snapshot.lock().map_err(|_|"Cloud host state unavailable")?.clone();
                    Ok(if snapshot.is_null(){json!({"state":"offline","error":"","sessions":[]})}else{snapshot})
                }
                "cloud_host_offline"=>{self.stop_inner().await;self.publish(state,"offline","",vec![]);Ok(Value::Null)}
                "cloud_host_online"=>{
                    if !cfg!(feature="rtc"){return Err("Cloud RTC disabled in this Runtime build".into());}
                    let config:Config=serde_json::from_value(args.clone()).map_err(|_|"invalid Cloud host configuration")?;config.signal_url()?;
                    {let worker=self.worker.lock().await;if worker.as_ref().is_some_and(|worker|worker.config==config&&!worker.task.is_finished()){return Ok(Value::Null);}}
                    if state.shutting_down.load(std::sync::atomic::Ordering::Acquire){return Err("Runtime shutting down".into());}
                    self.stop_inner().await;
                    state.remote_auth.call("remote_set_totp_identity",json!({"username":config.username}))?;
                    self.publish(state,"connecting","",vec![]);
                    let (stop,stopped)=oneshot::channel();let (actions,receiver)=mpsc::channel(64);
                    let owner=self.clone();let state=state.clone();let cfg=config.clone();
                    let task=tokio::spawn(async move {owner.run(state,cfg,stopped,receiver).await;});
                    *self.worker.lock().await=Some(Worker {config,stop,actions,task});Ok(Value::Null)
                }
                _=>{
                    let cid=args["cid"].as_str().filter(|id|!id.is_empty()&&id.len()<=128).ok_or("invalid Cloud controller identity")?;
                    let worker=self.worker.lock().await;let worker=worker.as_ref().filter(|worker|!worker.task.is_finished()).ok_or("Cloud host offline")?;
                    worker.actions.send(Action::Kick(cid.into(),method=="cloud_host_blacklist")).await.map_err(|_|"Cloud host offline")?;Ok(Value::Null)
                }
            }
        }.await)
    }
    async fn run(self:std::sync::Arc<Self>,state:AppState,config:Config,mut stopped:oneshot::Receiver<()>,mut actions:mpsc::Receiver<Action>) {
        let builder=reqwest::Client::builder().timeout(Duration::from_secs(15)).redirect(reqwest::redirect::Policy::none()).danger_accept_invalid_certs(config.local());
        let builder=if config.local(){builder.no_proxy()}else{builder};
        let http=match builder.build(){Ok(client)=>client,Err(_)=>{self.publish(&state,"error","Cloud HTTP client unavailable",vec![]);return;}};
        let mut attempts=0u32;
        loop {
            let url=match config.signal_url(){Ok(url)=>url,Err(error)=>{self.publish(&state,"error",&error,vec![]);return;}};
            let connected=tokio::select! {_=&mut stopped=>break, result=Signaling::<Value>::connect(&url)=>result};
            let mut signaling=match connected {Ok(signal)=>signal,Err(_)=>{
                self.publish(&state,"error","Cloud signaling connection failed",vec![]);attempts+=1;
                tokio::select! {_=&mut stopped=>break,_=tokio::time::sleep(Duration::from_secs((1u64<<attempts.min(5)).min(30)))=>{}}continue;
            }};
            let ice=ice::fetch_ice_servers(&http,&config.device_token,&config.api_base()).await;
            let sender=signaling.sender();let mut peers=HashMap::<String,Peer>::new();let mut scopes=HashMap::<String,Scope>::new();let mut denied=HashSet::new();let mut status="connecting";
            let mut early=HashMap::<String,Vec<SignalMsg>>::new();
            let (events,mut statuses)=mpsc::channel::<(String,String)>(64);
            let mut heartbeat=tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _=&mut stopped=>{close_peers(&mut peers).await;self.publish(&state,"offline","",vec![]);return;}
                    _=heartbeat.tick()=>{peers.retain(|_,peer|!peer.task.is_finished());self.publish(&state,status,"",sessions(&peers));}
                    update=statuses.recv()=>if let Some((cid,status_update))=update {if let Some(peer)=peers.get_mut(&cid){peer.status=status_update;}self.publish(&state,status,"",sessions(&peers));},
                    action=actions.recv()=>if let Some(Action::Kick(cid,blacklist))=action {
                        if blacklist {denied.insert(cid.clone());}
                        if let Some(peer)=peers.remove(&cid){close_peer(peer).await;}
                        let _=sender.send(SignalMsg::Kick{cid}).await;
                    },
                    frame=signaling.incoming.recv()=>{
                        let Some(frame)=frame else {break;};
                        let cid=frame["cid"].as_str().filter(|cid|!cid.is_empty()&&cid.len()<=128).map(str::to_owned);
                        if frame["t"]=="workspace-scope" {
                            if let (Some(cid),Some(token))=(cid,frame["token"].as_str().filter(|token|!token.is_empty()&&token.len()<=8192)) {
                                if scopes.len()>=64&&!scopes.contains_key(&cid){let _=sender.send(SignalMsg::Kick{cid}).await;continue;}
                                let checked=http.post(format!("{}/workspace-shares/verify-access",config.api_base())).bearer_auth(&config.device_token).json(&json!({"token":token})).send().await;
                                let scope=match checked {Ok(response) if response.status().is_success()=>response.json::<Value>().await.ok().filter(|response|response["ok"]==true).and_then(|response|serde_json::from_value::<Scope>(response["data"].clone()).ok()),_=>None};
                                if let Some(scope)=scope.filter(|scope|scope.device_name==config.device_name&&scope.role=="operator"&&!scope.delegable) {scopes.insert(cid,scope);} else {denied.insert(cid.clone());let _=sender.send(SignalMsg::Kick{cid}).await;}
                            }continue;
                        }
                        if frame["t"]=="welcome" {status="online";attempts=0;}
                        if frame["t"]=="error" {self.publish(&state,"error","Cloud relay refused the host",vec![]);break;}
                        let Some(cid)=cid else {self.publish(&state,status,"",sessions(&peers));continue;};
                        if denied.contains(&cid){let _=sender.send(SignalMsg::Kick{cid}).await;continue;}
                        let Ok(signal)=serde_json::from_value::<SignalMsg>(frame) else {continue;};
                        if matches!(signal,SignalMsg::PeerLeave{role:Role::Controller,..}) {if let Some(peer)=peers.remove(&cid){close_peer(peer).await;}scopes.remove(&cid);early.remove(&cid);}
                        else {
                            if matches!(signal,SignalMsg::Offer{..})&&!peers.contains_key(&cid) {
                                if peers.len()>=64 {let _=sender.send(SignalMsg::Kick{cid}).await;continue;}
                                let (input,incoming)=mpsc::channel(64);let (stop,stopped)=oneshot::channel();
                                if let Some(buffer)=early.remove(&cid){for signal in buffer {let _=input.send(signal).await;}}
                                let st=state.clone();let cfg=config.clone();let id=cid.clone();let signals=sender.clone();let ice=ice.clone();let scope=scopes.remove(&cid);let events=events.clone();
                                let task=tokio::spawn(async move {if let Err(error)=connection::run(st,cfg,id,signals,incoming,ice,scope,stopped,events).await {tracing::warn!(%error,"Cloud controller disconnected");}});
                                peers.insert(cid.clone(),Peer {stop:Some(stop),input,task,status:"connecting".into(),created:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64});
                            }
                            if let Some(peer)=peers.get(&cid){let _=peer.input.send(signal).await;}
                            else if matches!(signal,SignalMsg::Ice{..}|SignalMsg::E2eePubkey{..}) {
                                if early.len()<64||early.contains_key(&cid) {let buffer=early.entry(cid).or_default();if buffer.len()<16{buffer.push(signal);}}
                            }
                        }
                        peers.retain(|_,peer|!peer.task.is_finished());self.publish(&state,status,"",sessions(&peers));
                    }
                }
            }
            close_peers(&mut peers).await;self.publish(&state,"connecting","",vec![]);
        }
        self.publish(&state,"offline","",vec![]);
    }
}
fn sessions(peers:&HashMap<String,Peer>)->Vec<Value> {let mut ids=peers.keys().collect::<Vec<_>>();ids.sort();ids.into_iter().filter_map(|id|peers.get(id).map(|peer|json!({"cid":id,"state":peer.status,"connectedAt":peer.created}))).collect()}
async fn close_peer(mut peer:Peer) {if let Some(stop)=peer.stop.take(){let _=stop.send(());}if tokio::time::timeout(Duration::from_secs(3),&mut peer.task).await.is_err(){peer.task.abort();let _=(&mut peer.task).await;}}
async fn close_peers(peers:&mut HashMap<String,Peer>){for (_,peer) in peers.drain(){close_peer(peer).await;}}
