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
use serde::Serialize;

use crate::config::{self, AuthFile};
use crate::ice;
use crate::rtc::WebRtcHost;
use crate::session::{HostIdentity, RemoteSession};
use crate::signaling::{Role, SignalMsg, Signaling};
use ridge_core::DeviceIdentity;

/// 重连退避上下限。
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MIN_BACKOFF: Duration = Duration::from_secs(2);

const CLOUD_STATUS_FILE_ENV: &str = "RIDGE_REMOTE_CLOUD_STATUS_FILE";

#[derive(Debug)]
struct FatalSignalError {
    code: String,
    detail: String,
}

impl std::fmt::Display for FatalSignalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for FatalSignalError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CloudDaemonStatus<'a> {
    schema: u32,
    pid: u32,
    state: &'a str,
    detail: &'a str,
    updated_at: u64,
}

struct StatusGuard;

impl Drop for StatusGuard {
    fn drop(&mut self) {
        let Some(path) = status_path() else { return };
        if let Ok(raw) = std::fs::read(&path) {
            if let Ok(status) = serde_json::from_slice::<serde_json::Value>(&raw) {
                if status.get("state").and_then(|value| value.as_str()) == Some("error") {
                    return;
                }
            }
        }
        write_status(&path, "stopped", "公网 Remote sidecar 已退出");
    }
}

fn status_path() -> Option<std::path::PathBuf> {
    std::env::var_os(CLOUD_STATUS_FILE_ENV).map(std::path::PathBuf::from)
}

fn publish_status(state: &str, detail: &str) {
    let Some(path) = status_path() else { return };
    write_status(&path, state, detail);
}

fn write_status(path: &std::path::Path, state: &str, detail: &str) {
    let body = CloudDaemonStatus {
        schema: 1,
        pid: std::process::id(),
        state,
        detail,
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default(),
    };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension("json.tmp");
    let Ok(bytes) = serde_json::to_vec(&body) else {
        return;
    };
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = replace_status_snapshot(&tmp, path);
    }
}

