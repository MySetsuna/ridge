//! Ridge 内核进程生命周期（REQ-RIDGE-KERNEL-HOST-01 最佳架构）。
//!
//! 独立 `ridge-kernel` 进程持有 control plane；桌面/rdg 为外壳。
//! 发现：`%LOCALAPPDATA%/ridge/kernel.pid` + `kernel.json`。

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

pub use ridge_kernel::registry::KernelEndpoint;

pub fn kernel_pid_path() -> PathBuf {
    ridge_kernel::registry::kernel_pid_path()
}

pub fn read_endpoint() -> Option<KernelEndpoint> {
    ridge_kernel::registry::read_endpoint()
}

pub fn read_kernel_pid() -> Option<u32> {
    read_endpoint().map(|e| e.pid).or_else(|| {
        fs::read_to_string(kernel_pid_path())
            .ok()?
            .trim()
            .parse()
            .ok()
    })
}

use ridge_kernel::client::{health_ok, is_process_alive, running_endpoint, shutdown_endpoint};

pub fn is_kernel_running() -> bool {
    running_endpoint().is_some()
}

pub fn ensure_kernel_running() -> Result<KernelEndpoint, String> {
    ridge_kernel::lifecycle::ensure_kernel_running()
}

/// OS-only launch/auth seam. No Kernel endpoint or private token is returned
/// to JavaScript; the Runtime issues its own limited presentation credential.
#[tauri::command]
pub async fn bootstrap_runtime_client(
    window: tauri::WebviewWindow,
) -> Result<ridge_kernel::local_client::ClientGrant, String> {
    let url = window.url().map_err(|error| error.to_string())?;
    let origin = if url.scheme() == "tauri" && url.host_str() == Some("localhost") {
        "tauri://localhost".to_string()
    } else {
        url.origin().ascii_serialization()
    };
    let device_id = format!("{}:{}", window.label(), uuid::Uuid::new_v4());
    tauri::async_runtime::spawn_blocking(move || {
        let endpoint = ensure_kernel_running()?;
        let status = ridge_kernel::client::request_json(&endpoint,"GET","/v1/status",None)?;
        if !status["domain"].as_array().is_some_and(|domains|
            ["local.presentation", "business.commands", "business.events"].iter().all(|required| domains.iter().any(|domain| domain == required))) {
            return Err("Running Runtime lacks the shared presentation command/event transport; update it explicitly before attaching".into());
        }
        let request = serde_json::json!({"deviceId":device_id,"origin":origin});
        let value = ridge_kernel::client::request_json(
            &endpoint,
            "POST",
            "/v1/local/clients",
            Some(&request),
        )?;
        let grant: ridge_kernel::local_client::ClientGrant = serde_json::from_value(value)
            .map_err(|error| format!("invalid Runtime client grant: {error}"))?;
        if grant.protocol_version != ridge_kernel::client::KERNEL_PROTOCOL_VERSION
            || grant.device_id != device_id
            || grant.origin != origin
            || grant.ws_url != format!("ws://127.0.0.1:{}/v1/local/ws", endpoint.port)
            || grant.token == endpoint.token
            || grant.token.len() != 64
            || status["host_id"].as_str() != Some(grant.host_id.as_str())
            || status["runtime_epoch"].as_str() != Some(grant.runtime_epoch.as_str())
        {
            return Err("Runtime client grant identity mismatch".into());
        }
        Ok(grant)
    })
    .await
    .map_err(|error| format!("Runtime bootstrap worker failed: {error}"))?
}

/// 监视启动/附着时确认过的精确内核 PID；瞬时 HTTP 故障不误退，
/// 且进程在 watcher 首轮前死亡仍会触发外壳退出（验收④）。
/// `should_stop` 为 true 时停止监视（本进程主动彻底退出途中）。
pub fn spawn_kernel_death_watcher(
    kernel_endpoint: KernelEndpoint,
    mut on_death: impl FnMut() + Send + 'static,
    should_stop: impl Fn() -> bool + Send + 'static,
) -> Result<std::thread::JoinHandle<()>, String> {
    let kernel_pid = kernel_endpoint.pid;
    std::thread::Builder::new()
        .name("ridge-kernel-watch".into())
        .spawn(move || {
            let mut health_failures = 0u32;
            loop {
                if should_stop() {
                    break;
                }
                let process_alive = is_process_alive(kernel_pid);
                let healthy = process_alive && health_ok(&kernel_endpoint);
                let (next_failures, should_exit) =
                    watcher_health_step(health_failures, process_alive, healthy);
                health_failures = next_failures;
                if should_exit {
                    if !process_alive {
                        tracing::warn!(
                            target: "ridge::kernel_lifecycle",
                            kernel_pid,
                            "ridge-kernel gone; shell will exit"
                        );
                    } else {
                        tracing::warn!(
                            target: "ridge::kernel_lifecycle",
                            kernel_pid,
                            health_failures,
                            "ridge-kernel health failed repeatedly; shell will exit"
                        );
                    }
                    on_death();
                    break;
                }
                thread::sleep(Duration::from_millis(1500));
            }
        })
        .map_err(|error| format!("spawn ridge-kernel watcher: {error}"))
}

fn watcher_health_step(previous_failures: u32, process_alive: bool, healthy: bool) -> (u32, bool) {
    const HEALTH_FAILURE_LIMIT: u32 = 3;
    if !process_alive || healthy {
        return (0, !process_alive);
    }
    let failures = previous_failures.saturating_add(1);
    (failures, failures >= HEALTH_FAILURE_LIMIT)
}

/// 彻底退出：请求内核 shutdown（不杀本桌面进程之外的逻辑由调用方 exit）。
pub fn shutdown_kernel() -> Result<(), String> {
    let Some(ep) = read_endpoint() else {
        return Ok(());
    };
    shutdown_endpoint(&ep, Duration::from_secs(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_kernel_running_without_registry_is_false_or_live() {
        let observed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let callback_observed = std::sync::Arc::clone(&observed);
        let watcher = spawn_kernel_death_watcher(
            KernelEndpoint {
                pid: u32::MAX,
                port: 0,
                token: String::new(),
                started_at_unix: 0,
            },
            move || callback_observed.store(true, std::sync::atomic::Ordering::Release),
            || false,
        )
        .expect("watcher thread should start");
        watcher.join().expect("watcher should exit after dead PID");
        assert!(observed.load(std::sync::atomic::Ordering::Acquire));
        // 无登记时必 false；有本机存活 kernel 时 true——仅断言不 panic。
        let _ = is_kernel_running();
    }

    #[test]
    fn watcher_health_requires_consecutive_failures_and_resets_on_recovery() {
        assert_eq!(watcher_health_step(0, true, false), (1, false));
        assert_eq!(watcher_health_step(1, true, false), (2, false));
        assert_eq!(watcher_health_step(2, true, false), (3, true));
        assert_eq!(watcher_health_step(3, true, true), (0, false));
        assert_eq!(watcher_health_step(0, false, false), (0, true));
    }
}
