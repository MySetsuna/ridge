//! LAN presentation adapter over the common Host; TLS failures never downgrade.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::sync::{mpsc,oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};

use super::lan_proto;
use super::canonical_view::View;
use super::session::Session;

type LanWsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type PendingRpc=Arc<Mutex<std::collections::HashMap<String,oneshot::Sender<std::result::Result<serde_json::Value,String>>>>>;
struct RpcGuard {id:String,pending:PendingRpc}
impl Drop for RpcGuard {fn drop(&mut self){self.pending.lock().remove(&self.id);}}

enum ConnectionTarget {
    Lan { hostport:String, credential:String },
    Local(ridge_kernel::local_client::ClientGrant),
}
impl ConnectionTarget {
    async fn open(&self)->Result<LanWsStream> {
        match self {
            Self::Lan{hostport,credential}=>connect_lan_stream(hostport,"token",credential).await,
            Self::Local(grant)=>{
                let mut url=reqwest::Url::parse(&grant.ws_url)?;
                url.query_pairs_mut().append_pair("deviceId",&grant.device_id);
                let mut request=url.as_str().into_client_request()?;
                request.headers_mut().insert("origin",grant.origin.parse()?);
                request.headers_mut().insert("sec-websocket-protocol",format!("ridge-client, auth-{}",grant.token).parse()?);
                let mut limits=tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
                limits.max_message_size=Some(ridge_term::terminal_protocol::MAX_FRAME_BYTES);
                limits.max_frame_size=limits.max_message_size;
                limits.max_write_buffer_size=2*ridge_term::terminal_protocol::MAX_FRAME_BYTES;
                let (stream,_)=tokio::time::timeout(std::time::Duration::from_secs(10),
                    tokio_tungstenite::connect_async_with_config(request,Some(limits),false)).await
                    .context("local Runtime connection timed out")?.context("local Runtime connection failed")?;
                Ok(stream)
            }
        }
    }
}

pub(crate) fn local_cwd(cwd:Option<&str>)->Result<String>{
    if let Some(cwd)=cwd.filter(|value|!value.is_empty()){return Ok(cwd.to_owned());}
    std::env::current_dir()?.into_os_string().into_string().map_err(|_|anyhow!("CLI working directory must be valid UTF-8"))
}

pub(crate) fn spawn_local_terminal(endpoint:&ridge_kernel::registry::KernelEndpoint,shell:Option<&str>,cwd:Option<&str>)->Result<ridge_term::terminal_protocol::PaneRef>{
    let id=uuid::Uuid::new_v4();let workspace=uuid::Uuid::new_v4();
    let cwd=local_cwd(cwd)?;
    let created=ridge_kernel::client::create_domain_pty(endpoint,id,shell,Some(&cwd),Some(workspace),"shell",Some("ridge-interactive")).map_err(anyhow::Error::msg)?;
    if created!=id{return Err(anyhow!("local Runtime terminal identity mismatch"));}
    Ok(ridge_term::terminal_protocol::PaneRef::new(workspace.to_string(),id.to_string()))
}

pub async fn connect_local(shell:Option<String>,cwd:Option<String>)->Result<(LanControllerSession,mpsc::Receiver<Vec<u8>>)> {
    let (grant,target)=tokio::task::spawn_blocking(move ||->Result<_>{
        use ridge_kernel::client;
        let endpoint=crate::kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
        let target=spawn_local_terminal(&endpoint,shell.as_deref(),cwd.as_deref())?;
        let device=format!("ridge-cli-local-{}",uuid::Uuid::new_v4());let origin="http://127.0.0.1";
        let grant:ridge_kernel::local_client::ClientGrant=serde_json::from_value(client::request_json(&endpoint,"POST","/v1/local/clients",
            Some(&serde_json::json!({"deviceId":device,"origin":origin}))).map_err(anyhow::Error::msg)?)?;
        if grant.protocol_version!=client::KERNEL_PROTOCOL_VERSION||grant.device_id!=device||grant.origin!=origin
            ||grant.ws_url!=format!("ws://127.0.0.1:{}/v1/local/ws",endpoint.port)||grant.token.len()!=64 {
            return Err(anyhow!("local Runtime grant identity mismatch"));
        }
        Ok((grant,target))
    }).await??;
    let connection=ConnectionTarget::Local(grant);
    let stream=connection.open().await?;
    Ok(session_from_stream(stream,connection,Some(target)))
}

