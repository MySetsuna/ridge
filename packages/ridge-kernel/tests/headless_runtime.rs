#[path="common/runtime_fixture.rs"]
mod fixture;
use fixture::{start,attach,rpc};
use ridge_kernel::client;
use serde_json::json;
use uuid::Uuid;
use std::time::{Duration,Instant};
fn origin(layout:&serde_json::Value,pane:Uuid)->Option<&serde_json::Value>{
    if layout["id"]==pane.to_string()&&layout["type"]=="leaf"{return layout.get("origin");}
    layout["children"].as_array()?.iter().find_map(|child|origin(child,pane))
}
async fn file(path:&std::path::Path) {
    let deadline=Instant::now()+Duration::from_secs(5);
    loop {if std::fs::read(path).is_ok_and(|bytes|!bytes.is_empty()){return;}assert!(Instant::now()<deadline,"real native child did not write file");tokio::time::sleep(Duration::from_millis(20)).await;}
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn headless_session_adopts_same_process_detaches_without_kill_and_can_be_resummoned() {
    let root=std::env::temp_dir().join(format!("ridge-headless-owner-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;let mut first=attach(&owned.endpoint).await;
    let workspace=rpc(&mut first,1,"create_workspace",json!({"name":"native view"})).await["result"].clone();
    let http=reqwest::Client::builder().no_proxy().build().unwrap();
    let listed=http.get(format!("http://127.0.0.1:{}/api/v1/tmux/list-sessions",owned.endpoint.port)).header("x-ridge-kernel-token",&owned.endpoint.token).query(&[("format","#S|#{session_windows}|#{session_width}|#{session_height}")]).send().await.unwrap();
    assert!(listed.status().is_success());
    assert!(listed.text().await.unwrap().lines().any(|line|line=="native view|1|80|24"));
    let denied=http.post(format!("http://127.0.0.1:{}/api/v1/tmux/new-session",owned.endpoint.port)).header("x-ridge-kernel-token",&owned.endpoint.token).json(&json!({"socket":"headless","name":"outside","cwd":root.parent()})).send().await.unwrap();
    assert_eq!(denied.status(),reqwest::StatusCode::FORBIDDEN);
    let created=rpc(&mut first,2,"new_headless_session",json!({"name":"owned-headless","cwd":root})).await;
    assert_eq!(created["result"],"owned-headless","{created}");
    assert_eq!(rpc(&mut first,3,"summon_native_session",json!({"socket":"headless","target":"owned-headless","workspaceId":workspace})).await["result"],1);
    let listed=client::list_domain_ptys(&owned.endpoint).unwrap();let original=listed.iter().find(|pty|pty.launch_profile.is_none()&&pty.workspace_id.map(|id|id.to_string())==workspace.as_str().map(str::to_owned)).unwrap();
    let instance=ridge_core::process_identity::instance(original.child_pid.unwrap()).unwrap();let pane=original.pty_id;
    let layout=rpc(&mut first,30,"get_pane_layout_for",json!({"workspaceId":workspace})).await;
    let native_origin=origin(&layout["result"],pane).expect("adopted native view needs its canonical origin").clone();
    assert_eq!(native_origin["kind"],"headless");assert_eq!(native_origin["host_id"],"headless");
    assert_eq!(native_origin["host_label"],"headless");
    assert!(native_origin["session_id"].as_str().unwrap().starts_with("headless:"));
    assert_eq!(rpc(&mut first,4,"summon_native_session",json!({"socket":"headless","target":"owned-headless","workspaceId":workspace})).await["result"],0,"same view summon is idempotent");
    let output=root.join("before.txt");
    let command=format!("echo NATIVE_OWNER_BEFORE > \"{}\"\r\n",output.to_string_lossy().replace('\\',"/"));
    assert!(rpc(&mut first,5,"write_to_pty",json!({"workspaceId":workspace,"paneId":pane,"data":command})).await["error"].is_null());file(&output).await;
    let destination=rpc(&mut first,6,"create_workspace",json!({"name":"moved native view"})).await["result"].clone();
    let graph=root.join("workspace-graph.json");let backup=root.join("workspace-graph.before-move.json");
    std::fs::rename(&graph,&backup).unwrap();std::fs::create_dir(&graph).unwrap();
    let failed=rpc(&mut first,60,"summon_native_session",json!({"socket":"headless","target":"owned-headless","workspaceId":destination})).await;
    assert!(!failed["error"].is_null(),"failed persistence must fail summon: {failed}");
    let retained=client::list_domain_ptys(&owned.endpoint).unwrap();
    assert_eq!(retained.iter().filter(|pty|pty.child_pid==Some(instance.pid)).count(),1,"failed move leaked a second view");
    assert!(retained.iter().any(|pty|pty.pty_id==pane&&pty.workspace_id.map(|id|id.to_string()).as_deref()==workspace.as_str()));
    assert!(ridge_core::process_identity::validate(&instance).is_ok());
    std::fs::remove_dir(&graph).unwrap();std::fs::rename(&backup,&graph).unwrap();
    assert_eq!(rpc(&mut first,7,"summon_native_session",json!({"socket":"headless","target":"owned-headless","workspaceId":destination})).await["result"],1);
    let moved=client::list_domain_ptys(&owned.endpoint).unwrap().into_iter().find(|pty|pty.child_pid==Some(instance.pid)).unwrap();
    assert_eq!(moved.workspace_id.map(|id|id.to_string()).as_deref(),destination.as_str());
    assert_eq!(moved.child_start_instance.as_deref(),Some(instance.start_instance.as_str()));
    let layout=rpc(&mut first,70,"get_pane_layout_for",json!({"workspaceId":destination})).await;
    assert_eq!(origin(&layout["result"],moved.pty_id),Some(&native_origin),"moving a view must retain original native identity");
    assert!(!client::list_domain_ptys(&owned.endpoint).unwrap().iter().any(|pty|pty.pty_id==pane));
    let stale=rpc(&mut first,8,"write_to_pty",json!({"workspaceId":workspace,"paneId":pane,"data":"echo STALE_VIEW\r\n"})).await;
    assert!(!stale["error"].is_null(),"old view must lose input authority: {stale}");
    let workspace=destination;let pane=moved.pty_id;
    let mut second=attach(&owned.endpoint).await;drop(first);
    assert!(ridge_core::process_identity::validate(&instance).is_ok());
    let closed=rpc(&mut second,1,"close_pane",json!({"workspaceId":workspace,"paneId":pane})).await;
    assert!(closed["error"].is_null(),"{closed}");
    assert!(!client::list_domain_ptys(&owned.endpoint).unwrap().iter().any(|pty|pty.pty_id==pane),"{closed}: original view still registered");
    assert!(ridge_core::process_identity::validate(&instance).is_ok(),"closing a view must not kill headless child");
    let inventory=rpc(&mut second,9,"list_native_sessions",json!({})).await;
    let summoned=rpc(&mut second,2,"summon_native_session",json!({"socket":"headless","target":"owned-headless","workspaceId":workspace})).await;
    assert_eq!(summoned["result"],1,"{summoned}; {inventory}; after-close={:?}",client::list_domain_ptys(&owned.endpoint).unwrap());
    let adopted=client::list_domain_ptys(&owned.endpoint).unwrap().into_iter().find(|pty|pty.child_pid==Some(instance.pid)).unwrap();
    assert_eq!(adopted.child_start_instance.as_deref(),Some(instance.start_instance.as_str()));
    let layout=rpc(&mut second,20,"get_pane_layout_for",json!({"workspaceId":workspace})).await;
    assert_eq!(origin(&layout["result"],adopted.pty_id),Some(&native_origin),"resummoning must retain original native identity");
    let output=root.join("after.txt");let command=format!("echo NATIVE_OWNER_AFTER > \"{}\"\r\n",output.to_string_lossy().replace('\\',"/"));
    assert!(rpc(&mut second,3,"write_to_pty",json!({"workspaceId":workspace,"paneId":adopted.pty_id,"data":command})).await["error"].is_null());file(&output).await;
    assert!(rpc(&mut second,4,"terminate_native_session",json!({"socket":"headless","target":"owned-headless"})).await["error"].is_null());
    let deadline=Instant::now()+Duration::from_secs(3);while ridge_core::process_identity::validate(&instance).is_ok(){assert!(Instant::now()<deadline);tokio::time::sleep(Duration::from_millis(20)).await;}
}
