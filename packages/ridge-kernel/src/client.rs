//! Shell-neutral discovery and health probes for the kernel control plane.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::registry::{clear_registry, read_endpoint, KernelEndpoint};

pub const KERNEL_HOST_ARG: &str = "--ridge-kernel-host";
pub const KERNEL_PROTOCOL_VERSION: u64 = 1;

/// Read-only domain projections shared by desktop and headless shells.
///
/// These are deliberately smaller than the shell-owned workspace/teammate
/// models: the kernel owns stable identity and topology, while each shell may
/// decorate the projection with UI-only names or window claims. Keeping the
/// decoder here gives future callers one authenticated, source-checked seam
/// without silently replacing the existing shell state.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct KernelWorkspaceSnapshot {
    pub active: Option<String>,
    #[serde(default)]
    pub workspaces: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct KernelAgentRosterSnapshot {
    pub leader_id: Option<String>,
    #[serde(default)]
    pub roster: Vec<ridge_core::teammate::model::Teammate>,
}

/// Exact identity-set comparison between the kernel projection and a shell's
/// visible projection. No names, ordering, or UI-only decorations participate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DomainIdentityDiff {
    pub intersection: Vec<String>,
    pub only_kernel: Vec<String>,
    pub only_shell: Vec<String>,
}

/// Explicit stable-key evidence that the two projections disagree on an
/// entity identity. The comparator never infers this from list position.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DomainIdentityMismatch {
    pub scope: String,
    pub key: String,
    pub kernel_id: String,
    pub shell_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DomainConvergenceReport {
    pub workspaces: DomainIdentityDiff,
    pub agents: DomainIdentityDiff,
    pub identity_mismatches: Vec<DomainIdentityMismatch>,
}

fn checked_identity_set(scope: &str, ids: &[String]) -> Result<BTreeSet<String>, String> {
    let mut result = BTreeSet::new();
    for id in ids {
        if id.trim().is_empty() {
            return Err(format!("{scope} projection contains an empty identity"));
        }
        if !result.insert(id.clone()) {
            return Err(format!(
                "{scope} projection contains duplicate identity {id}"
            ));
        }
    }
    Ok(result)
}

fn identity_diff(
    kernel_scope: &str,
    kernel_ids: &[String],
    shell_ids: &[String],
) -> Result<DomainIdentityDiff, String> {
    let kernel = checked_identity_set(&format!("kernel {kernel_scope}"), kernel_ids)?;
    let shell = checked_identity_set(&format!("shell {kernel_scope}"), shell_ids)?;
    Ok(DomainIdentityDiff {
        intersection: kernel.intersection(&shell).cloned().collect(),
        only_kernel: kernel.difference(&shell).cloned().collect(),
        only_shell: shell.difference(&kernel).cloned().collect(),
    })
}

/// Build a fail-visible, read-only convergence report. `identity_mismatches`
/// must come from an explicit stable key (for example pane ID); this function
/// deliberately does not guess mismatches from list ordering or set gaps.
pub fn build_domain_convergence_report(
    kernel_workspaces: &[String],
    shell_workspaces: &[String],
    kernel_agents: &[String],
    shell_agents: &[String],
    identity_mismatches: &[DomainIdentityMismatch],
) -> Result<DomainConvergenceReport, String> {
    for mismatch in identity_mismatches {
        if mismatch.scope.trim().is_empty()
            || mismatch.key.trim().is_empty()
            || mismatch.kernel_id.trim().is_empty()
            || mismatch.shell_id.trim().is_empty()
        {
            return Err("identity mismatch contains an empty field".to_string());
        }
        if mismatch.kernel_id == mismatch.shell_id {
            return Err(format!(
                "identity mismatch {}:{} has equal IDs {}",
                mismatch.scope, mismatch.key, mismatch.kernel_id
            ));
        }
    }
    let mut mismatches = identity_mismatches.to_vec();
    mismatches.sort_by(|left, right| {
        (&left.scope, &left.key, &left.kernel_id, &left.shell_id).cmp(&(
            &right.scope,
            &right.key,
            &right.kernel_id,
            &right.shell_id,
        ))
    });
    Ok(DomainConvergenceReport {
        workspaces: identity_diff("workspace", kernel_workspaces, shell_workspaces)?,
        agents: identity_diff("agent", kernel_agents, shell_agents)?,
        identity_mismatches: mismatches,
    })
}

/// Fetch both kernel-owned projections, then compare them with the shell's
/// visible IDs. Errors remain visible to the caller; no fallback to shell
/// state or persistence is performed.
pub fn read_domain_convergence(
    endpoint: &KernelEndpoint,
    shell_workspaces: &[String],
    shell_agents: &[String],
    identity_mismatches: &[DomainIdentityMismatch],
) -> Result<DomainConvergenceReport, String> {
    let kernel_workspaces = read_domain_workspaces(endpoint)?;
    let kernel_agents = read_domain_agent_roster(endpoint)?;
    let kernel_agent_ids = kernel_agents
        .roster
        .iter()
        .map(|agent| agent.id.clone())
        .collect::<Vec<_>>();
    build_domain_convergence_report(
        &kernel_workspaces.workspaces,
        shell_workspaces,
        &kernel_agent_ids,
        shell_agents,
        identity_mismatches,
    )
}

