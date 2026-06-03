//! 会话编排：信令 → WebRTC → E2EE → PTY，把各部件串成一条远控通路。
//!
//! 数据流（cli = host = answerer）：
//! ```text
//!   controller ──WS offer/ICE──▶ Signaling ──▶ HostPeer(answerer)
//!                                                    │ DataChannel(E2EE 帧)
//!   PTY 输出 ─16ms 攒批─▶ Session.seal ─────────────▶ tx ─▶ controller
//!   controller ─E2EE 帧─▶ rx ─▶ Session.open ─▶ ControlMsg ─▶ PTY 输入/resize/搜索
//! ```
//!
//! 握手（§7.1）：DataChannel 打开后先交换 `0x01 || pub(32)` 两条二进制消息，
//! 派生会话密钥后才放行业务帧。

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use tokio::sync::mpsc;

use crate::batching::BatchingBuffer;
use crate::e2ee::{Dir, Handshake, Session as CryptoSession};
use crate::fs_reuse::{self, SearchOptions};
use crate::protocol::{self, ControlMsg, HostMsg};
use crate::pty::PtyBridge;
use crate::rtc::{HostPeer, PeerInbound, PeerOutbound};
use crate::signaling::{SignalMsg, SignalSender};

/// 一个 controller 会话的生命周期（从 offer 到断开）。
pub struct RemoteSession;

