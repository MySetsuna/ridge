//! Standards-shaped wire fixture; proves driver behavior, not external interoperability.
use ridge_acp::{existing::ExistingConversation, transport::AcpTransport};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf};

async fn read(reader: &mut BufReader<ReadHalf<DuplexStream>>) -> Value {
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn send(writer: &mut WriteHalf<DuplexStream>, value: Value) {
    writer
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
}

#[tokio::test]
async fn prompt_completion_closes_observation_before_a_later_notification_in_the_same_read() {
    use std::sync::atomic::{AtomicBool,Ordering};
    let (client,peer)=tokio::io::duplex(65536);let (input,output)=tokio::io::split(client);
    let (reader,mut writer)=tokio::io::split(peer);let mut reader=BufReader::new(reader);
    let agent=tokio::spawn(async move {
        let init=read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}})).await;
        let load=read(&mut reader).await;send(&mut writer,json!({"jsonrpc":"2.0","id":load["id"],"result":{}})).await;
        let prompt=read(&mut reader).await;
        let notify=|text|json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"existing","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}});
        let response=json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}});
        writer.write_all(format!("{}\n{response}\n{}\n",notify("during"),notify("after")).as_bytes()).await.unwrap();
    });
    let active=Arc::new(AtomicBool::new(false));let observed=Arc::new(Mutex::new(Vec::new()));
    let capture=active.clone();let output_events=observed.clone();
    let session=ExistingConversation::load(AcpTransport::from_io(output,input),"existing".into(),std::env::temp_dir().to_string_lossy().into(),Arc::new(move|params| {
        output_events.lock().unwrap().push((params["update"]["content"]["text"].clone(),capture.load(Ordering::Acquire)));Ok(())
    }),Arc::new(|_|Box::pin(async {None}))).await.unwrap();
    active.store(true,Ordering::Release);let finish=active.clone();
    session.prompt_observed("task".into(),Some(Arc::new(move||finish.store(false,Ordering::Release)))).unwrap().await.unwrap().unwrap();
    agent.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2),async {while observed.lock().unwrap().len()<2 {tokio::task::yield_now().await;}}).await.unwrap();
    assert_eq!(*observed.lock().unwrap(),vec![(json!("during"),true),(json!("after"),false)]);
}

