//! Existing history data commits only through the service, retaining archived originals.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{start,attach,rpc};
use ridge_kernel::{client, registry::KernelEndpoint};
use serde_json::{json, Value};
use std::{fs,time::Duration};
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;
fn history(endpoint: &KernelEndpoint, args: Value) -> Value {
    client::request_json(endpoint,"POST","/v1/domain/workspace-files",Some(&args)).unwrap()
}
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn legacy_history_fields_and_deleted_records_remain_recoverable() {
    let root=std::env::temp_dir().join(format!("ridge-snapshot-history-{}",Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path=root.join("workspace_history.json");
    let original=json!({"legacyStore":"keep","items":[{"id":"existing","name":"old","saved_at":"2000-01-01","pane_count":0,"is_pinned":false,"pane_tree_json":"{}","legacyItem":"keep"}]});
    let bytes=serde_json::to_vec(&original).unwrap();
    fs::write(&path,&bytes).unwrap();
    let owned=start(&root).await;
    let listed=history(&owned.endpoint,json!({"operation":"snapshot-list"}));
    assert_eq!(listed["value"],original["items"]);
    let renamed=history(&owned.endpoint,json!({"operation":"snapshot-rename","path":"existing","name":"renamed"}));
    assert_eq!(renamed["ok"],true,"{renamed}");
    assert_eq!(history(&owned.endpoint,json!({"operation":"snapshot-pin","path":"existing"}))["ok"],true);
    let stored:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["legacyStore"],"keep");
    assert_eq!(stored["items"][0]["legacyItem"],"keep");
    assert_eq!(stored["items"][0]["name"],"renamed");
    assert_eq!(stored["items"][0]["is_pinned"],true);
    assert!(fs::read_dir(&root).unwrap().flatten().any(|entry|
        entry.file_name().to_string_lossy().starts_with("workspace_history.archive-")
        && fs::read(entry.path()).ok().as_deref()==Some(bytes.as_slice())));
    let workspace=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",Some(&json!({"name":"live authority"}))).unwrap()["workspace_id"].clone();
    let saved=history(&owned.endpoint,json!({"operation":"snapshot-save","workspaceId":workspace}));
    assert_eq!(saved["ok"],true,"{saved}");
    let item=history(&owned.endpoint,json!({"operation":"snapshot-get","path":saved["value"]}));
    assert_eq!(item["value"]["name"],"live authority");
    assert!(serde_json::from_str::<Value>(item["value"]["pane_tree_json"].as_str().unwrap()).unwrap().is_object());
    let other=client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",Some(&json!({"name":"second client"}))).unwrap()["workspace_id"].clone();
    let mut first=attach(&owned.endpoint).await;
    let mut second=attach(&owned.endpoint).await;
    rpc(&mut first,1,"switch_workspace",json!({"workspaceId":workspace})).await;
    rpc(&mut second,1,"switch_workspace",json!({"workspaceId":other})).await;
    let mut notifications=attach(&owned.endpoint).await;
    rpc(&mut notifications,0,"list_saved_workspaces",json!({})).await;
    let first_saved=rpc(&mut first,2,"save_workspace",json!({})).await;
    let notification=tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            if let Some(Ok(Message::Text(text)))=notifications.next().await {
                let event:Value=serde_json::from_str(&text).unwrap();
                if event["name"]=="workspace-history-changed" {break event;}
            }
        }
    }).await.expect("other presentation must observe committed history");
    assert!(notification["payload"].is_null(),"history notification must not leak content");
    drop(notifications);
    let second_saved=rpc(&mut second,2,"save_workspace",json!({})).await;
    assert!(first_saved["result"].is_string(),"{first_saved}");
    assert!(second_saved["result"].is_string(),"{second_saved}");
    let public=rpc(&mut first,3,"list_saved_workspaces",json!({})).await;
    assert!(public["result"].as_array().unwrap().iter().any(|item|item["id"]==first_saved["result"] && item["name"]=="live authority"));
    assert!(public["result"].as_array().unwrap().iter().any(|item|item["id"]==second_saved["result"] && item["name"]=="second client"));
    let reopened=rpc(&mut first,4,"restore_workspace",json!({"historyId":saved["value"]})).await;
    assert_eq!(reopened["result"],workspace);
    let selection=rpc(&mut second,4,"get_active_workspace_id",json!({})).await;
    assert_eq!(selection["result"],other,"restoration must not steal another client's selection");
    assert!(rpc(&mut first,5,"delete_saved_workspace",json!({"historyId":first_saved["result"]})).await["error"].is_null());
    assert!(rpc(&mut second,5,"delete_saved_workspace",json!({"historyId":second_saved["result"]})).await["error"].is_null());
    drop(first);drop(second);
    let mut scope_store:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut hidden=item["value"].clone();
    hidden["id"]=json!("outside-root");
    let mut hidden_tree:Value=serde_json::from_str(hidden["pane_tree_json"].as_str().unwrap()).unwrap();
    for pane in hidden_tree["panes"].as_object_mut().unwrap().values_mut() {pane["cwd"]=json!(std::env::temp_dir().join("outside-history-scope"));}
    hidden["pane_tree_json"]=json!(serde_json::to_string(&hidden_tree).unwrap());
    scope_store["items"].as_array_mut().unwrap().push(hidden);
    let scope_bytes=serde_json::to_vec(&scope_store).unwrap();
    fs::write(&path,&scope_bytes).unwrap();
    let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"rename_saved_workspace","args":{"historyId":"outside-root","name":"must not change"},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(denied["ok"],false,"{denied}");
    assert_eq!(fs::read(&path).unwrap(),scope_bytes);
    let filtered=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"list_saved_workspaces","args":{},"native":false,"roots":[root],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert!(!filtered["value"].as_array().unwrap().iter().any(|item|item["id"]=="outside-root"));
    assert_eq!(history(&owned.endpoint,json!({"operation":"snapshot-delete","path":"outside-root"}))["ok"],true);
    let mut payload:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut unbound=item["value"].clone();
    unbound.as_object_mut().unwrap().remove("workspace_id");
    unbound["id"]=json!("unbound-copy");
    payload["items"].as_array_mut().unwrap().push(unbound);
    let conflict_bytes=serde_json::to_vec(&payload).unwrap();
    fs::write(&path,&conflict_bytes).unwrap();
    let conflict=history(&owned.endpoint,json!({"operation":"snapshot-restore","path":"unbound-copy"}));
    assert_eq!(conflict["ok"],false,"{conflict}");
    assert_eq!(fs::read(&path).unwrap(),conflict_bytes);
    assert_eq!(history(&owned.endpoint,json!({"operation":"snapshot-delete","path":"unbound-copy"}))["ok"],true);
    let restored=history(&owned.endpoint,json!({"operation":"snapshot-restore","path":saved["value"]}));
    assert_eq!(restored["ok"],true,"{restored}");
    assert_eq!(restored["value"]["workspaceId"],workspace);
    assert_eq!(restored["value"]["alreadyOpen"],true);
    client::request_json(&owned.endpoint,"POST","/v1/domain/workspaces",None).unwrap();
    let closed=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
        "method":"close_workspace","args":{"workspaceId":workspace},"native":true,"roots":[],"connectionId":Uuid::new_v4()
    }))).unwrap();
    assert_eq!(closed["ok"],true,"{closed}");
    let graph_path=root.join("workspace-graph.json");
    let original_graph=root.join("workspace-graph.fixture-original");
    fs::rename(&graph_path,&original_graph).unwrap();
    fs::create_dir(&graph_path).unwrap();
    let history_bytes=fs::read(&path).unwrap();
    let failed_commit=history(&owned.endpoint,json!({"operation":"snapshot-restore","path":saved["value"]}));
    assert_eq!(failed_commit["ok"],false,"{failed_commit}");
    assert_eq!(fs::read(&path).unwrap(),history_bytes);
    assert!(!client::read_domain_workspaces(&owned.endpoint).unwrap().entries.iter().any(|entry|entry.id==workspace.as_str().unwrap()));
    fs::remove_dir(&graph_path).unwrap();
    fs::rename(&original_graph,&graph_path).unwrap();
    let restored=history(&owned.endpoint,json!({"operation":"snapshot-restore","path":saved["value"]}));
    assert_eq!(restored["ok"],true,"{restored}");
    assert_eq!(restored["value"]["workspaceId"],workspace);
    assert_eq!(restored["value"]["alreadyOpen"],false);
    assert!(client::list_domain_ptys(&owned.endpoint).unwrap().is_empty(),"history restore must not launch PTYs");
    let deleted=history(&owned.endpoint,json!({"operation":"snapshot-delete","path":"existing"}));
    assert_eq!(deleted["ok"],true,"{deleted}");
    drop(owned);
    let restarted=start(&root).await;
    let listed=history(&restarted.endpoint,json!({"operation":"snapshot-list"}));
    assert_eq!(listed["value"].as_array().unwrap().len(),1);
    assert_eq!(listed["value"][0]["id"],saved["value"]);
    let reattached=history(&restarted.endpoint,json!({"operation":"snapshot-restore","path":saved["value"]}));
    assert_eq!(reattached["value"]["workspaceId"],workspace);
    assert_eq!(reattached["value"]["alreadyOpen"],true);
    fs::write(&path,b"42").unwrap();
    let failed=history(&restarted.endpoint,json!({"operation":"snapshot-delete","path":saved["value"]}));
    assert_eq!(failed["ok"],false,"{failed}");
    assert_eq!(fs::read(&path).unwrap(),b"42");
}
