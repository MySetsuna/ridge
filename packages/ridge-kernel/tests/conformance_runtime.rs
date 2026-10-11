//! Real shared-bootstrap, generation and PTY ownership conformance.
//! Former timestamp substitutes are archived under CHG-070.
use base64::Engine;
use ridge_kernel::{client, lifecycle, registry};
use ridge_term::terminal_protocol::{self, PaneRef, TerminalBody, TerminalInput};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn launch(root: &std::path::Path, label: &str, create: bool) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "bootstrap_helper", "--ignored"])
        .env("RIDGE_KERNEL_DATA_DIR", root)
        .env(
            "RIDGE_KERNEL_HOST_BINARY",
            env!("CARGO_BIN_EXE_ridge-kernel"),
        )
        .env("RIDGE_BOOTSTRAP_RESULT", root.join(label))
        .env("RIDGE_BOOTSTRAP_CREATE_PTY", if create { "1" } else { "0" })
        .env("RIDGE_TEST_ALLOW_NON_BREAKAWAY", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            fs::File::create(root.join(format!("{label}.out.log"))).unwrap(),
        ))
        .stderr(Stdio::from(
            fs::File::create(root.join(format!("{label}.err.log"))).unwrap(),
        ))
        .spawn()
        .unwrap()
}
fn wait(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        assert!(
            Instant::now() < deadline,
            "bootstrap helper exceeded deadline"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "launched by the owned process conformance test"]
fn bootstrap_helper() {
    let ep = lifecycle::ensure_kernel_running().unwrap();
    let mut result = json!({"pid":ep.pid,"startedAt":ep.started_at_unix});
    if std::env::var("RIDGE_BOOTSTRAP_CREATE_PTY").as_deref() == Ok("1") {
        let ws = client::request_json(
            &ep,
            "POST",
            "/v1/domain/workspaces",
            Some(&json!({"name":"070-owned-test"})),
        )
        .unwrap();
        let workspace = uuid::Uuid::parse_str(ws["workspace_id"].as_str().unwrap()).unwrap();
        let pane = uuid::Uuid::new_v4();
        let args = if cfg!(windows) {
            vec!["/d".into(), "/q".into(), "/k".into()]
        } else {
            vec![]
        };
        client::create_domain_pty_with_command(
            &ep,
            client::DomainPtyLaunch {
                pty_id: pane,
                program: Some(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }),
                args: &args,
                cwd: None,
                workspace_id: Some(workspace),
                role: "shell",
                launch_profile: Some("070-owned-test"),
                env: None,
                cols: Some(80),
                rows: Some(24),
            },
        )
        .unwrap();
        let info = client::list_domain_ptys(&ep)
            .unwrap()
            .into_iter()
            .find(|p| p.id == pane)
            .unwrap();
        result["workspace"] = json!(workspace);
        result["pane"] = json!(pane);
        result["ptyPid"] = json!(info.child_pid.unwrap());
    }
    // Public identities only; private Kernel tokens never enter the artifact.
    fs::write(
        std::env::var_os("RIDGE_BOOTSTRAP_RESULT").unwrap(),
        result.to_string(),
    )
    .unwrap();
}
struct OwnedKernel;
impl Drop for OwnedKernel {
    fn drop(&mut self) {
        if let Some(ep) = registry::read_endpoint().filter(client::health_ok) {
            let _ = client::shutdown_endpoint(&ep, Duration::from_secs(2));
        }
    }
}
#[test]
fn concurrent_shells_share_one_kernel_and_pty_outlives_its_creating_shell() {
    let root = std::env::temp_dir().join(format!("ridge-070-bootstrap-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    std::env::set_var("RIDGE_KERNEL_DATA_DIR", &root);
    std::env::set_var(
        "RIDGE_KERNEL_HOST_BINARY",
        env!("CARGO_BIN_EXE_ridge-kernel"),
    );
    std::env::set_var("RIDGE_TEST_ALLOW_NON_BREAKAWAY", "1");
    let _cleanup = OwnedKernel;
    let mut a = launch(&root, "first.json", true);
    let mut b = launch(&root, "second.json", false);
    wait(&mut a);
    wait(&mut b);
    let a: Value = serde_json::from_slice(&fs::read(root.join("first.json")).unwrap()).unwrap();
    let b: Value = serde_json::from_slice(&fs::read(root.join("second.json")).unwrap()).unwrap();
    assert_eq!(a["pid"], b["pid"]);
    assert_eq!(a["startedAt"], b["startedAt"]);
    let ep = lifecycle::ensure_kernel_running().unwrap();
    assert_eq!(u64::from(ep.pid), a["pid"].as_u64().unwrap());
    let status = client::request_json(&ep, "GET", "/v1/status", None).unwrap();
    let epoch = status["runtime_epoch"].as_str().unwrap().to_owned();
    assert_eq!(uuid::Uuid::parse_str(&epoch).unwrap().get_version_num(), 7);
    let pane = uuid::Uuid::parse_str(a["pane"].as_str().unwrap()).unwrap();
    let info = client::list_domain_ptys(&ep)
        .unwrap()
        .into_iter()
        .find(|p| p.id == pane)
        .unwrap();
    assert_eq!(
        u64::from(info.child_pid.unwrap()),
        a["ptyPid"].as_u64().unwrap()
    );
    let params = json!({"workspaceId":a["workspace"],"paneId":pane,"activationId":9});
    let path = format!("/v1/domain/ptys/{pane}/terminal");
    let snapshot = || {
        let response = client::request_json(
            &ep,
            "POST",
            &path,
            Some(&json!({"method":"get_terminal_snapshot","params":params})),
        )
        .unwrap();
        assert_eq!(response["ok"], true);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(response["value"].as_str().unwrap())
            .unwrap();
        terminal_protocol::decode_frame(&bytes).unwrap()
    };
    let initial = snapshot();
    let marker = format!("SURVIVED_{}", uuid::Uuid::new_v4());
    let input = TerminalInput {
        pane: PaneRef::new(a["workspace"].as_str().unwrap(), pane.to_string()),
        terminal_epoch: initial.terminal_epoch.clone(),
        activation_id: 9,
        source_id: "070-owned-shell".into(),
        sequence: 1,
        bytes: format!("echo {marker}\r").into_bytes(),
    };
    let request = json!({"method":"write_terminal_input","params":{"input":input}});
    let ack = client::request_json(&ep, "POST", &path, Some(&request)).unwrap();
    assert_eq!(ack["ok"], true);
    assert_eq!(ack["value"]["alreadyApplied"], false);
    let duplicate = client::request_json(&ep, "POST", &path, Some(&request)).unwrap();
    assert_eq!(duplicate["value"]["alreadyApplied"], true);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = snapshot();
        assert_eq!(frame.terminal_epoch, initial.terminal_epoch);
        if let TerminalBody::Snapshot(s) = frame.body {
            if s.primary
                .lines
                .iter()
                .any(|line| line.cells.iter().map(|c| c.ch).collect::<String>().trim() == marker)
            {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "surviving PTY did not execute canonical input"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    client::shutdown_endpoint(&ep, Duration::from_secs(2)).unwrap();
    let restarted = lifecycle::ensure_kernel_running().unwrap();
    let status = client::request_json(&restarted, "GET", "/v1/status", None).unwrap();
    assert_ne!(status["runtime_epoch"], epoch);
    assert_ne!(restarted.pid, ep.pid);
}
