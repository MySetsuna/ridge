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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn paired_lan_client_operates_existing_commune_session() {
    use cloud_ws::tungstenite::{
        client::IntoClientRequest as LanRequest, Error as WsError, Message,
    };
    use futures::SinkExt;
    use std::sync::Arc;

    let root = std::env::temp_dir().join(format!("ridge-lan-commune-{}", Uuid::new_v4()));
    let owned = runtime_fixture::start_with_env(
        &root,
        &[
            ("RIDGE_TEST_TLS_DIR", root.join("tls")),
            ("RIDGE_TEST_ALLOW_NON_BREAKAWAY", "1".into()),
        ],
    )
    .await;
    let endpoint = &owned.endpoint;
    let workspace = client::request_json(endpoint, "POST", "/v1/domain/workspaces", None).unwrap()
        ["workspace_id"]
        .clone();
    let graph = client::request_json(
        endpoint,
        "GET",
        &format!("/v1/domain/workspaces/{}", workspace.as_str().unwrap()),
        None,
    )
    .unwrap();
    let pane = &graph["panes"][0];
    let descriptor = root.join("existing-endpoint.json");
    let created = client::request_json(
        endpoint,
        "POST",
        "/v1/domain/ptys",
        Some(&json!({
            "pty_id":pane,"workspace_id":workspace,"program":std::env::current_exe().unwrap(),
            "args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,
            "env":{"RIDGE_COMMUNE_FIXTURE":descriptor},"role":"shell"
        })),
    )
    .unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let mut source: Value = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(bytes) = fs::read(&descriptor) {
                if let Ok(source) = serde_json::from_slice(&bytes) {
                    break source;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("ACP fixture descriptor");
    source["workspaceId"] = workspace.clone();
    source["paneId"] = pane.clone();
    source["cwd"] = json!(root);
    source["endpointOwner"] = source["process"].clone();
    let attached = command(endpoint, "register_commune_member", source, true);
    assert_eq!(attached["sessionId"], "fixture-existing");

    let mut owner = runtime_fixture::attach(endpoint).await;
    let enabled = runtime_fixture::rpc(
        &mut owner,
        1,
        "set_remote_enabled",
        json!({"enabled":true,"port":0}),
    )
    .await;
    assert!(enabled["error"].is_null(), "{enabled}");
    let info =
        runtime_fixture::rpc(&mut owner, 2, "get_remote_info", json!({})).await["result"].clone();
    assert_eq!(info["tls"], true);
    let addr = format!("127.0.0.1:{}", info["port"]);
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(fs::read(root.join("tls/ca.der")).unwrap().into())
        .unwrap();
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let http = reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .use_preconfigured_tls(tls.clone())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let device = "commune-lan-acceptance";
    let paired: Value = http
        .post(format!("https://{addr}/verify"))
        .form(&[
            ("code", info["totpCode"].as_str().unwrap()),
            ("device", device),
        ])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(paired["success"], true);
    let token = paired["token"].as_str().unwrap();
    let connect = |device: &str| {
        let url = format!("wss://{addr}/ws?device={device}");
        let mut request = LanRequest::into_client_request(url).unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        tokio::time::timeout(
            Duration::from_secs(5),
            cloud_ws::connect_async_tls_with_config(
                request,
                None,
                false,
                Some(cloud_ws::Connector::Rustls(Arc::new(tls.clone()))),
            ),
        )
    };
    assert!(
        matches!(connect("wrong-device").await.unwrap(), Err(WsError::Http(reply)) if reply.status() == 401)
    );
    let (mut socket, _) = connect(device)
        .await
        .unwrap()
        .expect("authenticated LAN WebSocket");
    let mut id = 0_u64;
    macro_rules! rpc {
        ($method:literal, $params:expr) => {{
            id += 1;
            socket.send(Message::Text(json!({
                "jsonrpc":"2.0","id":id,"method":$method,"params":$params
            }).to_string())).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Message::Text(frame) = socket.next().await.unwrap().unwrap() {
                        assert!(!frame.contains("private-argument"), "LAN leaked local permission payload");
                        let reply: Value = serde_json::from_str(&frame).unwrap();
                        if reply["id"] == id {
                            break reply;
                        }
                    }
                }
            }).await.expect($method)
        }};
    }
    assert_eq!(
        rpc!("switch_workspace", json!({"workspaceId":workspace}))["result"]["success"],
        true
    );
    let topology = rpc!("get_teammate_topology", json!({"workspaceId":workspace}));
    let member = &topology["result"]["roster"][0];
    assert_eq!(member["agentId"], attached["agentId"], "{topology}");
    assert_eq!(member["sessionId"], "fixture-existing");
    assert_eq!(member["canSend"], true);
    assert_eq!(topology["result"]["discovered"], json!([]));
    let history = rpc!(
        "read_agent_recent_replies",
        json!({"workspaceId":workspace,"limit":20})
    );
    assert!(
        history["result"].to_string().contains("existing history"),
        "{history}"
    );
    assert!(rpc!("list_hitl_pending_local", json!({}))["error"].is_object());

    let host_groups = rpc!("get_teammate_topology", json!({"groupScope":"host"}));
    let group = json!({"id":"lan-group","name":"LAN group","color":"#60a5fa",
        "memberAgentIds":[member["agentId"]],"createdAt":1});
    let edit = rpc!(
        "set_teammate_groups",
        json!({"groupScope":"host",
        "expectedRevision":host_groups["result"]["groupsRevision"],"groups":[group]})
    );
    let revision = &edit["result"]["groups"][0]["revision"];
    assert!(revision.is_number(), "{edit}");
    let dispatch = json!({"groupScope":"host","groupId":"lan-group","groupRevision":revision,
        "groupAction":"broadcast","message":"permission-needed","idempotency_key":"lan-group-prompt",
        "targets":[{"agentId":member["agentId"],"generation":member["generation"],"lease":member["lease"]}]});
    let receipt = rpc!("send_agent_message", &dispatch);
    let delivery = &receipt["result"]["children"][0]["deliveryId"];
    assert!(delivery.is_string(), "{receipt}");
    assert_eq!(receipt["result"]["groupId"], "lan-group");
    assert_eq!(
        receipt["result"]["children"][0]["member"]["agentId"],
        member["agentId"]
    );
    let retry = rpc!("send_agent_message", &dispatch);
    assert_eq!(
        retry["result"]["groupTaskId"],
        receipt["result"]["groupTaskId"]
    );
    assert_eq!(retry["result"]["children"][0]["deliveryId"], *delivery);

    let pending = wait_permission(endpoint);
    let projection = rpc!("list_hitl_pending", json!({}));
    let approval = &projection["result"][0];
    assert_eq!(
        rpc!("list_hitl_pending", json!({"workspaceId":workspace}))["result"],
        projection["result"]
    );
    assert_eq!(
        rpc!("list_hitl_pending", json!({"workspaceId":Uuid::new_v4()}))["result"],
        json!([])
    );
    assert_eq!(
        rpc!("get_orchestration_health", json!({"workspaceId":workspace}))["result"]["pendingHitl"],
        1
    );
    assert_eq!(approval["id"], pending["id"], "{projection}");
    assert!(approval["toolCall"].is_null() && approval["options"].is_null());
    let mut decision = json!({"id":approval["id"],"nonce":"wrong","verdict":"approve"});
    assert_eq!(
        rpc!("resolve_hitl_remote", &decision)["result"]["outcome"],
        "nonce-mismatch"
    );
    decision["nonce"] = approval["resolutionNonce"].clone();
    assert_eq!(
        rpc!("resolve_hitl_remote", &decision)["result"]["outcome"],
        "consumed"
    );
    assert_eq!(
        rpc!("resolve_hitl_remote", &decision)["result"]["outcome"],
        "already-resolved"
    );
    wait_execution(
        endpoint,
        &json!({"workspace_id":workspace,"agent_id":member["agentId"],
        "generation":member["generation"],"lease":member["lease"]}),
        delivery,
        "turn_ended",
    );
    let decisions = fs::read_to_string(root.join("permission-decisions")).unwrap();
    assert_eq!(decisions.lines().count(), 1);
    assert!(decisions.contains("\"optionId\":\"once\""));

    let mut outputs_request = dispatch.clone();
    outputs_request["message"] = json!("produce-outputs");
    outputs_request["idempotency_key"] = json!("lan-group-outputs");
    let outputs = rpc!("send_agent_message", &outputs_request);
    wait_execution(
        endpoint,
        &json!({"workspace_id":workspace,"agent_id":member["agentId"],
        "generation":member["generation"],"lease":member["lease"]}),
        &outputs["result"]["children"][0]["deliveryId"],
        "turn_ended",
    );
    let task = json!({"groupScope":"host","groupTaskId":outputs["result"]["groupTaskId"]});
    let native = command(
        endpoint,
        "get_agent_message_delivery_status",
        task.clone(),
        true,
    );
    assert!(native["children"][0]["artifacts"]
        .to_string()
        .contains("urn:fixture:result"));
    let status = rpc!("get_agent_message_delivery_status", &task);
    let replay = rpc!("send_agent_message", &outputs_request);
    for projection in [status, replay] {
        let artifacts = projection["result"]["children"][0]["artifacts"]
            .as_array()
            .unwrap();
        assert_eq!(artifacts.len(), 3);
        for artifact in artifacts {
            assert!(
                artifact["uri"].is_null() && artifact["textCursor"].is_null(),
                "{artifact}"
            );
        }
    }

    let sessions = runtime_fixture::rpc(&mut owner, 3, "list_remote_sessions", json!({})).await;
    let session = &sessions["result"][0];
    assert_eq!(session["deviceId"], device);
    let revoked = runtime_fixture::rpc(
        &mut owner,
        4,
        "disconnect_session",
        json!({"id":session["id"]}),
    )
    .await;
    assert_eq!(revoked["result"], true, "{revoked}");
    assert!(
        matches!(connect(device).await.unwrap(), Err(WsError::Http(reply)) if reply.status() == 401)
    );
}

