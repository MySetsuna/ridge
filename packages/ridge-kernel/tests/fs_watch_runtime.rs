//! Real OS file notifications through the shared authenticated Runtime boundary.
use futures::StreamExt;
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;
type Events = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
struct Owned(KernelEndpoint);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = client::shutdown_endpoint(&self.0, Duration::from_secs(3));
    }
}
async fn attach(endpoint: &KernelEndpoint, id: Uuid, native: bool) -> Events {
    let mut request = format!(
        "ws://127.0.0.1:{}/v1/domain/events?connectionId={id}&native={native}",
        endpoint.port
    )
    .into_client_request()
    .unwrap();
    request
        .headers_mut()
        .insert("x-ridge-kernel-token", endpoint.token.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    loop {
        if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
            if serde_json::from_str::<Value>(&text).unwrap()["payload"]["initial"] == true {
                return socket;
            }
        }
    }
}
fn command(
    endpoint: &KernelEndpoint,
    id: Uuid,
    native: bool,
    roots: Value,
    granted: Value,
) -> Value {
    client::request_json(endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"start_watching_paths","args":{"roots":roots},"native":native,"roots":granted,"connectionId":id}))).unwrap()
}
fn git_command(endpoint: &KernelEndpoint, id: Uuid, roots: Value, granted: Value) -> Value {
    client::request_json(endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"start_watching_repos","args":{"roots":roots},"native":false,"roots":granted,"connectionId":id}))).unwrap()
}
async fn scm(socket: &mut Events) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["name"] == "scm-repo-changed" {
                    return event;
                }
            }
        }
    })
    .await
    .expect("SCM notification deadline")
}
async fn changed(socket: &mut Events, needle: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                let event: Value = serde_json::from_str(&text).unwrap();
                if event["name"] == "fs-changed"
                    && event["payload"]["paths"].as_array().is_some_and(|paths| {
                        paths
                            .iter()
                            .any(|path| path.as_str().is_some_and(|path| path.ends_with(needle)))
                    })
                {
                    return event;
                }
            }
        }
    })
    .await
    .expect("real OS notification deadline")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_file_subscriptions_survive_presentation_detach_and_reconnect() {
    let root = std::env::temp_dir().join(format!("ridge-watch-runtime-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let folder = root.join("watched");
    fs::create_dir_all(&folder).unwrap();
    let mut process = Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR", &root)
        .env("RIDGE_KERNEL_FS_ROOT", &root)
        .env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let endpoint = loop {
        assert!(process.try_wait().unwrap().is_none());
        if let Ok(bytes) = fs::read(root.join("kernel.json")) {
            if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                if client::health_ok(&endpoint) {
                    break endpoint;
                }
            }
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let _owned = Owned(endpoint.clone());
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let mut native = attach(&endpoint, first, true).await;
    let mut remote = attach(&endpoint, second, false).await;
    let specs = json!([{"path":folder,"recursive":true}]);
    let scope = json!([root]);
    assert_eq!(
        command(&endpoint, first, true, specs.clone(), scope.clone())["ok"],
        true
    );
    assert_eq!(
        command(&endpoint, second, false, specs.clone(), scope.clone())["ok"],
        true
    );
    fs::write(folder.join("first.txt"), "first").unwrap();
    let (a, b) = tokio::join!(
        changed(&mut native, "first.txt"),
        changed(&mut remote, "first.txt")
    );
    assert_eq!(a["connectionId"], first.to_string());
    assert_eq!(b["connectionId"], second.to_string());
    assert_eq!(
        command(&endpoint, first, true, json!([]), scope.clone())["ok"],
        true
    );
    fs::write(folder.join("only-second.txt"), "second").unwrap();
    changed(&mut remote, "only-second.txt").await;
    assert!(
        tokio::time::timeout(Duration::from_millis(500), native.next())
            .await
            .is_err(),
        "one client's selection must not deliver another client's file events"
    );
    native.close(None).await.unwrap();
    assert_eq!(
        command(
            &endpoint,
            second,
            false,
            json!([{"path":std::env::temp_dir(),"recursive":true}]),
            scope.clone()
        )["ok"],
        false,
        "nested roots cannot bypass serving scope"
    );
    fs::write(folder.join("after-detach.txt"), "still live").unwrap();
    changed(&mut remote, "after-detach.txt").await;
    let third = Uuid::new_v4();
    let mut reconnected = attach(&endpoint, third, true).await;
    assert_eq!(command(&endpoint, third, true, specs, scope)["ok"], true);
    fs::write(folder.join("reconnected.txt"), "new subscription").unwrap();
    tokio::join!(
        changed(&mut reconnected, "reconnected.txt"),
        changed(&mut remote, "reconnected.txt")
    );
    let git = folder.join(".git");
    fs::create_dir_all(git.join("objects")).unwrap();
    fs::write(git.join("HEAD"), "ref: refs/heads/main").unwrap();
    assert_eq!(
        git_command(&endpoint, second, json!([folder]), json!([root]))["ok"],
        true
    );
    fs::write(git.join("index"), "index-change").unwrap();
    let git_event = scm(&mut remote).await;
    assert_eq!(git_event["payload"], folder.to_string_lossy().to_string());
    assert_eq!(git_event["connectionId"], second.to_string());
    fs::write(git.join("objects/noise"), "object").unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(500), remote.next())
            .await
            .is_err(),
        "Git object writes cannot cause a refresh loop"
    );
    assert_eq!(
        git_command(&endpoint, second, json!([]), json!([root]))["ok"],
        true
    );
    fs::write(
        folder.join("files-still-live.txt"),
        "independent subscriptions",
    )
    .unwrap();
    changed(&mut remote, "files-still-live.txt").await;
    reconnected.close(None).await.unwrap();
    remote.close(None).await.unwrap();
    assert!(
        client::health_ok(&endpoint),
        "watch lifecycle cannot stop the shared Runtime"
    );
}
