//! 会话编排：信令 → WebRTC → E2EE → PTY，把各部件串成一条远控通路。
//!
//! 统一远控 S3（契约 §11.1）：cli host 的 controller↔host 线协议**收敛到桌面同款**
//! mux + JSON-RPC，于是同一个浏览器 controller（`cloudControllerBoot`）既能驱动桌面
//! host 也能驱动本 cli host。
//!
//! 数据流（cli = host = answerer）：
//! ```text
//!   controller ──WS offer/ICE──▶ Signaling ──▶ HostPeer(answerer)
//!                                                    │ DataChannel(E2EE 帧)
//!   PTY 输出 ─16ms 攒批─▶ 0x10 PANE_RAW ───────────▶ tx ─▶ controller
//!   controller ─E2EE 帧─▶ rx ─▶ demux ─▶ 0x11 JSON-RPC ─▶ PTY 输入/resize/搜索/树
//!                                      └▶ 0x12 CONTROL  ─▶ TOTP 握手
//! ```
//!
//! 握手（§7.1）：DataChannel 打开后先交换 `0x01 || pub(32)` 两条二进制消息，
//! 派生会话密钥后才放行业务帧。
//!
//! 门控（契约 §4）：TOTP 验证通过前，业务 JSON-RPC（write_to_pty/resize_pane/search/
//! tree/get_active_workspace_id）一律拒绝（带 id 的回 JSON-RPC error，notification 丢弃），
//! 只放行 `$/hello`（协商）与 0x12 的 `totp-verify`。PTY 输出在 verified+subscribed 前
//! 暂存于攒批缓冲，验证并订阅后再连续推出（不丢初始 shell 提示符）。

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use tokio::sync::mpsc;

use std::path::PathBuf;

use crate::batching::BatchingBuffer;
use crate::core_host;
use crate::e2ee::{Dir, Handshake, Session as CryptoSession};
use crate::fs_reuse;
use crate::mux::{self, Inbound};
use crate::protocol::SessionControl;
use crate::pty::PtyBridge;
use crate::rpc::{
    self, Envelope, Method, RpcError, CANCEL_METHOD, HELLO_METHOD, JSON_RPC_INVALID_REQUEST,
    JSON_RPC_METHOD_NOT_FOUND,
};
use crate::rtc::{HostPeer, PeerInbound, PeerOutbound};
use crate::signaling::{SignalMsg, SignalSender};
use crate::totp::RemoteTotp;

/// cli host 服务的固定 pane id。cli 是单 pane terminal host：controller 订阅这个
/// 已知 paneId、对它 `write_to_pty`/`resize_pane`，host 用它给 PTY 字节打 0x10 帧。
pub const CLI_PANE_ID: &str = "ridge-cli-pane";

/// cli host 服务的固定 workspace id。controller 的 cloudPaneSource 用
/// `get_active_workspace_id` 解析活动 ws 以拼 `pty-output-{ws}-{pane}` event 名；
/// cli 是单 ws host，回这个固定值即可。
pub const CLI_WORKSPACE_ID: &str = "ridge-cli-ws";

/// 业务帧门控决策（纯逻辑，便于单测）：给定当前验证态与一帧入站信封，决定 host 行为。
#[derive(Debug, Clone, PartialEq)]
enum Gate {
    /// 放行：按 [`Envelope`] 处理。
    Allow(Envelope),
    /// 门控期收到带 id 的业务请求：回一个 JSON-RPC error（让 controller promise reject）。
    RejectWithError { id: Value },
    /// 门控期收到业务 notification（无 id）：静默丢弃。
    DropSilently,
    /// 非业务帧（忽略的坏帧 / response）。
    Ignore,
}

/// 判定一帧 0x11 JSON-RPC 信封在当前验证态下的处置（契约 §4 门控）。
/// `$/hello` 始终放行（协商在验证前进行）；其余业务方法在未验证时被拒。
fn gate_envelope(env: Envelope, verified: bool) -> Gate {
    match env {
        // 坏帧 / response → 始终忽略（与验证态无关）。
        Envelope::Ignore => Gate::Ignore,
        // $/hello 协商在门控前进行（controller 需先知道 host 能力）。
        Envelope::Notification { ref method, .. } if method == HELLO_METHOD => Gate::Allow(env),
        Envelope::Request { ref method, .. } if method == HELLO_METHOD => Gate::Allow(env),
        _ if verified => Gate::Allow(env),
        // 未验证：带 id 的请求回 error，notification 丢弃。
        Envelope::Request { id, .. } => Gate::RejectWithError { id },
        Envelope::Notification { .. } => Gate::DropSilently,
    }
}

