//! Controlled shared protocol service; real provider evidence lives in the Node acceptance.
use futures::{SinkExt, StreamExt};
use ridge_core::process_identity;
use ridge_kernel::{client, managed_console::AgentArgs};
use serde_json::{json, Value};
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}, time::Duration};
use tokio::{net::TcpListener, sync::broadcast};
use tokio_tungstenite::tungstenite::Message;
#[path = "common/runtime_fixture.rs"] mod runtime_fixture;

struct SharedService {
    endpoint: String,
    task: tokio::task::JoinHandle<()>,
    calls: Arc<Mutex<Vec<Value>>>,
    revoke: broadcast::Sender<String>,
}
impl Drop for SharedService {fn drop(&mut self) {self.task.abort();}}
impl SharedService {
    async fn start(token: Option<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/", listener.local_addr().unwrap());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let (revoke, _) = broadcast::channel(16);
        let changed = revoke.clone();
        let sessions = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let task = tokio::spawn(async move {
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.unwrap();
                        let token = token.clone();
                        let calls = observed.clone();
                        let sessions = sessions.clone();
                        let mut changed = changed.subscribe();
                        clients.spawn(async move {
                            use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
                            let socket = tokio_tungstenite::accept_hdr_async(stream, move |request: &Request, response: Response| {
                                if token.as_ref().is_some_and(|token| request.headers().get("authorization").and_then(|value| value.to_str().ok()) != Some(format!("Bearer {token}").as_str())) {
                                    return Err(Response::builder().status(401).body(None).unwrap());
                                }
                                Ok(response)
                            }).await;
                            let Ok(mut socket) = socket else {return;};
                            let mut own = String::new();
                            let mut awaiting: Option<(Value, String)> = None;
                            loop {
                                tokio::select! {
                                    Ok(id) = changed.recv() => {
                                        if id == own {let _ = socket.send(Message::Text(json!({"method":"thread/unloaded","params":{"threadId":id}}).to_string())).await;}
                                    }
                                    message = socket.next() => {
                                        let Some(Ok(Message::Text(text))) = message else {break;};
                                        let request: Value = serde_json::from_str(&text).unwrap();
                                        calls.lock().unwrap().push(request.clone());
                                        if request.get("method").is_none() {
                                            if let Some((_, turn)) = awaiting.take() {
                                                let _ = socket.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":own,"turn":{"id":turn,"status":"completed"}}}).to_string())).await;
                                            }
                                            continue;
                                        }
                                        let params = &request["params"];
                                        let result = match request["method"].as_str().unwrap() {
                                            "initialize" => json!({"userAgent":"controlled-shared-service"}),
                                            "initialized" => continue,
                                            "thread/start" => {
                                                own = uuid::Uuid::new_v4().to_string();
                                                sessions.lock().unwrap().push((own.clone(), params["cwd"].as_str().unwrap().into()));
                                                json!({"thread":{"id":own,"cwd":params["cwd"],"canAcceptDirectInput":true,"status":{"type":"idle"}}})
                                            }
                                            "thread/loaded/list" => json!({"data":sessions.lock().unwrap().iter().map(|(id,_)|id).collect::<Vec<_>>()}),
                                            "thread/read" | "thread/resume" => {
                                                let rows = sessions.lock().unwrap();
                                                let row = rows.iter().find(|(id,_)| id == params["threadId"].as_str().unwrap()).unwrap();
                                                json!({"thread":{"id":row.0,"cwd":row.1,"canAcceptDirectInput":true,"status":{"type":"idle"}}})
                                            }
                                            "thread/turns/list" => json!({"data":[],"nextCursor":null}),
                                            "turn/start" => {
                                                assert_eq!(params["threadId"], own);
                                                let turn = uuid::Uuid::new_v4().to_string();
                                                let message = params["input"][0]["text"].as_str().unwrap();
                                                let _ = socket.send(Message::Text(json!({"method":"item/completed","params":{"threadId":own,"item":{"id":turn,"type":"agentMessage","text":format!("{own}: {message}")}}}).to_string())).await;
                                                if message.contains("approve") {
                                                    let approval = json!(format!("permission-{turn}"));
                                                    let _ = socket.send(Message::Text(json!({"id":approval,"method":"item/commandExecution/requestApproval","params":{"threadId":own,"itemId":turn,"reason":"controlled approval","availableDecisions":["accept","decline","cancel"]}}).to_string())).await;
                                                    awaiting = Some((approval, turn.clone()));
                                                } else {
                                                    let _ = socket.send(Message::Text(json!({"method":"turn/completed","params":{"threadId":own,"turn":{"id":turn,"status":"completed"}}}).to_string())).await;
                                                }
                                                json!({"turn":{"id":turn}})
                                            }
                                            "turn/interrupt" => json!({}),
                                            method => panic!("unexpected controlled shared method {method}"),
                                        };
                                        if socket.send(Message::Text(json!({"id":request["id"],"result":result}).to_string())).await.is_err() {break;}
                                    }
                                }
                            }
                        });
                    }
                    Some(_) = clients.join_next() => {}
                }
            }
        });
        Self {endpoint, task, calls, revoke}
    }
}
fn command(endpoint: &ridge_kernel::registry::KernelEndpoint, method: &str, args: Value, native: bool) -> Value {
    client::request_json(endpoint, "POST", "/v1/domain/commands", Some(&json!({"method":method,"args":args,"native":native,"roots":[],"connectionId":uuid::Uuid::new_v4()}))).unwrap()
}
fn invoke(endpoint: &ridge_kernel::registry::KernelEndpoint, method: &str, args: Value) -> Value {
    let result = command(endpoint, method, args, true);
    assert_eq!(result["ok"], true, "{method}: {result}");
    result["value"].clone()
}
async fn wait(mut read: impl FnMut() -> Option<Value>) -> Value {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {if let Some(value) = read() {return value;} tokio::time::sleep(Duration::from_millis(30)).await;}
    }).await.expect("shared daemon acceptance deadline")
}
fn publication(root: &Path, process: &Value) -> PathBuf {
    std::fs::read_dir(root.join("agent-connections")).unwrap().filter_map(Result::ok).map(|entry|entry.path())
        .find(|path| path.extension().is_some_and(|ext|ext == "json") && serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap()["process"] == *process).unwrap()
}

