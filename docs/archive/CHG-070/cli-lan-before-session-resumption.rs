//! LAN presentation adapter over the common Host; TLS failures never downgrade.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};

use super::lan_proto;
use super::canonical_view::View;
use super::session::Session;

type LanWsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

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
}

impl LanControllerSession {
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
    fn send_input(&self, data: &[u8]) -> Result<()> {
        if data.len()>ridge_term::terminal_protocol::MAX_INPUT_BYTES { return Err(anyhow!("LAN input exceeds bound")); }
        let mut view=self.pane.lock();
        let input=view.input(data).map_err(anyhow::Error::msg)?;
        let message=serde_json::json!({"type":"invoke-request","_reqId":view.next_input,"cmd":"write_terminal_input","args":{"input":input}});
        self.to_host.try_send(Message::Text(message.to_string())).map_err(|_|anyhow!("LAN input queue full or closed"))?;
        view.next_input+=1;
        Ok(())
    }
    fn resize(&self, cols:u16,rows:u16)->Result<()> {
        *self.last_size.lock()=(cols,rows);
        if let Some(pane)=self.pane.lock().pane.as_ref().map(|pane|pane.pane_id.clone()) {
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
    if pane.lock().pane.as_ref()==Some(&target){return true;}
    let pid=&target.pane_id;let workspace=&target.workspace_id;
    if to_host.try_send(Message::Text(serde_json::json!({"type":"subscribe-pane","paneId":pid,"workspaceId":workspace,"activationId":1,"active":true}).to_string())).is_err(){return false;}
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
    let stream = connect_lan_stream(&hostport,key,value).await?;
    let (sink, read) = stream.split();
    let (to_host_tx, to_host_rx) = mpsc::channel::<Message>(64);
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(32);
    let pane = Arc::new(Mutex::new(View::default()));
    let last_size = Arc::new(Mutex::new((80u16, 24u16)));
    let seq = Arc::new(AtomicU64::new(1));

    let writer=spawn_lan_writer(sink, to_host_rx);
    let reader=spawn_lan_reader(
        read,
        to_host_tx.clone(),
        pane.clone(),
        last_size.clone(),
        seq.clone(),
        out_tx,
    );

    Ok((
        LanControllerSession {
            to_host: to_host_tx,
            tasks: vec![reader,writer],
            pane,
            last_size,
            seq,
        },
        out_rx,
    ))
}

async fn connect_lan_stream(hostport: &str, key: &str,value: &str) -> Result<LanWsStream> {
    let mut wss=reqwest::Url::parse(&format!("wss://{hostport}/ws")).context("invalid LAN host address")?;
    wss.query_pairs_mut().append_pair(key,value).append_pair("device","ridge-cli");
    let connector = tls_connector()?;
    let request = wss
        .as_str()
        .into_client_request()
        .context("构造 WS 请求失败")?;
    let mut limits=tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default();
    limits.max_message_size=Some(ridge_term::terminal_protocol::MAX_FRAME_BYTES);
    limits.max_frame_size=limits.max_message_size;
    let (stream,_)=tokio::time::timeout(std::time::Duration::from_secs(10),
        connect_async_tls_with_config(request,Some(limits),false,Some(connector))).await
        .context("LAN TLS connection timed out")?.context("LAN TLS connection failed; trust the host CA before retrying")?;
    Ok(stream)
}

fn spawn_lan_writer(
    mut sink: SplitSink<LanWsStream, Message>,
    mut messages: mpsc::Receiver<Message>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(message) = messages.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    })
}

fn spawn_lan_reader(
    mut read: SplitStream<LanWsStream>,
    to_host: mpsc::Sender<Message>,
    pane: Arc<Mutex<View>>,
    last_size: Arc<Mutex<(u16, u16)>>,
    seq: Arc<AtomicU64>,
    out_tx: mpsc::Sender<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(item) = read.next().await {
            let Ok(message) = item else {
                break;
            };
            if !handle_lan_message(message, &to_host, &pane, &last_size, &seq, &out_tx).await {
                break;
            }
        }
    })
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
    {
        let current=pane.lock();
        if let Some(target)=&current.pane {
            if value["workspaceId"]==target.workspace_id&&value["panes"].as_array().is_some_and(|panes|panes.iter().any(|pane|pane["id"]==target.pane_id)){return true;}
        }
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
    let result=ridge_term::terminal_protocol::decode_frame(buffer).map_err(|_|"TERMINAL_FRAME_INVALID")
        .and_then(|frame|pane.lock().apply(frame));
    match result {
        Ok(Some(bytes))=>out_tx.send(bytes).await.is_ok(),
        Ok(None)=>true,
        Err(error)=>{let _=out_tx.send(format!("\r\nCLI terminal error: {error}\r\n").into_bytes()).await;false}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn session(tx:mpsc::Sender<Message>)->LanControllerSession {
        let mut view=View::default();
        view.select(ridge_term::terminal_protocol::PaneRef::new("fixture-workspace",uuid::Uuid::new_v4().to_string()));
        view.epoch=Some("fixture-epoch".into());
        LanControllerSession{to_host:tx,tasks:Vec::new(),pane:Arc::new(Mutex::new(view)),last_size:Arc::new(Mutex::new((80,24))),seq:Arc::new(AtomicU64::new(1))}
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
