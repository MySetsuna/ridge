//! 内核发现文件读写（外壳与 kernel 共用路径约定）。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

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
    if let Some(path) = std::env::var_os("RIDGE_KERNEL_DATA_DIR").filter(|path| !path.is_empty()) {
        return PathBuf::from(path);
    }
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

pub fn kernel_lock_path() -> PathBuf {
    ridge_data_dir().join("kernel.lock")
}

pub fn kernel_boot_lock_path() -> PathBuf {
    ridge_data_dir().join("kernel.boot.lock")
}

/// Process-lifetime cross-shell singleton guard. The lock file may persist,
/// but the OS releases its lock when the owning process exits or crashes.
pub struct KernelInstanceGuard {
    _file: File,
}

impl KernelInstanceGuard {
    pub fn try_acquire() -> Result<Option<Self>> {
        Self::try_acquire_at(&kernel_lock_path())
    }

    fn try_acquire_at(path: &Path) -> Result<Option<Self>> {
        Ok(try_lock_file(path)?.map(|file| Self { _file: file }))
    }
}

/// Cross-process boot slot. Unlike [`KernelInstanceGuard`], this lock is only
/// held while a shell is spawning/waiting for the kernel endpoint; the kernel
/// process itself owns `kernel.lock` and never contends with this file.
pub struct KernelBootGuard {
    _file: File,
}

impl KernelBootGuard {
    pub fn try_acquire() -> Result<Option<Self>> {
        Ok(try_lock_file(&kernel_boot_lock_path())?.map(|file| Self { _file: file }))
    }
}

fn try_lock_file(path: &Path) -> Result<Option<File>> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => {
            Err(error).with_context(|| format!("lock {}", path.display()))
        }
    }
}

/// Kernel-owned remote topology. Records intentionally exclude credentials.
pub fn remote_hosts_path() -> PathBuf {
    ridge_data_dir().join("remote-hosts.json")
}

pub fn workspace_graph_path() -> PathBuf {
    ridge_data_dir().join("workspace-graph.json")
}

pub fn roster_path() -> PathBuf {
    ridge_data_dir().join("agent-roster.json")
}

pub fn agent_hub_path() -> PathBuf {
    ridge_data_dir().join("agent-hub.sqlite3")
}

pub fn load_roster() -> Result<ridge_core::teammate::topology::TopologyGraph> {
    let path = roster_path();
    if !path.exists() {
        return Ok(ridge_core::teammate::topology::TopologyGraph::new());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

pub fn save_roster_at(
    path: &std::path::Path,
    roster: &ridge_core::teammate::topology::TopologyGraph,
) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(roster).context("serialize agent roster")?,
    )
    .with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

pub fn load_workspace_graph() -> Result<ridge_core::workspace::graph::WorkspaceGraph> {
    let path = workspace_graph_path();
    if !path.exists() {
        return Ok(ridge_core::workspace::graph::WorkspaceGraph::new());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
}

pub fn save_workspace_graph_at(
    path: &std::path::Path,
    graph: &ridge_core::workspace::graph::WorkspaceGraph,
) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(graph).context("serialize workspace graph")?,
    )
    .with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}

pub fn load_remote_hosts() -> Result<HashMap<String, ridge_core::remote::HostRecord>> {
    let path = remote_hosts_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }
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
    fs::rename(&tmp, path).with_context(|| format!("replace {}", path.display()))?;
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
    clear_registry_at(&kernel_pid_path(), &kernel_json_path(), owner_pid);
}

fn clear_registry_at(pid_path: &Path, json_path: &Path, owner_pid: u32) {
    if let Ok(raw) = fs::read_to_string(pid_path) {
        if raw.trim().parse::<u32>().ok() == Some(owner_pid) {
            let _ = fs::remove_file(pid_path);
            let _ = fs::remove_file(json_path);
        }
    }
}

/// Clear stale endpoint files only while no kernel owns the instance lock.
/// The lock is the process identity; PID liveness alone is unsafe on Windows.
pub fn try_clear_registry_if_instance_free(owner_pid: u32) -> Result<bool> {
    try_clear_registry_if_instance_free_at(
        &kernel_lock_path(),
        &kernel_pid_path(),
        &kernel_json_path(),
        owner_pid,
    )
}

fn try_clear_registry_if_instance_free_at(
    lock_path: &Path,
    pid_path: &Path,
    json_path: &Path,
    owner_pid: u32,
) -> Result<bool> {
    let Some(_instance_guard) = KernelInstanceGuard::try_acquire_at(lock_path)? else {
        return Ok(false);
    };
    clear_registry_at(pid_path, json_path, owner_pid);
    Ok(true)
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

    #[test]
    fn process_lock_excludes_a_second_kernel_until_owner_drops() {
        let path = std::env::temp_dir().join(format!(
            "ridge-kernel-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = KernelInstanceGuard::try_acquire_at(&path)
            .unwrap()
            .expect("first owner");
        assert!(KernelInstanceGuard::try_acquire_at(&path)
            .unwrap()
            .is_none());
        drop(first);
        assert!(KernelInstanceGuard::try_acquire_at(&path)
            .unwrap()
            .is_some());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn process_lock_excludes_a_second_process() {
        let path = std::env::temp_dir().join(format!(
            "ridge-kernel-lock-cross-process-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = KernelInstanceGuard::try_acquire_at(&path)
            .unwrap()
            .expect("first process owns lock");

        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "registry::tests::process_lock_probe_child",
                "--nocapture",
            ])
            .env("RIDGE_KERNEL_LOCK_PROBE", &path)
            .output()
            .expect("spawn lock probe child");
        assert!(
            child.status.success(),
            "lock probe child failed: {}",
            String::from_utf8_lossy(&child.stderr)
        );
        assert!(String::from_utf8_lossy(&child.stdout).contains("probe-acquired=false"));

        drop(first);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn process_lock_probe_child() {
        let Some(path) = std::env::var_os("RIDGE_KERNEL_LOCK_PROBE") else {
            return;
        };
        let acquired = KernelInstanceGuard::try_acquire_at(Path::new(&path))
            .unwrap()
            .is_some();
        println!("probe-acquired={acquired}");
        assert!(!acquired, "child acquired a lock owned by another process");
    }

    #[test]
    fn stale_registry_cleanup_requires_free_instance_lock() {
        let root = std::env::temp_dir().join(format!(
            "ridge-registry-recovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let lock_path = root.join("kernel.lock");
        let pid_path = root.join("kernel.pid");
        let json_path = root.join("kernel.json");

        fs::write(&pid_path, "13652").unwrap();
        fs::write(&json_path, "{\"pid\":13652}").unwrap();
        assert!(
            try_clear_registry_if_instance_free_at(&lock_path, &pid_path, &json_path, 13652,)
                .unwrap()
        );
        assert!(!pid_path.exists());
        assert!(!json_path.exists());

        fs::write(&pid_path, "13652").unwrap();
        fs::write(&json_path, "{\"pid\":13652}").unwrap();
        let owner = KernelInstanceGuard::try_acquire_at(&lock_path)
            .unwrap()
            .expect("test kernel should own instance lock");
        assert!(
            !try_clear_registry_if_instance_free_at(&lock_path, &pid_path, &json_path, 13652,)
                .unwrap()
        );
        assert!(pid_path.exists());
        assert!(json_path.exists());
        drop(owner);
        assert!(
            try_clear_registry_if_instance_free_at(&lock_path, &pid_path, &json_path, 13652,)
                .unwrap()
        );
        assert!(!pid_path.exists());
        assert!(!json_path.exists());
        let _ = fs::remove_dir_all(root);
    }
}
