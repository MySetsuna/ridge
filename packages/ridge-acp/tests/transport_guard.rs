use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use ridge_acp::{AcpErrorWrap, transport::AcpTransport};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn revoked_binding_rejects_dispatch_cancel_and_late_approval_without_resend() {
    let (client, server) = tokio::io::duplex(8192);
    let (read, write) = tokio::io::split(client);
    let transport = AcpTransport::from_io(write, read);
    let valid = Arc::new(AtomicBool::new(true));
    let observed = valid.clone();
    transport.set_connection_guard(Arc::new(move || {
        if observed.load(Ordering::Acquire) {Ok(())} else {Err(AcpErrorWrap::Transport("original peer expired".into()))}
    })).unwrap();
    assert!(transport.set_connection_guard(Arc::new(|| Ok(()))).is_err());
    let (read, mut write) = tokio::io::split(server);
    let mut lines = BufReader::new(read).lines();
    let pending = transport.begin_call("session/prompt", json!({"sessionId":"original"})).unwrap();
    let sent: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(sent["method"], "session/prompt");
    write.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":\"approval\",\"method\":\"session/request_permission\",\"params\":{}}\n").await.unwrap();
    let mut requests = transport.take_peer_requests().unwrap();
    let request = requests.recv().await.unwrap();
    valid.store(false, Ordering::Release);
    assert!(transport.respond(&request.id, Ok(json!({"decision":"accept"}))).is_err());
    assert!(transport.begin_call("session/prompt", json!({})).is_err());
    assert!(transport.notify("session/cancel", json!({})).await.is_err());
    assert!(transport.is_closed());
    assert!(tokio::time::timeout(std::time::Duration::from_secs(1), pending.wait()).await.unwrap().is_err());
    assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(1), lines.next_line()).await.unwrap().unwrap(), None);
}

#[tokio::test]
async fn invalid_initial_proof_permanently_closes_transport_without_wire_data() {
    let (client, server) = tokio::io::duplex(1024);
    let (read, write) = tokio::io::split(client);
    let transport = AcpTransport::from_io(write, read);
    assert!(transport.set_connection_guard(Arc::new(|| Err(AcpErrorWrap::Transport("wrong peer".into())))).is_err());
    assert!(transport.begin_call("initialize", json!({})).is_err());
    let mut lines = BufReader::new(server).lines();
    assert_eq!(tokio::time::timeout(std::time::Duration::from_secs(1), lines.next_line()).await.unwrap().unwrap(), None);
}
