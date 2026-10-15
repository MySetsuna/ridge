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

use crate::config;

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
/// （iter-61 用户实测 ridge 公网 remote 完全接不上的可疑面之一）。
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
            "设备令牌所属账号与租户域名不符：请用该设备所属账号重新 `ridge login`。"
        }
        "DEVICE_TOKEN_MISMATCH" => "设备令牌与当前设备不匹配：请重新激活本机 `ridge login`。",
        "DEVICE_NOT_OWNED" => {
            "该设备不属于当前账号：请在云端控制台确认设备归属，或重新 `ridge login`。"
        }
        "DEVICE_PARKED" => "该设备已被停用：请在云端控制台恢复后重试。",
        _ => "请检查云端账号/设备状态后重试。",
    }
}

/// 向 Runtime 请求公网服务。`shell` / `cwd` 透传给 PTY；`root` 为 fs 服务根沙箱
/// （D-GM-9，缺省回退 `cwd` → 进程当前目录）。
pub(crate) fn start(endpoint:&ridge_kernel::registry::KernelEndpoint,shell:Option<String>,cwd:Option<String>,root:Option<String>)->Result<()> {
    let auth=config::load_auth()?.context("本机尚未激活云端设备：先运行 ridge login")?;
    if shell.is_some()||cwd.is_some() {
        let workspace=ridge_kernel::client::request_json(&endpoint,"POST","/v1/domain/workspaces",Some(&serde_json::json!({"name":"CLI remote"}))).map_err(anyhow::Error::msg)?;
        let workspace=uuid::Uuid::parse_str(workspace["workspace_id"].as_str().context("missing workspace identity")?)?;
        let pane=uuid::Uuid::new_v4();
        let topology=serde_json::json!({"pane_tree":{"root":{"Leaf":pane},"panes":{pane.to_string():{"id":pane,"mode":"Terminal","cwd":cwd}}}});
        let updated=ridge_kernel::client::request_json(&endpoint,"PUT",&format!("/v1/domain/workspaces/{workspace}/topology"),Some(&topology)).map_err(anyhow::Error::msg)?;
        if updated["ok"]!=true {anyhow::bail!("CLI remote workspace could not be prepared");}
        ridge_kernel::client::create_domain_pty(&endpoint,pane,shell.as_deref(),cwd.as_deref(),Some(workspace),"shell",None).map_err(anyhow::Error::msg)?;
    }
    let runtime=ridge_kernel::auth_client::remote_auth(endpoint.clone());
    let root=root.or(cwd).or_else(||std::env::current_dir().ok().map(|path|path.to_string_lossy().into_owned()));
    runtime.call("cloud_host_online",serde_json::json!({"deviceToken":auth.token,"deviceName":auth.device_name,"username":auth.username,"baseDomain":config::base_domain(),"root":root,"plaintext":std::env::var("RIDGE_CLOUD_DEV_PLAINTEXT").ok().as_deref()==Some("1")})).map_err(anyhow::Error::msg)?;
    Ok(())
}

pub async fn run(shell:Option<String>,cwd:Option<String>,root:Option<String>)->Result<()> {
    let endpoint=crate::kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
    start(&endpoint,shell,cwd,root)?;
    let runtime=ridge_kernel::auth_client::remote_auth(endpoint);
    loop {
        let state=runtime.call("get_cloud_host_state",serde_json::json!({})).map_err(anyhow::Error::msg)?;
        publish_status(state["state"].as_str().unwrap_or("error"),state["error"].as_str().unwrap_or(""));
        tokio::select! {
            _=tokio::signal::ctrl_c()=>{runtime.call("cloud_host_offline",serde_json::json!({})).map_err(anyhow::Error::msg)?;return Ok(());},
            _=tokio::time::sleep(Duration::from_secs(2))=>{},
        }
    }
}
