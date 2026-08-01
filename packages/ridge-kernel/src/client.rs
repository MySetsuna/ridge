//! Shell-neutral discovery and health probes for the kernel control plane.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use std::path::Path;
use serde_json::Value;

use crate::registry::{read_endpoint, KernelEndpoint};

#[cfg(windows)]
pub fn is_process_alive(pid: u32) -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .creation_flags(0x0800_0000)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_process_alive(pid: u32) -> bool {
    (unsafe { libc::kill(pid as libc::pid_t, 0) == 0 })
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn health_ok(endpoint: &KernelEndpoint) -> bool {
    let hostport = format!("127.0.0.1:{}", endpoint.port);
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", endpoint.port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
    let request = format!("GET /v1/health HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).is_ok()
        && {
            let mut body = String::new();
            stream.read_to_string(&mut body).is_ok()
                && (body.contains("\"ok\":true") || body.contains("\"ok\": true"))
        }
}

pub fn running_endpoint() -> Option<KernelEndpoint> {
    read_endpoint().filter(|endpoint| is_process_alive(endpoint.pid) && health_ok(endpoint))
}

pub fn spawn_detached(binary: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let result = {
        use std::os::windows::process::CommandExt;
        const FLAGS: u32 = 0x0000_0008 | 0x0000_0200 | 0x0800_0000;
        std::process::Command::new(binary).creation_flags(FLAGS).spawn()
    };
    #[cfg(not(windows))]
    let result = {
        std::process::Command::new(binary).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()
    };
    result.map(|_| ())
    .map_err(|error| format!("spawn ridge-kernel: {error}"))
}

pub fn wait_for_running(timeout: Duration) -> Option<KernelEndpoint> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(endpoint) = running_endpoint() { return Some(endpoint); }
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

/// Bounded authenticated JSON request for shell adapters. No retry: callers own
/// their projection policy and must not create duplicate control-plane writes.
pub fn request_json(
    endpoint: &KernelEndpoint,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, String> {
    let hostport = format!("127.0.0.1:{}", endpoint.port);
    let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port))
        .map_err(|error| format!("connect kernel: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_millis(1500))).map_err(|e| e.to_string())?;
    stream.set_write_timeout(Some(Duration::from_millis(1500))).map_err(|e| e.to_string())?;
    let payload = body.map(serde_json::to_string).transpose().map_err(|e| e.to_string())?.unwrap_or_default();
    let content_type = if body.is_some() { "Content-Type: application/json\r\n" } else { "" };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nx-ridge-kernel-token: {}\r\n{content_type}Content-Length: {}\r\n\r\n{payload}",
        endpoint.token, payload.len(),
    );
    stream.write_all(request.as_bytes()).map_err(|e| format!("write kernel: {e}"))?;
    let mut raw = String::new();
    stream.take(2 * 1024 * 1024).read_to_string(&mut raw).map_err(|e| format!("read kernel: {e}"))?;
    let (head, body) = raw.split_once("\r\n\r\n").ok_or_else(|| "malformed kernel response".to_string())?;
    if !head.starts_with("HTTP/1.1 200") { return Err(format!("kernel HTTP response: {}", head.lines().next().unwrap_or("unknown"))); }
    serde_json::from_str(body).map_err(|e| format!("parse kernel JSON: {e}"))
}
