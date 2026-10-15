#[path="common/runtime_fixture.rs"]
mod fixture;
use fixture::{attach,rpc,start};
use ridge_kernel::client;
use serde_json::{json,Value};
use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
use tokio::{io::{AsyncReadExt,AsyncWriteExt},net::TcpListener};
use uuid::Uuid;

#[tokio::test(flavor="multi_thread",worker_threads=2)]
async fn native_cloud_api_uses_runtime_and_remote_redirect_and_size_guards_precede_network(){
    let root=std::env::temp_dir().join(format!("ridge-cloud-http-{}",Uuid::new_v4()));
    let owned=start(&root).await;
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();let port=listener.local_addr().unwrap().port();
    let calls=Arc::new(AtomicUsize::new(0));let seen=calls.clone();
    let server=tokio::spawn(async move{
        loop{
            let (mut socket,_)=listener.accept().await.unwrap();let calls=seen.clone();
            tokio::spawn(async move{
                let mut input=Vec::new();let mut bytes=[0;4096];
                loop{
                    let count=socket.read(&mut bytes).await.unwrap();if count==0{return;}
                    input.extend_from_slice(&bytes[..count]);
                    if let Some(end)=input.windows(4).position(|bytes|bytes==b"\r\n\r\n"){
                        let head=String::from_utf8_lossy(&input[..end]);
                        let length=head.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("content-length: ").and_then(|value|value.parse::<usize>().ok())).unwrap_or(0);
                        if input.len()>=end+4+length{break;}
                    }
                    assert!(input.len()<16384);
                }
                calls.fetch_add(1,Ordering::SeqCst);
                let input=String::from_utf8(input).unwrap();
                let response=if input.starts_with("GET /redirect "){
                    "HTTP/1.1 302 Found\r\nLocation: http://forbidden.invalid/api/v1/leak\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
                }else if input.starts_with("GET /large "){
                    "HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n".into()
                }else{
                    assert!(input.starts_with("POST /api/v1/login "));assert!(input.to_ascii_lowercase().contains("authorization: bearer owned-user"));
                    assert!(input.ends_with("{\"owned\":true}"));
                    let body=r#"{"ok":false,"error":{"code":"OWNED_DENIED","message":"owned fixture"}}"#;
                    format!("HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len())
                };
                let _=socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let mut native=attach(&owned.endpoint).await;
    let request=json!({"method":"POST","url":format!("http://127.0.0.1:{port}/api/v1/login"),"headers":{"authorization":"Bearer owned-user","content-type":"application/json"},"body":"{\"owned\":true}"});
    let response=rpc(&mut native,1,"cloud_http",request.clone()).await;
    assert!(response["error"].is_null(),"{response}");assert_eq!(response["result"]["status"],401);
    assert_eq!(serde_json::from_str::<Value>(response["result"]["body"].as_str().unwrap()).unwrap()["error"]["code"],"OWNED_DENIED");
    let remote=client::request_json(&owned.endpoint,"POST","/v1/domain/commands",Some(&json!({"method":"cloud_http","args":request,"native":false,"roots":[],"connectionId":Uuid::new_v4()}))).unwrap();
    assert_eq!(remote["ok"],false);assert_eq!(calls.load(Ordering::SeqCst),1,"remote proxy denial must precede network access");
    for (index,url) in ["file:///owned","https://9527127.xyz.forbidden.invalid/api","http://owned@127.0.0.1/api","http://127.0.0.1.forbidden.invalid/api"].into_iter().enumerate(){
        let denied=rpc(&mut native,index as u64+2,"cloud_http",json!({"method":"GET","url":url,"headers":{},"body":null})).await;
        assert!(denied["error"].is_object(),"{denied}");
    }
    assert_eq!(calls.load(Ordering::SeqCst),1);
    for (index,path) in ["redirect","large"].into_iter().enumerate(){
        let denied=rpc(&mut native,index as u64+10,"cloud_http",json!({"method":"GET","url":format!("http://127.0.0.1:{port}/{path}"),"headers":{},"body":null})).await;
        assert!(denied["error"].is_object(),"{denied}");
    }
    assert_eq!(calls.load(Ordering::SeqCst),3);server.abort();
}