pub(crate) fn tls_connector()->Result<Connector> {
    Ok(Connector::Rustls(ridge_kernel::client::lan_tls_config().map_err(anyhow::Error::msg)?))
}

/// LAN 控制端会话：实现 [`Session`]，把按键/尺寸经 WS 回送到桌面 host。
pub struct LanControllerSession {
    to_host: mpsc::Sender<Message>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    /// 当前订阅的 pane（握手后由 reader 任务写入）。
    pane: Arc<Mutex<View>>,
    /// 最近一次本端视口尺寸 `(cols, rows)`；订阅时用它对齐远端 PTY。
    last_size: Arc<Mutex<(u16, u16)>>,
    /// claim-pane 单调序号。
    seq: Arc<AtomicU64>,
    failure: Arc<Mutex<Option<String>>>,
    pending:PendingRpc,
}

impl LanControllerSession {
    async fn load_history(&self,back:bool)->Result<Option<Vec<u8>>>{
        let (request,dimensions)={let mut view=self.pane.lock();if view.epoch.is_none(){return Ok(None);}(view.history_request(back),view.dimensions())};
        let Some(request)=request else{return Ok(self.presentation());};
        let encoded=self.call("get_terminal_history",serde_json::json!({"request":request})).await?;
        let encoded=encoded.as_str().filter(|encoded|encoded.len()<=ridge_term::terminal_protocol::MAX_FRAME_BYTES.div_ceil(3)*4).ok_or_else(||anyhow!("invalid terminal history response"))?;
        let bytes=base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let frame=ridge_term::terminal_protocol::decode_frame(&bytes).map_err(|_|anyhow!("invalid terminal history frame"))?;
        let mut view=self.pane.lock();
        if view.dimensions()!=dimensions{return Err(anyhow!("terminal resized while loading history; retry"));}
        view.apply_history(&request,frame).map(Some).map_err(anyhow::Error::msg)
    }
    pub fn pane_ref(&self)->Option<ridge_term::terminal_protocol::PaneRef>{self.pane.lock().pane.clone()}
    pub fn presentation(&self)->Option<Vec<u8>>{let view=self.pane.lock();view.epoch.as_ref()?;Some(view.render().into_bytes())}
    pub async fn call(&self,method:&str,args:serde_json::Value)->Result<serde_json::Value>{
        let started=super::performance::begin();
        let id=format!("cli-rpc-{}",uuid::Uuid::new_v4());let (tx,rx)=oneshot::channel();
        {let mut pending=self.pending.lock();if pending.len()>=64{return Err(anyhow!("CLI RPC queue full"));}pending.insert(id.clone(),tx);}
        let _guard=RpcGuard{id:id.clone(),pending:self.pending.clone()};
        let message=serde_json::json!({"jsonrpc":"2.0","id":id,"method":method,"params":args}).to_string();
        if message.len()>ridge_term::terminal_protocol::MAX_FRAME_BYTES{return Err(anyhow!("CLI RPC exceeds frame bound"));}
        let bytes=message.len();
        self.to_host.try_send(Message::Text(message)).map_err(|_|anyhow!("CLI transport queue full or closed"))?;
        let result=async {tokio::time::timeout(std::time::Duration::from_secs(10),rx).await.context("CLI RPC timed out; outcome unknown")?
            .context("CLI RPC connection closed; outcome unknown")?.map_err(anyhow::Error::msg)}.await;
        super::performance::finish("control_rpc",started,bytes,self.to_host.max_capacity()-self.to_host.capacity());
        if result.is_err(){super::performance::error("CLI_RPC_FAILED");}result
    }
    pub fn activate(&self,target:ridge_term::terminal_protocol::PaneRef)->Result<()>{
        let old={let mut view=self.pane.lock();let old=view.pane.clone().map(|pane|(pane,view.activation));
            view.activation=view.activation.checked_add(1).ok_or_else(||anyhow!("terminal activation exhausted"))?;
            view.reset_connection();view.select(target.clone());old};
        if let Some((pane,activation))=old {self.to_host.try_send(Message::Text(serde_json::json!({"type":"unsubscribe-pane","workspaceId":pane.workspace_id,"paneId":pane.pane_id,"activationId":activation}).to_string())).map_err(|_|anyhow!("CLI unsubscribe queue full or closed"))?;}
        if !subscribe(&self.to_host,&self.pane,&self.last_size,&self.seq,target.pane_id,target.workspace_id){return Err(anyhow!("CLI subscribe queue full or closed"));}
        Ok(())
    }
    /// 当前已订阅的 pane id（probe / 诊断用）。
    pub fn current_pane(&self) -> Option<String> {
        let view=self.pane.lock();
        view.epoch.as_ref()?;
        view.pane.as_ref().map(|pane|pane.pane_id.clone())
    }
}

