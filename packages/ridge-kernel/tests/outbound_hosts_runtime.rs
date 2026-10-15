#[path="common/runtime_fixture.rs"]
mod fixture;
use fixture::{start_with_env,attach,rpc};
use serde_json::json;
use std::time::{Duration,Instant};
use uuid::Uuid;
use futures::{SinkExt,StreamExt};

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn lan_connection_recovers_after_listener_interruption_without_presentation_or_new_totp(){
    let root=std::env::temp_dir().join(format!("ridge-lan-resume-{}",Uuid::new_v4()));
    let remote_root=root.join("remote");let local_root=root.join("local");
    for path in [&remote_root,&local_root]{std::fs::create_dir_all(path).unwrap();}
    let env=[("RIDGE_TEST_TLS_DIR",root.join("tls")),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into())];
    let remote=start_with_env(&remote_root,&env).await;let mut owner=attach(&remote.endpoint).await;
    let workspace=rpc(&mut owner,1,"create_workspace",json!({"name":"retained LAN session"})).await["result"].clone();
    let pane=rpc(&mut owner,2,"get_workspace_snapshot",json!({"workspaceId":workspace})).await["result"]["panes"][0]["id"].clone();
    assert!(rpc(&mut owner,3,"set_remote_enabled",json!({"enabled":true,"port":0})).await["error"].is_null());
    let info=rpc(&mut owner,4,"get_remote_info",json!({})).await["result"].clone();
    let local=start_with_env(&local_root,&env).await;let mut presentation=attach(&local.endpoint).await;
    let id=rpc(&mut presentation,1,"connect_host",json!({"kind":"remote","addr":format!("https://127.0.0.1:{}",info["port"]),"token":info["totpCode"]})).await["result"].clone();
    assert!(id.is_string());
    assert_eq!(rpc(&mut presentation,2,"host_list_snapshot",json!({})).await["result"][0]["status"],"connected");
    drop(presentation);
    assert!(rpc(&mut owner,5,"set_remote_enabled",json!({"enabled":false})).await["error"].is_null());
    assert!(rpc(&mut owner,6,"set_remote_enabled",json!({"enabled":true,"port":info["port"]})).await["error"].is_null());
    // No local presentation exists during the Runtime's recovery window.
    tokio::time::sleep(Duration::from_secs(3)).await;
    let mut reopened=attach(&local.endpoint).await;
    assert_eq!(rpc(&mut reopened,3,"host_list_snapshot",json!({})).await["result"][0]["status"],"connected","Runtime must recover the authenticated LAN socket autonomously");
    let sessions=rpc(&mut reopened,4,"list_host_sessions",json!({"hostId":id})).await;
    assert!(sessions["result"].as_array().unwrap().iter().any(|session|session["id"]==pane),"same remote pane must remain discoverable");
    let remote_sessions=rpc(&mut owner,7,"list_remote_sessions",json!({})).await;
    assert_eq!(remote_sessions["result"].as_array().unwrap().len(),1);
    assert_eq!(rpc(&mut owner,8,"disconnect_session",json!({"id":remote_sessions["result"][0]["id"]})).await["result"],true);
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {
        if rpc(&mut reopened,40,"host_list_snapshot",json!({})).await["result"][0]["status"]=="connecting" {break;}
        assert!(Instant::now()<deadline,"revoked session must enter visible recovery state");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let fresh=rpc(&mut owner,9,"get_remote_info",json!({})).await["result"]["totpCode"].clone();
    assert_eq!(rpc(&mut reopened,41,"connect_host",json!({"kind":"remote","addr":format!("https://127.0.0.1:{}",info["port"]),"token":fresh})).await["result"],id);
    assert_eq!(rpc(&mut reopened,42,"host_list_snapshot",json!({})).await["result"][0]["status"],"connected","fresh explicit TOTP must replace the revoked credential during recovery");
    assert!(rpc(&mut owner,10,"set_remote_enabled",json!({"enabled":false})).await["error"].is_null());
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {
        if rpc(&mut reopened,43,"host_list_snapshot",json!({})).await["result"][0]["status"]=="connecting" {break;}
        assert!(Instant::now()<deadline,"listener interruption must enter recovery");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(rpc(&mut reopened,5,"disconnect_host",json!({"hostId":id})).await["error"].is_null());
    assert!(rpc(&mut owner,11,"set_remote_enabled",json!({"enabled":true,"port":info["port"]})).await["error"].is_null());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(rpc(&mut reopened,6,"host_list_snapshot",json!({})).await["result"][0]["status"],"disconnected","explicit disconnect must cancel recovery");
    assert!(rpc(&mut owner,12,"list_remote_sessions",json!({})).await["result"].as_array().unwrap().is_empty());
    let code=rpc(&mut owner,13,"get_remote_info",json!({})).await["result"]["totpCode"].clone();
    assert_eq!(rpc(&mut reopened,44,"connect_host",json!({"kind":"remote","addr":format!("https://127.0.0.1:{}",info["port"]),"token":code})).await["result"],id);
    let sessions=rpc(&mut owner,14,"list_remote_sessions",json!({})).await;
    assert_eq!(rpc(&mut owner,15,"disconnect_session",json!({"id":sessions["result"][0]["id"]})).await["result"],true);
    let deadline=Instant::now()+Duration::from_secs(20);
    loop {
        let hosts=rpc(&mut reopened,45,"host_list_snapshot",json!({})).await;
        if hosts["result"][0]["status"]=="disconnected" && hosts["result"][0].get("runtimeManaged").is_none() {break;}
        assert!(Instant::now()<deadline,"revoked credential retries must terminate with a visible disconnected state");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn corrupted_existing_ca_disables_lan_without_rewriting_any_trust_material(){
    let root=std::env::temp_dir().join(format!("ridge-corrupt-ca-{}",Uuid::new_v4()));std::fs::create_dir(&root).unwrap();
    let tls=root.join("tls");
    let owned=start_with_env(&root,&[("RIDGE_TEST_TLS_DIR",tls.clone()),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into()),("RIDGE_REMOTE_ALLOW_INSECURE_HTTP","0".into())]).await;
    let mut owner=attach(&owned.endpoint).await;
    assert!(rpc(&mut owner,1,"set_remote_enabled",json!({"enabled":true,"port":0})).await["error"].is_null());
    assert!(rpc(&mut owner,2,"set_remote_enabled",json!({"enabled":false})).await["error"].is_null());
    std::fs::rename(tls.join("ca-key.pem"),tls.join("archived-ca-key.pem")).unwrap();
    std::fs::write(tls.join("ca-key.pem"),"CORRUPTED_OWNED_FIXTURE_KEY").unwrap();
    let names=["ca-key.pem","ca.pem","ca.der","cert.pem","key.pem","meta.txt"];
    let before=names.map(|name|std::fs::read(tls.join(name)).unwrap());
    let rejected=rpc(&mut owner,3,"set_remote_enabled",json!({"enabled":true,"port":0})).await;
    assert!(!rejected["error"].is_null(),"corrupt CA must fail visibly");
    assert_eq!(rpc(&mut owner,4,"get_remote_enabled",json!({})).await["result"],false);
    for (name,bytes) in names.into_iter().zip(before){assert!(std::fs::read(tls.join(name)).unwrap()==bytes,"existing trust file changed: {name}");}
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn a_different_host_ca_requires_explicit_native_trust_for_http_and_websocket(){
    let root=std::env::temp_dir().join(format!("ridge-native-ca-{}",Uuid::new_v4()));
    let remote_root=root.join("remote");let untrusted_root=root.join("untrusted");let trusted_root=root.join("trusted");
    for path in [&remote_root,&untrusted_root,&trusted_root]{std::fs::create_dir_all(path).unwrap();}
    let remote_tls=root.join("remote-tls");let remote=start_with_env(&remote_root,&[("RIDGE_TEST_TLS_DIR",remote_tls.clone()),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into())]).await;
    let mut owner=attach(&remote.endpoint).await;
    let workspace=rpc(&mut owner,1,"create_workspace",json!({"name":"other certificate owner"})).await["result"].clone();
    assert!(workspace.is_string());
    let remote_pane=rpc(&mut owner,11,"get_workspace_snapshot",json!({"workspaceId":workspace})).await["result"]["panes"][0]["id"].clone();
    assert!(remote_pane.is_string());
    assert!(rpc(&mut owner,2,"set_remote_enabled",json!({"enabled":true,"port":0})).await["error"].is_null());
    let info=rpc(&mut owner,3,"get_remote_info",json!({})).await["result"].clone();
    let addr=format!("https://127.0.0.1:{}",info["port"]);
    let empty_ca=root.join("empty-ca.pem");std::fs::write(&empty_ca,"").unwrap();
    let untrusted=start_with_env(&untrusted_root,&[("RIDGE_TEST_TLS_DIR",root.join("untrusted-tls")),("SSL_CERT_FILE",empty_ca),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into())]).await;
    let mut denied=attach(&untrusted.endpoint).await;
    let failure=rpc(&mut denied,1,"connect_host",json!({"kind":"remote","addr":addr,"token":info["totpCode"]})).await;
    assert!(failure["result"].is_string(),"failed connection must retain its visible Host record: {failure}");
    let rejected=rpc(&mut denied,2,"host_list_snapshot",json!({})).await;
    assert_eq!(rejected["result"][0]["status"],"error","unknown remote CA must be rejected: {rejected}");
    assert_eq!(rejected["result"][0]["sessions"],json!([]));
    assert!(rpc(&mut owner,4,"list_remote_sessions",json!({})).await["result"].as_array().unwrap().is_empty(),"failed TLS must not reach authenticated WebSocket admission");
    let trust_file=root.join("trusted-native-ca.pem");std::fs::copy(remote_tls.join("ca.pem"),&trust_file).unwrap();
    let trusted=start_with_env(&trusted_root,&[("RIDGE_TEST_TLS_DIR",root.join("unrelated-local-tls")),("SSL_CERT_FILE",trust_file),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into())]).await;
    let mut allowed=attach(&trusted.endpoint).await;
    let fresh=rpc(&mut owner,5,"get_remote_info",json!({})).await["result"]["totpCode"].clone();
    let connected=rpc(&mut allowed,1,"connect_host",json!({"kind":"remote","addr":addr,"token":fresh})).await;
    assert!(connected["error"].is_null(),"native-trusted peer must pass HTTP authentication and WS handshake: {connected}");
    let hosts=rpc(&mut allowed,2,"host_list_snapshot",json!({})).await;
    assert_eq!(hosts["result"][0]["status"],"connected");
    assert!(hosts["result"][0]["sessions"].as_array().unwrap().iter().any(|session|session["id"]==remote_pane));
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn host_link_proxy_reuses_local_grant_and_canonical_remote_stream(){
    use tokio_tungstenite::tungstenite::{client::IntoClientRequest,Message};
    let root=std::env::temp_dir().join(format!("ridge-host-link-{}",Uuid::new_v4()));
    let remote_root=root.join("remote");let local_root=root.join("local");
    std::fs::create_dir_all(&remote_root).unwrap();std::fs::create_dir_all(&local_root).unwrap();
    let env=[("RIDGE_TEST_TLS_DIR",root.join("tls")),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into())];
    let remote=start_with_env(&remote_root,&env).await;let mut owner=attach(&remote.endpoint).await;
    let workspace=rpc(&mut owner,1,"create_workspace",json!({"name":"relay shell","cwd":remote_root})).await["result"].clone();
    assert!(rpc(&mut owner,2,"set_remote_enabled",json!({"enabled":true,"port":0})).await["error"].is_null());
    let info=rpc(&mut owner,3,"get_remote_info",json!({})).await["result"].clone();
    let local=start_with_env(&local_root,&env).await;let mut presentation=attach(&local.endpoint).await;
    let id=rpc(&mut presentation,1,"connect_host",json!({"kind":"remote","addr":format!("https://127.0.0.1:{}",info["port"]),"token":info["totpCode"]})).await["result"].clone();
    let grant=fixture::grant(&local.endpoint);
    let mut url=url::Url::parse(&grant.ws_url).unwrap();url.path_segments_mut().unwrap().clear().extend(["v1","local","host-link",id.as_str().unwrap()]);url.query_pairs_mut().append_pair("deviceId",&grant.device_id);
    let mut request=url.as_str().into_client_request().unwrap();
    request.headers_mut().insert("origin",grant.origin.parse().unwrap());
    request.headers_mut().insert("sec-websocket-protocol",format!("ridge-client, auth-{}",grant.token).parse().unwrap());
    let mut forged=request.clone();forged.headers_mut().insert("origin","https://untrusted.example".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(forged).await.is_err(),"foreign origin cannot open a remote host relay");
    let mut missing=request.clone();missing.headers_mut().remove("sec-websocket-protocol");
    assert!(tokio_tungstenite::connect_async(missing).await.is_err(),"anonymous client cannot open a relay");
    let mut relay=tokio_tungstenite::connect_async(request).await.unwrap().0;
    let snapshot=rpc(&mut relay,1,"get_workspace_snapshot",json!({"workspaceId":workspace})).await;
    let pane=snapshot["result"]["panes"][0]["id"].clone();assert!(pane.is_string(),"{snapshot}");
    let private=rpc(&mut relay,2,"get_remote_info",json!({})).await;assert!(!private["error"].is_null(),"loopback proxy must retain remote privilege boundary");
    relay.send(Message::Text(json!({"type":"subscribe-pane","workspaceId":workspace,"paneId":pane,"activationId":41,"active":true}).to_string())).await.unwrap();
    let frame=tokio::time::timeout(Duration::from_secs(5),async{loop{if let Message::Binary(bytes)=relay.next().await.unwrap().unwrap(){if bytes.first()==Some(&0x13){break bytes;}}}}).await.unwrap();
    assert!(frame.len()>1,"relay must forward the actual canonical terminal frame");
    let output=remote_root.join("relay-child.txt");
    let command=format!("echo KERNEL_HOST_LINK_CHILD > \"{}\"\r\n",output.to_string_lossy().replace('\\',"/"));
    assert!(rpc(&mut relay,3,"write_to_pty",json!({"workspaceId":workspace,"paneId":pane,"data":command})).await["error"].is_null());
    let deadline=Instant::now()+Duration::from_secs(5);while !std::fs::read(&output).is_ok_and(|bytes|bytes.windows(22).any(|part|part==b"KERNEL_HOST_LINK_CHILD")){assert!(Instant::now()<deadline,"remote child did not execute relayed input");tokio::time::sleep(Duration::from_millis(20)).await;}
    drop(presentation);
    assert!(rpc(&mut relay,4,"list_workspaces",json!({})).await["error"].is_null(),"one local presentation closing must not close another relay");
    let mut reopened=attach(&local.endpoint).await;
    assert_eq!(rpc(&mut reopened,20,"connect_host",json!({"kind":"remote","addr":format!("https://127.0.0.1:{}",info["port"]),"token":info["totpCode"]})).await["result"],id);
    assert!(rpc(&mut relay,5,"list_workspaces",json!({})).await["error"].is_null(),"second window must reuse host connection without retiring first window subscription");
    assert!(rpc(&mut reopened,1,"disconnect_host",json!({"hostId":id})).await["error"].is_null());
    tokio::time::timeout(Duration::from_secs(3),async{while let Some(frame)=relay.next().await{if frame.is_err()||matches!(frame,Ok(Message::Close(_))){break;}}}).await.expect("explicit host disconnect must close all relay subscriptions");
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn authenticated_tls_peer_is_kernel_owned_and_never_fabricates_probe_sessions(){
    let root=std::env::temp_dir().join(format!("ridge-outbound-{}",Uuid::new_v4()));
    let remote_root=root.join("remote");let local_root=root.join("local");
    std::fs::create_dir_all(&remote_root).unwrap();std::fs::create_dir_all(&local_root).unwrap();
    let env=[("RIDGE_TEST_TLS_DIR",root.join("tls")),("RIDGE_TEST_ALLOW_NON_BREAKAWAY","1".into())];
    let remote=start_with_env(&remote_root,&env).await;let mut remote_client=attach(&remote.endpoint).await;
    let workspace=rpc(&mut remote_client,1,"create_workspace",json!({"name":"actual remote workspace"})).await["result"].clone();
    assert!(workspace.is_string());
    let enabled=rpc(&mut remote_client,2,"set_remote_enabled",json!({"enabled":true,"port":0})).await;
    assert!(enabled["error"].is_null(),"{enabled}");
    let info=rpc(&mut remote_client,3,"get_remote_info",json!({})).await["result"].clone();
    let local=start_with_env(&local_root,&env).await;let mut local_client=attach(&local.endpoint).await;
    let addr=format!("https://127.0.0.1:{}",info["port"]);
    let created=rpc(&mut local_client,1,"connect_host",json!({"kind":"remote","addr":addr,"token":info["totpCode"],"label":"real TLS peer"})).await;
    let id=created["result"].clone();assert!(id.is_string(),"{created}");
    let snapshot=rpc(&mut local_client,2,"host_list_snapshot",json!({})).await;
    let host=&snapshot["result"][0];assert_eq!(host["status"],"connected","{snapshot}");
    assert!(!host["sessions"].as_array().unwrap().is_empty());
    assert!(host["sessions"].as_array().unwrap().iter().all(|session|Uuid::parse_str(session["id"].as_str().unwrap()).is_ok()));
    let remote_pane=rpc(&mut remote_client,25,"get_workspace_snapshot",json!({"workspaceId":workspace})).await["result"]["panes"][0]["id"].clone();
    let local_workspace=rpc(&mut local_client,25,"create_workspace",json!({"name":"foreign view"})).await["result"].clone();
    let instances=||{let mut values:Vec<_>=ridge_kernel::client::list_domain_ptys(&local.endpoint).unwrap().into_iter().map(|pty|(pty.pty_id,pty.child_pid,pty.child_start_instance)).collect();values.sort();values};
    let before=instances();
    let args=json!({"workspaceId":local_workspace,"hostId":id,"sessionId":remote_pane,"remoteWorkspaceId":workspace});
    let graph=local_root.join("workspace-graph.json");let backup=local_root.join("workspace-graph.before-foreign.json");
    std::fs::rename(&graph,&backup).unwrap();std::fs::create_dir(&graph).unwrap();
    let failed=rpc(&mut local_client,26,"attach_host_session",args.clone()).await;
    assert!(!failed["error"].is_null(),"failed graph persistence must reject attachment");
    assert_eq!(instances(),before,"failed foreign attach must not start a local PTY");
    std::fs::remove_dir(&graph).unwrap();std::fs::rename(&backup,&graph).unwrap();
    let attached=rpc(&mut local_client,27,"attach_host_session",args.clone()).await;
    let view=attached["result"].clone();assert!(view.is_string(),"{attached}");
    let mut observer=attach(&local.endpoint).await;
    let subscribe=json!({"type":"subscribe-pane","workspaceId":local_workspace,"paneId":view,"activationId":17,"active":true});
    local_client.send(tokio_tungstenite::tungstenite::Message::Text(subscribe.to_string())).await.unwrap();
    observer.send(tokio_tungstenite::tungstenite::Message::Text(subscribe.to_string())).await.unwrap();
    let mut packets=Vec::new();
    for socket in [&mut local_client,&mut observer] {
        let bytes=tokio::time::timeout(Duration::from_secs(5),async{loop{if let tokio_tungstenite::tungstenite::Message::Binary(bytes)=socket.next().await.unwrap().unwrap(){if bytes.first()==Some(&0x13){break bytes;}}}}).await.expect("shared foreign subscription did not return a canonical snapshot");
        let packet=ridge_term::terminal_protocol::decode_frame(&bytes).unwrap();
        assert_eq!(packet.pane.pane_id,view.as_str().unwrap());assert_eq!(packet.pane.workspace_id,local_workspace.as_str().unwrap());assert_eq!(packet.activation_id,17);packets.push(packet);
    }
    assert_eq!(packets[0].terminal_epoch,packets[1].terminal_epoch,"two viewers must share the actual remote terminal lifetime");
    let output=remote_root.join("foreign-shared-input.txt");
    let data=format!("echo FOREIGN_SHARED_INPUT >> \"{}\"\r\n",output.to_string_lossy().replace('\\',"/"));
    let input=json!({"input":{"pane":{"workspaceId":local_workspace,"paneId":view},"terminalEpoch":packets[0].terminal_epoch,"activationId":17,"sourceId":"foreign-owner-input","sequence":1,"bytes":data.as_bytes()}});
    let ack=rpc(&mut local_client,260,"write_terminal_input",input.clone()).await;assert_eq!(ack["result"]["alreadyApplied"],false,"{ack}");
    let replay=rpc(&mut local_client,261,"write_terminal_input",input).await;assert_eq!(replay["result"]["alreadyApplied"],true,"{replay}");
    let deadline=Instant::now()+Duration::from_secs(5);while !std::fs::read(&output).is_ok_and(|bytes|bytes.windows(20).any(|part|part==b"FOREIGN_SHARED_INPUT")){assert!(Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;}
    drop(observer);
    let snapshot=rpc(&mut local_client,262,"get_terminal_snapshot",json!({"workspaceId":local_workspace,"paneId":view,"activationId":17})).await;
    use base64::Engine;
    let snapshot=ridge_term::terminal_protocol::decode_frame(&base64::engine::general_purpose::STANDARD.decode(snapshot["result"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(snapshot.pane.pane_id,view.as_str().unwrap());assert_eq!(snapshot.terminal_epoch,packets[0].terminal_epoch,"another viewer closing must not retire this viewer's foreign subscription");
    let resized=rpc(&mut local_client,2621,"resize_pane",json!({"workspaceId":local_workspace,"paneId":view,"rows":30,"cols":90,"owner":"remote"})).await;
    assert!(resized["error"].is_null(),"{resized}");
    let remote_size=ridge_kernel::client::list_domain_ptys(&remote.endpoint).unwrap().into_iter().find(|pty|Some(pty.pty_id.to_string().as_str())==remote_pane.as_str()).unwrap();
    assert_eq!((remote_size.rows,remote_size.cols),(30,90),"resize must reach the original remote PTY");
    let output_raw=remote_root.join("foreign-raw-input.txt");let data=format!("echo FOREIGN_RAW_INPUT > \"{}\"\r\n",output_raw.to_string_lossy().replace('\\',"/"));
    let raw=rpc(&mut local_client,2622,"write_to_pty",json!({"workspaceId":local_workspace,"paneId":view,"data":data})).await;
    assert!(raw["error"].is_null(),"legacy input must use the same remote PTY: {raw}");
    let deadline=Instant::now()+Duration::from_secs(5);while !std::fs::read(&output_raw).is_ok_and(|bytes|!bytes.is_empty()){assert!(Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;}
    assert_eq!(std::fs::read_to_string(&output).unwrap().matches("FOREIGN_SHARED_INPUT").count(),1,"typed replay must execute exactly once on the original child");
    let data=(0..50).map(|index|format!("echo FOREIGN_HISTORY_{index}\r\n")).collect::<String>();
    let wrote=rpc(&mut local_client,263,"write_terminal_input",json!({"input":{"pane":{"workspaceId":local_workspace,"paneId":view},"terminalEpoch":packets[0].terminal_epoch,"activationId":17,"sourceId":"foreign-owner-input","sequence":2,"bytes":data.as_bytes()}})).await;
    assert!(wrote["error"].is_null(),"{wrote}");
    let deadline=Instant::now()+Duration::from_secs(5);
    let history=loop {
        let result=rpc(&mut local_client,264,"get_terminal_snapshot",json!({"workspaceId":local_workspace,"paneId":view,"activationId":17})).await;
        let packet=ridge_term::terminal_protocol::decode_frame(&base64::engine::general_purpose::STANDARD.decode(result["result"].as_str().unwrap()).unwrap()).unwrap();
        if let ridge_term::terminal_protocol::TerminalBody::Snapshot(snapshot)=packet.body{if snapshot.history.end>snapshot.history.start{break snapshot.history;}}
        assert!(Instant::now()<deadline,"foreign child output never reached semantic history");tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let page=rpc(&mut local_client,265,"get_terminal_history",json!({"request":{"pane":{"workspaceId":local_workspace,"paneId":view},"terminalEpoch":packets[0].terminal_epoch,"activationId":17,"historyEpoch":history.epoch,"before":history.end,"maxRows":20}})).await;
    assert!(page["error"].is_null(),"{page}");
    let page=ridge_term::terminal_protocol::decode_frame(&base64::engine::general_purpose::STANDARD.decode(page["result"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(page.pane.pane_id,view.as_str().unwrap());
    assert!(matches!(page.body,ridge_term::terminal_protocol::TerminalBody::HistoryPage(ref page) if !page.lines.is_empty()),"actual foreign history must retain semantic rows");
    let spawn=rpc(&mut local_client,271,"create_pane",json!({"workspaceId":local_workspace,"paneId":view})).await;
    assert!(!spawn["error"].is_null(),"foreign view cannot fall back to a local shell: {spawn}");
    assert_eq!(instances(),before,"foreign view must not create a local shell or parser");
    assert!(!rpc(&mut local_client,28,"attach_host_session",args.clone()).await["error"].is_null(),"duplicate attachment must be rejected");
    let layout=rpc(&mut local_client,29,"get_pane_layout_for",json!({"workspaceId":local_workspace})).await;
    assert_eq!(layout["result"]["children"][1]["origin"]["host_id"],id,"{layout}");
    assert_eq!(layout["result"]["children"][1]["origin"]["workspace_id"],workspace);
    let attached_hosts=rpc(&mut local_client,291,"host_list_snapshot",json!({})).await;
    assert!(attached_hosts["result"][0]["sessions"].as_array().unwrap().iter().any(|item|item["id"]==remote_pane&&item["attached"]==true));
    assert_eq!(rpc(&mut local_client,290,"get_orchestration_health",json!({})).await["result"]["foreignAttached"],1);
    let original=ridge_kernel::client::list_domain_ptys(&remote.endpoint).unwrap().into_iter().find(|pty|Some(pty.pty_id.to_string().as_str())==remote_pane.as_str()).unwrap();
    let instance=ridge_core::process_identity::instance(original.child_pid.unwrap()).unwrap();
    let path=local_root.join("foreign-view.ridge");
    let saved=rpc(&mut local_client,294,"save_workspace_to_file",json!({"workspaceId":local_workspace,"path":path,"name":"foreign view"})).await;
    assert!(saved["error"].is_null(),"{saved}");
    let file=ridge_core::workspace::file::read(&path).unwrap();
    assert_eq!(file.foreign_panes[&Uuid::parse_str(view.as_str().unwrap()).unwrap()].session_id.to_string(),remote_pane.as_str().unwrap());
    let portable=local_root.join("exported-foreign-view.ridge");std::fs::copy(&path,&portable).unwrap();
    let target_workspace=ridge_kernel::client::request_json(&local.endpoint,"POST","/v1/domain/workspaces",None).unwrap()["workspace_id"].clone();
    let target_detail=rpc(&mut local_client,2941,"get_workspace_snapshot",json!({"workspaceId":target_workspace})).await;
    let target_pane=target_detail["result"]["panes"][0]["id"].clone();
    let source_pane=rpc(&mut local_client,2942,"get_workspace_snapshot",json!({"workspaceId":local_workspace})).await["result"]["panes"].as_array().unwrap().iter().find(|pane|pane["id"]!=view).unwrap()["id"].clone();
    let mut old_stream=ridge_kernel::client::connect_terminal_stream(local.endpoint.clone(),Uuid::parse_str(local_workspace.as_str().unwrap()).unwrap(),Uuid::parse_str(view.as_str().unwrap()).unwrap(),31).await.unwrap();
    assert!(rpc(&mut local_client,2943,"dock_pane",json!({"sourcePaneId":view,"targetPaneId":target_pane,"region":"right"})).await["error"].is_null());
    tokio::time::timeout(Duration::from_secs(2),async{while let Some(Ok(message))=old_stream.next().await{if message.is_close(){break;}}}).await.expect("old foreign workspace stream must close after docking");
    assert!(!rpc(&mut local_client,2944,"get_terminal_snapshot",json!({"workspaceId":local_workspace,"paneId":view,"activationId":17})).await["error"].is_null());
    let mut rebound=ridge_kernel::client::connect_terminal_stream(local.endpoint.clone(),Uuid::parse_str(target_workspace.as_str().unwrap()).unwrap(),Uuid::parse_str(view.as_str().unwrap()).unwrap(),32).await.unwrap();
    let moved=tokio::time::timeout(Duration::from_secs(5),async{loop{if let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes)))=rebound.next().await{break ridge_term::terminal_protocol::decode_frame(&bytes).unwrap();}}}).await.unwrap();
    assert_eq!(moved.terminal_epoch,packets[0].terminal_epoch);assert_eq!(moved.pane.workspace_id,target_workspace.as_str().unwrap());
    assert!(ridge_core::process_identity::validate(&instance).is_ok(),"docking a foreign view must retain its original remote child");
    assert!(rpc(&mut local_client,2945,"dock_pane",json!({"sourcePaneId":view,"targetPaneId":source_pane,"region":"right"})).await["error"].is_null());
    drop(rebound);
    assert!(rpc(&mut local_client,2946,"close_workspace",json!({"workspaceId":target_workspace})).await["error"].is_null());
    let closed=rpc(&mut local_client,292,"close_pane",json!({"workspaceId":local_workspace,"paneId":view})).await;
    assert!(closed["error"].is_null(),"{closed}");
    assert!(ridge_core::process_identity::validate(&instance).is_ok(),"closing local foreign view must not kill remote child");
    let hosts_after=rpc(&mut local_client,293,"host_list_snapshot",json!({})).await;
    assert!(hosts_after["result"][0]["sessions"].as_array().unwrap().iter().all(|item|item["attached"]==false));
    assert!(rpc(&mut local_client,295,"close_workspace",json!({"workspaceId":local_workspace})).await["error"].is_null());
    let opened=rpc(&mut local_client,296,"open_workspace_from_file",json!({"path":portable})).await;
    assert!(opened["error"].is_null(),"{opened}");
    let imported=opened["result"].clone();assert!(imported.is_string(),"{opened}");
    let restored=rpc(&mut local_client,297,"get_pane_layout_for",json!({"workspaceId":imported})).await;
    assert_eq!(restored["result"]["children"][1]["id"],view,"portable restore must retain the original foreign view identity: {restored}");
    assert_eq!(restored["result"]["children"][1]["origin"]["session_id"],remote_pane);
    assert!(ridge_core::process_identity::validate(&instance).is_ok(),"file restore must bind the original remote process");
    assert!(!rpc(&mut local_client,298,"create_pane",json!({"workspaceId":imported,"paneId":view})).await["error"].is_null());
    assert!(rpc(&mut local_client,299,"detach_host_session",json!({"workspaceId":imported,"paneId":view})).await["error"].is_null());
    let split=rpc(&mut remote_client,30,"split_pane",json!({"workspaceId":workspace,"paneId":host["sessions"][0]["id"],"direction":"horizontal"})).await;
    assert!(split["error"].is_null(),"{split}");
    let fresh=rpc(&mut local_client,30,"list_host_sessions",json!({"hostId":id})).await;
    assert_eq!(fresh["result"].as_array().unwrap().len(),host["sessions"].as_array().unwrap().len()+1,"live refresh must read the newly split remote pane: {fresh}");
    assert!(!std::fs::read_to_string(local_root.join("remote-hosts.json")).unwrap().contains(info["totpCode"].as_str().unwrap()));
    drop(local_client);
    let mut reopened=attach(&local.endpoint).await;
    assert_eq!(rpc(&mut reopened,3,"host_list_snapshot",json!({})).await["result"][0]["status"],"connected","presentation detach must leave outbound socket alive");
    let sessions=rpc(&mut remote_client,4,"list_remote_sessions",json!({})).await;
    assert_eq!(sessions["result"].as_array().unwrap().len(),1,"Kernel must hold one real authenticated LAN connection");
    assert!(rpc(&mut reopened,4,"disconnect_host",json!({"hostId":id})).await["error"].is_null());
    let deadline=Instant::now()+Duration::from_secs(3);
    loop {if rpc(&mut remote_client,5,"list_remote_sessions",json!({})).await["result"].as_array().unwrap().is_empty(){break;}assert!(Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;}
    assert_eq!(rpc(&mut reopened,5,"host_list_snapshot",json!({})).await["result"][0]["status"],"disconnected");
    assert!(rpc(&mut reopened,6,"forget_host",json!({"hostId":id})).await["error"].is_null());
    assert_eq!(rpc(&mut reopened,7,"host_list_snapshot",json!({})).await["result"],json!([]));
    let failed=rpc(&mut reopened,8,"connect_host",json!({"kind":"remote","addr":addr,"token":"INVALID_TOTP"})).await;
    assert!(failed["error"].is_null());
    let failure=rpc(&mut reopened,9,"host_list_snapshot",json!({})).await;
    assert_eq!(failure["result"][0]["status"],"error");assert_eq!(failure["result"][0]["sessions"],json!([]));
    let code=rpc(&mut remote_client,6,"get_remote_info",json!({})).await["result"]["totpCode"].clone();
    assert!(rpc(&mut reopened,10,"connect_host",json!({"kind":"remote","addr":addr,"token":code})).await["error"].is_null());
    assert_eq!(rpc(&mut reopened,11,"host_list_snapshot",json!({})).await["result"][0]["status"],"connected");
    drop(reopened);drop(local);
    let restarted=start_with_env(&local_root,&env).await;let mut presentation=attach(&restarted.endpoint).await;
    let restored=rpc(&mut presentation,1,"host_list_snapshot",json!({})).await;
    assert_eq!(restored["result"][0]["status"],"disconnected","Runtime restart must not advertise the dead WebSocket as connected");
    assert!(restored["result"][0]["sessions"].as_array().unwrap().iter().all(|session|session["attached"]==false));
}
