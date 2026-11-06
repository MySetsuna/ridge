//! `session_control` 单元测试（独立文件，控制 `session_control.rs` ≤600 行）。
//!
//! 14 个测试：
//! - 1 个 TOTP-bind 迁自 session.rs（绑定 tag 验签 + 拒 / 漏 transcript）
//! - 13 个 §7.4 trust-grant 云腿：hello / proof / lockout / transcript bind /
//!   deferred grant 等

use super::*;
use crate::e2ee::{build_bind_transcript, Handshake, Session as CryptoSession};
use crate::mux::{demux, Inbound};
use crate::protocol::SessionControl;
use crate::totp::RemoteTotp;
use ed25519_dalek::{Signer, SigningKey};
use std::path::PathBuf;
use tokio::sync::mpsc;

fn crypto_pair() -> (CryptoSession, CryptoSession) {
    let host_hs = Handshake::new();
    let ctrl_hs = Handshake::new();
    let host_pub = host_hs.public_bytes();
    let ctrl_pub = ctrl_hs.public_bytes();
    let host = host_hs.into_session(ctrl_pub, crate::e2ee::Dir::HostToController).unwrap();
    let ctrl = ctrl_hs.into_session(host_pub, crate::e2ee::Dir::ControllerToHost).unwrap();
    (host, ctrl)
}

fn drain(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Ok(f) = rx.try_recv() {
        out.push(f);
    }
    out
}

fn random_ctrl_signing() -> SigningKey {
    let mut bytes = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    SigningKey::from_bytes(&bytes)
}

/// 单进程一次性把 grants 目录覆盖到临时路径，让 `grant_store::{check,record,revoke_all}`
/// 走隔离位置（避免污染 `ProjectDirs("ridge")` 真实配置、避免测试间互相干扰）。
/// 用 `set_grants_dir_for_tests` 而非 env var 是因为 `std::env::set_var` 在 tokio
/// 多线程 runtime 下不可靠（race），OnceLock<PathBuf> 跨线程可见。
fn ensure_test_grants_dir() -> &'static std::path::Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!(
            "ridge-cli-grants-{}-{}",
            std::process::id(),
            rand::random::<u32>()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp grants dir");
        let prev = ridge_core::grant_store::set_grants_dir_for_tests(dir.clone());
        assert!(
            prev.is_none(),
            "ensure_test_grants_dir must be the first to install override"
        );
        dir
    })
    .as_path()
}

/// 每测唯一身份——`RemoteTotp::identity()` 默认空串，多测并行下共享 grants 文件会
/// 互相覆盖。每测用 `pid + 原子计数` 构造唯一字符串，grant_store 按 identity 隔离。
fn unique_test_identity(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("cli-test-{}-{}-{}", std::process::id(), n, tag)
}

// ── 既有 totp-bind 行为（从 session.rs 迁出）────────────────────────────────

#[tokio::test]
async fn totp_bind_unlocks_with_valid_tag_and_rejects_bad_tag() {
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(16);
    let totp = RemoteTotp::new();
    let transcript = build_bind_transcript(&[0x11u8; 32], &[0x22u8; 32]);
    let identity = totp.identity();

    // 1) 有效 tag → verified + totp-result{ok:true}。
    let good_tag = totp.current_bind_tag(&transcript);
    let good_b64 = b64_encode(&good_tag);
    let body = serde_json::to_vec(&SessionControl::TotpBind { tag: good_b64 }).unwrap();
    let mut verified = false;
    let mut ts = TrustState::default();
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut verified,
        &mut ts,
        Some(&transcript),
    )
    .await
    .unwrap();
    assert!(verified, "valid totp-bind tag must unlock the control channel");
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
    match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
        Inbound::Control(b) => {
            let sc: SessionControl = serde_json::from_slice(&b).unwrap();
            assert_eq!(sc, SessionControl::TotpResult { ok: true });
        }
        other => panic!("expected Control totp-result, got {other:?}"),
    }

    // 2) 坏 tag → 仍未 verified + totp-result{ok:false}。
    let bad_b64 = b64_encode(&[0u8; 32]);
    let body = serde_json::to_vec(&SessionControl::TotpBind { tag: bad_b64 }).unwrap();
    let mut verified2 = false;
    let mut ts2 = TrustState::default();
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut verified2,
        &mut ts2,
        Some(&transcript),
    )
    .await
    .unwrap();
    assert!(!verified2, "invalid totp-bind tag must NOT unlock");

    // 3) transcript 缺失 → 即便 tag 形式合法也判失败。
    let good_b64 = b64_encode(&totp.current_bind_tag(&transcript));
    let body = serde_json::to_vec(&SessionControl::TotpBind { tag: good_b64 }).unwrap();
    let mut verified3 = false;
    let mut ts3 = TrustState::default();
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified3, &mut ts3, None)
        .await
        .unwrap();
    assert!(!verified3, "totp-bind without a bind transcript must fail");
}

