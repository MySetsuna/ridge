//! Independent LAN Remote host process.
//!
//! The host owns only transport adapters.  PTYs, workspace identity and
//! scrollback live in `ridge-kernel`, so killing the desktop Tauri process does
//! not tear down the remote service or its sessions.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use ridge_remote::auth::{RemoteAuth, SessionStore};
use ridge_remote::host::RemoteHost;
use ridge_remote::serve::UaServeConfig;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::config;
use crate::kernel_ctl;
use crate::kernel_host_impl::KernelHost;

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

pub async fn run(requested_port: u16) -> Result<()> {
    let endpoint = tokio::task::spawn_blocking(kernel_ctl::ensure_kernel_running)
        .await
        .context("kernel bootstrap task failed")?
        .map_err(anyhow::Error::msg)?;
    let port = if requested_port == 0 {
        config::lan_port()
    } else {
        requested_port
    };
    let lan_ip = config::detect_lan_ip();
    let machine_name = host_name();
    let serve_cfg = UaServeConfig::resolve_ui_dirs();
    let (listener, actual_port) = ridge_remote::server::bind_tcp(port)?;
    let tls_config = ridge_remote::server::resolve_tls(&lan_ip, &machine_name).await;
    let tls_enabled = tls_config.is_some();
    let registry_path = registry_path()?;
    let enabled_file = enabled_path(&registry_path);
    let registry = HostRegistry {
        schema: 1,
        pid: std::process::id(),
        port: actual_port,
        lan_ip: lan_ip.clone(),
        tls: tls_enabled,
        enabled: read_enabled_flag(&enabled_file),
        started_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default(),
    };
    write_registry(&registry_path, &registry)?;

    let totp = Arc::new(RemoteAuth::new());
    totp.switch_identity(Some(&config::totp_identity()));
    let remote_enabled = Arc::new(AtomicBool::new(registry.enabled));
    let enabled_state = remote_enabled.clone();
    tokio::spawn(async move {
        loop {
            enabled_state.store(read_enabled_flag(&enabled_file), Ordering::Relaxed);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    });
    let host: Arc<dyn RemoteHost> = Arc::new(KernelHost {
        endpoint,
        totp: totp.clone(),
        sessions: SessionStore::new(),
        port: actual_port,
        lan_ip: lan_ip.clone(),
        machine_name: machine_name.clone(),
        serve_cfg,
        tls_enabled,
        remote_enabled,
    });
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let mdns = ridge_remote::mdns::spawn_mdns_broadcast(actual_port);

    eprintln!(
        "ridge host ready: {}://{}:{} (kernel pid={}, tls={})",
        if tls_enabled { "https" } else { "http" },
        lan_ip,
        actual_port,
        host_kernel_pid(&host),
        tls_enabled
    );
    eprintln!("TOTP: {}", totp.current_code());

    let server = ridge_remote::server_app::run(host, listener, tls_config, shutdown_rx, true);
    tokio::pin!(server);
    let result = tokio::select! {
        result = &mut server => result.map(|_| ()),
        signal = tokio::signal::ctrl_c() => {
            if signal.is_ok() {
                let _ = shutdown_tx.send(());
            }
            server.await.map(|_| ())
        }
    };
    mdns.1.store(true, Ordering::Relaxed);
    let _ = mdns.0.join();
    clear_registry(&registry_path, registry.pid);
    result
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
    let Ok(raw) = fs::read(path) else { return; };
    let Ok(current) = serde_json::from_slice::<HostRegistry>(&raw) else { return; };
    if current.pid == pid {
        let _ = fs::remove_file(path);
    }
}

fn enabled_path(registry: &Path) -> PathBuf {
    registry.with_file_name("remote-host.enabled")
}

fn read_enabled_flag(path: &Path) -> bool {
    match fs::read_to_string(path) {
        Ok(value) => value.trim() != "0",
        Err(_) => true,
    }
}

fn default_enabled() -> bool {
    true
}

fn host_kernel_pid(host: &Arc<dyn RemoteHost>) -> u32 {
    // The endpoint is intentionally not exposed through RemoteHost.  This is
    // only a diagnostic value; the kernel registry remains the source of truth.
    let _ = host;
    kernel_ctl::read_kernel_pid().unwrap_or_default()
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}
