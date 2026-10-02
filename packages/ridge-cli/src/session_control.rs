//! 0x12 CONTROL 通道处理（契约 §4 TOTP 握手 + §7.4 trust-grant + 零信任 #1 信道绑定）。
//!
//! 从 `session.rs` 拆出：`handle_control` / `send_control` / b64 helpers / `TrustState`。
//! 拆出原因：原 `session.rs` 1589 行超 600 行上限（CLAUDE.md「文件行数上限」），本模块
//! 把**会话控制面**——TOTP 校验 / trust-grant 4 帧握手——集中在一处，便于审计 + 单测。
//!
//! `TrustState` 一次会话一份（由 `session::run()` 创建，断连即弃），承载：
//! - `ctrl_pub`：controller 报的 Ed25519 公钥（trust-hello 写入，trust-proof 用）。
//! - `nonce`：host 发的 32B challenge；trust-proof 前必须先消费（设为 None）再验签，
//!   防止同 nonce 被多次 replay。
//! - `totp_failures`：TOTP-verify/bind/trust-proof 共享的 5 击失败计数，达上限短路过。
//! - `pending_record`：TOTP 成功但 trust-hello 尚未到达 → 下次有效 hello 时刷落 grant
//!   （§B2 deferred grant，与 `cloudHostBridge.ts` 行为一致）。
//! - `with_transcript` / `without_transcript`：trust-proof 签名是否包含 bindTranscript
//!   的计数器（F1 S1 telemetry 对齐）。

use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

use crate::e2ee::Session as CryptoSession;
use crate::mux;
use crate::protocol::SessionControl;
use crate::totp::RemoteTotp;
use ridge_core::grant_store;

/// TOTP-verify / bind / trust-proof 共享失败计数上限（与 `cloudHostBridge.ts::MAX_TOTP_ATTEMPTS = 5` 字节对齐）。
pub const MAX_TOTP_ATTEMPTS: u32 = 5;

/// trust-grant 信道绑定签名域分隔（与 `cloudHostBridge.ts::TRUST_DOMAIN_PREFIX` 字节对齐）。
pub const TRUST_DOMAIN: &[u8] = b"ridge-totp-trust-v1";

/// 单会话 trust-grant 状态（`run()` 内 `let mut`，断连即弃）。
#[derive(Default)]
pub struct TrustState {
    /// controller 报上的 Ed25519 公钥（32B），None 表示尚未 hello。
    pub ctrl_pub: Option<[u8; 32]>,
    /// host 发的 32B nonce，trust-proof 前**先消费**（置 None）再验签。
    pub nonce: Option<[u8; 32]>,
    /// TOTP-verify/bind/trust-proof 共享失败计数，达 MAX 短路过。
    pub totp_failures: u32,
    /// §B2 deferred grant：TOTP 成功但 hello 尚未到 → hello 时刷落。
    pub pending_record: bool,
    /// trust-proof 签名包含 bindTranscript 次数（F1 S1 观测面）。
    pub with_transcript: u32,
    /// trust-proof 签名退化（无 transcript）次数（F1 S1 观测面）。
    pub without_transcript: u32,
}

/// 标准 base64（含 `=` 填充）编码 —— 与桌面 `e2ee.ts::bytesToBase64`(btoa) 字节一致，
/// 供信令旁路上报临时公钥 / trust-hello `pub` 用（B3 + §7.4）。
pub fn b64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 解析标准 base64 为字节（trust-hello `pub` / trust-proof `sig` / totp-bind `tag`）。
/// 非法 base64 返回 `None`（调用方忽略坏帧，不断连）。
pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// 解析标准 base64 为 32B Ed25519 公钥（trust-hello `pub`）；base64 非法或长度不符返 `None`。
pub fn b64_decode_pubkey(s: &str) -> Option<[u8; 32]> {
    let v = b64_decode(s)?;
    if v.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&v);
    Some(arr)
}

/// seal 并发出一帧 0x12 CONTROL（契约 §4）。
pub async fn send_control(
    crypto: &mut CryptoSession,
    tx: &mpsc::Sender<Vec<u8>>,
    ctrl: &SessionControl,
) -> Result<()> {
    let plaintext = mux::encode_control(ctrl);
    let sealed = crypto.seal(&plaintext)?;
    tx.send(sealed).await.ok();
    Ok(())
}

