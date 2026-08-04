//! Process supervisor for the transport-only Remote/WebRTC sidecars.
//!
//! Tauri is a UI shell.  This module starts `rdg host` and `rdg remote
//! --daemon` as detached processes whose state is rooted in ridge-kernel and
//! whose lifetime is not tied to the WebView process.

use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LanRegistry {
    schema: u32,
    pid: u32,
    port: u16,
    lan_ip: String,
    tls: bool,
    #[serde(default = "default_enabled")]
    enabled: bool,
    started_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CloudRegistry {
    schema: u32,
    pid: u32,
    started_at: u64,
}

#[derive(Debug, Clone)]
pub struct LanHostStatus {
    pub pid: u32,
    pub port: u16,
    pub lan_ip: String,
    pub tls: bool,
    pub enabled: bool,
}

const LAN_REGISTRY: &str = "remote-host.json";
const CLOUD_AUTH: &str = "remote-cloud-auth.json";
const CLOUD_REGISTRY: &str = "remote-cloud.json";
const CLOUD_ENABLED: &str = "remote-cloud.enabled";

pub fn lan_registry_path(app: &AppHandle) -> Result<PathBuf, String> {
    app_data_dir(app).map(|dir| dir.join(LAN_REGISTRY))
}

pub fn cloud_auth_path(app: &AppHandle) -> Result<PathBuf, String> {
    app_data_dir(app).map(|dir| dir.join(CLOUD_AUTH))
}

pub fn ensure_lan_host(app: &AppHandle) -> Result<LanHostStatus, String> {
    let registry_path = lan_registry_path(app)?;
    if let Some(status) = read_live_lan(&registry_path) {
        return Ok(status);
    }
    let _ = fs::remove_file(&registry_path);
    let binary = locate_rdg_binary(app)?;
    let registry = registry_path
        .to_str()
        .ok_or_else(|| "remote host registry path is not valid UTF-8".to_string())?;
    let enabled_path = enabled_path(&registry_path);
    let enabled = enabled_path
        .to_str()
        .ok_or_else(|| "remote host enabled path is not valid UTF-8".to_string())?;
    let ui_root = app
        .path()
        .resource_dir()
        .ok()
        .map(|path| path.join("remote-dist"))
        .filter(|path| path.is_dir());
    let ui_root_text = ui_root
        .as_deref()
        .and_then(|path| path.to_str())
        .map(str::to_owned);
    let mut envs = vec![
        ("RIDGE_REMOTE_HOST_REGISTRY", registry),
        ("RIDGE_REMOTE_HOST_ENABLED_FILE", enabled),
    ];
    if let Some(root) = ui_root_text.as_deref() {
        envs.push(("RIDGE_REMOTE_UI_ROOT", root));
    }
    ridge_kernel::client::spawn_detached_with_env(
        &binary,
        &["host"],
        &envs,
    )?;

    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if let Some(status) = read_live_lan(&registry_path) {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "detached rdg host did not publish a ready registry within 12s (binary {})",
        binary.display()
    ))
}

pub fn lan_host_status(app: &AppHandle) -> Option<LanHostStatus> {
    lan_registry_path(app)
        .ok()
        .and_then(|path| read_live_lan(&path))
}

pub fn set_lan_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let registry = lan_registry_path(app)?;
    let path = enabled_path(&registry);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, if enabled { b"1" } else { b"0" }).map_err(|error| error.to_string())
}

pub fn stop_lan_host(app: &AppHandle) -> Result<(), String> {
    let registry_path = lan_registry_path(app)?;
    if let Some(status) = read_live_lan(&registry_path) {
        ridge_kernel::client::terminate_process(status.pid)?;
    }
    let _ = fs::remove_file(&registry_path);
    let _ = fs::remove_file(enabled_path(&registry_path));
    Ok(())
}

/// Persist credentials received from the WebView. The transport is started
/// only by an explicit `goOnline` action or by `reattach_cloud_host` when the
/// user had previously enabled cloud Remote. Credential sync must not turn a
/// passive login-state update into a public listener.
pub fn sync_cloud_credentials(
    app: &AppHandle,
    device_token: Option<&str>,
    device_name: Option<&str>,
    username: Option<&str>,
) -> Result<(), String> {
    let path = cloud_auth_path(app)?;
    match (device_token, device_name, username) {
        (Some(token), Some(device), Some(user))
            if !token.trim().is_empty()
                && !device.trim().is_empty()
                && !user.trim().is_empty() =>
        {
            let parent = path
                .parent()
                .ok_or_else(|| "cloud auth path has no parent".to_string())?;
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            let body = serde_json::json!({
                "token": token,
                "device_name": device,
                "username": user,
            });
            let tmp = path.with_extension("json.tmp");
            fs::write(&tmp, serde_json::to_vec(&body).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
            fs::rename(&tmp, &path).map_err(|error| error.to_string())?;
            let enabled_path = app_data_dir(app)?.join(CLOUD_ENABLED);
            if read_enabled_flag(&enabled_path, false) {
                ensure_cloud_host(app)
            } else {
                Ok(())
            }
        }
        _ => {
            let _ = fs::remove_file(path);
            let _ = stop_cloud_host(app);
            Ok(())
        }
    }
}

pub fn set_cloud_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let path = app_data_dir(app)?.join(CLOUD_ENABLED);
    fs::write(path, if enabled { b"1" } else { b"0" }).map_err(|error| error.to_string())
}

