#![cfg(windows)]
use ridge_core::process_identity::{instance, validate, descendant, ProcessInstance};
use ridge_tmux::{new_session, kill_session, kill_server, NewSessionReq};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Cleanup { socket: String, processes: Vec<ProcessInstance> }
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = kill_server(&self.socket);
        for process in &self.processes {
            if validate(process).is_ok() { ridge_core::process_guard::kill_process_tree(process.pid); }
        }
    }
}

#[test]
fn explicit_headless_termination_reclaims_attested_descendant() {
    let socket = format!("owned-tree-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos());
    let root = std::env::temp_dir().join(&socket);
    std::fs::create_dir_all(&root).unwrap();
    let receipt = root.join("owned-processes.txt");
    let path = receipt.to_string_lossy().replace('\'', "''");
    let command = format!("$child = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList '-NoProfile','-Command','Start-Sleep 120'; [IO.File]::WriteAllText('{path}', ($PID.ToString() + ',' + $child.Id.ToString())); Start-Sleep 120");
    let mut cleanup = Cleanup { socket: socket.clone(), processes: Vec::new() };
    new_session(NewSessionReq { socket: socket.clone(), name: Some("owned".into()), creator_workspace_id: None,
        creator_pane_id: None, window_name: None, cwd: Some(root.to_string_lossy().into_owned()), width: 80, height: 24,
        shell: Some("powershell.exe".into()), command: Some(command), attach_or_create: false, print: None }, &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !receipt.exists() { assert!(Instant::now() < deadline, "owned process receipt did not arrive"); std::thread::sleep(Duration::from_millis(20)); }
    let ids: Vec<u32> = std::fs::read_to_string(receipt).unwrap().split(',').map(|id| id.parse().unwrap()).collect();
    let parent = instance(ids[0]).unwrap();
    let child = instance(ids[1]).unwrap();
    assert!(descendant(&parent, &child).unwrap());
    cleanup.processes = vec![parent.clone(), child.clone()];
    kill_session(&socket, "=owned", &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while validate(&parent).is_ok() || validate(&child).is_ok() {
        assert!(Instant::now() < deadline, "explicit terminate left an owned process alive");
        std::thread::sleep(Duration::from_millis(20));
    }
}
