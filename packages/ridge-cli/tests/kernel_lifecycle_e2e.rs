use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use ridge_core::process_guard::{run_command_status_with_timeout, run_command_with_timeout};
use ridge_kernel::registry::KernelEndpoint;
use serde_json::{json, Value};

fn isolated_data_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "ridge-kernel-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn rdg_command(binary: &Path, data_dir: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env("RIDGE_KERNEL_DATA_DIR", data_dir)
        .env("RIDGE_CONFIRM_QUIT_KERNEL", "1")
        // The hosted Windows test runner can reject CREATE_BREAKAWAY_FROM_JOB.
        // Production never sets this opt-in; it exists only to keep the
        // lifecycle assertions deterministic under that constrained runner.
        .env("RIDGE_TEST_ALLOW_NON_BREAKAWAY", "1");
    command
}

fn run_rdg(binary: &Path, data_dir: &Path, args: &[&str]) -> std::process::Output {
    run_command_with_timeout(
        &mut rdg_command(binary, data_dir, args),
        Duration::from_secs(15),
    )
    .unwrap_or_else(|error| panic!("rdg {args:?}: {error}"))
}

fn ensure_rdg(binary: &Path, data_dir: &Path) -> std::process::ExitStatus {
    run_command_status_with_timeout(
        &mut rdg_command(binary, data_dir, &["kernel", "ensure"]),
        Duration::from_secs(15),
    )
    .unwrap_or_else(|error| panic!("rdg kernel ensure: {error}"))
}

fn spawn_rdg(binary: &Path, data_dir: &Path, args: &[&str]) -> Child {
    rdg_command(binary, data_dir, args)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn rdg {args:?}: {error}"))
}

fn wait_for_endpoint(data_dir: &Path, timeout: Duration) -> KernelEndpoint {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(bytes) = fs::read(data_dir.join("kernel.json")) {
            if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                if ridge_kernel::client::health_ok(&endpoint) {
                    return endpoint;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "ridge-kernel did not become healthy before client lifecycle probe timed out"
        );
        std::thread::sleep(Duration::from_millis(40));
    }
}

struct KernelCleanup {
    binary: PathBuf,
    data_dir: PathBuf,
}

impl Drop for KernelCleanup {
    fn drop(&mut self) {
        let _ = run_command_with_timeout(
            &mut rdg_command(&self.binary, &self.data_dir, &["kernel", "stop"]),
            Duration::from_secs(10),
        );
        let _ = fs::remove_dir_all(&self.data_dir);
    }
}

fn mcp_request(endpoint: &KernelEndpoint, body: Value) -> Value {
    ridge_kernel::client::request_json(endpoint, "POST", "/api/v1/mcp", Some(&body))
        .unwrap_or_else(|error| panic!("kernel MCP request failed: {error}"))
}

fn mcp_tool(endpoint: &KernelEndpoint, id: u64, name: &str, arguments: Value) -> String {
    let response = mcp_request(
        endpoint,
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        }),
    );
    assert!(
        response.get("error").is_none() || response["error"].is_null(),
        "MCP tool {name} failed: {response}"
    );
    assert_ne!(
        response["result"]["isError"], true,
        "MCP tool {name} returned an error: {response}"
    );
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("MCP tool {name} returned no text: {response}"))
        .to_string()
}

