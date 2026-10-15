//! Independent LAN Remote host process.
//!
//! The host owns only transport adapters.  PTYs, workspace identity and
//! scrollback live in `ridge-kernel`, so killing the desktop Tauri process does
//! not tear down the remote service or its sessions.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config;
use crate::kernel_ctl;

const REGISTRY_ENV: &str = "RIDGE_REMOTE_HOST_REGISTRY";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostRegistry {
    pub schema: u32,
    pub pid: u32,
    pub port: u16,
    pub lan_ip: String,
    pub tls: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub started_at: u64,
}

pub async fn run(requested_port:u16)->Result<()> {
    let endpoint=tokio::task::spawn_blocking(kernel_ctl::ensure_kernel_running).await.context("kernel bootstrap task failed")?.map_err(anyhow::Error::msg)?;
    let runtime=ridge_kernel::auth_client::remote_auth(endpoint.clone());
    runtime.call("remote_set_totp_identity",serde_json::json!({"username":config::totp_identity()})).map_err(anyhow::Error::msg)?;
    let port=if requested_port==0 {config::lan_port()} else {requested_port};
    runtime.call("set_remote_enabled",serde_json::json!({"enabled":true,"port":port})).map_err(anyhow::Error::msg)?;
    let info=runtime.call("get_remote_info",serde_json::json!({})).map_err(anyhow::Error::msg)?;
    let registry_path=registry_path()?;
    let registry=HostRegistry {schema:1,pid:endpoint.pid,port:info["port"].as_u64().ok_or_else(||anyhow::anyhow!("missing LAN port"))? as u16,
        lan_ip:info["lanIp"].as_str().unwrap_or_default().into(),tls:info["tls"]==true,enabled:info["remoteEnabled"]==true,started_at:endpoint.started_at_unix};
    write_registry(&registry_path,&registry)?;
    eprintln!("ridge host ready: {}://{}:{} (kernel pid={}, tls={})",if registry.tls {"https"} else {"http"},registry.lan_ip,registry.port,endpoint.pid,registry.tls);
    if std::env::var("RIDGE_PRINT_TOTP").ok().as_deref()==Some("1")||std::env::args().any(|arg|arg=="--print-totp") {
        eprintln!("TOTP: {}",runtime.current_code());
    }
    tokio::signal::ctrl_c().await?;
    runtime.call("set_remote_enabled",serde_json::json!({"enabled":false})).map_err(anyhow::Error::msg)?;
    clear_registry(&registry_path,registry.pid);
    Ok(())
}

fn registry_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os(REGISTRY_ENV) {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create host registry directory {}", parent.display()))?;
        }
        return Ok(path);
    }
    Ok(config::auth_path()?.with_file_name("remote-host.json"))
}

fn write_registry(path: &Path, value: &HostRegistry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn clear_registry(path: &Path, pid: u32) {
    let Ok(raw) = fs::read(path) else {
        return;
    };
    let Ok(current) = serde_json::from_slice::<HostRegistry>(&raw) else {
        return;
    };
    if current.pid == pid {
        let _ = fs::remove_file(path);
    }
}

fn default_enabled() -> bool {
    true
}