fn decode_domain_snapshot<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("kernel domain request failed")
            .to_string());
    }
    if value.get("source").and_then(Value::as_str) != Some("ridge-kernel") {
        return Err("kernel domain response has unexpected source".to_string());
    }
    serde_json::from_value(value).map_err(|error| format!("decode kernel domain response: {error}"))
}

/// Read the kernel-owned workspace identity/topology projection.
pub fn read_domain_workspaces(
    endpoint: &KernelEndpoint,
) -> Result<KernelWorkspaceSnapshot, String> {
    let value = request_json(endpoint, "GET", "/v1/domain/workspaces", None)?;
    decode_domain_snapshot(value)
}

/// Read the kernel-owned Agent roster projection.
pub fn read_domain_agent_roster(
    endpoint: &KernelEndpoint,
) -> Result<KernelAgentRosterSnapshot, String> {
    let value = request_json(endpoint, "GET", "/v1/domain/agents/roster", None)?;
    decode_domain_snapshot(value)
}

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
        "{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nx-ridge-kernel-token: {}\r\nx-ridge-token: {}\r\n{content_type}Content-Length: {}\r\n\r\n{payload}",
        endpoint.token, endpoint.token, payload.len(),
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
    use serde_json::json;
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
    fn domain_read_contract_decodes_identity_without_shell_decorations() {
        let workspaces: KernelWorkspaceSnapshot = decode_domain_snapshot(json!({
            "ok": true,
            "source": "ridge-kernel",
            "active": "workspace-a",
            "workspaces": ["workspace-a", "workspace-b"],
        }))
        .unwrap();
        assert_eq!(workspaces.active.as_deref(), Some("workspace-a"));
        assert_eq!(workspaces.workspaces, ["workspace-a", "workspace-b"]);

        let roster: KernelAgentRosterSnapshot = decode_domain_snapshot(json!({
            "ok": true,
            "source": "ridge-kernel",
            "leader_id": "codex-1",
            "roster": [{
                "id": "codex-1",
                "name": "Codex",
                "pane_id": 7,
                "role": "Worker",
                "status": "Idle",
                "capability": "Skilled"
            }]
        }))
        .unwrap();
        assert_eq!(roster.leader_id.as_deref(), Some("codex-1"));
        assert_eq!(roster.roster[0].id, "codex-1");
        assert_eq!(roster.roster[0].pane_id, 7);
    }

    #[test]
    fn domain_read_contract_rejects_error_and_non_kernel_sources() {
        let error = decode_domain_snapshot::<KernelWorkspaceSnapshot>(json!({
            "ok": false,
            "source": "ridge-kernel",
            "error": "not ready"
        }))
        .unwrap_err();
        assert_eq!(error, "not ready");

        let source = decode_domain_snapshot::<KernelWorkspaceSnapshot>(json!({
            "ok": true,
            "source": "tauri",
            "active": null,
            "workspaces": []
        }))
        .unwrap_err();
        assert_eq!(source, "kernel domain response has unexpected source");
    }

    #[test]
    fn convergence_report_separates_exact_sets_and_sorts_mismatches() {
        let report = build_domain_convergence_report(
            &["workspace-b".into(), "workspace-a".into()],
            &["workspace-a".into(), "workspace-c".into()],
            &["agent-b".into(), "agent-a".into()],
            &["agent-a".into(), "agent-c".into()],
            &[DomainIdentityMismatch {
                scope: "agent".into(),
                key: "pane:7".into(),
                kernel_id: "agent-b".into(),
                shell_id: "agent-c".into(),
            }],
        )
        .unwrap();
        assert_eq!(
            report.workspaces.intersection,
            vec!["workspace-a".to_string()]
        );
        assert_eq!(
            report.workspaces.only_kernel,
            vec!["workspace-b".to_string()]
        );
        assert_eq!(
            report.workspaces.only_shell,
            vec!["workspace-c".to_string()]
        );
        assert_eq!(report.agents.intersection, vec!["agent-a".to_string()]);
        assert_eq!(report.identity_mismatches[0].key, "pane:7");
        assert_eq!(
            serde_json::to_value(&report).unwrap()["workspaces"]["onlyKernel"],
            json!(["workspace-b"])
        );
    }

    #[test]
    fn convergence_report_fails_visible_on_duplicate_or_invalid_identity() {
        let duplicate = build_domain_convergence_report(
            &["workspace-a".into(), "workspace-a".into()],
            &[],
            &[],
            &[],
            &[],
        )
        .unwrap_err();
        assert!(duplicate.contains("duplicate identity"));

        let empty_mismatch = build_domain_convergence_report(
            &[],
            &[],
            &[],
            &[],
            &[DomainIdentityMismatch {
                scope: "agent".into(),
                key: "pane:7".into(),
                kernel_id: "".into(),
                shell_id: "agent-c".into(),
            }],
        )
        .unwrap_err();
        assert_eq!(empty_mismatch, "identity mismatch contains an empty field");
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