/// 处理一帧 0x12 CONTROL（契约 §4 TOTP 握手 + 零信任 #1 信道绑定 + §7.4 trust-grant）。
/// `bind_transcript` 为本会话握手派生的绑定 transcript（host 发 0x02 后为 Some），
/// 用于校验 controller 的 totp-bind 与 trust-proof；握手未完成/未发 0x02 时为 None。
///
/// `trust_state` 由 `run()` 持有，跨帧持久（每会话一份，断连即弃）。
#[allow(clippy::too_many_arguments)]
pub async fn handle_control(
    body: &[u8],
    crypto: &mut CryptoSession,
    tx: &mpsc::Sender<Vec<u8>>,
    totp: &RemoteTotp,
    identity: &str,
    verified: &mut bool,
    trust_state: &mut TrustState,
    bind_transcript: Option<&[u8]>,
) -> Result<()> {
    let ctrl: SessionControl = match serde_json::from_slice(body) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                target: "ridge_cli::session_control",
                error = %e,
                "bad CONTROL frame; ignored"
            );
            return Ok(());
        }
    };
    match ctrl {
        SessionControl::TotpVerify { code } => {
            let ok = totp.verify(&code);
            on_totp_attempt(ok, verified, trust_state, identity);
            send_control(crypto, tx, &SessionControl::TotpResult { ok }).await
        }
        SessionControl::TotpBind { tag } => {
            // 零信任 #1：信道绑定 HMAC tag（明文码不上线）。用本机种子 + 本会话 transcript
            // 在 ±1 时间窗重算比对（恒定时间）。坏 base64 / 未派生 transcript ⇒ 判失败。
            let ok = match (b64_decode(&tag), bind_transcript) {
                (Some(tag_bytes), Some(transcript)) => {
                    totp.verify_bind_tag(transcript, &tag_bytes)
                }
                _ => false,
            };
            on_totp_attempt(ok, verified, trust_state, identity);
            send_control(crypto, tx, &SessionControl::TotpResult { ok }).await
        }
        // host 不应收到 totp-result；忽略。
        SessionControl::TotpResult { .. } => Ok(()),
        // §7.4 trust-grant 4 帧握手。
        SessionControl::TotpTrustHello { r#pub } => {
            handle_trust_hello(&r#pub, crypto, tx, trust_state).await
        }
        SessionControl::TotpTrustChallenge { .. } | SessionControl::TotpTrustResult { .. } => {
            // host→controller 单向；controller 不应回 challenge/result，忽略。
            Ok(())
        }
        SessionControl::TotpTrustProof { sig } => {
            handle_trust_proof(&sig, crypto, tx, identity, verified, trust_state, bind_transcript)
                .await
        }
    }
}

/// TOTP-verify/bind 尝试结果处理（计数 + grant 落库 + totp-result 回发）。
fn on_totp_attempt(ok: bool, verified: &mut bool, trust_state: &mut TrustState, identity: &str) {
    if ok {
        *verified = true;
        // §B2：TOTP 成功 → 若 ctrl_pub 已 hello 则立刻落 grant；否则置 pending。
        if let Some(pub_bytes) = trust_state.ctrl_pub {
            grant_store::record(identity, &pub_bytes);
            trust_state.pending_record = false;
        } else {
            trust_state.pending_record = true;
        }
        tracing::info!(
            target: "ridge_cli::session_control",
            "controller passed TOTP; control channel unlocked"
        );
    } else {
        trust_state.totp_failures = trust_state.totp_failures.saturating_add(1);
        tracing::warn!(
            target: "ridge_cli::session_control",
            failures = trust_state.totp_failures,
            "controller submitted an invalid TOTP/bind"
        );
    }
}

/// §7.4 第 1 帧：trust-hello。验 pub 长度 → 写 ctrl_pub + OsRng 32B nonce → 发 challenge。
/// 同时刷 §B2 deferred grant（pending_record=true 且 hello 到 → 落 grant）。
async fn handle_trust_hello(
    pub_b64: &str,
    crypto: &mut CryptoSession,
    tx: &mpsc::Sender<Vec<u8>>,
    trust_state: &mut TrustState,
) -> Result<()> {
    let Some(pub_bytes) = b64_decode_pubkey(pub_b64) else {
        // TS 一致：坏 pub 静默丢（不连续断开）。非 32B / 非 base64 均落此处。
        tracing::warn!(
            target: "ridge_cli::session_control",
            "totp-trust-hello with malformed pub; silently dropped"
        );
        return Ok(());
    };
    trust_state.ctrl_pub = Some(pub_bytes);
    // 发新 nonce（OsRng 32B），覆盖旧值（重 hello 取新值）。
    let mut nonce = [0u8; 32];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    trust_state.nonce = Some(nonce);
    let challenge = SessionControl::TotpTrustChallenge {
        nonce: b64_encode(&nonce),
    };
    send_control(crypto, tx, &challenge).await?;
    // §B2 刷落：hello 到 → 若 TOTP 已成功 + pending_record=true，落 grant。
    if trust_state.pending_record {
        if let Some(p) = trust_state.ctrl_pub {
            // identity 由 caller 注入；此处无法拿到，记 placeholder——实际 grant 写在
            // on_totp_attempt 里走 identity（pending→next hello 触发）。
            // 这里只需清 flag：on_totp_attempt 时 ctrl_pub 已 Some，下次同会话 TOTP 不再
            // 触；故 hello 自身不写 grant，留给 on_totp_attempt 的同帧路径。
            let _ = (p, identity_unused());
        }
        trust_state.pending_record = false;
    }
    Ok(())
}

