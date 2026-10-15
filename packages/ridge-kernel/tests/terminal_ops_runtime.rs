#[path="common/runtime_fixture.rs"]
mod fixture;
use fixture::{start,attach,rpc};
use serde_json::json;
use uuid::Uuid;
use base64::Engine;
use futures::{SinkExt,StreamExt};
use tokio_tungstenite::tungstenite::Message;
use ridge_term::terminal_protocol::{decode_frame,TerminalBody};
#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn terminal_clear_updates_shared_history_without_replacing_child_and_mode_is_durable(){
    let root=std::env::temp_dir().join(format!("ridge-terminal-ops-{}",Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;let mut first=attach(&owned.endpoint).await;
    let workspace=rpc(&mut first,1,"create_workspace",json!({"name":"terminal commands"})).await["result"].clone();
    let detail=rpc(&mut first,2,"get_workspace_snapshot",json!({"workspaceId":workspace})).await;
    let pane=detail["result"]["panes"][0]["id"].clone();
    let before=ridge_kernel::client::list_domain_ptys(&owned.endpoint).unwrap().into_iter().find(|pty|Some(pty.pty_id.to_string().as_str())==pane.as_str()).unwrap();
    let instance=ridge_core::process_identity::instance(before.child_pid.unwrap()).unwrap();
    first.send(Message::Text(json!({"type":"subscribe-pane","workspaceId":workspace,"paneId":pane,"activationId":1,"active":true}).to_string())).await.unwrap();
    let original=tokio::time::timeout(std::time::Duration::from_secs(5),async{loop{if let Message::Binary(bytes)=first.next().await.unwrap().unwrap(){if bytes.first()==Some(&0x13){break decode_frame(&bytes).unwrap();}}}}).await.unwrap();
    let TerminalBody::Snapshot(initial)=original.body else{panic!("initial snapshot required")};
    let mut second=attach(&owned.endpoint).await;
    assert!(rpc(&mut second,1,"clear_pane_terminal",json!({"workspaceId":workspace,"paneId":pane})).await["error"].is_null());
    let cleared=rpc(&mut first,3,"get_terminal_snapshot",json!({"workspaceId":workspace,"paneId":pane,"activationId":1})).await;
    let cleared=decode_frame(&base64::engine::general_purpose::STANDARD.decode(cleared["result"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(cleared.terminal_epoch,original.terminal_epoch);assert!(ridge_core::process_identity::validate(&instance).is_ok());
    let TerminalBody::Snapshot(snapshot)=cleared.body else{panic!("clear must expose canonical snapshot")};
    assert!(snapshot.history.epoch>initial.history.epoch,"clear must invalidate the old semantic history cursor");
    assert!(!rpc(&mut second,2,"clear_pane_terminal",json!({"workspaceId":Uuid::new_v4(),"paneId":pane})).await["error"].is_null());
    let mode=json!({"Editor":{"file_path":root.join("readme.md"),"language":"markdown"}});
    assert!(rpc(&mut first,4,"toggle_mode",json!({"workspaceId":workspace,"paneId":pane,"mode":mode})).await["error"].is_null());
    let saved:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("workspace-graph.json")).unwrap()).unwrap();
    assert_eq!(saved["workspaces"][workspace.as_str().unwrap()]["pane_tree"]["panes"][pane.as_str().unwrap()]["mode"],mode);
    assert!(!rpc(&mut first,5,"toggle_mode",json!({"workspaceId":workspace,"paneId":pane,"mode":{"Editor":{"file_path":root.parent(),"language":"text"}}})).await["error"].is_null(),"editor path must honor Runtime filesystem roots");
    let png="iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Y9ZrWQAAAAASUVORK5CYII=";
    let uploaded=rpc(&mut first,6,"save_clipboard_image_to_temp",json!({"pngBase64":png})).await;
    let path=uploaded["result"].as_str().unwrap();
    assert_eq!(std::fs::read(path).unwrap(),base64::engine::general_purpose::STANDARD.decode(png).unwrap());
    assert_eq!(rpc(&mut second,3,"resolve_pasted_image_path",json!({"text":format!("\"{path}\"")})).await["result"],path);
    assert!(!rpc(&mut second,4,"resolve_pasted_image_path",json!({"text":root.parent().unwrap().join("outside.png")})).await["error"].is_null());
    assert!(!rpc(&mut first,7,"save_clipboard_image_to_temp",json!({"pngBase64":"bm90IGEgcG5n"})).await["error"].is_null(),"bad PNG cannot become an attachment file");
    assert_eq!(std::fs::read_dir(root.join("clipboard")).unwrap().count(),1);
    let mut large=base64::engine::general_purpose::STANDARD.decode(png).unwrap();large.resize(2*1024*1024,0);
    let uploaded=rpc(&mut first,8,"save_clipboard_image_to_temp",json!({"pngBase64":base64::engine::general_purpose::STANDARD.encode(&large)})).await;
    assert!(uploaded["error"].is_null(),"bounded attachment must cross the shared RPC body boundary: {uploaded}");
    assert_eq!(std::fs::read(uploaded["result"].as_str().unwrap()).unwrap(),large);
}