impl Drop for LanControllerSession {
    fn drop(&mut self) { for task in &self.tasks { task.abort(); } }
}
impl Session for LanControllerSession {
    fn presentation(&self)->Option<Vec<u8>>{LanControllerSession::presentation(self)}
    fn follow_tail(&self)->Option<Vec<u8>>{let mut view=self.pane.lock();view.follow_tail().then(||view.render().into_bytes())}
    fn scroll_history(&self,back:bool)->impl std::future::Future<Output=Result<Option<Vec<u8>>>>+Send {self.load_history(back)}
    fn input_ready(&self)->bool {self.pane.lock().epoch.is_some()}
    fn failure(&self)->Option<String> {self.failure.lock().clone()}
    fn send_input(&self, data: &[u8]) -> Result<()> {
        let started=super::performance::begin();
        if data.len()>ridge_term::terminal_protocol::MAX_INPUT_BYTES { return Err(anyhow!("LAN input exceeds bound")); }
        let mut view=self.pane.lock();
        let input=view.input(data).map_err(anyhow::Error::msg)?;
        let message=serde_json::json!({"type":"invoke-request","_reqId":view.next_input,"cmd":"write_terminal_input","args":{"input":input}});
        self.to_host.try_send(Message::Text(message.to_string())).map_err(|_|anyhow!("LAN input queue full or closed"))?;
        view.next_input+=1;
        super::performance::finish("input_queue",started,data.len(),self.to_host.max_capacity()-self.to_host.capacity());
        Ok(())
    }
    fn resize(&self, cols:u16,rows:u16)->Result<()> {
        *self.last_size.lock()=(cols,rows);
        let target={let view=self.pane.lock();view.epoch.as_ref().and_then(|_|view.pane.as_ref()).map(|pane|pane.pane_id.clone())};
        if let Some(pane)=target {
            let seq=self.seq.fetch_add(1,Ordering::Relaxed);
            self.to_host.try_send(Message::Text(lan_proto::claim_pane(&pane,rows,cols,seq)))
                .map_err(|_|anyhow!("LAN control queue full or closed"))?;
        }
        Ok(())
    }
}

