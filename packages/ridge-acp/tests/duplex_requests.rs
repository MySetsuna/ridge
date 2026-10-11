//! Bidirectional ACP wire tests over an already-established stream; no Agent spawn.
use ridge_acp::transport::AcpTransport;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn peer_request_ids_do_not_consume_client_responses_and_respond_once() {
    let (client, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(client);
    let transport = AcpTransport::from_io(write, read);
    let mut requests = transport.take_peer_requests().unwrap();
    assert!(transport.take_peer_requests().is_none());
    let mut notifications = transport.subscribe_notifications();
    let call = tokio::spawn({
        let transport = transport.clone();
        async move {
            transport
                .call("initialize", json!({"protocolVersion":1}))
                .await
        }
    });
    let (reader, mut writer) = tokio::io::split(peer);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    let outgoing: Value = serde_json::from_str(&line).unwrap();
    for id in [json!(outgoing["id"]), json!("approval-string-id")] {
        writer.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission","params":{"sessionId":"existing","options":[]}})).as_bytes()).await.unwrap();
        let request = tokio::time::timeout(Duration::from_secs(2), requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.id, id);
        assert_eq!(request.method, "session/request_permission");
        assert!(!call.is_finished());
        let result = json!({"outcome":{"outcome":"cancelled"}});
        transport.respond(&request.id, Ok(result.clone())).unwrap();
        assert!(transport.respond(&request.id, Ok(result.clone())).is_err());
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["id"], id);
        assert_eq!(reply["result"], result);
        assert!(reply.get("method").is_none());
    }
    writer.write_all(format!("{}\n",json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"existing","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"reply"}}}})).as_bytes()).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), notifications.recv())
            .await
            .unwrap()
            .unwrap()
            .method,
        "session/update"
    );
    writer
        .write_all(
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","id":outgoing["id"],"result":{"protocolVersion":1}})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(
        call.await.unwrap().unwrap().result.unwrap()["protocolVersion"],
        1
    );
    transport.shutdown();
}

#[tokio::test]
async fn peer_eof_closes_transport_and_wakes_pending_calls() {
    let (client, peer) = tokio::io::duplex(4096);
    let (read, write) = tokio::io::split(client);
    let transport = AcpTransport::from_io(write, read);
    let call = tokio::spawn({
        let transport = transport.clone();
        async move {
            transport
                .call("session/load", json!({"sessionId":"existing"}))
                .await
        }
    });
    tokio::task::yield_now().await;
    drop(peer);
    assert!(tokio::time::timeout(Duration::from_secs(2), call)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(transport.is_closed());
    assert!(transport.call("session/load", json!({})).await.is_err());
}
