//! Owned stdio process evidence. External CLI interoperability is a separate matrix.
use futures::{SinkExt, StreamExt};
use ridge_core::process_identity;
use ridge_kernel::managed_agent::StdioAgent;
use serde_json::json;
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Message},
    MaybeTlsStream, WebSocketStream,
};
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
#[path = "common/runtime_fixture.rs"]
mod runtime_fixture;

fn command(
    endpoint: &ridge_kernel::registry::KernelEndpoint,
    method: &str,
    args: Value,
    native: bool,
) -> Value {
    ridge_kernel::client::request_json(endpoint,"POST","/v1/domain/commands",Some(&json!({"method":method,"args":args,"native":native,"roots":[],"connectionId":uuid::Uuid::new_v4()}))).unwrap()
}
async fn wait_value(mut read: impl FnMut() -> Option<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(value) = read() {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("owned collaboration Runtime deadline")
}

async fn connect(
    descriptor: &Value,
    token: &str,
) -> Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!("ws://{}/", descriptor["endpoint"].as_str().unwrap())
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {token}").parse().unwrap());
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}
async fn rpc(
    socket: &mut Socket,
    id: i64,
    method: &str,
    params: Value,
    events: &mut Vec<Value>,
) -> Value {
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string(),
        ))
        .await
        .unwrap();
    reply(socket, id, events).await
}
async fn reply(socket: &mut Socket, id: i64, events: &mut Vec<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let message=socket.next().await.unwrap().unwrap();
            if let Message::Text(text)=message {
                let value:Value=serde_json::from_str(&text).unwrap();
                if value["method"]=="session/request_permission" {
                    assert!(value["params"]["options"].as_array().unwrap().iter().any(|option|option["optionId"]=="once"));
                    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":value["id"],"result":{"outcome":{"outcome":"selected","optionId":"once"}}}).to_string())).await.unwrap();
                    events.push(value);
                } else if value["id"]==id {return value;} else {events.push(value);}
            }
        }
    }).await.unwrap_or_else(|_|panic!("RPC {id} timed out; observed {events:?}"))
}

fn fixture_binary() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let binary = target.join("debug").join(if cfg!(windows) {
        "fake_acp_agent.exe"
    } else {
        "fake_acp_agent"
    });
    assert!(
        binary.is_file(),
        "build ridge-acp --bin fake_acp_agent before this acceptance test"
    );
    binary
}

#[tokio::test]
async fn explicit_stdio_process_uses_its_actual_session_permissions_and_cancel_then_reaps() {
    let observed = Arc::new(Mutex::new(Vec::<Value>::new()));
    let updates = observed.clone();
    let approved = Arc::new(Mutex::new(Vec::<Value>::new()));
    let permissions = approved.clone();
    let agent = StdioAgent::start(
        &fixture_binary(),
        &["--standard-stdio".into()],
        &std::env::temp_dir(),
        Arc::new(move |update| {
            updates.lock().unwrap().push(update.clone());
            Ok(())
        }),
        Arc::new(move |params| {
            permissions.lock().unwrap().push(params);
            Box::pin(async { Some("once".into()) })
        }),
    )
    .await
    .unwrap();
    let process = agent.process().clone();
    process_identity::validate(&process).unwrap();
    let session = agent.session();
    assert!(session.session_id().starts_with("standard-"));
    assert_eq!(
        session.agent_info().unwrap()["name"],
        "owned-standard-agent"
    );
    let response = session
        .prompt("one owned turn".into())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response["stopReason"], "end_turn");
    assert_eq!(
        observed.lock().unwrap()[0]["sessionId"],
        session.session_id()
    );
    assert_eq!(
        observed.lock().unwrap()[0]["update"]["content"]["text"],
        "one owned turn"
    );
    session
        .prompt("permission-needed".into())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(approved.lock().unwrap().len(), 1);
    assert_eq!(
        approved.lock().unwrap()[0]["sessionId"],
        session.session_id()
    );
    let pending = session.prompt("wait-for-cancel".into()).unwrap();
    session.cancel().await.unwrap();
    assert_eq!(pending.await.unwrap().unwrap()["stopReason"], "cancelled");
    agent.stop().await.unwrap();
    assert!(process_identity::validate(&process).is_err());
}

