use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
        .env("RIDGE_CONFIRM_QUIT_KERNEL", "1");
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
        response["result"]["isError"],
        true,
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
    assert_eq!(receipt["status"], "submit_dispatched");
    assert_eq!(receipt["terminalAccepted"], true);
    let inbox: Value = serde_json::from_str(&mcp_tool(
        &endpoint,
        5,
        "ridge_inbox_read",
        json!({ "target_pane_id": pane_id }),
    ))
    .unwrap();
    assert!(
        inbox
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| {
                message["text"] == "RIDGE_KERNEL_DELEGATION_E2E"
            })),
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
