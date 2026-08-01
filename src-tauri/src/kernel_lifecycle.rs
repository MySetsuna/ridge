//! Ridge 内核进程生命周期（REQ-RIDGE-KERNEL-HOST-01 最佳架构）。
//!
//! 独立 `ridge-kernel` 进程持有 control plane；桌面/rdg 为外壳。
//! 发现：`%LOCALAPPDATA%/ridge/kernel.pid` + `kernel.json`。

use std::fs;
use std::path::{Path, PathBuf};
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
    read_endpoint()
        .map(|e| e.pid)
        .or_else(|| fs::read_to_string(kernel_pid_path()).ok()?.trim().parse().ok())
}

use ridge_kernel::client::{
    health_ok, is_process_alive, running_endpoint, spawn_detached, terminate_process,
    wait_for_running,
};

pub fn is_kernel_running() -> bool {
    running_endpoint().is_some()
}

fn simple_http_post_auth(url: &str, token: &str) -> bool {
    (|| -> Option<bool> {
        let rest = url.strip_prefix("http://")?;
        let (hostport, path) = rest.split_once('/')?;
        let path = format!("/{path}");
        let (host, port_s) = hostport.split_once(':')?;
        let port: u16 = port_s.parse().ok()?;
        let mut stream = std::net::TcpStream::connect((host, port)).ok()?;
        stream
            .set_read_timeout(Some(Duration::from_millis(1500)))
            .ok()?;
        stream
            .set_write_timeout(Some(Duration::from_millis(1500)))
            .ok()?;
        use std::io::{Read, Write};
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nContent-Length: 0\r\nx-ridge-kernel-token: {token}\r\n\r\n"
        );
        stream.write_all(req.as_bytes()).ok()?;
        let mut buf = String::new();
        let _ = stream.read_to_string(&mut buf);
        Some(buf.contains("200") || buf.contains("\"ok\":true"))
    })()
    .unwrap_or(false)
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

/// 解析 ridge-kernel 可执行路径：同目录 / PATH / target/debug。
pub fn resolve_kernel_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in ["ridge-kernel.exe", "ridge-kernel"] {
                let p = dir.join(name);
                if p.is_file() {
                    return Some(p);
                }
            }
            // dev: target/debug next to src-tauri
            let dev = dir
                .join("..")
                .join("..")
                .join("target")
                .join("debug")
                .join(if cfg!(windows) {
                    "ridge-kernel.exe"
                } else {
                    "ridge-kernel"
                });
            if dev.is_file() {
                return Some(dev.canonicalize().unwrap_or(dev));
            }
            let dev2 = Path::new("target").join("debug").join(if cfg!(windows) {
                "ridge-kernel.exe"
            } else {
                "ridge-kernel"
            });
            if dev2.is_file() {
                return Some(dev2);
            }
        }
    }
    which_in_path("ridge-kernel")
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        });
        if p.is_file() {
            return Some(p);
        }
        #[cfg(windows)]
        {
            let p2 = dir.join(name);
            if p2.is_file() {
                return Some(p2);
            }
        }
    }
    None
}

/// 桌面 setup：detect-or-spawn 独立 ridge-kernel。
pub fn ensure_kernel_running() -> Result<KernelEndpoint, String> {
    let self_pid = std::process::id();
    let file_pid = read_kernel_pid();
    let alive = file_pid.is_some_and(is_process_alive);
    let decision = decide_boot(self_pid, file_pid, alive);

    match decision {
        KernelBootDecision::AttachExisting { pid } => {
            if let Some(ep) = read_endpoint().filter(|e| e.pid == pid && health_ok(e)) {
                tracing::info!(
                    target: "ridge::kernel_lifecycle",
                    pid,
                    port = ep.port,
                    "attached to existing ridge-kernel"
                );
                return Ok(ep);
            }
            // json 坏/health 失败 → 当 stale 处理
            tracing::warn!(
                target: "ridge::kernel_lifecycle",
                pid,
                "existing kernel pid alive but control plane unhealthy; will respawn"
            );
            let _ = fs::remove_file(kernel_pid_path());
            let _ = fs::remove_file(kernel_json_path());
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

    if let Some(ep) = running_endpoint() {
        return Ok(ep);
    }

    let bin = resolve_kernel_binary().ok_or_else(|| {
        "ridge-kernel binary not found (build packages/ridge-kernel or place next to ridge.exe)"
            .to_string()
    })?;
    tracing::info!(target: "ridge::kernel_lifecycle", path = %bin.display(), "spawning ridge-kernel");
    spawn_detached(&bin)?;
    wait_for_running(Duration::from_secs(8)).ok_or_else(|| {
        "ridge-kernel did not become healthy in time (check kernel.json / logs)".to_string()
    })
}

/// 已见过内核存活后若内核死亡 → 外壳必须自退（验收④）。
/// `should_stop` 为 true 时停止监视（本进程主动彻底退出途中）。
pub fn spawn_kernel_death_watcher(
    mut on_death: impl FnMut() + Send + 'static,
    should_stop: impl Fn() -> bool + Send + 'static,
) {
    std::thread::Builder::new()
        .name("ridge-kernel-watch".into())
        .spawn(move || {
            let mut saw = false;
            loop {
                if should_stop() {
                    break;
                }
                let alive = is_kernel_running();
                if alive {
                    saw = true;
                } else if saw {
                    tracing::warn!(
                        target: "ridge::kernel_lifecycle",
                        "ridge-kernel gone; shell will exit"
                    );
                    on_death();
                    break;
                }
                thread::sleep(Duration::from_millis(1500));
            }
        })
        .ok();
}

/// 彻底退出：请求内核 shutdown（不杀本桌面进程之外的逻辑由调用方 exit）。
pub fn shutdown_kernel() -> Result<(), String> {
    let Some(ep) = read_endpoint() else {
        return Ok(());
    };
    if !is_process_alive(ep.pid) {
        let _ = fs::remove_file(kernel_pid_path());
        let _ = fs::remove_file(kernel_json_path());
        return Ok(());
    }
    let url = format!("http://127.0.0.1:{}/v1/shutdown", ep.port);
    if !simple_http_post_auth(&url, &ep.token) {
        terminate_process(ep.pid)?;
    }
    // wait up to 2s
    for _ in 0..20 {
        if !is_process_alive(ep.pid) {
            let _ = fs::remove_file(kernel_pid_path());
            let _ = fs::remove_file(kernel_json_path());
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("kernel process {} did not exit within 2s", ep.pid))
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
        // 无登记时必 false；有本机存活 kernel 时 true——仅断言不 panic。
        let _ = is_kernel_running();
    }
}