fn replace_status_snapshot(tmp: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(first_error) => {
            if !path.exists() {
                return Err(first_error);
            }
            // Windows rename does not replace an existing destination. Move
            // the last complete snapshot aside, commit the new one, then drop
            // the backup; restore it if the commit itself fails.
            let backup = path.with_extension("json.bak");
            if backup.exists() {
                std::fs::remove_file(&backup)?;
            }
            std::fs::rename(path, &backup)?;
            match std::fs::rename(tmp, path) {
                Ok(()) => {
                    let _ = std::fs::remove_file(backup);
                    Ok(())
                }
                Err(error) => {
                    let _ = std::fs::rename(backup, path);
                    Err(error)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_status_replaces_previous_snapshot() {
        let dir = std::env::temp_dir().join(format!(
            "ridge-cloud-status-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let path = dir.join("status.json");

        write_status(&path, "starting", "booting");
        write_status(&path, "online", "ready");

        let status: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("status snapshot exists"))
                .expect("status snapshot is valid JSON");
        assert_eq!(status["state"], "online");
        assert_eq!(status["detail"], "ready");

        let _ = std::fs::remove_dir_all(dir);
    }
}

/// relay 的**终态**错误码（重连必然重蹈覆辙：凭据/账号/权限问题，须人工处理）。
/// 收到即打印可读指引并终止 daemon，而不是无声退避重连——旧行为下 host 看似
/// "在线"（进程还在）、controller 端却永远停在「正在连接远程桌面」，无从诊断
/// （iter-61 用户实测 rdg 公网 remote 完全接不上的可疑面之一）。
const FATAL_SIGNAL_CODES: &[&str] = &[
    "USERNAME_MISMATCH",
    "DEVICE_TOKEN_MISMATCH",
    "DEVICE_NOT_OWNED",
    "DEVICE_PARKED",
];

/// 终态错误码 → 人话 + 下一步动作。
fn fatal_hint(code: &str) -> &'static str {
    match code {
        "USERNAME_MISMATCH" => {
            "设备令牌所属账号与租户域名不符：请用该设备所属账号重新 `rdg login`。"
        }
        "DEVICE_TOKEN_MISMATCH" => "设备令牌与当前设备不匹配：请重新激活本机 `rdg login`。",
        "DEVICE_NOT_OWNED" => {
            "该设备不属于当前账号：请在云端控制台确认设备归属，或重新 `rdg login`。"
        }
        "DEVICE_PARKED" => "该设备已被停用：请在云端控制台恢复后重试。",
        _ => "请检查云端账号/设备状态后重试。",
    }
}

/// 跑 daemon。`shell` / `cwd` 透传给每个会话的 PTY；`root` 透传为 fs 服务根沙箱
/// （D-GM-9，缺省回退 `cwd` → 进程当前目录）。
pub async fn run(shell: Option<String>, cwd: Option<String>, root: Option<String>) -> Result<()> {
    let _status_guard = StatusGuard;
    publish_status("starting", "正在读取设备凭据");
    let auth = match config::load_auth()
        .context("failed to load credentials")?
        .context("本机尚未激活云端设备：先跑 `rdg login`（或 `rdg login --browser`）绑定本机，再启动公网远控")
    {
        Ok(auth) => auth,
        Err(error) => {
            publish_status("error", &format!("{error:#}"));
            return Err(error);
        }
    };

    // WebRTC is a detached shell adapter over the long-lived kernel.  Ensure
    // the kernel exists before a controller can create its first PTY; this
    // prevents a startup race from falling back to a daemon-local PTY.
    if let Err(error) = crate::kernel_ctl::ensure_kernel_running() {
        tracing::warn!(target: "ridge_cli::daemon", %error, "kernel bootstrap unavailable; session may retry");
    }

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
        publish_status("connecting", "正在连接信令中继");
        match serve_once(
            &http,
            &auth,
            &device_identity,
            shell.clone(),
            cwd.clone(),
            root.clone(),
        )
        .await
        {
            Ok(()) => {
                // 信令正常断开：重置退避后立即重连。
                backoff = MIN_BACKOFF;
            }
            Err(e) => {
                if e.downcast_ref::<FatalSignalError>().is_some() {
                    return Err(e);
                }
                publish_status(
                    "connecting",
                    &format!("连接失败，{} 秒后重试：{e:#}", backoff.as_secs()),
                );
                tracing::warn!(
                    target: "ridge_cli::daemon",
                    error = %e,
                    backoff_secs = backoff.as_secs(),
                    "session loop error; reconnecting"
                );
                // 可见性（iter-62）：连不上中继时旧实现只 warn 进日志再静默退避，前台
                // `rdg remote` 一片空白，用户只能从控制端的「远程主机当前不在线」反推。
                // 把失败原因与下次重试间隔直接打到 stderr（TUI 模式下 stderr 已重定向到
                // 日志文件，不会糊屏）。
                eprintln!(
                    "! 连接信令中继失败（{}s 后重试）：{e:#}\n  入口：{}",
                    backoff.as_secs(),
                    auth.public_entry()
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

    tracing::info!(target: "ridge_cli::daemon", "signaling socket connected; waiting for relay welcome");
    // 可见性（iter-61）：daemon 是长驻前台进程，用户只有 stdout 可看。日志默认进
    // 文件/stderr，「连上了没有」全靠猜——这里把关键节点直接打到 stdout。
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
            SignalMsg::Welcome { peer_present, .. } => {
                publish_status("online", "已连接信令中继，等待控制端接入");
                println!(
                    "✓ 已连接信令中继：{}（等待控制端接入…）",
                    auth.public_entry()
                );
                peer_present
            }
            SignalMsg::PeerJoin { ref role, .. } => *role == Role::Controller,
            SignalMsg::Error { code, message } => {
                tracing::warn!(target: "ridge_cli::daemon", %code, %message, "signaling error");
                if FATAL_SIGNAL_CODES.contains(&code.as_str()) {
                    // 终态：重连必然重蹈覆辙。打印可读指引并让 daemon 退出（非零码），
                    // 而非静默热重连——否则 controller 侧只能看到永远的「正在连接」。
                    eprintln!("✗ 云端拒绝本设备接入（{code}）：{message}");
                    eprintln!("  {}", fatal_hint(&code));
                    let detail = fatal_hint(&code).to_string();
                    publish_status("error", &format!("{code}: {detail}"));
                    return Err(FatalSignalError { code, detail }.into());
                }
                eprintln!("! 信令错误（{code}）：{message}");
                continue;
            }
            _ => false,
        };

        if controller_present {
            tracing::info!(target: "ridge_cli::daemon", "controller present; starting session");
            println!("→ 控制端已接入，正在建立加密会话…");
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