/// 订阅一个 pane：记录为当前 pane、发 `subscribe-pane`，并按最近视口尺寸 `claim-pane`
/// 让远端 PTY reflow 到本端大小。
fn subscribe(
    to_host: &mpsc::Sender<Message>,
    pane: &Arc<Mutex<View>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
    pid: String,
    workspace:String,
) -> bool {
    let target=ridge_term::terminal_protocol::PaneRef::new(workspace,pid);
    {let view=pane.lock();if view.pane.as_ref()==Some(&target)&&view.epoch.is_some(){return true;}}
    let pid=&target.pane_id;let workspace=&target.workspace_id;
    let activation=pane.lock().activation;
    if to_host.try_send(Message::Text(serde_json::json!({"type":"subscribe-pane","paneId":pid,"workspaceId":workspace,"activationId":activation,"active":true}).to_string())).is_err(){return false;}
    let (cols, rows) = *last_size.lock();
    let s = seq.fetch_add(1, Ordering::Relaxed);
    if to_host.try_send(Message::Text(lan_proto::claim_pane(&pid, rows, cols, s))).is_err(){return false;}
    pane.lock().select(target);
    true
}

/// 连接桌面 LAN host，完成握手并返回 `(会话, 输出字节流)`。
///
/// `host` 为 `ip` 或 `ip:port`（缺省端口 9527）。`code`（TOTP）与 `token`（session）
/// 二选一。返回的 `Receiver<Vec<u8>>` 由 [`super::run_session`] 透传到本地终端。
pub async fn connect_lan(
    host: &str,
    code: Option<String>,
    token: Option<String>,
) -> Result<(LanControllerSession, mpsc::Receiver<Vec<u8>>)> {
    let hostport = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:9527")
    };
    let (key,value)=match (&code,&token) {
        (Some(code),_) => ("code",code),
        (None,Some(token)) => ("token",token),
        _ => return Err(anyhow!("需要 --code <TOTP> 或 --token <session>")),
    };
    if value.len()>4096 { return Err(anyhow!("LAN credential exceeds bound")); }
    let credential=if key=="code" {authenticate_lan(&hostport,value).await?} else {value.clone()};
    let connection=ConnectionTarget::Lan{hostport,credential};
    let stream=connection.open().await?;
    Ok(session_from_stream(stream,connection,None))
}

fn session_from_stream(stream:LanWsStream,connection:ConnectionTarget,target:Option<ridge_term::terminal_protocol::PaneRef>)->(LanControllerSession,mpsc::Receiver<Vec<u8>>) {
    let (to_host_tx, to_host_rx) = mpsc::channel::<Message>(64);
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(32);
    let pane = Arc::new(Mutex::new(View::default()));
    if let Some(target)=target {pane.lock().select(target);}
    let last_size = Arc::new(Mutex::new((80u16, 24u16)));
    let seq = Arc::new(AtomicU64::new(1));

    let failure=Arc::new(Mutex::new(None));
    let pending=PendingRpc::default();let actor_pending=pending.clone();
    let (actor_pane,actor_size,actor_seq,actor_tx,actor_failure)=(pane.clone(),last_size.clone(),seq.clone(),to_host_tx.clone(),failure.clone());
    let task=tokio::spawn(async move {
        if let Err(error)=run_lan_transport(stream,&connection,to_host_rx,&actor_tx,&actor_pane,&actor_size,&actor_seq,&out_tx,&actor_pending).await {
            actor_pane.lock().epoch=None;
            *actor_failure.lock()=Some(error.to_string());
            let _=out_tx.send(format!("\r\nCLI terminal connection error: {error}\r\n").into_bytes()).await;
        }
    });

    (
        LanControllerSession {
            to_host: to_host_tx,
            tasks: vec![task],
            pane,
            last_size,
            seq,
            failure,
            pending,
        },
        out_rx,
    )
}

