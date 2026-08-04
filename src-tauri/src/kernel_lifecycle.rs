//! Ridge 内核进程生命周期（REQ-RIDGE-KERNEL-HOST-01 最佳架构）。
//!
//! 独立 `ridge-kernel` 进程持有 control plane；桌面/rdg 为外壳。
//! 发现：`%LOCALAPPDATA%/ridge/kernel.pid` + `kernel.json`。

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub use ridge_kernel::registry::KernelEndpoint;

pub fn kernel_pid_path() -> PathBuf {
    ridge_kernel::registry::kernel_pid_path()
}

pub fn kernel_json_path() -> PathBuf {
    ridge_kernel::registry::kernel_json_path()
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

use ridge_kernel::client::{
    health_ok, is_process_alive, running_endpoint, shutdown_endpoint, spawn_detached,
    wait_for_running,
};

static KERNEL_BOOT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn kernel_boot_lock() -> &'static Mutex<()> {
    KERNEL_BOOT_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn is_kernel_running() -> bool {
    running_endpoint().is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelBootDecision {
    BecomeHost,
    AlreadyHost,
    AttachExisting { pid: u32 },
    StalePidClearAndBecomeHost { stale_pid: u32 },
}

pub fn decide_boot(
    self_pid: u32,
    file_pid: Option<u32>,
    file_pid_alive: bool,
) -> KernelBootDecision {
    match file_pid {
        None => KernelBootDecision::BecomeHost,
        Some(pid) if pid == self_pid => KernelBootDecision::AlreadyHost,
        Some(pid) if file_pid_alive => KernelBootDecision::AttachExisting { pid },
        Some(pid) => KernelBootDecision::StalePidClearAndBecomeHost { stale_pid: pid },
    }
}

/// 桌面 setup：detect-or-spawn 独立 ridge-kernel。
pub fn ensure_kernel_running() -> Result<KernelEndpoint, String> {
    // Setup and the first Pane can arrive concurrently. Hold one process-local
    // gate across detect, spawn, and readiness so neither caller can launch a
    // second kernel or observe the first one's half-written registry.
    let _boot_guard = kernel_boot_lock()
        .lock()
        .map_err(|_| "ridge-kernel boot lock poisoned".to_string())?;
    let self_pid = std::process::id();
    let file_pid = read_kernel_pid();
    let alive = file_pid.is_some_and(is_process_alive);
    let decision = decide_boot(self_pid, file_pid, alive);

    match decision {
        KernelBootDecision::AttachExisting { pid } => {
            if let Some(ep) = wait_for_running(Duration::from_secs(8)).filter(|e| e.pid == pid) {
                tracing::info!(
                    target: "ridge::kernel_lifecycle",
                    pid,
                    port = ep.port,
                    "attached to existing ridge-kernel"
                );
                return Ok(ep);
            }
            return Err(format!(
                "live ridge-kernel PID {pid} is unhealthy or protocol-incompatible; refusing a second instance"
            ));
        }
        KernelBootDecision::StalePidClearAndBecomeHost { stale_pid } => {
            tracing::info!(target: "ridge::kernel_lifecycle", stale_pid, "clear stale kernel registry");
            let _ = fs::remove_file(kernel_pid_path());
            let _ = fs::remove_file(kernel_json_path());
        }
        KernelBootDecision::AlreadyHost => {
            // 桌面进程不应再写自己为 kernel（kernel 是独立二进制）。清掉误写的自 PID。
            if file_pid == Some(self_pid) {
                let _ = fs::remove_file(kernel_pid_path());
                let _ = fs::remove_file(kernel_json_path());
            }
        }
        KernelBootDecision::BecomeHost => {}
    }

    // The desktop and a detached `rdg host` may bootstrap concurrently. A
    // process-local mutex cannot serialize that case; reserve a separate
    // cross-process boot slot until the new kernel publishes a healthy
    // endpoint. The kernel's own instance lock remains independent.
    let boot_guard = if matches!(
        decision,
        KernelBootDecision::BecomeHost | KernelBootDecision::StalePidClearAndBecomeHost { .. }
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(ep) = running_endpoint() {
                return Ok(ep);
            }
            match ridge_kernel::registry::KernelBootGuard::try_acquire() {
                Ok(Some(guard)) => break Some(guard),
                Ok(None) if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(80));
                }
                Ok(None) => {
                    return Err("another ridge-kernel is booting but did not become healthy in time".into());
                }
                Err(error) => return Err(format!("acquire ridge-kernel boot slot: {error}")),
            }
        }
    } else {
        None
    };

    if let Some(ep) = running_endpoint() {
        return Ok(ep);
    }

    let bin = std::env::current_exe().map_err(|error| format!("locate ridge desktop: {error}"))?;
    tracing::info!(target: "ridge::kernel_lifecycle", path = %bin.display(), "spawning embedded ridge-kernel host");
    spawn_detached(&bin, &[ridge_kernel::client::KERNEL_HOST_ARG])?;
    let endpoint = wait_for_running(Duration::from_secs(8));
    drop(boot_guard);
    endpoint.ok_or_else(|| {
        "ridge-kernel did not become healthy in time (check kernel.json / logs)".to_string()
    })
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

fn watcher_health_step(
    previous_failures: u32,
    process_alive: bool,
    healthy: bool,
) -> (u32, bool) {
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
    fn decide_boot_no_file_becomes_host() {
        assert_eq!(
            decide_boot(100, None, false),
            KernelBootDecision::BecomeHost
        );
    }

    #[test]
    fn decide_boot_self_already_host() {
        assert_eq!(
            decide_boot(100, Some(100), true),
            KernelBootDecision::AlreadyHost
        );
    }

    #[test]
    fn decide_boot_other_alive_attach() {
        assert_eq!(
            decide_boot(100, Some(200), true),
            KernelBootDecision::AttachExisting { pid: 200 }
        );
    }

    #[test]
    fn decide_boot_stale_clears() {
        assert_eq!(
            decide_boot(100, Some(200), false),
            KernelBootDecision::StalePidClearAndBecomeHost { stale_pid: 200 }
        );
    }

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

    #[test]
    fn kernel_boot_lock_serializes_concurrent_bootstraps() {
        use std::sync::mpsc;

        let first_guard = kernel_boot_lock().lock().expect("boot lock should be usable");
        let (attempted_tx, attempted_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            attempted_tx.send(()).expect("worker should start");
            let _guard = kernel_boot_lock().lock().expect("worker should acquire lock");
            acquired_tx.send(()).expect("worker should report acquisition");
        });

        attempted_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should reach the boot gate");
        assert!(acquired_rx.try_recv().is_err());
        drop(first_guard);
        acquired_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should acquire after the first bootstrap exits");
        worker.join().expect("worker should exit cleanly");
    }
}
