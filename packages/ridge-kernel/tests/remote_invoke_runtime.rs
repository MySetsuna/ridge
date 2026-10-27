#[path="common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach,rpc,start};
use serde_json::json;
use uuid::Uuid;
use ridge_kernel::client;
use futures::{SinkExt,StreamExt};
use tokio_tungstenite::tungstenite::Message;
use base64::Engine;

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn notification_hello_negotiates_and_rejects_incompatible_terminal_versions() {
    let root=std::env::temp_dir().join(format!("ridge-notification-hello-{}",Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;
    let mut socket=attach(&owned.endpoint).await;
    for (version, expected) in [(ridge_term::terminal_protocol::PROTOCOL_VERSION as u64,"$/hello"),(0,"$/bye")] {
        socket.send(Message::Text(json!({"jsonrpc":"2.0","method":"$/hello","params":{
            "protocolVersion":ridge_remote::rpc::HOST_PROTOCOL_VERSION,"terminalProtocolVersion":version,
            "capabilities":["pane","fs","unsupported"]
        }}).to_string())).await.unwrap();
        let response=tokio::time::timeout(std::time::Duration::from_secs(2),async {
            loop {
                if let Message::Text(text)=socket.next().await.unwrap().unwrap() {
                    let value:serde_json::Value=serde_json::from_str(&text).unwrap();
                    if value["method"]=="$/hello"||value["method"]=="$/bye" {break value;}
                }
            }
        }).await.expect("Kernel must answer an id-less hello notification");
        assert_eq!(response["method"],expected,"{response}");
        assert!(response.get("id").is_none());
        if expected=="$/hello" {assert_eq!(response["params"]["capabilities"],json!(["pane","fs"]));}
        else {assert_eq!(response["params"]["reason"],"terminal-protocol-upgrade-required");}
    }
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn native_unsubscribe_retires_only_its_own_activation_and_can_resubscribe() {
    let root=std::env::temp_dir().join(format!("ridge-native-unsubscribe-{}",Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;
    let mut first=attach(&owned.endpoint).await;
    // Subscription fencing does not depend on the interactive PowerShell profile
    // reaching its final cwd. Bind an owned shell inside the narrow fixture root.
    let workspace=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",None).unwrap()["workspace_id"].clone();
    let pane=client::request_json(&owned.endpoint,"GET",&format!("/v1/domain/workspaces/{}",workspace.as_str().unwrap()),None).unwrap()["panes"][0].clone();
    let created=client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({"workspace_id":workspace,"pty_id":pane,"program":if cfg!(windows){"cmd.exe"}else{"/bin/sh"},"args":[],"cwd":root,"env":{},"role":"shell","cols":80,"rows":24}))).unwrap();
    assert_eq!(created["ok"],true,"{created}");
    let target=json!({"workspaceId":workspace,"paneId":pane,"activationId":1});
    let mut second=attach(&owned.endpoint).await;
    for socket in [&mut first,&mut second] {
        socket.send(Message::Text(json!({"jsonrpc":"2.0","method":"subscribe-pane","params":target}).to_string())).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3),async {
            loop {if matches!(socket.next().await.unwrap().unwrap(),Message::Binary(_)){break;}}
        }).await.expect("subscription must produce its authoritative snapshot");
    }
    first.send(Message::Text(json!({"jsonrpc":"2.0","method":"unsubscribe-pane","params":target}).to_string())).await.unwrap();
    assert!(rpc(&mut first,4,"get_terminal_snapshot",target.clone()).await["error"].is_object(),"unsubscribed activation must be rejected");
    assert!(rpc(&mut second,4,"get_terminal_snapshot",target.clone()).await["error"].is_null(),"another connection retains its own subscription");
    let mut next=target.clone();next["activationId"]=json!(2);
    first.send(Message::Text(json!({"jsonrpc":"2.0","method":"subscribe-pane","params":next}).to_string())).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3),async {
        loop {if let Message::Binary(bytes)=first.next().await.unwrap().unwrap() {
            if ridge_term::terminal_protocol::decode_frame(&bytes).unwrap().activation_id==2 {break;}
        }}
    }).await.expect("new activation must produce a snapshot before reading it");
    assert!(rpc(&mut first,5,"get_terminal_snapshot",next).await["error"].is_null());
    assert!(rpc(&mut first,6,"get_terminal_snapshot",target).await["error"].is_object());
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn cloud_transport_reuses_host_domain_state_and_seals_remote_role() {
    let root=std::env::temp_dir().join(format!("ridge-cloud-dispatch-{}",Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let file=root.join("shared.txt");std::fs::write(&file,"same Runtime filesystem").unwrap();
    let owned=start(&root).await;
    let mut presentation=attach(&owned.endpoint).await;
    // Seed PTYs must start inside this fixture's deliberately narrow filesystem grant.
    let configured=rpc(&mut presentation,1000,"set_user_default_cwd",json!({"path":root})).await;
    assert!(configured["error"].is_null(),"{configured}");
    let invoke=|client,method,args|json!({"clientId":client,"method":method,"args":args});
    let first=rpc(&mut presentation,1,"dispatch_remote_invoke",invoke("controller-one","create_workspace",json!({"name":"Cloud one"}))).await;
    assert!(first["error"].is_null(),"{first}");
    let first_id=first["result"].clone();assert!(Uuid::parse_str(first_id.as_str().unwrap()).is_ok());
    let second=rpc(&mut presentation,2,"dispatch_remote_invoke",invoke("controller-two","create_workspace",json!({"name":"Cloud two"}))).await;
    let second_id=second["result"].clone();assert_ne!(first_id,second_id);
    assert_eq!(rpc(&mut presentation,3,"dispatch_remote_invoke",invoke("controller-one","get_active_workspace_id",json!({}))).await["result"],first_id);
    assert_eq!(rpc(&mut presentation,4,"dispatch_remote_invoke",invoke("controller-two","get_active_workspace_id",json!({}))).await["result"],second_id);
    let listed=rpc(&mut presentation,5,"list_workspaces",json!({})).await;
    assert!(listed["result"].as_array().unwrap().iter().any(|entry|entry["id"]==first_id));
    let pane=rpc(&mut presentation,9,"create_pane",json!({"workspaceId":first_id,"cwd":root,"shell":if cfg!(windows){"cmd.exe"}else{"/bin/sh"}})).await;
    assert!(pane["error"].is_null(),"{pane}");
    let pane_id=pane["result"]["paneId"].clone();
    let binding=json!({"clientId":"controller-one","workspaceId":first_id,"paneId":pane_id,"activationId":17,"active":true});
    let snapshot_args=json!({"workspaceId":first_id,"paneId":pane_id,"activationId":17});
    assert!(rpc(&mut presentation,10,"dispatch_remote_invoke",invoke("controller-one","get_terminal_snapshot",snapshot_args.clone())).await["error"].is_object());
    presentation.send(Message::Text(json!({"jsonrpc":"2.0","method":"subscribe-pane","params":snapshot_args}).to_string())).await.unwrap();
    let bound=rpc(&mut presentation,11,"bind_remote_terminal",binding.clone()).await;
    assert!(bound["error"].is_null(),"{bound}; pane result={pane}");
    let snapshot=rpc(&mut presentation,12,"dispatch_remote_invoke",invoke("controller-one","get_terminal_snapshot",snapshot_args.clone())).await;
    assert!(snapshot["error"].is_null(),"{snapshot}");
    let workspace=Uuid::parse_str(first_id.as_str().unwrap()).unwrap();
    let terminal=Uuid::parse_str(pane_id.as_str().unwrap()).unwrap();
    assert!(client::connect_terminal_stream(owned.endpoint.clone(),Uuid::new_v4(),terminal,17).await.is_err());
    let mut stream=client::connect_terminal_stream(owned.endpoint.clone(),workspace,terminal,17).await.unwrap();
    let initial=tokio::time::timeout(std::time::Duration::from_secs(3),stream.next()).await.unwrap().unwrap().unwrap();
    let Message::Text(metadata)=initial else {panic!("canonical source must start with fenced pane metadata")};
    let metadata:serde_json::Value=serde_json::from_str(&metadata).unwrap();
    assert_eq!(metadata["type"],"event");assert_eq!(metadata["name"],"pane-meta-changed");
    assert_eq!(metadata["payload"]["workspaceId"],first_id);assert_eq!(metadata["payload"]["paneId"],pane_id);
    assert_eq!(metadata["payload"]["activationId"],17);
    let initial=tokio::time::timeout(std::time::Duration::from_secs(3),stream.next()).await.unwrap().unwrap().unwrap();
    let Message::Binary(bytes)=initial else {panic!("metadata must be followed by a canonical binary snapshot")};
    let packet=ridge_term::terminal_protocol::decode_frame(&bytes).unwrap();
    assert_eq!(metadata["payload"]["terminalEpoch"],packet.terminal_epoch.to_string());
    let saved_packet=base64::engine::general_purpose::STANDARD.decode(snapshot["result"].as_str().unwrap()).unwrap();
    let saved_packet=ridge_term::terminal_protocol::decode_frame(&saved_packet).unwrap();
    assert_eq!(packet.terminal_epoch,saved_packet.terminal_epoch);assert_eq!(packet.pane,saved_packet.pane);assert_eq!(packet.activation_id,17);
    let input=json!({"input":{"pane":{"workspaceId":first_id,"paneId":pane_id},"terminalEpoch":packet.terminal_epoch,"activationId":17,"sourceId":"cloud-test-input","sequence":1,"bytes":[13,10]}});
    let ack=rpc(&mut presentation,24,"dispatch_remote_invoke",invoke("controller-one","write_terminal_input",input.clone())).await;
    assert!(ack["error"].is_null(),"{ack}");assert_eq!(ack["result"]["alreadyApplied"],false);
    assert_eq!(rpc(&mut presentation,25,"dispatch_remote_invoke",invoke("controller-one","write_terminal_input",input)).await["result"]["alreadyApplied"],true);
    stream.close(None).await.unwrap();
    assert!(rpc(&mut presentation,13,"dispatch_remote_invoke",invoke("controller-two","get_terminal_snapshot",snapshot_args.clone())).await["error"].is_object());
    let mut stale=binding.clone();stale["active"]=json!(false);stale["activationId"]=json!(16);
    assert!(rpc(&mut presentation,14,"bind_remote_terminal",stale).await["error"].is_null());
    let mut stale_active=binding.clone();stale_active["activationId"]=json!(16);
    assert!(rpc(&mut presentation,141,"bind_remote_terminal",stale_active).await["error"].is_object(),"old activation cannot replace the live Cloud binding");
    assert!(rpc(&mut presentation,15,"dispatch_remote_invoke",invoke("controller-one","get_terminal_snapshot",snapshot_args.clone())).await["error"].is_null());
    let mut unbind=binding.clone();unbind["active"]=json!(false);
    assert!(rpc(&mut presentation,16,"bind_remote_terminal",unbind).await["error"].is_null());
    assert!(rpc(&mut presentation,17,"dispatch_remote_invoke",invoke("controller-one","get_terminal_snapshot",snapshot_args)).await["error"].is_object());
    let script=root.join(if cfg!(windows){"reply.cmd"}else{"reply.sh"});
    std::fs::write(&script,if cfg!(windows){"@echo off\r\necho CORE_CLOUD_DISPATCH_LIVE\r\n"}else{"printf 'CORE_CLOUD_DISPATCH_LIVE\\n'\n"}).unwrap();
    let input=if cfg!(windows){format!("call \"{}\"\r\n",script.display())}else{format!("sh '{}'\n",script.display().to_string().replace('\'',"'\\''"))};
    let wrote=rpc(&mut presentation,18,"dispatch_remote_invoke",invoke("controller-one","write_to_pty",json!({"workspaceId":first_id,"paneId":pane_id,"data":input}))).await;
    assert!(wrote["error"].is_null(),"{wrote}");
    let deadline=std::time::Instant::now()+std::time::Duration::from_secs(5);
    loop {
        let output=client::request_json(&owned.endpoint,"GET",&format!("/v1/domain/ptys/{}",pane_id.as_str().unwrap()),None).unwrap();
        let bytes=base64::engine::general_purpose::STANDARD.decode(output["data_b64"].as_str().unwrap()).unwrap();
        if String::from_utf8_lossy(&bytes).contains("CORE_CLOUD_DISPATCH_LIVE") {break;}
        assert!(std::time::Instant::now()<deadline,"real child reply missing after Cloud facade input");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let content=rpc(&mut presentation,6,"dispatch_remote_invoke",invoke("controller-one","read_file",json!({"path":file}))).await;
    assert!(content["error"].is_null(),"{content}");
    let health=rpc(&mut presentation,19,"dispatch_remote_invoke",invoke("controller-one","get_orchestration_health",json!({}))).await;
    assert!(health["error"].is_null(),"{health}");assert_eq!(health["result"]["suspendedAgents"],0);
    assert!(rpc(&mut presentation,20,"set_automation_input_paused",json!({"workspaceId":first_id,"paneId":pane_id,"paused":true})).await["error"].is_null());
    let health=rpc(&mut presentation,21,"dispatch_remote_invoke",invoke("controller-one","get_orchestration_health",json!({}))).await;
    assert_eq!(health["result"]["suspendedAgents"],1);assert_eq!(health["result"]["level"],"degraded");
    let memory=root.join("workspace-memory").join(format!("{}.json",first_id.as_str().unwrap()));
    let saved=std::fs::read(&memory).unwrap();std::fs::write(&memory,b"damaged memory document").unwrap();
    assert!(rpc(&mut presentation,22,"dispatch_remote_invoke",invoke("controller-one","get_orchestration_health",json!({}))).await["error"].is_object());
    assert_eq!(std::fs::read(&memory).unwrap(),b"damaged memory document");std::fs::write(&memory,saved).unwrap();
    assert!(std::process::Command::new("git").args(["init","--quiet"]).current_dir(&root).output().unwrap().status.success());
    assert_eq!(rpc(&mut presentation,23,"dispatch_remote_invoke",invoke("controller-one","git_stash_list",json!({"repoRoot":root}))).await["result"],json!([]));
    for method in ["get_remote_info","remote_reset_totp","sign_device_identity","connect_host","dispatch_remote_invoke","bind_remote_terminal"] {
        let denied=rpc(&mut presentation,7,"dispatch_remote_invoke",invoke("controller-one",method,json!({}))).await;
        assert!(denied["error"].is_object(),"must seal remote role for {method}");
    }
    let outside=root.with_extension("outside.txt");std::fs::write(&outside,"must remain outside operator roots").unwrap();
    assert!(rpc(&mut presentation,8,"dispatch_remote_invoke",invoke("controller-one","read_file",json!({"path":outside}))).await["error"].is_object());
    let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"dispatch_remote_invoke","args":invoke("controller-one","create_workspace",json!({})),"native":false,"roots":[],"connectionId":Uuid::new_v4()
    }))).unwrap();assert_eq!(denied["ok"],false);
    let mut independent=attach(&owned.endpoint).await;
    drop(presentation);
    assert!(rpc(&mut independent,1,"list_workspaces",json!({})).await["result"].as_array().unwrap().iter().any(|entry|entry["id"]==second_id));
}
