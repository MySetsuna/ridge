//! Actual owned Kernel/PTY/process/socket registration, no brand adapters.
use futures::StreamExt;
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
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
                    if text == "permission-needed" || text == "cancel-permission" {
                        writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":"external-permission","method":"session/request_permission",
                            "params":{"sessionId":"fixture-existing","toolCall":{"toolCallId":"tool","title":"private-operation","rawInput":{"secret":"private-argument"}},
                            "options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},{"optionId":"always","name":"Allow always","kind":"allow_always"},{"optionId":"deny","name":"Reject","kind":"reject_once"}]}})).unwrap();
                        let mut cancelled = false;
                        let outcome = loop {
                            let mut line = String::new();
                            assert!(reader.read_line(&mut line).unwrap() > 0);
                            let reply: Value = serde_json::from_str(&line).unwrap();
                            if reply["method"] == "session/cancel" {
                                cancelled = true;
                                continue;
                            }
                            assert_eq!(reply["id"], "external-permission");
                            assert!(reply["error"].is_null(), "permission response: {reply}");
                            break reply["result"]["outcome"].clone();
                        };
                        if text == "cancel-permission" {
                            assert_eq!(outcome["outcome"], "cancelled");
                            if !cancelled {
                                let mut line = String::new();
                                reader.read_line(&mut line).unwrap();
                                assert_eq!(
                                    serde_json::from_str::<Value>(&line).unwrap()["method"],
                                    "session/cancel"
                                );
                            }
                        } else {
                            assert_eq!(outcome["optionId"], "once");
                        }
                        let mut log = fs::OpenOptions::new()
                            .append(true)
                            .create(true)
                            .open("permission-decisions")
                            .unwrap();
                        writeln!(log, "{outcome}").unwrap();
                        writeln!(socket,"{}",json!({"jsonrpc":"2.0","id":value["id"],"result":{"stopReason":if text=="cancel-permission" {"cancelled"}else{"end_turn"}}})).unwrap();
                        continue;
                    }
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

fn command_reply(endpoint: &KernelEndpoint, method: &str, args: Value, native: bool) -> Value {
    client::request_json(endpoint,"POST","/v1/domain/commands",Some(&json!({"method":method,"args":args,"native":native,"roots":[],"connectionId":Uuid::new_v4()}))).unwrap()
}
fn command(endpoint: &KernelEndpoint, method: &str, args: Value, native: bool) -> Value {
    let reply = command_reply(endpoint, method, args, native);
    assert_eq!(reply["ok"], true, "Runtime command failed: {reply}");
    reply["value"].clone()
}
fn wait_permission(endpoint: &KernelEndpoint) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let rows = command(endpoint, "list_hitl_pending_local", json!({}), true);
        if !rows.as_array().unwrap().is_empty() {
            return rows[0].clone();
        }
        assert!(
            Instant::now() < deadline,
            "real permission request did not reach Runtime"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
type Events =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
async fn event(socket: &mut Events) -> Value {
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
            return serde_json::from_str(&text).unwrap();
        }
    }
}
async fn events(endpoint: &KernelEndpoint, native: bool) -> Events {
    let mut request = format!(
        "ws://127.0.0.1:{}/v1/domain/events?connectionId={}&native={native}",
        endpoint.port,
        Uuid::new_v4()
    )
    .into_client_request()
    .unwrap();
    request
        .headers_mut()
        .insert("x-ridge-kernel-token", endpoint.token.parse().unwrap());
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    assert_eq!(event(&mut socket).await["name"], "workspace-list-changed");
    socket
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
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (mut local_events, mut remote_events) = runtime.block_on(async {
        (
            events(&endpoint, true).await,
            events(&endpoint, false).await,
        )
    });
    let native_target = json!({"workspaceId":workspace,"paneId":pane,"agentId":identity.agent_id,"generation":identity.generation,"lease":identity.lease});
    let members = command(
        &endpoint,
        "get_teammate_topology",
        json!({"workspaceId":workspace}),
        true,
    );
    assert_eq!(members["roster"].as_array().unwrap().len(), 1);
    assert_eq!(members["roster"][0]["name"], "unknown-protocol-process");
    assert_eq!(members["roster"][0]["sessionId"], "fixture-existing");
    assert_eq!(
        command_reply(
            &endpoint,
            "get_teammate_topology",
            json!({"workspaceId":Uuid::new_v4()}),
            true
        )["ok"],
        false
    );
    let mut prompt = native_target.clone();
    prompt["message"] = json!("permission-needed");
    prompt["idempotency_key"] = json!("permission-prompt");
    let permission_delivery = command(&endpoint, "send_agent_message", prompt, true);
    let pending = wait_permission(&endpoint);
    let remote = command(&endpoint, "list_hitl_pending", json!({}), false);
    for private in [
        "private-operation",
        "private-argument",
        "action",
        "options",
        "toolCall",
    ] {
        assert!(!remote.to_string().contains(private));
    }
    assert_eq!(
        command_reply(&endpoint, "list_hitl_pending_local", json!({}), false)["ok"],
        false
    );
    runtime.block_on(async {
        loop {
            let value = event(&mut local_events).await;
            if value["name"] == "teammate://hitl-approval-required" {
                assert_eq!(
                    value["payload"]["toolCall"]["rawInput"]["secret"],
                    "private-argument"
                );
                break;
            }
        }
        loop {
            let value = event(&mut remote_events).await;
            assert_ne!(value["name"], "teammate://hitl-approval-required");
            assert!(!value.to_string().contains("private-argument"));
            if value["name"] == "teammate://hitl-pending-changed" {
                break;
            }
        }
    });
    assert_eq!(
        command(
            &endpoint,
            "resolve_hitl_remote",
            json!({"id":pending["id"],"nonce":"wrong","verdict":"approve"}),
            false
        )["outcome"],
        "nonce-mismatch"
    );
    assert_eq!(
        command_reply(
            &endpoint,
            "resolve_hitl_request",
            json!({"id":pending["id"],"optionId":"not-offered"}),
            true
        )["ok"],
        false
    );
    assert_eq!(
        command_reply(
            &endpoint,
            "resolve_hitl_request",
            json!({"id":pending["id"],"optionId":"once","workspaceId":Uuid::new_v4()}),
            true
        )["ok"],
        false
    );
    assert_eq!(
        command_reply(
            &endpoint,
            "resolve_hitl_request",
            json!({"id":pending["id"],"verdict":"modify","replacement":"arbitrary command"}),
            true
        )["ok"],
        false
    );
    let approve = {
        let endpoint = endpoint.clone();
        let id = pending["id"].clone();
        std::thread::spawn(move || {
            command(
                &endpoint,
                "resolve_hitl_request",
                json!({"id":id,"optionId":"once"}),
                true,
            )
        })
    };
    let remote_approve = {
        let endpoint = endpoint.clone();
        let id = pending["id"].clone();
        let nonce = pending["resolutionNonce"].clone();
        std::thread::spawn(move || {
            command(
                &endpoint,
                "resolve_hitl_remote",
                json!({"id":id,"nonce":nonce,"verdict":"approve"}),
                false,
            )
        })
    };
    let local_won = approve.join().unwrap() == true;
    let remote_won = remote_approve.join().unwrap()["outcome"] == "consumed";
    assert_eq!(
        usize::from(local_won) + usize::from(remote_won),
        1,
        "only one real client may resolve the Agent request"
    );
    assert_eq!(
        command(
            &endpoint,
            "resolve_hitl_remote",
            json!({"id":pending["id"],"nonce":pending["resolutionNonce"],"verdict":"approve"}),
            false
        )["outcome"],
        "already-resolved"
    );
    wait_execution(
        &endpoint,
        &target,
        &permission_delivery["deliveryId"],
        "turn_ended",
    );
    assert_eq!(
        fs::read_to_string(root.join("permission-decisions"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let mut prompt = native_target.clone();
    prompt["message"] = json!("cancel-permission");
    prompt["idempotency_key"] = json!("cancel-permission-prompt");
    let cancellation = command(&endpoint, "send_agent_message", prompt, true);
    let cancelled_permission = wait_permission(&endpoint);
    let mut cancel = native_target.clone();
    cancel["deliveryId"] = cancellation["deliveryId"].clone();
    command(&endpoint, "cancel_agent_message", cancel, true);
    wait_execution(&endpoint, &target, &cancellation["deliveryId"], "cancelled");
    assert!(
        command(&endpoint, "list_hitl_pending_local", json!({}), true)
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        command(
            &endpoint,
            "resolve_hitl_request",
            json!({"id":cancelled_permission["id"],"optionId":"once"}),
            true
        ),
        false
    );
    let records = fs::read_to_string(&history)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(records
        .iter()
        .any(|row| row["kind"] == "permission_decided"));
    assert!(records
        .iter()
        .any(|row| row["kind"] == "permission_cancelled"));
    assert!(records
        .windows(2)
        .all(|pair| pair[0]["sequence"].as_u64().unwrap() < pair[1]["sequence"].as_u64().unwrap()));
    let before_history_read = fs::read(&history).unwrap();
    let first_history = command(
        &endpoint,
        "read_agent_recent_replies",
        json!({"workspaceId":workspace,"limit":2}),
        true,
    );
    let history_cursor = first_history.as_array().unwrap().last().unwrap()["cursor"]
        .as_str()
        .unwrap()
        .to_owned();
    let other_workspace =
        client::request_json(&endpoint, "POST", "/v1/domain/workspaces", None).unwrap()
            ["workspace_id"]
            .clone();
    let group_id = Uuid::new_v4().to_string();
    let group = json!({"id":group_id,"name":"exact-group","color":"#60a5fa","memberAgentIds":[identity.agent_id],"leaderAgentId":identity.agent_id,"createdAt":1});
    let legacy_dir = root.join("workspace-memory");
    fs::create_dir_all(&legacy_dir).unwrap();
    let legacy_path = legacy_dir.join(format!("{}.json",workspace.as_str().unwrap()));
    let legacy_bytes = serde_json::to_vec(&json!({"teammateGroups":[group],"tasks":[{"objective":"legacy objective, not proven execution"}]})).unwrap();
    fs::write(&legacy_path,&legacy_bytes).unwrap();
    let ambiguous_group = json!({"id":"unmapped","name":"preserve-old","color":"#60a5fa","memberAgentIds":["old-provider-alias"]});
    let group_snapshot = command(&endpoint,"set_teammate_groups",json!({"workspaceId":workspace,"expectedRevision":0,"legacyGroups":[group,ambiguous_group]}),true);
    assert_eq!(group_snapshot["groups"].as_array().unwrap().len(),1);
    assert_eq!(fs::read(&legacy_path).unwrap(),legacy_bytes,"migration must leave the source recoverable");
    let persisted_groups: Value = serde_json::from_slice(&fs::read(root.join("commune-groups.json")).unwrap()).unwrap();
    assert_eq!(persisted_groups["workspaces"][workspace.as_str().unwrap()]["legacy"]["browserGroups"][1],ambiguous_group);
    assert_eq!(persisted_groups["workspaces"][workspace.as_str().unwrap()]["legacy"]["workspaceMemory"]["tasks"][0]["objective"],"legacy objective, not proven execution");
    assert_eq!(group_snapshot["groupsRevision"],1);
    assert_eq!(group_snapshot["groups"][0]["revision"],1);
    let remote_group_snapshot = command(&endpoint,"get_teammate_topology",json!({"workspaceId":workspace}),false);
    assert_eq!(remote_group_snapshot["groups"],group_snapshot["groups"]);
    assert_eq!(remote_group_snapshot["groupsRevision"],1);
    assert_eq!(command_reply(&endpoint,"set_teammate_groups",json!({"workspaceId":workspace,"expectedRevision":0,"groups":[]}),false)["ok"],false);
    assert_eq!(command_reply(&endpoint,"set_teammate_groups",json!({"workspaceId":other_workspace,"expectedRevision":0,"groups":[group]}),false)["ok"],false);
    let mut renamed = group.clone(); renamed["name"] = json!("remote-rename");
    let mut remote_edit = command(&endpoint,"set_teammate_groups",json!({"workspaceId":workspace,"expectedRevision":1,"groups":[renamed]}),false);
    assert_eq!(remote_edit["groups"][0]["revision"],2);
    assert_eq!(command(&endpoint,"get_teammate_topology",json!({"workspaceId":workspace}),true)["groups"],remote_edit["groups"]);
    assert_eq!(
        command_reply(
            &endpoint,
            "read_agent_recent_replies",
            json!({"workspaceId":other_workspace,"cursor":history_cursor}),
            true
        )["ok"],
        false
    );
    assert_eq!(
        command_reply(
            &endpoint,
            "read_agent_recent_replies",
            json!({"workspaceId":workspace,"cursor":history_cursor}),
            false
        )["ok"],
        false
    );
    let rest = command(
        &endpoint,
        "read_agent_recent_replies",
        json!({"workspaceId":workspace,"limit":200,"cursor":history_cursor}),
        true,
    );
    assert!(rest.as_array().unwrap().iter().all(|row| first_history
        .as_array()
        .unwrap()
        .iter()
        .all(|first| first["id"] != row["id"])));
    assert_eq!(
        fs::read(&history).unwrap(),
        before_history_read,
        "history reads must not write or replay the session"
    );
    let remote_history = command(
        &endpoint,
        "read_agent_recent_replies",
        json!({"workspaceId":workspace,"limit":200}),
        false,
    );
    assert!(!remote_history.to_string().contains("private-argument"));
    assert!(remote_history
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["kind"] == "agent_message_chunk" && row["text"] == "existing history"));
    assert!(remote_history
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["memberId"] == identity.agent_id && row["sessionId"] == "fixture-existing"));
    runtime.block_on(async {
        local_events.close(None).await.unwrap();
        remote_events.close(None).await.unwrap();
    });
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
    second["objective"] = json!("after-cancel");
    second["group_id"] = json!(group_id);
    second["group_revision"] = json!(2);
    second["from"] = json!("user-client");
    second["idempotency_key"] = json!("prompt-2");
    let mut group_request = native_target.clone();
    group_request["message"] = json!("after-cancel");
    group_request["idempotency_key"] = json!("prompt-2");
    group_request["groupId"] = json!(group_id);
    group_request["groupRevision"] = json!(2);
    let queued = command(&endpoint,"send_agent_message",group_request.clone(),true);
    assert_eq!(queued["kind"],"task");
    assert!(queued["taskId"].is_string());
    assert_eq!(queued["payload"]["groupContext"]["groupRevision"],2);
    assert_eq!(queued["payload"]["groupContext"]["targets"][0]["generation"],identity.generation);
    let mut edited_group = remote_edit["groups"][0].clone();
    edited_group["color"] = json!("#123456");
    edited_group.as_object_mut().unwrap().remove("leaderAgentId");
    remote_edit = command(&endpoint,"set_teammate_groups",json!({"workspaceId":workspace,"expectedRevision":2,"groups":[edited_group]}),false);
    let duplicate = command(&endpoint,"send_agent_message",group_request.clone(),true);
    assert_eq!(duplicate["deliveryId"],queued["deliveryId"]);
    assert_eq!(duplicate["payload"]["groupContext"]["groupRevision"],2);
    let mut conflicting = group_request.clone(); conflicting["message"] = json!("different-objective");
    assert_eq!(command_reply(&endpoint,"send_agent_message",conflicting,true)["ok"],false);
    group_request["idempotency_key"] = json!("stale-new-task");
    assert_eq!(command_reply(&endpoint,"send_agent_message",group_request.clone(),true)["ok"],false);
    group_request["groupRevision"] = json!(3);
    assert_eq!(command_reply(&endpoint,"send_agent_message",group_request,true)["ok"],false,"a leaderless group cannot choose another recipient");
    assert_eq!(
        command(
            &endpoint,
            "get_teammate_topology",
            json!({"workspaceId":workspace}),
            true
        )["roster"][0]["currentDelivery"]["deliveryId"],
        sent["deliveryId"],
        "queued work must not replace the executing task projection"
    );
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
        tool(&endpoint, "ridge_create_task", second)["execution"],
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
    assert_eq!(command(&endpoint,"get_teammate_topology",json!({"workspaceId":workspace}),true)["groups"],remote_edit["groups"],"process exit must retain exact group membership");
}