/// 一个 controller 会话的生命周期（从 offer 到断开）。
pub struct RemoteSession;

impl RemoteSession {
    /// 跑一个完整会话直到对端断开或出错。`shell` / `cwd` 决定本地 shell；
    /// `root` 覆盖 fs 服务根沙箱（D-GM-9，缺省回退 `cwd` → 进程当前目录）。
    pub async fn run(
        peer: &impl HostPeer,
        ice_urls: Vec<String>,
        signaling: &SignalSender,
        signal_rx: &mut mpsc::Receiver<SignalMsg>,
        shell: Option<String>,
        cwd: Option<String>,
        root: Option<String>,
    ) -> Result<()> {
        // fs 服务根沙箱（D-GM-9）：本会话所有 search / list_dir 命令限定于此。
        let roots: Vec<PathBuf> = core_host::resolve_serving_roots(root.as_deref(), cwd.as_deref());
        if roots.is_empty() {
            tracing::warn!(
                target: "ridge_cli::session",
                "fs serving root unset and CWD unreadable; filesystem commands are UNRESTRICTED \
                 (set --root / RIDGE_REMOTE_ROOT to confine)"
            );
        } else {
            tracing::info!(
                target: "ridge_cli::session",
                roots = ?roots,
                "fs commands confined to serving root(s)"
            );
        }

        // 1. 建立 answerer 的信令输入/输出泵接口。
        let (inbound_tx, inbound_rx) = mpsc::channel::<PeerInbound>(64);
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<PeerOutbound>(64);

        let dc_io = peer.answer(ice_urls, inbound_rx, outbound_tx).await?;

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

        let peer_pub = match tokio::time::timeout(Duration::from_secs(15), dc_io.rx.recv()).await {
            Ok(Some(frame)) => Handshake::parse_peer_frame(&frame)?,
            Ok(None) => bail!("data channel closed during handshake"),
            Err(_) => bail!("E2EE handshake timed out"),
        };
        let mut crypto = handshake.into_session(peer_pub, Dir::HostToController)?;

        // 4b. 云远控二次验证（契约 §4）：每会话一份随机 TOTP，打到 TUI。未验证前业务帧
        //     被门控（见 gate_envelope）。
        let totp = RemoteTotp::load_or_create(&crate::config::totp_identity());
        let mut verified = false;
        // controller 经 `subscribe-pane` / `register_pane_delta_channel` 订阅后才推 PTY 流
        //（桌面同款语义）。订阅 + 验证前 PTY 输出暂存于攒批缓冲，不丢失初始提示符。
        let mut subscribed = false;
        Self::print_totp_prompt(&totp);

        // 5. 主循环。
        // §5.3 cid 寻址：relay 把 controller 的 offer/ice **加盖该 controller 的 cid** 后转发
        // 给 host；host 回的 answer/ice **必须带回同一 cid**，否则 relay 丢弃该帧（旧 cli 无
        // cid 字段 → answer 永远到不了 controller，即 P0「浏览器连不上无头 host」）。cli 是
        // 单会话 host：从入站 offer 取 cid，之后所有出站 answer/ice 原样回盖。cid 为 None 时
        // （如非 relay 的直连场景）按无 cid 发送，行为同旧版。
        let mut session_cid: Option<String> = None;
        let mut batch = BatchingBuffer::new();

        loop {
            // 攒批截止时刻 → flush 定时器。仅在 verified+subscribed（真会推流）时才按
            // batch.deadline() 取近期截止；否则给一个远期 sleep 占位——避免门控期 batch
            // 不被 drain、过期 deadline 让 sleep_until 立即 resolve 而忙等空转。
            let flush_sleep = match batch.deadline() {
                Some(dl) if verified && subscribed => tokio::time::sleep_until(dl.into()),
                _ => tokio::time::sleep(Duration::from_secs(3600)),
            };
            tokio::pin!(flush_sleep);

            tokio::select! {
                // PTY 输出 → 攒批。
                maybe_out = pty_out_rx.recv() => {
                    match maybe_out {
                        Some(bytes) => {
                            batch.push(&bytes);
                            // 仅在 verified+subscribed 后才 flush 到线上；否则继续累积。
                            if verified && subscribed && batch.should_flush() {
                                Self::flush_pane(&mut batch, &mut crypto, &dc_io.tx).await?;
                            }
                        }
                        None => {
                            if verified && subscribed {
                                Self::flush_pane(&mut batch, &mut crypto, &dc_io.tx).await.ok();
                            }
                            tracing::info!(target: "ridge_cli::session", "shell exited; ending session");
                            break;
                        }
                    }
                }

                _ = &mut flush_sleep => {
                    if verified && subscribed {
                        Self::flush_pane(&mut batch, &mut crypto, &dc_io.tx).await?;
                    }
                }

                // 来自 controller 的 E2EE 帧 → 解密 → demux → 分派。
                maybe_in = dc_io.rx.recv() => {
                    match maybe_in {
                        Some(frame) => {
                            let was_subscribed = subscribed;
                            if let Err(e) = Self::handle_inbound(
                                &frame, &mut crypto, &pty, &dc_io.tx, &roots,
                                &totp, &mut verified, &mut subscribed,
                            ).await {
                                tracing::warn!(target: "ridge_cli::session", error = %e, "inbound frame rejected");
                            }
                            // 刚验证并订阅 → 把暂存的初始 PTY 输出立即推出。
                            if verified && subscribed && (!was_subscribed || batch.should_flush()) {
                                Self::flush_pane(&mut batch, &mut crypto, &dc_io.tx).await.ok();
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
                            // §5.3：回盖入站 offer 携带的 cid，relay 据此定向投递给该 controller。
                            PeerOutbound::Answer(sdp) => SignalMsg::Answer {
                                sdp,
                                cid: session_cid.clone(),
                            },
                            PeerOutbound::Ice(candidate) => SignalMsg::Ice {
                                candidate,
                                cid: session_cid.clone(),
                            },
                        };
                        signaling.send(msg).await.ok();
                    }
                }

                // 来自 relay 的信令。
                maybe_relay = signal_rx.recv() => {
                    match maybe_relay {
                        Some(SignalMsg::Offer { sdp, cid }) => {
                            // relay 加盖的该 controller 的 cid：记下，供出站 answer/ice 回盖。
                            if cid.is_some() {
                                session_cid = cid;
                            }
                            inbound_tx.send(PeerInbound::Offer(sdp)).await.ok();
                        }
                        Some(SignalMsg::Ice { candidate, cid }) => {
                            // 防御：若 ice 先于 offer 到达（一般不会），也捕获 cid。
                            if session_cid.is_none() && cid.is_some() {
                                session_cid = cid;
                            }
                            inbound_tx.send(PeerInbound::Ice(candidate)).await.ok();
                        }
                        Some(SignalMsg::PeerLeave { .. }) => {
                            tracing::info!(target: "ridge_cli::session", "controller left; ending session");
                            break;
                        }
                        Some(SignalMsg::Error { code, message }) => {
                            tracing::warn!(target: "ridge_cli::session", %code, %message, "signaling error");
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

    /// flush 攒批缓冲：取走合并大包 → 0x10 PANE_RAW（带固定 paneId）→ seal → DataChannel。
    async fn flush_pane(
        batch: &mut BatchingBuffer,
        crypto: &mut CryptoSession,
        tx: &mpsc::Sender<Vec<u8>>,
    ) -> Result<()> {
        if let Some(merged) = batch.take() {
            let plaintext = match mux::encode_pane(CLI_PANE_ID, &merged) {
                Ok(f) => f,
                Err(e) => {
                    // CLI_PANE_ID 是常量、不会过长；防御性丢弃，不断连。
                    tracing::error!(target: "ridge_cli::session", error = %e, "pane frame encode failed");
                    return Ok(());
                }
            };
            let sealed = crypto.seal(&plaintext)?;
            tx.send(sealed)
                .await
                .map_err(|_| anyhow!("data channel send channel closed"))?;
        }
        Ok(())
    }

    /// 把本会话的 6 位 TOTP + otpauth URI 打到 stderr（引导用户在浏览器 controller 输入）。
    fn print_totp_prompt(totp: &RemoteTotp) {
        const CYAN: &str = "\x1b[36m";
        const YELLOW: &str = "\x1b[33m";
        const BOLD: &str = "\x1b[1m";
        const DIM: &str = "\x1b[2m";
        const RESET: &str = "\x1b[0m";

        let label = hostname_label();
        let code = totp.current_code();
        let uri = totp.otpauth_uri(&label);
        let period = RemoteTotp::period_secs();

        eprintln!();
        eprintln!("{CYAN}{BOLD}  ╔══════════════════════════════════════════════╗{RESET}");
        eprintln!("{CYAN}{BOLD}  ║       RIDGE · 云远控二次验证 (TOTP)            ║{RESET}");
        eprintln!("{CYAN}{BOLD}  ╚══════════════════════════════════════════════╝{RESET}");
        eprintln!();
        eprintln!("  {DIM}在浏览器控制端输入下面的 6 位验证码以解锁控制：{RESET}");
        eprintln!();
        eprintln!("        {YELLOW}{BOLD}▎ {code} ▎{RESET}");
        eprintln!();
        eprintln!("  {DIM}每 {period}s 刷新；验证前控制端无法操作本机 shell。{RESET}");
        eprintln!("  {DIM}otpauth: {uri}{RESET}");
        eprintln!();
    }

    /// 解密一帧 controller→host，按 mux 通道分派（统一远控 S3）。
    ///   - 0x12 CONTROL → TOTP 握手（门控前唯一放行的业务通道）。
    ///   - 0x11 JSON   → JSON-RPC：$/hello 协商、$/cancel、subscribe-pane、
    ///     use-global-workspace、write_to_pty、resize_pane、get_active_workspace_id、
    ///     search、get_directory_children。
    ///   - 0x10 PANE   → controller 一般不发；忽略。
    ///
    /// 门控（契约 §4）：`verified` 为 false 时业务帧被拒（见 gate_envelope），只处理
    /// `$/hello` 与 `totp-verify`。
    #[allow(clippy::too_many_arguments)]
    async fn handle_inbound(
        frame: &[u8],
        crypto: &mut CryptoSession,
        pty: &PtyBridge,
        tx: &mpsc::Sender<Vec<u8>>,
        roots: &[PathBuf],
        totp: &RemoteTotp,
        verified: &mut bool,
        subscribed: &mut bool,
    ) -> Result<()> {
        let plaintext = crypto.open(frame)?;

        match mux::demux(&plaintext) {
            Inbound::Control(body) => Self::handle_control(&body, crypto, tx, totp, verified).await,
            Inbound::Json(body) => {
                let env = rpc::parse_envelope(&body);
                match gate_envelope(env, *verified) {
                    Gate::Allow(env) => {
                        Self::handle_envelope(env, crypto, pty, tx, roots, subscribed).await
                    }
                    Gate::RejectWithError { id } => {
                        let err = RpcError::with_data(
                            JSON_RPC_INVALID_REQUEST,
                            "TOTP verification required",
                            serde_json::json!({ "kind": "totp-required" }),
                        );
                        Self::send_json(crypto, tx, &rpc::error_response(&id, &err)).await
                    }
                    Gate::DropSilently => {
                        tracing::warn!(target: "ridge_cli::session", "business notification dropped before TOTP verification");
                        Ok(())
                    }
                    Gate::Ignore => Ok(()),
                }
            }
            Inbound::Pane { pane_id, .. } => {
                tracing::warn!(target: "ridge_cli::session", pane_id = %pane_id, "unexpected inbound PANE_RAW; ignored");
                Ok(())
            }
            Inbound::Unknown(tag) => {
                tracing::debug!(target: "ridge_cli::session", tag, "unknown mux channel; ignored");
                Ok(())
            }
            Inbound::Empty => Ok(()),
        }
    }

    /// 处理一帧 0x12 CONTROL（契约 §4 TOTP 握手）。
    async fn handle_control(
        body: &[u8],
        crypto: &mut CryptoSession,
        tx: &mpsc::Sender<Vec<u8>>,
        totp: &RemoteTotp,
        verified: &mut bool,
    ) -> Result<()> {
        let ctrl: SessionControl = match serde_json::from_slice(body) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(target: "ridge_cli::session", error = %e, "bad CONTROL frame; ignored");
                return Ok(());
            }
        };
        match ctrl {
            SessionControl::TotpVerify { code } => {
                let ok = totp.verify(&code);
                if ok {
                    *verified = true;
                    tracing::info!(target: "ridge_cli::session", "controller passed TOTP; control channel unlocked");
                } else {
                    tracing::warn!(target: "ridge_cli::session", "controller submitted an invalid TOTP code");
                }
                Self::send_control(crypto, tx, &SessionControl::TotpResult { ok }).await
            }
            // host 不应收到 totp-result；忽略。
            SessionControl::TotpResult { .. } => Ok(()),
        }
    }

    /// 处理一帧已放行的 0x11 JSON-RPC 信封。
    async fn handle_envelope(
        env: Envelope,
        crypto: &mut CryptoSession,
        pty: &PtyBridge,
        tx: &mpsc::Sender<Vec<u8>>,
        roots: &[PathBuf],
        subscribed: &mut bool,
    ) -> Result<()> {
        match env {
            Envelope::Notification { method, params } => {
                Self::handle_notification(&method, &params, crypto, tx, subscribed).await
            }
            Envelope::Request { id, method, params } => {
                if method == HELLO_METHOD {
                    // 罕见：带 id 的 $/hello。回 $/hello 通知 + 对 id 回空 result（避免悬挂）。
                    Self::send_json(crypto, tx, &rpc::negotiate_hello(&params)).await?;
                    return Self::send_json(crypto, tx, &rpc::result_response(&id, Value::Null))
                        .await;
                }
                if method == CANCEL_METHOD {
                    return Ok(()); // 本 host 的命令都是即时完成的，无需中止。
                }
                Self::dispatch_request(&id, &method, &params, crypto, pty, tx, roots, subscribed)
                    .await
            }
            Envelope::Ignore => Ok(()),
        }
    }

    /// notification 路由（无 id，不回响应）。
    async fn handle_notification(
        method: &str,
        params: &Value,
        crypto: &mut CryptoSession,
        tx: &mpsc::Sender<Vec<u8>>,
        subscribed: &mut bool,
    ) -> Result<()> {
        match method {
            HELLO_METHOD => Self::send_json(crypto, tx, &rpc::negotiate_hello(params)).await,
            CANCEL_METHOD => Ok(()),
            "subscribe-pane" => {
                *subscribed = true;
                tracing::info!(target: "ridge_cli::session", "controller subscribed pane; streaming PTY");
                Ok(())
            }
            // controller 启动时发的全局工作区意图（桌面 SPA 行为）；cli 单 ws，no-op。
            "use-global-workspace" => Ok(()),
            other => {
                tracing::debug!(target: "ridge_cli::session", method = other, "unhandled notification; ignored");
                Ok(())
            }
        }
    }

    /// 派发一个带 id 的业务请求 → 回 JSON-RPC result/error。
    #[allow(clippy::too_many_arguments)]
    async fn dispatch_request(
        id: &Value,
        method: &str,
        params: &Value,
        crypto: &mut CryptoSession,
        pty: &PtyBridge,
        tx: &mpsc::Sender<Vec<u8>>,
        roots: &[PathBuf],
        subscribed: &mut bool,
    ) -> Result<()> {
        match rpc::route_method(method, params) {
            Method::WritePty { data } => {
                if let Err(e) = pty.write_input(data.as_bytes()) {
                    let err = RpcError::new(
                        rpc::JSON_RPC_INTERNAL_ERROR,
                        format!("pty write failed: {e}"),
                    );
                    return Self::send_json(crypto, tx, &rpc::error_response(id, &err)).await;
                }
                Self::send_json(crypto, tx, &rpc::result_response(id, Value::Null)).await
            }
            Method::ResizePane { cols, rows } => {
                if let Err(e) = pty.resize(cols, rows) {
                    let err = RpcError::new(
                        rpc::JSON_RPC_INTERNAL_ERROR,
                        format!("pty resize failed: {e}"),
                    );
                    return Self::send_json(crypto, tx, &rpc::error_response(id, &err)).await;
                }
                // resize 请求亦视为「pane 已活跃」信号——确保即便 controller 没显式
                // subscribe-pane（仅经 register_pane_delta_channel 走 invoke 路径）也开始推流。
                *subscribed = true;
                Self::send_json(crypto, tx, &rpc::result_response(id, Value::Null)).await
            }
            Method::GetActiveWorkspaceId => {
                Self::send_json(
                    crypto,
                    tx,
                    &rpc::result_response(id, Value::String(CLI_WORKSPACE_ID.to_string())),
                )
                .await
            }
            Method::Search {
                root,
                query,
                use_regex,
                case_sensitive,
            } => {
                let results = fs_reuse::search(roots, &root, &query, use_regex, case_sensitive);
                let value = serde_json::to_value(results).unwrap_or(Value::Null);
                Self::send_json(crypto, tx, &rpc::result_response(id, value)).await
            }
            Method::DirectoryChildren { path } => {
                match fs_reuse::list_dir(roots, std::path::Path::new(&path)) {
                    Ok(entries) => {
                        let value = serde_json::to_value(entries).unwrap_or(Value::Null);
                        Self::send_json(crypto, tx, &rpc::result_response(id, value)).await
                    }
                    Err(e) => {
                        // 不泄露内部路径细节（rust/security.md）。
                        let err = RpcError::new(
                            rpc::JSON_RPC_INTERNAL_ERROR,
                            format!("cannot list directory: {}", e.kind()),
                        );
                        Self::send_json(crypto, tx, &rpc::error_response(id, &err)).await
                    }
                }
            }
            Method::Unsupported(name) => {
                // cli 不服务的 IDE 命令：回 METHOD_NOT_FOUND（controller 已经因 $/hello
                // 灰掉这些面板，正常不会发；防御性回错而非悬挂）。
                let err = RpcError::new(
                    JSON_RPC_METHOD_NOT_FOUND,
                    format!("method '{name}' not supported by ridge-cli host"),
                );
                Self::send_json(crypto, tx, &rpc::error_response(id, &err)).await
            }
        }
    }

    /// seal 并发出一帧 0x11 JSON。
    async fn send_json(
        crypto: &mut CryptoSession,
        tx: &mpsc::Sender<Vec<u8>>,
        value: &Value,
    ) -> Result<()> {
        let plaintext = mux::encode_json(value);
        let sealed = crypto.seal(&plaintext)?;
        tx.send(sealed).await.ok();
        Ok(())
    }

    /// seal 并发出一帧 0x12 CONTROL。
    async fn send_control(
        crypto: &mut CryptoSession,
        tx: &mpsc::Sender<Vec<u8>>,
        ctrl: &SessionControl,
    ) -> Result<()> {
        let plaintext = mux::encode_control(ctrl);
        let sealed = crypto.seal(&plaintext)?;
        tx.send(sealed).await.ok();
        Ok(())
    }
}

/// 主机名（用于 otpauth label，仅展示用）。读 `HOSTNAME`/`COMPUTERNAME` 环境变量，
/// 缺失则回退 `ridge-host`。无额外依赖。
fn hostname_label() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "ridge-host".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(method: &str) -> Envelope {
        Envelope::Request {
            id: json!(1),
            method: method.into(),
            params: json!({}),
        }
    }
    fn notif(method: &str) -> Envelope {
        Envelope::Notification {
            method: method.into(),
            params: json!({}),
        }
    }

    #[test]
    fn hello_passes_gate_before_verification() {
        assert!(matches!(
            gate_envelope(notif(HELLO_METHOD), false),
            Gate::Allow(_)
        ));
        assert!(matches!(
            gate_envelope(req(HELLO_METHOD), false),
            Gate::Allow(_)
        ));
    }

    #[test]
    fn business_request_rejected_before_verification() {
        match gate_envelope(req("write_to_pty"), false) {
            Gate::RejectWithError { id } => assert_eq!(id, json!(1)),
            other => panic!("expected RejectWithError, got {other:?}"),
        }
    }

    #[test]
    fn business_notification_dropped_before_verification() {
        assert_eq!(
            gate_envelope(notif("subscribe-pane"), false),
            Gate::DropSilently
        );
    }

    #[test]
    fn everything_passes_gate_after_verification() {
        assert!(matches!(
            gate_envelope(req("write_to_pty"), true),
            Gate::Allow(_)
        ));
        assert!(matches!(
            gate_envelope(notif("subscribe-pane"), true),
            Gate::Allow(_)
        ));
        assert!(matches!(gate_envelope(req("search"), true), Gate::Allow(_)));
    }

    #[test]
    fn ignore_envelope_stays_ignored() {
        assert_eq!(gate_envelope(Envelope::Ignore, true), Gate::Ignore);
        assert_eq!(gate_envelope(Envelope::Ignore, false), Gate::Ignore);
    }

    #[test]
    fn pane_and_workspace_ids_are_stable_constants() {
        // 这些值是 controller 端订阅/解析的契约锚点；锁死避免漂移。
        assert_eq!(CLI_PANE_ID, "ridge-cli-pane");
        assert_eq!(CLI_WORKSPACE_ID, "ridge-cli-ws");
    }

    // ── 端到端集成测试：完整 inbound 路径（demux + JSON-RPC + TOTP 门控）──────────
    //
    // 用一对真实 CryptoSession（host/controller）+ 真实 PTY 驱动 `handle_inbound`，
    // 模拟浏览器 controller 发来的 E2EE 帧，断言 host 回的帧的 mux 通道 + JSON 形状
    // 与契约 / 桌面 host 一致。

    use crate::e2ee::{Dir, Handshake, Session as CryptoSession};
    use crate::mux::{demux, encode_control, encode_json, Inbound};
    use crate::protocol::SessionControl;
    use crate::pty::PtyBridge;

    /// 建立一对 host/controller 加密会话（host=dir0，controller=dir1）。
    fn crypto_pair() -> (CryptoSession, CryptoSession) {
        let host_hs = Handshake::new();
        let ctrl_hs = Handshake::new();
        let host_pub = host_hs.public_bytes();
        let ctrl_pub = ctrl_hs.public_bytes();
        let host = host_hs
            .into_session(ctrl_pub, Dir::HostToController)
            .unwrap();
        let ctrl = ctrl_hs
            .into_session(host_pub, Dir::ControllerToHost)
            .unwrap();
        (host, ctrl)
    }

    /// drain 一个 mpsc::Receiver 里当前可取的全部帧（host 经 tx 发出的密文）。
    fn drain(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(f) = rx.try_recv() {
            out.push(f);
        }
        out
    }

    /// 当前 TOTP code（测试内复刻：直接用 totp.current_code，与 controller 读 TUI 一致）。
    #[tokio::test]
    async fn full_inbound_path_gates_then_serves_terminal_and_fs() {
        let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
        // PTY 不是本测试的被测对象（协议路由才是）。若该环境无可用 shell（精简 CI），
        // 优雅跳过而非误报失败；路由/门控已由本模块的纯单测全覆盖。
        let (pty, _pty_out_rx) = match PtyBridge::spawn(None, None) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("skipping full_inbound_path test: no usable shell to spawn PTY ({e})");
                return;
            }
        };
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
        let totp = RemoteTotp::new();
        let mut verified = false;
        let mut subscribed = false;

        // serving root = 一个临时目录（让 search/tree 能命中且受沙箱约束）。
        let dir = std::env::temp_dir().join(format!("ridge-cli-session-it-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hit.txt"), "needle here\n").unwrap();
        let roots = vec![dir.clone()];

        // controller 用 dir1 seal 一帧明文（已 mux 编码）。
        let seal =
            |ctrl: &mut CryptoSession, plaintext: Vec<u8>| ctrl.seal(&plaintext).unwrap();

        // 1) $/hello（门控前放行）：host 应回一帧 0x11 $/hello，能力为 cli 子集。
        let hello = encode_json(&json!({
            "jsonrpc": "2.0", "method": "$/hello",
            "params": { "protocolVersion": 1, "capabilities": ["pane","invoke","fs","git","search","workspace","theme"] }
        }));
        let frame = seal(&mut ctrl_crypto, hello);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        assert_eq!(out.len(), 1, "hello should produce exactly one reply");
        let reply: Value = match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Json(b) => serde_json::from_slice(&b).unwrap(),
            other => panic!("expected Json hello reply, got {other:?}"),
        };
        assert_eq!(reply["method"], "$/hello");
        let caps: Vec<String> =
            serde_json::from_value(reply["params"]["capabilities"].clone()).unwrap();
        assert_eq!(caps, vec!["pane", "fs", "search"]);

        // 2) 未验证就发 write_to_pty（带 id）→ host 回 JSON-RPC error（totp-required）。
        let write = encode_json(&json!({
            "jsonrpc": "2.0", "id": 10, "method": "write_to_pty",
            "params": { "paneId": CLI_PANE_ID, "data": "echo hi\n" }
        }));
        let frame = seal(&mut ctrl_crypto, write);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        assert_eq!(out.len(), 1);
        let err: Value = match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Json(b) => serde_json::from_slice(&b).unwrap(),
            other => panic!("expected Json error, got {other:?}"),
        };
        assert_eq!(err["id"], 10);
        assert_eq!(err["error"]["data"]["kind"], "totp-required");
        assert!(!verified, "still unverified after rejected business frame");

        // 3) TOTP 验证（0x12 CONTROL）→ host 回 0x12 totp-result ok，置 verified。
        let totp_frame = encode_control(&SessionControl::TotpVerify {
            code: totp.current_code(),
        });
        let frame = seal(&mut ctrl_crypto, totp_frame);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        assert_eq!(out.len(), 1);
        match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Control(b) => {
                let sc: SessionControl = serde_json::from_slice(&b).unwrap();
                assert_eq!(sc, SessionControl::TotpResult { ok: true });
            }
            other => panic!("expected Control totp-result, got {other:?}"),
        }
        assert!(verified, "verified after correct TOTP");

