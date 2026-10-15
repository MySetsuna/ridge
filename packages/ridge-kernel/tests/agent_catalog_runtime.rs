//! The existing Agent override file has one Runtime writer and a read-only preview.
#[path="common/runtime_fixture.rs"] mod fixture;
use fixture::{start,attach,rpc};
use ridge_kernel::client;
use serde_json::json;
use uuid::Uuid;

#[test]
#[ignore = "owned running CLI fixture"]
fn discovered_agent_fixture() { loop { std::thread::sleep(std::time::Duration::from_secs(1)); } }

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn catalog_discovers_exact_pane_process_without_inventing_membership_or_history() {
 let root=std::env::temp_dir().join(format!("ridge-agent-observation-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
 let binary=root.join(if cfg!(windows){"custom-observed-agent.exe"}else{"custom-observed-agent"});
 std::fs::copy(std::env::current_exe().unwrap(),&binary).unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;
 let profile=json!({"id":"custom-observed","processNames":["custom-observed-agent"],"executable":"custom-observed-agent","resumeArgv":[],"yoloArgs":[]});
 assert!(rpc(&mut socket,1,"save_agent_profile_overrides",json!({"overrides":[profile]})).await["error"].is_null());
 let workspace=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",Some(&json!({"name":"observed"}))).unwrap()["workspace_id"].clone();
 let graph=client::request_json(&owned.endpoint,"GET",&format!("/v1/domain/workspaces/{}",workspace.as_str().unwrap()),None).unwrap();
 let pane=graph["panes"][0].clone();
 let launch=client::request_json(&owned.endpoint,"POST","/v1/domain/ptys",Some(&json!({"pty_id":pane,"workspace_id":workspace,"program":binary,"args":["--exact","discovered_agent_fixture","--ignored"],"cwd":root,"cols":80,"rows":24}))).unwrap();
 assert_eq!(launch["ok"],true,"{launch}");
 let topology=rpc(&mut socket,2,"get_teammate_topology",json!({"workspaceId":workspace})).await;
 assert!(topology["error"].is_null(),"{topology}");
 assert_eq!(topology["result"]["roster"],json!([]));
 let observed=&topology["result"]["discovered"];assert_eq!(observed.as_array().unwrap().len(),1,"{topology}");
 assert_eq!(observed[0]["paneId"],pane);assert_eq!(observed[0]["name"],"custom-observed");
 assert_eq!(observed[0]["connectionState"],"discovered");assert!(observed[0].get("sessionId").is_none());
 let original=client::request_json(&owned.endpoint,"GET","/v1/domain/ptys",None).unwrap();
 let process=original["ptys"].as_array().unwrap().iter().find(|p|p["pty_id"]==pane).unwrap();
 assert_eq!(observed[0]["process"]["pid"],process["child_pid"]);
 assert_eq!(observed[0]["process"]["startInstance"],process["child_start_instance"]);
 let view_args=json!({"workspaceId":workspace,"paneId":pane,"process":observed[0]["process"],"terminalEpoch":observed[0]["terminalEpoch"]});
 let view=rpc(&mut socket,20,"get_agent_terminal_view",view_args.clone()).await;
 assert!(view["error"].is_null(),"{view}");assert_eq!(view["result"]["paneId"],pane);
 assert_eq!(view["result"]["keyBytes"]["Enter"],json!([13]));
 let mut stale=view_args.clone();stale["terminalEpoch"]=json!("replaced");
 assert!(rpc(&mut socket,21,"get_agent_terminal_view",stale).await["error"].is_object());
 let mut reused=view_args.clone();reused["process"]["startInstance"]=json!("reused-pid");
 assert!(rpc(&mut socket,22,"get_agent_terminal_view",reused).await["error"].is_object());
 let outside=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",Some(&json!({"name":"outside"}))).unwrap()["workspace_id"].clone();
 assert_eq!(rpc(&mut socket,3,"get_teammate_topology",json!({"workspaceId":outside})).await["result"]["discovered"],json!([]));
 let mut cross=view_args.clone();cross["workspaceId"]=outside.clone();
 assert!(rpc(&mut socket,23,"get_agent_terminal_view",cross).await["error"].is_object());
 let input=json!({"process":observed[0]["process"],"input":{"pane":{"workspaceId":workspace,"paneId":pane},"terminalEpoch":"replaced","activationId":1,"sourceId":"owned-sidebar","sequence":1,"bytes":[13]}});
 assert!(rpc(&mut socket,24,"write_agent_terminal_input",input).await["error"].is_object());
 for method in ["get_agent_terminal_view","write_agent_terminal_input"] {
  let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":method,"args":view_args,"native":false,"roots":[root],"connectionId":Uuid::new_v4()}))).unwrap();assert_eq!(denied["ok"],false,"{denied}");
 }
 let remote=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"get_teammate_topology","args":{"workspaceId":workspace},"native":false,"roots":[root],"connectionId":Uuid::new_v4()}))).unwrap();
 assert_eq!(remote["value"]["discovered"],json!([]),"{remote}");
 let history=rpc(&mut socket,4,"read_agent_recent_replies",json!({"workspaceId":workspace})).await;
 assert_eq!(history["result"],json!([]));
 let after=client::request_json(&owned.endpoint,"GET","/v1/domain/ptys",None).unwrap();
 assert_eq!(after,original,"reading observations must not create, replace or resize user processes");
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn catalog_preserves_metadata_archives_and_refuses_corruption() {
 let root=std::env::temp_dir().join(format!("ridge-agent-catalog-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;
 let file=root.join("agent-profile-overrides.json");
 let profile=json!({"id":"user-agent","processNames":["user-cli"],"executable":"user-cli","resumeArgv":[],"yoloArgs":[],"futureMetadata":{"protocol":"acp"}});
 let preview=rpc(&mut socket,1,"list_agent_profiles",json!({"overrides":[profile.clone()]})).await;
 assert!(preview["result"].as_array().unwrap().iter().any(|row|row["id"]=="user-agent"&&row["futureMetadata"]["protocol"]=="acp"));
 assert_eq!(preview["result"].as_array().unwrap().iter().find(|row|row["id"]=="user-agent").unwrap()["yoloPosition"],"before");
 assert!(!file.exists(),"preview must never persist");
 assert!(rpc(&mut socket,2,"save_agent_profile_overrides",json!({"overrides":[profile.clone()]})).await["error"].is_null());
 let original=std::fs::read(&file).unwrap();
 let mut changed=profile.clone();changed.as_object_mut().unwrap().remove("futureMetadata");changed["processNames"]=json!(["changed-cli"]);
 assert!(rpc(&mut socket,3,"save_agent_profile_overrides",json!({"overrides":[changed]})).await["error"].is_null());
 let raw=rpc(&mut socket,4,"read_agent_profile_overrides",json!({})).await;
 assert_eq!(raw["result"][0]["futureMetadata"]["protocol"],"acp");
 assert_eq!(raw["result"][0]["processNames"],json!(["changed-cli"]));
 let domain=client::request_json(&owned.endpoint,"GET","/v1/domain/agents",None).unwrap();
 assert_eq!(domain["ok"],true);
 let listed=domain["profiles"].as_array().unwrap().iter().find(|row|row["id"]=="user-agent").unwrap();
 assert_eq!(listed["processNames"],raw["result"][0]["processNames"]);
 assert_eq!(listed["futureMetadata"],raw["result"][0]["futureMetadata"]);

 assert!(std::fs::read_dir(&root).unwrap().flatten().any(|entry|entry.file_name().to_string_lossy().starts_with("agent-profile-overrides.archive-")&&std::fs::read(entry.path()).ok().as_deref()==Some(original.as_slice())));
 for method in ["list_agent_profiles","read_agent_profile_overrides","save_agent_profile_overrides"] {
  let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":method,"args":{"overrides":[profile]},"native":false,"roots":[root],"connectionId":Uuid::new_v4()}))).unwrap();assert_eq!(denied["ok"],false);
 }
 drop(socket);drop(owned);let restarted=start(&root).await;let mut socket=attach(&restarted.endpoint).await;
 assert_eq!(rpc(&mut socket,5,"read_agent_profile_overrides",json!({})).await["result"],raw["result"]);
 let corrupt=b"{broken";std::fs::write(&file,corrupt).unwrap();
 assert!(rpc(&mut socket,6,"list_agent_profiles",json!({})).await["error"].is_object());
 assert!(rpc(&mut socket,7,"save_agent_profile_overrides",json!({"overrides":[]})).await["error"].is_object());
 assert_eq!(client::request_json(&restarted.endpoint,"GET","/v1/domain/agents",None).unwrap()["ok"],false);
 assert_eq!(std::fs::read(&file).unwrap(),corrupt);
}

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn concurrent_catalog_writers_preserve_each_peers_unknown_metadata() {
 let root=std::env::temp_dir().join(format!("ridge-catalog-writers-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
 let initial=json!([{"id":"shared-profile","processNames":["initial"],"executable":"shared-cli","resumeArgv":[],"yoloArgs":[],"originalMetadata":"retained"}]);
 let file=root.join("agent-profile-overrides.json");let original=serde_json::to_vec(&initial).unwrap();std::fs::write(&file,&original).unwrap();
 let owned=start(&root).await;let mut a=attach(&owned.endpoint).await;let mut b=attach(&owned.endpoint).await;
 let mut first=initial.clone();first[0].as_object_mut().unwrap().remove("originalMetadata");first[0]["processNames"]=json!(["first"]);first[0]["peerOneMetadata"]=json!({"retained":true});
 let mut second=initial.clone();second[0].as_object_mut().unwrap().remove("originalMetadata");second[0]["processNames"]=json!(["second"]);second[0]["peerTwoMetadata"]=json!({"retained":true});
 let (first_result,second_result)=tokio::join!(rpc(&mut a,1,"save_agent_profile_overrides",json!({"overrides":first})),rpc(&mut b,1,"save_agent_profile_overrides",json!({"overrides":second})));
 assert!(first_result["error"].is_null(),"{first_result}");assert!(second_result["error"].is_null(),"{second_result}");
 let saved=rpc(&mut a,2,"read_agent_profile_overrides",json!({})).await;
 assert_eq!(saved["result"][0]["originalMetadata"],"retained");
 assert_eq!(saved["result"][0]["peerOneMetadata"]["retained"],true);assert_eq!(saved["result"][0]["peerTwoMetadata"]["retained"],true);
 assert!([json!(["first"]),json!(["second"])].contains(&saved["result"][0]["processNames"]));
 let archives=std::fs::read_dir(&root).unwrap().flatten().filter(|entry|entry.file_name().to_string_lossy().starts_with("agent-profile-overrides.archive-")).map(|entry|std::fs::read(entry.path()).unwrap()).collect::<Vec<_>>();
 assert_eq!(archives.len(),2);assert!(archives.contains(&original));
 assert!(archives.iter().any(|bytes|{let value:serde_json::Value=serde_json::from_slice(bytes).unwrap();value[0]["peerOneMetadata"].is_object()||value[0]["peerTwoMetadata"].is_object()}));
}

#[cfg(windows)]
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn catalog_write_failure_preserves_the_committed_file_and_reports_error() {
 let root=std::env::temp_dir().join(format!("ridge-catalog-write-failure-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
 let file=root.join("agent-profile-overrides.json");
 let original=serde_json::to_vec(&json!([{"id":"retained","processNames":["retained-cli"],"executable":"retained-cli","resumeArgv":[],"yoloArgs":[]}])).unwrap();
 std::fs::write(&file,&original).unwrap();
 let owned=start(&root).await;let mut socket=attach(&owned.endpoint).await;
 let mut permissions=std::fs::metadata(&file).unwrap().permissions();permissions.set_readonly(true);std::fs::set_permissions(&file,permissions).unwrap();
 let result=rpc(&mut socket,1,"save_agent_profile_overrides",json!({"overrides":[]})).await;
 let mut permissions=std::fs::metadata(&file).unwrap().permissions();permissions.set_readonly(false);std::fs::set_permissions(&file,permissions).unwrap();
 assert!(result["error"].is_object(),"a failed write must not acknowledge a commit: {result}");
 assert_eq!(std::fs::read(&file).unwrap(),original);
 assert_eq!(rpc(&mut socket,2,"read_agent_profile_overrides",json!({})).await["result"],serde_json::from_slice::<serde_json::Value>(&original).unwrap());
}
