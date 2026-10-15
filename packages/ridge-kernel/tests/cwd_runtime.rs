//! Live cwd, durable graph/file updates and rollback use the same Runtime boundary.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use base64::Engine;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use serde_json::{json, Value};
use std::{
    fs,
    time::{Duration, Instant},
};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_observes_real_cd_preserves_saved_fields_and_rolls_back_failed_graph_commit() {
    let root = std::env::temp_dir().join(format!("ridge-cwd-runtime-{}", Uuid::new_v4()));
    let moved = root.join("moved");
    let failed = root.join("failed");
    fs::create_dir_all(&moved).unwrap();
    fs::create_dir_all(&failed).unwrap();
    let owned = start(&root).await;
    let workspace = client::request_json(
        &owned.endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"cwd owner"})),
    )
    .unwrap()["workspace_id"]
        .clone();
    let pane = Uuid::new_v4();
    let topology = json!({"pane_tree":{"root":{"Leaf":pane},"panes":{pane.to_string():{"id":pane,"mode":"Terminal","cwd":root}}}});
    client::request_json(
        &owned.endpoint,
        "PUT",
        &format!(
            "/v1/domain/workspaces/{}/topology",
            workspace.as_str().unwrap()
        ),
        Some(&topology),
    )
    .unwrap();
    let created=client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({
        "pty_id":pane,"workspace_id":workspace,"program":if cfg!(windows){"cmd.exe"}else{"/bin/sh"},
        "args":if cfg!(windows){json!(["/d","/q","/k"])}else{json!([])},"cwd":root,"env":{},"role":"shell","cols":80,"rows":24
    }))).unwrap();
    assert_eq!(created["ok"], true);
    let mut socket = attach(&owned.endpoint).await;
    let params = json!({"workspaceId":workspace,"paneId":pane});
    let path = root.join("saved.ridge");
    assert!(rpc(
        &mut socket,
        1,
        "save_workspace_to_file",
        json!({"workspaceId":workspace,"name":"Saved cwd","path":path})
    )
    .await["error"]
        .is_null());
    let mut document: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    document["legacyUnknown"] = json!({"keep":"verbatim"});
    document["pane_tree"]["panes"][pane.to_string()]["legacyPaneField"] = json!("keep");
    fs::write(&path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
    let change = |target: &std::path::Path, marker: &str| {
        let command = if cfg!(windows) {
            format!(
                "cd /d \"{}\"\r\necho ready > \"{}\"\r\n",
                target.display(),
                root.join(marker).display()
            )
        } else {
            format!(
                "cd \"{}\"\necho ready > \"{}\"\n",
                target.display(),
                root.join(marker).display()
            )
        };
        client::write_domain_pty(&owned.endpoint, pane, command.as_bytes()).unwrap();
    };
    change(&moved, "moved-ready");
    let deadline = Instant::now() + Duration::from_secs(8);
    while !root.join("moved-ready").exists() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let result = rpc(&mut socket, 2, "get_pane_cwd", params.clone()).await;
    assert!(result["error"].is_null(), "{result}");
    let normalized = result["result"]
        .as_str()
        .expect("actual OS cwd")
        .to_string();
    let snapshot_cwd = || {
        let response=client::request_json(&owned.endpoint,"POST",&format!("/v1/domain/ptys/{pane}/terminal"),
            Some(&json!({"method":"get_terminal_snapshot","params":{"workspaceId":workspace,"paneId":pane,"activationId":15}}))).unwrap();
        assert_eq!(response["ok"], true);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(response["value"].as_str().unwrap())
            .unwrap();
        let ridge_term::terminal_protocol::TerminalBody::Snapshot(snapshot) =
            ridge_term::terminal_protocol::decode_frame(&bytes)
                .unwrap()
                .body
        else {
            unreachable!()
        };
        snapshot.cwd
    };
    assert_eq!(
        snapshot_cwd(),
        normalized,
        "semantic terminal projection must share Runtime cwd"
    );
    assert_eq!(
        std::path::Path::new(&normalized),
        moved.as_path(),
        "OS cwd must reflect a real cd, even without OSC 7"
    );
    let saved = fs::read(&path).unwrap();
    let updated: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(
        updated["pane_tree"]["panes"][pane.to_string()]["cwd"],
        normalized
    );
    assert_eq!(updated["legacyUnknown"], document["legacyUnknown"]);
    assert_eq!(
        updated["pane_tree"]["panes"][pane.to_string()]["legacyPaneField"],
        "keep"
    );
    assert_eq!(
        client::list_domain_ptys(&owned.endpoint)
            .unwrap()
            .into_iter()
            .find(|pty| pty.pty_id == pane)
            .unwrap()
            .cwd
            .as_deref(),
        Some(normalized.as_str())
    );

    // Replace only this owned fixture's graph path with a directory to force a commit error.
    let graph_path = root.join("workspace-graph.json");
    let held = root.join("original-graph.json");
    fs::rename(&graph_path, &held).unwrap();
    fs::create_dir(&graph_path).unwrap();
    change(&failed, "failed-ready");
    let deadline = Instant::now() + Duration::from_secs(8);
    while !root.join("failed-ready").exists() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let rejected = rpc(&mut socket, 3, "get_pane_cwd", params.clone()).await;
    assert!(!rejected["error"].is_null());
    assert_eq!(
        fs::read(&path).unwrap(),
        saved,
        "associated file must roll back after graph commit failure"
    );
    assert_eq!(
        snapshot_cwd(),
        normalized,
        "failed commits must not publish new terminal metadata"
    );
    assert_eq!(
        client::list_domain_ptys(&owned.endpoint)
            .unwrap()
            .into_iter()
            .find(|pty| pty.pty_id == pane)
            .unwrap()
            .cwd
            .as_deref(),
        Some(normalized.as_str())
    );
    fs::remove_dir(&graph_path).unwrap();
    fs::rename(&held, &graph_path).unwrap();
    assert!(rpc(&mut socket, 4, "get_pane_cwd", params).await["error"].is_null());
    let committed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        std::path::Path::new(
            committed["pane_tree"]["panes"][pane.to_string()]["cwd"]
                .as_str()
                .unwrap()
        ),
        failed.as_path()
    );
    let archives: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("saved.ridge.archive-")
        })
        .collect();
    assert!(!archives.is_empty());
    socket.close(None).await.unwrap();
    assert!(client::health_ok(&owned.endpoint));
}
