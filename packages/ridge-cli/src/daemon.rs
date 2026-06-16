//! daemon 主循环（`ridge-cli remote --daemon`）。
//!
//! 流程：
//! 1. 读 `~/.config/ridge/auth.json`（未配对则提示先 `--enable`）。
//! 2. 拉取 ICE servers（契约 §5.2）。
//! 3. 连信令 WS（`role=host`，§5）。
//! 4. 收到 `welcome(peerPresent:true)` 或 `peer-join(controller)` → 起一个会话
//!    （controller 是 offerer，host 是 answerer）。
//! 5. 会话结束后回到等待；信令断开则按指数退避重连。

use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::{self, AuthFile};
use crate::ice;
use crate::rtc::WebRtcHost;
use crate::session::{HostIdentity, RemoteSession};
use crate::signaling::{Role, SignalMsg, Signaling};
use ridge_core::DeviceIdentity;

/// 重连退避上下限。
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MIN_BACKOFF: Duration = Duration::from_secs(2);

/// 跑 daemon。`shell` / `cwd` 透传给每个会话的 PTY；`root` 透传为 fs 服务根沙箱
/// （D-GM-9，缺省回退 `cwd` → 进程当前目录）。
pub async fn run(shell: Option<String>, cwd: Option<String>, root: Option<String>) -> Result<()> {
    let auth = config::load_auth()
        .context("failed to load credentials")?
        .context("no device credentials — run `ridge-cli remote --enable` first")?;

    tracing::info!(
        target: "ridge_cli::daemon",
        device = %auth.device_name,
        username = %auth.username,
        entry = %auth.public_entry(),
        "starting ridge-cli daemon"
    );

    // 零信任 #2：进程级初始化 Ed25519 设备身份（生成/加载 device_identity.key，
    // DPAPI/0600，与 auth.json 同根）。指纹打到日志，供 TOFU 首次信任时用户带外核对。
    // P2 握手将用它签名本次临时 X25519 公钥（本任务仅做密钥基建，不接握手帧）。
    let device_identity = ridge_core::DeviceIdentity::load_or_create();
    tracing::info!(
        target: "ridge_cli::daemon",
        fingerprint = %device_identity.fingerprint(),
        "device identity ready (Ed25519, zero-trust #2)"
    );

    let http = reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")?;

    let mut backoff = MIN_BACKOFF;
    loop {
        match serve_once(&http, &auth, &device_identity, shell.clone(), cwd.clone(), root.clone())
            .await
        {
            Ok(()) => {
                // 信令正常断开：重置退避后立即重连。
                backoff = MIN_BACKOFF;
            }
            Err(e) => {
                tracing::warn!(
                    target: "ridge_cli::daemon",
                    error = %e,
                    backoff_secs = backoff.as_secs(),
                    "session loop error; reconnecting"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// 连一次信令，处理其上的所有会话直到信令断开。
async fn serve_once(
    http: &reqwest::Client,
    auth: &AuthFile,
    device_identity: &DeviceIdentity,
    shell: Option<String>,
    cwd: Option<String>,
    root: Option<String>,
) -> Result<()> {
    let ice_servers = ice::fetch_ice_servers(http, &auth.token).await;
    let mut signaling = Signaling::connect(&auth.signaling_ws_url())
        .await
        .context("signaling connect failed")?;
    let sender = signaling.sender();
    let peer = WebRtcHost;

    tracing::info!(target: "ridge_cli::daemon", "signaling connected; waiting for controller");

    loop {
        let ev = match signaling.incoming.recv().await {
            Some(ev) => ev,
            None => {
                tracing::info!(target: "ridge_cli::daemon", "signaling closed");
                return Ok(());
            }
        };

        // controller 在场 → 起会话（host 作 answerer）。
        // `..` 忽略共享 schema 新增的 cid 字段：此处只判定「是否有 controller」，cid 的
        // 捕获/回盖在 RemoteSession 内进行（见 session.rs，从入站 offer 取 cid）。
        let controller_present = match ev {
            SignalMsg::Welcome { peer_present, .. } => peer_present,
            SignalMsg::PeerJoin { ref role, .. } => *role == Role::Controller,
            SignalMsg::Error { code, message } => {
                tracing::warn!(target: "ridge_cli::daemon", %code, %message, "signaling error");
                continue;
            }
            _ => false,
        };

        if controller_present {
            tracing::info!(target: "ridge_cli::daemon", "controller present; starting session");
            // 会话借用 incoming 读 offer/ICE，并用 cheap-clone 的 sender 回 answer/ICE。
            // 零信任 #2：注入设备身份签名材料（host 握手发 0x02）。
            if let Err(e) = RemoteSession::run(
                &peer,
                ice_servers.clone(),
                &sender,
                &mut signaling.incoming,
                shell.clone(),
                cwd.clone(),
                root.clone(),
                HostIdentity {
                    device_identity,
                    device_name: &auth.device_name,
                    username: &auth.username,
                },
            )
            .await
            {
                tracing::warn!(target: "ridge_cli::daemon", error = %e, "session ended with error");
            }
            tracing::info!(target: "ridge_cli::daemon", "session ended; waiting for next controller");
        }
    }
}
