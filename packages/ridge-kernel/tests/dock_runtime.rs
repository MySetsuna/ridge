#[path = "common/runtime_fixture.rs"]
mod fixture;
use base64::Engine;
use fixture::{attach, rpc, start};
use futures::{SinkExt, StreamExt};
use ridge_kernel::{client, registry::KernelEndpoint};
use ridge_term::terminal_protocol::{decode_frame, TerminalBody};
use serde_json::{json, Value};
use std::{fs, time::{Duration, Instant}};
use uuid::Uuid;

fn request(endpoint: &KernelEndpoint, method: &str, path: &str, args: Value) -> Value {
    client::request_json(endpoint, method, path, Some(&args)).unwrap()
}
fn workspace(endpoint: &KernelEndpoint) -> (Uuid, Uuid) {
    let created = request(endpoint, "POST", "/v1/domain/workspaces", json!({}));
    let workspace = Uuid::parse_str(created["workspace_id"].as_str().unwrap()).unwrap();
    let detail = client::request_json(endpoint, "GET", &format!("/v1/domain/workspaces/{workspace}"), None).unwrap();
    (workspace, Uuid::parse_str(detail["panes"][0].as_str().unwrap()).unwrap())
}
fn packet(reply: &Value) -> ridge_term::terminal_protocol::TerminalFrame {
    assert!(reply["error"].is_null(), "{reply}");
    decode_frame(&base64::engine::general_purpose::STANDARD.decode(reply["result"].as_str().unwrap()).unwrap()).unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dock_preserves_live_child_history_and_fences_old_workspace_without_stealing_other_selection() {
    let root = std::env::temp_dir().join(format!("ridge-dock-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let endpoint = &owned.endpoint;
    let (from, a) = workspace(endpoint);
    let (to, c) = workspace(endpoint);
    let (third, _) = workspace(endpoint);
    let split = request(endpoint, "POST", &format!("/v1/domain/workspaces/{from}/split"), json!({"pane_id":a,"direction":"horizontal"}));
    let b = Uuid::parse_str(split["pane_id"].as_str().unwrap()).unwrap();
    for (workspace, pane) in [(from, a), (from, b), (to, c)] {
        let (program, args) = if cfg!(windows) { ("cmd.exe", vec!["/d", "/q", "/k"]) } else { ("/bin/sh", vec![]) };
        assert_eq!(request(endpoint, "POST", "/v1/domain/ptys", json!({"pty_id":pane,"workspace_id":workspace,"program":program,"args":args,"cwd":root,"cols":80,"rows":24}))["ok"], true);
    }
    let originals = client::list_domain_ptys(endpoint).unwrap();
    let instances: Vec<_> = originals.iter().map(|pty| ridge_core::process_identity::instance(pty.child_pid.unwrap()).unwrap()).collect();
    let mut first = attach(endpoint).await;
    let mut second = attach(endpoint).await;
    assert!(rpc(&mut first, 1, "switch_workspace", json!({"workspaceId":from})).await["error"].is_null());
    assert!(rpc(&mut second, 1, "switch_workspace", json!({"workspaceId":third})).await["error"].is_null());
    request(endpoint, "POST", &format!("/v1/domain/workspaces/{third}/activate"), json!({}));
    assert!(rpc(&mut first, 2, "dock_pane", json!({"sourcePaneId":a,"targetPaneId":b,"region":"center"})).await["error"].is_null());
    // An unpublished graph must not retarget the live PTY or change its layout.
    let graph = root.join("workspace-graph.json");
    let backup = root.join("workspace-graph.before-dock.json");
    fs::rename(&graph, &backup).unwrap(); fs::create_dir(&graph).unwrap();
    let failed = rpc(&mut first, 3, "dock_pane", json!({"sourcePaneId":a,"targetPaneId":c,"region":"right"})).await;
    assert!(!failed["error"].is_null(), "{failed}");
    assert_eq!(client::list_domain_ptys(endpoint).unwrap().iter().find(|pty| pty.pty_id == a).unwrap().workspace_id, Some(from));
    fs::remove_dir(&graph).unwrap(); fs::rename(&backup, &graph).unwrap();
    let path = root.join("target.ridge");
    assert!(rpc(&mut first, 4, "save_workspace_to_file", json!({"workspaceId":to,"path":path,"name":"target"})).await["error"].is_null());
    let restricted = root.join("restricted"); fs::create_dir(&restricted).unwrap();
    let denied = request(endpoint, "POST", "/v1/domain/commands", json!({"method":"dock_pane","args":{"sourcePaneId":a,"targetPaneId":c,"region":"right"},"native":true,"roots":[restricted],"connectionId":Uuid::new_v4()}));
    assert_eq!(denied["ok"], false, "a client cannot move a pane whose associated file exceeds its roots: {denied}");
    assert_eq!(client::list_domain_ptys(endpoint).unwrap().iter().find(|pty| pty.pty_id == a).unwrap().workspace_id, Some(from));
    let mut old_stream = client::connect_terminal_stream(endpoint.clone(), from, a, 11).await.unwrap();
    let initial = loop { if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = old_stream.next().await.unwrap().unwrap() { break decode_frame(&bytes).unwrap(); } };
    first.send(tokio_tungstenite::tungstenite::Message::Text(json!({"type":"subscribe-pane","workspaceId":from,"paneId":a,"activationId":11,"active":true}).to_string())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5),async{loop{if let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(_)))=first.next().await{break;}}}).await.unwrap();
    let output = (0..120).map(|index| format!("echo DOCK_HISTORY_{index}\r\n")).collect::<String>();
    let input = |workspace, sequence, bytes: &[u8]| json!({"input":{"pane":{"workspaceId":workspace,"paneId":a},"terminalEpoch":initial.terminal_epoch,"activationId":11,"sourceId":"dock-input","sequence":sequence,"bytes":bytes}});
    let original_input = input(from, 1, output.as_bytes());
    let ack = rpc(&mut first, 5, "write_terminal_input", original_input.clone()).await;
    assert_eq!(ack["result"]["alreadyApplied"], false, "{ack}");
    assert_eq!(rpc(&mut first, 6, "write_terminal_input", original_input).await["result"]["alreadyApplied"], true);
    let deadline = Instant::now() + Duration::from_secs(8);
    let history = loop {
        let frame = packet(&rpc(&mut first, 7, "get_terminal_snapshot", json!({"workspaceId":from,"paneId":a,"activationId":11})).await);
        if let TerminalBody::Snapshot(snapshot) = frame.body { if snapshot.history.end > 100 { break snapshot.history; } }
        assert!(Instant::now() < deadline, "child output did not reach history");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(rpc(&mut first, 8, "dock_pane", json!({"sourcePaneId":a,"targetPaneId":c,"region":"right"})).await["error"].is_null());
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(Ok(message)) = old_stream.next().await { if message.is_close() { break; } }
    }).await.expect("old workspace stream must close");
    assert!(client::connect_terminal_stream(endpoint.clone(), from, a, 12).await.is_err());
    let mut new_stream = client::connect_terminal_stream(endpoint.clone(), to, a, 12).await.unwrap();
    let rebound = loop { if let tokio_tungstenite::tungstenite::Message::Binary(bytes) = new_stream.next().await.unwrap().unwrap() { break decode_frame(&bytes).unwrap(); } };
    assert_eq!(rebound.terminal_epoch, initial.terminal_epoch);
    assert_eq!(rebound.pane.workspace_id, to.to_string());
    assert!(matches!(rebound.body, TerminalBody::Snapshot(ref snapshot) if snapshot.history.epoch == history.epoch));
    assert!(!rpc(&mut first, 9, "get_terminal_snapshot", json!({"workspaceId":from,"paneId":a,"activationId":11})).await["error"].is_null());
    let marker = root.join("after-dock.txt");
    let data = format!("echo AFTER_DOCK > \"{}\"\r\n", marker.to_string_lossy().replace('\\', "/"));
    assert!(!rpc(&mut first, 10, "write_terminal_input", input(from, 2, data.as_bytes())).await["error"].is_null());
    assert!(!rpc(&mut first, 11, "write_to_pty", json!({"workspaceId":from,"paneId":a,"data":data})).await["error"].is_null());
    assert!(!rpc(&mut first, 12, "resize_pane", json!({"workspaceId":from,"paneId":a,"rows":33,"cols":91})).await["error"].is_null());
    assert!(!marker.exists());
    first.send(tokio_tungstenite::tungstenite::Message::Text(json!({"type":"subscribe-pane","workspaceId":to,"paneId":a,"activationId":11,"active":true}).to_string())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5),async{loop{if let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes)))=first.next().await{if decode_frame(&bytes).is_ok_and(|frame| frame.pane.workspace_id==to.to_string()){break;}}}}).await.unwrap();
    assert_eq!(rpc(&mut first, 13, "write_terminal_input", input(to, 2, data.as_bytes())).await["result"]["alreadyApplied"], false);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fs::read(&marker).is_ok_and(|bytes| !bytes.is_empty()) { assert!(Instant::now() < deadline); tokio::time::sleep(Duration::from_millis(20)).await; }
    let history_request = |workspace| json!({"request":{"pane":{"workspaceId":workspace,"paneId":a},"terminalEpoch":initial.terminal_epoch,"activationId":11,"historyEpoch":history.epoch,"before":history.end,"maxRows":64}});
    assert!(!rpc(&mut first, 14, "get_terminal_history", history_request(from)).await["error"].is_null());
    assert!(matches!(packet(&rpc(&mut first, 15, "get_terminal_history", history_request(to)).await).body, TerminalBody::HistoryPage(ref page) if !page.lines.is_empty() && page.lines.len() <= 64));
    assert!(rpc(&mut first, 16, "dock_pane", json!({"sourcePaneId":b,"targetPaneId":c,"region":"bottom"})).await["error"].is_null());
    assert_eq!(rpc(&mut first, 17, "get_active_workspace_id", json!({})).await["result"], to.to_string());
    assert_eq!(rpc(&mut second, 2, "get_active_workspace_id", json!({})).await["result"], third.to_string());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if ridge_core::workspace::file::read(&path).is_ok_and(|file| ridge_core::workspace::file::validate(&file).is_ok_and(|tree| tree.panes.contains_key(&a) && tree.panes.contains_key(&b))) { break; }
        assert!(Instant::now() < deadline, "associated file did not autosave moved leaves"); tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let final_graph: ridge_core::workspace::graph::WorkspaceGraph = serde_json::from_slice(&fs::read(&graph).unwrap()).unwrap();
    assert!(final_graph.workspace(from).is_none());
    assert_eq!(final_graph.leaves(to).unwrap().len(), 3);
    assert!(fs::read_dir(&root).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().starts_with("workspace-graph.json.archive-") && fs::read(entry.path()).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(&from.to_string()))));
    for original in originals {
        let live = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty| pty.pty_id == original.pty_id).unwrap();
        assert_eq!((live.child_pid, live.child_start_instance), (original.child_pid, original.child_start_instance));
        assert_eq!(live.workspace_id, Some(to));
    }
    for instance in instances { assert!(ridge_core::process_identity::validate(&instance).is_ok()); }
}
