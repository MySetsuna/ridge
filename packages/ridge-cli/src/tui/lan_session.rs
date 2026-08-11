//! E4：`LanControllerSession` —— 连接桌面 LAN host 的 WS 控制端。
//!
//! 把 [`lan_proto`] 的纯编解码接到真实 `tokio-tungstenite` 连接：
//! - `wss://host:port/ws?code=<TOTP>`（或 `?token=`），**接受自签证书**
//!   （桌面 host 默认自签 WSS；TLS 不可用时回退明文 `ws://`）。
//! - 握手：收到 `hello` → `list-panes`；`panes` 非空订阅首个、空则 `create-pane`；
//!   `create-pane-result` → 订阅该 pane；随后入站二进制帧（16B paneId + PTY 字节）
//!   的负载灌入输出通道，由 [`super::run_session`] 透传到本地终端。
//! - 回送：按键 → `stdin`，本端 resize → `claim-pane`（reflow 远端真实 PTY）。
//!
//! 协议与"接受自签 + ?code= 鉴权 + 帧格式"均已用 CDP 对真实运行的桌面 host 联调
//! 验证（见 `scripts/cdp-lan-probe.mjs`，结果 PASS），再据此落地本驱动。

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
    connect_async, connect_async_tls_with_config, Connector, MaybeTlsStream, WebSocketStream,
};

use super::lan_proto::{self, parse_binary_frame};
use super::session::Session;

type LanWsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// rustls 校验器：接受**任意**服务端证书。桌面 LAN host 用自签证书（等价于浏览器
/// 流程里用户"信任本机 CA"）——LAN 场景的真正鉴权是 TOTP `?code=`，不是证书链。
#[derive(Debug)]
struct AcceptAnyServerCert;

impl rustls::client::danger::ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        use rustls::SignatureScheme::*;
        vec![
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ECDSA_NISTP521_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            ED25519,
        ]
    }
}

/// 构造接受自签证书的 rustls 连接器（ring provider，与现有依赖树一致）。
///
/// 复用方：LAN 控制端（桌面 LAN host 自签）+ 云端信令 dev 模式（本地 ridge-cloud
/// 自签，见 `signaling::Signaling::connect`，**仅 `config::is_dev_mode()` 放开**）。
pub(crate) fn tls_connector() -> Result<Connector> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| anyhow!("rustls 协议版本初始化失败: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert))
        .with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(config)))
}

/// LAN 控制端会话：实现 [`Session`]，把按键/尺寸经 WS 回送到桌面 host。
pub struct LanControllerSession {
    to_host: mpsc::UnboundedSender<Message>,
    /// 当前订阅的 pane（握手后由 reader 任务写入）。
    pane: Arc<Mutex<Option<String>>>,
    /// 最近一次本端视口尺寸 `(cols, rows)`；订阅时用它对齐远端 PTY。
    last_size: Arc<Mutex<(u16, u16)>>,
    /// claim-pane 单调序号。
    seq: Arc<AtomicU64>,
}

impl LanControllerSession {
    /// 当前已订阅的 pane id（probe / 诊断用）。
    pub fn current_pane(&self) -> Option<String> {
        self.pane.lock().clone()
    }
}

impl Session for LanControllerSession {
    fn send_input(&self, data: &[u8]) -> Result<()> {
        if let Some(pane) = self.pane.lock().clone() {
            // 终端输入为 UTF-8（ASCII 转义序列 + 文本）；与浏览器客户端一致按字符串回送。
            let s = String::from_utf8_lossy(data);
            let _ = self
                .to_host
                .send(Message::Text(lan_proto::stdin(&pane, s.as_ref())));
        }
        Ok(())
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        *self.last_size.lock() = (cols, rows);
        if let Some(pane) = self.pane.lock().clone() {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            let _ = self
                .to_host
                .send(Message::Text(lan_proto::claim_pane(&pane, rows, cols, seq)));
        }
        Ok(())
    }
}

/// 订阅一个 pane：记录为当前 pane、发 `subscribe-pane`，并按最近视口尺寸 `claim-pane`
/// 让远端 PTY reflow 到本端大小。
fn subscribe(
    to_host: &mpsc::UnboundedSender<Message>,
    pane: &Arc<Mutex<Option<String>>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
    pid: String,
) {
    *pane.lock() = Some(pid.clone());
    let _ = to_host.send(Message::Text(lan_proto::subscribe_pane(&pid)));
    let (cols, rows) = *last_size.lock();
    let s = seq.fetch_add(1, Ordering::Relaxed);
    let _ = to_host.send(Message::Text(lan_proto::claim_pane(&pid, rows, cols, s)));
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
    let query = match (&code, &token) {
        (Some(code), _) => format!("code={code}"),
        (None, Some(token)) => format!("token={token}"),
        (None, None) => return Err(anyhow!("需要 --code <TOTP> 或 --token <session>")),
    };
    let stream = connect_lan_stream(&hostport, &query).await?;
    let (sink, read) = stream.split();
    let (to_host_tx, to_host_rx) = mpsc::unbounded_channel::<Message>();
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(512);
    let pane = Arc::new(Mutex::new(None::<String>));
    let last_size = Arc::new(Mutex::new((80u16, 24u16)));
    let seq = Arc::new(AtomicU64::new(1));

    spawn_lan_writer(sink, to_host_rx);
    spawn_lan_reader(
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
            pane,
            last_size,
            seq,
        },
        out_rx,
    ))
}

