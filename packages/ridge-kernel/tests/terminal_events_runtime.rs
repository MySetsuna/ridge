#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use futures::{SinkExt, StreamExt};
use ridge_kernel::{client, registry::KernelEndpoint};
use ridge_term::terminal_protocol::{decode_frame, TerminalFrame};
use serde_json::{json, Value};
use std::{fs, time::Duration};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[test]
#[ignore = "owned PTY child, launched only by the integration test"]
fn terminal_event_child_fixture() {
    if std::env::var_os("RIDGE_EVENT_CHILD").is_none() { return; }
    use std::io::{BufRead, Write};
    for line in std::io::stdin().lock().lines() {
        let mut out = std::io::stdout().lock();
        match line.unwrap().trim() {
            "first" => {
                out.write_all(b"\x1b]133;AX\x07\x1b]133;").unwrap(); out.flush().unwrap();
                std::thread::sleep(Duration::from_millis(15));
                out.write_all(b"A\x07").unwrap();
            }
            "second" => { out.write_all(b"\x1b]633;A;optional\x1b\\").unwrap(); }
            _ => { out.write_all(b"ordinary output\r\n").unwrap(); }
        }
        out.flush().unwrap();
    }
}
fn workspace(endpoint: &KernelEndpoint) -> (Uuid, Uuid) {
    let created = client::request_json(endpoint, "POST", "/v1/domain/workspaces", None).unwrap();
    let workspace = Uuid::parse_str(created["workspace_id"].as_str().unwrap()).unwrap();
    let detail = client::request_json(endpoint, "GET", &format!("/v1/domain/workspaces/{workspace}"), None).unwrap();
    (workspace, Uuid::parse_str(detail["panes"][0].as_str().unwrap()).unwrap())
}
async fn subscribe(socket: &mut Socket, workspace: Uuid, pane: Uuid, activation: u64) -> TerminalFrame {
    socket.send(Message::Text(json!({"type":"subscribe-pane","workspaceId":workspace,"paneId":pane,"activationId":activation,"active":true}).to_string())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop { match socket.next().await.unwrap().unwrap() {
            Message::Binary(bytes) if bytes.first() == Some(&0x13) => {
                let frame = decode_frame(&bytes).unwrap();
                if frame.pane.workspace_id == workspace.to_string() && frame.activation_id == activation { break frame; }
            }
            Message::Text(text) => { let event: Value = serde_json::from_str(&text).unwrap(); assert_ne!(event["name"], "pane-terminal-error", "a dock or replacement activation must not fail the connection"); }
            _ => {}
        }}
    }).await.unwrap()
}
async fn event(socket: &mut Socket, name: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop { if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
            let event: Value = serde_json::from_str(&text).unwrap();
            if event["name"] == name { break event["payload"].clone(); }
            assert_ne!(event["name"], "pane-terminal-error", "unexpected current terminal failure: {event}");
        }}
    }).await.expect("Runtime event deadline")
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_prompt_tracks_dock_and_terminal_failure_is_connection_and_activation_scoped() {
    let root = std::env::temp_dir().join(format!("ridge-terminal-events-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let endpoint = &owned.endpoint;
    let (from, pane) = workspace(endpoint);
    let (to, target) = workspace(endpoint);
    let launch = json!({"pty_id":pane,"workspace_id":from,"program":std::env::current_exe().unwrap(),"args":["--exact","terminal_event_child_fixture","--ignored","--nocapture"],"cwd":root,"env":{"RIDGE_EVENT_CHILD":"1"},"cols":80,"rows":24});
    assert_eq!(client::request_json(endpoint, "POST", "/v1/domain/ptys", Some(&launch)).unwrap()["ok"], true);
    let original = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty| pty.pty_id == pane).unwrap();
    let instance = ridge_core::process_identity::instance(original.child_pid.unwrap()).unwrap();
    let mut first = attach(endpoint).await;
    let mut observer = attach(endpoint).await;
    assert!(rpc(&mut observer, 1, "list_workspaces", json!({})).await["error"].is_null());
    let initial = subscribe(&mut first, from, pane, 11).await;
    client::write_domain_pty_in_workspace(endpoint, pane, from, b"first\r\n").unwrap();
    for socket in [&mut first, &mut observer] {
        let prompt = event(socket, &format!("pane-prompt-{from}-{pane}")).await;
        assert_eq!(prompt["workspaceId"], from.to_string());
        assert_eq!(prompt["paneId"], pane.to_string());
        assert_eq!(prompt["terminalEpoch"], initial.terminal_epoch);
    }
    assert!(rpc(&mut first, 2, "dock_pane", json!({"sourcePaneId":pane,"targetPaneId":target,"region":"right"})).await["error"].is_null());
    let rebound = subscribe(&mut first, to, pane, 12).await;
    assert_eq!(initial.terminal_epoch, rebound.terminal_epoch);
    client::write_domain_pty_in_workspace(endpoint, pane, to, b"second\r\n").unwrap();
    for socket in [&mut first, &mut observer] {
        let prompt = event(socket, &format!("pane-prompt-{to}-{pane}")).await;
        assert_eq!(prompt["workspaceId"], to.to_string());
        assert_eq!(prompt["terminalEpoch"], initial.terminal_epoch);
    }
    assert!(ridge_core::process_identity::validate(&instance).is_ok());
    // Replace activation before forcing a real stream failure; only 13 may fail.
    subscribe(&mut first, to, pane, 13).await;
    // A same-activation resync replaces its transport lease, not its terminal.
    let resynced = subscribe(&mut first, to, pane, 13).await;
    assert_eq!(resynced.terminal_epoch, initial.terminal_epoch);
    first.send(Message::Text(json!({"type":"subscribe-pane","workspaceId":to,"paneId":pane,"activationId":12,"active":true}).to_string())).await.unwrap();
    // Cross the old task's idle check, then prove cleanup kept the replacement.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let current = rpc(&mut first, 3, "get_terminal_snapshot", json!({"workspaceId":to,"paneId":pane,"activationId":13})).await;
    assert!(current["result"].is_string(), "old cleanup or stale activation removed the current stream: {current}");
    assert_eq!(client::request_json(endpoint, "DELETE", &format!("/v1/domain/ptys/{pane}"), None).unwrap()["ok"], true);
    let failed = event(&mut first, "pane-terminal-error").await;
    assert_eq!(failed, json!({"workspaceId":to,"paneId":pane,"activationId":13}));
    // Event is delivered directly to the subscribed connection, never broadcast.
    observer.send(Message::Text(json!({"jsonrpc":"2.0","id":9,"method":"list_workspaces","params":{}}).to_string())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop { if let Message::Text(text) = observer.next().await.unwrap().unwrap() {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_ne!(value["name"], "pane-terminal-error", "other viewers must not inherit this activation's failure");
            if value["id"] == 9 { break; }
        }}
    }).await.unwrap();
}