async fn authenticate_lan(hostport:&str,code:&str)->Result<String> {
    let url=reqwest::Url::parse(&format!("https://{hostport}/verify")).context("invalid LAN authentication address")?;
    if !url.username().is_empty()||url.password().is_some(){return Err(anyhow!("LAN address must not contain credentials"));}
    let tls=ridge_kernel::client::lan_tls_config().map_err(anyhow::Error::msg)?;
    let client=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).use_preconfigured_tls((*tls).clone()).https_only(true)
        .timeout(std::time::Duration::from_secs(10)).build().context("LAN TLS client initialization failed")?;
    let mut response=client.post(url).form(&[("code",code),("device","ridge-cli")]).send().await
        .context("LAN authentication transport failed")?.error_for_status().context("LAN authentication rejected")?;
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.context("LAN authentication response failed")? {
        if bytes.len()+chunk.len()>65536{return Err(anyhow!("LAN authentication response exceeds bound"));}
        bytes.extend_from_slice(&chunk);
    }
    let response:serde_json::Value=serde_json::from_slice(&bytes).map_err(|_|anyhow!("invalid LAN authentication response"))?;
    if response["success"]!=true{return Err(anyhow!("LAN authentication rejected"));}
    response["token"].as_str().filter(|token|!token.is_empty()&&token.len()<=4096)
        .map(str::to_owned).ok_or_else(||anyhow!("LAN authentication omitted a bounded session token"))
}

async fn connect_lan_stream(hostport: &str, key: &str,value: &str) -> Result<LanWsStream> {
    let mut wss=reqwest::Url::parse(&format!("wss://{hostport}/ws")).context("invalid LAN host address")?;
    wss.query_pairs_mut().append_pair("device","ridge-cli");
    if key!="token" {wss.query_pairs_mut().append_pair(key,value);}
    let connector = tls_connector()?;
    let mut request = wss
        .as_str()
        .into_client_request()
        .context("构造 WS 请求失败")?;
    if key=="token" {request.headers_mut().insert("authorization",format!("Bearer {value}").parse().map_err(|_|anyhow!("invalid LAN session token"))?);}
    let mut limits=tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    limits.max_message_size=Some(ridge_term::terminal_protocol::MAX_FRAME_BYTES);
    limits.max_frame_size=limits.max_message_size;
    limits.max_write_buffer_size=2*ridge_term::terminal_protocol::MAX_FRAME_BYTES;
    let (stream,_)=tokio::time::timeout(std::time::Duration::from_secs(10),
        connect_async_tls_with_config(request,Some(limits),false,Some(connector))).await
        .context("LAN TLS connection timed out")?.context("LAN TLS connection failed; trust the host CA before retrying")?;
    Ok(stream)
}

async fn run_lan_transport(
    mut socket:LanWsStream,connection:&ConnectionTarget,mut messages:mpsc::Receiver<Message>,
    to_host:&mpsc::Sender<Message>,pane:&Arc<Mutex<View>>,last_size:&Arc<Mutex<(u16,u16)>>,
    seq:&Arc<AtomicU64>,out_tx:&mpsc::Sender<Vec<u8>>,pending:&PendingRpc,
)->Result<()> {
    loop {
        loop {tokio::select! {
            message=messages.recv()=>{let Some(message)=message else{return Ok(());};let started=super::performance::begin();let bytes=message.len();let result=tokio::time::timeout(std::time::Duration::from_secs(10),socket.send(message)).await;super::performance::finish("socket_send",started,bytes,messages.len());if result.map_or(true,|result|result.is_err()){break;}},
            frame=socket.next()=>match frame {
                None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break,
                Some(Ok(message))=>{
                    if let Message::Text(text)=&message {if let Ok(value)=serde_json::from_str::<serde_json::Value>(text){
                        if let Some(id)=value["id"].as_str(){if let Some(reply)=pending.lock().remove(id){
                            let result=if let Some(error)=value.get("error"){Err(error.to_string())}else{Ok(value["result"].clone())};let _=reply.send(result);continue;
                        }}
                    }}
                    if !handle_lan_message(message,to_host,pane,last_size,seq,out_tx).await {
                    return Err(anyhow!("LAN terminal protocol rejected a frame or command"));
                    }
                },
            },
        }}
        pane.lock().reset_connection();
        for (_,reply) in pending.lock().drain(){let _=reply.send(Err("CLI connection lost; RPC outcome unknown".into()));}
        let _=tokio::time::timeout(std::time::Duration::from_secs(1),socket.close(None)).await;
        while messages.try_recv().is_ok() {} // Never replay unconfirmed input from a dead connection.
        if out_tx.send(b"\r\nLAN disconnected; unconfirmed input will not be replayed. Reconnecting...\r\n".to_vec()).await.is_err(){return Ok(());}
        let mut resumed=None;
        for attempt in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(250u64<<attempt)).await;
            if out_tx.is_closed(){return Ok(());}
            if let Ok(stream)=connection.open().await {resumed=Some(stream);break;}
        }
        socket=resumed.ok_or_else(||match connection {
            ConnectionTarget::Lan{..}=>anyhow!("LAN reconnect exhausted; authenticate explicitly with a fresh TOTP"),
            ConnectionTarget::Local(_)=>anyhow!("local Runtime reconnect exhausted; restart the TUI to rediscover the Runtime"),
        })?;
        while messages.try_recv().is_ok() {}
    }
}

