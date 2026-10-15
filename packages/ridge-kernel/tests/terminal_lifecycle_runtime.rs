#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use base64::Engine;
use futures::{SinkExt, StreamExt};
use ridge_kernel::{client, registry::KernelEndpoint};
use ridge_term::terminal_protocol::{decode_frame, TerminalBody, TerminalFrame};
use serde_json::{json, Value};
use std::{fs, time::{Duration, Instant}};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
#[path = "common/commune_fixture.rs"]
mod commune_fixture;
#[test]
#[ignore = "owned existing-protocol child launched inside the test shell"]
fn terminal_lifecycle_agent_fixture() { commune_fixture::run(); }

fn workspace(endpoint: &KernelEndpoint) -> (Uuid, Uuid) {
    let created = client::request_json(endpoint, "POST", "/v1/domain/workspaces", None).unwrap();
    let workspace = Uuid::parse_str(created["workspace_id"].as_str().unwrap()).unwrap();
    let detail = client::request_json(endpoint, "GET", &format!("/v1/domain/workspaces/{workspace}"), None).unwrap();
    (workspace, Uuid::parse_str(detail["panes"][0].as_str().unwrap()).unwrap())
}
async fn frame(socket: &mut Socket, previous: Option<&str>) -> TerminalFrame {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop { match socket.next().await.expect("terminal stream ended").unwrap() {
            Message::Binary(bytes) if bytes.first() == Some(&0x13) => {
                let frame = decode_frame(&bytes).unwrap();
                if previous != Some(frame.terminal_epoch.as_str()) && matches!(frame.body, TerminalBody::Snapshot(_)) { break frame; }
            }
            Message::Text(text) => { let event: Value = serde_json::from_str(&text).unwrap(); assert_ne!(event["name"], "pane-terminal-error", "normal shell replacement must not fail its viewer"); }
            Message::Close(_) => panic!("normal shell replacement closed its canonical stream"),
            _ => {}
        }}
    }).await.expect("canonical replacement snapshot deadline")
}
async fn closed(socket: &mut Socket, epoch: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop { if let Message::Text(text) = socket.next().await.unwrap().unwrap() {
            let event: Value = serde_json::from_str(&text).unwrap();
            assert_ne!(event["name"], "pane-terminal-error", "ordinary shell exit must not fail another viewer");
            if event["name"] == "pane-pty-closed" && event["payload"]["terminalEpoch"] == epoch { break event["payload"].clone(); }
        }}
    }).await.expect("Runtime exit notification deadline")
}
fn input(workspace: Uuid, pane: Uuid, epoch: &str, sequence: u64, data: &[u8]) -> Value {
    json!({"input":{"pane":{"workspaceId":workspace,"paneId":pane},"terminalEpoch":epoch,"activationId":21,"sourceId":"lifecycle-input","sequence":sequence,"bytes":data}})
}
fn shell() -> &'static str { if cfg!(windows) { "cmd.exe" } else { "/bin/sh" } }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_view_receives_actual_stopped_state_and_recovery_error() {
    let root=std::env::temp_dir().join(format!("ridge-shell-failure-{}",Uuid::new_v4()));fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;let endpoint=&owned.endpoint;
    let (workspace,pane)=workspace(endpoint);let mut control=attach(endpoint).await;
    assert!(rpc(&mut control,1,"create_pane",json!({"workspaceId":workspace,"paneId":pane,"shell":shell(),"cwd":root})).await["error"].is_null());
    let original=client::list_domain_ptys(endpoint).unwrap()[0].clone();
    let mut stream=client::connect_terminal_stream(endpoint.clone(),workspace,pane,11).await.unwrap();let initial=frame(&mut stream,None).await;
    // Trusted fixture injection models stale restored metadata outside operator roots.
    let topology=json!({"pane_tree":{"root":{"Leaf":pane},"panes":{pane.to_string():{"id":pane,"mode":"Terminal","cwd":root.parent()}}}});
    assert_eq!(client::request_json(endpoint,"PUT",&format!("/v1/domain/workspaces/{workspace}/topology"),Some(&topology)).unwrap()["ok"],true);
    let mut observer=attach(endpoint).await;rpc(&mut observer,2,"list_workspaces",json!({})).await;
    client::write_domain_pty_in_workspace(endpoint,pane,workspace,b"exit\r\n").unwrap();
    let notice=closed(&mut observer,&initial.terminal_epoch).await;
    assert!(notice["replacementEpoch"].is_null());assert!(notice["error"].as_str().unwrap().contains("outside Runtime roots"));
    let mut late=attach(endpoint).await;
    assert!(rpc(&mut late,3,"activate_pane_pty",json!({"workspaceId":workspace,"paneId":pane,"rows":40,"cols":120})).await["error"].is_null(),"an exited terminal must remain attachable as a read projection");
    assert!(!rpc(&mut late,4,"activate_pane_pty",json!({"workspaceId":Uuid::new_v4(),"paneId":pane})).await["error"].is_null());
    late.send(Message::Text(json!({"type":"subscribe-pane","workspaceId":workspace,"paneId":pane,"activationId":37,"active":true}).to_string())).await.unwrap();
    let metadata=tokio::time::timeout(Duration::from_secs(5),async{loop{if let Message::Text(text)=late.next().await.unwrap().unwrap(){let event:Value=serde_json::from_str(&text).unwrap();if event["name"]=="pane-meta-changed"&&event["payload"]["activationId"]==37{break event["payload"].clone();}}}}).await.unwrap();
    assert_eq!(metadata["status"],"Exited");assert_eq!(metadata["terminalEpoch"],initial.terminal_epoch);
    let raw=client::request_json(endpoint,"GET","/v1/domain/ptys",None).unwrap();
    assert_eq!(metadata["ptyGeneration"],raw["ptys"][0]["pane_index"]);assert!(metadata["runtimeEpoch"].is_string());
    assert!(metadata["error"].as_str().unwrap().contains("outside Runtime roots"));
    assert_eq!(frame(&mut late,None).await.terminal_epoch,initial.terminal_epoch);
    assert_eq!(client::list_domain_ptys(endpoint).unwrap()[0].child_pid,original.child_pid);
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn common_powershell_creation_and_rebuild_keep_real_prompt_integration() {
    let root=std::env::temp_dir().join(format!("ridge-shell-prompt-{}",Uuid::new_v4())); fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;
    let (workspace,pane)=workspace(&owned.endpoint);
    let mut controller=attach(&owned.endpoint).await;
    let mut observer=attach(&owned.endpoint).await;
    rpc(&mut observer,1,"list_workspaces",json!({})).await;
    let result=rpc(&mut controller,1,"create_pane",json!({"workspaceId":workspace,"paneId":pane,"shell":"powershell.exe","cwd":root})).await;
    assert!(result["error"].is_null(),"{result}");
    assert_eq!(client::list_domain_ptys(&owned.endpoint).unwrap()[0].launch_profile.as_deref(),Some("ridge-interactive"));
    let name=format!("pane-prompt-{workspace}-{pane}");
    let initial=tokio::time::timeout(Duration::from_secs(10),async{loop{if let Message::Text(text)=observer.next().await.unwrap().unwrap(){let event:Value=serde_json::from_str(&text).unwrap();if event["name"]==name{break event["payload"]["terminalEpoch"].as_str().unwrap().to_owned();}}}}).await.expect("real PowerShell startup prompt missing");
    client::write_domain_pty_in_workspace(&owned.endpoint,pane,workspace,b"exit\r\n").unwrap();
    let (replacement,prompt)=tokio::time::timeout(Duration::from_secs(10),async{
        let mut replacement=None; let mut prompt=None;
        while replacement.is_none()||prompt.is_none(){if let Message::Text(text)=observer.next().await.unwrap().unwrap(){let event:Value=serde_json::from_str(&text).unwrap();
            if event["name"]=="pane-pty-closed"&&event["payload"]["terminalEpoch"]==initial {replacement=event["payload"]["replacementEpoch"].as_str().map(str::to_owned);}
            if event["name"]==name&&event["payload"]["terminalEpoch"]!=initial {prompt=event["payload"]["terminalEpoch"].as_str().map(str::to_owned);}
        }}
        (replacement.unwrap(),prompt.unwrap())
    }).await.expect("rebuilt PowerShell did not emit its actual prompt");
    assert_eq!(replacement,prompt); assert_ne!(replacement,initial);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_rebuilds_ordinary_shell_without_presentation_and_fences_old_input() {
    let root = std::env::temp_dir().join(format!("ridge-shell-lifecycle-{}", Uuid::new_v4())); fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let endpoint = &owned.endpoint;
    let (workspace, pane) = workspace(endpoint);
    let mut first = attach(endpoint).await;
    let created = rpc(&mut first, 1, "create_pane", json!({"workspaceId":workspace,"paneId":pane,"shell":shell(),"cwd":root})).await;
    assert!(created["error"].is_null(), "{created}");
    let original = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty| pty.pty_id == pane).unwrap();
    let instance = ridge_core::process_identity::instance(original.child_pid.unwrap()).unwrap();
    let mut stream = client::connect_terminal_stream(endpoint.clone(), workspace, pane, 17).await.unwrap();
    let initial = frame(&mut stream, None).await;
    drop(first);
    client::write_domain_pty_in_workspace(endpoint, pane, workspace, b"exit\r\n").unwrap();
    let replacement = frame(&mut stream, Some(&initial.terminal_epoch)).await;
    assert_eq!(replacement.pane, initial.pane);
    let live = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty| pty.pty_id == pane).unwrap();
    assert_ne!((live.child_pid, &live.child_start_instance), (original.child_pid, &original.child_start_instance));
    assert!(ridge_core::process_identity::validate(&instance).is_err());
    let mut first = attach(endpoint).await;
    let mut observer = attach(endpoint).await;
    for (socket, activation) in [(&mut first, 21), (&mut observer, 31)] {
        socket.send(Message::Text(json!({"type":"subscribe-pane","workspaceId":workspace,"paneId":pane,"activationId":activation,"active":true}).to_string())).await.unwrap();
        assert_eq!(frame(socket, None).await.terminal_epoch, replacement.terminal_epoch);
    }
    assert!(rpc(&mut first, 2, "create_pane", json!({"workspaceId":workspace,"paneId":pane,"shell":shell()})).await["error"].is_null());
    assert_eq!(client::list_domain_ptys(endpoint).unwrap().iter().find(|pty| pty.pty_id == pane).unwrap().child_pid, live.child_pid);
    let marker = root.join("replacement-input.txt");
    let command = format!("echo REPLACEMENT_INPUT > \"{}\"\r\n", marker.to_string_lossy().replace('\\', "/"));
    assert!(!rpc(&mut first, 3, "write_terminal_input", input(workspace, pane, &initial.terminal_epoch, 1, command.as_bytes())).await["error"].is_null());
    assert!(!marker.exists());
    assert_eq!(rpc(&mut first, 4, "write_terminal_input", input(workspace, pane, &replacement.terminal_epoch, 1, command.as_bytes())).await["result"]["alreadyApplied"], false);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !marker.exists() { assert!(Instant::now() < deadline); tokio::time::sleep(Duration::from_millis(20)).await; }
    assert_eq!(rpc(&mut first, 5, "write_terminal_input", input(workspace, pane, &replacement.terminal_epoch, 2, b"exit\r\n")).await["result"]["alreadyApplied"], false);
    let notice = closed(&mut observer, &replacement.terminal_epoch).await;
    assert!(notice["replacementEpoch"].is_string(), "{notice}");
    let next = frame(&mut stream, Some(&replacement.terminal_epoch)).await;
    assert_eq!(notice["replacementEpoch"], next.terminal_epoch);
    // Simulate the fresh shell dying before any user input: automation input
    // must not re-arm an unattended sequence of replacement children.
    client::write_domain_agent_pty(endpoint, pane, b"exit\r\n").unwrap();
    let stopped = closed(&mut observer, &next.terminal_epoch).await;
    assert!(stopped["replacementEpoch"].is_null(), "a startup failure must not create another child: {stopped}");
    let dead = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty| pty.pty_id == pane).unwrap();
    assert_eq!(dead.status, "Exited");
    assert_eq!(client::list_domain_ptys(endpoint).unwrap().len(), 1);
    let retained = rpc(&mut first,6,"get_terminal_snapshot",json!({"workspaceId":workspace,"paneId":pane,"activationId":21})).await;
    let retained = decode_frame(&base64::engine::general_purpose::STANDARD.decode(retained["result"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(retained.terminal_epoch,next.terminal_epoch,"a stopped shell keeps its bounded read projection");
    assert!(!rpc(&mut first,7,"write_terminal_input",input(workspace,pane,&next.terminal_epoch,1,b"must not accept dead input\r\n")).await["error"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explicit_program_exits_without_automatic_replacement() {
    let root = std::env::temp_dir().join(format!("ridge-explicit-exit-{}", Uuid::new_v4())); fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let (workspace, pane) = workspace(&owned.endpoint);
    let mut observer = attach(&owned.endpoint).await;
    rpc(&mut observer, 1, "list_workspaces", json!({})).await;
    let args = if cfg!(windows) { vec!["/d", "/q", "/k"] } else { vec!["-i"] };
    let body = json!({"pty_id":pane,"workspace_id":workspace,"program":shell(),"args":args,"cwd":root,"launch_profile":"remote-lan","cols":80,"rows":24});
    assert_eq!(client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&body)).unwrap()["ok"], true);
    let mut stream = client::connect_terminal_stream(owned.endpoint.clone(),workspace,pane,11).await.unwrap();
    let initial = frame(&mut stream,None).await;
    let original = client::list_domain_ptys(&owned.endpoint).unwrap()[0].clone();
    client::write_domain_pty_in_workspace(&owned.endpoint,pane,workspace,b"exit 17\r\n").unwrap();
    let notice = closed(&mut observer,&initial.terminal_epoch).await;
    assert!(notice["replacementEpoch"].is_null(), "explicit argv must never be replayed: {notice}");
    let dead = client::list_domain_ptys(&owned.endpoint).unwrap();
    assert_eq!(dead.len(),1); assert_eq!(dead[0].child_pid,original.child_pid); assert_eq!(dead[0].status,"Exited");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registered_protocol_member_prevents_ordinary_shell_rebuild() {
    let root = std::env::temp_dir().join(format!("ridge-registered-exit-{}", Uuid::new_v4())); fs::create_dir_all(&root).unwrap();
    let owned = start(&root).await;
    let endpoint = &owned.endpoint;
    let (workspace, pane) = workspace(endpoint);
    let mut observer = attach(endpoint).await;
    assert!(rpc(&mut observer,1,"create_pane",json!({"workspaceId":workspace,"paneId":pane,"shell":shell(),"cwd":root})).await["error"].is_null());
    let original = client::list_domain_ptys(endpoint).unwrap().into_iter().find(|pty| pty.pty_id==pane).unwrap();
    assert_eq!(original.launch_profile.as_deref(),Some("ridge-interactive"));
    let root_instance = ridge_core::process_identity::instance(original.child_pid.unwrap()).unwrap();
    let mut stream = client::connect_terminal_stream(endpoint.clone(),workspace,pane,11).await.unwrap();
    let initial = frame(&mut stream,None).await;
    let descriptor = root.join("existing-agent.json");
    let executable = std::env::current_exe().unwrap();
    let command = if cfg!(windows) {
        format!("set \"RIDGE_COMMUNE_FIXTURE={}\" && \"{}\" --exact terminal_lifecycle_agent_fixture --ignored --nocapture\r\n",descriptor.display(),executable.display())
    } else { format!("RIDGE_COMMUNE_FIXTURE='{}' '{}' --exact terminal_lifecycle_agent_fixture --ignored --nocapture\n",descriptor.display(),executable.display()) };
    client::write_domain_pty_in_workspace(endpoint,pane,workspace,command.as_bytes()).unwrap();
    let deadline = Instant::now()+Duration::from_secs(6);
    let described: Value = loop {
        if let Ok(bytes)=fs::read(&descriptor) {if let Ok(value)=serde_json::from_slice(&bytes){break value;}}
        assert!(Instant::now()<deadline,"existing protocol child did not publish its endpoint"); tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let agent: ridge_core::process_identity::ProcessInstance = serde_json::from_value(described["process"].clone()).unwrap();
    assert!(ridge_core::process_identity::descendant(&root_instance,&agent).unwrap());
    let body=json!({"workspaceId":workspace,"paneId":pane,"process":described["process"],"endpointOwner":described["process"],"endpoint":described["endpoint"],"sessionId":described["sessionId"],"cwd":root});
    let registered=client::request_json(endpoint,"POST","/v1/domain/commune/register",Some(&body)).unwrap();
    assert_eq!(registered["ok"],true,"{registered}");
    ridge_core::process_identity::validate(&root_instance).unwrap();
    ridge_core::process_identity::validate(&agent).unwrap();
    if cfg!(windows) {
        let killed=std::process::Command::new("taskkill").args(["/PID",&root_instance.pid.to_string(),"/T","/F"]).output().unwrap();
        assert!(killed.status.success(),"{}",String::from_utf8_lossy(&killed.stderr));
    } else {
        for process in [&agent,&root_instance] {
            ridge_core::process_identity::validate(process).unwrap();
            assert!(std::process::Command::new("kill").args(["-TERM",&process.pid.to_string()]).status().unwrap().success());
        }
    }
    let notice=closed(&mut observer,&initial.terminal_epoch).await;
    assert!(notice["replacementEpoch"].is_null(),"registered membership must not silently become another child: {notice}");
    assert!(ridge_core::process_identity::validate(&root_instance).is_err());
    let dead=client::list_domain_ptys(endpoint).unwrap();
    assert_eq!(dead.len(),1); assert_eq!(dead[0].child_pid,original.child_pid); assert_eq!(dead[0].status,"Exited");
    let roster=client::read_domain_agent_roster(endpoint).unwrap();
    assert!(roster.agent_identities.iter().any(|member|member.agent_id==registered["value"]["agentId"].as_str().unwrap()&&member.protocol_binding.is_some()));
}
