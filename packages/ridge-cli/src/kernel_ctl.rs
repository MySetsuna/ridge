//! 与桌面/ridge-kernel 共享的内核发现与控制（REQ-RIDGE-KERNEL-HOST-01）。

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use ridge_kernel::client::{
    is_process_alive, running_endpoint, shutdown_endpoint, spawn_detached, wait_for_running,
};
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
            .ok()
            .and_then(|s| s.trim().parse().ok())
    })
}

pub fn is_kernel_running() -> bool {
    running_endpoint().is_some()
}

pub fn is_kernel_process_alive(pid: u32) -> bool {
    is_process_alive(pid)
}

/// 是否有存活内核且不是本 rdg 进程（桌面/独立 kernel 在跑）。
pub fn desktop_kernel_running() -> bool {
    match read_endpoint() {
        Some(ep) if ep.pid != std::process::id() && is_process_alive(ep.pid) => true,
        _ => false,
    }
}

pub fn status_line() -> String {
    match read_endpoint() {
        Some(ep) if is_process_alive(ep.pid) => {
            format!(
                "内核运行中 (PID {} port {} health={})",
                ep.pid,
                ep.port,
                if is_kernel_running() { "ok" } else { "down" }
            )
        }
        Some(ep) => format!("内核已退出 (残留 PID {})", ep.pid),
        None => "内核未登记".into(),
    }
}

/// rdg 启动：内核不在则拉起（与桌面 detect-or-spawn 同契约）。
pub fn ensure_kernel_running() -> Result<KernelEndpoint, String> {
    if let Some(ep) = read_endpoint() {
        if is_process_alive(ep.pid) {
            if ridge_kernel::client::health_ok(&ep) {
                return Ok(ep);
            }
            return Err(format!(
                "live ridge-kernel PID {} is unhealthy or protocol-incompatible; refusing a second instance",
                ep.pid
            ));
        }
        // stale
        let _ = fs::remove_file(kernel_pid_path());
        let _ = fs::remove_file(kernel_json_path());
    }
    let bin = std::env::current_exe().map_err(|error| format!("定位 rdg: {error}"))?;
    spawn_detached(&bin, &[ridge_kernel::client::KERNEL_HOST_ARG])?;
    wait_for_running(Duration::from_secs(8)).ok_or_else(|| "ridge-kernel 未在时限内就绪".into())
}

/// 经内核领域 API 读 agent profiles（DOMAIN 验收路径）。
pub fn domain_agents() -> Result<String, String> {
    let ep = read_endpoint().ok_or_else(|| "内核未登记".to_string())?;
    if !is_kernel_running() {
        return Err("内核不可用".into());
    }
    let url = format!("http://127.0.0.1:{}/v1/domain/agents", ep.port);
    http_get_auth(&url, &ep.token).ok_or_else(|| "domain/agents 请求失败".into())
}

/// 经内核领域 API 列目录。
pub fn domain_fs_list(path: &str) -> Result<String, String> {
    let ep = read_endpoint().ok_or_else(|| "内核未登记".to_string())?;
    if !is_kernel_running() {
        return Err("内核不可用".into());
    }
    let enc = path.replace(' ', "%20");
    let url = format!("http://127.0.0.1:{}/v1/domain/fs/list?path={enc}", ep.port);
    http_get_auth(&url, &ep.token).ok_or_else(|| "domain/fs/list 请求失败".into())
}

fn http_get_auth(url: &str, token: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = rest.split_once('/')?;
    let path = format!("/{path}");
    let (host, port_s) = hostport.split_once(':')?;
    let port: u16 = port_s.parse().ok()?;
    let mut stream = TcpStream::connect((host, port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(2000)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(2000)))
        .ok()?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nx-ridge-kernel-token: {token}\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;
    buf.split("\r\n\r\n").nth(1).map(|s| s.to_string())
}

/// 经内核 Git status（DOMAIN）。
pub fn domain_git_status(path: &str) -> Result<String, String> {
    let ep = read_endpoint().ok_or_else(|| "内核未登记".to_string())?;
    if !is_kernel_running() {
        return Err("内核不可用".into());
    }
    let enc = path.replace(' ', "%20");
    let url = format!(
        "http://127.0.0.1:{}/v1/domain/git/status?path={enc}",
        ep.port
    );
    http_get_auth(&url, &ep.token).ok_or_else(|| "domain/git/status 请求失败".into())
}

/// 最小 MCP tools/list 冒烟（经内核 /api/v1/mcp）。
pub fn mcp_tools_list_smoke() -> Result<String, String> {
    let ep = read_endpoint().ok_or_else(|| "内核未登记".to_string())?;
    if !is_kernel_running() {
        return Err("内核不可用".into());
    }
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    http_post_json(
        &format!("http://127.0.0.1:{}/api/v1/mcp", ep.port),
        &ep.token,
        body,
    )
    .ok_or_else(|| "mcp tools/list 失败".into())
}

fn http_post_json(url: &str, token: &str, body: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = rest.split_once('/')?;
    let path = format!("/{path}");
    let (host, port_s) = hostport.split_once(':')?;
    let port: u16 = port_s.parse().ok()?;
    let mut stream = TcpStream::connect((host, port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(2000)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(2000)))
        .ok()?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-ridge-token: {token}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;
    buf.split("\r\n\r\n").nth(1).map(|s| s.to_string())
}

pub fn stop_kernel() -> Result<(), String> {
    let Some(ep) = read_endpoint() else {
        return Err("未找到 kernel.json".into());
    };
    if !is_process_alive(ep.pid) {
        let _ = fs::remove_file(kernel_pid_path());
        let _ = fs::remove_file(kernel_json_path());
        return Err(format!("进程 {} 已不存在", ep.pid));
    }
    shutdown_endpoint(&ep, Duration::from_secs(2))
}

/// 彻底退出二次确认：命令行 Y/N（默认 N）。非 TTY 仅当 `RIDGE_CONFIRM_QUIT_KERNEL=1` 通过。
/// 调用方须已离开 TUI raw/alternate screen，否则 stdin 读不到回车。
pub fn confirm_quit_kernel_with_desktop() -> bool {
    use std::io::{self, IsTerminal, Write};

    if !io::stdin().is_terminal() {
        return std::env::var("RIDGE_CONFIRM_QUIT_KERNEL").ok().as_deref() == Some("1");
    }
    eprintln!();
    eprintln!("桌面端或内核仍在运行。彻底退出将结束 Ridge 内核并导致桌面端退出。");
    eprint!("是否继续？[y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_alive() {
        assert!(is_process_alive(std::process::id()));
    }
}
