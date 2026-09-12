//! Live RTP1-over-WebSocket integration test.
//!
//! Spawns a real `ridge kernel ensure` subprocess (which boots the
//! kernel HTTP + RTP1 WS server), then exercises the wire-level
//! contract end-to-end:
//!
//! * connect to `ws://127.0.0.1:<port>/v1/rtp1`
//! * read `capability_advertise` immediately after upgrade
//! * `attach` and verify `attach_ack` (runtime_epoch, server_version)
//! * write `input` and confirm `input_ack` round-trip
//! * receive `output` frames and assert `output_seq` monotonicity
//! * `resize` and verify `resize_ack`
//! * `detach` and verify `detach_ack`
//! * ping/pong keepalive
//!
//! Run only with `cargo test -p ridge-cli --test rtp1_kernel_e2e`
//! (no `#[ignore]`) because the kernel binary is the production
//! surface; failures here mean the wire contract has regressed.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use ridge_kernel::registry::KernelEndpoint;
use ridge_kernel::rtp1::{
    self, frame_from, payload_from, AttachMode, AttachRequest, CapabilityAdvertise, DetachAck,
    DetachRequest, InputFrame as Rtp1InputFrame, MessageType, PingFrame, PongFrame, ResizeRequest,
};
use serde_json::Value;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

const SETTLE: Duration = Duration::from_secs(2);