async fn connect_lan_stream(hostport: &str, query: &str) -> Result<LanWsStream> {
    let wss = format!("wss://{hostport}/ws?{query}&device=rdg-cli");
    let connector = tls_connector()?;
    let request = wss
        .as_str()
        .into_client_request()
        .context("构造 WS 请求失败")?;
    match connect_async_tls_with_config(request, None, false, Some(connector)).await {
        Ok((stream, _)) => Ok(stream),
        Err(error) => {
            tracing::warn!(target: "ridge_cli", error = %error, "wss 连接失败，回退明文 ws");
            let plain = format!("ws://{hostport}/ws?{query}&device=rdg-cli");
            let request = plain
                .as_str()
                .into_client_request()
                .context("构造 WS 请求失败")?;
            let (stream, _) = connect_async(request).await.context("ws 连接失败")?;
            Ok(stream)
        }
    }
}

fn spawn_lan_writer(
    mut sink: SplitSink<LanWsStream, Message>,
    mut messages: mpsc::UnboundedReceiver<Message>,
) {
    tokio::spawn(async move {
        while let Some(message) = messages.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
}

fn spawn_lan_reader(
    mut read: SplitStream<LanWsStream>,
    to_host: mpsc::UnboundedSender<Message>,
    pane: Arc<Mutex<Option<String>>>,
    last_size: Arc<Mutex<(u16, u16)>>,
    seq: Arc<AtomicU64>,
    out_tx: mpsc::Sender<Vec<u8>>,
) {
    tokio::spawn(async move {
        while let Some(item) = read.next().await {
            let Ok(message) = item else {
                break;
            };
            if !handle_lan_message(message, &to_host, &pane, &last_size, &seq, &out_tx).await {
                break;
            }
        }
    });
}

async fn handle_lan_message(
    message: Message,
    to_host: &mpsc::UnboundedSender<Message>,
    pane: &Arc<Mutex<Option<String>>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
    out_tx: &mpsc::Sender<Vec<u8>>,
) -> bool {
    match message {
        Message::Text(text) => {
            handle_lan_text(&text, to_host, pane, last_size, seq);
            true
        }
        Message::Binary(buffer) => handle_lan_binary(&buffer, pane, out_tx).await,
        Message::Ping(payload) => {
            let _ = to_host.send(Message::Pong(payload));
            true
        }
        Message::Close(_) => false,
        _ => true,
    }
}

fn handle_lan_text(
    text: &str,
    to_host: &mpsc::UnboundedSender<Message>,
    pane: &Arc<Mutex<Option<String>>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    match value["type"].as_str() {
        Some("hello") => {
            let _ = to_host.send(Message::Text(lan_proto::list_panes()));
        }
        Some("panes") => handle_panes_frame(&value, to_host, pane, last_size, seq),
        Some("create-pane-result") => {
            handle_create_pane_frame(&value, to_host, pane, last_size, seq)
        }
        _ => {}
    }
}

fn handle_panes_frame(
    value: &serde_json::Value,
    to_host: &mpsc::UnboundedSender<Message>,
    pane: &Arc<Mutex<Option<String>>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
) {
    let first = value["panes"]
        .as_array()
        .and_then(|panes| panes.first())
        .and_then(|pane| pane["id"].as_str())
        .map(String::from);
    match first {
        Some(pane_id) => subscribe(to_host, pane, last_size, seq, pane_id),
        None => {
            let _ = to_host.send(Message::Text(lan_proto::create_pane()));
        }
    }
}

fn handle_create_pane_frame(
    value: &serde_json::Value,
    to_host: &mpsc::UnboundedSender<Message>,
    pane: &Arc<Mutex<Option<String>>>,
    last_size: &Arc<Mutex<(u16, u16)>>,
    seq: &Arc<AtomicU64>,
) {
    if value["success"].as_bool() != Some(true) {
        return;
    }
    if let Some(pane_id) = value["paneId"].as_str() {
        subscribe(to_host, pane, last_size, seq, pane_id.to_string());
    }
}

async fn handle_lan_binary(
    buffer: &[u8],
    pane: &Arc<Mutex<Option<String>>>,
    out_tx: &mpsc::Sender<Vec<u8>>,
) -> bool {
    let Some(frame) = parse_binary_frame(buffer) else {
        return true;
    };
    let wanted = pane.lock().clone();
    if wanted.is_some() && wanted.as_deref() != Some(frame.pane_id.as_str()) {
        return true;
    }
    out_tx.send(frame.bytes).await.is_ok()
}
