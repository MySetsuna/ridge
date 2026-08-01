//! Shell-neutral discovery and health probes for the kernel control plane.

use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::registry::{clear_registry, read_endpoint, KernelEndpoint};

pub const KERNEL_HOST_ARG: &str = "--ridge-kernel-host";
pub const KERNEL_PROTOCOL_VERSION: u64 = 1;

pub fn kernel_host_requested() -> bool {
    std::env::args().nth(1).as_deref() == Some(KERNEL_HOST_ARG)
}

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

#[cfg(windows)]
pub fn terminate_process(pid: u32) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|error| format!("terminate kernel process {pid}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "terminate kernel process {pid}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(windows))]
pub fn terminate_process(pid: u32) -> Result<(), String> {
    let group = -(pid as libc::pid_t);
    if unsafe { libc::kill(group, libc::SIGKILL) } == 0 {
        Ok(())
    } else {
        let group_error = std::io::Error::last_os_error();
        if group_error.raw_os_error() == Some(libc::ESRCH) {
            if unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) } == 0 {
                return Ok(());
            }
            let process_error = std::io::Error::last_os_error();
            if process_error.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            return Err(format!("terminate kernel process {pid}: {process_error}"));
        }
        Err(format!(
            "terminate kernel process group {pid}: {group_error}"
        ))
    }
}

pub fn health_ok(endpoint: &KernelEndpoint) -> bool {
    let hostport = format!("127.0.0.1:{}", endpoint.port);
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", endpoint.port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
    let request =
        format!("GET /v1/health HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).is_ok() && {
        let mut body = String::new();
        stream.read_to_string(&mut body).is_ok()
            && body
                .split_once("\r\n\r\n")
                .and_then(|(_, json)| serde_json::from_str::<Value>(json).ok())
                .is_some_and(|health| {
                    health.get("ok").and_then(Value::as_bool) == Some(true)
                        && health.get("role").and_then(Value::as_str) == Some("ridge-kernel")
                        && health.get("protocolVersion").and_then(Value::as_u64)
                            == Some(KERNEL_PROTOCOL_VERSION)
                })
    }
}

pub fn running_endpoint() -> Option<KernelEndpoint> {
    read_endpoint().filter(|endpoint| is_process_alive(endpoint.pid) && health_ok(endpoint))
}

fn configure_detached(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const FLAGS: u32 = 0x0000_0008 | 0x0000_0200 | 0x0800_0000;
        command.creation_flags(FLAGS);
    }
    #[cfg(not(windows))]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

pub fn spawn_detached(binary: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = Command::new(binary);
    command.args(args);
    configure_detached(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("spawn ridge-kernel: {error}"))
}

pub fn wait_for_running(timeout: Duration) -> Option<KernelEndpoint> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(endpoint) = running_endpoint() {
            return Some(endpoint);
        }
        std::thread::sleep(Duration::from_millis(80));
    }
    None
}

fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !is_process_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    !is_process_alive(pid)
}

/// Graceful control-plane shutdown followed by bounded process-tree force.
/// Registry cleanup happens only after the operating system proves death.
pub fn shutdown_endpoint(endpoint: &KernelEndpoint, timeout: Duration) -> Result<(), String> {
    shutdown_endpoint_with(endpoint, timeout, clear_registry)
}

