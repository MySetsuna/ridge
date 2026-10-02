//! LAN §7.4 trust-grant HTTP 路由（POST /trust-grant 与 POST /trust-grant/proof）。
//!
//! 与云腿差异：
//! - LAN 无 bindTranscript：签名消息退化为 `prefix || nonce`（TS 已知 S1 退役面），计
//!   `without_transcript` 计数器以便观测。
//! - 共享 `?code=` TOTP 同一节流/封禁闸门：均经 `host.pre_verify_gate` + `host.post_verify_record`。
//! - 必须携带会话令牌（`Authorization: Bearer <t>` 或 `?token=`），路径与 `/file`、`/ws` 同款设备+IP 绑定。
//!
//! 文件行数控制：handler ~180 行 + 测试 ~260 行。

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::Engine as _;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use ridge_core::device_identity;

use crate::host::RemoteHost;

// ── 协议常量 ────────────────────────────────────────────────────────────────

/// 信任签名消息前缀（与 TS `cloudHostBridge.ts:454-473` 同源；云 + LAN 腿共用）。
pub const TRUST_SIGN_PREFIX: &[u8] = b"ridge-totp-trust-v1";

/// nonce 字节长度。
pub const NONCE_LEN: usize = 32;
/// 公钥字节长度（Ed25519）。
pub const PUB_LEN: usize = 32;
/// 签名字节长度（Ed25519）。
pub const SIG_LEN: usize = 64;

/// hello→proof 最大允许时延（60s 懒清）。超时的 hello entry 在下一次 hello/proof 时清理。
pub const LAN_TRUST_TTL_SECS: u64 = 60;

// ── 共享状态 ────────────────────────────────────────────────────────────────

/// 单个 hello 缓存条目；证明过期或验证完成即清。
#[derive(Clone, Debug)]
struct LanTrustEntry {
    ctrl_pub: [u8; PUB_LEN],
    nonce: [u8; NONCE_LEN],
    created: u64,
}

/// 跨进程内 LAN trust-grant hello→proof 配对状态。注入到 [`crate::server_app::router`]
/// 之前即应包成 `Arc`，handler 持有 `Arc`。
#[derive(Default)]
pub struct LanTrustState {
    inner: Mutex<HashMap<TrustKey, LanTrustEntry>>,
}

/// `(device_id, ip)` 复合键；空 device 与 IP 字符串直接拼。
type TrustKey = (String, String);

impl LanTrustState {
    /// 新建空状态。
    pub fn new() -> Self {
        Self::default()
    }

    /// 60s TTL 懒清：返回清理掉的条目数（仅测试断言用）。
    fn cleanup_expired(&self, now: u64) -> usize {
        let mut map = self.inner.lock();
        let before = map.len();
        map.retain(|_, e| now.saturating_sub(e.created) < LAN_TRUST_TTL_SECS);
        before - map.len()
    }

    /// 写入 hello 缓存（覆盖同名旧条目）。
    fn put(&self, device_id: &str, ip: &str, entry: LanTrustEntry) {
        let now = now_unix();
        self.cleanup_expired(now);
        self.inner.lock().insert((device_id.to_string(), ip.to_string()), entry);
    }

    /// 取出并移除 hello 缓存（一次性消费：防重放）。
    fn take(&self, device_id: &str, ip: &str) -> Option<LanTrustEntry> {
        let now = now_unix();
        self.cleanup_expired(now);
        self.inner.lock().remove(&(device_id.to_string(), ip.to_string()))
    }

    /// 缓存长度（仅测试断言用）。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }
}

/// LAN trust-grant handler 共享的强类型 State：宿主 trait + hello→proof 缓存。
///
/// 由 [`crate::server_app::router`] 在装配时整体注入（`.with_state(ctx)`）。
/// 不经 `FromRef` 派生 —— 路由仅这一种 state，无需向下分发。
#[derive(Clone)]
pub struct LanTrustCtx {
    pub host: Arc<dyn RemoteHost>,
    pub state: Arc<LanTrustState>,
}

// ── 请求/响应 ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloBody {
    /// base64（32B）控制器 Ed25519 公钥。
    pub pub_key: String,
}

#[derive(Serialize)]
struct HelloResponse {
    /// base64（32B）随机 nonce —— 控制器下一步签名 `prefix || nonce`。
    nonce: String,
}

#[derive(Deserialize)]
pub struct ProofBody {
    /// base64（64B）Ed25519 签名。
    pub sig: String,
}

#[derive(Serialize)]
struct ProofResponse {
    trusted: bool,
}

// ── 公开 handler（供 server_app 注册）────────────────────────────────────────

