//! 信令 WebSocket 客户端（契约 §5）。
//!
//! cli 作为 host（answerer）连接 `wss://{device}-{username}.{base}/ws?token=&role=host`，
//! 接收服务端连接事件 + controller 转发的 offer / ICE，回 answer / ICE。relay 只
//! 逐字转发，绝不解析内容。

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

/// 信令消息（契约 §5.1，tag 字段为 `t`）。涵盖服务端连接事件与两端互发的
/// SDP / ICE 帧。`#[serde(other)]` 兜底未知 tag，向后兼容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum SignalMsg {
    /// 服务端：连接成功。`peer_present` 表示房间里是否已有对端。
    Welcome {
        room: String,
        role: String,
        #[serde(rename = "peerPresent", default)]
        peer_present: bool,
    },
    /// 服务端：对端加入。
    PeerJoin { role: String },
    /// 服务端：对端离开。
    PeerLeave { role: String },
    /// 服务端：错误。
    Error {
        code: String,
        #[serde(default)]
        message: String,
    },
    /// controller→host：SDP offer。
    Offer { sdp: String },
    /// host→controller：SDP answer。
    Answer { sdp: String },
    /// 两端互发：ICE candidate（`null` 表示候选收集结束）。
    Ice { candidate: Option<Value> },
}

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
}

/// 信令连接句柄：`incoming` 收服务端 / 对端消息；`sender()` 取发送句柄。
pub struct Signaling {
    pub incoming: mpsc::Receiver<SignalMsg>,
    outgoing: mpsc::Sender<SignalMsg>,
}

impl Signaling {
    /// 连接信令 WS。`ws_url` 必须已带 `?token=&role=host`（见 `AuthFile::signaling_ws_url`）。
    pub async fn connect(ws_url: &str) -> Result<Self> {
        let (ws_stream, _resp) = tokio_tungstenite::connect_async(ws_url)
            .await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn welcome_parses_peer_present_camelcase() {
        let m: SignalMsg = serde_json::from_str(
            r#"{"t":"welcome","room":"vps-bob","role":"host","peerPresent":true}"#,
        )
        .unwrap();
        match m {
            SignalMsg::Welcome {
                room,
                role,
                peer_present,
            } => {
                assert_eq!(room, "vps-bob");
                assert_eq!(role, "host");
                assert!(peer_present);
            }
            _ => panic!("expected welcome"),
        }
    }

    #[test]
    fn offer_and_ice_parse() {
        let offer: SignalMsg = serde_json::from_str(r#"{"t":"offer","sdp":"v=0..."}"#).unwrap();
        matches!(offer, SignalMsg::Offer { .. });

        let ice_null: SignalMsg = serde_json::from_str(r#"{"t":"ice","candidate":null}"#).unwrap();
        match ice_null {
            SignalMsg::Ice { candidate } => assert!(candidate.is_none()),
            _ => panic!("expected ice"),
        }
    }

    #[test]
    fn answer_serializes_with_tag() {
        let a = SignalMsg::Answer {
            sdp: "v=0".to_string(),
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(s.contains("\"t\":\"answer\""));
        assert!(s.contains("\"sdp\":\"v=0\""));
    }
}
