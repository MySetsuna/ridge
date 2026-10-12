//! Actual owned Kernel/PTY/process/socket registration, no brand adapters.
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[test]
#[ignore = "already-running protocol process fixture"]
fn existing_protocol_endpoint_fixture() {
    let path = std::env::var_os("RIDGE_COMMUNE_FIXTURE").expect("fixture path");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    fs::write(path,serde_json::to_vec(&json!({"process":ridge_core::process_identity::instance(std::process::id()).unwrap(),"endpoint":listener.local_addr().unwrap().to_string(),"sessionId":"fixture-existing"})).unwrap()).unwrap();
    for accepted in listener.incoming() {
        let mut socket = accepted.unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut pending = Value::Null;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let value: Value = serde_json::from_str(&line).unwrap();
            let result = match value["method"].as_str().unwrap() {
                "initialize" => {
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":true},"agentInfo":{"name":"unknown-protocol-process","version":"fixture"}})
                }
                "session/load" => {
                    assert_eq!(value["params"]["sessionId"], "fixture-existing");
                    writeln!(socket,"{}",json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fixture-existing","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"existing history"}}}})).unwrap();
                    json!({})
                }
                "session/prompt" => {
                    assert_eq!(value["params"]["sessionId"], "fixture-existing");
                    assert!(pending.is_null(), "one writer per existing conversation");
                    let text = value["params"]["prompt"][0]["text"].as_str().unwrap();
                    let mut log = fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open("prompt-log")
                        .unwrap();
                    writeln!(log, "{text}").unwrap();
                    if text == "disconnect-during-prompt" {
                        break;
                    }
                    if text == "wait-for-cancel" {
                        pending = value["id"].clone();
                        continue;
                    }
                    json!({"stopReason":"end_turn"})
                }
                "session/cancel" => {
                    assert_eq!(value["params"]["sessionId"], "fixture-existing");
                    assert!(!pending.is_null());
                    writeln!(
                        socket,
                        "{}",
                        json!({"jsonrpc":"2.0","id":pending,"result":{"stopReason":"cancelled"}})
                    )
                    .unwrap();
                    pending = Value::Null;
                    continue;
                }
                other => panic!("registration must not call {other}"),
            };
            writeln!(
                socket,
                "{}",
                json!({"jsonrpc":"2.0","id":value["id"],"result":result})
            )
            .unwrap();
        }
    }
}

