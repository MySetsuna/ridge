//! Real local presentation grant/RPC/terminal isolation on an owned Kernel.
use base64::Engine;
use futures::{SinkExt, StreamExt};
use ridge_kernel::{client, local_client::ClientGrant, registry::KernelEndpoint};
use ridge_term::terminal_protocol::{self, TerminalBody};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

async fn rpc(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string(),
        ))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = frame {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["id"] == id {
                assert!(
                    value.get("error").is_none(),
                    "RPC {method} failed: {}",
                    value["error"]["message"]
                );
                return value["result"].clone();
            }
        }
        assert!(Instant::now() < deadline, "scoped RPC did not answer");
    }
}

struct OwnedKernel {
    endpoint: KernelEndpoint,
}
impl Drop for OwnedKernel {
    fn drop(&mut self) {
        if client::health_ok(&self.endpoint) {
            let _ = client::shutdown_endpoint(&self.endpoint, Duration::from_secs(2));
        }
    }
}

fn request(
    grant: &ClientGrant,
    device: &str,
    origin: &str,
) -> tokio_tungstenite::tungstenite::http::Request<()> {
    let mut request = format!("{}?deviceId={device}", grant.ws_url)
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("ridge-client, auth-{}", grant.token)
            .parse()
            .unwrap(),
    );
    request
}

