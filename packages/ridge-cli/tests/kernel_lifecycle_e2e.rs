use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use ridge_core::process_guard::{run_command_status_with_timeout, run_command_with_timeout};

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

    let endpoint: serde_json::Value =
        serde_json::from_slice(&fs::read(data_dir.join("kernel.json")).unwrap()).unwrap();
    let owner_pid = endpoint["pid"].as_u64().unwrap();

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
    assert!(String::from_utf8_lossy(&mcp.stdout).contains("ridge_kernel_fs_list"));

    let stopped = run_rdg(&binary, &data_dir, &["kernel", "stop"]);
    assert!(
        stopped.status.success(),
        "kernel stop failed: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(!data_dir.join("kernel.json").exists());
    assert!(!data_dir.join("kernel.pid").exists());
}