fn tool(endpoint: &KernelEndpoint, name: &str, args: Value) -> Value {
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string();
    let mut socket = std::net::TcpStream::connect(("127.0.0.1", endpoint.port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(socket,"POST /api/v1/mcp HTTP/1.1\r\nHost: localhost\r\nx-ridge-token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", endpoint.token, body.len(), body).unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    let rpc: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(rpc["error"].is_null(), "MCP error: {rpc}");
    assert_ne!(rpc["result"]["isError"], true, "MCP tool error: {rpc}");
    serde_json::from_str(rpc["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

fn wait_execution(endpoint: &KernelEndpoint, target: &Value, id: &Value, expected: &str) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let mut args = target.clone();
        args["delivery_id"] = id.clone();
        let receipt = tool(endpoint, "ridge_delivery_status", args);
        if receipt["execution"]["state"] == expected {
            return receipt;
        }
        assert!(Instant::now() < deadline, "expected {expected}: {receipt}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
struct Owned(KernelEndpoint);
impl Drop for Owned {
    fn drop(&mut self) {
        if client::health_ok(&self.0) {
            let _ = client::shutdown_endpoint(&self.0, Duration::from_secs(3));
        }
    }
}
#[test]
fn registration_validates_real_instances_and_keeps_existing_process() {
    let root = std::env::temp_dir().join(format!("ridge-commune-registration-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut kernel = Command::new(env!("CARGO_BIN_EXE_ridge-kernel"))
        .env("RIDGE_KERNEL_DATA_DIR", &root)
        .env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let endpoint = loop {
        assert!(kernel.try_wait().unwrap().is_none());
        if let Ok(bytes) = fs::read(root.join("kernel.json")) {
            if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                if client::health_ok(&endpoint) {
                    break endpoint;
                }
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    };
    let _owned = Owned(endpoint.clone());
    assert_eq!(endpoint.pid, kernel.id());
    let workspace = client::request_json(&endpoint, "POST", "/v1/domain/workspaces", None).unwrap()
        ["workspace_id"]
        .clone();
    let pane = Uuid::new_v4();
    let descriptor = root.join("existing-endpoint.json");
    let created=client::request_json(&endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane,"workspace_id":workspace,"program":std::env::current_exe().unwrap(),"args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,"env":{"RIDGE_COMMUNE_FIXTURE":descriptor},"role":"shell","cols":80,"rows":24}))).unwrap();
    assert_eq!(created["ok"], true);
    let deadline = Instant::now() + Duration::from_secs(8);
    let described = loop {
        if let Ok(bytes) = fs::read(&descriptor) {
            if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                break value;
            }
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    };
    let ptys = client::list_domain_ptys(&endpoint).unwrap();
    let original = ptys.iter().find(|pty| pty.pty_id == pane).unwrap();
    assert_eq!(
        original.child_pid.unwrap(),
        described["process"]["pid"].as_u64().unwrap() as u32
    );
    assert!(original.child_start_instance.is_some());
    let body = json!({"workspaceId":workspace,"paneId":pane,"process":described["process"],"endpointOwner":described["process"],"endpoint":described["endpoint"],"sessionId":described["sessionId"],"cwd":root});
    let mut unauthorized = endpoint.clone();
    unauthorized.token = "client-not-kernel".into();
    assert!(client::request_json(
        &unauthorized,
        "POST",
        "/v1/domain/commune/register",
        Some(&body)
    )
    .is_err());
    let mut wrong = body.clone();
    wrong["process"]["startInstance"] = json!("reused-pid");
    assert_eq!(
        client::request_json(
            &endpoint,
            "POST",
            "/v1/domain/commune/register",
            Some(&wrong)
        )
        .unwrap()["ok"],
        false
    );
    let accepted = client::request_json(
        &endpoint,
        "POST",
        "/v1/domain/commune/register",
        Some(&body),
    )
    .unwrap();
    assert_eq!(accepted["ok"], true, "{}", accepted["error"]);
    let duplicate = client::request_json(
        &endpoint,
        "POST",
        "/v1/domain/commune/register",
        Some(&body),
    )
    .unwrap();
    assert_eq!(accepted, duplicate);
    let roster = client::read_domain_agent_roster(&endpoint).unwrap();
    let identity = roster
        .agent_identities
        .iter()
        .find(|identity| identity.agent_id == accepted["value"]["agentId"].as_str().unwrap())
        .unwrap();
    assert_eq!(identity.session_id, "fixture-existing");
    assert_eq!(identity.pane_id, pane.to_string());
    assert_eq!(
        identity.protocol_binding.as_ref().unwrap().process.pid,
        original.child_pid.unwrap()
    );
    assert_eq!(
        client::list_domain_ptys(&endpoint)
            .unwrap()
            .iter()
            .find(|pty| pty.pty_id == pane)
            .unwrap()
            .child_pid,
        original.child_pid
    );
    let history = root
        .join("commune-history")
        .join(format!("{}.jsonl", identity.agent_id));
    assert!(fs::read_to_string(&history)
        .unwrap()
        .contains("existing history"));
    let target = json!({"workspace_id":workspace,"agent_id":identity.agent_id,"generation":identity.generation,"lease":identity.lease});
    let mut first = target.clone();
    first["message"] = json!("wait-for-cancel");
    first["idempotency_key"] = json!("prompt-1");
    let sent = tool(&endpoint, "ridge_send_message", first.clone());
    assert_eq!(sent["deliveryAdapter"], "runtime_api");
    assert_eq!(sent["status"], "adapter_accepted");
    wait_execution(&endpoint, &target, &sent["deliveryId"], "running");
    let deadline = Instant::now() + Duration::from_secs(5);
    while fs::read_to_string(root.join("prompt-log")).ok().as_deref() != Some("wait-for-cancel\n") {
        assert!(
            Instant::now() < deadline,
            "existing Agent did not receive prompt"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let replay = tool(&endpoint, "ridge_send_message", first);
    assert_eq!(replay["deliveryId"], sent["deliveryId"]);
    assert_eq!(replay["deduplicated"], true);
    let mut second = target.clone();
    second["message"] = json!("after-cancel");
    second["idempotency_key"] = json!("prompt-2");
    let queued = tool(&endpoint, "ridge_send_message", second.clone());
    assert_eq!(
        fs::read_to_string(root.join("prompt-log")).unwrap(),
        "wait-for-cancel\n"
    );
    let mut cancel = target.clone();
    cancel["delivery_id"] = sent["deliveryId"].clone();
    let requested = tool(&endpoint, "ridge_cancel_delivery", cancel);
    assert_eq!(requested["cancellationRequested"], true);
    let cancelled = wait_execution(&endpoint, &target, &sent["deliveryId"], "cancelled");
    assert_eq!(
        cancelled["execution"]["response"]["stopReason"],
        "cancelled"
    );
    let ended = wait_execution(&endpoint, &target, &queued["deliveryId"], "turn_ended");
    assert_eq!(ended["execution"]["response"]["stopReason"], "end_turn");
    assert_eq!(ended["status"], "adapter_accepted");
    assert_eq!(ended["agentAcknowledged"], false);
    assert_eq!(
        tool(&endpoint, "ridge_send_message", second)["execution"],
        ended["execution"]
    );
    assert_eq!(
        fs::read_to_string(root.join("prompt-log")).unwrap(),
        "wait-for-cancel\nafter-cancel\n"
    );
    let mut third = target.clone();
    third["message"] = json!("disconnect-during-prompt");
    third["idempotency_key"] = json!("prompt-3");
    let lost = tool(&endpoint, "ridge_send_message", third);
    wait_execution(&endpoint, &target, &lost["deliveryId"], "unknown");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let roster = client::read_domain_agent_roster(&endpoint).unwrap();
        let member = roster
            .agent_identities
            .iter()
            .find(|member| member.agent_id == identity.agent_id)
            .unwrap();
        if !member.online {
            assert_eq!(
                member.lifecycle,
                ridge_core::teammate::communication::AgentLifecycle::Discovered
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "lost connection retained its control lease"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Offline members keep their receipts and process; unknown prompts are not retried.
    assert_eq!(
        wait_execution(&endpoint, &target, &lost["deliveryId"], "unknown")["status"],
        "adapter_accepted"
    );
    assert_eq!(
        client::list_domain_ptys(&endpoint)
            .unwrap()
            .iter()
            .find(|pty| pty.pty_id == pane)
            .unwrap()
            .child_pid,
        original.child_pid
    );
    assert_eq!(
        fs::read_to_string(root.join("prompt-log")).unwrap(),
        "wait-for-cancel\nafter-cancel\ndisconnect-during-prompt\n"
    );
    assert_eq!(
        client::list_domain_ptys(&endpoint).unwrap().len(),
        ptys.len()
    );
    client::destroy_domain_pty(&endpoint, pane).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let updated = client::read_domain_agent_roster(&endpoint).unwrap();
        let identity = updated
            .agent_identities
            .iter()
            .find(|identity| identity.agent_id == accepted["value"]["agentId"].as_str().unwrap())
            .expect("protocol identity/history must survive process exit");
        if !identity.online {
            assert_eq!(
                identity.lifecycle,
                ridge_core::teammate::communication::AgentLifecycle::Stopped
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "process exit did not retire its live lease"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(fs::read_to_string(&history)
        .unwrap()
        .contains("existing history"));
}