        // 4) subscribe-pane（notification）→ 置 subscribed，无回帧。
        let sub = encode_json(
            &json!({ "jsonrpc": "2.0", "method": "subscribe-pane", "params": { "paneId": CLI_PANE_ID } }),
        );
        let frame = seal(&mut ctrl_crypto, sub);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        assert!(subscribed);
        assert!(
            drain(&mut rx).is_empty(),
            "subscribe-pane is a notification, no reply"
        );

        // 5) write_to_pty（已验证）→ result null。
        let write2 = encode_json(&json!({
            "jsonrpc": "2.0", "id": 11, "method": "write_to_pty",
            "params": { "paneId": CLI_PANE_ID, "data": "true\n" }
        }));
        let frame = seal(&mut ctrl_crypto, write2);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        let ok: Value = match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Json(b) => serde_json::from_slice(&b).unwrap(),
            other => panic!("expected Json result, got {other:?}"),
        };
        assert_eq!(ok["id"], 11);
        assert!(ok["result"].is_null());

        // 6) get_active_workspace_id → 固定 ws id。
        let gw = encode_json(
            &json!({ "jsonrpc": "2.0", "id": 12, "method": "get_active_workspace_id" }),
        );
        let frame = seal(&mut ctrl_crypto, gw);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        let ws: Value = match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Json(b) => serde_json::from_slice(&b).unwrap(),
            other => panic!("expected Json result, got {other:?}"),
        };
        assert_eq!(ws["result"], CLI_WORKSPACE_ID);

        // 7) search → 命中 hit.txt（result 是 SearchResult 数组）。
        let search = encode_json(&json!({
            "jsonrpc": "2.0", "id": 13, "method": "search",
            "params": { "root": dir.to_string_lossy(), "query": "needle", "useRegex": false, "caseSensitive": false }
        }));
        let frame = seal(&mut ctrl_crypto, search);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        let sr: Value = match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Json(b) => serde_json::from_slice(&b).unwrap(),
            other => panic!("expected Json result, got {other:?}"),
        };
        let hits = sr["result"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "search should find the needle once");
        assert!(hits[0]["content"].as_str().unwrap().contains("needle"));

        // 8) 不支持的方法 → METHOD_NOT_FOUND（防御性）。
        let git = encode_json(
            &json!({ "jsonrpc": "2.0", "id": 14, "method": "git_status", "params": {} }),
        );
        let frame = seal(&mut ctrl_crypto, git);
        RemoteSession::handle_inbound(
            &frame,
            &mut host_crypto,
            &pty,
            &tx,
            &roots,
            &totp,
            &mut verified,
            &mut subscribed,
        )
        .await
        .unwrap();
        let out = drain(&mut rx);
        let nf: Value = match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
            Inbound::Json(b) => serde_json::from_slice(&b).unwrap(),
            other => panic!("expected Json error, got {other:?}"),
        };
        assert_eq!(nf["error"]["code"], rpc::JSON_RPC_METHOD_NOT_FOUND);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