#[test]
fn shared_configuration_requires_explicit_local_protocol_without_claimed_session_or_process_args() {
    for value in [
        json!({"connect":"ws://127.0.0.1:4500/"}),
        json!({"connect":"ws://example.com:4500/","protocol":"app-server"}),
        json!({"connect":"ws://127.0.0.1:4500/?token=secret","protocol":"app-server"}),
        json!({"connect":"ws://127.0.0.1:4500/","protocol":"app-server","profile":"codex"}),
        json!({"connect":"ws://127.0.0.1:4500/","protocol":"app-server","arguments":["--session","another"]}),
        json!({"connect":"ws://127.0.0.1:4500/","protocol":"acp","bearerTokenFile":"/token"}),
        json!({"connect":"ws://127.0.0.1:4500/","protocol":"app-server","sessionId":"another"}),
    ] {
        assert!(serde_json::from_value::<AgentArgs>(value).map(|args|args.validate().is_err()).unwrap_or(true));
    }
    serde_json::from_value::<AgentArgs>(json!({"connect":"ws://127.0.0.1:4500/","protocol":"app-server"})).unwrap().validate().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_same_cwd_clients_retain_groups_and_fixed_sessions_while_forged_and_revoked_bindings_fail() {
    let root = std::env::temp_dir().join(format!("ridge-shared-binding-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let daemon = SharedService::start(None).await;
    let runtime = runtime_fixture::start(&root).await;
    let preview = invoke(&runtime.endpoint, "get_collaboration_launch_options", json!({}));
    assert!(daemon.calls.lock().unwrap().is_empty(), "preview must not connect or create a session");
    let mut candidates = Vec::new();
    for _ in 0..2 {
        let request = json!({"workspaceId":uuid::Uuid::new_v4(),"paneId":uuid::Uuid::new_v4(),"runtimeEpoch":preview["runtimeEpoch"],"entry":"agent","cwd":root,"collaboration":{"connect":daemon.endpoint,"protocol":"app-server"}});
        assert_eq!(command(&runtime.endpoint,"create_independent_terminal",request.clone(),false)["ok"],false);
        let created = invoke(&runtime.endpoint,"create_independent_terminal",request.clone());
        assert_eq!(invoke(&runtime.endpoint,"create_independent_terminal",request),created);
        let started = std::time::Instant::now();
        let candidate = wait(|| {
            let panes = client::list_domain_ptys(&runtime.endpoint).unwrap();
            let pane = panes.iter().find(|pane|json!(pane.pty_id)==created["paneId"]).unwrap();
            assert!(!pane.status.eq_ignore_ascii_case("exited"),"shared console exited: {}; calls {:?}",String::from_utf8_lossy(&client::scrollback_domain_pty(&runtime.endpoint,pane.pty_id,4096).unwrap_or_default()),daemon.calls.lock().unwrap());
            let topology=invoke(&runtime.endpoint,"get_teammate_topology",json!({"workspaceId":created["workspaceId"]}));
            let candidate=topology["discovered"].as_array()?.iter().find(|row|row["connectionReady"] == true).cloned();
            assert!(candidate.is_some() || started.elapsed()<Duration::from_secs(8),"no shared candidate; pane {pane:?}; projection {topology}; console {}; calls {:?}",String::from_utf8_lossy(&client::scrollback_domain_pty(&runtime.endpoint,pane.pty_id,4096).unwrap_or_default()),daemon.calls.lock().unwrap());
            candidate
        }).await;
        candidates.push(candidate);
    }
    let original_processes = client::list_domain_ptys(&runtime.endpoint).unwrap();
    invoke(&runtime.endpoint,"set_teammate_groups",json!({"groupScope":"host","expectedRevision":0,"groups":[{"id":"original","name":"Original group","color":"#60a5fa","memberAgentIds":[],"createdAt":1}],"enrollments":candidates.iter().map(|candidate|json!({"candidateRef":candidate["candidateRef"],"groupId":"original"})).collect::<Vec<_>>()}));
    let groups_before = std::fs::read(root.join("commune-groups.json")).unwrap();
    let mut identities = Vec::new();
    let mut session_ids = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let mut selected = json!({"workspaceId":candidate["workspaceId"],"paneId":candidate["paneId"],"candidateRef":candidate["candidateRef"]});
        let original_session = invoke(&runtime.endpoint,"prepare_commune_connection",selected.clone())["sessionId"].clone();
        let file = publication(&root,&candidate["process"]);
        let original = std::fs::read(&file).unwrap();
        let mut forged: Value = serde_json::from_slice(&original).unwrap();
        forged["process"]["startInstance"]=json!("recycled-process-instance");
        std::fs::write(&file,serde_json::to_vec(&forged).unwrap()).unwrap();
        assert_eq!(command(&runtime.endpoint,"prepare_commune_connection",selected.clone(),true)["ok"],false);
        std::fs::write(&file,&original).unwrap();
        let refreshed=invoke(&runtime.endpoint,"prepare_commune_connection",json!({"workspaceId":candidate["workspaceId"],"paneId":candidate["paneId"]}));
        assert_ne!(refreshed["candidateRef"],selected["candidateRef"],"discarded process proof must require fresh discovery");
        selected["candidateRef"]=refreshed["candidateRef"].clone();
        forged=serde_json::from_slice(&original).unwrap();
        let other:Value=serde_json::from_slice(&std::fs::read(publication(&root,&candidates[1-index]["process"])).unwrap()).unwrap();
        forged["endpoint"]=other["endpoint"].clone();
        std::fs::write(&file,serde_json::to_vec(&forged).unwrap()).unwrap();
        assert_eq!(command(&runtime.endpoint,"prepare_commune_connection",selected.clone(),true)["ok"],false,"another client's endpoint must not inherit this process proof");
        std::fs::write(&file,&original).unwrap();
        let prepared=invoke(&runtime.endpoint,"prepare_commune_connection",selected.clone());
        assert_eq!(prepared["sessionId"],original_session);
        forged=serde_json::from_slice(&original).unwrap();
        forged["sessionId"] = json!("foreign-session");
        std::fs::write(&file,serde_json::to_vec(&forged).unwrap()).unwrap();
        let mut stale = selected.clone(); stale["sessionRef"] = prepared["sessionRef"].clone();
        assert_eq!(command(&runtime.endpoint,"register_commune_member",stale,true)["ok"],false);
        assert_eq!(command(&runtime.endpoint,"register_commune_member",selected.clone(),true)["ok"],false,"writable status must not override the held channel's exact session");
        std::fs::write(&file,&original).unwrap();
        let identity = invoke(&runtime.endpoint,"register_commune_member",selected);
        assert_eq!(identity["sessionId"],prepared["sessionId"]);
        session_ids.push(identity["sessionId"].clone());
        identities.push(identity);
    }
    assert_ne!(session_ids[0],session_ids[1]);
    assert_eq!(std::fs::read(root.join("commune-groups.json")).unwrap(),groups_before);
    assert_eq!(daemon.calls.lock().unwrap().iter().filter(|call|call["method"] == "thread/start").count(),2,"exact terminal creation retry must not create another session");
    // An unrelated terminal cannot borrow the shared process's namespace, even
    // though this daemon deliberately supports read/resume for either session.
    let ordinary = invoke(&runtime.endpoint,"create_independent_terminal",json!({"workspaceId":uuid::Uuid::new_v4(),"paneId":uuid::Uuid::new_v4(),"runtimeEpoch":preview["runtimeEpoch"],"entry":"tui","cwd":root}));
    let pane = client::list_domain_ptys(&runtime.endpoint).unwrap().into_iter().find(|pane|json!(pane.pty_id)==ordinary["paneId"]).unwrap();
    let source = process_identity::instance(pane.child_pid.unwrap()).unwrap();
    let unproven = json!({"workspaceId":ordinary["workspaceId"],"paneId":ordinary["paneId"],"process":source,"endpointOwner":process_identity::instance(std::process::id()).unwrap(),"endpoint":daemon.endpoint,"protocol":"app-server","sessionId":session_ids[0],"cwd":root});
    assert_eq!(command(&runtime.endpoint,"register_commune_member",unproven,true)["ok"],false);
    let mut target = json!({"workspaceId":candidates[0]["workspaceId"],"paneId":candidates[0]["paneId"],"agentId":identities[0]["agentId"],"generation":identities[0]["generation"],"lease":identities[0]["lease"]});
    let mut prompt = target.clone();prompt["message"]=json!("approve this turn");prompt["idempotency_key"]=json!(uuid::Uuid::new_v4());
    let accepted=invoke(&runtime.endpoint,"send_agent_message",prompt.clone());
    let permission = wait(|| invoke(&runtime.endpoint,"list_hitl_pending_local",json!({})).as_array()?.first().cloned()).await;
    daemon.revoke.send(session_ids[0].as_str().unwrap().into()).unwrap();
    wait(|| {
        let topology=invoke(&runtime.endpoint,"get_teammate_topology",json!({"groupScope":"host"}));
        topology["members"].as_array()?.iter().find(|row|row["agentId"]==identities[0]["agentId"] && row["protocolConnected"]==false).cloned()
    }).await;
    let late=command(&runtime.endpoint,"resolve_hitl_request",json!({"id":permission["id"],"optionId":"accept"}),true);
    assert_ne!(late["value"],true,"ended session cannot accept a late approval");
    assert_eq!(invoke(&runtime.endpoint,"send_agent_message",prompt)["deliveryId"],accepted["deliveryId"],"old accepted intent retains its receipt after revocation");
    target["message"]=json!("must not resend");target["idempotency_key"]=json!(uuid::Uuid::new_v4());
    assert_eq!(command(&runtime.endpoint,"send_agent_message",target,true)["ok"],false);
    assert!(!daemon.calls.lock().unwrap().iter().any(|call|call["result"]["decision"] == "accept"));
    assert_eq!(std::fs::read(root.join("commune-groups.json")).unwrap(),groups_before);
    for original in &original_processes {
        let retained=client::list_domain_ptys(&runtime.endpoint).unwrap().into_iter().find(|pane|pane.pty_id==original.pty_id).unwrap();
        assert_eq!(retained.child_pid,original.child_pid);
    }
    let peer = process_identity::instance(std::process::id()).unwrap();
    drop(runtime);
    process_identity::validate(&peer).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shared_authentication_failure_creates_no_session_and_never_exposes_the_token() {
    let daemon=SharedService::start(Some("private-controlled-token".into())).await;
    let result=ridge_kernel::managed_agent::CollaborationTerminal::connect_shared("app-server",&daemon.endpoint,None,&std::env::temp_dir(),Arc::new(|_|Ok(())),Arc::new(|_|Box::pin(async{None})),None).await;
    let error=result.err().expect("missing Bearer must be rejected");
    assert!(error.contains("authentication"));assert!(!error.contains("private-controlled-token"));
    assert!(daemon.calls.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn generic_shared_acp_uses_its_real_channel_for_identity_prompt_and_permission() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    // One test owns this process's publication at a time. Runtime clients in
    // the other test publish under their separate console process instances.
    for websocket in [false, true] {
        let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint=format!("ws://{}/",listener.local_addr().unwrap());
        let calls=Arc::new(Mutex::new(Vec::<Value>::new()));let observed=calls.clone();
        let daemon=tokio::spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();
            let (rpc,wire)=tokio::io::duplex(8192);
            let (reader,mut writer)=tokio::io::split(rpc);
            let adapter=if websocket {
                let socket=tokio_tungstenite::accept_async(stream).await.unwrap();
                tokio::spawn(async move {
                    let (mut outgoing,mut incoming)=socket.split();let (read,mut write)=tokio::io::split(wire);let mut lines=BufReader::new(read).lines();
                    loop {tokio::select! {
                        line=lines.next_line()=>{let Ok(Some(line))=line else{break;};if outgoing.send(Message::Text(line)).await.is_err(){break;}}
                        message=incoming.next()=>{let Some(Ok(Message::Text(message)))=message else{break;};if write.write_all(format!("{message}\n").as_bytes()).await.is_err(){break;}}
                    }}
                })
            } else {tokio::spawn(async move {let mut wire=wire;let mut stream=stream;let _=tokio::io::copy_bidirectional(&mut wire,&mut stream).await;})};
            let mut lines=BufReader::new(reader).lines();let mut prompt=None;
            while let Ok(Some(line))=lines.next_line().await {
                let request:Value=serde_json::from_str(&line).unwrap();observed.lock().unwrap().push(request.clone());
                let result=match request["method"].as_str() {
                    Some("initialize")=>json!({"protocolVersion":1,"agentInfo":{"name":"custom-shared-acp"},"agentCapabilities":{}}),
                    Some("session/new")=>json!({"sessionId":"exact-shared-acp"}),
                    Some("session/prompt")=>{
                        assert_eq!(request["params"]["sessionId"],"exact-shared-acp");prompt=Some(request["id"].clone());
                        writer.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":"permission","method":"session/request_permission","params":{"sessionId":"exact-shared-acp","toolCall":{"toolCallId":"owned-tool"},"options":[{"optionId":"once","name":"Allow once","kind":"allow_once"}]}})).as_bytes()).await.unwrap();continue;
                    }
                    None=>{assert_eq!(request["result"]["outcome"]["optionId"],"once");writer.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":prompt.take().unwrap(),"result":{"stopReason":"end_turn"}})).as_bytes()).await.unwrap();continue;}
                    _=>panic!("unexpected shared ACP request {request}"),
                };
                writer.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":request["id"],"result":result})).as_bytes()).await.unwrap();
            }
            adapter.abort();
        });
        let terminal=ridge_kernel::managed_agent::CollaborationTerminal::connect_shared(if websocket{"acp-ws"}else{"acp"},&endpoint,None,&std::env::temp_dir(),Arc::new(|_|Ok(())),Arc::new(|_|Box::pin(async{Some("once".into())})),None).await.unwrap();
        assert_eq!(terminal.process().pid,std::process::id());
        assert_eq!(terminal.session().session_id(),"exact-shared-acp");
        let result=terminal.session().prompt("one original channel turn".into()).unwrap().await.unwrap().unwrap();assert_eq!(result["stopReason"],"end_turn");
        assert_eq!(calls.lock().unwrap().iter().filter(|call|call["method"]=="session/new").count(),1);
        assert_eq!(calls.lock().unwrap().iter().filter(|call|call["method"]=="session/prompt").count(),1);
        drop(terminal);daemon.abort();
    }
}
