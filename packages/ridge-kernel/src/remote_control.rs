//! Runtime-owned LAN listener and remote access controls.
use crate::{host::KernelHost,server::AppState};
use ridge_remote::{access::{RemoteBlacklist,RemoteClientRegistry,BlacklistEntry},auth::{SessionStore,VerifyThrottle},serve::UaServeConfig};
use serde_json::{json,Value};
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};

pub struct Access {
    pub clients: RemoteClientRegistry,
    pub blacklist: RemoteBlacklist,
}
impl Default for Access {
    fn default()->Self {Self {clients:Default::default(),blacklist:Default::default()}}
}
struct Listener {
    host:Arc<KernelHost>,
    shutdown:tokio::sync::oneshot::Sender<()>,
    task:tokio::task::JoinHandle<()>,
    mdns:(std::thread::JoinHandle<()>,Arc<AtomicBool>),
}
pub struct Lan {
    pub access:Arc<Access>,
    sessions:Arc<SessionStore>,
    throttle:Arc<VerifyThrottle>,
    listener:tokio::sync::Mutex<Option<Listener>>,
}
impl Default for Lan {
    fn default()->Self {Self {access:Default::default(),sessions:Arc::new(SessionStore::new()),throttle:Arc::new(VerifyThrottle::new()),listener:Default::default()}}
}
impl Lan {
    pub fn open()->Result<Self,String> {
        let root=std::env::var_os("RIDGE_CDP_NATIVE_DATA_DIR").filter(|_|cfg!(debug_assertions)).map(std::path::PathBuf::from)
            .unwrap_or_else(crate::registry::ridge_data_dir);
        Ok(Self {access:Arc::new(Access {clients:Default::default(),blacklist:RemoteBlacklist::open(root.join("remote-blacklist.json"))?}),
            sessions:Arc::new(SessionStore::new()),throttle:Arc::new(VerifyThrottle::new()),listener:Default::default()})
    }
    pub async fn stop(&self) {
        if let Some(listener)=self.listener.lock().await.take() {
            listener.host.remote_enabled.store(false,Ordering::Release);
            let _=listener.shutdown.send(());
            listener.mdns.1.store(true,Ordering::Relaxed);
            let _=tokio::task::spawn_blocking(move||listener.mdns.0.join()).await;
            let mut task=listener.task;
            if tokio::time::timeout(std::time::Duration::from_secs(5),&mut task).await.is_err() {task.abort();let _=task.await;}
        }
    }
    async fn enable(&self,state:&AppState,port:u16)->Result<(),String> {
        let mut slot=self.listener.lock().await;
        if let Some(listener)=slot.as_ref() {
            if !listener.task.is_finished() {return Ok(());}
            return Err("LAN listener exited; disable it before restarting".into());
        }
        let (listener,actual)=ridge_remote::server::bind_tcp(port).map_err(|e|e.to_string())?;
        let lan_ip=ridge_remote::net::detect_lan_ip();
        let machine_name=sysinfo::System::host_name().unwrap_or_else(||"unknown".into());
        let tls=ridge_remote::tls::resolve_config_multi(&ridge_remote::net::detect_lan_ips(),&machine_name).await;
        let insecure=std::env::var("RIDGE_REMOTE_ALLOW_INSECURE_HTTP").is_ok_and(|v|v=="1"||v.eq_ignore_ascii_case("true"));
        if tls.is_none() && !insecure {return Err("LAN TLS unavailable".into());}
        let host=Arc::new(KernelHost {endpoint:crate::registry::KernelEndpoint {pid:state.pid,port:state.port,token:state.token.clone(),started_at_unix:state.started_at_unix},
            totp:state.remote_auth.clone(),totp_throttle:self.throttle.clone(),sessions:self.sessions.clone(),access:self.access.clone(),
            port:actual,lan_ip,machine_name,serve_cfg:UaServeConfig::resolve_ui_dirs(),tls_enabled:tls.is_some(),remote_enabled:Arc::new(AtomicBool::new(true))});
        let (shutdown,rx)=tokio::sync::oneshot::channel();
        let task_host=host.clone();
        let task=tokio::spawn(async move {
            let result=ridge_remote::server_app::run(task_host.clone(),Arc::new(ridge_remote::lan_trust::LanTrustState::new()),listener,tls,rx,true).await;
            task_host.remote_enabled.store(false,Ordering::Release);
            if let Err(error)=result {tracing::error!(%error,"LAN listener exited");}
        });
        *slot=Some(Listener {host,shutdown,task,mdns:ridge_remote::mdns::spawn_mdns_broadcast(actual)});
        Ok(())
    }
    pub async fn dispatch(&self,state:&AppState,method:&str,args:&Value,native:bool)->Option<Result<Value,String>> {
        if !matches!(method,"get_remote_info"|"set_remote_enabled"|"get_remote_enabled"|"list_remote_sessions"|"disconnect_session"|"add_to_blacklist"|"list_blacklist"|"remove_from_blacklist") {return None;}
        if !native {return Some(Err("remote access controls require native trust".into()));}
        Some(async {
            match method {
                "set_remote_enabled"=>{
                    let enabled=args["enabled"].as_bool().ok_or("missing enabled")?;
                    if enabled {
                        let port=match args.get("port") {None=>9527,Some(port)=>u16::try_from(port.as_u64().ok_or("invalid LAN port")?).map_err(|_|"invalid LAN port")?};
                        if port>65526 {return Err("LAN port outside probe range".into());}
                        self.enable(state,port).await?;
                    } else {self.stop().await;}
                    Ok(Value::Null)
                }
                "get_remote_enabled"=>{
                    let slot=self.listener.lock().await;
                    Ok(json!(slot.as_ref().is_some_and(|listener|!listener.task.is_finished()&&listener.host.remote_enabled.load(Ordering::Acquire))))
                }
                "get_remote_info"=>{
                    let slot=self.listener.lock().await;
                    let live=slot.as_ref().filter(|listener|!listener.task.is_finished()&&listener.host.remote_enabled.load(Ordering::Acquire));
                    let machine=sysinfo::System::host_name().unwrap_or_else(||"unknown".into());
                    let auth=state.remote_auth.call("remote_totp_info",json!({"machineName":machine}))?;
                    Ok(json!({"port":live.map(|listener|listener.host.port).unwrap_or(0),"lanIp":ridge_remote::net::detect_lan_ip(),"lanIps":ridge_remote::net::detect_lan_ips(),
                        "totpCode":auth["code"],"otpauthUri":auth["uri"],"ready":live.is_some(),"remoteEnabled":live.is_some(),"devMode":cfg!(debug_assertions),"tls":live.is_some_and(|listener|listener.host.tls_enabled),"machineName":machine}))
                }
                "list_remote_sessions"=>{
                    let now=std::time::SystemTime::now();
                    let mut sessions=self.access.clients.list();sessions.sort_by_key(|session|session.id);
                    Ok(json!(sessions.into_iter().map(|session|json!({"id":session.id,"remoteAddr":session.remote_addr,"deviceId":session.device_id,"userAgent":session.user_agent,
                        "connectedSecs":now.duration_since(session.connected_at).unwrap_or_default().as_secs()})).collect::<Vec<_>>()))
                }
                "list_blacklist"=>serde_json::to_value(self.access.blacklist.list()).map_err(|e|e.to_string()),
                "remove_from_blacklist"=>Ok(json!(self.access.blacklist.remove_checked(args["id"].as_str().ok_or("missing blacklist id")?)?)),
                _=>{
                    let id=args["id"].as_u64().ok_or("missing session id")?;
                    let Some(info)=self.access.clients.info_of(id) else {return Ok(json!(false));};
                    if method=="add_to_blacklist" {
                        let device_id=(!info.device_id.is_empty()).then_some(info.device_id.clone());
                        let ip=(!info.remote_addr.is_empty()&&info.remote_addr!="unknown").then_some(info.remote_addr.clone());
                        if device_id.is_none()&&ip.is_none() {return Ok(json!(false));}
                        self.access.blacklist.add_checked(BlacklistEntry {id:uuid::Uuid::new_v4().to_string(),label:device_id.as_deref().map(|device|device.chars().take(8).collect()).unwrap_or_else(||info.remote_addr.clone()),
                            device_id,ip,added_at:std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()})?;
                    }
                    if let Some(token)=info.token {self.sessions.invalidate(&token);}
                    Ok(json!(self.access.clients.kick(id)))
                }
            }
        }.await)
    }
}