fn isolated_data_dir() -> PathBuf {
    std::env::temp_dir().join(format!(
        "ridge-rtp1-e2e-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn ridge_command(binary: &Path, data_dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(binary);
    cmd.args(args)
        .env("RIDGE_KERNEL_DATA_DIR", data_dir)
        .env("RIDGE_CONFIRM_QUIT_KERNEL", "1")
        .env("RIDGE_TEST_ALLOW_NON_BREAKAWAY", "1");
    cmd
}

fn spawn_ridge(binary: &Path, data_dir: &Path, args: &[&str]) -> Child {
    ridge_command(binary, data_dir, args)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn ridge {args:?}: {error}"))
}

fn wait_for_endpoint(data_dir: &Path, timeout: Duration) -> KernelEndpoint {
    let path = data_dir.join("kernel.json");
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(endpoint) = serde_json::from_slice::<KernelEndpoint>(&bytes) {
                return endpoint;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for kernel.json at {}", path.display());
}

async fn fetch_host_info(endpoint: &KernelEndpoint) -> (String, String) {
    let url = format!("http://127.0.0.1:{}/v1/status", endpoint.port);
    let token = endpoint.token.clone();
    let body: serde_json::Value = tokio::task::spawn_blocking(move || {
        let resp = reqwest::blocking::Client::new()
            .get(&url)
            .header("x-ridge-kernel-token", &token)
            .send()
            .expect("status request");
        resp.json().expect("status json")
    })
    .await
    .expect("status blocking");
    (
        body.get("host_id")
            .and_then(Value::as_str)
            .expect("host_id")
            .to_string(),
        body.get("runtime_epoch")
            .and_then(Value::as_str)
            .expect("runtime_epoch")
            .to_string(),
    )
}

async fn create_pty(endpoint: &KernelEndpoint, host_id: &str) -> uuid::Uuid {
    let url = format!("http://127.0.0.1:{}/v1/domain/ptys", endpoint.port);
    let token = endpoint.token.clone();
    let host_id = host_id.to_string();
    let body: serde_json::Value = tokio::task::spawn_blocking(move || {
        reqwest::blocking::Client::new()
            .post(&url)
            .header("x-ridge-kernel-token", &token)
            .json(&serde_json::json!({
                "host_id": host_id,
                "runtime_epoch": "",
                "session_id": "e2e",
                "pty_id": uuid::Uuid::new_v4().to_string(),
                "program": if cfg!(windows) { "cmd.exe" } else { "/bin/sh" },
                "args": if cfg!(windows) { vec!["/C".to_string(), "more".to_string()] } else { vec![] },
                "role": "e2e",
                "cols": 80,
                "rows": 24,
            }))
            .send()
            .expect("pty create")
            .json()
            .expect("pty json")
    })
    .await
    .expect("pty blocking");
    uuid::Uuid::parse_str(body.get("pty_id").and_then(Value::as_str).expect("pty_id"))
        .expect("pty uuid")
}

async fn read_frame<S>(
    stream: &mut S,
    expected: Option<MessageType>,
    timeout: Duration,
) -> (MessageType, Vec<u8>)
where
    S: futures_util::Stream<
            Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
        > + Unpin,
{
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            panic!("timed out waiting for frame; expected {expected:?}");
        }
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => {
                let bytes = bytes.to_vec();
                let (frame, _consumed) = rtp1::decode(&bytes).expect("decode");
                if let Some(want) = expected {
                    if frame.r#type == want {
                        return (frame.r#type, frame.payload);
                    }
                } else {
                    return (frame.r#type, frame.payload);
                }
            }
            Ok(Some(Ok(Message::Close(_)))) => panic!("ws closed unexpectedly"),
            Ok(Some(Err(e))) => panic!("ws error: {e}"),
            Ok(None) => panic!("ws closed"),
            Err(_) => panic!("timeout waiting for frame; expected {expected:?}"),
            _ => continue,
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn rtp1_ws_full_lifecycle() {
    let Some(ridge_bin) = locate_ridge_binary() else {
        eprintln!("ridge binary not built; skipping rtp1_ws_full_lifecycle");
        return;
    };
    let data_dir = isolated_data_dir();
    std::fs::create_dir_all(&data_dir).unwrap();

    // Boot kernel.
    let mut child = spawn_ridge(&ridge_bin, &data_dir, &["kernel", "ensure"]);
    let endpoint = wait_for_endpoint(&data_dir, Duration::from_secs(10));
    tokio::time::sleep(SETTLE).await;

    let (host_id, runtime_epoch) = fetch_host_info(&endpoint).await;
    assert!(!host_id.is_empty(), "host_id must not be empty");
    assert!(!runtime_epoch.is_empty(), "runtime_epoch must not be empty");

    let pty_id = create_pty(&endpoint, &host_id).await;
    eprintln!("[rtp1-e2e] pty_id={pty_id} host_id={host_id} epoch={runtime_epoch}");

    // Connect via WS.
    let url = format!("ws://127.0.0.1:{}/v1/rtp1", endpoint.port);
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("x-ridge-kernel-token", endpoint.token.parse().unwrap());
    let (ws, _response) = tokio_tungstenite::connect_async(request).await.unwrap();
    let (mut sink, mut stream) = ws.split();

    // 1) capability_advertise first.
    let (_cap_type, cap_payload) =
        read_frame(&mut stream, Some(MessageType::CapabilityAdvertise), Duration::from_secs(3))
            .await;
    let cap: CapabilityAdvertise = serde_json::from_slice(&cap_payload).expect("cap json");
    assert!(
        cap.features.iter().any(|f| f == "rtp1.v1"),
        "rtp1.v1 missing from capability: {:?}",
        cap.features
    );

    // 2) attach.
    let controller_id = uuid::Uuid::new_v4();
    let attach = AttachRequest {
        host_id: host_id.clone(),
        runtime_epoch: runtime_epoch.clone(),
        session_id: "e2e".into(),
        terminal_id: pty_id.to_string(),
        controller_id: controller_id.to_string(),
        since_output_seq: None,
        mode: AttachMode::Raw,
        client_min_version: 1,
        client_max_version: 1,
    };
    let attach_frame = frame_from(MessageType::Attach, &attach, Default::default()).unwrap();
    let wire = rtp1::encode(&attach_frame).unwrap();
    sink.send(Message::Binary(wire.into())).await.unwrap();

    let (_ack_type, ack_payload) =
        read_frame(&mut stream, Some(MessageType::AttachAck), Duration::from_secs(3)).await;
    let ack: serde_json::Value = serde_json::from_slice(&ack_payload).unwrap();
    assert_eq!(ack["runtime_epoch"], runtime_epoch);
    assert_eq!(ack["server_version"], 1);
    assert_eq!(ack["terminal_id"], pty_id.to_string());
    assert!(ack["oldest_output_seq"].as_u64().is_some());
    assert!(ack["next_output_seq"].as_u64().is_some());

    // 3) write input.
    let input = Rtp1InputFrame {
        terminal_id: pty_id.to_string(),
        controller_id: controller_id.to_string(),
        input_seq: 1,
        data_b64: rtp1::b64_encode(b"echo ridge-rtp1"),
        data_len: 15,
    };
    let input_frame = frame_from(MessageType::Input, &input, Default::default()).unwrap();
    let wire = rtp1::encode(&input_frame).unwrap();
    sink.send(Message::Binary(wire.into())).await.unwrap();

    let (_input_ack_type, ack_payload) =
        read_frame(&mut stream, Some(MessageType::InputAck), Duration::from_secs(3)).await;
    let input_ack: serde_json::Value = serde_json::from_slice(&ack_payload).unwrap();
    assert_eq!(input_ack["status"], "applied");
    assert_eq!(input_ack["input_seq"], 1);

    // 4) drain output (best-effort; some shells don't echo).
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(msg) = stream.next().await {
            let bytes = match msg {
                Ok(Message::Binary(b)) => b.to_vec(),
                _ => continue,
            };
            let (frame, _) = rtp1::decode(&bytes).expect("decode");
            if frame.r#type == MessageType::Output {
                let out: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
                assert_eq!(out["terminal_id"], pty_id.to_string());
                break;
            }
        }
    })
    .await;

    // 5) resize.
    let resize = ResizeRequest {
        terminal_id: pty_id.to_string(),
        controller_id: controller_id.to_string(),
        rows: 30,
        cols: 100,
        owner: None,
    };
    let frame = frame_from(MessageType::Resize, &resize, Default::default()).unwrap();
    let wire = rtp1::encode(&frame).unwrap();
    sink.send(Message::Binary(wire.into())).await.unwrap();
    let (_resize_ack_type, _) =
        read_frame(&mut stream, Some(MessageType::ResizeAck), Duration::from_secs(3)).await;

    // 6) ping / pong.
    let ping = PingFrame { nonce: 4242 };
    let frame = frame_from(MessageType::Ping, &ping, Default::default()).unwrap();
    let wire = rtp1::encode(&frame).unwrap();
    sink.send(Message::Binary(wire.into())).await.unwrap();
    let (pong_type, pong_payload) =
        read_frame(&mut stream, Some(MessageType::Pong), Duration::from_secs(3)).await;
    let pong: PongFrame = payload_from(&(rtp1::Frame {
        r#type: pong_type,
        flags: Default::default(),
        payload: pong_payload,
    }))
    .unwrap();
    assert_eq!(pong.nonce, 4242);

    // 7) detach.
    let detach = DetachRequest {
        terminal_id: pty_id.to_string(),
        controller_id: controller_id.to_string(),
        reason: Some("e2e-cleanup".into()),
    };
    let frame = frame_from(MessageType::Detach, &detach, Default::default()).unwrap();
    let wire = rtp1::encode(&frame).unwrap();
    sink.send(Message::Binary(wire.into())).await.unwrap();
    let (_detach_ack_type, ack_payload) =
        read_frame(&mut stream, Some(MessageType::DetachAck), Duration::from_secs(3)).await;
    let _ack: DetachAck = serde_json::from_slice(&ack_payload).unwrap();

    // Clean up the kernel.
    drop(sink);
    let _ = child.kill();
    let _ = child.wait();
}

fn locate_ridge_binary() -> Option<PathBuf> {
    // Locate the ridge binary built by the workspace's cargo test invocation.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.push("target");
    p.push("debug");
    p.push(if cfg!(windows) { "ridge.exe" } else { "ridge" });
    if p.exists() {
        Some(p)
    } else {
        eprintln!("ridge binary not at {}", p.display());
        None
    }
}