pub fn reattach_cloud_host(app: &AppHandle) -> Result<(), String> {
    let enabled = app_data_dir(app)?.join(CLOUD_ENABLED);
    if !read_enabled_flag(&enabled, false) {
        return Ok(());
    }
    ensure_cloud_host(app)
}

pub fn stop_cloud_host(app: &AppHandle) -> Result<(), String> {
    let registry_path = app_data_dir(app)?.join(CLOUD_REGISTRY);
    if let Some(record) = read_json::<CloudRegistry>(&registry_path) {
        if ridge_kernel::client::is_process_alive(record.pid) {
            ridge_kernel::client::terminate_process(record.pid)?;
        }
    }
    let _ = fs::remove_file(registry_path);
    Ok(())
}

pub fn ensure_cloud_host(app: &AppHandle) -> Result<(), String> {
    let auth_path = cloud_auth_path(app)?;
    if !auth_path.exists() {
        return Ok(());
    }
    let registry_path = app_data_dir(app)?.join(CLOUD_REGISTRY);
    if let Some(record) = read_json::<CloudRegistry>(&registry_path) {
        if record.schema == 1 && ridge_kernel::client::is_process_alive(record.pid) {
            return Ok(());
        }
        let _ = fs::remove_file(&registry_path);
    }
    let binary = locate_rdg_binary(app)?;
    let auth = auth_path
        .to_str()
        .ok_or_else(|| "cloud auth path is not valid UTF-8".to_string())?;
    let binding_path = auth_path.with_file_name("remote-cloud-pane.json");
    let binding = binding_path
        .to_str()
        .ok_or_else(|| "cloud PTY binding path is not valid UTF-8".to_string())?;
    let pid = ridge_kernel::client::spawn_detached_with_env(
        &binary,
        &["remote", "--daemon"],
        &[
            ("RIDGE_AUTH_FILE", auth),
            ("RIDGE_REMOTE_PTY_BINDING_FILE", binding),
        ],
    )?;
    let record = CloudRegistry {
        schema: 1,
        pid,
        started_at: unix_now(),
    };
    write_json(&registry_path, &record)?;
    Ok(())
}

fn read_live_lan(path: &Path) -> Option<LanHostStatus> {
    let record = read_json::<LanRegistry>(path)?;
    if record.schema != 1 || !ridge_kernel::client::is_process_alive(record.pid) {
        return None;
    }
    if !tcp_ready(record.port) {
        return None;
    }
    Some(LanHostStatus {
        pid: record.pid,
        port: record.port,
        lan_ip: record.lan_ip,
        tls: record.tls,
        enabled: read_enabled_flag(&enabled_path(path), record.enabled),
    })
}

fn enabled_path(registry: &Path) -> PathBuf {
    registry.with_file_name("remote-host.enabled")
}

fn default_enabled() -> bool {
    true
}

fn read_enabled_flag(path: &Path, fallback: bool) -> bool {
    match fs::read_to_string(path) {
        Ok(value) => value.trim() != "0",
        Err(_) => fallback,
    }
}

fn tcp_ready(port: u16) -> bool {
    let Ok(address) = format!("127.0.0.1:{port}").parse() else {
        return false;
    };
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let path = app.path().app_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&path).map_err(|error| error.to_string())?;
    Ok(path)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let raw = fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "registry path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    fs::rename(&tmp, path).map_err(|error| error.to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

fn locate_rdg_binary(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("RIDGE_RDG_BINARY") {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(resource) = app.path().resource_dir() {
        candidates.push(resource.join(exe_name("rdg")));
        if let Ok(entries) = fs::read_dir(resource) {
            candidates.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("rdg") || value.starts_with("rdg-"))
            }));
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(parent) = current.parent() {
            candidates.push(parent.join(exe_name("rdg")));
            for ancestor in parent.ancestors().take(4) {
                candidates.push(ancestor.join("target").join("debug").join(exe_name("rdg")));
                candidates.push(ancestor.join("target").join("release").join(exe_name("rdg")));
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| "rdg sidecar not found; set RIDGE_RDG_BINARY or build ridge-cli".into())
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_schema_is_stable() {
        let record = LanRegistry {
            schema: 1,
            pid: 7,
            port: 9527,
            lan_ip: "192.168.1.2".into(),
            tls: true,
            enabled: true,
            started_at: 1,
        };
        let value = serde_json::to_value(record).unwrap();
        assert_eq!(value["pid"], 7);
        assert_eq!(value["lanIp"], "192.168.1.2");
    }
}
