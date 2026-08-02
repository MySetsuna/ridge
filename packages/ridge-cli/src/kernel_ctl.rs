//! 与桌面/ridge-kernel 共享的内核发现与控制（REQ-RIDGE-KERNEL-HOST-01）。

use std::fs;
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
    kernel_get_json("/v1/domain/agents")
}

/// 经内核领域 API 列目录。
pub fn domain_fs_list(path: &str) -> Result<String, String> {
    kernel_get_json(&format!("/v1/domain/fs/list?path={}", encode_query_component(path)))
        .map(|value| value.to_string())
}

/// 经内核 Git status（DOMAIN）。
pub fn domain_git_status(path: &str) -> Result<String, String> {
    kernel_get_json(&format!(
        "/v1/domain/git/status?path={}",
        encode_query_component(path)
    ))
    .map(|value| value.to_string())
}

/// 最小 MCP tools/list 冒烟（经内核 /api/v1/mcp）。
pub fn mcp_tools_list_smoke() -> Result<String, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
    });
    let value = kernel_request(
        read_endpoint().ok_or_else(|| "内核未登记".to_string())?,
        "POST",
        "/api/v1/mcp",
        Some(&body),
    )?;
    Ok(value.to_string())
}

fn kernel_get_json(path: &str) -> Result<String, String> {
    let endpoint = read_endpoint().ok_or_else(|| "内核未登记".to_string())?;
    let value = kernel_request(endpoint, "GET", path, None)?;
    Ok(value.to_string())
}

fn kernel_request(
    endpoint: KernelEndpoint,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    if !is_kernel_running() {
        return Err("内核不可用".into());
    }
    ridge_kernel::client::request_json(&endpoint, method, path, body)
}

/// Encode one query component without introducing a new URL dependency in the
/// CLI shell. This keeps Windows paths (`C:\\work\\a b`) and reserved bytes
/// from changing the kernel route or query boundaries.
fn encode_query_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0F) as usize] as char);
        }
    }
    encoded
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

    #[test]
    fn query_component_encodes_windows_paths_and_reserved_bytes() {
        assert_eq!(
            encode_query_component(r"C:\\work tree\a?b#c%20"),
            "C%3A%5C%5Cwork%20tree%5Ca%3Fb%23c%2520"
        );
    }
}
