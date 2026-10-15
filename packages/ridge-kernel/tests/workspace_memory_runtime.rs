//! One persistent document owner; Native clients propose changes against a captured snapshot.
#[path="common/runtime_fixture.rs"]mod fixture;
use fixture::{start,attach,rpc};
use ridge_kernel::client;
use serde_json::json;
use uuid::Uuid;
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn memory_owner_preserves_sections_refuses_stale_proposals_and_replays() {
 let root=std::env::temp_dir().join(format!("ridge-memory-owner-{}",Uuid::new_v4()));let dir=root.join("workspace-memory");std::fs::create_dir_all(&dir).unwrap();let wid=Uuid::new_v4();let pane=Uuid::new_v4();let file=dir.join(format!("{wid}.json"));
 let original=serde_json::to_vec(&json!({"suspendedPanes":[pane],"unknown":{"retained":true},"rollbackPatches":[{"id":"old"}]})).unwrap();std::fs::write(&file,&original).unwrap();
 let owned=start(&root).await;let mut a=attach(&owned.endpoint).await;let mut b=attach(&owned.endpoint).await;
 assert_eq!(rpc(&mut a,1,"get_workspace_memory_directory",json!({})).await["result"],json!(dir));
 let snapshot=rpc(&mut a,2,"read_workspace_memory_document",json!({"workspaceId":wid})).await["result"].clone();
 assert!(rpc(&mut b,1,"set_workspace_memory",json!({"workspaceId":wid,"goal":"new goal","constraints":["retain data"],"tasks":[{"id":"task"}]})).await["error"].is_null());
 let stale=rpc(&mut a,3,"commit_workspace_memory_document",json!({"workspaceId":wid,"expected":snapshot,"value":{"goal":"stale"}})).await;assert!(stale["error"].is_object(),"{stale}");
 let current=rpc(&mut a,4,"get_workspace_memory",json!({"workspaceId":wid})).await["result"].clone();assert_eq!(current["goal"],"new goal");assert_eq!(current["unknown"]["retained"],true);assert_eq!(current["suspendedPanes"][0],json!(pane));assert_eq!(current["rollbackPatches"][0]["id"],"old");
 let mut next=current.clone();next.as_object_mut().unwrap().remove("suspendedPanes");assert!(rpc(&mut a,5,"commit_workspace_memory_document",json!({"workspaceId":wid,"expected":current,"value":next})).await["error"].is_null());
 let committed=rpc(&mut a,6,"get_workspace_memory",json!({"workspaceId":wid})).await["result"].clone();assert!(committed.get("suspendedPanes").is_none());assert_eq!(committed["unknown"]["retained"],true);
 let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"commit_workspace_memory_document","native":false,"roots":[],"connectionId":Uuid::new_v4(),"args":{"workspaceId":wid,"expected":committed,"value":{}}}))).unwrap();assert_eq!(denied["ok"],false);
 drop(a);drop(b);drop(owned);let owned=start(&root).await;let mut a=attach(&owned.endpoint).await;assert_eq!(rpc(&mut a,7,"get_workspace_memory",json!({"workspaceId":wid})).await["result"],committed);
 let corrupt=b"{broken";std::fs::write(&file,corrupt).unwrap();assert!(rpc(&mut a,8,"get_workspace_memory",json!({"workspaceId":wid})).await["error"].is_object());assert!(rpc(&mut a,9,"set_workspace_memory",json!({"workspaceId":wid,"goal":"must not overwrite"})).await["error"].is_object());assert_eq!(std::fs::read(&file).unwrap(),corrupt);
 assert!(rpc(&mut a,10,"retire_workspace_memory_document",json!({"workspaceId":wid})).await["error"].is_null());assert!(!file.exists());assert!(std::fs::read_dir(dir).unwrap().flatten().any(|entry|std::fs::read(entry.path()).ok().as_deref()==Some(corrupt)));
}