#[tokio::test]
async fn loads_exact_existing_session_and_preserves_large_replay_without_new() {
    let (client, peer) = tokio::io::duplex(65536);
    let (input, output) = tokio::io::split(client);
    let transport = AcpTransport::from_io(output, input);
    let (reader, mut writer) = tokio::io::split(peer);
    let mut reader = BufReader::new(reader);
    let agent = tokio::spawn(async move {
        let init = read(&mut reader).await;
        assert_eq!(init["method"], "initialize");
        assert_eq!(init["params"]["clientCapabilities"]["terminal"], false);
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"agentInfo":{"name":"unknown-compatible-agent","version":"test"}}})).await;
        let load = read(&mut reader).await;
        assert_eq!(load["method"], "session/load");
        assert_eq!(load["params"]["sessionId"], "already-running-conversation");
        for index in 0..600 {
            send(&mut writer,json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"already-running-conversation","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":index.to_string()},"opaque":{"preserve":index}}}})).await;
        }
        send(&mut writer,json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"other-conversation","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"must not route"}}}})).await;
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":load["id"],"result":{}}),
        )
        .await;
        let prompt = read(&mut reader).await;
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["sessionId"], load["params"]["sessionId"]);
        send(&mut writer,json!({"jsonrpc":"2.0","id":"approval-id","method":"session/request_permission","params":{"sessionId":"already-running-conversation","toolCall":{"toolCallId":"tool-1"},"options":[{"optionId":"allow-this","name":"Allow","kind":"allow_once"}]}})).await;
        let permission = read(&mut reader).await;
        assert_eq!(permission["id"], "approval-id");
        assert_eq!(permission["result"]["outcome"]["optionId"], "allow-this");
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
        let next = read(&mut reader).await;
        assert_eq!(next["method"], "session/prompt");
        let cancel = read(&mut reader).await;
        assert_eq!(cancel["method"], "session/cancel");
        assert!(cancel.get("id").is_none());
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":next["id"],"result":{"stopReason":"cancelled"}}),
        )
        .await;
    });
    let updates = Arc::new(Mutex::new(Vec::new()));
    let sink = updates.clone();
    let session = ExistingConversation::load(
        transport,
        "already-running-conversation".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(move |value| {
            sink.lock().unwrap().push(value.clone());
            Ok(())
        }),
        Arc::new(|_| Box::pin(async { Some("allow-this".into()) })),
    )
    .await
    .unwrap();
    assert_eq!(updates.lock().unwrap().len(), 600);
    assert_eq!(
        updates.lock().unwrap()[599]["update"]["opaque"]["preserve"],
        599
    );
    assert_eq!(
        session.agent_info().unwrap()["name"],
        "unknown-compatible-agent"
    );
    let first = session.prompt("message".into()).unwrap();
    assert!(session.prompt("must not overlap".into()).is_err());
    assert_eq!(first.await.unwrap().unwrap()["stopReason"], "end_turn");
    let dropped = session.prompt("second".into()).unwrap();
    drop(dropped);
    assert!(session.is_busy());
    session.cancel().await.unwrap();
    agent.await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while session.is_busy() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn malformed_permission_options_do_not_reach_the_user_or_produce_an_approval() {
    let (client, peer) = tokio::io::duplex(4096);
    let (input, output) = tokio::io::split(client);
    let agent = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(peer);
        let mut reader = BufReader::new(reader);
        let init = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}})).await;
        let load = read(&mut reader).await;
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":load["id"],"result":{}}),
        )
        .await;
        let prompt = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":"duplicate-options","method":"session/request_permission",
            "params":{"sessionId":"existing","toolCall":{"toolCallId":"tool"},"options":[{"optionId":"duplicate","name":"Allow","kind":"allow_once"},{"optionId":"duplicate","name":"Deny","kind":"reject_once"}]}})).await;
        let denied = read(&mut reader).await;
        assert_eq!(denied["id"], "duplicate-options");
        assert_eq!(denied["error"]["code"], -32602);
        assert!(denied["result"].is_null());
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
    });
    let session = ExistingConversation::load(
        AcpTransport::from_io(output, input),
        "existing".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(|_| Ok(())),
        Arc::new(|_| panic!("invalid permission options must not be presented")),
    )
    .await
    .unwrap();
    assert_eq!(
        session
            .prompt("invalid permission".into())
            .unwrap()
            .await
            .unwrap()
            .unwrap()["stopReason"],
        "end_turn"
    );
    agent.await.unwrap();
}

#[tokio::test]
async fn resumes_an_existing_session_without_replay_when_the_peer_negotiates_resume() {
    let (client, peer) = tokio::io::duplex(4096);
    let (input, output) = tokio::io::split(client);
    let agent = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(peer);
        let mut reader = BufReader::new(reader);
        let init = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"sessionCapabilities":{"resume":{}}}}})).await;
        let resume = read(&mut reader).await;
        assert_eq!(resume["method"], "session/resume");
        assert_eq!(resume["params"]["sessionId"], "already-existing");
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":resume["id"],"result":{}}),
        )
        .await;
        let prompt = read(&mut reader).await;
        assert_eq!(prompt["method"], "session/prompt");
        send(&mut writer,json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"already-existing","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"observed live"}}}})).await;
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"end_turn"}}),
        )
        .await;
    });
    let observed = Arc::new(Mutex::new(Vec::new()));
    let sink = observed.clone();
    let session = ExistingConversation::load(
        AcpTransport::from_io(output, input),
        "already-existing".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(move |event| {
            sink.lock().unwrap().push(event.clone());
            Ok(())
        }),
        Arc::new(|_| Box::pin(async { None })),
    )
    .await
    .unwrap();
    assert!(!session.history_replayed());
    assert!(observed.lock().unwrap().is_empty());
    assert_eq!(
        session
            .prompt("continue".into())
            .unwrap()
            .await
            .unwrap()
            .unwrap()["stopReason"],
        "end_turn"
    );
    assert_eq!(observed.lock().unwrap().len(), 1);
    agent.await.unwrap();
}

#[tokio::test]
async fn missing_load_capability_fails_without_creating_or_loading_a_session() {
    let (client, peer) = tokio::io::duplex(4096);
    let (input, output) = tokio::io::split(client);
    let transport = AcpTransport::from_io(output, input);
    let agent = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(peer);
        let mut reader = BufReader::new(reader);
        let init = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{}}})).await;
        let mut line = String::new();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut line))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let loaded = ExistingConversation::load(
        transport,
        "existing".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(|_| Ok(())),
        Arc::new(|_| Box::pin(async { None })),
    )
    .await;
    assert!(loaded.is_err());
    agent.await.unwrap();
}