// ── §7.4 trust-grant 13 个测试（云腿）─────────────────────────────────────

fn trust_hello_body(pub_b64: &str) -> Vec<u8> {
    serde_json::to_vec(&SessionControl::TotpTrustHello { r#pub: pub_b64.to_string() }).unwrap()
}
#[allow(dead_code)]
fn trust_challenge_body(nonce_b64: &str) -> Vec<u8> {
    serde_json::to_vec(&SessionControl::TotpTrustChallenge { nonce: nonce_b64.to_string() })
        .unwrap()
}
fn trust_proof_body(sig_b64: &str) -> Vec<u8> {
    serde_json::to_vec(&SessionControl::TotpTrustProof { sig: sig_b64.to_string() }).unwrap()
}

#[tokio::test]
async fn trust_hello_with_valid_pub_sends_challenge() {
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    let mut ts = TrustState::default();

    let body = trust_hello_body(&b64_encode(&ctrl_pub));
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut false,
        &mut ts,
        None,
    )
    .await
    .unwrap();
    assert_eq!(ts.ctrl_pub, Some(ctrl_pub));
    assert!(ts.nonce.is_some());
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
    match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
        Inbound::Control(b) => {
            let sc: SessionControl = serde_json::from_slice(&b).unwrap();
            match sc {
                SessionControl::TotpTrustChallenge { nonce } => {
                    let raw = b64_decode(&nonce).expect("nonce b64 must decode");
                    assert_eq!(raw.len(), 32);
                }
                other => panic!("expected Challenge, got {other:?}"),
            }
        }
        other => panic!("expected Control, got {other:?}"),
    }
}

#[tokio::test]
async fn trust_hello_with_short_pub_silently_dropped() {
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let mut ts = TrustState::default();

    let body = trust_hello_body(&b64_encode(&[1u8, 2u8]));
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut false,
        &mut ts,
        None,
    )
    .await
    .unwrap();
    assert!(ts.ctrl_pub.is_none());
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test]
async fn trust_hello_with_bad_base64_silently_dropped() {
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let mut ts = TrustState::default();

    let body = trust_hello_body("@@@@not-base64");
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut false,
        &mut ts,
        None,
    )
    .await
    .unwrap();
    assert!(ts.ctrl_pub.is_none());
    assert!(drain(&mut rx).is_empty());
}

#[tokio::test]
async fn trust_hello_overwrites_pub_and_issues_fresh_nonce() {
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, mut _rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let s1 = random_ctrl_signing();
    let s2 = random_ctrl_signing();
    let p1 = s1.verifying_key().to_bytes();
    let p2 = s2.verifying_key().to_bytes();
    let mut ts = TrustState::default();

    let body = trust_hello_body(&b64_encode(&p1));
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut false, &mut ts, None)
        .await
        .unwrap();
    let n1 = ts.nonce;
    assert_eq!(ts.ctrl_pub, Some(p1));
    let body = trust_hello_body(&b64_encode(&p2));
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut false, &mut ts, None)
        .await
        .unwrap();
    let n2 = ts.nonce;
    assert_eq!(ts.ctrl_pub, Some(p2));
    assert_ne!(n1, n2, "second hello must issue a fresh nonce");
}

#[tokio::test]
async fn trust_proof_with_no_hello_emits_no_result() {
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let mut ts = TrustState::default();

    let body = trust_proof_body(&b64_encode(&[0u8; 64]));
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut false, &mut ts, None)
        .await
        .unwrap();
    assert!(
        drain(&mut rx).is_empty(),
        "proof-without-hello must emit no result"
    );
    assert_eq!(ts.totp_failures, 0, "failures not bumped when nonce missing");
}

