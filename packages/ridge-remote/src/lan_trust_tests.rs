//! LAN §7.4 trust-grant HTTP handler 测试。
//!
//! 用 [`MockHost`] 录 [`HostAuth`] 调用；`axum::Router::oneshot` + 自定 Request
//! 注入 `ConnectInfo<SocketAddr>` 模拟真实对端 IP；POST JSON 经 `tower::ServiceExt::oneshot`。
//!
//! 7 个测试镜像 TS 端 `lanTrustGrant.test.ts`（`lan_trust_grant_returns_anon_pub` 等）。
//!
//! 文件大小控制：~260 行。

use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header::AUTHORIZATION, HeaderValue, Request, StatusCode};
use axum::Router;
use base64::Engine as _;
use parking_lot::Mutex;
use ridge_core::device_identity::{verify, DeviceIdentity, PUBLIC_KEY_LEN, SIGNATURE_LEN};
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::lan_trust::{LanTrustState, NONCE_LEN, PUB_LEN, SIG_LEN, TRUST_SIGN_PREFIX};
use crate::server_app::router;
use crate::serve::UaServeConfig;

use crate::host::{HostMeta, RemoteHost, WsConn, WorkspaceProvider};

// ── MockHost：记录调用、模拟签发/校验/grant ────────────────────────────────