#[cfg(any(unix, windows))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unix_endpoint_requires_the_actual_socket_peer_before_protocol_data() {
    let root = std::env::temp_dir().join(format!(
        "ridge-uds-{}",
        &Uuid::new_v4().simple().to_string()[..8]
    ));
    let owned = runtime_fixture::start(&root).await;
    let mut sources = Vec::new();
    for id in ["one", "two"] {
        let workspace = client::request_json(
            &owned.endpoint,
            "POST",
            "/v1/domain/workspaces",
            Some(&json!({"name":id})),
        )
        .unwrap()["workspace_id"]
            .clone();
        let graph = client::request_json(
            &owned.endpoint,
            "GET",
            &format!("/v1/domain/workspaces/{}", workspace.as_str().unwrap()),
            None,
        )
        .unwrap();
        let pane = &graph["panes"][0];
        let descriptor = root.join(format!("{id}.json"));
        let socket = root.join(format!("{id}.sock"));
        let created = client::request_json(
            &owned.endpoint,
            "POST",
            "/v1/domain/ptys",
            Some(&json!({
                "pty_id":pane,"workspace_id":workspace,"program":std::env::current_exe().unwrap(),
                "args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,
                "env":{"RIDGE_COMMUNE_FIXTURE":descriptor,"RIDGE_COMMUNE_FIXTURE_SOCKET":socket,
                    "RIDGE_COMMUNE_LIST_SESSIONS":"1"},"role":"shell"
            })),
        )
        .unwrap();
        assert_eq!(created["ok"], true, "{created}");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut source: Value = loop {
            if let Ok(bytes) = fs::read(&descriptor) {
                if let Ok(source) = serde_json::from_slice(&bytes) {
                    break source;
                }
            }
            assert!(Instant::now() < deadline, "Unix socket fixture unavailable");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        source["workspaceId"] = workspace;
        source["paneId"] = pane.clone();
        source["cwd"] = json!(root);
        source["endpointOwner"] = source["process"].clone();
        sources.push(source);
    }
    let mut wrong = sources[0].clone();
    wrong["endpoint"] = sources[1]["endpoint"].clone();
    let rejected = command_reply(&owned.endpoint, "register_commune_member", wrong, true);
    assert_eq!(rejected["ok"], false);
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unix socket peer"),
        "{rejected}"
    );
    assert!(!root.join("session-operations").exists());
    assert!(!root.join("agent-roster.json").exists());
    let attached = command(
        &owned.endpoint,
        "register_commune_member",
        sources.remove(0),
        true,
    );
    assert_eq!(attached["sessionId"], "fixture-existing");
    assert!(attached["binding"]["endpoint"]
        .as_str()
        .unwrap()
        .starts_with("unix://"));
    let operations = fs::read_to_string(root.join("session-operations")).unwrap();
    assert_eq!(operations.lines().count(), 2);
    assert!(operations.contains("session/load"));
    drop(owned);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listing_requires_an_explicit_current_session_choice_from_the_exact_process() {
    use sha2::{Digest, Sha256};

    let root = std::env::temp_dir().join(format!("ridge-session-choice-{}", Uuid::new_v4()));
    let owned = runtime_fixture::start(&root).await;
    let workspace = client::request_json(
        &owned.endpoint,
        "POST",
        "/v1/domain/workspaces",
        Some(&json!({"name":"session choice"})),
    )
    .unwrap()["workspace_id"]
        .clone();
    let graph = client::request_json(
        &owned.endpoint,
        "GET",
        &format!("/v1/domain/workspaces/{}", workspace.as_str().unwrap()),
        None,
    )
    .unwrap();
    let pane = &graph["panes"][0];
    let descriptor_path = root.join("endpoint.json");
    let created = client::request_json(&owned.endpoint, "POST", "/v1/domain/ptys", Some(&json!({
        "pty_id":pane,"workspace_id":workspace,"program":std::env::current_exe().unwrap(),
        "args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,
        "env":{"RIDGE_COMMUNE_FIXTURE":descriptor_path,"RIDGE_COMMUNE_LIST_SESSIONS":"1"},"role":"shell"
    }))).unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let deadline = Instant::now() + Duration::from_secs(10);
    let endpoint: Value = loop {
        if let Ok(bytes) = fs::read(&descriptor_path) {
            if let Ok(endpoint) = serde_json::from_slice(&bytes) {
                break endpoint;
            }
        }
        assert!(Instant::now() < deadline, "fixture endpoint unavailable");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let process = &endpoint["process"];
    let digest = format!(
        "{:x}",
        Sha256::digest(process["startInstance"].as_str().unwrap().as_bytes())
    );
    let registry = root.join("agent-connections");
    fs::create_dir(&registry).unwrap();
    let published = registry.join(format!("{}-{digest}.json", process["pid"]));
    let descriptor = json!({
        "agentName":"multi-session-fixture","protocol":"acp","process":process,"endpointOwner":process,
        "endpoint":endpoint["endpoint"],"cwd":root,
    });
    let publish = |value: &Value| fs::write(&published, serde_json::to_vec(value).unwrap()).unwrap();
    let source = json!({"workspaceId":workspace,"paneId":pane});
    let mut unowned = descriptor.clone();
    unowned["sessionId"] = json!("fixture-existing");
    unowned["endpointOwner"] = json!(ridge_core::process_identity::instance(std::process::id()).unwrap());
    publish(&unowned);
    let denied = command_reply(&owned.endpoint, "prepare_commune_connection", source.clone(), true);
    assert_eq!(denied["ok"], false, "{denied}");
    assert!(denied.to_string().contains("exact pane process tree"), "{denied}");

    let unrelated = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    unowned["endpointOwner"] = process.clone();
    unowned["endpoint"] = json!(unrelated.local_addr().unwrap().to_string());
    publish(&unowned);
    let denied = command_reply(&owned.endpoint, "prepare_commune_connection", source.clone(), true);
    assert_eq!(denied["ok"], false, "the actual socket must belong to the claimed process: {denied}");

    unowned["endpoint"] = endpoint["endpoint"].clone();
    publish(&unowned);
    let verified = command(&owned.endpoint, "prepare_commune_connection", source.clone(), true);
    assert!(verified["sessionRef"].is_string(), "{verified}");
    assert!(!root.join("session-operations").exists(), "ownership preview must not send an ACP request");
    publish(&descriptor);
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "prepare_commune_connection",
            source.clone(),
            false
        )["ok"],
        false
    );
    assert!(!root.join("session-operations").exists());
    let preview = command(&owned.endpoint, "prepare_commune_connection", source, true);
    let choices = preview["sessionChoices"].as_array().unwrap();
    assert_eq!(
        choices
            .iter()
            .map(|choice| choice["sessionId"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["fixture-existing", "fixture-alternative"]
    );
    assert!(preview["sessionId"].is_null());
    let log = || fs::read_to_string(root.join("session-operations")).unwrap();
    assert_eq!(log().lines().count(), 3);
    assert!(!root.join("agent-roster.json").exists());
    let mut registration =
        json!({"candidateRef":preview["candidateRef"],"workspaceId":workspace,"paneId":pane});
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "register_commune_member",
            registration.clone(),
            true
        )["ok"],
        false
    );
    assert_eq!(
        log().lines().count(),
        3,
        "missing choice must not open a protocol client"
    );
    registration["sessionRef"] = json!("stale-or-forged");
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "register_commune_member",
            registration.clone(),
            true
        )["ok"],
        false
    );
    assert!(!log().contains("session/load"));
    assert!(!root.join("agent-roster.json").exists());
    registration["sessionRef"] = choices[1]["sessionRef"].clone();
    let identity = command(
        &owned.endpoint,
        "register_commune_member",
        registration,
        true,
    );
    assert_eq!(identity["sessionId"], "fixture-alternative");
    let operations = log();
    assert_eq!(
        operations
            .lines()
            .filter(|line| line.starts_with("session/load"))
            .collect::<Vec<_>>(),
        ["session/load \"fixture-alternative\""]
    );
    assert!(!operations.contains("session/new"));
    assert!(!operations.contains("session/prompt"));
    drop(owned);
    let _ = fs::remove_dir_all(root);
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn organization_enrollment_is_atomic_reuses_protocol_identity_and_survives_restart() {
    let root = std::env::temp_dir().join(format!("ridge-organization-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let groups_path = root.join("commune-groups.json");
    let old = br#"{"version":2,"workspaces":{},"host":{"revision":0,"groups":[]}}"#;
    fs::write(&groups_path, old).unwrap();
    let owned = runtime_fixture::start(&root).await;
    let binary = std::env::current_exe().unwrap();
    command(
        &owned.endpoint,
        "save_agent_profile_overrides",
        json!({"overrides":[{"id":"same-agent","processNames":[binary.file_stem().unwrap().to_string_lossy()],"executable":binary,"resumeArgv":[],"yoloArgs":[]}]}),
        true,
    );
    let mut candidates = Vec::new();
    let mut registrations = Vec::new();
    for index in 0..2 {
        let workspace =
            client::request_json(&owned.endpoint, "POST", "/v1/domain/workspaces", None).unwrap()
                ["workspace_id"]
                .clone();
        let detail = client::request_json(
            &owned.endpoint,
            "GET",
            &format!("/v1/domain/workspaces/{}", workspace.as_str().unwrap()),
            None,
        )
        .unwrap();
        let pane = detail["panes"][0].clone();
        let descriptor = root.join(format!("endpoint-{index}.json"));
        let created = client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane,"workspace_id":workspace,"program":binary,"args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,"env":{"RIDGE_COMMUNE_FIXTURE":descriptor},"role":"shell","cols":80,"rows":24}))).unwrap();
        assert_eq!(created["ok"], true, "{created}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !descriptor.exists() {
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let descriptor: Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
        registrations.push(json!({"workspaceId":workspace,"paneId":pane,"process":descriptor["process"],"endpointOwner":descriptor["process"],"endpoint":descriptor["endpoint"],"sessionId":descriptor["sessionId"],"cwd":root}));
        let observation = loop {
            let topology = command(
                &owned.endpoint,
                "get_teammate_topology",
                json!({"workspaceId":workspace}),
                true,
            );
            if let Some(candidate) = topology["discovered"].as_array().unwrap().first() {
                break candidate.clone();
            }
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert!(observation["candidateRef"].is_string());
        let remote = command(
            &owned.endpoint,
            "get_teammate_topology",
            json!({"workspaceId":workspace}),
            false,
        );
        assert_eq!(
            remote["discovered"][0]["candidateRef"],
            observation["candidateRef"]
        );
        for field in ["process", "terminalEpoch", "endpoint", "source"] {
            assert!(remote["discovered"][0].get(field).is_none());
        }
        candidates.push(observation);
    }
    assert_eq!(candidates[0]["name"], candidates[1]["name"]);
    assert_ne!(candidates[0]["candidateRef"], candidates[1]["candidateRef"]);
    assert_eq!(
        fs::read(&groups_path).unwrap(),
        old,
        "discovery and projection must remain read-only"
    );
    assert!(
        !root.join("agent-roster.json").exists(),
        "terminal observations must not invent session identities"
    );
    let make_group =
        |id: &str| json!({"id":id,"name":id,"color":"#60a5fa","memberAgentIds":[],"createdAt":1});
    let enrollment = json!({"groupScope":"host","expectedRevision":0,"groups":[make_group("one")],"enrollments":[{"candidateRef":candidates[0]["candidateRef"],"groupId":"one"}]});
    let mut forged = enrollment.clone();
    forged["enrollments"][0]["candidateRef"] = json!(Uuid::new_v4());
    assert_eq!(
        command_reply(&owned.endpoint, "set_teammate_groups", forged, false)["ok"],
        false
    );
    let other_root = root.join("other-runtime");
    let other = runtime_fixture::start(&other_root).await;
    assert_eq!(
        command_reply(
            &other.endpoint,
            "set_teammate_groups",
            enrollment.clone(),
            false
        )["ok"],
        false,
        "reference cannot cross Runtime"
    );
    drop(other);
    let mut invalid = enrollment.clone();
    invalid["groups"][0]["color"] = json!("invalid");
    assert_eq!(
        command_reply(&owned.endpoint, "set_teammate_groups", invalid, false)["ok"],
        false
    );
    assert!(command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"groupScope":"host"}),
        true
    )["members"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        fs::read(&groups_path).unwrap(),
        old,
        "failed join must create neither identity nor group reference"
    );
    let joined = command(
        &owned.endpoint,
        "set_teammate_groups",
        enrollment.clone(),
        false,
    );
    let agent_id = joined["groups"][0]["memberAgentIds"][0].clone();
    assert!(agent_id.is_string());
    assert_eq!(joined["members"].as_array().unwrap().len(), 1);
    assert_eq!(joined["members"][0]["terminalAvailable"], true);
    assert_eq!(joined["members"][0]["protocolConnected"], false);
    assert_eq!(joined["members"][0]["canSend"], false);
    assert!(joined["members"][0].get("lease").is_none());
    assert!(
        joined["members"][0].get("source").is_none(),
        "Remote organization projection must redact process source"
    );
    let document: Value = serde_json::from_slice(&fs::read(&groups_path).unwrap()).unwrap();
    assert_eq!(document["version"], 3);
    assert_eq!(document["members"].as_object().unwrap().len(), 1);
    assert!(fs::read_dir(&root).unwrap().flatten().any(|entry| entry
        .file_name()
        .to_string_lossy()
        .starts_with("commune-groups-v2-")
        && fs::read(entry.path()).ok().as_deref() == Some(old.as_slice())));
    assert!(!root.join("agent-roster.json").exists());
    // A lost reply is reconciled from the revision. Explicit retry is idempotent by source.
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "set_teammate_groups",
            enrollment.clone(),
            false
        )["ok"],
        false
    );
    let mut retry = enrollment;
    retry["expectedRevision"] = joined["groupsRevision"].clone();
    retry["groups"] = joined["groups"].clone();
    let retried = command(&owned.endpoint, "set_teammate_groups", retry, false);
    assert_eq!(
        retried["groups"][0]["memberAgentIds"],
        json!([agent_id.clone()])
    );
    assert_eq!(retried["members"].as_array().unwrap().len(), 1);
    {
        let mut left = runtime_fixture::attach(&owned.endpoint).await;
        let mut right = runtime_fixture::attach(&owned.endpoint).await;
        let request = json!({"groupScope":"host","expectedRevision":retried["groupsRevision"],"groups":retried["groups"],"enrollments":[{"candidateRef":candidates[0]["candidateRef"],"groupId":"one"}]});
        let (first, second) = tokio::join!(
            runtime_fixture::rpc(&mut left, 1, "set_teammate_groups", request.clone()),
            runtime_fixture::rpc(&mut right, 1, "set_teammate_groups", request)
        );
        assert_ne!(
            first["error"].is_null(),
            second["error"].is_null(),
            "one CAS writer must lose"
        );
        let winner = if first["error"].is_null() {
            &first
        } else {
            &second
        };
        assert_eq!(winner["result"]["members"].as_array().unwrap().len(), 1);
        assert_eq!(
            winner["result"]["groups"][0]["memberAgentIds"],
            json!([agent_id.clone()])
        );
    }
    let authority = command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"groupScope":"host"}),
        true,
    );
    let before_invalid_edits = fs::read(&groups_path).unwrap();
    for (field, ids, diagnostic) in [
        (
            "archiveMemberIds",
            json!(["unknown"]),
            "unknown organization member",
        ),
        (
            "archiveMemberIds",
            json!([agent_id.clone(), agent_id.clone()]),
            "duplicate",
        ),
        (
            "restoreMemberIds",
            json!([agent_id.clone()]),
            "already active",
        ),
    ] {
        let mut request = json!({"groupScope":"host","expectedRevision":authority["groupsRevision"],"groups":authority["groups"]});
        request[field] = ids;
        let rejected = command_reply(&owned.endpoint, "set_teammate_groups", request, true);
        assert_eq!(rejected["ok"], false);
        assert!(rejected.to_string().contains(diagnostic), "{rejected}");
        assert_eq!(fs::read(&groups_path).unwrap(), before_invalid_edits);
    }
    let overlap = json!({"groupScope":"host","expectedRevision":authority["groupsRevision"],"groups":authority["groups"],"archiveMemberIds":[agent_id],"restoreMemberIds":[agent_id]});
    let rejected = command_reply(&owned.endpoint, "set_teammate_groups", overlap, true);
    assert_eq!(rejected["ok"], false);
    assert!(rejected
        .to_string()
        .contains("cannot be archived and restored together"));
    let no_control = json!({"groupScope":"host","groupId":"one","groupRevision":retried["groups"][0]["revision"],"groupAction":"broadcast","objective":"must-not-run","idempotency_key":Uuid::new_v4(),"targets":[{"agentId":agent_id,"generation":1,"lease":"invented"}]});
    assert_eq!(
        command_reply(&owned.endpoint, "send_agent_message", no_control, true)["ok"],
        false
    );
    assert!(!root.join("prompt-log").exists());
    let before_attach = fs::read(&groups_path).unwrap();
    let connected = command(
        &owned.endpoint,
        "register_commune_member",
        registrations[0].clone(),
        true,
    );
    assert_eq!(connected["agentId"], agent_id);
    assert_eq!(
        fs::read(&groups_path).unwrap(),
        before_attach,
        "attach must keep organization file and group references unchanged"
    );
    let projected = command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"groupScope":"host"}),
        false,
    );
    assert_eq!(projected["members"][0]["protocolConnected"], true);
    assert_eq!(projected["members"][0]["canSend"], true);
    let mut wrong_member = registrations[0].clone();
    wrong_member["memberAgentId"] = json!(Uuid::new_v4());
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "register_commune_member",
            wrong_member,
            true
        )["ok"],
        false
    );
    let target = json!({"workspaceId":registrations[0]["workspaceId"],"paneId":registrations[0]["paneId"],"agentId":agent_id,"generation":connected["generation"],"lease":connected["lease"]});
    let mut permission_prompt = target.clone();
    permission_prompt["message"] = json!("permission-needed");
    permission_prompt["idempotency_key"] = json!("organization-permission");
    let permission_delivery = command(
        &owned.endpoint,
        "send_agent_message",
        permission_prompt,
        true,
    );
    let pending = wait_permission(&owned.endpoint);
    assert_eq!(pending["agentId"], agent_id);
    assert_eq!(pending["generation"], connected["generation"]);
    let mut blocked_rebind = registrations[1].clone();
    blocked_rebind["memberAgentId"] = agent_id.clone();
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "register_commune_member",
            blocked_rebind,
            true
        )["ok"],
        false,
        "active original connection/task/approval must never be overwritten"
    );
    assert_eq!(
        command(
            &owned.endpoint,
            "resolve_hitl_request",
            json!({"id":pending["id"],"optionId":"once"}),
            true
        ),
        true
    );
    wait_execution(
        &owned.endpoint,
        &json!({"workspace_id":registrations[0]["workspaceId"],"agent_id":agent_id,"generation":connected["generation"],"lease":connected["lease"]}),
        &permission_delivery["deliveryId"],
        "turn_ended",
    );
    // Enrollment and authenticated handshake share one authority, whichever commits first.
    let mut a = runtime_fixture::attach(&owned.endpoint).await;
    let mut b = runtime_fixture::attach(&owned.endpoint).await;
    let mut groups = projected["groups"].as_array().unwrap().clone();
    groups.push(make_group("two"));
    let second_enrollment = json!({"groupScope":"host","expectedRevision":projected["groupsRevision"],"groups":groups,"enrollments":[{"candidateRef":candidates[1]["candidateRef"],"groupId":"two"}]});
    let (enrolled, attached) = tokio::join!(
        runtime_fixture::rpc(&mut a, 1, "set_teammate_groups", second_enrollment),
        runtime_fixture::rpc(
            &mut b,
            1,
            "register_commune_member",
            registrations[1].clone()
        )
    );
    assert!(enrolled["error"].is_null(), "{enrolled}");
    assert!(attached["error"].is_null(), "{attached}");
    assert_eq!(
        enrolled["result"]["groups"][1]["memberAgentIds"],
        json!([attached["result"]["agentId"].clone()])
    );
    let snapshot = command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"groupScope":"host"}),
        true,
    );
    assert_eq!(snapshot["members"].as_array().unwrap().len(), 2);
    assert_ne!(
        snapshot["members"][0]["agentId"],
        snapshot["members"][1]["agentId"]
    );
    // Removing a group reference and archiving are separate from disconnecting protocol.
    let mut empty = snapshot["groups"].clone();
    empty[0]["memberAgentIds"] = json!([]);
    let archive = json!({"groupScope":"host","expectedRevision":snapshot["groupsRevision"],"groups":empty,"archiveMemberIds":[agent_id]});
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "set_teammate_groups",
            archive.clone(),
            true
        )["ok"],
        false,
        "active connection blocks archival even if request removes group membership"
    );
    command(
        &owned.endpoint,
        "release_teammate_agent",
        json!({"workspaceId":registrations[0]["workspaceId"],"paneId":registrations[0]["paneId"],"agentId":agent_id,"generation":connected["generation"],"lease":connected["lease"]}),
        true,
    );
    let archived = command(&owned.endpoint, "set_teammate_groups", archive, true);
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "register_commune_member",
            registrations[0].clone(),
            true
        )["ok"],
        false,
        "automatic/exact attachment must never restore an archived member"
    );
    let restored = command(
        &owned.endpoint,
        "set_teammate_groups",
        json!({"groupScope":"host","expectedRevision":archived["groupsRevision"],"groups":archived["groups"],"restoreMemberIds":[agent_id]}),
        true,
    );
    assert_eq!(
        restored["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["agentId"] == agent_id)
            .unwrap()["canSend"],
        false,
        "restore grants no control"
    );
    let reconnect = command(
        &owned.endpoint,
        "register_commune_member",
        registrations[0].clone(),
        true,
    );
    assert_eq!(reconnect["agentId"], agent_id);
    assert!(reconnect["generation"].as_u64() > connected["generation"].as_u64());
    let prior_groups = fs::read(&groups_path).unwrap();
    let history_path = root
        .join("commune-history")
        .join(format!("{}.jsonl", agent_id.as_str().unwrap()));
    let prior_history = fs::read(&history_path).unwrap();
    let pane_id = registrations[0]["paneId"].as_str().unwrap();
    let workspace_id = registrations[0]["workspaceId"].as_str().unwrap();
    client::request_json(
        &owned.endpoint,
        "DELETE",
        &format!("/v1/domain/ptys/{pane_id}"),
        None,
    )
    .unwrap();
    let stale_join = json!({"groupScope":"host","expectedRevision":restored["groupsRevision"],"groups":restored["groups"],"enrollments":[{"candidateRef":candidates[0]["candidateRef"],"groupId":"one"}]});
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "set_teammate_groups",
            stale_join.clone(),
            false
        )["ok"],
        false,
        "exited process reference must fail before persistence"
    );
    assert_eq!(fs::read(&groups_path).unwrap(), prior_groups);
    let replacement_path = root.join("replacement.json");
    let created = client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane_id,"workspace_id":workspace_id,"program":binary,"args":["--exact","existing_protocol_endpoint_fixture","--ignored"],"cwd":root,"env":{"RIDGE_COMMUNE_FIXTURE":replacement_path},"role":"shell","cols":80,"rows":24}))).unwrap();
    assert_eq!(created["ok"], true, "{created}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !replacement_path.exists() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let replacement: Value = serde_json::from_slice(&fs::read(replacement_path).unwrap()).unwrap();
    let changed = command(
        &owned.endpoint,
        "get_teammate_topology",
        json!({"workspaceId":workspace_id}),
        true,
    );
    assert_ne!(
        changed["discovered"][0]["candidateRef"],
        candidates[0]["candidateRef"]
    );
    assert!(
        changed["discovered"][0]["agentId"].is_null(),
        "same pane and name must not silently rebind a new process"
    );
    assert_eq!(
        command_reply(&owned.endpoint, "set_teammate_groups", stale_join, false)["ok"],
        false,
        "terminal recreation cannot revive old reference"
    );
    let mut rebind = json!({"workspaceId":workspace_id,"paneId":pane_id,"process":replacement["process"],"endpointOwner":replacement["process"],"endpoint":replacement["endpoint"],"sessionId":replacement["sessionId"],"cwd":root,"memberAgentId":agent_id});
    loop {
        let snapshot = command(
            &owned.endpoint,
            "get_teammate_topology",
            json!({"groupScope":"host"}),
            true,
        );
        if snapshot["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["agentId"] == agent_id)
            .unwrap()["protocolConnected"]
            == false
        {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    rebind["automatic"] = json!(true);
    assert_eq!(
        command_reply(
            &owned.endpoint,
            "register_commune_member",
            rebind.clone(),
            true
        )["ok"],
        false,
        "automatic connection cannot choose an old organization member"
    );
    rebind["automatic"] = json!(false);
    let rebound = command(&owned.endpoint, "register_commune_member", rebind, true);
    assert_eq!(rebound["agentId"], agent_id);
    assert!(rebound["generation"].as_u64() > reconnect["generation"].as_u64());
    assert_eq!(fs::read(&groups_path).unwrap(), prior_groups);
    assert!(
        fs::read(&history_path).unwrap().starts_with(&prior_history),
        "rebind must append coverage without rewriting old tasks, approvals or history"
    );
    drop(a);
    drop(b);
    drop(owned);
    let restarted = runtime_fixture::start(&root).await;
    let restored = command(
        &restarted.endpoint,
        "get_teammate_topology",
        json!({"groupScope":"host"}),
        true,
    );
    assert_eq!(restored["members"].as_array().unwrap().len(), 2);
    assert!(restored["members"]
        .as_array()
        .unwrap()
        .iter()
        .all(|member| member["canSend"] == false && member["protocolConnected"] == false));
    let stale = json!({"groupScope":"host","expectedRevision":restored["groupsRevision"],"groups":restored["groups"],"enrollments":[{"candidateRef":candidates[0]["candidateRef"],"groupId":"one"}]});
    assert_eq!(
        command_reply(&restarted.endpoint, "set_teammate_groups", stale, true)["ok"],
        false
    );
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
                assert_eq!(value["payload"]["workspaceId"], workspace);
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
