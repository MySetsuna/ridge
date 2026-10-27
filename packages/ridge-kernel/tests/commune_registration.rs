//! Actual owned Kernel/PTY/process/socket registration, no brand adapters.
use futures::StreamExt;
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

#[test]
#[ignore = "already-running protocol process fixture"]
fn existing_protocol_endpoint_fixture() { commune_fixture::run(); }

#[path = "common/commune_fixture.rs"]
mod commune_fixture;
#[path = "common/runtime_fixture.rs"]
mod runtime_fixture;

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn membership_retirement_fences_turns_preserves_process_history_and_survives_restart(){
    let root=std::env::temp_dir().join(format!("ridge-membership-{}",Uuid::new_v4()));
    let owned=runtime_fixture::start(&root).await;
    let workspace=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",Some(&json!({"name":"membership"}))).unwrap()["workspace_id"].clone();
    let pane=Uuid::new_v4();let descriptor_path=root.join("endpoint.json");
    let created=client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane,"workspace_id":workspace,"program":std::env::current_exe().unwrap(),"args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,"env":{"RIDGE_COMMUNE_FIXTURE":descriptor_path},"role":"shell","cols":80,"rows":24}))).unwrap();
    assert_eq!(created["ok"],true,"{created}");
    let deadline=Instant::now()+Duration::from_secs(10);
    while !descriptor_path.exists(){assert!(Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;}
    let descriptor:Value=serde_json::from_slice(&fs::read(&descriptor_path).unwrap()).unwrap();
    let registration=json!({"workspaceId":workspace,"paneId":pane,"process":descriptor["process"],"endpointOwner":descriptor["process"],"endpoint":descriptor["endpoint"],"sessionId":descriptor["sessionId"],"cwd":root});
    let mut socket=runtime_fixture::attach(&owned.endpoint).await;
    let first=runtime_fixture::rpc(&mut socket,1,"register_commune_member",registration.clone()).await;
    assert!(first["error"].is_null(),"{first}");
    let identity=first["result"].clone();
    let target=json!({"workspaceId":workspace,"paneId":pane,"agentId":identity["agentId"],"generation":identity["generation"],"lease":identity["lease"]});
    let hub_target=json!({"workspace_id":workspace,"agent_id":identity["agentId"],"generation":identity["generation"],"lease":identity["lease"]});
    let before=client::list_domain_ptys(&owned.endpoint).unwrap().into_iter().find(|pty|pty.pty_id==pane).unwrap();
    assert_eq!(command_reply(&owned.endpoint,"release_teammate_agent",target.clone(),false)["ok"],false);
    let mut prompt=target.clone();prompt["message"]=json!("wait-for-cancel");prompt["idempotency_key"]=json!(Uuid::new_v4());
    let receipt=command(&owned.endpoint,"send_agent_message",prompt,true);
    wait_execution(&owned.endpoint,&hub_target,&receipt["deliveryId"],"running");
    assert_eq!(command_reply(&owned.endpoint,"release_teammate_agent",target.clone(),true)["ok"],false,"active turn must not be detached");
    let mut cancel=target.clone();cancel["deliveryId"]=receipt["deliveryId"].clone();command(&owned.endpoint,"cancel_agent_message",cancel,true);
    wait_execution(&owned.endpoint,&hub_target,&receipt["deliveryId"],"cancelled");
    let roster_path=root.join("agent-roster.json");let preserved=root.join("agent-roster-preserved.json");
    fs::rename(&roster_path,&preserved).unwrap();fs::create_dir(&roster_path).unwrap();
    assert_eq!(command_reply(&owned.endpoint,"release_teammate_agent",target.clone(),true)["ok"],false,"failed persistence must not detach the live conversation");
    assert!(command(&owned.endpoint,"get_teammate_topology",json!({"workspaceId":workspace}),true)["roster"].as_array().unwrap().iter().any(|member|member["agentId"]==identity["agentId"]&&member["online"]==true));
    fs::remove_dir(&roster_path).unwrap();fs::rename(&preserved,&roster_path).unwrap();
    assert_eq!(command(&owned.endpoint,"release_teammate_agent",target.clone(),true)["removed"],true);
    assert_eq!(command(&owned.endpoint,"release_teammate_agent",target.clone(),true)["removed"],true,"duplicate removal is idempotent");
    assert!(command(&owned.endpoint,"get_teammate_topology",json!({"workspaceId":workspace}),true)["roster"].as_array().unwrap().is_empty());
    let after=client::list_domain_ptys(&owned.endpoint).unwrap().into_iter().find(|pty|pty.pty_id==pane).unwrap();
    assert_eq!(after.child_pid,before.child_pid);assert_eq!(after.child_start_instance,before.child_start_instance);
    assert!(ridge_core::process_identity::validate(&serde_json::from_value(descriptor["process"].clone()).unwrap()).is_ok());
    let history=command(&owned.endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"limit":100}),true);
    assert!(history.to_string().contains("existing history"),"removed membership must retain readable protocol history");
    let second=command(&owned.endpoint,"register_commune_member",registration,true);
    assert_eq!(second["agentId"],identity["agentId"]);assert!(second["generation"].as_u64()>identity["generation"].as_u64());
    assert_eq!(command_reply(&owned.endpoint,"release_teammate_agent",target.clone(),true)["ok"],false,"stale removal must not affect a reconnected member");
    let mut next=target;next["generation"]=second["generation"].clone();next["lease"]=second["lease"].clone();
    assert_eq!(command(&owned.endpoint,"release_teammate_agent",next,true)["removed"],true);
    drop(socket);drop(owned);
    let restarted=runtime_fixture::start(&root).await;
    assert!(command(&restarted.endpoint,"get_teammate_topology",json!({"workspaceId":workspace}),true)["roster"].as_array().unwrap().is_empty());
    assert!(command(&restarted.endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"limit":100}),true).to_string().contains("existing history"));
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
    let detail = client::request_json(&endpoint, "GET", &format!("/v1/domain/workspaces/{}", workspace.as_str().unwrap()), None).unwrap();
    let pane = Uuid::parse_str(detail["panes"][0].as_str().unwrap()).unwrap();
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
    client::write_domain_agent_pty(&endpoint,pane,b"\r").unwrap();
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
    let other = client::request_json(&endpoint, "POST", "/v1/domain/workspaces", None).unwrap()["workspace_id"].clone();
    let other_detail = client::request_json(&endpoint, "GET", &format!("/v1/domain/workspaces/{}", other.as_str().unwrap()), None).unwrap();
    let dock = command_reply(&endpoint, "dock_pane", json!({"sourcePaneId":pane,"targetPaneId":other_detail["panes"][0],"region":"right"}), true);
    assert_eq!(dock["ok"], false, "registered protocol workspace cannot silently change: {dock}");
    assert!(dock["error"]["message"].as_str().unwrap().contains("registered workspace"), "{dock}");
    assert_eq!(client::list_domain_ptys(&endpoint).unwrap().iter().find(|pty| pty.pty_id == pane).unwrap().workspace_id.unwrap().to_string(), workspace.as_str().unwrap());
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
    let runtime_identity=command(&endpoint,"get_pty_runtime_identity",json!({"workspaceId":workspace,"paneId":pane}),true);
    assert_eq!(runtime_identity["agentId"],identity.agent_id);
    assert_eq!(runtime_identity["generation"],identity.generation);
    assert_eq!(runtime_identity["lease"],identity.lease);
    assert_eq!(command_reply(&endpoint,"get_pty_runtime_identity",json!({"workspaceId":workspace,"paneId":pane}),false)["ok"],false);
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
    let remote_members = command(&endpoint, "get_teammate_topology", json!({"workspaceId":workspace}), false);
    assert_eq!(remote_members["roster"][0]["canSend"], true);
    assert_eq!(remote_members["roster"][0]["canCancel"], true);
    assert_eq!(remote_members["discovered"], json!([]), "remote method grants do not confer native process discovery");
    assert_eq!(
        command_reply(
            &endpoint,
            "get_teammate_topology",
            json!({"workspaceId":Uuid::new_v4()}),
            true
        )["ok"],
        false
    );
    let refused=client::write_domain_agent_pty(&endpoint,pane,b"must-not-inject\r").unwrap_err();
    assert!(refused.contains("existing protocol"),"{refused}");
    let retained=client::scrollback_domain_pty(&endpoint,pane,64*1024).unwrap();
    assert!(!String::from_utf8_lossy(&retained).contains("must-not-inject"),"rejected automation must not enter PTY replay");
    client::write_domain_pty(&endpoint,pane,b"\r").unwrap();
    let mut prompt = native_target.clone();
    prompt["message"] = json!("permission-needed");
    prompt["idempotency_key"] = json!("permission-prompt");
    let permission_delivery = command(&endpoint, "send_agent_message", prompt, true);
    let pending = wait_permission(&endpoint);
    assert_eq!(command(&endpoint,"get_pending_hitl_count",json!({}),true),json!(1));
    assert_eq!(command_reply(&endpoint,"get_pending_hitl_count",json!({}),false)["ok"],false);
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
    assert_eq!(command(&endpoint,"get_pending_hitl_count",json!({}),true),json!(0));
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
    let decisions=records.iter().filter(|row|row["kind"]=="permission_decided").collect::<Vec<_>>();
    assert!(!decisions.is_empty());
    assert!(decisions.iter().all(|row| row["workspaceId"]==json!(workspace)
        && matches!(row["source"].as_str(),Some("local"|"remote"))
        && matches!(row["optionKind"].as_str(),Some("allow_once"|"allow_always"|"reject_once"|"reject_always"))));
    assert!(records
        .iter()
        .any(|row| row["kind"] == "permission_decided"));
    assert!(records
        .iter()
        .any(|row| row["kind"] == "permission_cancelled"));
    let audit=command(&endpoint,"list_hitl_audit_remote",json!({"workspaceId":workspace,"limit":50}),false);
    let audited=audit["items"].as_array().unwrap();
    assert!(!audited.is_empty());
    assert!(audited.len()<=50);
    assert!(audited.iter().any(|row|row["outcome"]=="decided" && matches!(row["verdict"].as_str(),Some("approve"|"reject"))));
    assert!(audited.iter().any(|row|row["outcome"]=="cancelled"));
    assert!(audited.iter().all(|row|row["riskLevel"]=="unknown"));
    assert!(!audit.to_string().contains("private-operation"));
    assert!(!audit.to_string().contains("private-argument"));
    let local_decisions=command(&endpoint,"list_hitl_decisions",json!({"workspaceId":workspace}),true);
    assert_eq!(local_decisions.as_array().unwrap(),&audited.iter().rev().cloned().collect::<Vec<_>>());
    assert_eq!(command_reply(&endpoint,"list_hitl_decisions",json!({"workspaceId":workspace}),false)["ok"],false);
    let one=command(&endpoint,"list_hitl_audit_remote",json!({"workspaceId":workspace,"limit":1}),true);
    assert_eq!(one["items"].as_array().unwrap().len(),1);
    assert_eq!(command_reply(&endpoint,"list_hitl_audit_remote",json!({"workspaceId":Uuid::new_v4()}),true)["ok"],false);

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
    let assistants=command(&endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"limit":200,"assistantOnly":true}),true);
    let assistants=assistants.as_array().unwrap();
    assert!(!assistants.is_empty());
    assert!(assistants.iter().all(|row|matches!(row["kind"].as_str(),Some("agent_message_chunk"|"agentMessage"))));
    assert_eq!(command_reply(&endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"cursor":history_cursor,"assistantOnly":true}),true)["ok"],false,"unfiltered cursors cannot change their source filter");
    let filtered=command(&endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"limit":1,"assistantOnly":true}),true);
    if let Some(cursor)=filtered.as_array().unwrap().last().and_then(|row|row["cursor"].as_str()) {
        assert_eq!(command_reply(&endpoint,"read_agent_recent_replies",json!({"workspaceId":workspace,"cursor":cursor}),true)["ok"],false,"assistant cursors cannot become unfiltered");
    }
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
    client::shutdown_endpoint(&endpoint,Duration::from_secs(3)).unwrap();
    let stopped=Instant::now()+Duration::from_secs(5);
    while kernel.try_wait().unwrap().is_none() {
        assert!(Instant::now()<stopped,"owned Kernel shutdown deadline");
        std::thread::sleep(Duration::from_millis(20));
    }
    let restarted=runtime.block_on(runtime_fixture::start(&root));
    let durable_audit=command(&restarted.endpoint,"list_hitl_audit_remote",json!({"workspaceId":workspace,"limit":50}),false);
    assert!(audit["items"].as_array().unwrap().iter().all(|before|
        durable_audit["items"].as_array().unwrap().iter().any(|after|before==after)),"audit facts must survive service restart");
    assert!(client::list_domain_ptys(&restarted.endpoint).unwrap().is_empty(),"audit reads must not launch replacement processes");

}
