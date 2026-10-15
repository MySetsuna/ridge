//! 信令 WebSocket 客户端（契约 §5）。
//!
//! cli 作为 host（answerer）连接 `wss://{device}-{username}.{base}/ws?token=&role=host`，
//! 接收服务端连接事件 + controller 转发的 offer / ICE，回 answer / ICE。relay 只
//! 逐字转发，绝不解析内容。

use anyhow::{Context, Result};
use futures::{Sink, SinkExt, Stream, StreamExt};
use tokio::sync::mpsc;
use cloud_ws::tungstenite::Message;

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
    pub fn from_channel(outgoing: mpsc::Sender<SignalMsg>) -> Self {
        Self { outgoing }
    }
}

/// 信令连接句柄：`incoming` 收服务端 / 对端消息；`sender()` 取发送句柄。
pub struct Signaling<E=SignalMsg> {
    pub incoming: mpsc::Receiver<E>,
    outgoing: mpsc::Sender<SignalMsg>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl<E> Drop for Signaling<E> {fn drop(&mut self){for task in &self.tasks {task.abort();}}}

impl<E:serde::de::DeserializeOwned+Send+'static> Signaling<E> {
    /// 连接信令 WS。`ws_url` 必须已带 `?token=&role=host`（见 `AuthFile::signaling_ws_url`）。
    pub async fn connect(ws_url: &str) -> Result<Self> {
        use cloud_ws::tungstenite::client::IntoClientRequest;
        let parsed=url::Url::parse(ws_url).context("invalid signaling endpoint")?;
        let local=parsed.host_str().is_some_and(|host|host=="localhost"||host.ends_with(".localhost")||host=="127.0.0.1"||host=="[::1]");
        if !matches!(parsed.scheme(),"wss"|"ws")||parsed.scheme()=="ws"&&!local {anyhow::bail!("public signaling requires TLS");}
        let mut target=parsed.clone();
        if parsed.host_str().is_some_and(|host|host.ends_with(".localhost")) {target.set_host(Some("127.0.0.1")).context("local signaling address")?;}
        let mut request=target.as_str().into_client_request()?;
        request.headers_mut().insert("host",parsed[url::Position::BeforeHost..url::Position::AfterPort].parse().context("signaling host identity")?);
        let connected=if local&&parsed.scheme()=="wss" {
            cloud_ws::connect_async_tls_with_config(request,None,false,Some(super::local_tls::tls_connector()?)).await
        } else {cloud_ws::connect_async(request).await};
        let (ws_stream,_resp)=connected.context("signaling WS connect failed")?;
        let (write, read) = ws_stream.split();

        let (in_tx, in_rx) = mpsc::channel::<E>(64);
        let (out_tx, out_rx) = mpsc::channel::<SignalMsg>(64);

        // 读任务：WS 文本帧 → SignalMsg → incoming 通道。
        let read_task=tokio::spawn(read_signaling(read, in_tx));

        // 写任务：outgoing 通道 → WS 文本帧。
        let write_task=tokio::spawn(write_signaling(write, out_rx));

        Ok(Self {
            incoming: in_rx,
            outgoing: out_tx,
            tasks:vec![read_task,write_task],
        })
    }

    /// 取一个 cheap-clone 的发送句柄（可与 `incoming` 同时持有，避免借用冲突）。
    pub fn sender(&self) -> SignalSender {
        SignalSender {
            outgoing: self.outgoing.clone(),
        }
    }
}

async fn read_signaling<R,E:serde::de::DeserializeOwned>(mut read: R, in_tx: mpsc::Sender<E>)
where
    R: Stream<Item = Result<Message, cloud_ws::tungstenite::Error>> + Unpin,
{
    while let Some(frame) = read.next().await {
        match frame {
            Ok(Message::Text(txt)) => match serde_json::from_str::<E>(&txt) {
                Ok(msg) => {
                    if in_tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(e) => tracing::warn!(
                    target: "ridge_cli::signaling",
                    error = %e,
                    "unparseable signaling frame"
                ),
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
}

async fn write_signaling<W>(mut write: W, mut out_rx: mpsc::Receiver<SignalMsg>)
where
    W: Sink<Message, Error = cloud_ws::tungstenite::Error> + Unpin,
{
    while let Some(msg) = out_rx.recv().await {
        let Ok(txt) = serde_json::to_string(&msg) else {
            tracing::warn!(target: "ridge_cli::signaling", "signaling encode failed");
            continue;
        };
        if write.send(Message::Text(txt)).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    struct RecordingSink {
        messages: Arc<Mutex<Vec<Message>>>,
    }

    impl Sink<Message> for RecordingSink {
        type Error = cloud_ws::tungstenite::Error;

        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            self.get_mut().messages.lock().unwrap().push(item);
            Ok(())
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn read_signaling_accepts_json_and_ignores_non_data_frames() {
        let (tx, mut rx) = mpsc::channel(4);
        let frames = stream::iter(vec![
            Ok(Message::Text(
                r#"{"t":"peer-join","role":"controller","cid":"c1"}"#.into(),
            )),
            Ok(Message::Text("not-json".into())),
            Ok(Message::Ping(Vec::new().into())),
            Ok(Message::Binary(vec![1].into())),
            Ok(Message::Pong(Vec::new().into())),
            Ok(Message::Close(None)),
        ]);

        read_signaling(frames, tx).await;

        assert_eq!(
            rx.recv().await,
            Some(SignalMsg::PeerJoin {
                role: Role::Controller,
                cid: Some("c1".to_string()),
            }),
        );
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn read_signaling_stops_when_incoming_channel_is_closed() {
        let (tx, rx) = mpsc::channel::<SignalMsg>(1);
        drop(rx);
        let frames = stream::iter(vec![Ok(Message::Text(
            r#"{"t":"peer-join","role":"controller"}"#.into(),
        ))]);

        read_signaling(frames, tx).await;
    }

    #[tokio::test]
    async fn write_signaling_serializes_messages_until_sender_closes() {
        let messages = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink {
            messages: messages.clone(),
        };
        let (tx, rx) = mpsc::channel(1);
        tx.send(SignalMsg::Error {
            code: "TEST".to_string(),
            message: "failed".to_string(),
        })
        .await
        .unwrap();
        drop(tx);

        write_signaling(sink, rx).await;

        let messages = messages.lock().unwrap();
        assert_eq!(messages.len(), 1);
        let Message::Text(text) = &messages[0] else {
            panic!("expected a text signaling frame");
        };
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(value["t"], "error");
        assert_eq!(value["code"], "TEST");
    }
}

// 信令消息的 (de)序列化测试已上移到 `ridge-signaling` 的跨语言 golden-fixture
// conformance（SSOT 所有者负责锁线形）；此处不再重复本地 serde 单测。
