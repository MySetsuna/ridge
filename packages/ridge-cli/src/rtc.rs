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
        ice_urls: Vec<String>,
        inbound: mpsc::Receiver<PeerInbound>,
        outbound: mpsc::Sender<PeerOutbound>,
    ) -> Result<DataChannelIo>;
}

#[cfg(feature = "rtc")]
mod imp {
    use super::*;
    use std::sync::Arc;
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
            ice_urls: Vec<String>,
            mut inbound: mpsc::Receiver<PeerInbound>,
            outbound: mpsc::Sender<PeerOutbound>,
        ) -> Result<DataChannelIo> {
            let api = APIBuilder::new().build();
            let config = RTCConfiguration {
                ice_servers: vec![RTCIceServer {
                    urls: if ice_urls.is_empty() {
                        vec![FALLBACK_STUN.to_string()]
                    } else {
                        ice_urls
                    },
                    ..Default::default()
                }],
                ..Default::default()
            };
            let pc = Arc::new(api.new_peer_connection(config).await?);

            // DataChannel：由 controller(offerer) 创建，host 通过 on_data_channel 接管。
            let (dc_in_tx, dc_in_rx) = mpsc::channel::<Vec<u8>>(256);
            let (dc_out_tx, mut dc_out_rx) = mpsc::channel::<Vec<u8>>(256);
            let dc_holder: Arc<tokio::sync::Mutex<Option<Arc<RTCDataChannel>>>> =
                Arc::new(tokio::sync::Mutex::new(None));

            let dc_holder_cb = dc_holder.clone();
            pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
                let dc_in_tx = dc_in_tx.clone();
                let dc_holder_cb = dc_holder_cb.clone();
                Box::pin(async move {
                    if dc.label() != DATA_CHANNEL_LABEL {
                        tracing::warn!(target: "ridge_cli::rtc", label = %dc.label(), "ignoring unexpected data channel");
                        return;
                    }
                    *dc_holder_cb.lock().await = Some(dc.clone());

                    let dc_in_tx2 = dc_in_tx.clone();
                    dc.on_message(Box::new(move |msg: DataChannelMessage| {
                        let dc_in_tx2 = dc_in_tx2.clone();
                        Box::pin(async move {
                            let _ = dc_in_tx2.send(msg.data.to_vec()).await;
                        })
                    }));
                    dc.on_open(Box::new(|| {
                        tracing::info!(target: "ridge_cli::rtc", "data channel open");
                        Box::pin(async {})
                    }));
                })
            }));

            // 本地 ICE candidate → 信令。
            let outbound_ice = outbound.clone();
            pc.on_ice_candidate(Box::new(move |cand| {
                let outbound_ice = outbound_ice.clone();
                Box::pin(async move {
                    let payload = match cand {
                        Some(c) => match c.to_json() {
                            Ok(init) => serde_json::to_value(init).ok(),
                            Err(_) => None,
                        },
                        None => None, // 收集结束
                    };
                    let _ = outbound_ice.send(PeerOutbound::Ice(payload)).await;
                })
            }));

            // 出站泵：dc_out_rx → DataChannel.send。等 DataChannel 就绪后再发。
            let dc_holder_send = dc_holder.clone();
            tokio::spawn(async move {
                while let Some(bytes) = dc_out_rx.recv().await {
                    // 自旋等待 channel 建立（offer 处理后很快就绪）。
                    let dc = loop {
                        if let Some(dc) = dc_holder_send.lock().await.clone() {
                            break dc;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    };
                    if let Err(e) = dc.send(&bytes::Bytes::from(bytes)).await {
                        tracing::warn!(target: "ridge_cli::rtc", error = %e, "data channel send failed");
                        break;
                    }
                }
            });

            // 信令输入泵：处理 offer / 远端 ICE。
            let pc_sig = pc.clone();
            let outbound_ans = outbound.clone();
            tokio::spawn(async move {
                while let Some(ev) = inbound.recv().await {
                    match ev {
                        PeerInbound::Offer(sdp) => {
                            if let Err(e) = handle_offer(&pc_sig, &outbound_ans, sdp).await {
                                tracing::error!(target: "ridge_cli::rtc", error = %e, "offer handling failed");
                            }
                        }
                        PeerInbound::Ice(Some(cand)) => {
                            if let Ok(init) = serde_json::from_value::<RTCIceCandidateInit>(cand) {
                                if let Err(e) = pc_sig.add_ice_candidate(init).await {
                                    tracing::warn!(target: "ridge_cli::rtc", error = %e, "add_ice_candidate failed");
                                }
                            }
                        }
                        PeerInbound::Ice(None) => { /* 远端候选收集结束，无需处理 */ }
                    }
                }
            });

            Ok(DataChannelIo {
                rx: dc_in_rx,
                tx: dc_out_tx,
            })
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
            _ice_urls: Vec<String>,
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