async fn handle_lan_message(
    message: Message,
    to_host: &mpsc::Sender<Message>,
    pane: &Arc<Mutex<View>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
    out_tx: &mpsc::Sender<Vec<u8>>,
) -> bool {
    match message {
        Message::Text(text) => {
            handle_lan_text(&text, to_host, pane, last_size, seq)
        }
        Message::Binary(buffer) => handle_lan_binary(&buffer, pane, out_tx).await,
        Message::Ping(payload) => {
            to_host.try_send(Message::Pong(payload)).is_ok()
        }
        Message::Close(_) => false,
        _ => true,
    }
}

fn handle_lan_text(
    text: &str,
    to_host: &mpsc::Sender<Message>,
    pane: &Arc<Mutex<View>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return true;
    };
    match value["type"].as_str() {
        Some("hello") => {
            if value["terminalProtocolVersion"]!=ridge_term::terminal_protocol::PROTOCOL_VERSION{return false;}
            let target=pane.lock().pane.clone();
            if let Some(target)=target{return subscribe(to_host,pane,last_size,seq,target.pane_id,target.workspace_id);}
            to_host.try_send(Message::Text(lan_proto::list_panes())).is_ok()
        }
        Some("panes") => handle_panes_frame(&value, to_host, pane, last_size, seq),
        Some("create-pane-result") => {
            handle_create_pane_frame(&value, to_host, pane, last_size, seq)
        }
        Some("invoke-result")=>value.get("_error").is_none(),
        _ => true
    }
}

fn handle_panes_frame(
    value: &serde_json::Value,
    to_host: &mpsc::Sender<Message>,
    pane: &Arc<Mutex<View>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
) -> bool {
    let current=pane.lock().pane.clone();
    if let Some(target)=current {
        if value["workspaceId"]==target.workspace_id&&value["panes"].as_array().is_some_and(|panes|panes.iter().any(|pane|pane["id"]==target.pane_id)) {
            return subscribe(to_host,pane,last_size,seq,target.pane_id,target.workspace_id);
        }
        return false;
    }
    let first = value["panes"]
        .as_array()
        .and_then(|panes| panes.first())
        .and_then(|pane| pane["id"].as_str())
        .map(String::from);
    match first {
        Some(pane_id) => {
            let Some(workspace)=value["workspaceId"].as_str() else{return false;};
            subscribe(to_host, pane, last_size, seq, pane_id,workspace.to_string())
        },
        None => {
            to_host.try_send(Message::Text(lan_proto::create_pane())).is_ok()
        }
    }
}

fn handle_create_pane_frame(
    value: &serde_json::Value,
    to_host: &mpsc::Sender<Message>,
    pane: &Arc<Mutex<View>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
) -> bool {
    if value["success"].as_bool() != Some(true) {
        return false;
    }
    to_host.try_send(Message::Text(lan_proto::list_panes())).is_ok()
}

