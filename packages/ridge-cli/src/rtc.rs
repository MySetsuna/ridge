//! host 侧 WebRTC（契约 §0/§5/§7）。
//!
//! cli 永远是 **answerer**：等 controller 的 offer，建立 RTCPeerConnection，开
//! DataChannel(`label="ridge"`, ordered)，在其上叠 §7 E2EE，桥接 PTY↔DataChannel。
//!
//! 通过 `HostPeer` trait 抽象，便于：
//! - `feature = "rtc"`（默认）：用 `webrtc` crate 真实实现。
//! - 关掉特性：编译期 stub（清晰 TODO），让设备码流 / E2EE / PTY / 攒批 / 信令
//!   在受限 CI 上也能编译过（契约允许 RTCPeerConnection 部分 stub，其余必须真实）。

use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::ice::IceServerConfig;

/// DataChannel 标签（契约 §7）。
pub const DATA_CHANNEL_LABEL: &str = "ridge";
/// v1 公共 STUN（契约 §5.2，仅作 ice-servers 拉取失败时的兜底）。
pub const FALLBACK_STUN: &str = "stun:stun.l.google.com:19302";

/// 从信令送达 host 的事件（answerer 输入）。
#[derive(Debug)]
pub enum PeerInbound {
    /// controller 的 offer SDP。
    Offer(String),
    /// 远端 ICE candidate（`None` = 收集结束）。
    Ice(Option<Value>),
}

/// host 发回信令的事件（answerer 输出）。
#[derive(Debug)]
pub enum PeerOutbound {
    /// 本地 answer SDP。
    Answer(String),
    /// 本地 ICE candidate（`None` = 收集结束）。
    Ice(Option<Value>),
}

/// DataChannel 上的双向字节流（E2EE 帧）。
pub struct DataChannelIo {
    /// 收：来自 controller 的 E2EE 帧。
    pub rx: mpsc::Receiver<Vec<u8>>,
    /// 发：要发给 controller 的 E2EE 帧。
    pub tx: mpsc::Sender<Vec<u8>>,
}

/// host answerer 抽象。
#[allow(async_fn_in_trait)]
pub trait HostPeer {
    /// 处理一次 controller 会话：消费信令输入、产出信令输出，
    /// 返回 DataChannel 打开后的双向字节通道。
    async fn answer(
        &self,
        ice_servers: Vec<IceServerConfig>,
        inbound: mpsc::Receiver<PeerInbound>,
        outbound: mpsc::Sender<PeerOutbound>,
    ) -> Result<DataChannelIo>;
}

#[cfg(feature = "rtc")]
mod imp {
    use super::{
        DataChannelIo, HostPeer, PeerInbound, PeerOutbound, DATA_CHANNEL_LABEL, FALLBACK_STUN,
    };
    use crate::ice::IceServerConfig;
    use anyhow::Result;
    use serde_json::Value;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use webrtc::api::APIBuilder;
    use webrtc::data_channel::data_channel_message::DataChannelMessage;
    use webrtc::data_channel::RTCDataChannel;
    use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
    use webrtc::ice_transport::ice_server::RTCIceServer;
    use webrtc::peer_connection::configuration::RTCConfiguration;
    use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

    /// 真实 WebRTC 实现。
    pub struct WebRtcHost;

    impl HostPeer for WebRtcHost {
        async fn answer(
            &self,
            ice_servers: Vec<IceServerConfig>,
            inbound: mpsc::Receiver<PeerInbound>,
            outbound: mpsc::Sender<PeerOutbound>,
        ) -> Result<DataChannelIo> {
            let api = APIBuilder::new().build();
            let rtc_ice_servers = map_ice_servers(ice_servers);
            let config = RTCConfiguration {
                ice_servers: rtc_ice_servers,
                ..Default::default()
            };
            let pc = Arc::new(api.new_peer_connection(config).await?);

            // DataChannel：由 controller(offerer) 创建，host 通过 on_data_channel 接管。
            let (dc_in_tx, dc_in_rx) = mpsc::channel::<Vec<u8>>(256);
            let (dc_out_tx, dc_out_rx) = mpsc::channel::<Vec<u8>>(256);
            let dc_holder: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>> =
                Arc::new(tokio::sync::Mutex::new(None));

            register_data_channel(&pc, dc_in_tx, dc_holder.clone());

            // 本地 ICE candidate → 信令。
            register_ice_handler(&pc, outbound.clone());

            // 出站泵：dc_out_rx → DataChannel.send。等 DataChannel 就绪后再发。
            spawn_data_channel_sender(dc_holder.clone(), dc_out_rx);

            // 信令输入泵：处理 offer / 远端 ICE。
            spawn_signaling_receiver(pc.clone(), inbound, outbound.clone());

            Ok(DataChannelIo {
                rx: dc_in_rx,
                tx: dc_out_tx,
            })
        }
    }

    fn map_ice_servers(ice_servers: Vec<IceServerConfig>) -> Vec<RTCIceServer> {
        if ice_servers.is_empty() {
            return vec![RTCIceServer {
                urls: vec![FALLBACK_STUN.to_string()],
                ..Default::default()
            }];
        }
        ice_servers
            .into_iter()
            .map(|server| RTCIceServer {
                urls: server.urls,
                username: server.username.unwrap_or_default(),
                credential: server.credential.unwrap_or_default(),
                ..Default::default()
            })
            .collect()
    }