#[tokio::test]
async fn cancellation_resolves_actual_permission_but_waits_for_agent_terminal_response() {
    let (client, peer) = tokio::io::duplex(4096);
    let (input, output) = tokio::io::split(client);
    let transport = AcpTransport::from_io(output, input);
    let (approval_seen, wait_approval) = tokio::sync::oneshot::channel();
    let seen = Arc::new(Mutex::new(Some(approval_seen)));
    let (cancel_seen, wait_cancel) = tokio::sync::oneshot::channel();
    let (finish, wait_finish) = tokio::sync::oneshot::channel();
    let agent = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(peer);
        let mut reader = BufReader::new(reader);
        let init = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}})).await;
        let load = read(&mut reader).await;
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":load["id"],"result":{}}),
        )
        .await;
        let prompt = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":99,"method":"session/request_permission","params":{"sessionId":"existing","toolCall":{"toolCallId":"tool"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]}})).await;
        let mut cancel = false;
        let mut resolved = false;
        while !cancel || !resolved {
            let value = read(&mut reader).await;
            if value["method"] == "session/cancel" {
                cancel = true;
            } else {
                assert_eq!(value["id"], 99);
                assert_eq!(value["result"]["outcome"]["outcome"], "cancelled");
                resolved = true;
            }
        }
        cancel_seen.send(()).unwrap();
        wait_finish.await.unwrap();
        send(
            &mut writer,
            json!({"jsonrpc":"2.0","id":prompt["id"],"result":{"stopReason":"cancelled"}}),
        )
        .await;
    });
    let session = ExistingConversation::load(
        transport,
        "existing".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(|_| Ok(())),
        Arc::new(move |_| {
            seen.lock().unwrap().take().unwrap().send(()).unwrap();
            Box::pin(std::future::pending())
        }),
    )
    .await
    .unwrap();
    let result = session.prompt("requires permission".into()).unwrap();
    wait_approval.await.unwrap();
    session.cancel().await.unwrap();
    wait_cancel.await.unwrap();
    assert!(session.is_busy());
    assert!(session
        .prompt("must not overlap cancelled but unfinished work".into())
        .is_err());
    finish.send(()).unwrap();
    assert_eq!(result.await.unwrap().unwrap()["stopReason"], "cancelled");
    agent.await.unwrap();
}

#[tokio::test]
async fn replay_sink_failure_never_reports_loaded_history() {
    let (client, peer) = tokio::io::duplex(4096);
    let (input, output) = tokio::io::split(client);
    let transport = AcpTransport::from_io(output, input);
    let agent = tokio::spawn(async move {
        let (reader, mut writer) = tokio::io::split(peer);
        let mut reader = BufReader::new(reader);
        let init = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","id":init["id"],"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}})).await;
        let load = read(&mut reader).await;
        send(&mut writer,json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"existing","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"must persist"}}}})).await;
        let _ = writer
            .write_all(
                format!("{}\n", json!({"jsonrpc":"2.0","id":load["id"],"result":{}})).as_bytes(),
            )
            .await;
    });
    let session = ExistingConversation::load(
        transport,
        "existing".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(|_| {
            Err(ridge_acp::AcpErrorWrap::Io(
                "fixture storage failure".into(),
            ))
        }),
        Arc::new(|_| Box::pin(async { None })),
    )
    .await;
    assert!(session.is_err());
    agent.await.unwrap();
}

#[tokio::test]
async fn cancelled_attachment_releases_stream_and_handlers_without_session_creation() {
    let (client, peer) = tokio::io::duplex(4096);
    let (input, output) = tokio::io::split(client);
    let transport = AcpTransport::from_io(output, input);
    let load = tokio::spawn(ExistingConversation::load(
        transport,
        "existing".into(),
        std::env::temp_dir().to_string_lossy().into(),
        Arc::new(|_| Ok(())),
        Arc::new(|_| Box::pin(async { None })),
    ));
    let (reader, _writer) = tokio::io::split(peer);
    let mut reader = BufReader::new(reader);
    assert_eq!(read(&mut reader).await["method"], "initialize");
    load.abort();
    let _ = load.await;
    let mut trailing = String::new();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), reader.read_line(&mut trailing))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