async fn handle_lan_binary(
    buffer: &[u8],
    pane: &Arc<Mutex<View>>,
    out_tx: &mpsc::Sender<Vec<u8>>,
) -> bool {
    let started=super::performance::begin();
    let frame=ridge_term::terminal_protocol::decode_frame(buffer).map_err(|_|"TERMINAL_FRAME_INVALID");
    super::performance::finish("decode",started,buffer.len(),0);
    let result=frame.and_then(|frame|pane.lock().apply(frame));
    match result {
        Ok(Some(bytes))=>{let started=super::performance::begin();let count=bytes.len();let result=out_tx.send(bytes).await;super::performance::finish("output_queue",started,count,out_tx.max_capacity()-out_tx.capacity());result.is_ok()},
        Ok(None)=>true,
        Err(error)=>{super::performance::error(error);let _=out_tx.send(format!("\r\nCLI terminal error: {error}\r\n").into_bytes()).await;false}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(tx:mpsc::Sender<Message>)->LanControllerSession {
        let mut view=View::default();
        view.select(ridge_term::terminal_protocol::PaneRef::new("fixture-workspace",uuid::Uuid::new_v4().to_string()));
        view.epoch=Some("fixture-epoch".into());
        LanControllerSession{to_host:tx,tasks:Vec::new(),pane:Arc::new(Mutex::new(view)),last_size:Arc::new(Mutex::new((80,24))),seq:Arc::new(AtomicU64::new(1)),failure:Arc::new(Mutex::new(None)),pending:PendingRpc::default()}
    }
    #[tokio::test]
    async fn cancelled_rpc_and_queue_rejection_release_pending_slots(){
        let (tx,mut rx)=mpsc::channel(1);let sess=session(tx);
        let mut call=Box::pin(sess.call("get_theme_data",serde_json::json!({})));
        tokio::select!{
            _=&mut call=>panic!("RPC completed without a reply"),
            message=rx.recv()=>assert!(message.is_some()),
        }
        assert_eq!(sess.pending.lock().len(),1);drop(call);assert!(sess.pending.lock().is_empty());
        sess.to_host.try_send(Message::Text("occupied".into())).unwrap();
        assert!(sess.call("get_theme_data",serde_json::json!({})).await.is_err());
        assert!(sess.pending.lock().is_empty());
    }
    #[tokio::test]
    async fn preselected_local_terminal_subscribes_without_global_workspace_selection(){
        let (tx,mut rx)=mpsc::channel(4);let sess=session(tx);
        let target=sess.pane.lock().pane.clone().unwrap();sess.pane.lock().reset_connection();
        let hello=serde_json::json!({"type":"hello","terminalProtocolVersion":ridge_term::terminal_protocol::PROTOCOL_VERSION});
        assert!(handle_lan_text(&hello.to_string(),&sess.to_host,&sess.pane,&sess.last_size,&sess.seq));
        let Message::Text(message)=rx.try_recv().unwrap() else{panic!("expected subscription");};
        let message:serde_json::Value=serde_json::from_str(&message).unwrap();
        assert_eq!(message["type"],"subscribe-pane");assert_eq!(message["workspaceId"],target.workspace_id);assert_eq!(message["paneId"],target.pane_id);
        assert!(!sess.input_ready(),"input must await the canonical snapshot");
    }
    #[tokio::test]
    async fn input_is_fifo_and_reports_full_closed_and_oversized_queues(){
        let (tx,mut rx)=mpsc::channel(2);let sess=session(tx);
        sess.send_input(b"first").unwrap();sess.send_input(b"second").unwrap();
        assert!(sess.send_input(b"third").is_err());
        assert_eq!(sess.pane.lock().next_input,3,"rejected input consumed a sequence");
        for expected in ["first","second"]{
            let Message::Text(text)=rx.recv().await.unwrap() else{panic!("expected input");};
            assert_eq!(serde_json::from_str::<serde_json::Value>(&text).unwrap()["args"]["input"]["bytes"],serde_json::json!(expected.as_bytes()));
        }
        assert!(sess.send_input(&vec![b'x';ridge_term::terminal_protocol::MAX_INPUT_BYTES+1]).is_err());
        drop(rx);assert!(sess.send_input(b"closed").is_err());
    }
    #[tokio::test]
    async fn repeated_pane_projection_keeps_input_source_and_live_snapshot(){
        let (tx,mut rx)=mpsc::channel(4);let sess=session(tx);
        let (target,source)={let view=sess.pane.lock();(view.pane.clone().unwrap(),view.source.clone())};
        assert!(subscribe(&sess.to_host,&sess.pane,&sess.last_size,&sess.seq,target.pane_id,target.workspace_id));
        let view=sess.pane.lock();assert_eq!(view.source,source);assert_eq!(view.epoch.as_deref(),Some("fixture-epoch"));
        assert!(rx.try_recv().is_err(),"duplicate projection resubscribed an existing activation");
    }
    #[tokio::test]
    async fn another_pane_added_by_other_client_does_not_change_cli_target(){
        let (tx,mut rx)=mpsc::channel(4);let sess=session(tx);
        let target=sess.pane.lock().pane.clone().unwrap();
        let projection=serde_json::json!({"workspaceId":target.workspace_id,"panes":[{"id":"another-pane"},{"id":target.pane_id}]});
        assert!(handle_panes_frame(&projection,&sess.to_host,&sess.pane,&sess.last_size,&sess.seq));
        assert_eq!(sess.pane.lock().pane.as_ref(),Some(&target));
        assert!(rx.try_recv().is_err());
    }
    #[tokio::test]
    async fn reconnect_resubscribes_original_pane_and_missing_target_fails_closed(){
        let (tx,mut rx)=mpsc::channel(4);let sess=session(tx);
        let target=sess.pane.lock().pane.clone().unwrap();
        sess.pane.lock().reset_connection();
        assert!(!sess.input_ready());
        assert!(sess.send_input(b"offline").is_err());
        let projection=serde_json::json!({"workspaceId":target.workspace_id,"panes":[{"id":"another-pane"},{"id":target.pane_id}]});
        assert!(handle_panes_frame(&projection,&sess.to_host,&sess.pane,&sess.last_size,&sess.seq));
        let Message::Text(message)=rx.try_recv().unwrap() else {panic!("expected subscription");};
        let message:serde_json::Value=serde_json::from_str(&message).unwrap();
        assert_eq!(message["type"],"subscribe-pane");
        assert_eq!(message["paneId"],target.pane_id);
        rx.try_recv().unwrap();
        assert!(!sess.input_ready(),"input must await the new canonical snapshot");
        assert!(!handle_panes_frame(&serde_json::json!({"workspaceId":target.workspace_id,"panes":[{"id":"another-pane"}]}),&sess.to_host,&sess.pane,&sess.last_size,&sess.seq));
        assert!(rx.try_recv().is_err(),"missing target must not select or create another pane");
    }
    #[tokio::test]
    async fn tls_failure_never_retries_plaintext_or_sends_credentials(){
        use tokio::io::{AsyncReadExt,AsyncWriteExt};
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr=listener.local_addr().unwrap();
        let server=tokio::spawn(async move{
            let (mut socket,_)=listener.accept().await.unwrap();let mut bytes=vec![0;4096];
            let read=socket.read(&mut bytes).await.unwrap();bytes.truncate(read);
            socket.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await.unwrap();drop(socket);
            assert!(tokio::time::timeout(std::time::Duration::from_millis(300),listener.accept()).await.is_err(),"plaintext fallback opened a second connection");
            assert!(!String::from_utf8_lossy(&bytes).contains("private-fixture-code"));
        });
        assert!(connect_lan_stream(&addr.to_string(),"code","private-fixture-code").await.is_err());
        server.await.unwrap();
    }
}
