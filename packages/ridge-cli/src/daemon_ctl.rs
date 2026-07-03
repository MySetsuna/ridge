//! 守护进程生命周期管理：PID 文件 + 信号。
//!
//! PID 文件存放在 `~/.config/ridge/daemon.pid`。
//! Unix: 通过 `libc::kill` 系统调用发 SIGTERM/SIGKILL 与探测存活（不 shell 出
//!   `kill` 命令，避免 procps 诊断泄漏到终端 stderr）。
//! Windows: PID 文件语义相同，用 tasklist/taskkill。

use std::fs;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

use anyhow::{Context, Result};

const PID_FILE: &str = "daemon.pid";

fn config_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.config_dir().join("ridge"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn pid_path() -> PathBuf {
    config_dir().join(PID_FILE)
}

pub fn read_pid() -> Option<u32> {
    let content = fs::read_to_string(pid_path()).ok()?;
    content.trim().parse().ok()
}

pub fn write_pid(pid: u32) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir).context("create config dir")?;
    fs::write(dir.join(PID_FILE), pid.to_string()).context("write pid file")
}

pub fn remove_pid() {
    let _ = fs::remove_file(pid_path());
}

/// Unix: `kill(pid, 0)` 系统调用检查进程存活。
///
/// 直接走 libc，而非 shell 出 `kill -0`：procps 的 `kill` 在进程不存在时会把
/// `kill: (<pid>): No such process` 打到继承来的终端 stderr（探测函数返回值本身是对的，
/// 但这行噪音会漏出来，且 TUI 每帧探测时刷屏）。libc 直调既无输出也不 fork 进程。
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    // kill(pid, 0)：不发信号，仅做存在性/权限检查。
    //   0            → 进程存在且可发信号 → 存活
    //   -1 且 ESRCH  → 进程不存在         → 已退出
    //   -1 且 EPERM  → 进程存在但无权限   → 仍算存活
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Windows: 用 `tasklist` 检查进程。
#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map(|o| {
            let out = String::from_utf8_lossy(&o.stdout);
            out.contains(&pid.to_string())
        })
        .unwrap_or(false)
}

pub fn is_running() -> bool {
    read_pid().is_some_and(|pid| is_process_alive(pid))
}

pub fn status() -> String {
    match read_pid() {
        Some(pid) if is_process_alive(pid) => format!("运行中 (PID {})", pid),
        Some(pid) => format!("已退出 (PID {}，残留 PID 文件)", pid),
        None => "未运行".into(),
    }
}

/// Unix daemonize: fork + setsid。依赖系统 `kill` 命令（POSIX 必备）。
/// 暂不真正 fork（保持在前台 `rdg` 进程内），仅记录 PID 供外部管理。
#[cfg(unix)]
pub fn start_daemon() -> Result<()> {
    if is_running() {
        anyhow::bail!("守护进程已在运行 (PID {})", read_pid().unwrap());
    }
    write_pid(std::process::id())?;
    println!("守护进程 PID {} 已记录", std::process::id());
    Ok(())
}

#[cfg(windows)]
pub fn start_daemon() -> Result<()> {
    if is_running() {
        anyhow::bail!("守护进程已在运行 (PID {})", read_pid().unwrap());
    }
    write_pid(std::process::id())?;
    Ok(())
}

/// Unix: `SIGTERM` 优雅停止，超时后 `SIGKILL`（均经 `libc::kill` 直发）。
#[cfg(unix)]
pub fn stop_daemon() -> Result<()> {
    let pid = read_pid().context("未找到 PID 文件")?;
    if !is_process_alive(pid) {
        remove_pid();
        anyhow::bail!("进程 {} 已不存在", pid);
    }

    // SIGTERM 优雅停止（libc 直发，避免 shell 出 kill 泄漏 stderr）。
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }

    // 等最多 5 秒。
    for _ in 0..50 {
        if !is_process_alive(pid) {
            remove_pid();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // 超时 → SIGKILL。
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    remove_pid();
    anyhow::bail!("进程 {} 未响应 SIGTERM，已 SIGKILL", pid)
}

#[cfg(windows)]
pub fn stop_daemon() -> Result<()> {
    let pid = read_pid().context("未找到 PID 文件")?;
    Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .status()
        .context("taskkill 失败")?;
    remove_pid();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_process_alive_reports_self() {
        // 当前进程一定存活。
        assert!(is_process_alive(std::process::id()));
    }

    #[test]
    fn is_process_alive_rejects_dead_pid() {
        // 一个远超系统 pid_max、几乎不可能被占用的 PID → 应判为已退出。
        // 这正是用户遇到的 `kill: (694): No such process` 场景的最小复现：
        // 修复前（shell 出 `kill -0`）此调用会把该诊断打到 stderr；修复后（libc）静默。
        assert!(!is_process_alive(0x7FFF_FFF0));
    }
}