#[tokio::test]
async fn scoped_local_client_cannot_become_kernel_or_lan_authority() {
    let root =
        std::env::temp_dir().join(format!("ridge-070-local-client-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut process = Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR", &root)
        .env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let endpoint = loop {
        assert!(process.try_wait().unwrap().is_none(), "owned Kernel exited");
        if let Ok(bytes) = fs::read(root.join("kernel.json")) {
            if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                if client::health_ok(&endpoint) {
                    break endpoint;
                }
            }
        }
        assert!(Instant::now() < deadline, "owned Kernel readiness deadline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let _cleanup = OwnedKernel {
        endpoint: endpoint.clone(),
    };
    assert_eq!(endpoint.pid, process.id());
    let payload = json!({"deviceId":"owned-window","origin":"http://tauri.localhost"});
    let mut unauthenticated = endpoint.clone();
    unauthenticated.token = "invalid".into();
    assert!(client::request_json(
        &unauthenticated,
        "POST",
        "/v1/local/clients",
        Some(&payload)
    )
    .is_err());
    assert!(client::request_json(
        &endpoint,
        "POST",
        "/v1/local/clients",
        Some(&json!({"deviceId":"owned-window","origin":"https://evil.test"}))
    )
    .is_err());
    let grant: ClientGrant = serde_json::from_value(
        client::request_json(&endpoint, "POST", "/v1/local/clients", Some(&payload)).unwrap(),
    )
    .unwrap();
    assert!(grant.token != endpoint.token);
    assert_eq!(grant.token.len(), 64);
    assert!(!grant.ws_url.contains(&grant.token));
    assert_eq!(grant.protocol_version, client::KERNEL_PROTOCOL_VERSION);
    let mut limited = endpoint.clone();
    limited.token = grant.token.clone();
    assert!(client::request_json(&limited, "GET", "/v1/status", None).is_err());
    assert!(tokio_tungstenite::connect_async(request(
        &grant,
        "other-window",
        "http://tauri.localhost"
    ))
    .await
    .is_err());
    assert!(tokio_tungstenite::connect_async(request(
        &grant,
        "owned-window",
        "http://localhost:9876"
    ))
    .await
    .is_err());
    let workspace = client::request_json(
        &endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"local-owned-test"})),
    )
    .unwrap();
    let workspace_id = uuid::Uuid::parse_str(workspace["workspace_id"].as_str().unwrap()).unwrap();
    let pane_id = uuid::Uuid::new_v4();
    client::create_domain_pty(
        &endpoint,
        pane_id,
        Some(if cfg!(windows) { "cmd.exe" } else { "/bin/sh" }),
        None,
        Some(workspace_id),
        "shell",
        Some("local-owned-test"),
    )
    .unwrap();
    let original = client::list_domain_ptys(&endpoint)
        .unwrap()
        .into_iter()
        .find(|p| p.id == pane_id)
        .unwrap();
    let (mut socket, response) =
        tokio_tungstenite::connect_async(request(&grant, "owned-window", "http://tauri.localhost"))
            .await
            .unwrap();
    assert_eq!(
        response.headers().get("sec-websocket-protocol").unwrap(),
        "ridge-client"
    );
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":1,"method":"get_theme_data","params":{}}).to_string(),
        ))
        .await
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = frame {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["id"] == 1 {
                assert!(value.get("error").is_none());
                assert_eq!(value["result"]["version"], 1);
                break;
            }
        }
        assert!(Instant::now() < deadline, "shared Host RPC did not answer");
    }
    socket.send(Message::Text(json!({"jsonrpc":"2.0","method":"subscribe-pane","params":{"workspaceId":workspace_id,"paneId":pane_id,"activationId":9}}).to_string())).await.unwrap();
    let terminal_epoch = loop {
        let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Binary(bytes) = frame {
            let frame = terminal_protocol::decode_frame(&bytes).unwrap();
            assert_eq!(frame.pane.pane_id, pane_id.to_string());
            assert_eq!(frame.activation_id, 9);
            break frame.terminal_epoch;
        }
    };
    let marker = format!("LOCAL_{}", uuid::Uuid::new_v4());
    let input = json!({"pane":{"workspaceId":workspace_id,"paneId":pane_id},"terminalEpoch":terminal_epoch,"activationId":9,"sourceId":"local-test","sequence":1,"bytes":format!("echo {marker}\r").into_bytes()});
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":2,"method":"write_terminal_input","params":{"input":input}}).to_string())).await.unwrap();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = frame {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["id"] == 2 {
                assert!(value.get("error").is_none());
                assert_eq!(value["result"]["sequence"], 1);
                break;
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let value=client::request_json(&endpoint,"POST",&format!("/v1/domain/ptys/{pane_id}/terminal"),Some(&json!({"method":"get_terminal_snapshot","params":{"workspaceId":workspace_id,"paneId":pane_id,"activationId":10}}))).unwrap();
        assert_eq!(value["ok"], true);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(value["value"].as_str().unwrap())
            .unwrap();
        if let TerminalBody::Snapshot(snapshot) =
            terminal_protocol::decode_frame(&bytes).unwrap().body
        {
            if snapshot
                .primary
                .lines
                .iter()
                .any(|line| line.cells.iter().map(|c| c.ch).collect::<String>().trim() == marker)
            {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "local client input did not execute"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let second_workspace = client::request_json(
        &endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"second-local-window"})),
    )
    .unwrap();
    let file = root.join("client-file.txt");
    rpc(&mut socket, 101, "create_file", json!({"path":file})).await;
    rpc(
        &mut socket,
        102,
        "write_file",
        json!({"path":file,"content":"shared runtime file content"}),
    )
    .await;
    assert_eq!(
        rpc(&mut socket, 103, "read_file", json!({"path":file})).await,
        "shared runtime file content"
    );
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "shared runtime file content"
    );
    let initialized = ridge_core::commands::git::run_git_guarded(
        root.to_string_lossy().into(),
        vec!["init".into(), "--quiet".into()],
    )
    .await
    .unwrap();
    assert!(initialized.status.success());
    let info = rpc(
        &mut socket,
        104,
        "get_git_info_with_cwd",
        json!({"cwd":root}),
    )
    .await;
    assert_eq!(info["is_git_repo"], true);
    let status = rpc(&mut socket, 105, "get_scm_status", json!({"repoRoot":root})).await;
    assert!(status.is_object());
    for (key, value) in [
        ("user.name", "070 fixture"),
        ("user.email", "070-fixture@example.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        let configured = ridge_core::commands::git::run_git_guarded(
            root.to_string_lossy().into(),
            vec!["config".into(), "--local".into(), key.into(), value.into()],
        )
        .await
        .unwrap();
        assert!(configured.status.success());
    }
    rpc(
        &mut socket,
        106,
        "git_stage",
        json!({"repoRoot":root,"paths":["client-file.txt"]}),
    )
    .await;
    let staged = ridge_core::commands::git::run_git_guarded(
        root.to_string_lossy().into(),
        vec!["diff".into(), "--cached".into(), "--name-only".into()],
    )
    .await
    .unwrap();
    assert_eq!(
        String::from_utf8(staged.stdout).unwrap().trim(),
        "client-file.txt"
    );
    rpc(
        &mut socket,
        107,
        "git_commit",
        json!({"repoRoot":root,"message":"owned RPC commit"}),
    )
    .await;
    let committed = ridge_core::commands::git::run_git_guarded(
        root.to_string_lossy().into(),
        vec!["show".into(), "HEAD:client-file.txt".into()],
    )
    .await
    .unwrap();
    assert!(committed.status.success());
    assert_eq!(
        String::from_utf8(committed.stdout).unwrap(),
        "shared runtime file content"
    );
    let tree = json!({"root":{"Leaf":pane_id},"panes":{pane_id.to_string():{"id":pane_id,"mode":"Terminal"}}});
    client::request_json(
        &endpoint,
        "PUT",
        &format!("/v1/domain/workspaces/{workspace_id}/topology"),
        Some(&json!({"pane_tree":tree})),
    )
    .unwrap();
    let workspace_file = root.join("owned-workspace.ridge");
    assert_eq!(
        rpc(
            &mut socket,
            108,
            "save_workspace_to_file",
            json!({"workspaceId":workspace_id,"name":"Owned workspace","path":workspace_file})
        )
        .await,
        workspace_file.to_string_lossy().as_ref()
    );
    let original_file = fs::read(&workspace_file).unwrap();
    let payload = ridge_core::workspace::file::read(&workspace_file).unwrap();
    assert_eq!(payload.name, "Owned workspace");
    assert!(ridge_core::workspace::file::validate(&payload)
        .unwrap()
        .panes
        .contains_key(&pane_id));
    rpc(
        &mut socket,
        109,
        "save_workspace_to_file",
        json!({"workspaceId":workspace_id,"name":"Updated workspace","path":workspace_file}),
    )
    .await;
    let backup = fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(".ridge.archive-"))
        })
        .unwrap();
    assert_eq!(fs::read(backup).unwrap(), original_file);
    let info = rpc(
        &mut socket,
        110,
        "get_workspace_save_info",
        json!({"workspaceId":workspace_id}),
    )
    .await;
    assert_eq!(info["file_path"], workspace_file.to_string_lossy().as_ref());
    assert_eq!(info["name"], "Updated workspace");
    let state_path = root.join("workspace-graph.json");
    let state_backup = root.join("workspace-graph.json.fixture-archive");
    let prior_snapshot = fs::read(&workspace_file).unwrap();
    fs::rename(&state_path, &state_backup).unwrap();
    fs::create_dir(&state_path).unwrap();
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":114,"method":"save_workspace_to_file","params":{"workspaceId":workspace_id,"name":"Must roll back","path":workspace_file}}).to_string())).await.unwrap();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(3), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(text) = frame {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["id"] == 114 {
                assert!(value["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("previous snapshot restored"));
                break;
            }
        }
    }
    assert_eq!(fs::read(&workspace_file).unwrap(), prior_snapshot);
    fs::remove_dir(&state_path).unwrap();
    fs::rename(&state_backup, &state_path).unwrap();
    let info = rpc(
        &mut socket,
        115,
        "get_workspace_save_info",
        json!({"workspaceId":workspace_id}),
    )
    .await;
    assert_eq!(info["name"], "Updated workspace");
    assert_eq!(
        rpc(
            &mut socket,
            111,
            "open_workspace_from_file",
            json!({"path":workspace_file})
        )
        .await,
        workspace_id.to_string()
    );
    let fresh_tree = ridge_core::workspace::pane_tree::PaneTree::default();
    let imported_file = root.join("imported.ridge");
    ridge_core::workspace::file::write(
        &imported_file,
        &ridge_core::workspace::file::RidgeFile {
            version: 1,
            name: "Legacy import".into(),
            saved_at: "2026-10-11T00:00:00Z".into(),
            pane_tree: serde_json::to_value(&fresh_tree).unwrap(),
            git_repos: vec![],
            index_path: None,
            pane_titles: std::collections::HashMap::new(),
        },
    )
    .unwrap();
    let count = client::list_domain_ptys(&endpoint).unwrap().len();
    let imported = rpc(
        &mut socket,
        112,
        "open_workspace_from_file",
        json!({"path":imported_file}),
    )
    .await;
    assert!(imported.as_str().is_some());
    assert_eq!(
        client::list_domain_ptys(&endpoint).unwrap().len(),
        count,
        "import must not launch processes"
    );
    rpc(
        &mut socket,
        113,
        "switch_workspace",
        json!({"workspaceId":workspace_id}),
    )
    .await;
    let second_id = second_workspace["workspace_id"].clone();
    let global_before = client::request_json(&endpoint, "GET", "/v1/domain/workspaces", None)
        .unwrap()["active"]
        .clone();
    let second_grant: ClientGrant = serde_json::from_value(
        client::request_json(
            &endpoint,
            "POST",
            "/v1/local/clients",
            Some(&json!({"deviceId":"second-window","origin":"http://tauri.localhost"})),
        )
        .unwrap(),
    )
    .unwrap();
    let (mut other, _) = tokio_tungstenite::connect_async(request(
        &second_grant,
        "second-window",
        "http://tauri.localhost",
    ))
    .await
    .unwrap();
    rpc(
        &mut other,
        3,
        "switch_workspace",
        json!({"workspaceId":second_id}),
    )
    .await;
    assert_eq!(
        rpc(&mut socket, 4, "get_active_workspace_id", json!({})).await,
        workspace_id.to_string()
    );
    assert_eq!(
        rpc(&mut other, 5, "get_active_workspace_id", json!({})).await,
        second_id
    );
    assert_eq!(
        client::request_json(&endpoint, "GET", "/v1/domain/workspaces", None).unwrap()["active"],
        global_before
    );
    rpc(
        &mut socket,
        6,
        "switch_workspace",
        json!({"workspaceId":second_id}),
    )
    .await;
    rpc(
        &mut other,
        7,
        "switch_workspace",
        json!({"workspaceId":workspace_id}),
    )
    .await;
    assert_eq!(
        rpc(&mut socket, 8, "get_active_workspace_id", json!({})).await,
        second_id
    );
    socket.close(None).await.unwrap();
    let (mut reconnected, _) =
        tokio_tungstenite::connect_async(request(&grant, "owned-window", "http://tauri.localhost"))
            .await
            .unwrap();
    assert_eq!(
        rpc(&mut reconnected, 9, "get_active_workspace_id", json!({})).await,
        second_id
    );
    assert_eq!(
        rpc(&mut other, 10, "get_active_workspace_id", json!({})).await,
        workspace_id.to_string()
    );
    reconnected.close(None).await.unwrap();
    other.close(None).await.unwrap();
    let remaining = client::list_domain_ptys(&endpoint)
        .unwrap()
        .into_iter()
        .find(|p| p.id == pane_id)
        .unwrap();
    assert_eq!(remaining.child_pid, original.child_pid);
    assert!(client::health_ok(&endpoint));
}