fn shutdown_endpoint_with(
    endpoint: &KernelEndpoint,
    timeout: Duration,
    cleanup: impl FnOnce(u32),
) -> Result<(), String> {
    if !is_process_alive(endpoint.pid) {
        cleanup(endpoint.pid);
        return Ok(());
    }

    let graceful = request_json(endpoint, "POST", "/v1/shutdown", None).is_ok();
    if graceful && wait_for_exit(endpoint.pid, timeout) {
        cleanup(endpoint.pid);
        return Ok(());
    }

    terminate_process(endpoint.pid)?;
    if !wait_for_exit(endpoint.pid, timeout) {
        return Err(format!(
            "kernel process {} did not exit within {}ms after process-tree termination",
            endpoint.pid,
            timeout.as_millis()
        ));
    }
    cleanup(endpoint.pid);
    Ok(())
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
    stream
        .set_read_timeout(Some(Duration::from_millis(1500)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_millis(1500)))
        .map_err(|e| e.to_string())?;
    let payload = body
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let content_type = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nx-ridge-kernel-token: {}\r\n{content_type}Content-Length: {}\r\n\r\n{payload}",
        endpoint.token, payload.len(),
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write kernel: {e}"))?;
    let mut raw = String::new();
    stream
        .take(2 * 1024 * 1024)
        .read_to_string(&mut raw)
        .map_err(|e| format!("read kernel: {e}"))?;
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| "malformed kernel response".to_string())?;
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!(
            "kernel HTTP response: {}",
            head.lines().next().unwrap_or("unknown")
        ));
    }
    serde_json::from_str(body).map_err(|e| format!("parse kernel JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const TREE_HELPER: &str = "client::tests::detached_process_tree_helper";

    fn health_probe(body: &'static str) -> bool {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        let healthy = health_ok(&KernelEndpoint {
            pid: std::process::id(),
            port,
            token: "unused".into(),
            started_at_unix: 1,
        });
        server.join().unwrap();
        healthy
    }

    #[test]
    fn health_requires_matching_kernel_protocol() {
        assert!(health_probe(
            r#"{"ok":true,"role":"ridge-kernel","protocolVersion":1}"#
        ));
        assert!(!health_probe(
            r#"{"ok":true,"role":"ridge-kernel","protocolVersion":2}"#
        ));
        assert!(!health_probe(r#"{"ok":true,"role":"ridge-kernel"}"#));
    }

    #[test]
    fn detached_process_tree_helper() {
        match std::env::var("RIDGE_KERNEL_TREE_HELPER").ok().as_deref() {
            Some("parent") => {
                let pid_path = std::env::var_os("RIDGE_KERNEL_TREE_PID_PATH").unwrap();
                let mut grandchild = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TREE_HELPER, "--nocapture"])
                    .env("RIDGE_KERNEL_TREE_HELPER", "grandchild")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap();
                fs::write(pid_path, grandchild.id().to_string()).unwrap();
                loop {
                    thread::sleep(Duration::from_secs(30));
                    let _ = grandchild.try_wait();
                }
            }
            Some("grandchild") => loop {
                thread::sleep(Duration::from_secs(30));
            },
            _ => {}
        }
    }

    #[test]
    fn shutdown_kills_detached_process_tree_before_cleanup() {
        let pid_path = std::env::temp_dir().join(format!(
            "ridge-kernel-tree-{}-{}.pid",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", TREE_HELPER, "--nocapture"])
            .env("RIDGE_KERNEL_TREE_HELPER", "parent")
            .env("RIDGE_KERNEL_TREE_PID_PATH", &pid_path);
        configure_detached(&mut command);
        let mut parent = command.spawn().unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !pid_path.exists() && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        let grandchild_pid: u32 = fs::read_to_string(&pid_path)
            .expect("grandchild pid")
            .trim()
            .parse()
            .unwrap();
        let endpoint = KernelEndpoint {
            pid: parent.id(),
            port: 9,
            token: "unreachable-test-token".into(),
            started_at_unix: 1,
        };
        let cleaned = Arc::new(AtomicBool::new(false));
        let cleaned_for_callback = Arc::clone(&cleaned);
        shutdown_endpoint_with(&endpoint, Duration::from_secs(3), move |pid| {
            assert!(!is_process_alive(pid), "cleanup ran before parent death");
            cleaned_for_callback.store(true, Ordering::Release);
        })
        .unwrap();
        let _ = parent.wait();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while is_process_alive(grandchild_pid) && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        assert!(cleaned.load(Ordering::Acquire));
        assert!(
            !is_process_alive(grandchild_pid),
            "grandchild survived tree kill"
        );
        let _ = fs::remove_file(pid_path);
    }
}
