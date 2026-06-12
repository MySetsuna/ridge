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
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, connect_async_tls_with_config, Connector};

use super::lan_proto::{self, parse_binary_frame};
use super::session::Session;

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
        (Some(c), _) => format!("code={c}"),
        (None, Some(t)) => format!("token={t}"),
        (None, None) => return Err(anyhow!("需要 --code <TOTP> 或 --token <session>")),
    };

    // 默认 wss + 接受自签；失败回退明文 ws（host 只在无法产生证书时才退明文）。
    let wss = format!("wss://{hostport}/ws?{query}&device=rdg-cli");
    let connector = tls_connector()?;
    let req = wss
        .as_str()
        .into_client_request()
        .context("构造 WS 请求失败")?;
    let stream = match connect_async_tls_with_config(req, None, false, Some(connector)).await {
        Ok((s, _)) => s,
        Err(e) => {
            tracing::warn!(target: "ridge_cli", error = %e, "wss 连接失败，回退明文 ws");
            let plain = format!("ws://{hostport}/ws?{query}&device=rdg-cli");
            let req2 = plain
                .as_str()
                .into_client_request()
                .context("构造 WS 请求失败")?;
            let (s, _) = connect_async(req2).await.context("ws 连接失败")?;
            s
        }
    };

    let (mut sink, mut read) = stream.split();
    let (to_host_tx, mut to_host_rx) = mpsc::unbounded_channel::<Message>();
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(512);

    let pane = Arc::new(Mutex::new(None::<String>));
    let last_size = Arc::new(Mutex::new((80u16, 24u16)));
    let seq = Arc::new(AtomicU64::new(1));

    // 写任务：把出站帧排空到 WS sink。
    tokio::spawn(async move {
        while let Some(msg) = to_host_rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    // 读任务：握手 + 把入站 PTY 字节灌入输出通道。
    {
        let to_host = to_host_tx.clone();
        let pane = pane.clone();
        let last_size = last_size.clone();
        let seq = seq.clone();
        tokio::spawn(async move {
            while let Some(item) = read.next().await {
                let msg = match item {
                    Ok(m) => m,
                    Err(_) => break,
                };
                match msg {
                    Message::Text(txt) => {
                        let Ok(v) = serde_json::from_str::<serde_json::Value>(&txt) else {
                            continue;
                        };
                        match v["type"].as_str() {
                            Some("hello") => {
                                let _ = to_host.send(Message::Text(lan_proto::list_panes()));
                            }
                            Some("panes") => {
                                let first = v["panes"]
                                    .as_array()
                                    .and_then(|a| a.first())
                                    .and_then(|p| p["id"].as_str())
                                    .map(String::from);
                                match first {
                                    Some(pid) => {
                                        subscribe(&to_host, &pane, &last_size, &seq, pid)
                                    }
                                    None => {
                                        let _ = to_host
                                            .send(Message::Text(lan_proto::create_pane()));
                                    }
                                }
                            }
                            Some("create-pane-result") => {
                                if v["success"].as_bool() == Some(true) {
                                    if let Some(pid) = v["paneId"].as_str() {
                                        subscribe(
                                            &to_host,
                                            &pane,
                                            &last_size,
                                            &seq,
                                            pid.to_string(),
                                        );
                                    }
                                }
                            }
                            // pong / pty-meta / pty-resized / event / theme / error：信息帧，忽略。
                            _ => {}
                        }
                    }
                    Message::Binary(buf) => {
                        if let Some(frame) = parse_binary_frame(&buf) {
                            // 单 pane 控制端：只转发已订阅 pane 的字节（订阅前的极少数帧放行）。
                            let want = pane.lock().clone();
                            if want.is_none() || want.as_deref() == Some(frame.pane_id.as_str()) {
                                if out_tx.send(frame.bytes).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    Message::Ping(p) => {
                        let _ = to_host.send(Message::Pong(p));
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            // 读任务结束 → out_tx 落出作用域 → 输出通道关闭 → run_session 主循环退出。
        });
    }

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
