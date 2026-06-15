//! 守护进程生命周期管理：PID 文件 + 信号。
//!
//! PID 文件存放在 `~/.config/ridge/daemon.pid`。
//! Unix: `kill` 命令发 SIGTERM/SIGKILL；`ps` 检查存活。
//! Windows: PID 文件语义相同，信号部分仅记录。

use std::fs;
use std::path::PathBuf;
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

/// Unix: `kill -0 <pid>` 检查进程存活。
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

/// Unix: `kill -TERM <pid>` 优雅停止，超时后 SIGKILL。
#[cfg(unix)]
pub fn stop_daemon() -> Result<()> {
    let pid = read_pid().context("未找到 PID 文件")?;
    if !is_process_alive(pid) {
        remove_pid();
        anyhow::bail!("进程 {} 已不存在", pid);
    }

    Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .context("kill -TERM 失败")?;

    // 等最多 5 秒。
    for _ in 0..50 {
        if !is_process_alive(pid) {
            remove_pid();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // 超时 → SIGKILL。
    Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status()
        .ok();
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
