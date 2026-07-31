//! 内核发现文件读写（外壳与 kernel 共用路径约定）。

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelEndpoint {
    pub pid: u32,
    pub port: u16,
    pub token: String,
    pub started_at_unix: u64,
}

pub fn ridge_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge")
}

pub fn kernel_pid_path() -> PathBuf {
    ridge_data_dir().join("kernel.pid")
}

pub fn kernel_json_path() -> PathBuf {
    ridge_data_dir().join("kernel.json")
}

pub fn write_registry(ep: &KernelEndpoint) -> Result<()> {
    let dir = ridge_data_dir();
    fs::create_dir_all(&dir).context("create ridge data dir")?;
    fs::write(kernel_pid_path(), ep.pid.to_string()).context("write kernel.pid")?;
    let raw = serde_json::to_string_pretty(ep).context("serialize kernel.json")?;
    fs::write(kernel_json_path(), raw).context("write kernel.json")?;
    Ok(())
}

pub fn clear_registry(owner_pid: u32) {
    if let Ok(raw) = fs::read_to_string(kernel_pid_path()) {
        if raw.trim().parse::<u32>().ok() == Some(owner_pid) {
            let _ = fs::remove_file(kernel_pid_path());
            let _ = fs::remove_file(kernel_json_path());
        }
    }
}

#[allow(dead_code)] // 外壳侧读；本进程偶发诊断可复用
pub fn read_endpoint() -> Option<KernelEndpoint> {
    let raw = fs::read_to_string(kernel_json_path()).ok()?;
    serde_json::from_str(&raw).ok()
}
