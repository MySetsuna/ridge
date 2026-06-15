//! 信令 WebSocket 客户端（契约 §5）。
//!
//! cli 作为 host（answerer）连接 `wss://{device}-{username}.{base}/ws?token=&role=host`，
//! 接收服务端连接事件 + controller 转发的 offer / ICE，回 answer / ICE。relay 只
//! 逐字转发，绝不解析内容。

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

// 信令消息 schema 收敛到单一事实来源 `ridge-signaling`（findings-align P0/P1）。
// 该 crate 的 `SignalMsg` 是四端（relay / 桌面 host / 桌面 controller / 本 cli）共用的
// **唯一定义**，且**全程承载可选 `cid`** —— 这正是修复「浏览器 controller 连不上无头
// host」P0 的关键：relay 要求 host→controller 的 answer/ice **必带 cid**，缺失即丢弃；
// 旧的本地 `SignalMsg` 无 cid 字段，故 cli 回的 answer 永远到不了 controller。改用共享
// 类型后，cli 把入站 offer 携带的 cid 原样回盖到 answer/ice 即可被 relay 正确路由。
pub use ridge_signaling::{Role, SignalMsg};

/// 往 relay 发信令的句柄（cheap-clone，可与 `incoming` 同时持有，规避借用冲突）。
#[derive(Clone)]
pub struct SignalSender {
    outgoing: mpsc::Sender<SignalMsg>,
}

impl SignalSender {
    /// 发一条信令消息。
    pub async fn send(&self, msg: SignalMsg) -> anyhow::Result<()> {
        self.outgoing
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("signaling send channel closed"))
    }

    /// 测试用构造：从一个 `mpsc::Sender` 直接造发送句柄（绕过真实 WS），便于在
    /// session 测试里断言 host 发出的信令（answer/e2ee-pubkey 等）。
    #[cfg(test)]
    pub(crate) fn new_for_test(outgoing: mpsc::Sender<SignalMsg>) -> Self {
        Self { outgoing }
    }
}

/// 信令连接句柄：`incoming` 收服务端 / 对端消息；`sender()` 取发送句柄。
pub struct Signaling {
    pub incoming: mpsc::Receiver<SignalMsg>,
    outgoing: mpsc::Sender<SignalMsg>,
}

impl Signaling {
    /// 连接信令 WS。`ws_url` 必须已带 `?token=&role=host`（见 `AuthFile::signaling_ws_url`）。
    pub async fn connect(ws_url: &str) -> Result<Self> {
        // 生产：默认 webpki-roots 校验（公共 CA，严格）。dev 模式（base domain 含
        // localhost/127.0.0.1）：本地 ridge-cloud 用 rcgen 自签证书，webpki-roots 会拒绝
        // ——故仅此时复用 LAN 那套 `AcceptAnyServerCert` 连接器接受自签。
        // **严格仅 `is_dev_mode()` 放开；生产路径绝不接受自签，防降级 MITM。**
        let (ws_stream, _resp) = if crate::config::is_dev_mode() {
            tracing::warn!(
                target: "ridge_cli::signaling",
                "dev 模式：信令 WS 接受自签 TLS（仅本地 ridge-cloud；生产走公共 CA 严格校验）"
            );
            let connector = crate::tui::lan_session::tls_connector()
                .context("dev 自签 TLS 连接器构造失败")?;
            tokio_tungstenite::connect_async_tls_with_config(ws_url, None, false, Some(connector))
                .await
        } else {
            tokio_tungstenite::connect_async(ws_url).await
        }
        .context("signaling WS connect failed")?;
        let (mut write, mut read) = ws_stream.split();

        let (in_tx, in_rx) = mpsc::channel::<SignalMsg>(64);
        let (out_tx, mut out_rx) = mpsc::channel::<SignalMsg>(64);

        // 读任务：WS 文本帧 → SignalMsg → incoming 通道。
        tokio::spawn(async move {
            while let Some(frame) = read.next().await {
                match frame {
                    Ok(Message::Text(txt)) => match serde_json::from_str::<SignalMsg>(&txt) {
                        Ok(msg) => {
                            if in_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(target: "ridge_cli::signaling", error = %e, raw = %txt, "unparseable signaling frame");
                        }
                    },
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Binary(_)) => {}
                    Ok(Message::Frame(_)) => {}
                    Err(e) => {
                        tracing::warn!(target: "ridge_cli::signaling", error = %e, "signaling read error");
                        break;
                    }
                }
            }
            tracing::info!(target: "ridge_cli::signaling", "signaling read loop ended");
        });

        // 写任务：outgoing 通道 → WS 文本帧。
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                let txt = match serde_json::to_string(&msg) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!(target: "ridge_cli::signaling", error = %e, "signaling encode failed");
                        continue;
                    }
                };
                if write.send(Message::Text(txt)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            incoming: in_rx,
            outgoing: out_tx,
        })
    }

    /// 取一个 cheap-clone 的发送句柄（可与 `incoming` 同时持有，避免借用冲突）。
    pub fn sender(&self) -> SignalSender {
        SignalSender {
            outgoing: self.outgoing.clone(),
        }
    }
}

// 信令消息的 (de)序列化测试已上移到 `ridge-signaling` 的跨语言 golden-fixture
// conformance（SSOT 所有者负责锁线形）；此处不再重复本地 serde 单测。
