//! Exercise the authenticated client and HTTP bridge, not a mocked dispatcher.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use serde_json::json;
use std::fs;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_directory_expansion_keeps_the_initial_tree_permission_context() {
    let root = std::env::temp_dir().join(format!("ridge-native-files-{}", Uuid::new_v4()));
    let folder = root.join("nested folder");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("hello.txt"), "hello from native scope").unwrap();
    let owned = start(&root).await;
    let mut socket = attach(&owned.endpoint).await;
    // No PTY cwd points here. Native access is authorized by the operator root.
    let tree = rpc(&mut socket, 1, "get_file_tree", json!({"path":root})).await;
    assert!(tree["error"].is_null(), "{tree}");
    let page = rpc(&mut socket, 2, "get_directory_children", json!({"path":folder})).await;
    assert!(page["error"].is_null(), "{page}");
    assert_eq!(page["result"]["entries"][0]["name"], "hello.txt");
    let opened = rpc(&mut socket, 3, "read_file_for_editor", json!({"path":folder.join("hello.txt")})).await;
    assert_eq!(opened["result"]["content"], "hello from native scope", "{opened}");
    let denied = rpc(&mut socket, 4, "get_directory_children", json!({"path":root.parent().unwrap()})).await;
    assert!(!denied["error"].is_null(), "operator root must still be enforced");
    let missing = rpc(&mut socket, 5, "get_directory_children", json!({"path":root.join("missing")})).await;
    assert!(!missing["error"].is_null(), "missing directory is not a successful empty page");
    let malformed = rpc(&mut socket, 6, "text_search", json!([])).await;
    assert_eq!(malformed["error"]["code"], -32602);
    let alias = rpc(&mut socket, 7, "list-files", json!({"path":folder})).await;
    assert_eq!(alias["result"]["entries"][0]["name"], "hello.txt", "connection survives rejected params");
    let search = rpc(&mut socket, 8, "search_files", json!({"path":folder,"query":"hello"})).await;
    assert!(search["error"].is_null(), "{search}");
    assert!(!search["result"].as_array().unwrap().is_empty());
    let remote = client::request_json(&owned.endpoint, "POST", "/v1/domain/commands",
        Some(&json!({"method":"get_directory_children","args":{"path":root},"native":false,"roots":[folder],"connectionId":Uuid::new_v4()}))).unwrap();
    assert_eq!(remote["ok"], false, "native fix must not bypass remote serving roots");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn editor_reads_survive_http_json_expansion_but_reject_oversized_files() {
    let root = std::env::temp_dir().join(format!("ridge-editor-bound-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let path = root.join("large.txt");
    let read = || client::request_json(&owned.endpoint, "POST", "/v1/domain/commands",
        Some(&json!({"method":"read_file_for_editor","args":{"path":path},"native":true,"roots":[],"connectionId":Uuid::new_v4()})));
    for content in ["a".repeat(3 * 1024 * 1024), format!("{}{}", "a".repeat(8192), "\u{0001}".repeat(1024 * 1024))] {
        fs::write(&path, &content).unwrap();
        let response = read().expect("permitted editor content must cross the HTTP bridge");
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["value"]["content"].as_str(), Some(content.as_str()));
    }
    fs::write(&path, vec![b'a'; 5 * 1024 * 1024]).unwrap();
    let mut socket = attach(&owned.endpoint).await;
    let full = rpc(&mut socket, 1, "read_file_for_editor", json!({"path":path})).await;
    assert_eq!(full["result"]["content"].as_str().map(str::len), Some(5 * 1024 * 1024));
    fs::write(&path, vec![b'a'; 5 * 1024 * 1024 + 1]).unwrap();
    assert_eq!(read().unwrap()["ok"], false);
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn powershell_split_inherits_live_cwd_and_keeps_named_topology_current() {
    use serde_json::Value;
    use std::{path::Path, time::{Duration, Instant}};
    async fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready() {
            assert!(Instant::now() < deadline, "real shell / autosave deadline");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
    let root = std::env::temp_dir().join(format!("ridge-split-cwd-{}", Uuid::new_v4()));
    let nested = root.join("nested cwd");
    let moved = nested.join("child cwd");
    fs::create_dir_all(&moved).unwrap();
    let owned = start(&root).await;
    let endpoint = &owned.endpoint;
    let workspace = client::request_json(endpoint, "POST", "/v1/domain/workspaces", None).unwrap()["workspace_id"].clone();
    let detail = client::request_json(endpoint, "GET", &format!("/v1/domain/workspaces/{}", workspace.as_str().unwrap()), None).unwrap();
    let pane = detail["panes"][0].clone();
    let mut socket = attach(endpoint).await;
    let created = rpc(&mut socket, 1, "create_pane", json!({"workspaceId":workspace,"paneId":pane,"shell":"powershell.exe","cwd":root})).await;
    assert!(created["error"].is_null(), "{created}");
    let write = |pane: &Value, command: String| {
        client::write_domain_pty(endpoint, Uuid::parse_str(pane.as_str().unwrap()).unwrap(), command.as_bytes()).unwrap();
    };
    let marker = root.join("parent-cwd.txt");
    write(&pane, format!("Set-Location -LiteralPath '{}'; (Get-Location).Path | Set-Content -LiteralPath '{}'\r\n", nested.display(), marker.display()));
    wait_until(|| marker.exists()).await;
    assert_eq!(Path::new(fs::read_to_string(&marker).unwrap().trim_start_matches('\u{feff}').trim()), nested);
    // Wait for the actual prompt's OSC 7 observation, not a manually seeded graph.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cwd = rpc(&mut socket, 2, "get_pane_cwd", json!({"workspaceId":workspace,"paneId":pane})).await;
        assert!(cwd["error"].is_null(), "{cwd}");
        if cwd["result"].as_str().is_some_and(|cwd| Path::new(cwd) == nested) { break; }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let saved_path = root.join("named.ridge");
    let saved = rpc(&mut socket, 3, "save_workspace_to_file", json!({"workspaceId":workspace,"name":"Named continuity","path":saved_path})).await;
    assert!(saved["error"].is_null(), "{saved}");
    let mut old: Value = serde_json::from_slice(&fs::read(&saved_path).unwrap()).unwrap();
    old["legacyUnknown"] = json!({"keep":true});
    old["pane_tree"]["panes"][pane.as_str().unwrap()]["legacyPaneField"] = json!("keep");
    fs::write(&saved_path, serde_json::to_vec(&old).unwrap()).unwrap();
    let split = rpc(&mut socket, 4, "split_pane", json!({"workspaceId":workspace,"paneId":pane,"direction":"horizontal","shell":"powershell.exe"})).await;
    assert!(split["error"].is_null(), "{split}");
    assert_eq!(Path::new(split["result"]["initial_cwd"].as_str().unwrap()), nested);
    let child = split["result"]["pane_id"].clone();
    let child_marker = root.join("child-inherited.txt");
    write(&child, format!("(Get-Location).Path | Set-Content -LiteralPath '{}'\r\n", child_marker.display()));
    wait_until(|| child_marker.exists()).await;
    assert_eq!(Path::new(fs::read_to_string(&child_marker).unwrap().trim_start_matches('\u{feff}').trim()), nested);
    let file_panes = || -> Value { serde_json::from_slice::<Value>(&fs::read(&saved_path).unwrap()).unwrap()["pane_tree"]["panes"].clone() };
    wait_until(|| file_panes().as_object().unwrap().len() == 2).await;
    // Reproduce the file snapshot that is still pending during debounce.
    fs::write(&saved_path, serde_json::to_vec(&old).unwrap()).unwrap();
    let moved_marker = root.join("child-moved.txt");
    write(&child, format!("Set-Location -LiteralPath '{}'; (Get-Location).Path | Set-Content -LiteralPath '{}'\r\n", moved.display(), moved_marker.display()));
    wait_until(|| moved_marker.exists()).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cwd = rpc(&mut socket, 5, "get_pane_cwd", json!({"workspaceId":workspace,"paneId":child})).await;
        assert!(cwd["error"].is_null(), "stale saved topology must not reject a live child: {cwd}");
        if cwd["result"].as_str().is_some_and(|cwd| Path::new(cwd) == moved) { break; }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let repeated = rpc(&mut socket, 6, "split_pane", json!({"workspaceId":workspace,"paneId":child,"direction":"vertical","shell":"powershell.exe"})).await;
    assert!(repeated["error"].is_null(), "{repeated}");
    assert_eq!(Path::new(repeated["result"]["initial_cwd"].as_str().unwrap()), moved);
    wait_until(|| file_panes().as_object().unwrap().len() == 3).await;
    let document: Value = serde_json::from_slice(&fs::read(&saved_path).unwrap()).unwrap();
    assert_eq!(document["legacyUnknown"], old["legacyUnknown"]);
    assert_eq!(document["pane_tree"]["panes"][pane.as_str().unwrap()]["legacyPaneField"], "keep");
    let closed = rpc(&mut socket, 7, "close_pane", json!({"workspaceId":workspace,"paneId":repeated["result"]["pane_id"]})).await;
    assert!(closed["error"].is_null(), "{closed}");
    wait_until(|| file_panes().as_object().unwrap().len() == 2).await;
    let mcp = client::request_json(endpoint, "POST", "/api/v1/mcp", Some(&json!({
        "jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"ridge_split_pane",
            "arguments":{"workspace_id":workspace,"direction":"horizontal","role":"shell"}}
    }))).unwrap();
    assert!(mcp["error"].is_null(), "{mcp}");
    assert_ne!(mcp["result"]["isError"], true, "{mcp}");
    let mcp_result: Value = serde_json::from_str(mcp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let mcp_pane = Uuid::parse_str(mcp_result["paneId"].as_str().unwrap()).unwrap();
    let process = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty|pty.pty_id == mcp_pane).unwrap();
    assert_eq!(Path::new(process.cwd.as_deref().unwrap()), moved, "MCP uses the same live cwd contract");
    assert_eq!(Path::new(&ridge_core::commands::process::get_process_cwd(process.child_pid.unwrap()).unwrap()), moved);
    wait_until(|| file_panes().as_object().unwrap().len() == 3).await;
    assert_eq!(process.launch_profile.as_deref(), Some("ridge-interactive"));
    let mcp_marker = root.join("mcp-moved.txt");
    write(&mcp_result["paneId"], format!("Set-Location -LiteralPath '{}'; (Get-Location).Path | Set-Content -LiteralPath '{}'\r\n", nested.display(), mcp_marker.display()));
    wait_until(|| mcp_marker.exists()).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cwd = rpc(&mut socket, 8, "get_pane_cwd", json!({"workspaceId":workspace,"paneId":mcp_result["paneId"]})).await;
        assert!(cwd["error"].is_null(), "{cwd}");
        if cwd["result"].as_str().is_some_and(|cwd| Path::new(cwd) == nested) { break; }
        assert!(Instant::now() < deadline, "MCP shell must keep producing cwd observations after cd");
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let after_cd = rpc(&mut socket, 9, "split_pane", json!({"workspaceId":workspace,"paneId":mcp_result["paneId"],"direction":"horizontal","shell":"powershell.exe"})).await;
    assert!(after_cd["error"].is_null(), "{after_cd}");
    assert_eq!(Path::new(after_cd["result"]["initial_cwd"].as_str().unwrap()), nested);
}