#[derive(Clone)]
struct MockHost {
    session_token: String,
    totp_code: String,
    blacklist: Arc<AtomicBool>,
    banned: Arc<AtomicBool>,
    pub pre_calls: Arc<AtomicUsize>,
    pub post_calls: Arc<AtomicUsize>,
    pub trust_check_calls: Arc<AtomicUsize>,
    pub trust_record_calls: Arc<AtomicUsize>,
    pub last_recorded_pub: Arc<Mutex<Option<Vec<u8>>>>,
    granted_pubs: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl MockHost {
    fn new() -> Self {
        Self {
            session_token: "test-token".to_string(),
            totp_code: "123456".to_string(),
            blacklist: Arc::new(AtomicBool::new(false)),
            banned: Arc::new(AtomicBool::new(false)),
            pre_calls: Arc::new(AtomicUsize::new(0)),
            post_calls: Arc::new(AtomicUsize::new(0)),
            trust_check_calls: Arc::new(AtomicUsize::new(0)),
            trust_record_calls: Arc::new(AtomicUsize::new(0)),
            last_recorded_pub: Arc::new(Mutex::new(None)),
            granted_pubs: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl HostMeta for MockHost {
    fn port(&self) -> u16 { 0 }
    fn lan_ip(&self) -> String { "127.0.0.1".into() }
    fn machine_name(&self) -> String { "mock".into() }
    fn remote_enabled(&self) -> Arc<AtomicBool> { Arc::new(AtomicBool::new(true)) }
    fn tls_enabled(&self) -> bool { false }
    fn serve_cfg(&self) -> UaServeConfig {
        UaServeConfig { remote_dir: PathBuf::from("static/remote") }
    }
}

impl crate::host::HostAuth for MockHost {
    fn verify_code(&self, code: &str) -> bool { code == self.totp_code }
    fn is_blacklisted(&self, _device: &str, _ip: &str) -> bool { self.blacklist.load(Ordering::Relaxed) }
    fn pre_verify_gate(&self, _ip: &str, _device: &str) -> Result<(), ()> {
        self.pre_calls.fetch_add(1, Ordering::Relaxed);
        if self.banned.load(Ordering::Relaxed) { Err(()) } else { Ok(()) }
    }
    fn post_verify_record(&self, _ip: &str, _device: &str, _valid: bool) {
        self.post_calls.fetch_add(1, Ordering::Relaxed);
    }
    fn create_session_token(&self, _device: &str, _ip: &str) -> String { self.session_token.clone() }
    fn validate_token(&self, token: &str) -> bool { token == self.session_token }
    fn validate_token_bound(&self, token: &str, _device: &str, _ip: &str) -> bool {
        token == self.session_token
    }
    fn validate_token_device_strict(&self, token: &str, _device: &str, _ip: &str) -> bool {
        token == self.session_token
    }
    fn totp_trust_check(&self, ctrl_pub: &[u8]) -> bool {
        self.trust_check_calls.fetch_add(1, Ordering::Relaxed);
        self.granted_pubs.lock().iter().any(|p| p.as_slice() == ctrl_pub)
    }
    fn totp_trust_record(&self, ctrl_pub: &[u8]) {
        self.trust_record_calls.fetch_add(1, Ordering::Relaxed);
        *self.last_recorded_pub.lock() = Some(ctrl_pub.to_vec());
        self.granted_pubs.lock().push(ctrl_pub.to_vec());
    }
}

impl WorkspaceProvider for MockHost {
    fn list_workspaces_json(&self) -> Value { json!({"workspaces": []}) }
    fn switch_workspace(&self, _id: &str) -> Result<Value, crate::host::HostError> {
        Ok(json!({"success": true}))
    }
    fn create_workspace(&self, _name: Option<String>) -> Result<Value, crate::host::HostError> {
        Ok(json!({"success": true}))
    }
    fn close_workspace(&self, _id: &str) -> Result<Value, crate::host::HostError> {
        Ok(json!({"success": true}))
    }
    fn allowed_file_roots(&self) -> Vec<PathBuf> { Vec::new() }
}

impl RemoteHost for MockHost {
    fn serve_websocket(
        self: Arc<Self>,
        _socket: axum::extract::ws::WebSocket,
        _conn: WsConn,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async {})
    }
}

// ── 测试夹具 ──────────────────────────────────────────────────────────────

const TEST_DEVICE: &str = "test-device-001";
const TEST_IP_STR: &str = "10.0.0.42:12345";

fn test_ip() -> SocketAddr {
    TEST_IP_STR.parse().expect("valid IP")
}

/// 走 server_app::router 的完整装配（含 security_headers / remote_gate / serve fallback 等）。
/// 单测聚焦 trust-grant handler，所以 ctx 只装 host + state。
fn simple_router(host: Arc<MockHost>, state: Arc<LanTrustState>) -> Router {
    let host_dyn: Arc<dyn RemoteHost> = host.clone();
    router(host_dyn, state)
}

fn auth_header(token: &str) -> (axum::http::HeaderName, HeaderValue) {
    (AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", token)).unwrap())
}

fn device_header() -> (&'static str, &'static str) {
    ("x-ridge-device", TEST_DEVICE)
}

fn pub_b64(pub_bytes: &[u8; PUBLIC_KEY_LEN]) -> String {
    base64::engine::general_purpose::STANDARD.encode(pub_bytes)
}

fn nonce_b64(nonce: &[u8; NONCE_LEN]) -> String {
    base64::engine::general_purpose::STANDARD.encode(nonce)
}

fn sig_b64(sig: &[u8; SIGNATURE_LEN]) -> String {
    base64::engine::general_purpose::STANDARD.encode(sig)
}

/// POST 模拟请求（含 ConnectInfo）；`Server::oneshot` 要求 Service 形如 Router。
async fn post_json(
    app: Router,
    path: &str,
    body: Value,
    with_auth: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .extension(ConnectInfo(test_ip()));
    if with_auth {
        let (k, v) = auth_header("test-token");
        builder = builder.header(k, v);
        let (dk, dv) = device_header();
        builder = builder.header(dk, dv);
    }
    let req = builder
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
    let v: Value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap_or(Value::Null) };
    (status, v)
}

// ── 7 个测试 ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn lan_trust_grant_returns_anon_pub() {
    let host = Arc::new(MockHost::new());
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host.clone(), state.clone());
    let id = DeviceIdentity::from_seed(&[1u8; 32]);
    let pub_bytes = id.public_bytes();

    let (status, body) = post_json(
        app,
        "/trust-grant",
        json!({ "pubKey": pub_b64(&pub_bytes) }),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let nonce_str = body.get("nonce").and_then(Value::as_str).expect("nonce");
    let nonce = base64::engine::general_purpose::STANDARD.decode(nonce_str).unwrap();
    assert_eq!(nonce.len(), NONCE_LEN);
    // hello 写入缓存
    assert_eq!(state.len(), 1);
}

#[tokio::test]
async fn lan_trust_proof_with_valid_sig_records_grant() {
    let host = Arc::new(MockHost::new());
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host.clone(), state.clone());
    let id = DeviceIdentity::from_seed(&[2u8; 32]);
    let pub_bytes = id.public_bytes();

    // hello
    let (_, hello) = post_json(
        app.clone(),
        "/trust-grant",
        json!({ "pubKey": pub_b64(&pub_bytes) }),
        true,
    )
    .await;
    let nonce: [u8; NONCE_LEN] = base64::engine::general_purpose::STANDARD
        .decode(hello.get("nonce").and_then(Value::as_str).unwrap())
        .unwrap()
        .try_into()
        .unwrap();

    // 签名 prefix || nonce
    let mut msg = Vec::with_capacity(TRUST_SIGN_PREFIX.len() + NONCE_LEN);
    msg.extend_from_slice(TRUST_SIGN_PREFIX);
    msg.extend_from_slice(&nonce);
    let sig = id.sign(&msg);

    let (status, body) = post_json(
        app,
        "/trust-grant/proof",
        json!({ "sig": sig_b64(&sig) }),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("trusted").and_then(Value::as_bool), Some(true));
    // grant 落 + check 调用
    assert_eq!(host.trust_record_calls.load(Ordering::Relaxed), 1);
    assert!(host.last_recorded_pub.lock().is_some());
    // hello 一次性消费
    assert_eq!(state.len(), 0);
}

#[tokio::test]
async fn lan_trust_proof_with_bad_sig_returns_false_and_no_record() {
    let host = Arc::new(MockHost::new());
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host.clone(), state.clone());
    let id = DeviceIdentity::from_seed(&[3u8; 32]);
    let pub_bytes = id.public_bytes();

