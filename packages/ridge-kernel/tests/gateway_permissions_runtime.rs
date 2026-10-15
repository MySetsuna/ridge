//! The Runtime owns the ordinary-command approval waiter; this test never executes its dangerous action.
#[path="common/runtime_fixture.rs"] mod fixture;
use fixture::{start,attach,rpc};
use ridge_kernel::client;
use base64::Engine;
use serde_json::json;
use uuid::Uuid;
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn gateway_pending_and_decisions_use_runtime_with_exact_terminal_binding() {
 let root=std::env::temp_dir().join(format!("ridge-gateway-permission-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
 let owned=start(&root).await;
 let workspace=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",Some(&json!({}))).unwrap();
 let wid=workspace["workspace_id"].as_str().unwrap().to_owned();let pane=Uuid::new_v4();
 let terminal=client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane,"workspace_id":wid,"program":if cfg!(windows){"cmd.exe"}else{"/bin/sh"},"args":if cfg!(windows){json!(["/d","/q","/k"])}else{json!([])},"env":{},"cwd":root,"role":"shell","cols":80,"rows":24}))).unwrap();assert_eq!(terminal["ok"],true,"{terminal}");
 client::write_domain_agent_pty(&owned.endpoint,pane,b"echo RIDGE_AGENT_LF_OWNER\n\n").unwrap();
 tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{
  let output=client::scrollback_domain_pty(&owned.endpoint,pane,64*1024).unwrap();
  if String::from_utf8_lossy(&output).contains("\nRIDGE_AGENT_LF_OWNER"){break;}
  tokio::time::sleep(std::time::Duration::from_millis(20)).await;
 }}).await.unwrap_or_else(|error|panic!("Runtime submit output missing: {error}; replay={:?}",String::from_utf8_lossy(&client::scrollback_domain_pty(&owned.endpoint,pane,64*1024).unwrap())));
 let mut socket=attach(&owned.endpoint).await;
 assert!(rpc(&mut socket,21,"set_workspace_memory",json!({"workspaceId":wid,"goal":"kept"})).await["error"].is_null());
 assert!(rpc(&mut socket,24,"set_automation_input_paused",json!({"workspaceId":wid,"paneId":pane,"paused":true})).await["error"].is_null());

 let pause_file=root.join("workspace-memory").join(format!("{wid}.json"));let paused_bytes=std::fs::read(&pause_file).unwrap();
 assert!(rpc(&mut socket,28,"set_automation_input_paused",json!({"workspaceId":wid,"paneId":pane,"paused":true})).await["error"].is_null());assert_eq!(std::fs::read(&pause_file).unwrap(),paused_bytes);
 let mut observer=attach(&owned.endpoint).await;
 assert_eq!(rpc(&mut observer,1,"get_automation_input_paused",json!({"workspaceId":wid,"paneId":pane})).await["result"],true);
 assert_eq!(rpc(&mut observer,2,"get_automation_pause_count",json!({})).await["result"],1);
 let blocked=client::write_domain_agent_pty(&owned.endpoint,pane,b"echo AUTOMATION_MUST_NOT_ENTER\n").unwrap_err();assert!(blocked.contains("suspended"),"{blocked}");
 client::write_domain_pty(&owned.endpoint,pane,b"echo HUMAN_RUNTIME_GATE_OWNER\r").unwrap();
 tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let output=client::scrollback_domain_pty(&owned.endpoint,pane,64*1024).unwrap();if snapshot_has_line(&owned.endpoint,&wid,pane,"HUMAN_RUNTIME_GATE_OWNER"){assert!(!String::from_utf8_lossy(&output).contains("AUTOMATION_MUST_NOT_ENTER"));break;}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap_or_else(|error|panic!("human input missing: {error}; replay={:?}",String::from_utf8_lossy(&client::scrollback_domain_pty(&owned.endpoint,pane,64*1024).unwrap())));
 assert!(rpc(&mut observer,3,"set_automation_input_paused",json!({"workspaceId":wid,"paneId":pane,"paused":false})).await["error"].is_null());
 assert_eq!(rpc(&mut socket,25,"get_automation_input_paused",json!({"workspaceId":wid,"paneId":pane})).await["result"],false);
 assert_eq!(rpc(&mut socket,26,"get_automation_pause_count",json!({})).await["result"],0);
 assert_eq!(rpc(&mut socket,27,"get_workspace_memory",json!({"workspaceId":wid})).await["result"]["goal"],"kept");
 client::write_domain_agent_pty(&owned.endpoint,pane,b"echo AUTOMATION_RESUMED\n").unwrap();

 assert!(rpc(&mut socket,1,"set_hitl_enabled",json!({"enabled":true})).await["error"].is_null());
 let endpoint=owned.endpoint.clone();let workspace=wid.clone();
 let waiting=tokio::task::spawn_blocking(move||client::request_json(&endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"request_gateway_approval","native":true,"roots":[],"connectionId":Uuid::new_v4(),"args":{"workspaceId":workspace,"paneId":pane,"initiator":"owned-requester","action":"rm -rf / # private-secret"}}))).unwrap());
 let local=tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let pending=rpc(&mut socket,2,"list_hitl_pending_local",json!({})).await;if pending["result"].as_array().is_some_and(|rows|!rows.is_empty()){break pending["result"][0].clone();}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap();
 assert!(local["action"].as_str().unwrap().contains("private-secret"));
 assert_eq!(rpc(&mut socket,3,"get_pending_hitl_count",json!({})).await["result"],1);
 let remote=rpc(&mut socket,4,"list_hitl_pending",json!({})).await;assert!(!remote.to_string().contains("private-secret"));
 let id=local["id"].clone();let nonce=local["resolutionNonce"].clone();
 let wrong=rpc(&mut socket,5,"resolve_hitl_remote",json!({"id":id,"resolutionNonce":"wrong","verdict":"approve"})).await;assert_eq!(wrong["result"]["outcome"],"nonce-mismatch");
 let modify=rpc(&mut socket,6,"resolve_hitl_remote",json!({"id":id,"resolutionNonce":nonce,"verdict":"modify","replacement":"echo safe"})).await;assert_eq!(modify["result"]["outcome"],"bad-verdict");
 assert_eq!(rpc(&mut socket,7,"resolve_hitl_request",json!({"id":id,"verdict":"modify","replacement":"echo approved replacement"})).await["result"],true);
 let reply=waiting.await.unwrap();assert_eq!(reply["ok"],true,"{reply}");assert_eq!(reply["value"]["replacement"],"echo approved replacement");
 assert_eq!(rpc(&mut socket,8,"get_pending_hitl_count",json!({})).await["result"],0);
 assert_eq!(rpc(&mut socket,9,"resolve_hitl_request",json!({"id":id,"verdict":"modify","replacement":"another input"})).await["result"],false);
 let audit=rpc(&mut socket,10,"list_hitl_audit_remote",json!({"workspaceId":wid})).await;assert_eq!(audit["result"]["items"][0]["verdict"],"modify");assert!(!audit.to_string().contains("private-secret"));assert!(!audit.to_string().contains("approved replacement"));
 let raw=std::fs::read_to_string(root.join("gateway-permissions.jsonl")).unwrap();assert!(!raw.contains("private-secret"));assert!(!raw.contains("approved replacement"));
 let rejected=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"request_gateway_approval","native":false,"roots":[],"connectionId":Uuid::new_v4(),"args":{"workspaceId":wid,"paneId":pane,"action":"rm -rf /"}}))).unwrap();assert_eq!(rejected["ok"],false);


 {
  use std::io::Write;
  let body=json!({"method":"submit_gateway_input","native":true,"roots":[],"connectionId":Uuid::new_v4(),"args":{"workspaceId":wid,"paneId":pane,"initiator":"detaching caller","text":"rm -rf / # fixture-never-execute\n"}}).to_string();
  let mut caller=std::net::TcpStream::connect(("127.0.0.1",owned.endpoint.port)).unwrap();
  write!(caller,"POST /v1/domain/commands HTTP/1.1\r\nHost: localhost\r\nx-ridge-kernel-token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",owned.endpoint.token,body.len(),body).unwrap();
  let pending=tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let rows=rpc(&mut socket,30,"list_hitl_pending_local",json!({})).await;if rows["result"].as_array().is_some_and(|rows|!rows.is_empty()){break rows["result"][0]["id"].clone();}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap();
  drop(caller);
  assert_eq!(rpc(&mut socket,31,"resolve_hitl_request",json!({"id":pending,"verdict":"modify","replacement":"echo KERNEL_OWNED_GATEWAY_SUBMIT"})).await["result"],true);
  tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{if snapshot_has_line(&owned.endpoint,&wid,pane,"KERNEL_OWNED_GATEWAY_SUBMIT"){break;}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap();
  assert!(!String::from_utf8_lossy(&client::scrollback_domain_pty(&owned.endpoint,pane,64*1024).unwrap()).contains("fixture-never-execute"));
 }
 let endpoint=owned.endpoint.clone();let workspace=wid.clone();
 let paused_submit=tokio::task::spawn_blocking(move||client::request_json(&endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"submit_gateway_input","native":true,"roots":[],"connectionId":Uuid::new_v4(),"args":{"workspaceId":workspace,"paneId":pane,"initiator":"owned requester","text":"rm -rf / # paused-fixture-never-execute\n"}}))).unwrap());
 let pending=tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let rows=rpc(&mut socket,32,"list_hitl_pending_local",json!({})).await;if rows["result"].as_array().is_some_and(|rows|!rows.is_empty()){break rows["result"][0]["id"].clone();}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap();
 assert!(rpc(&mut observer,4,"set_automation_input_paused",json!({"workspaceId":wid,"paneId":pane,"paused":true})).await["error"].is_null());
 assert_eq!(rpc(&mut socket,33,"resolve_hitl_request",json!({"id":pending,"verdict":"modify","replacement":"echo MUST_NOT_ENTER_PAUSED_KERNEL"})).await["result"],true);
 let denied=paused_submit.await.unwrap();assert_eq!(denied["ok"],false,"{denied}");assert!(!String::from_utf8_lossy(&client::scrollback_domain_pty(&owned.endpoint,pane,64*1024).unwrap()).contains("MUST_NOT_ENTER_PAUSED_KERNEL"));
 assert!(rpc(&mut observer,5,"set_automation_input_paused",json!({"workspaceId":wid,"paneId":pane,"paused":false})).await["error"].is_null());
 let endpoint=owned.endpoint.clone();let workspace=wid.clone();
 let stale_waiter=tokio::task::spawn_blocking(move||client::request_json(&endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"request_gateway_approval","native":true,"roots":[],"connectionId":Uuid::new_v4(),"args":{"workspaceId":workspace,"paneId":pane,"initiator":"owned-requester","action":"rm -rf /"}}))).unwrap());
 let stale=tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let pending=rpc(&mut socket,13,"list_hitl_pending_local",json!({})).await;if pending["result"].as_array().is_some_and(|rows|!rows.is_empty()){break pending["result"][0]["id"].clone();}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.unwrap();
 let closed=client::request_json(&owned.endpoint,"DELETE",&format!("/v1/domain/ptys/{pane}"),None).unwrap();assert_eq!(closed["ok"],true,"{closed}");
 assert!(rpc(&mut socket,14,"resolve_hitl_request",json!({"id":stale,"verdict":"approve"})).await["error"].is_object());
 assert_eq!(rpc(&mut socket,15,"resolve_hitl_request",json!({"id":stale,"verdict":"reject"})).await["result"],true);
 assert_eq!(stale_waiter.await.unwrap()["value"]["verdict"],"reject");
 drop(socket);drop(owned);let restarted=start(&root).await;let mut socket=attach(&restarted.endpoint).await;
 let replay=rpc(&mut socket,11,"list_hitl_audit_remote",json!({})).await;assert!(replay["result"]["items"].as_array().unwrap().iter().any(|row|row["id"]==id));
 assert_eq!(rpc(&mut socket,12,"get_pending_hitl_count",json!({})).await["result"],0);
}

fn snapshot_has_line(endpoint:&ridge_kernel::registry::KernelEndpoint,workspace:&str,pane:Uuid,expected:&str)->bool{
 let result=client::request_json(endpoint,"POST",&format!("/v1/domain/ptys/{pane}/terminal"),Some(&json!({"method":"get_terminal_snapshot","params":{"workspaceId":workspace,"paneId":pane,"activationId":1}}))).unwrap();
 assert_eq!(result["ok"],true,"{result}");
 let bytes=base64::engine::general_purpose::STANDARD.decode(result["value"].as_str().unwrap()).unwrap();
 let frame=ridge_term::terminal_protocol::decode_frame(&bytes).unwrap();
 let ridge_term::terminal_protocol::TerminalBody::Snapshot(snapshot)=frame.body else{panic!("expected semantic snapshot")};
 snapshot.primary.lines.iter().chain(snapshot.scrollback.iter()).any(|line|line.cells.iter().map(|cell|cell.ch).collect::<String>().trim()==expected)
}