/// POST /trust-grant body {"pub":"<b64>"} → 200 {"nonce":"<b64>"} | 400 | 401 | 429。
pub async fn trust_hello_handler(
    State(ctx): State<LanTrustCtx>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<HelloBody>,
) -> impl IntoResponse {
    let ip = addr.ip().to_string();
    let device_id = extract_device_from_headers(&headers);

    // (1) 令牌鉴权：必须已持 24h 信任或刚输过 TOTP 的设备（共享 /ws 同款闸门）。
    if !is_request_authed(&ctx.host, &headers, &device_id, &ip) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }

    // (2) 公钥 32B 解码 + 校验（base64 + 长度）。
    let raw_pub = match base64::engine::general_purpose::STANDARD.decode(&body.pub_key) {
        Ok(b) => b,
        Err(_) => return bad_request("pub must be base64"),
    };
    if raw_pub.len() != PUB_LEN {
        return bad_request("pub must decode to 32 bytes");
    }
    let mut ctrl_pub = [0u8; PUB_LEN];
    ctrl_pub.copy_from_slice(&raw_pub);

    // (3) TOTP 暴力破解节流 + 黑名单闸门（与 ?code= 同款）。
    if ctx.host.is_blacklisted(&device_id, &ip)
        || ctx.host.pre_verify_gate(&ip, &device_id).is_err()
    {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    // (4) hello 本体不消耗"验证失败"计数（TS 一致；信任失败计数仅 proof 步累加）。
    //     仍 record 一次"成功事件"以保持 throttle 滑动窗口对齐（?code= 是真验证，
    //     hello 是鉴权预备动作，记成功等价）。
    ctx.host.post_verify_record(&ip, &device_id, true);

    // (5) OsRng 32B nonce + 缓存。
    let nonce = generate_nonce();
    ctx.state.put(
        &device_id,
        &ip,
        LanTrustEntry { ctrl_pub, nonce, created: now_unix() },
    );

    let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce);
    (StatusCode::OK, Json(HelloResponse { nonce: nonce_b64 })).into_response()
}

/// POST /trust-grant/proof body {"sig":"<b64>"} → 200 {"trusted":bool} | 400 | 401 | 429。
pub async fn trust_proof_handler(
    State(ctx): State<LanTrustCtx>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<ProofBody>,
) -> impl IntoResponse {
    let ip = addr.ip().to_string();
    let device_id = extract_device_from_headers(&headers);

    if !is_request_authed(&ctx.host, &headers, &device_id, &ip) {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }

    // (1) 黑名单 + 节流闸门。
    if ctx.host.is_blacklisted(&device_id, &ip)
        || ctx.host.pre_verify_gate(&ip, &device_id).is_err()
    {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    // (2) hello 缓存查找 + 一次性消费。
    let Some(entry) = ctx.state.take(&device_id, &ip) else {
        return (StatusCode::BAD_REQUEST, "no pending hello").into_response();
    };

    // (3) sig 64B 解码。
    let raw_sig = match base64::engine::general_purpose::STANDARD.decode(&body.sig) {
        Ok(b) => b,
        Err(_) => return bad_request("sig must be base64"),
    };
    if raw_sig.len() != SIG_LEN {
        return bad_request("sig must decode to 64 bytes");
    }
    let mut sig = [0u8; SIG_LEN];
    sig.copy_from_slice(&raw_sig);

    // (4) 验签：msg = prefix || nonce（LAN 无 transcript，退化）。
    let mut msg = Vec::with_capacity(TRUST_SIGN_PREFIX.len() + NONCE_LEN);
    msg.extend_from_slice(TRUST_SIGN_PREFIX);
    msg.extend_from_slice(&entry.nonce);
    let trusted = device_identity::verify(&entry.ctrl_pub, &msg, &sig);

    // (5) 失败计数反馈到 throttle（无论签 true/false，POST /verify 一致 record）。
    //     LAN 总是 without_transcript；与云腿 with_transcript=0 + without_transcript=1
    //     等价（云腿区分仅用于观测；两条腿都按"一次 verify 事件"反馈 throttle）。
    ctx.host.post_verify_record(&ip, &device_id, trusted);

    if !trusted {
        return (StatusCode::OK, Json(ProofResponse { trusted: false })).into_response();
    }

    // (6) 签名通过 → 落 grant 24h 窗（LAN 退化面等价于云 deferred grant 路径，
    //     proof 即 TOTP 验证后的落点）；下次 hello 由 controller 发起即可复用。
    ctx.host.totp_trust_record(&entry.ctrl_pub);
    (StatusCode::OK, Json(ProofResponse { trusted: true })).into_response()
}

// ── 内部辅助 ────────────────────────────────────────────────────────────────

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn generate_nonce() -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut n).expect("OS CSPRNG failed");
    n
}

fn bad_request(msg: &'static str) -> axum::response::Response {
    (StatusCode::BAD_REQUEST, msg).into_response()
}

/// 从 `X-Ridge-Device` 头取 device id（与 `/ws`、`/file` 一致；缺省空串）。
fn extract_device_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-ridge-device")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// 复制 server_app 的 Bearer 令牌校验（设备+IP 严格绑定；空 device 走 IP-only 兜底，
/// 与现有 `/file`、`/ws` 同款语义）。headers 已含 ?token= 解析不在此 —— 当前 trust
/// handler 不支持 query token，遵循 POST 端点只用 `Authorization` 头约定。
fn is_request_authed(
    host: &Arc<dyn RemoteHost>,
    headers: &HeaderMap,
    device_id: &str,
    ip: &str,
) -> bool {
    let header_token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer ").map(str::trim));
    header_token
        .map(|t| host.validate_token_device_strict(t, device_id, ip))
        .unwrap_or(false)
}

// ── 单测 ──────────────────────────────────────────────────────────────────
#[cfg(test)]
#[path = "lan_trust_tests.rs"]
mod tests;
