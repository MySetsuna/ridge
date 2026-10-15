#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use serde_json::json;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_lists_share_committed_topology_across_presentations_and_restart() {
    let root = std::env::temp_dir().join(format!("ridge-host-list-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let mut first = attach(&owned.endpoint).await;
    let mut second = attach(&owned.endpoint).await;
    assert_eq!(rpc(&mut first, 1, "host_list_snapshot", json!({})).await["result"], json!([]));
    let host = json!({"id":"peer", "kind":"rdg", "label":"owned peer", "addr":"127.0.0.1:7522",
        "status":"disconnected", "detail":"registered", "sessions":[{"id":"session-a", "title":"owned session", "attached":false}]});
    let saved = client::request_json(&owned.endpoint, "POST", "/v1/domain/remote-hosts", Some(&host)).unwrap();
    assert_eq!(saved["ok"], true);
    let hosts = rpc(&mut first, 2, "host_list_snapshot", json!({})).await;
    assert_eq!(hosts["result"], json!([host]));
    assert_eq!(rpc(&mut second, 1, "list_host_sessions", json!({"hostId":"peer"})).await["result"], host["sessions"]);
    client::attach_domain_remote_host_session(&owned.endpoint, "peer", "session-a").unwrap();
    assert!(std::fs::read_dir(&root).unwrap().flatten().any(|entry|
        entry.file_name().to_string_lossy().starts_with("remote-hosts.archive-")
        && serde_json::from_slice::<serde_json::Value>(&std::fs::read(entry.path()).unwrap()).unwrap()["peer"]["sessions"][0]["attached"] == false));
    drop(first);
    assert_eq!(rpc(&mut second, 2, "list_host_sessions", json!({"hostId":"peer"})).await["result"][0]["attached"], true);
    for args in [json!({}), json!({"hostId":"missing"}), json!({"hostId":42})] {
        assert!(rpc(&mut second, 3, "list_host_sessions", args).await["error"].is_object());
    }
    for method in ["host_list_snapshot", "list_host_sessions"] {
        let reply = client::request_json(&owned.endpoint, "POST", "/v1/domain/commands", Some(&json!({
            "method":method, "args":{"hostId":"peer"}, "native":false, "roots":[root], "connectionId":Uuid::new_v4()
        }))).unwrap();
        assert_eq!(reply["ok"], false, "Remote must not inherit Native host registry access: {reply}");
    }
    drop(second); drop(owned);
    let restarted = start(&root).await;
    let mut reattached = attach(&restarted.endpoint).await;
    assert_eq!(rpc(&mut reattached, 1, "host_list_snapshot", json!({})).await["result"][0]["id"], "peer");
    assert_eq!(rpc(&mut reattached, 2, "list_host_sessions", json!({"hostId":"peer"})).await["result"][0]["id"], "session-a");
    assert_eq!(rpc(&mut reattached,3,"list_host_sessions",json!({"hostId":"peer"})).await["result"][0]["attached"],false,"Runtime restart cannot restore a dead transport attachment");
}

#[test]
fn damaged_topology_prevents_runtime_startup_without_overwriting_original() {
    let root = std::env::temp_dir().join(format!("ridge-damaged-host-list-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let damaged = br#"{"peer":{"sessions":"broken"}}"#;
    std::fs::write(root.join("remote-hosts.json"), damaged).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR", &root).env("RIDGE_KERNEL_FS_ROOT", &root)
        .env("RIDGE_WORKSPACE_SAVE_DIR", &root).env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .output().unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("restore remote host topology"));
    assert_eq!(std::fs::read(root.join("remote-hosts.json")).unwrap(), damaged);
    assert!(!root.join("kernel.json").exists());
    assert!(ridge_kernel::registry::save_remote_hosts_at(&root.join("remote-hosts.json"), &Default::default()).is_err());
    assert_eq!(std::fs::read(root.join("remote-hosts.json")).unwrap(), damaged);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retired_frontend_registration_cannot_create_live_hosts_or_rewrite_legacy_data() {
    let root = std::env::temp_dir().join(format!("ridge-host-retirement-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let legacy = json!({"reported-peer":{"id":"reported-peer","kind":"remote","label":"legacy peer","addr":"frontend-remote-link","status":"disconnected","detail":"legacy","sessions":[{"id":"pane-a","title":"kept","attached":false}]}});
    let bytes = serde_json::to_vec(&legacy).unwrap();
    std::fs::write(root.join("remote-hosts.json"), &bytes).unwrap();
    let owned = start(&root).await;
    let mut presentation = attach(&owned.endpoint).await;
    let registration = json!({"hostId":"reported-peer","kind":"remote","label":"fabricated live peer","sessions":[{"id":"pane-a","title":"replacement"}]});
    let host_files = || std::fs::read_dir(&root).unwrap().flatten().filter(|entry|entry.file_name().to_string_lossy().starts_with("remote-hosts.")).count();
    let before_files = host_files();
    for args in [registration.clone(), json!({})] {
        assert!(rpc(&mut presentation, 1, "register_frontend_host", args).await["error"].is_object());
        assert_eq!(std::fs::read(root.join("remote-hosts.json")).unwrap(), bytes);
    }
    assert_eq!(host_files(), before_files, "retired registration must not create a Host archive or rewrite");
    let hosts = rpc(&mut presentation, 2, "host_list_snapshot", json!({})).await;
    assert_eq!(hosts["result"][0]["label"], "legacy peer");
    assert_eq!(hosts["result"][0]["status"], "disconnected");
    assert_eq!(rpc(&mut presentation, 3, "list_host_sessions", json!({"hostId":"reported-peer"})).await["result"][0]["title"], "kept");
    let denied = client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"register_frontend_host","args":registration,"native":false,"roots":[root],"connectionId":Uuid::new_v4()}))).unwrap();
    assert_eq!(denied["ok"], false);
    assert_eq!(std::fs::read(root.join("remote-hosts.json")).unwrap(), bytes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_reachability_cannot_claim_authenticated_host_or_persist_credentials() {
    let root = std::env::temp_dir().join(format!("ridge-host-probe-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let mut presentation = attach(&owned.endpoint).await;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let args = json!({"kind":"rdg","addr":addr,"label":"owned TCP listener","token":"must-not-persist"});
    let remote = client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"connect_host","args":args,"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote["ok"], false);
    assert!(!root.join("remote-hosts.json").exists());
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock, "Remote denial must precede the TCP probe");
    let listener=tokio::net::TcpListener::from_std(listener).unwrap();
    let refusal=tokio::spawn(async move {
        use tokio::io::{AsyncReadExt,AsyncWriteExt};
        let (mut socket,peer)=listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let mut bytes=[0u8;8192];let _=socket.read(&mut bytes).await.unwrap();
        socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
    });
    let reply = rpc(&mut presentation, 1, "connect_host", args.clone()).await;
    assert_eq!(reply["result"], format!("rdg:{addr}"), "{reply}");
    refusal.await.unwrap();
    let snapshot = rpc(&mut presentation, 2, "host_list_snapshot", json!({})).await;
    assert_eq!(snapshot["result"][0]["status"], "error");
    assert_eq!(snapshot["result"][0]["sessions"], json!([]), "TCP reachability must not fabricate a live session");
    assert!(snapshot["result"][0]["detail"].as_str().unwrap().contains("authentication"));
    assert!(!std::fs::read_to_string(root.join("remote-hosts.json")).unwrap().contains("must-not-persist"));
    let before = std::fs::read(root.join("remote-hosts.json")).unwrap();
    let mut credentials = args.clone(); credentials["addr"] = json!("https://user:private@127.0.0.1:1");
    assert!(rpc(&mut presentation, 3, "connect_host", credentials).await["error"].is_object());
    assert_eq!(std::fs::read(root.join("remote-hosts.json")).unwrap(), before);
    assert!(rpc(&mut presentation, 4, "connect_host", args).await["error"].is_null());
    assert_eq!(rpc(&mut presentation, 5, "host_list_snapshot", json!({})).await["result"][0]["status"], "error");
}