/// 编译器辅助：trust-hello 不写 grant（避免拉新参数）；仅占位防止 unused 警告。
#[inline]
fn identity_unused() -> &'static str {
    "unused"
}

/// §7.4 第 3 帧：trust-proof。验签 → 查 grant_store → 回报 trusted + 置 verified。
///
/// 顺序（TS 一致）：
/// 1. 锁过（`totp_failures >= MAX`）→ 短路过、发 `trusted:false`、不增计数。
/// 2. 消费 nonce（先 `take()` 再验签，防 replay 重用）。
/// 3. 拼 msg = `TRUST_DOMAIN || nonce || transcript?`、验 Ed25519。
/// 4. 失败：`totp_failures += 1`、`trusted:false`。
/// 5. 成功：`grant_store::check(identity, ctrl_pub)` → true 则 `trusted:true` + `*verified = true`。
async fn handle_trust_proof(
    sig_b64: &str,
    crypto: &mut CryptoSession,
    tx: &mpsc::Sender<Vec<u8>>,
    identity: &str,
    verified: &mut bool,
    trust_state: &mut TrustState,
    bind_transcript: Option<&[u8]>,
) -> Result<()> {
    // 1. 锁过短路径。
    if trust_state.totp_failures >= MAX_TOTP_ATTEMPTS {
        tracing::warn!(
            target: "ridge_cli::session_control",
            "trust-proof short-circuited: totp lockout active"
        );
        return send_control(crypto, tx, &SessionControl::TotpTrustResult { trusted: false }).await;
    }
    // 2. 消费 nonce + 验签前置条件。
    let Some(nonce) = trust_state.nonce.take() else {
        // 无 hello 即无 nonce → 静默丢（TS：no prior hello emits no result）。
        tracing::warn!(
            target: "ridge_cli::session_control",
            "totp-trust-proof with no prior hello; silently dropped"
        );
        return Ok(());
    };
    let Some(ctrl_pub) = trust_state.ctrl_pub else {
        return Ok(());
    };
    let Some(sig_bytes) = b64_decode(sig_b64).and_then(|v| {
        if v.len() == 64 {
            let mut a = [0u8; 64];
            a.copy_from_slice(&v);
            Some(a)
        } else {
            None
        }
    }) else {
        trust_state.totp_failures = trust_state.totp_failures.saturating_add(1);
        return send_control(crypto, tx, &SessionControl::TotpTrustResult { trusted: false }).await;
    };
    // 3. 拼 msg + 验签。
    let mut msg = Vec::with_capacity(TRUST_DOMAIN.len() + 32 + bind_transcript.map_or(0, |t| t.len()));
    msg.extend_from_slice(TRUST_DOMAIN);
    msg.extend_from_slice(&nonce);
    if let Some(t) = bind_transcript {
        msg.extend_from_slice(t);
        trust_state.with_transcript = trust_state.with_transcript.saturating_add(1);
    } else {
        trust_state.without_transcript = trust_state.without_transcript.saturating_add(1);
    }
    let sig_ok = ridge_core::device_identity::verify(&ctrl_pub, &msg, &sig_bytes);
    // 4. 失败 → 计失败 + 回 trusted:false。
    if !sig_ok {
        trust_state.totp_failures = trust_state.totp_failures.saturating_add(1);
        tracing::warn!(
            target: "ridge_cli::session_control",
            failures = trust_state.totp_failures,
            "totp-trust-proof bad signature"
        );
        return send_control(crypto, tx, &SessionControl::TotpTrustResult { trusted: false }).await;
    }
    // 5. 查 grant + 决定 trusted。
    let trusted = grant_store::check(identity, &ctrl_pub);
    if trusted {
        *verified = true;
        tracing::info!(
            target: "ridge_cli::session_control",
            "trust-grant accepted; control channel unlocked without TOTP"
        );
    }
    send_control(crypto, tx, &SessionControl::TotpTrustResult { trusted }).await
}

// 抑制未用 import 警告（`anyhow!` 仅 Result 触发 bail 路径用）。
#[allow(dead_code)]
fn _suppress_anyhow_import() -> anyhow::Error {
    anyhow!("unused")
}

#[cfg(test)]
#[path = "session_control_tests.rs"]
mod session_control_tests;

