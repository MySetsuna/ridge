//! Real service restart, archived settings and actual child cwd through production RPC.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use serde_json::json;
use std::{fs, time::{Duration, Instant}};
use uuid::Uuid;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn default_cwd_commits_before_success_and_survives_presentation_and_service_restart() {
    let root = std::env::temp_dir().join(format!("ridge-settings-{}", Uuid::new_v4()));
    let cwd = root.join("configured");
    fs::create_dir_all(&cwd).unwrap();
    let settings = root.join("runtime-settings.json");
    let legacy = br#"{"legacyPreference":"retained"}"#;
    fs::write(&settings, legacy).unwrap();
    let owned = start(&root).await;
    let mut socket = attach(&owned.endpoint).await;
    let initial = rpc(&mut socket, 0, "get_user_default_cwd", json!({})).await;
    assert_eq!(initial["result"]["configured"], false, "{initial}");
    let mut observer = attach(&owned.endpoint).await;
    let _ = rpc(&mut observer, 0, "get_user_default_cwd", json!({})).await;
    let reply = rpc(&mut socket, 1, "set_user_default_cwd", json!({"path":format!("  {}  ",cwd.display())})).await;
    assert!(reply["error"].is_null(), "{reply}");
    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = observer.next().await {
                let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                if event["name"] == "user-default-cwd-changed" {break event;}
            }
        }
    }).await.expect("second presentation must receive committed settings notification");
    assert!(event["payload"].is_null(), "notification must not leak host paths");
    let reread = rpc(&mut observer, 1, "get_user_default_cwd", json!({})).await;
    assert_eq!(reread["result"]["path"], json!(cwd), "{reread}");
    let stored: serde_json::Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(stored["defaultCwd"], json!(cwd));
    assert_eq!(stored["legacyPreference"], "retained");
    assert!(fs::read_dir(&root).unwrap().flatten().any(|entry|
        entry.file_name().to_string_lossy().starts_with("runtime-settings.archive-")
        && fs::read(entry.path()).ok().as_deref() == Some(legacy)));
    let invalid = rpc(&mut socket, 9, "set_user_default_cwd", json!({"path":42})).await;
    assert!(!invalid["error"].is_null(), "{invalid}");
    assert_eq!(fs::read(&settings).unwrap(), serde_json::to_vec_pretty(&stored).unwrap());
    let outside = std::env::temp_dir().join("outside-settings");
    let denied = rpc(&mut socket, 2, "set_user_default_cwd", json!({"path":outside})).await;
    assert!(!denied["error"].is_null(), "{denied}");
    assert_eq!(fs::read(&settings).unwrap(), serde_json::to_vec_pretty(&stored).unwrap());
    let unexpected = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if let Some(Ok(Message::Text(text))) = observer.next().await {
                let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                if event["name"] == "user-default-cwd-changed" {break event;}
            }
        }
    }).await;
    assert!(unexpected.is_err(), "rejected settings update must not announce a commit");
    drop(socket);
    drop(observer);
    drop(owned);
    let restarted = start(&root).await;
    let workspace = client::request_json(&restarted.endpoint, "POST", "/v1/domain/workspaces", None).unwrap()["workspace_id"].clone();
    let created = client::request_json(&restarted.endpoint, "POST", "/v1/domain/ptys", Some(&json!({
        "workspace_id":workspace,"program":if cfg!(windows){"cmd.exe"}else{"/bin/sh"},
        "args":if cfg!(windows){json!(["/d","/c","cd > actual-cwd.txt"])}else{json!(["-c","pwd > actual-cwd.txt"])},
        "role":"shell","cols":80,"rows":24
    }))).unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let output = cwd.join("actual-cwd.txt");
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::metadata(&output).map(|meta| meta.len()).unwrap_or(0) == 0 {
        assert!(Instant::now() < deadline, "child failed to use configured cwd");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let observed = fs::read_to_string(output).unwrap();
    assert_eq!(fs::canonicalize(observed.trim()).unwrap(), fs::canonicalize(&cwd).unwrap());
    let mut socket = attach(&restarted.endpoint).await;
    let cleared = rpc(&mut socket, 3, "set_user_default_cwd", json!({"path":"   "})).await;
    assert!(cleared["error"].is_null(), "{cleared}");
    let cleared_file: serde_json::Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert!(cleared_file["defaultCwd"].is_null());
    assert_eq!(cleared_file["legacyPreference"], "retained");
    let remote = client::request_json(&restarted.endpoint, "POST", "/v1/domain/commands", Some(&json!({
        "method":"set_user_default_cwd","args":{"path":cwd},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote["ok"], true, "{remote}");
    let remote_file: serde_json::Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
    assert_eq!(remote_file["defaultCwd"], json!(cwd));
    let remote_denied = client::request_json(&restarted.endpoint, "POST", "/v1/domain/commands", Some(&json!({
        "method":"set_user_default_cwd","args":{"path":outside},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(remote_denied["ok"], false, "{remote_denied}");
    let read_back = rpc(&mut socket, 5, "get_user_default_cwd", json!({})).await;
    assert_eq!(read_back["result"], json!({"configured":true,"path":cwd}), "{read_back}");
    fs::write(&settings, b"corrupt-original").unwrap();
    let failed = rpc(&mut socket, 4, "set_user_default_cwd", json!({"path":null})).await;
    assert!(!failed["error"].is_null(), "{failed}");
    assert_eq!(fs::read(&settings).unwrap(), b"corrupt-original");
}
