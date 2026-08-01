//! 内核发现文件读写（外壳与 kernel 共用路径约定）。

use std::fs;
use std::path::PathBuf;
use std::collections::HashMap;

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

/// Kernel-owned remote topology. Records intentionally exclude credentials.
pub fn remote_hosts_path() -> PathBuf {
    ridge_data_dir().join("remote-hosts.json")
}

pub fn workspace_graph_path() -> PathBuf { ridge_data_dir().join("workspace-graph.json") }

pub fn load_workspace_graph() -> Result<ridge_core::workspace::graph::WorkspaceGraph> {
    let path = workspace_graph_path();
    if !path.exists() { return Ok(ridge_core::workspace::graph::WorkspaceGraph::new()); }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

pub fn save_workspace_graph_at(path: &std::path::Path, graph: &ridge_core::workspace::graph::WorkspaceGraph) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(graph).context("serialize workspace graph")?)
        .with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

pub fn load_remote_hosts() -> Result<HashMap<String, ridge_core::remote::HostRecord>> {
    let path = remote_hosts_path();
    if !path.exists() { return Ok(HashMap::new()); }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

/// Atomic replacement prevents a stopped shell from leaving a partial topology file.
pub fn save_remote_hosts(hosts: &HashMap<String, ridge_core::remote::HostRecord>) -> Result<()> {
    save_remote_hosts_at(&remote_hosts_path(), hosts)
}

pub fn save_remote_hosts_at(
    path: &std::path::Path,
    hosts: &HashMap<String, ridge_core::remote::HostRecord>,
) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let raw = serde_json::to_vec_pretty(hosts).context("serialize remote hosts")?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, raw).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_contract_round_trips_for_all_shells() {
        let endpoint = KernelEndpoint {
            pid: 42,
            port: 34567,
            token: "test-token".into(),
            started_at_unix: 1,
        };
        let json = serde_json::to_string(&endpoint).unwrap();
        let decoded: KernelEndpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.pid, 42);
        assert_eq!(decoded.port, 34567);
        assert_eq!(decoded.token, "test-token");
    }
}