#[test]
fn standalone_rdg_converges_to_one_kernel_and_serves_domain_and_mcp() {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_rdg"));
    let data_dir = isolated_data_dir();
    fs::create_dir_all(&data_dir).unwrap();
    let _cleanup = KernelCleanup {
        binary: binary.clone(),
        data_dir: data_dir.clone(),
    };

    let first_binary = binary.clone();
    let first_dir = data_dir.clone();
    let first = std::thread::spawn(move || ensure_rdg(&first_binary, &first_dir));
    let second_binary = binary.clone();
    let second_dir = data_dir.clone();
    let second = std::thread::spawn(move || ensure_rdg(&second_binary, &second_dir));
    for status in [first.join().unwrap(), second.join().unwrap()] {
        assert!(status.success(), "concurrent ensure failed: {status}");
    }

    let endpoint: KernelEndpoint =
        serde_json::from_slice(&fs::read(data_dir.join("kernel.json")).unwrap()).unwrap();
    let owner_pid = endpoint.pid;

    let status = run_rdg(&binary, &data_dir, &["kernel", "status"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains(&owner_pid.to_string()));

    let root = data_dir.to_string_lossy().into_owned();
    let fs_list = run_rdg(&binary, &data_dir, &["kernel", "fs-list", &root]);
    assert!(
        fs_list.status.success(),
        "kernel FS domain unavailable: {}",
        String::from_utf8_lossy(&fs_list.stderr)
    );
    assert!(String::from_utf8_lossy(&fs_list.stdout).contains("kernel.json"));

    let initialized = mcp_request(
        &endpoint,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "ridge-e2e", "version": "test" } },
        }),
    );
    assert_eq!(initialized["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(
        initialized["result"]["serverInfo"]["name"],
        "agents-commune"
    );

    let mcp = run_rdg(&binary, &data_dir, &["kernel", "mcp-smoke"]);
    assert!(
        mcp.status.success(),
        "kernel MCP unavailable: {}",
        String::from_utf8_lossy(&mcp.stderr)
    );
    assert!(String::from_utf8_lossy(&mcp.stdout).contains("ridge_split_pane"));

    let split: Value = serde_json::from_str(&mcp_tool(
        &endpoint,
        2,
        "ridge_split_pane",
        json!({
            "direction": "vertical",
            "role": "worker",
            "initial_cmd": "echo RIDGE_KERNEL_MCP_OK",
        }),
    ))
    .unwrap();
    let pane_id = split["paneId"].as_str().expect("split pane id");
    let workspace_id = split["workspaceId"].as_str().expect("split workspace id");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let capture = mcp_tool(
            &endpoint,
            3,
            "ridge_capture_pane",
            json!({ "target_pane_id": pane_id, "lines": 40 }),
        );
        if capture.contains("RIDGE_KERNEL_MCP_OK") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "kernel MCP pane never produced marker; last capture: {capture}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let identity = json!({
        "agent_id": "e2e-agent",
        "session_id": "e2e-session",
        "workspace_id": workspace_id,
        "pane_id": pane_id,
        "cwd": data_dir.to_string_lossy(),
        "executable": "ridge-cli-e2e",
        "argv": ["--e2e"],
        "generation": 1,
        "lease": "e2e-lease",
        "lifecycle": "Online",
        "online": true,
        "last_seen_unix_ms": 1,
        "capabilities": ["messages", "tasks", "events"]
    });
    let committed = ridge_kernel::client::request_json(
        &endpoint,
        "POST",
        "/v1/domain/agents/identities/commit",
        Some(&identity),
    )
    .expect("commit e2e agent identity");
    assert_eq!(committed["ok"], true, "identity commit failed: {committed}");

    let receipt: Value = serde_json::from_str(&mcp_tool(
        &endpoint,
        4,
        "ridge_delegate_task",
        json!({
            "target_pane_id": pane_id,
            "objective": "RIDGE_KERNEL_DELEGATION_E2E",
        }),
    ))
    .unwrap();
    assert_eq!(receipt["status"], "queued");
    assert_eq!(receipt["deliveryAdapter"], "mcp_pull");
    assert_eq!(receipt["terminalAccepted"], false);
    let inbox: Value = serde_json::from_str(&mcp_tool(
        &endpoint,
        5,
        "ridge_inbox_read",
        json!({ "target_pane_id": pane_id }),
    ))
    .unwrap();
    assert!(
        inbox.as_array().is_some_and(|messages| messages
            .iter()
            .any(|message| { message["payload"]["objective"] == "RIDGE_KERNEL_DELEGATION_E2E" })),
        "delegation was not retained in the shared inbox: {inbox}"
    );

    let stopped = run_rdg(&binary, &data_dir, &["kernel", "stop"]);
    assert!(
        stopped.status.success(),
        "kernel stop failed: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(!data_dir.join("kernel.json").exists());
    assert!(!data_dir.join("kernel.pid").exists());
    assert!(
        ridge_kernel::client::request_json(
            &endpoint,
            "POST",
            "/api/v1/mcp",
            Some(&json!({"jsonrpc":"2.0","id":6,"method":"tools/list"})),
        )
        .is_err(),
        "stopped kernel still accepted MCP requests"
    );
}

