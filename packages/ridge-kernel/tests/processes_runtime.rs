//! Real foreground child and exact process ownership through production local RPC.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use serde_json::json;
use std::{
    fs,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[test]
#[ignore = "owned child process launched in the real PTY"]
fn foreground_child_fixture() {
    fs::write(
        std::env::var_os("RIDGE_FOREGROUND_READY").unwrap(),
        b"ready",
    )
    .unwrap();
    loop {
        std::thread::sleep(Duration::from_millis(100));
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreground_queries_use_actual_kernel_child_and_reject_cross_workspace() {
    let root = std::env::temp_dir().join(format!("ridge-foreground-runtime-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let workspace = client::request_json(
        &owned.endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"foreground"})),
    )
    .unwrap()["workspace_id"]
        .clone();
    let other = client::request_json(&owned.endpoint, "POST", "/v1/domain/workspaces", None)
        .unwrap()["workspace_id"]
        .clone();
    let pane = Uuid::new_v4();
    let ready = root.join("ready");
    let created=client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({
        "pty_id":pane,"workspace_id":workspace,"program":if cfg!(windows){"cmd.exe"}else{"/bin/sh"},
        "args":if cfg!(windows){json!(["/d","/q","/k"])}else{json!([])},"cwd":root,
        "env":{"RIDGE_FOREGROUND_READY":ready},"role":"shell","cols":80,"rows":24
    }))).unwrap();
    assert_eq!(created["ok"], true);
    let mut socket = attach(&owned.endpoint).await;
    let params = json!({"workspaceId":workspace,"paneId":pane});
    let ordinary = rpc(&mut socket, 1, "get_pty_runtime_identity", params.clone()).await;
    assert!(ordinary["error"].is_null());
    assert!(
        ordinary["result"].is_null(),
        "shell executable/title cannot fabricate an Agent identity"
    );
    let executable = std::env::current_exe().unwrap();
    client::write_domain_pty(
        &owned.endpoint,
        pane,
        format!(
            "\"{}\" --exact foreground_child_fixture --ignored\r",
            executable.display()
        )
        .as_bytes(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while !ready.exists() {
        assert!(
            Instant::now() < deadline,
            "foreground child readiness deadline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let reply = rpc(
        &mut socket,
        2,
        "get_pane_foreground_process",
        params.clone(),
    )
    .await;
    assert!(reply["error"].is_null(), "{reply}");
    let observed = reply["result"]
        .as_str()
        .expect("actual foreground child name");
    assert!(
        executable
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_lowercase()
            .starts_with(&observed.to_lowercase()),
        "OS reported a different child: {observed}"
    );
    let cross = rpc(
        &mut socket,
        3,
        "get_pane_foreground_process",
        json!({"workspaceId":other,"paneId":pane}),
    )
    .await;
    assert!(!cross["error"].is_null());
    let malformed = rpc(
        &mut socket,
        4,
        "get_pty_runtime_identity",
        json!({"workspaceId":workspace,"paneId":"invalid"}),
    )
    .await;
    assert!(!malformed["error"].is_null());
    let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"get_pane_foreground_process","args":params,"native":false,"roots":[],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(denied["ok"], false);
    client::destroy_domain_pty(&owned.endpoint, pane).unwrap();
    let gone = rpc(&mut socket, 5, "get_pane_foreground_process", params).await;
    assert!(!gone["error"].is_null());
    assert!(gone["error"]["message"].as_str().unwrap().contains("pane does not belong to workspace"));
    socket.close(None).await.unwrap();
}