impl RemoteSession {
    /// 跑一个完整会话直到对端断开或出错。`shell` / `cwd` 决定本地 shell。
    pub async fn run(
        peer: &impl HostPeer,
        ice_urls: Vec<String>,
        signaling: &SignalSender,
        signal_rx: &mut mpsc::Receiver<SignalMsg>,
        shell: Option<String>,
        cwd: Option<String>,
    ) -> Result<()> {
        // 1. 建立 answerer 的信令输入/输出泵接口。
        let (inbound_tx, inbound_rx) = mpsc::channel::<PeerInbound>(64);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<PeerOutbound>(64);

        let dc_io = peer.answer(ice_urls, inbound_rx, outbound_tx).await?;

        // 2. answerer 输出 → 信令 WS。
        //    （单独任务，因为信令 send 是 async 且与 inbound 解耦。）
        //    用 outbound_rx 把 answer/ICE 发回 relay。
        //    注意：Signaling::send 借 &self，这里用 outgoing 通道 clone 不可行，
        //    所以把它放在调用方循环里 drain。简单起见，这里转交一个独立任务，
        //    通过 signaling 的 send 串行发送。
        //    —— 为避免对 Signaling 生命周期取 'static，我们在本函数内循环 select。

        // 3. PTY。
        let (pty, mut pty_out_rx) = PtyBridge::spawn(shell.as_deref(), cwd.as_deref())?;

        // 4. E2EE 握手：先发本端公钥，等对端公钥。
        let mut dc_io = dc_io;
        let handshake = Handshake::new();
        dc_io
            .tx
            .send(handshake.encode_frame())
            .await
            .map_err(|_| anyhow!("data channel closed before handshake"))?;

        // 等对端握手帧（带超时，避免永久挂起）。
        let peer_pub = match tokio::time::timeout(Duration::from_secs(15), dc_io.rx.recv()).await {
            Ok(Some(frame)) => Handshake::parse_peer_frame(&frame)?,
            Ok(None) => bail!("data channel closed during handshake"),
            Err(_) => bail!("E2EE handshake timed out"),
        };
        let mut crypto = handshake.into_session(peer_pub, Dir::HostToController)?;

        // 5. 主循环：攒批 PTY 输出、解密控制消息、把 answer/ICE 转发到信令。
        let mut batch = BatchingBuffer::new();

        loop {
            // 计算攒批截止时刻（无数据时给一个远期 sleep 占位）。
            let flush_sleep = match batch.deadline() {
                Some(dl) => tokio::time::sleep_until(dl.into()),
                None => tokio::time::sleep(Duration::from_secs(3600)),
            };
            tokio::pin!(flush_sleep);

            tokio::select! {
                // PTY 输出 → 攒批。
                maybe_out = pty_out_rx.recv() => {
                    match maybe_out {
                        Some(bytes) => {
                            batch.push(&bytes);
                            // 超过硬上限立即 flush。
                            if batch.should_flush() {
                                Self::flush_batch(&mut batch, &mut crypto, &dc_io.tx).await?;
                            }
                        }
                        None => {
                            // shell 退出：flush 残留后结束会话。
                            Self::flush_batch(&mut batch, &mut crypto, &dc_io.tx).await.ok();
                            tracing::info!(target: "ridge_cli::session", "shell exited; ending session");
                            break;
                        }
                    }
                }

                // 攒批窗口到点 → flush。
                _ = &mut flush_sleep => {
                    Self::flush_batch(&mut batch, &mut crypto, &dc_io.tx).await?;
                }

                // 来自 controller 的 E2EE 帧 → 解密 → 控制消息。
                maybe_in = dc_io.rx.recv() => {
                    match maybe_in {
                        Some(frame) => {
                            if let Err(e) = Self::handle_inbound(&frame, &mut crypto, &pty, &dc_io.tx).await {
                                tracing::warn!(target: "ridge_cli::session", error = %e, "inbound frame rejected");
                            }
                        }
                        None => {
                            tracing::info!(target: "ridge_cli::session", "data channel closed; ending session");
                            break;
                        }
                    }
                }

                // answerer 的 answer/ICE → 信令 WS。
                maybe_sig = outbound_rx.recv() => {
                    if let Some(out) = maybe_sig {
                        let msg = match out {
                            PeerOutbound::Answer(sdp) => SignalMsg::Answer { sdp },
                            PeerOutbound::Ice(candidate) => SignalMsg::Ice { candidate },
                        };
                        signaling.send(msg).await.ok();
                    }
                }

                // 来自 relay 的信令（offer / 远端 ICE / peer-leave）。
                maybe_relay = signal_rx.recv() => {
                    match maybe_relay {
                        Some(SignalMsg::Offer { sdp }) => {
                            inbound_tx.send(PeerInbound::Offer(sdp)).await.ok();
                        }
                        Some(SignalMsg::Ice { candidate }) => {
                            inbound_tx.send(PeerInbound::Ice(candidate)).await.ok();
                        }
                        Some(SignalMsg::PeerLeave { .. }) => {
                            tracing::info!(target: "ridge_cli::session", "controller left; ending session");
                            break;
                        }
                        Some(SignalMsg::Error { code, message }) => {
                            tracing::warn!(target: "ridge_cli::session", %code, %message, "signaling error");
                            // REPLACED 等错误：结束本会话。
                            break;
                        }
                        Some(_) => { /* welcome / peer-join 已在上层处理 */ }
                        None => {
                            tracing::info!(target: "ridge_cli::session", "signaling closed; ending session");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// flush 攒批缓冲：取走合并大包 → seal → DataChannel。
    async fn flush_batch(
        batch: &mut BatchingBuffer,
        crypto: &mut CryptoSession,
        tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<()> {
        if let Some(merged) = batch.take() {
            let plaintext = protocol::frame_pty_output(&merged);
            let sealed = crypto.seal(&plaintext)?;
            tx.send(sealed)
                .await
                .map_err(|_| anyhow!("data channel send channel closed"))?;
        }
        Ok(())
    }

    /// 解密一帧 controller→host，按控制消息分派。
    async fn handle_inbound(
        frame: &[u8],
        crypto: &mut CryptoSession,
        pty: &PtyBridge,
        tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<()> {
        let plaintext = crypto.open(frame)?;
        let msg: ControlMsg = serde_json::from_slice(&plaintext)
            .map_err(|e| anyhow!("control message parse failed: {e}"))?;
        match msg {
            ControlMsg::Input { data } => {
                pty.write_input(data.as_bytes())?;
            }
            ControlMsg::Resize { cols, rows } => {
                pty.resize(cols, rows)?;
            }
            ControlMsg::Search {
                root,
                query,
                use_regex,
                case_sensitive,
            } => {
                let opts = SearchOptions {
                    use_regex,
                    case_sensitive,
                    ..Default::default()
                };
                let results = fs_reuse::search_text(std::path::Path::new(&root), &query, &opts);
                let reply = protocol::frame_host_json(&HostMsg::SearchResult { results });
                let sealed = crypto.seal(&reply)?;
                tx.send(sealed).await.ok();
            }
            ControlMsg::Tree { path } => {
                let reply = match fs_reuse::list_dir(std::path::Path::new(&path)) {
                    Ok(entries) => HostMsg::Tree { entries },
                    Err(e) => HostMsg::Error {
                        // 不泄露内部细节（rust/security.md）。
                        message: format!("cannot list directory: {}", e.kind_str()),
                    },
                };
                let sealed = crypto.seal(&protocol::frame_host_json(&reply))?;
                tx.send(sealed).await.ok();
            }
        }
        Ok(())
    }
}

/// 给 `std::io::Error` 一个不泄露路径的简短类别串。
trait IoErrorKindStr {
    fn kind_str(&self) -> &'static str;
}
impl IoErrorKindStr for std::io::Error {
    fn kind_str(&self) -> &'static str {
        use std::io::ErrorKind::*;
        match self.kind() {
            NotFound => "not found",
            PermissionDenied => "permission denied",
            _ => "io error",
        }
    }
}