#[tokio::test]
async fn trust_proof_bad_signature_trusted_false_and_bumps_counter() {
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    let mut ts = TrustState::default();
    ts.ctrl_pub = Some(ctrl_pub);
    ts.nonce = Some([7u8; 32]);

    let body = trust_proof_body(&b64_encode(&[0u8; 64])); // 全 0 签名
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut false, &mut ts, None)
        .await
        .unwrap();
    assert_eq!(ts.totp_failures, 1);
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
    // host→controller 帧由 ctrl_crypto 解。
    match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
        Inbound::Control(b) => {
            let sc: SessionControl = serde_json::from_slice(&b).unwrap();
            assert_eq!(sc, SessionControl::TotpTrustResult { trusted: false });
        }
        other => panic!("expected Control, got {other:?}"),
    }
}

#[tokio::test]
async fn trust_proof_valid_signature_with_grant_trusted_true_opens_gate() {
    let _ = ensure_test_grants_dir();
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = unique_test_identity("valid-sig-with-grant");
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    grant_store::record(&identity, &ctrl_pub);
    let mut ts = TrustState::default();
    ts.ctrl_pub = Some(ctrl_pub);
    ts.nonce = Some([9u8; 32]);

    let mut msg = Vec::new();
    msg.extend_from_slice(TRUST_DOMAIN);
    msg.extend_from_slice(&[9u8; 32]);
    let sig_bytes = signing.sign(&msg).to_bytes();
    let body = trust_proof_body(&b64_encode(&sig_bytes));
    let mut verified = false;
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified, &mut ts, None)
        .await
        .unwrap();
    assert!(verified, "trusted grant + valid sig must unlock");
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
    match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
        Inbound::Control(b) => {
            let sc: SessionControl = serde_json::from_slice(&b).unwrap();
            assert_eq!(sc, SessionControl::TotpTrustResult { trusted: true });
        }
        other => panic!("expected Control, got {other:?}"),
    }
    assert_eq!(ts.without_transcript, 1);
    grant_store::revoke_all(&identity);
}

#[tokio::test]
async fn trust_proof_lockout_short_circuits_no_failure_bump() {
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let mut ts = TrustState::default();
    ts.ctrl_pub = Some([1u8; 32]);
    ts.nonce = Some([1u8; 32]);
    ts.totp_failures = MAX_TOTP_ATTEMPTS; // 已锁

    let body = trust_proof_body(&b64_encode(&[0u8; 64]));
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut false, &mut ts, None)
        .await
        .unwrap();
    assert_eq!(ts.totp_failures, MAX_TOTP_ATTEMPTS, "锁过不增计数");
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1, "短路径仍发 trusted:false");
}

#[tokio::test]
async fn trust_proof_with_transcript_binds_signature() {
    let _ = ensure_test_grants_dir();
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = unique_test_identity("transcript-bind");
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    grant_store::record(&identity, &ctrl_pub);
    let transcript = build_bind_transcript(&[0x11u8; 32], &[0x22u8; 32]);
    let mut ts = TrustState::default();
    ts.ctrl_pub = Some(ctrl_pub);
    ts.nonce = Some([3u8; 32]);

    let mut msg = Vec::new();
    msg.extend_from_slice(TRUST_DOMAIN);
    msg.extend_from_slice(&[3u8; 32]);
    msg.extend_from_slice(&transcript);
    let sig_bytes = signing.sign(&msg).to_bytes();
    let body = trust_proof_body(&b64_encode(&sig_bytes));
    let mut verified = false;
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut verified,
        &mut ts,
        Some(&transcript),
    )
    .await
    .unwrap();
    assert!(verified);
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
    match demux(&ctrl_crypto.open(&out[0]).unwrap()) {
        Inbound::Control(b) => {
            let sc: SessionControl = serde_json::from_slice(&b).unwrap();
            assert_eq!(sc, SessionControl::TotpTrustResult { trusted: true });
        }
        other => panic!("expected Control, got {other:?}"),
    }
    assert_eq!(ts.with_transcript, 1);
    grant_store::revoke_all(&identity);
}

#[tokio::test]
async fn trust_proof_no_transcript_uses_prefix_nonce_only() {
    let _ = ensure_test_grants_dir();
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = unique_test_identity("no-transcript");
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    grant_store::record(&identity, &ctrl_pub);
    let mut ts = TrustState::default();
    ts.ctrl_pub = Some(ctrl_pub);
    ts.nonce = Some([5u8; 32]);

    let mut msg = Vec::new();
    msg.extend_from_slice(TRUST_DOMAIN);
    msg.extend_from_slice(&[5u8; 32]);
    let sig_bytes = signing.sign(&msg).to_bytes();
    let body = trust_proof_body(&b64_encode(&sig_bytes));
    let mut verified = false;
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified, &mut ts, None)
        .await
        .unwrap();
    assert!(verified);
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
    let _ = ctrl_crypto.open(&out[0]).unwrap();
    grant_store::revoke_all(&identity);
}