#[tokio::test]
async fn dropping_an_owned_stdio_agent_reclaims_only_its_child_process() {
    let current = process_identity::instance(std::process::id()).unwrap();
    let agent = StdioAgent::start(
        &fixture_binary(),
        &["--standard-stdio".into()],
        &std::env::temp_dir(),
        Arc::new(|_| Ok(())),
        Arc::new(|_| Box::pin(async { None })),
    )
    .await
    .unwrap();
    let child = agent.process().clone();
    drop(agent);
    tokio::time::timeout(Duration::from_secs(2), async {
        while process_identity::validate(&child).is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    process_identity::validate(&current).unwrap();
}

#[tokio::test]
async fn cancelling_startup_reclaims_its_own_process_before_session_creation_finishes() {
    let root = std::env::temp_dir().join(format!("ridge-cancel-startup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let pid_file = root.join("owned-child.pid");
    let cwd = root.clone();
    let report = pid_file.clone();
    let startup = tokio::spawn(async move {
        ridge_kernel::managed_agent::CollaborationTerminal::start(
            "acp",
            &fixture_binary(),
            &[
                "--standard-stdio".into(),
                "--stall-create".into(),
                "--pid-file".into(),
                report.to_string_lossy().into_owned(),
            ],
            &cwd,
            Arc::new(|_| Ok(())),
            Arc::new(|_| Box::pin(async { None })),
            None,
        )
        .await
    });
    let report = wait_value(|| {
        std::fs::read_to_string(&pid_file)
            .ok()?
            .parse::<u32>()
            .ok()
            .map(|pid| json!(pid))
    })
    .await;
    let pid = report.as_u64().unwrap() as u32;
    let child = process_identity::instance(pid).unwrap();
    startup.abort();
    assert!(matches!(startup.await,Err(error) if error.is_cancelled()));
    wait_value(|| {
        process_identity::validate(&child)
            .is_err()
            .then(|| json!(true))
    })
    .await;
    std::fs::remove_file(pid_file).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[tokio::test]
async fn collaboration_bridge_authenticates_replays_and_reconnects_to_the_same_owned_session() {
    let root = std::env::temp_dir().join(format!("ridge-managed-agent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    // This integration process never uses a user's Runtime or registry.
    std::env::set_var("RIDGE_KERNEL_DATA_DIR", &root);
    let terminal = ridge_kernel::managed_agent::CollaborationTerminal::start(
        "acp",
        &fixture_binary(),
        &["--standard-stdio".into()],
        &root,
        Arc::new(|_| Ok(())),
        Arc::new(|_| Box::pin(async { None })),
        None,
    )
    .await
    .unwrap();
    let process = terminal.process().clone();
    let descriptor_path = terminal.descriptor_path().to_owned();
    let descriptor: Value =
        serde_json::from_slice(&std::fs::read(&descriptor_path).unwrap()).unwrap();
    let token = std::fs::read_to_string(descriptor["bearerTokenFile"].as_str().unwrap()).unwrap();
    let session = terminal.session();
    let identity = session.session_id().to_owned();
    let refused = connect(&descriptor, "wrong-token").await.err().unwrap();
    assert!(
        matches!(refused,tokio_tungstenite::tungstenite::Error::Http(response) if response.status()==401)
    );
    assert_eq!(session.session_id(), identity);
    session
        .prompt("before attachment".into())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let mut socket = connect(&descriptor, &token).await.unwrap();
    let mut events = Vec::new();
    assert_eq!(
        rpc(
            &mut socket,
            1,
            "initialize",
            json!({"protocolVersion":1}),
            &mut events
        )
        .await["result"]["agentCapabilities"]["loadSession"],
        true
    );
    assert!(rpc(
        &mut socket,
        20,
        "session/load",
        json!({"sessionId":identity,"cwd":root.join("another-directory")}),
        &mut events
    )
    .await["error"]
        .is_object());
    assert!(rpc(
        &mut socket,
        2,
        "session/load",
        json!({"sessionId":identity,"cwd":root,"mcpServers":[]}),
        &mut events
    )
    .await["error"]
        .is_null());
    assert!(events
        .iter()
        .any(|event| event["params"]["update"]["content"]["text"] == "before attachment"));
    assert!(rpc(
        &mut socket,
        3,
        "session/new",
        json!({"cwd":root}),
        &mut events
    )
    .await["error"]
        .is_object());
    let mut other = connect(&descriptor, &token).await.unwrap();
    rpc(
        &mut other,
        1,
        "initialize",
        json!({"protocolVersion":1}),
        &mut Vec::new(),
    )
    .await;
    assert!(rpc(
        &mut other,
        2,
        "session/load",
        json!({"sessionId":identity,"cwd":root}),
        &mut Vec::new()
    )
    .await["error"]
        .is_object());
    other.close(None).await.unwrap();
    let completed = rpc(
        &mut socket,
        4,
        "session/prompt",
        json!({"sessionId":identity,"prompt":[{"type":"text","text":"permission-needed"}]}),
        &mut events,
    )
    .await;
    assert_eq!(completed["result"]["stopReason"], "end_turn");
    assert!(events
        .iter()
        .any(|event| event["method"] == "session/request_permission"));
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":5,"method":"session/prompt","params":{"sessionId":identity,"prompt":[{"type":"text","text":"wait-for-cancel"}]}}).to_string())).await.unwrap();
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":identity}})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        reply(&mut socket, 5, &mut events).await["result"]["stopReason"],
        "cancelled"
    );
    socket.close(None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    process_identity::validate(&process).unwrap();
    session
        .prompt("after disconnect".into())
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    let mut reconnect = connect(&descriptor, &token).await.unwrap();
    rpc(
        &mut reconnect,
        1,
        "initialize",
        json!({"protocolVersion":1}),
        &mut events,
    )
    .await;
    events.clear();
    assert!(rpc(
        &mut reconnect,
        2,
        "session/load",
        json!({"sessionId":identity,"cwd":root}),
        &mut events
    )
    .await["error"]
        .is_null());
    assert!(events
        .iter()
        .any(|event| event["params"]["update"]["content"]["text"] == "after disconnect"));
    assert_eq!(session.session_id(), identity);
    drop(terminal);
    tokio::time::timeout(Duration::from_secs(2), async {
        while process_identity::validate(&process).is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!descriptor_path.exists());
    assert!(!PathBuf::from(descriptor["bearerTokenFile"].as_str().unwrap()).exists());
    std::fs::remove_dir(root.join("agent-connections")).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_collaboration_terminal_retries_without_recreation_and_keeps_group_and_agent() {
    use ridge_kernel::client;
    let root = std::env::temp_dir().join(format!("ridge-managed-runtime-{}", uuid::Uuid::new_v4()));
    let owned = runtime_fixture::start(&root).await;
    assert_eq!(
        command(
            &owned.endpoint,
            "get_collaboration_launch_options",
            json!({}),
            false
        )["ok"],
        false
    );
    let preview = command(
        &owned.endpoint,
        "get_collaboration_launch_options",
        json!({}),
        true,
    );
    assert_eq!(preview["ok"], true, "{preview}");
    assert!(!root.join("agent-connections").exists());
    let workspace = json!(uuid::Uuid::new_v4());
    let pane = json!(uuid::Uuid::new_v4());
    let request = json!({"workspaceId":workspace,"paneId":pane,"runtimeEpoch":preview["value"]["runtimeEpoch"],
        "entry":"agent","cwd":root,"collaboration":{"command":fixture_binary(),"arguments":["--standard-stdio"]}});
    assert_eq!(
        command(
            &owned.endpoint,
            "create_independent_terminal",
            request.clone(),
            false
        )["ok"],
        false
    );
    let mut stale = request.clone();
    stale["runtimeEpoch"] = json!("old-runtime");
    assert_eq!(
        command(&owned.endpoint, "create_independent_terminal", stale, true)["ok"],
        false
    );
    assert!(client::list_domain_ptys(&owned.endpoint)
        .unwrap()
        .is_empty());
    let created = command(
        &owned.endpoint,
        "create_independent_terminal",
        request.clone(),
        true,
    );
    assert_eq!(created["ok"], true, "{created}");
    let original = client::list_domain_ptys(&owned.endpoint).unwrap();
    let repeated = command(
        &owned.endpoint,
        "create_independent_terminal",
        request.clone(),
        true,
    );
    assert_eq!(created["value"], repeated["value"]);
    assert_eq!(
        client::list_domain_ptys(&owned.endpoint).unwrap()[0].child_pid,
        original[0].child_pid
    );
    let mut changed = request.clone();
    changed["collaboration"]["arguments"] = json!(["--standard-stdio", "changed"]);
    assert_eq!(
        command(
            &owned.endpoint,
            "create_independent_terminal",
            changed,
            true
        )["ok"],
        false
    );
    let candidate = wait_value(|| {
        let projection = command(
            &owned.endpoint,
            "get_teammate_topology",
            json!({"workspaceId":workspace}),
            true,
        );
        projection["value"]["discovered"]
            .as_array()?
            .iter()
            .find(|candidate| candidate["connectionReady"] == true)
            .cloned()
    })
    .await;
    let selected =
        json!({"candidateRef":candidate["candidateRef"],"workspaceId":workspace,"paneId":pane});
    assert_eq!(
        command(
            &owned.endpoint,
            "prepare_commune_connection",
            selected.clone(),
            false
        )["ok"],
        false
    );
    let prepared = command(
        &owned.endpoint,
        "prepare_commune_connection",
        selected.clone(),
        true,
    );
    assert_eq!(prepared["ok"], true, "{prepared}");
    assert_eq!(prepared["value"]["protocol"], "acp-ws");
    assert!(prepared["value"].get("bearerTokenFile").is_none());
    assert!(!root.join("agent-roster.json").exists());
    let enrolled = command(
        &owned.endpoint,
        "set_teammate_groups",
        json!({"groupScope":"host","expectedRevision":0,"groups":[{"id":"terminal-first","name":"Terminal first","color":"#60a5fa","memberAgentIds":[],"createdAt":1}],"enrollments":[{"candidateRef":candidate["candidateRef"],"groupId":"terminal-first"}]}),
        true,
    );
    assert_eq!(enrolled["ok"], true, "{enrolled}");
    let before = std::fs::read(root.join("commune-groups.json")).unwrap();
    let organization = command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"groupScope":"host"}),
        true,
    );
    let agent_id = organization["value"]["members"][0]["agentId"].clone();
    let attached = command(
        &owned.endpoint,
        "register_commune_member",
        selected.clone(),
        true,
    );
    assert_eq!(attached["ok"], true, "{attached}");
    assert_eq!(attached["value"]["agentId"], agent_id);
    assert_eq!(
        std::fs::read(root.join("commune-groups.json")).unwrap(),
        before
    );
    let identity = &attached["value"];
    let mut target = json!({"workspaceId":workspace,"paneId":pane,"agentId":agent_id,"generation":identity["generation"],"lease":identity["lease"]});
    let mut prompt = target.clone();
    prompt["message"] = json!("permission-needed");
    prompt["idempotency_key"] = json!(uuid::Uuid::new_v4());
    let delivery = command(&owned.endpoint, "send_agent_message", prompt, true);
    assert_eq!(delivery["ok"], true, "{delivery}");
    let permission = wait_value(|| {
        command(&owned.endpoint, "list_hitl_pending_local", json!({}), true)["value"]
            .as_array()?
            .first()
            .cloned()
    })
    .await;
    assert_eq!(permission["agentId"], agent_id);
    assert_eq!(
        command(
            &owned.endpoint,
            "resolve_hitl_request",
            json!({"id":permission["id"],"optionId":"once"}),
            true
        )["value"],
        true
    );
    wait_value(|| {
        let mut args = target.clone();
        args["deliveryId"] = delivery["value"]["deliveryId"].clone();
        let status = command(
            &owned.endpoint,
            "get_agent_message_delivery_status",
            args,
            true,
        );
        assert_eq!(status["ok"], true, "{status}");
        (status["value"]["execution"]["state"] == "turn_ended").then(|| status["value"].clone())
    })
    .await;
    for (message, expected_state) in [("auth-needed", "rejected"), ("prompt-error", "unknown")] {
        let mut prompt = target.clone();
        prompt["message"] = json!(message);
        prompt["idempotency_key"] = json!(uuid::Uuid::new_v4());
        let delivery = command(&owned.endpoint, "send_agent_message", prompt, true);
        assert_eq!(delivery["ok"], true, "{delivery}");
        let mut args = target.clone();
        args["deliveryId"] = delivery["value"]["deliveryId"].clone();
        let receipt = wait_value(|| {
            let status = command(&owned.endpoint, "get_agent_message_delivery_status", args.clone(), true);
            let execution = &status["value"]["execution"];
            matches!(execution["state"].as_str(), Some("rejected" | "unknown")).then(|| execution.clone())
        }).await;
        assert_eq!(receipt["state"], expected_state, "{receipt}");
        if message == "auth-needed" {
            assert_eq!(receipt["reason"], "agent_authentication_required");
            assert!(!receipt.to_string().contains("private-provider-detail"));
        }
    }
    assert_eq!(
        command(
            &owned.endpoint,
            "release_teammate_agent",
            target.clone(),
            true
        )["value"]["removed"],
        true
    );
    let child: process_identity::ProcessInstance =
        serde_json::from_value(candidate["process"].clone()).unwrap();
    process_identity::validate(&child).unwrap();
    let pty = uuid::Uuid::parse_str(pane.as_str().unwrap()).unwrap();
    client::write_domain_pty(&owned.endpoint, pty, b"local after disconnect\r").unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    let reattached = command(&owned.endpoint, "register_commune_member", selected, true);
    assert_eq!(reattached["ok"], true, "{reattached}");
    assert_eq!(reattached["value"]["agentId"], agent_id);
    assert_eq!(reattached["value"]["sessionId"], identity["sessionId"]);
    assert_eq!(
        std::fs::read(root.join("commune-groups.json")).unwrap(),
        before
    );
    target["generation"] = reattached["value"]["generation"].clone();
    target["lease"] = reattached["value"]["lease"].clone();
    assert_eq!(
        command(&owned.endpoint, "release_teammate_agent", target, true)["value"]["removed"],
        true
    );
    client::destroy_domain_pty(&owned.endpoint, pty).unwrap();
    wait_value(|| {
        process_identity::validate(&child)
            .is_err()
            .then(|| json!(true))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_cli_creates_a_runtime_terminal_and_controller_exit_preserves_its_agent() {
    use ridge_kernel::client;
    use std::process::Stdio;
    use tokio::process::Command;
    let root = std::env::temp_dir().join(format!("ridge-external-agent-{}", uuid::Uuid::new_v4()));
    let owned = runtime_fixture::start(&root).await;
    let cli = fixture_binary().with_file_name(if cfg!(windows) { "ridge.exe" } else { "ridge" });
    let mut controller = Command::new(cli)
        .args(["agent", "--command"])
        .arg(fixture_binary())
        .arg("--cwd")
        .arg(&root)
        .args(["--", "--standard-stdio"])
        .env("RIDGE_KERNEL_DATA_DIR", &root)
        .env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let terminal = wait_value(|| {
        let ptys = client::list_domain_ptys(&owned.endpoint).unwrap();
        let pty = ptys.first()?;
        let workspace = pty.workspace_id?;
        let topology = command(
            &owned.endpoint,
            "get_teammate_topology",
            json!({"workspaceId":workspace}),
            true,
        );
        let candidate = topology["value"]["discovered"]
            .as_array()?
            .iter()
            .find(|candidate| candidate["connectionReady"] == true)?;
        Some(json!({"workspaceId":workspace,"paneId":pty.id,"process":candidate["process"]}))
    })
    .await;
    let process: process_identity::ProcessInstance =
        serde_json::from_value(terminal["process"].clone()).unwrap();
    controller.kill().await.unwrap();
    controller.wait().await.unwrap();
    process_identity::validate(&process).unwrap();
    assert_eq!(client::list_domain_ptys(&owned.endpoint).unwrap().len(), 1);
    let pane = uuid::Uuid::parse_str(terminal["paneId"].as_str().unwrap()).unwrap();
    client::write_domain_pty(&owned.endpoint, pane, b"/exit\r").unwrap();
    wait_value(|| {
        process_identity::validate(&process)
            .is_err()
            .then(|| json!(true))
    })
    .await;
    let exited = client::request_json(&owned.endpoint, "GET", "/v1/domain/ptys", None).unwrap();
    assert_eq!(
        exited["ptys"][0]["launch_profile"]
            .as_str()
            .unwrap()
            .split(':')
            .next(),
        Some("ridge-collaboration")
    );
    drop(owned);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn piped_cli_line_endings_reach_the_owned_console_and_report_its_real_exit() {
    use ridge_kernel::client;
    use std::process::Stdio;
    use tokio::{io::AsyncWriteExt, process::Command};
    let root = std::env::temp_dir().join(format!("ridge-piped-agent-{}", uuid::Uuid::new_v4()));
    let owned = runtime_fixture::start(&root).await;
    let cli = fixture_binary().with_file_name(if cfg!(windows) { "ridge.exe" } else { "ridge" });
    let mut child = Command::new(cli)
        .args(["agent", "--command"])
        .arg(fixture_binary())
        .arg("--cwd")
        .arg(&root)
        .args(["--", "--standard-stdio"])
        .env("RIDGE_KERNEL_DATA_DIR", &root)
        .env("RIDGE_AUTH_FILE", root.join("auth.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\r\n/exit\n")
        .await
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(15), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Session "));
    let ptys = client::list_domain_ptys(&owned.endpoint).unwrap();
    assert_eq!(ptys.len(), 1);
    assert_eq!(ptys[0].status, "Exited");
    assert!(std::fs::read_dir(root.join("agent-connections"))
        .unwrap()
        .next()
        .is_none());
    drop(owned);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_collaboration_startup_retains_its_terminal_and_retry_does_not_spawn_again() {
    use ridge_kernel::client;
    let root = std::env::temp_dir().join(format!("ridge-failed-agent-{}", uuid::Uuid::new_v4()));
    let owned = runtime_fixture::start(&root).await;
    let preview = command(
        &owned.endpoint,
        "get_collaboration_launch_options",
        json!({}),
        true,
    );
    let request = json!({"workspaceId":uuid::Uuid::new_v4(),"paneId":uuid::Uuid::new_v4(),
        "runtimeEpoch":preview["value"]["runtimeEpoch"],"entry":"agent","cwd":root,
        "collaboration":{"command":root.join("missing-agent.exe"),"arguments":[]}});
    let created = command(
        &owned.endpoint,
        "create_independent_terminal",
        request.clone(),
        true,
    );
    assert_eq!(created["ok"], true, "{created}");
    let exited = wait_value(|| {
        let snapshot =
            client::request_json(&owned.endpoint, "GET", "/v1/domain/ptys", None).unwrap();
        let pty = &snapshot["ptys"][0];
        (pty["status"] == "Exited" && pty["exit_code"] == 1).then(|| pty.clone())
    })
    .await;
    let retried = command(
        &owned.endpoint,
        "create_independent_terminal",
        request.clone(),
        true,
    );
    assert_eq!(created["value"], retried["value"]);
    assert_eq!(
        client::list_domain_ptys(&owned.endpoint).unwrap()[0].child_pid,
        exited["child_pid"].as_u64().map(|pid| pid as u32)
    );
    let topology = command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"workspaceId":request["workspaceId"]}),
        true,
    );
    assert!(topology["value"]["roster"].as_array().unwrap().is_empty());
    assert!(!root.join("agent-connections").exists());
    drop(owned);
    let _ = std::fs::remove_dir_all(root);
}