    let (_, hello) = post_json(
        app.clone(),
        "/trust-grant",
        json!({ "pubKey": pub_b64(&pub_bytes) }),
        true,
    )
    .await;
    let nonce: [u8; NONCE_LEN] = base64::engine::general_purpose::STANDARD
        .decode(hello.get("nonce").and_then(Value::as_str).unwrap())
        .unwrap()
        .try_into()
        .unwrap();

    // 用别的密钥签 —— 必拒
    let other = DeviceIdentity::from_seed(&[99u8; 32]);
    let mut msg = Vec::with_capacity(TRUST_SIGN_PREFIX.len() + NONCE_LEN);
    msg.extend_from_slice(TRUST_SIGN_PREFIX);
    msg.extend_from_slice(&nonce);
    let bad_sig = other.sign(&msg);

    let (status, body) = post_json(
        app,
        "/trust-grant/proof",
        json!({ "sig": sig_b64(&bad_sig) }),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.get("trusted").and_then(Value::as_bool), Some(false));
    assert_eq!(host.trust_record_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn lan_trust_proof_without_hello_returns_400() {
    let host = Arc::new(MockHost::new());
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host, state);
    let sig = [0u8; SIGNATURE_LEN];
    let (status, _) = post_json(
        app,
        "/trust-grant/proof",
        json!({ "sig": sig_b64(&sig) }),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn lan_trust_hello_requires_session_token() {
    let host = Arc::new(MockHost::new());
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host, state);
    let id = DeviceIdentity::from_seed(&[4u8; 32]);
    let pub_bytes = id.public_bytes();

    // 不带 Authorization → 401
    let (status, _) = post_json(
        app,
        "/trust-grant",
        json!({ "pubKey": pub_b64(&pub_bytes) }),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn lan_trust_throttle_banned_returns_429() {
    let host = Arc::new(MockHost::new());
    host.banned.store(true, Ordering::Relaxed);
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host, state);
    let id = DeviceIdentity::from_seed(&[5u8; 32]);
    let pub_bytes = id.public_bytes();

    let (status, _) = post_json(
        app,
        "/trust-grant",
        json!({ "pubKey": pub_b64(&pub_bytes) }),
        true,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn lan_trust_proof_replay_blocked_after_consume() {
    let host = Arc::new(MockHost::new());
    let state = Arc::new(LanTrustState::new());
    let app = simple_router(host.clone(), state.clone());
    let id = DeviceIdentity::from_seed(&[6u8; 32]);
    let pub_bytes = id.public_bytes();

    let (_, hello) = post_json(
        app.clone(),
        "/trust-grant",
        json!({ "pubKey": pub_b64(&pub_bytes) }),
        true,
    )
    .await;
    let nonce: [u8; NONCE_LEN] = base64::engine::general_purpose::STANDARD
        .decode(hello.get("nonce").and_then(Value::as_str).unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let mut msg = Vec::with_capacity(TRUST_SIGN_PREFIX.len() + NONCE_LEN);
    msg.extend_from_slice(TRUST_SIGN_PREFIX);
    msg.extend_from_slice(&nonce);
    let sig = id.sign(&msg);

    // 首次 proof → trusted:true
    let (_, body1) = post_json(
        app.clone(),
        "/trust-grant/proof",
        json!({ "sig": sig_b64(&sig) }),
        true,
    )
    .await;
    assert_eq!(body1.get("trusted").and_then(Value::as_bool), Some(true));

    // 重放同一 sig → 缓存已 consume，必 400
    let (status2, _) = post_json(
        app,
        "/trust-grant/proof",
        json!({ "sig": sig_b64(&sig) }),
        true,
    )
    .await;
    assert_eq!(status2, StatusCode::BAD_REQUEST);
    // 仍只 record 1 次（重放不算）
    assert_eq!(host.trust_record_calls.load(Ordering::Relaxed), 1);
    // 抑制 nonce 变量未用告警（nonce 仅首次使用）
    let _ = nonce_b64(&nonce);
}

// ── 单测辅助：直接 device_identity::verify 跨核校验 ─────────────────────────

#[test]
fn device_identity_verify_cross_check() {
    let id = DeviceIdentity::from_seed(&[7u8; 32]);
    let pub_bytes = id.public_bytes();
    let sig = id.sign(b"hello");
    assert!(verify(&pub_bytes, b"hello", &sig));
    assert!(!verify(&pub_bytes, b"world", &sig));
    // PUB_LEN / SIG_LEN 常量镜像用
    assert_eq!(pub_bytes.len(), PUB_LEN);
    assert_eq!(sig.len(), SIG_LEN);
}
