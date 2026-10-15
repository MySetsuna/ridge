#![cfg(windows)]
use ridge_core::process_identity::{instance, validate, descendant, ProcessInstance};
use ridge_kernel::pty::{PtyLaunch, PtyRegistry};
use std::time::{Duration, Instant};
use uuid::Uuid;
#[path = "common/runtime_fixture.rs"]
mod fixture;

struct Cleanup(Vec<ProcessInstance>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for process in &self.0 {
            if validate(process).is_ok() { ridge_core::process_guard::kill_process_tree(process.pid); }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn kernel_destroy_reclaims_descendant_before_reaping_original_child() {
    let id = Uuid::new_v4();
    let root = std::env::temp_dir().join(format!("ridge-owned-pty-tree-{id}"));
    std::fs::create_dir_all(&root).unwrap();
    let receipt = root.join("processes.txt");
    let path = receipt.to_string_lossy().replace('\'', "''");
    let command = format!("$child = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile','-Command','Start-Sleep 120'; [IO.File]::WriteAllText('{path}', ($PID.ToString() + ',' + $child.Id.ToString())); Start-Sleep 120");
    let args = vec!["-NoProfile".into(), "-Command".into(), command];
    let registry = PtyRegistry::default();
    registry.spawn_command_for(PtyLaunch { id, workspace_id: None, role: "shell", program: Some("powershell.exe"), args: &args,
        cwd: root.to_str(), launch_profile: None, env: None, initial_size: None }).unwrap();
    let parent = instance(registry.info(id).unwrap().child_pid.unwrap()).unwrap();
    let mut cleanup = Cleanup(vec![parent.clone()]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !receipt.exists() { assert!(Instant::now() < deadline, "owned child receipt did not arrive"); tokio::time::sleep(Duration::from_millis(20)).await; }
    let ids: Vec<u32> = std::fs::read_to_string(receipt).unwrap().split(',').map(|id| id.parse().unwrap()).collect();
    assert_eq!(ids[0], parent.pid);
    let child = instance(ids[1]).unwrap();
    assert!(descendant(&parent, &child).unwrap());
    cleanup.0.push(child.clone());
    registry.destroy(id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while validate(&parent).is_ok() || validate(&child).is_ok() {
        assert!(Instant::now() < deadline, "Kernel destroy left an attested process alive");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    registry.destroy(id).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_drop_reclaims_its_attested_process_tree() {
    let root = std::env::temp_dir().join(format!("ridge-drop-pty-tree-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let receipt = root.join("processes.txt");
    let path = receipt.to_string_lossy().replace('\'', "''");
    let command = format!("$child = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile','-Command','Start-Sleep 120'; [IO.File]::WriteAllText('{path}', ($PID.ToString() + ',' + $child.Id.ToString())); Start-Sleep 120");
    let args = vec!["-NoProfile".into(), "-Command".into(), command];
    let (bridge, _output) = ridge_kernel::pty::PtyBridge::spawn_command(Some("powershell.exe"), &args, root.to_str(), None).unwrap();
    let parent = instance(bridge.process_id().unwrap()).unwrap();
    let mut cleanup = Cleanup(vec![parent.clone()]);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !receipt.exists() { assert!(Instant::now() < deadline); tokio::time::sleep(Duration::from_millis(20)).await; }
    let ids: Vec<u32> = std::fs::read_to_string(receipt).unwrap().split(',').map(|id| id.parse().unwrap()).collect();
    assert_eq!(ids[0], parent.pid);
    let child = instance(ids[1]).unwrap();
    assert!(descendant(&parent, &child).unwrap());
    cleanup.0.push(child.clone());
    drop(bridge);
    let deadline = Instant::now() + Duration::from_secs(3);
    while validate(&parent).is_ok() || validate(&child).is_ok() {
        assert!(Instant::now() < deadline, "Drop left an attested process alive");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_service_shutdown_reclaims_owned_pty_tree_without_client_fallback() {
    let root = std::env::temp_dir().join(format!("ridge-service-pty-tree-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let receipt = root.join("processes.txt");
    let path = receipt.to_string_lossy().replace('\'', "''");
    let command = format!("$child = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile','-Command','Start-Sleep 120'; [IO.File]::WriteAllText('{path}', ($PID.ToString() + ',' + $child.Id.ToString())); Start-Sleep 120");
    let owned = fixture::start(&root).await;
    let presentation = fixture::attach(&owned.endpoint).await;
    let kernel = instance(owned.endpoint.pid).unwrap();
    let created = ridge_kernel::client::request_json(&owned.endpoint, "POST", "/v1/domain/ptys", Some(&serde_json::json!({
        "program":"powershell.exe", "args":["-NoProfile","-Command",command], "cwd":root
    }))).unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let mut cleanup = Cleanup(Vec::new());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !receipt.exists() { assert!(Instant::now() < deadline); tokio::time::sleep(Duration::from_millis(20)).await; }
    let ids: Vec<u32> = std::fs::read_to_string(receipt).unwrap().split(',').map(|id| id.parse().unwrap()).collect();
    let parent = instance(ids[0]).unwrap();
    let child = instance(ids[1]).unwrap();
    assert!(descendant(&kernel, &parent).unwrap());
    assert!(descendant(&parent, &child).unwrap());
    cleanup.0 = vec![parent.clone(), child.clone()];
    drop(presentation);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(validate(&kernel).is_ok());
    assert!(validate(&parent).is_ok());
    assert!(validate(&child).is_ok(), "presentation detach must retain the owned PTY tree");
    let reply = ridge_kernel::client::request_json(&owned.endpoint, "POST", "/v1/shutdown", None).unwrap();
    assert_eq!(reply["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while validate(&kernel).is_ok() || validate(&parent).is_ok() || validate(&child).is_ok() {
        assert!(Instant::now() < deadline, "graceful service shutdown left an owned process alive");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!root.join("kernel.json").exists());
    drop(owned);
}