#[test]
fn detached_kernel_survives_client_process_exit_and_second_attach() {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_rdg"));
    let data_dir = isolated_data_dir();
    fs::create_dir_all(&data_dir).unwrap();
    let _cleanup = KernelCleanup {
        binary: binary.clone(),
        data_dir: data_dir.clone(),
    };

    // The shell/client is disposable: once the detached kernel is healthy,
    // terminate the waiting rdg process and prove a fresh client can attach
    // to the same PID. This guards the Windows CREATE flags / Unix setsid
    // contract instead of only checking logical semaphore convergence.
    let mut client = spawn_rdg(&binary, &data_dir, &["kernel", "ensure"]);
    let endpoint = wait_for_endpoint(&data_dir, Duration::from_secs(8));
    if client.try_wait().unwrap().is_none() {
        client.kill().expect("terminate disposable rdg client");
    }
    let _ = client.wait();

    let attached = run_rdg(&binary, &data_dir, &["kernel", "ensure"]);
    assert!(
        attached.status.success(),
        "second client failed to attach: {}",
        String::from_utf8_lossy(&attached.stderr)
    );
    let reattached = wait_for_endpoint(&data_dir, Duration::from_secs(2));
    assert_eq!(reattached.pid, endpoint.pid);
    assert!(ridge_kernel::client::health_ok(&reattached));
}

fn wait_for_kernel_marker(
    endpoint: &KernelEndpoint,
    pty_id: uuid::Uuid,
    lease_id: uuid::Uuid,
    marker: &str,
) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut captured = Vec::new();
    loop {
        match ridge_kernel::client::poll_domain_pty_output(endpoint, pty_id, lease_id, 250, 32)
            .expect("kernel PTY output poll")
        {
            ridge_kernel::client::KernelPtyOutput::Data(bytes) => {
                captured.extend(bytes);
                if String::from_utf8_lossy(&captured).contains(marker) {
                    return captured;
                }
            }
            ridge_kernel::client::KernelPtyOutput::Lagged => {
                ridge_kernel::client::resync_domain_pty_output(endpoint, pty_id, lease_id)
                    .expect("kernel PTY output resync");
            }
            ridge_kernel::client::KernelPtyOutput::Timeout => {}
        }
        assert!(
            Instant::now() < deadline,
            "kernel PTY never produced marker {marker:?}; captured={:?}",
            String::from_utf8_lossy(&captured)
        );
    }
}

#[test]
fn kernel_pty_survives_client_detach_and_replays_after_cursor() {
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_rdg"));
    let data_dir = isolated_data_dir();
    fs::create_dir_all(&data_dir).unwrap();
    let _cleanup = KernelCleanup {
        binary: binary.clone(),
        data_dir: data_dir.clone(),
    };
    assert!(ensure_rdg(&binary, &data_dir).success());
    let endpoint = wait_for_endpoint(&data_dir, Duration::from_secs(8));
    let pty_id = uuid::Uuid::new_v4();

    ridge_kernel::client::create_domain_pty(
        &endpoint,
        pty_id,
        None,
        Some(data_dir.to_string_lossy().as_ref()),
        None,
        "shell",
        Some("ridge-interactive"),
    )
    .expect("create stable kernel PTY");

    let first_lease = ridge_kernel::client::attach_domain_pty_output(&endpoint, pty_id, None)
        .expect("attach first output lease");
    ridge_kernel::client::write_domain_pty(
        &endpoint,
        pty_id,
        b"echo RIDGE_KERNEL_REATTACH_ONE\r\n",
    )
    .expect("write first marker");
    let _ = wait_for_kernel_marker(&endpoint, pty_id, first_lease, "RIDGE_KERNEL_REATTACH_ONE");

    let latest_seq = ridge_kernel::client::list_domain_ptys(&endpoint)
        .expect("list kernel PTYs")
        .into_iter()
        .find(|info| info.pty_id == pty_id)
        .expect("stable PTY remains registered")
        .next_seq
        .saturating_sub(1);
    ridge_kernel::client::detach_domain_pty_output(&endpoint, pty_id, first_lease)
        .expect("detach desktop-side output lease");

    // Simulate the desktop shell being gone: the kernel-owned child remains
    // writable, and a replacement proxy resumes after the last consumed frame.
    ridge_kernel::client::write_domain_pty(
        &endpoint,
        pty_id,
        b"echo RIDGE_KERNEL_REATTACH_TWO\r\n",
    )
    .expect("write while desktop proxy is detached");
    let second_lease =
        ridge_kernel::client::attach_domain_pty_output(&endpoint, pty_id, Some(latest_seq))
            .expect("reattach output lease after desktop restart");
    let replayed =
        wait_for_kernel_marker(&endpoint, pty_id, second_lease, "RIDGE_KERNEL_REATTACH_TWO");
    assert!(String::from_utf8_lossy(&replayed).contains("RIDGE_KERNEL_REATTACH_TWO"));
    ridge_kernel::client::detach_domain_pty_output(&endpoint, pty_id, second_lease)
        .expect("detach replacement output lease");
    ridge_kernel::client::destroy_domain_pty(&endpoint, pty_id).expect("destroy test PTY");
}
