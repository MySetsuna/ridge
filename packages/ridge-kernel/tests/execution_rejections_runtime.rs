//! External cards stay in Runtime without becoming permission or execution requests.
#[path = "common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach, rpc, start};
use ridge_kernel::client;
use futures::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use serde_json::json;
use uuid::Uuid;
#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn rejection_cards_survive_presentation_detach_and_never_authorize_execution() {
    let root=std::env::temp_dir().join(format!("ridge-rejections-{}",Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;
    let mut socket=attach(&owned.endpoint).await;
    let mut observer=attach(&owned.endpoint).await;
    let report=json!({"initiator":"tester","action":"secret-command","executor":"external-gateway",
        "policySource":"external-policy","requestId":"secret-request","reason":"secret-reason","nextStep":"use the external gateway"});
    let accepted=rpc(&mut socket,1,"report_execution_rejection",report.clone()).await;
    let id=accepted["result"].as_str().expect("report accepted").to_owned();
    let changed=tokio::time::timeout(std::time::Duration::from_secs(5),async {
        loop {
            if let Some(Ok(Message::Text(text)))=observer.next().await {
                let event:serde_json::Value=serde_json::from_str(&text).unwrap();
                if event["name"]=="teammate://execution-rejections-changed" {break event;}
            }
        }
    }).await.expect("report notification must reach the other client");
    assert!(changed["payload"].is_null());
    assert!(!changed.to_string().contains("secret-command"));
    let list=rpc(&mut socket,2,"list_execution_rejections_local",json!({})).await;
    assert_eq!(list["result"][0]["id"],id);
    assert_eq!(list["result"][0]["action"],"secret-command");
    assert_eq!(rpc(&mut socket,3,"get_pending_hitl_count",json!({})).await["result"],0);
    assert_eq!(rpc(&mut socket,4,"resolve_hitl_request",json!({"id":id,"verdict":"approve"})).await["result"],false);
    for method in ["report_execution_rejection","list_execution_rejections_local","dismiss_execution_rejection"] {
        let denied=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({
            "method":method,"args":report,"native":false,"roots":[root],"connectionId":Uuid::new_v4()
        }))).unwrap();
        assert_eq!(denied["ok"],false,"{denied}");
    }
    let audit=rpc(&mut socket,5,"list_hitl_audit_remote",json!({})).await;
    assert_eq!(audit["result"]["items"][0]["source"],"external-execution-rejection");
    for secret in ["secret-command","secret-request","secret-reason"] {assert!(!audit.to_string().contains(secret));}
    drop(socket);
    let mut presentation=attach(&owned.endpoint).await;
    assert_eq!(rpc(&mut presentation,1,"list_execution_rejections_local",json!({})).await["result"][0]["id"],id);
    assert_eq!(rpc(&mut presentation,2,"dismiss_execution_rejection",json!({"id":id})).await["result"],true);
    assert_eq!(rpc(&mut presentation,3,"dismiss_execution_rejection",json!({"id":id})).await["result"],false);
    assert_eq!(rpc(&mut presentation,4,"list_execution_rejections_local",json!({})).await["result"],json!([]));
    assert_eq!(rpc(&mut presentation,5,"list_hitl_audit_remote",json!({})).await["result"]["items"].as_array().unwrap().len(),1);
    assert!(rpc(&mut presentation,6,"report_execution_rejection",json!({"executor":"incomplete"})).await["error"].is_object());
    for i in 0..51 {let result=rpc(&mut presentation,10+i,"report_execution_rejection",report.clone()).await;assert!(result["result"].is_string(),"{result}");}
    assert_eq!(rpc(&mut presentation,100,"list_execution_rejections_local",json!({})).await["result"].as_array().unwrap().len(),50);
}

#[tokio::test(flavor="multi_thread", worker_threads=2)]
async fn kernel_mcp_reports_use_the_same_local_card_store() {
    use std::io::{Read, Write};
    let root=std::env::temp_dir().join(format!("ridge-mcp-rejections-{}",Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let owned=start(&root).await;
    let body=json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"ridge_report_execution_rejection","arguments":{"executor":"gateway","policy_source":"external",
        "request_id":"request","reason":"denied","next_step":"external authorization"}
    }}).to_string();
    let mut socket=std::net::TcpStream::connect(("127.0.0.1",owned.endpoint.port)).unwrap();
    socket.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    write!(socket,"POST /api/v1/mcp HTTP/1.1\r\nHost: localhost\r\nx-ridge-token: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",owned.endpoint.token,body.len(),body).unwrap();
    let mut response=String::new();socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"),"{response}");
    let reply:serde_json::Value=serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_ne!(reply["result"]["isError"],true,"{reply}");
    let receipt:serde_json::Value=serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(receipt["retry"],"not_available_from_ridge");
    let mut presentation=attach(&owned.endpoint).await;
    let list=rpc(&mut presentation,1,"list_execution_rejections_local",json!({})).await;
    assert_eq!(list["result"][0]["id"],receipt["reportId"]);
    assert_eq!(list["result"][0]["action"],"");
}
