//! Owned Kernel, actual CLI processes/PTYs and a local Cloud authentication fixture.
use axum::{
    extract::{
        ws::{Message as AxumMessage, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

struct Owned {
    root: PathBuf,
    endpoint: KernelEndpoint,
    process: Child,
}
impl Owned {
    async fn start() -> Self {
        let root = std::env::temp_dir().join(format!("ridge-cli-create-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("auth.json"),
            json!({"token":"valid","device_name":"fixture","username":"owner"}).to_string(),
        )
        .unwrap();
        let mut process = Command::new(env!("CARGO_BIN_EXE_ridge"))
            .arg(client::KERNEL_HOST_ARG)
            .env("RIDGE_KERNEL_DATA_DIR", &root)
            .env("RIDGE_KERNEL_FS_ROOT", &root)
            .env("RIDGE_WORKSPACE_SAVE_DIR", &root)
            .env("RIDGE_AUTH_FILE", root.join("auth.json"))
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            assert!(
                process.try_wait().unwrap().is_none(),
                "owned Kernel exited during boot"
            );
            if let Ok(bytes) = fs::read(root.join("kernel.json")) {
                if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                    if endpoint.pid == process.id() && client::health_ok(&endpoint) {
                        return Self {
                            root,
                            endpoint,
                            process,
                        };
                    }
                }
            }
            assert!(Instant::now() < deadline, "owned Kernel boot deadline");
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    }
    fn request(&self, entry: &str) -> Value {
        let status = client::request_json(&self.endpoint, "GET", "/v1/status", None).unwrap();
        json!({"workspaceId":Uuid::new_v4(),"paneId":Uuid::new_v4(),"runtimeEpoch":status["runtime_epoch"],
            "entry":entry,"shell":shell(),"cwd":self.root})
    }
    fn create(&self, args: &Value, native: bool) -> Value {
        command(
            &self.endpoint,
            "create_independent_terminal",
            args.clone(),
            native,
        )
    }
    fn counts(&self) -> (usize, usize) {
        (
            client::read_domain_workspaces(&self.endpoint)
                .unwrap()
                .workspaces
                .len(),
            client::list_domain_ptys(&self.endpoint).unwrap().len(),
        )
    }
    fn cli(&self, args: &[&str], relay: &Relay) -> Cli {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(env!("CARGO_BIN_EXE_ridge"));
        cmd.args(args);
        cmd.env("RIDGE_KERNEL_DATA_DIR", &self.root);
        cmd.env("RIDGE_AUTH_FILE", self.root.join("auth.json"));
        cmd.env("RIDGE_BASE_DOMAIN", &relay.domain);
        cmd.env("RIDGE_CLOUD_DEV_PLAINTEXT", "1");
        cmd.env("TERM", "xterm-256color");
        cmd.cwd(&self.root);
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let output = Arc::new(Mutex::new(Vec::new()));
        let capture = output.clone();
        let mut reader = pair.master.try_clone_reader().unwrap();
        std::thread::spawn(move || {
            let mut bytes = [0; 4096];
            while let Ok(count) = reader.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                let mut output = capture.lock().unwrap();
                if output.len() < 512 * 1024 {
                    output.extend_from_slice(&bytes[..count]);
                }
            }
        });
        let input = pair.master.take_writer().unwrap();
        Cli {
            child,
            _master: pair.master,
            input,
            output,
        }
    }
    async fn wait_count(&self, count: usize, cli: &mut Cli) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if self.counts().1 == count {
                break;
            }
            assert!(
                cli.child.try_wait().unwrap().is_none(),
                "CLI exited before creating a terminal: {}",
                cli.text()
            );
            assert!(
                Instant::now() < deadline,
                "CLI creation deadline: {}",
                cli.text()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(
            cli.child.try_wait().unwrap().is_none(),
            "CLI failed after terminal creation: {}",
            cli.text()
        );
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = client::shutdown_endpoint(&self.endpoint, Duration::from_secs(3));
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.process.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if self.process.try_wait().ok().flatten().is_none() {
            let _ = self.process.kill();
        }
        let _ = self.process.wait();
    }
}
struct Cli {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
    input: Box<dyn Write + Send>,
    output: Arc<Mutex<Vec<u8>>>,
}
impl Cli {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }
    async fn terminal_round_trip(&mut self) {
        self.input.write_all(child_input()).unwrap();
        self.input.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.text().contains("888") {
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "TUI exited during child round trip: {}",
                self.text()
            );
            assert!(
                Instant::now() < deadline,
                "TUI child reply missing: {}",
                self.text()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    async fn expect_failure(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(
                    !status.success(),
                    "failed startup returned success: {}",
                    self.text()
                );
                return;
            }
            assert!(
                Instant::now() < deadline,
                "rejected CLI did not exit: {}",
                self.text()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
impl Drop for Cli {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Relay {
    domain: String,
    reject: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Relay {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let domain = format!("localhost:{}", listener.local_addr().unwrap().port());
        let reject = Arc::new(AtomicBool::new(false));
        let app = Router::new()
            .route("/ws", get(relay_socket))
            .route(
                "/api/v1/ice-servers",
                get(|| async {
                    Json(json!({"ok":true,"data":{"iceServers":[{"urls":["stun:127.0.0.1:9"]}]}}))
                }),
            )
            .with_state(reject.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            domain,
            reject,
            task,
        }
    }
    fn config(&self, root: &Owned) -> Value {
        json!({"deviceToken":"valid","deviceName":"fixture","username":"owner","baseDomain":self.domain,"root":root.root,"plaintext":true})
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn relay_socket(
    State(reject): State<Arc<AtomicBool>>,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    if reject.load(Ordering::Acquire) || query.get("token").map(String::as_str) != Some("valid") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(|mut socket| async move {
        socket
            .send(AxumMessage::Text(
                json!({"t":"welcome","peerPresent":false}).to_string(),
            ))
            .await
            .unwrap();
        while socket.recv().await.is_some() {}
    })
}
fn shell() -> &'static str {
    if cfg!(windows) {
        "cmd.exe"
    } else {
        "/bin/sh"
    }
}
fn command(endpoint: &KernelEndpoint, method: &str, args: Value, native: bool) -> Value {
    client::request_json(endpoint, "POST", "/v1/domain/commands", Some(&json!({"method":method,"args":args,"native":native,"roots":[],"connectionId":Uuid::new_v4()}))).unwrap()
}
fn send_without_reading(endpoint: &KernelEndpoint, args: &Value) -> std::net::TcpStream {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    let body = json!({"method":"create_independent_terminal","args":args,"native":true,"roots":[],"connectionId":Uuid::new_v4()}).to_string();
    write!(stream, "POST /v1/domain/commands HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nx-ridge-kernel-token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", endpoint.token, body.len(), body).unwrap();
    stream
}
async fn round_trip(root: &Owned, pane: Uuid) {
    let lease = client::attach_domain_pty_output(&root.endpoint, pane, None).unwrap();
    client::write_domain_pty(&root.endpoint, pane, child_input()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut output = Vec::new();
    loop {
        if let client::KernelPtyOutput::Data(bytes) =
            client::poll_domain_pty_output(&root.endpoint, pane, lease, 100, 64).unwrap()
        {
            output.extend_from_slice(&bytes);
        }
        if output.windows(3).any(|bytes| bytes == b"888") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "real child reply missing: {}",
            String::from_utf8_lossy(&output)
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    client::detach_domain_pty_output(&root.endpoint, pane, lease).unwrap();
}
fn child_input() -> &'static [u8] {
    if cfg!(windows) {
        b"cmd.exe /d /c \"set /a 400+488\"\r\n"
    } else {
        b"printf '%d\\n' $((400+488))\n"
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_actual_cli_entries_keep_independent_identities_and_existing_selection() {
    let root = Owned::start().await;
    let relay = Relay::start().await;
    let baseline = root.request("dashboard");
    assert_eq!(root.create(&baseline, true)["ok"], true);
    let original = client::read_domain_workspaces(&root.endpoint)
        .unwrap()
        .active;
    let observer_grant = client::request_json(
        &root.endpoint,
        "POST",
        "/v1/local/clients",
        Some(&json!({"deviceId":Uuid::new_v4(),"origin":"http://127.0.0.1"})),
    )
    .unwrap();
    let mut observer = attach(&observer_grant).await.unwrap();
    let cwd = root.root.to_str().unwrap();
    let mut total = 1;
    for args in [
        vec!["tui", "--shell", shell(), "--cwd", cwd],
        vec![],
        vec![
            "tui",
            "--lan-host",
            "--port",
            "0",
            "--shell",
            shell(),
            "--cwd",
            cwd,
        ],
        vec!["remote", "--shell", shell(), "--cwd", cwd],
    ] {
        let previous = client::list_domain_ptys(&root.endpoint).unwrap();
        let mut cli = root.cli(&args, &relay);
        total += 1;
        root.wait_count(total, &mut cli).await;
        let after = client::list_domain_ptys(&root.endpoint).unwrap();
        let created: Vec<_> = after
            .iter()
            .filter(|pty| !previous.iter().any(|old| old.id == pty.id))
            .collect();
        assert_eq!(created.len(), 1);
        let visible = rpc(
            &mut observer,
            100 + total as u64,
            "list_workspaces",
            &json!({}),
        )
        .await;
        assert!(visible["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| workspace["id"] == created[0].workspace_id.unwrap().to_string()));
        let selected = rpc(
            &mut observer,
            200 + total as u64,
            "get_active_workspace_id",
            &json!({}),
        )
        .await;
        assert_eq!(selected["result"].as_str(), original.as_deref());
        assert_eq!(created[0].cwd.as_deref(), Some(cwd));
        assert_eq!(
            created[0].launch_profile.as_deref(),
            if args.first() == Some(&"remote") {
                None
            } else {
                Some("ridge-interactive")
            }
        );
        assert_ne!(
            created[0].workspace_id.unwrap().to_string(),
            baseline["workspaceId"].as_str().unwrap()
        );
        assert_eq!(root.counts().0, total);
        assert_eq!(
            client::read_domain_workspaces(&root.endpoint)
                .unwrap()
                .active,
            original
        );
        round_trip(&root, created[0].id).await;
        if args.len() == 5 && args[0] == "tui" {
            cli.terminal_round_trip().await;
        }
        drop(cli);
        assert_eq!(root.counts(), (total, total));
    }
    let mut cloud_only = root.cli(&["remote"], &relay);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(cloud_only.child.try_wait().unwrap().is_none());
    assert_eq!(root.counts(), (total, total));
    drop(cloud_only);
    let mut pager = root.cli(
        &["tui", "--sessions", "2", "--shell", shell(), "--cwd", cwd],
        &relay,
    );
    root.wait_count(total + 2, &mut pager).await;
    assert_eq!(root.counts().0, total + 1);
    assert_eq!(
        client::read_domain_workspaces(&root.endpoint)
            .unwrap()
            .active,
        original
    );
    pager.terminal_round_trip().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_cli_entries_reject_invalid_operator_and_failed_launches_without_residue() {
    let root = Owned::start().await;
    let relay = Relay::start().await;
    let baseline = root.request("dashboard");
    assert_eq!(root.create(&baseline, true)["ok"], true);
    let mut wrong = root.endpoint.clone();
    wrong.token = "invalid-local-operator".into();
    fs::write(
        root.root.join("kernel.json"),
        serde_json::to_vec(&wrong).unwrap(),
    )
    .unwrap();
    for args in [
        vec!["tui"],
        vec![],
        vec!["tui", "--lan-host"],
        vec!["remote", "--shell", shell()],
    ] {
        root.cli(&args, &relay).expect_failure().await;
        assert_eq!(root.counts(), (1, 1));
    }
    fs::write(
        root.root.join("kernel.json"),
        serde_json::to_vec(&root.endpoint).unwrap(),
    )
    .unwrap();
    for entry in ["tui", "dashboard", "lan", "cloud"] {
        let mut args = root.request(entry);
        if entry == "lan" {
            args["port"] = json!(0);
        }
        if entry == "cloud" {
            args["cloud"] = relay.config(&root);
        }
        assert_eq!(
            root.create(&args, false)["ok"],
            false,
            "remote gained local create authority"
        );
        args["shell"] = json!(root.root.join("missing-shell-executable"));
        assert_eq!(root.create(&args, true)["ok"], false);
        assert_eq!(root.counts(), (1, 1));
        args["shell"] = json!(shell());
        assert_eq!(root.create(&args, true)["ok"], true);
        let pane = Uuid::parse_str(args["paneId"].as_str().unwrap()).unwrap();
        round_trip(&root, pane).await;
        client::destroy_domain_pty(&root.endpoint, pane).unwrap();
        // An explicitly destroyed terminal is never recreated by replay.
        assert_eq!(root.create(&args, true)["ok"], false);
        assert_eq!(
            command(
                &root.endpoint,
                "close_workspace",
                json!({"workspaceId":args["workspaceId"]}),
                true
            )["ok"],
            true
        );
    }
    round_trip(
        &root,
        Uuid::parse_str(baseline["paneId"].as_str().unwrap()).unwrap(),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lan_and_cloud_authentication_fail_before_cli_terminal_creation() {
    let root = Owned::start().await;
    let relay = Relay::start().await;
    // bind_tcp probes the requested range of ten ports.
    let (port, _occupied) = loop {
        let first = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = first.local_addr().unwrap().port();
        if port > 65526 {
            continue;
        }
        let mut sockets = vec![first];
        for candidate in port + 1..port + 10 {
            if let Ok(socket) = std::net::TcpListener::bind(("0.0.0.0", candidate)) {
                sockets.push(socket);
            } else {
                break;
            }
        }
        if sockets.len() == 10 {
            break (port, sockets);
        }
    };
    root.cli(&["tui", "--lan-host", "--port", &port.to_string()], &relay)
        .expect_failure()
        .await;
    assert_eq!(root.counts(), (0, 0));
    relay.reject.store(true, Ordering::Release);
    root.cli(&["remote", "--shell", shell()], &relay)
        .expect_failure()
        .await;
    assert_eq!(root.counts(), (0, 0));
    relay.reject.store(false, Ordering::Release);
    let mut healthy = root.cli(&["remote", "--shell", shell()], &relay);
    root.wait_count(1, &mut healthy).await;
    let mut conflict = root.request("cloud");
    let mut config = relay.config(&root);
    config["root"] = json!(root.root.join("different-serving-root"));
    conflict["cloud"] = config;
    assert_eq!(root.create(&conflict, true)["ok"], false);
    assert_eq!(root.counts(), (1, 1));
    assert_eq!(
        command(&root.endpoint, "get_cloud_host_state", json!({}), true)["value"]["state"],
        "online"
    );
    round_trip(
        &root,
        client::list_domain_ptys(&root.endpoint).unwrap()[0].id,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn committed_replays_and_partial_save_failures_are_consistent_for_every_entry() {
    let root = Owned::start().await;
    let relay = Relay::start().await;
    let mut expected = 0;
    for entry in ["tui", "dashboard", "lan", "cloud"] {
        let mut args = root.request(entry);
        if entry == "lan" {
            args["port"] = json!(0);
        }
        if entry == "cloud" {
            args["cloud"] = relay.config(&root);
        }
        let path = root.root.join("workspace-graph.json");
        let original = fs::read(&path).ok();
        fs::write(&path, b"broken-owned-fixture").unwrap();
        assert_eq!(root.create(&args, true)["ok"], false);
        assert_eq!(root.counts(), (expected, expected));
        assert_eq!(fs::read(&path).unwrap(), b"broken-owned-fixture");
        if let Some(bytes) = original {
            fs::write(&path, bytes).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        let unread = send_without_reading(&root.endpoint, &args);
        let deadline = Instant::now() + Duration::from_secs(10);
        while root.counts().1 != expected + 1 {
            assert!(
                Instant::now() < deadline,
                "unobserved creation did not commit"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        // The caller loses the real response, then retries with the original identities.
        drop(unread);
        let endpoint = root.endpoint.clone();
        let repeated = args.clone();
        // Two concurrent deliveries, each valid even if its response is lost.
        let concurrent = std::thread::spawn(move || {
            command(&endpoint, "create_independent_terminal", repeated, true)
        });
        let first = root.create(&args, true);
        let second = concurrent.join().unwrap();
        assert_eq!(first["ok"], true, "{first}");
        assert_eq!(first["value"], second["value"]);
        expected += 1;
        assert_eq!(root.counts(), (expected, expected));
        let replay = root.create(&args, true);
        assert_eq!(replay["value"], first["value"]);
        let mut changed = args.clone();
        changed["runtimeEpoch"] = json!("old-kernel-generation");
        assert_eq!(root.create(&changed, true)["ok"], false);
        assert_eq!(root.create(&args, false)["ok"], false);
        assert_eq!(root.counts(), (expected, expected));
        let pane = Uuid::parse_str(args["paneId"].as_str().unwrap()).unwrap();
        round_trip(&root, pane).await;
    }
}

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
async fn attach(grant: &Value) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!(
        "{}?deviceId={}",
        grant["wsUrl"].as_str().unwrap(),
        grant["deviceId"].as_str().unwrap()
    )
    .into_client_request()
    .unwrap();
    request
        .headers_mut()
        .insert("origin", grant["origin"].as_str().unwrap().parse().unwrap());
    request.headers_mut().insert(
        "sec-websocket-protocol",
        format!("ridge-client, auth-{}", grant["token"].as_str().unwrap())
            .parse()
            .unwrap(),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}
async fn rpc(socket: &mut Socket, id: u64, method: &str, args: &Value) -> Value {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":args}).to_string(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["id"] == id {
                    return value;
                }
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_tui_socket_rejects_invalid_grant_and_replays_original_identity_after_disconnect() {
    let root = Owned::start().await;
    let grant = client::request_json(
        &root.endpoint,
        "POST",
        "/v1/local/clients",
        Some(&json!({"deviceId":Uuid::new_v4(),"origin":"http://127.0.0.1"})),
    )
    .unwrap();
    let mut invalid = grant.clone();
    invalid["token"] = json!("0".repeat(64));
    assert!(attach(&invalid).await.is_err());
    assert_eq!(root.counts(), (0, 0));
    let args = root.request("tui");
    let mut socket = attach(&grant).await.unwrap();
    let first = rpc(&mut socket, 1, "create_independent_terminal", &args).await;
    assert!(first["error"].is_null(), "{first}");
    drop(socket);
    let mut resumed = attach(&grant).await.unwrap();
    assert_eq!(
        rpc(&mut resumed, 2, "create_independent_terminal", &args).await["result"],
        first["result"]
    );
    assert_eq!(root.counts(), (1, 1));
}