#[tokio::test]
async fn transcript_asymmetry_proof_fails_no_grant_lookup() {
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = totp.identity();
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    let mut ts = TrustState::default();
    ts.ctrl_pub = Some(ctrl_pub);
    ts.nonce = Some([11u8; 32]);

    let host_transcript = build_bind_transcript(&[0xAAu8; 32], &[0xBBu8; 32]);
    let ctrl_transcript = build_bind_transcript(&[0xCCu8; 32], &[0xDDu8; 32]);
    let mut msg = Vec::new();
    msg.extend_from_slice(TRUST_DOMAIN);
    msg.extend_from_slice(&[11u8; 32]);
    msg.extend_from_slice(&ctrl_transcript);
    let sig_bytes = signing.sign(&msg).to_bytes();
    let body = trust_proof_body(&b64_encode(&sig_bytes));
    let mut verified = false;
    handle_control(
        &body,
        &mut host_crypto,
        &tx,
        &totp,
        &identity,
        &mut verified,
        &mut ts,
        Some(&host_transcript),
    )
    .await
    .unwrap();
    assert!(!verified);
    assert_eq!(ts.totp_failures, 1);
    let out = drain(&mut rx);
    assert_eq!(out.len(), 1);
}

#[tokio::test]
async fn totp_success_before_trust_hello_records_on_hello() {
    let _ = ensure_test_grants_dir();
    let (mut host_crypto, mut ctrl_crypto) = crypto_pair();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = unique_test_identity("totp-before-hello");
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    let mut ts = TrustState::default();
    let mut verified = false;

    let body = trust_hello_body(&b64_encode(&ctrl_pub));
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified, &mut ts, None)
        .await
        .unwrap();
    assert_eq!(ts.ctrl_pub, Some(ctrl_pub));
    drain(&mut rx);

    let body = serde_json::to_vec(&SessionControl::TotpVerify {
        code: totp.current_code(),
    })
    .unwrap();
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified, &mut ts, None)
        .await
        .unwrap();
    assert!(verified);

    let mut ts2 = TrustState::default();
    ts2.ctrl_pub = Some(ctrl_pub);
    ts2.nonce = Some([13u8; 32]);
    let mut msg = Vec::new();
    msg.extend_from_slice(TRUST_DOMAIN);
    msg.extend_from_slice(&[13u8; 32]);
    let sig_bytes = signing.sign(&msg).to_bytes();
    let body = trust_proof_body(&b64_encode(&sig_bytes));
    let mut verified2 = false;
    let (mut h2, mut c2) = crypto_pair();
    handle_control(
        &body,
        &mut h2,
        &tx,
        &totp,
        &identity,
        &mut verified2,
        &mut ts2,
        None,
    )
    .await
    .unwrap();
    assert!(verified2);
    let out = drain(&mut rx);
    if let Some(f) = out.first() {
        let _ = c2.open(f);
    }
    grant_store::revoke_all(&identity);
}

#[tokio::test]
async fn totp_success_after_trust_hello_records_immediately() {
    let _ = ensure_test_grants_dir();
    let (mut host_crypto, _ctrl_crypto) = crypto_pair();
    let (tx, _rx) = mpsc::channel::<Vec<u8>>(8);
    let totp = RemoteTotp::new();
    let identity = unique_test_identity("totp-after-hello");
    let signing = random_ctrl_signing();
    let ctrl_pub = signing.verifying_key().to_bytes();
    let mut ts = TrustState::default();
    let mut verified = false;

    let body = trust_hello_body(&b64_encode(&ctrl_pub));
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified, &mut ts, None)
        .await
        .unwrap();
    let body = serde_json::to_vec(&SessionControl::TotpVerify {
        code: totp.current_code(),
    })
    .unwrap();
    handle_control(&body, &mut host_crypto, &tx, &totp, &identity, &mut verified, &mut ts, None)
        .await
        .unwrap();
    assert!(verified);
    grant_store::revoke_all(&identity);
}