    fn register_data_channel(
        pc: &Arc<webrtc::peer_connection::RTCPeerConnection>,
        dc_in_tx: mpsc::Sender<Vec<u8>>,
        dc_holder: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>>,
    ) {
        pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let dc_in_tx = dc_in_tx.clone();
            let dc_holder = dc_holder.clone();
            Box::pin(async move {
                if dc.label() != DATA_CHANNEL_LABEL {
                    tracing::warn!(target: "ridge_cli::rtc", label = %dc.label(), "ignoring unexpected data channel");
                    return;
                }
                *dc_holder.lock().await = Some(dc.clone());
                let dc_in_tx = dc_in_tx.clone();
                dc.on_message(Box::new(move |msg: DataChannelMessage| {
                    let dc_in_tx = dc_in_tx.clone();
                    Box::pin(async move {
                        let _ = dc_in_tx.send(msg.data.to_vec()).await;
                    })
                }));
                dc.on_open(Box::new(|| {
                    tracing::info!(target: "ridge_cli::rtc", "data channel open");
                    Box::pin(async {})
                }));
            })
        }));
    }

    fn register_ice_handler(
        pc: &Arc<webrtc::peer_connection::RTCPeerConnection>,
        outbound: mpsc::Sender<PeerOutbound>,
    ) {
        pc.on_ice_candidate(Box::new(move |candidate| {
            let outbound = outbound.clone();
            Box::pin(async move {
                let payload = candidate
                    .and_then(|candidate| candidate.to_json().ok())
                    .and_then(|candidate| serde_json::to_value(candidate).ok());
                let _ = outbound.send(PeerOutbound::Ice(payload)).await;
            })
        }));
    }

    fn spawn_data_channel_sender(
        dc_holder: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>>,
        mut dc_out_rx: mpsc::Receiver<Vec<u8>>,
    ) {
        tokio::spawn(async move {
            while let Some(bytes) = dc_out_rx.recv().await {
                let dc = wait_for_data_channel(&dc_holder).await;
                if let Err(e) = dc.send(&bytes::Bytes::from(bytes)).await {
                    tracing::warn!(target: "ridge_cli::rtc", error = %e, "data channel send failed");
                    break;
                }
            }
        });
    }

    async fn wait_for_data_channel(
        dc_holder: &Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>>,
    ) -> Arc<RTCDataChannel> {
        loop {
            if let Some(dc) = dc_holder.lock().await.clone() {
                return dc;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    fn spawn_signaling_receiver(
        pc: Arc<webrtc::peer_connection::RTCPeerConnection>,
        mut inbound: mpsc::Receiver<PeerInbound>,
        outbound: mpsc::Sender<PeerOutbound>,
    ) {
        tokio::spawn(async move {
            while let Some(event) = inbound.recv().await {
                handle_signaling_event(&pc, &outbound, event).await;
            }
        });
    }

    async fn handle_signaling_event(
        pc: &Arc<webrtc::peer_connection::RTCPeerConnection>,
        outbound: &mpsc::Sender<PeerOutbound>,
        event: PeerInbound,
    ) {
        match event {
            PeerInbound::Offer(sdp) => {
                if let Err(error) = handle_offer(pc, outbound, sdp).await {
                    tracing::error!(target: "ridge_cli::rtc", error = %error, "offer handling failed");
                }
            }
            PeerInbound::Ice(Some(candidate)) => add_ice_candidate(pc, candidate).await,
            PeerInbound::Ice(None) => {}
        }
    }

    async fn add_ice_candidate(
        pc: &Arc<webrtc::peer_connection::RTCPeerConnection>,
        candidate: Value,
    ) {
        let Ok(init) = serde_json::from_value::<RTCIceCandidateInit>(candidate) else {
            return;
        };
        if let Err(error) = pc.add_ice_candidate(init).await {
            tracing::warn!(target: "ridge_cli::rtc", error = %error, "add_ice_candidate failed");
        }
    }

    async fn handle_offer(
        pc: &Arc<webrtc::peer_connection::RTCPeerConnection>,
        outbound: &mpsc::Sender<PeerOutbound>,
        sdp: String,
    ) -> Result<()> {
        let offer = RTCSessionDescription::offer(sdp)?;
        pc.set_remote_description(offer).await?;
        let answer = pc.create_answer(None).await?;
        pc.set_local_description(answer.clone()).await?;
        outbound.send(PeerOutbound::Answer(answer.sdp)).await.ok();
        Ok(())
    }
}

#[cfg(not(feature = "rtc"))]
mod imp {
    use super::*;

    /// 编译期 stub（`--no-default-features` 时启用）。RTCPeerConnection 未集成；
    /// 设备码流 / E2EE / PTY / 攒批 / 信令仍真实可用。
    ///
    /// TODO(rtc): 启用 `rtc` 特性以获得真实 WebRTC answerer。
    pub struct WebRtcHost;

    impl HostPeer for WebRtcHost {
        async fn answer(
            &self,
            _ice_servers: Vec<IceServerConfig>,
            _inbound: mpsc::Receiver<PeerInbound>,
            _outbound: mpsc::Sender<PeerOutbound>,
        ) -> Result<DataChannelIo> {
            anyhow::bail!(
                "WebRTC host disabled: rebuild with the `rtc` feature (default) to enable peer connections"
            )
        }
    }
}

pub use imp::WebRtcHost;
