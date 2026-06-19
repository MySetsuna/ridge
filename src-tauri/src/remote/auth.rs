use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use ridge_core::RemoteTotp;

/// 本机 TOTP 持有者（契约 §4）。内部委托 ridge-core 的 `RemoteTotp`（唯一权威
/// 实现），secret 由其持久化层跨重启恢复。`RwLock` 支持登录态变化时**实时切换**
/// 活动种子（远控 server 持 `Arc<RemoteAuth>`，需内部可变）。
pub struct RemoteAuth {
    totp: RwLock<RemoteTotp>,
}

impl RemoteAuth {
    /// 启动时桌面尚未登录 → 先用 `"default"` 身份的持久化种子；登录后由
    /// `switch_identity` 切到账号专属种子。
    pub fn new() -> Self {
        Self {
            totp: RwLock::new(RemoteTotp::load_or_create("default")),
        }
    }

    /// 生成当前 6 位 TOTP。
    pub fn current_code(&self) -> String {
        self.totp.read().current_code()
    }

    /// 校验用户输入的 code（±1 窗口在 `RemoteTotp::verify` 内）。
    pub fn verify(&self, code: &str) -> bool {
        self.totp.read().verify(code)
    }

    /// 零信任 #1：校验对端在 transcript 上的信道绑定 tag（±1 窗口，透传 `RemoteTotp::verify_bind_tag`）。
    pub fn verify_bind_tag(&self, transcript: &[u8], tag: &[u8]) -> bool {
        self.totp.read().verify_bind_tag(transcript, tag)
    }

    /// 当前 code + otpauth URI 一次取（同一把读锁，避免时间步在两次调用间跳变）。
    pub fn code_and_uri(&self, machine_name: &str) -> (String, String) {
        let g = self.totp.read();
        (g.current_code(), g.otpauth_uri(machine_name))
    }

    /// 供二维码生成的 `otpauth://` URI。
    pub fn otpauth_uri(&self, machine_name: &str) -> String {
        self.totp.read().otpauth_uri(machine_name)
    }

    /// 重置当前身份的种子（已配对验证器即失效，需重新扫码）。
    pub fn reset_totp(&self) {
        self.totp.write().reset();
    }

    /// 切换活动种子到指定云身份（`None` → `"default"`）。登录/登出时调用。
    pub fn switch_identity(&self, username: Option<&str>) {
        self.totp.write().switch_identity(username.unwrap_or("default"));
    }

    /// 测试用：临时随机种子，不落盘（避免单测写真实 AppData）。
    #[cfg(test)]
    fn ephemeral() -> Self {
        Self {
            totp: RwLock::new(RemoteTotp::new()),
        }
    }
}

/// SECURITY (audit H5): shortened from 3 days to 12 hours. A session token is a
/// bearer credential for full shell/file control over the LAN; a 3-day window
/// gave a stolen/forgotten token a very long replay life. 12h still spans a
/// normal working session while bounding exposure if a device is lost or a token
/// leaks (e.g. via the `?token=` query string in logs/history).
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);

/// Per-token binding + issue time. SECURITY (audit H5): tokens are pinned to the
/// device id and source IP they were issued to, so a token sniffed/exfiltrated
/// from one device can't be replayed from another host on the LAN.
#[derive(Clone)]
struct SessionRecord {
    created: Instant,
    /// Stable client-generated device id at issuance ("" if the client sent none).
    device_id: String,
    /// Source IP the token was issued to.
    ip: String,
}

pub struct SessionStore {
    tokens: Mutex<HashMap<String, SessionRecord>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Issue a token bound to the issuing device id + IP (audit H5).
    pub fn create_session_bound(&self, device_id: &str, ip: &str) -> String {
        // SECURITY (audit C2): session tokens are bearer credentials for full
        // remote control, so they MUST be unguessable. Draw 32 bytes (256 bits)
        // from the OS CSPRNG and hex-encode (64 chars, same format as before).
        // The previous xorshift PRNG could be reverse-engineered from observed
        // tokens to forge new ones, bypassing TOTP entirely.
        let token = generate_session_token();
        self.tokens.lock().insert(
            token.clone(),
            SessionRecord {
                created: Instant::now(),
                device_id: device_id.to_string(),
                ip: ip.to_string(),
            },
        );
        self.cleanup_expired();
        token
    }

    /// Existence + TTL check only (no binding). Used where the request context
    /// can't supply the device/IP to compare against (e.g. the `/session`
    /// liveness poll). The binding is enforced on the channels that actually
    /// grant control — `/ws` and `/file` — via [`validate_token_bound`].
    pub fn validate_token(&self, token: &str) -> bool {
        let mut map = self.tokens.lock();
        if let Some(rec) = map.get(token) {
            if rec.created.elapsed() < SESSION_TTL {
                return true;
            }
            map.remove(token);
        }
        false
    }

    /// Validate a token AND that it is being presented from the same identity it
    /// was issued to (audit H5). `device_id` may be empty when the caller can't
    /// supply one (e.g. `/file` image requests); in that case only the IP is
    /// compared. The IP must always match. The device id must match when BOTH
    /// the stored and presented ids are non-empty.
    pub fn validate_token_bound(&self, token: &str, device_id: &str, ip: &str) -> bool {
        let mut map = self.tokens.lock();
        let Some(rec) = map.get(token) else {
            return false;
        };
        if rec.created.elapsed() >= SESSION_TTL {
            map.remove(token);
            return false;
        }
        // IP must always match the issuing IP.
        if rec.ip != ip {
            return false;
        }
        // Device id must match when both sides provide one. (An empty presented
        // id — e.g. an `<img>`-driven `/file` fetch — falls back to the IP pin.)
        if !rec.device_id.is_empty() && !device_id.is_empty() && rec.device_id != device_id {
            return false;
        }
        true
    }

    /// Like [`validate_token_bound`], but for control-granting endpoints
    /// (`/ws`, `/workspace/*`) an EMPTY presented device is NOT allowed to fall
    /// back to the IP pin when the token WAS issued with a device binding.
    ///
    /// SECURITY (audit L-3): the bound check skips the device comparison whenever
    /// either side is empty, so an attacker sharing the victim's NAT egress IP
    /// could downgrade a device-bound token to IP-only by simply omitting the
    /// device. Control paths close that escape: a token carrying a device id MUST
    /// have that exact id presented. A token issued WITHOUT a device (stored id
    /// empty — legacy clients, or `<img>`-only `/file` flows that use the bound
    /// check) still validates on the IP pin alone, so existing sessions and
    /// header-less image fetches aren't locked out.
    pub fn validate_token_device_strict(&self, token: &str, device_id: &str, ip: &str) -> bool {
        let mut map = self.tokens.lock();
        let Some(rec) = map.get(token) else {
            return false;
        };
        if rec.created.elapsed() >= SESSION_TTL {
            map.remove(token);
            return false;
        }
        if rec.ip != ip {
            return false;
        }
        // A device-bound token must present its exact device — no empty-device
        // downgrade to the IP pin. Deviceless (legacy) tokens keep the IP pin.
        if !rec.device_id.is_empty() && rec.device_id != device_id {
            return false;
        }
        true
    }

    /// Revoke a session token so the device can no longer reconnect with it
    /// (force-disconnect). The device must re-enter the auth code to obtain a
    /// fresh token. No-op if the token is unknown.
    pub fn invalidate(&self, token: &str) {
        self.tokens.lock().remove(token);
    }

    fn cleanup_expired(&self) {
        self.tokens
            .lock()
            .retain(|_, rec| rec.created.elapsed() < SESSION_TTL);
    }
}

// ── TOTP brute-force throttle (audit C1) ────────────────────────────────────

/// Failures before exponential backoff kicks in.
const THROTTLE_SOFT_LIMIT: u32 = 5;
/// Failures before a hard temp-ban + blacklist auto-add.
const THROTTLE_HARD_LIMIT: u32 = 10;
/// Temp-ban duration once the hard cap is hit.
const THROTTLE_BAN: Duration = Duration::from_secs(15 * 60);
/// Base unit for exponential backoff between the soft and hard limits.
const THROTTLE_BACKOFF_BASE: Duration = Duration::from_secs(2);
/// Cap on a single backoff step so the math can't overflow / stall forever.
const THROTTLE_BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Sliding window after which an idle key's failure count resets to 0. Prevents
/// a slow trickle of mistypes (one every few minutes) from ever locking out a
/// legitimate user, while still catching a real burst-force.
const THROTTLE_RESET_AFTER: Duration = Duration::from_secs(15 * 60);
/// Global verify rate limit: max accepted verify *attempts* per window across
/// all sources. A flood beyond this is shed regardless of per-key state, so a
/// botnet spreading the guess across many IPs/devices still can't outrun it.
const THROTTLE_GLOBAL_MAX: u32 = 30;
const THROTTLE_GLOBAL_WINDOW: Duration = Duration::from_secs(10);

/// Outcome of `VerifyThrottle::check` — whether a verify attempt may proceed,
/// and if not, why (for logging; the HTTP layer returns a *uniform* message to
/// the client so it can't distinguish lockout from a wrong code — audit M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleDecision {
    /// Attempt may proceed to the actual TOTP check.
    Allow,
    /// Caller is in exponential-backoff cooldown; must wait `retry_after`.
    Backoff { retry_after: Duration },
    /// Caller is temp-banned (hard cap reached). `banned` is true the first time
    /// the ban trips so the caller can auto-add to the persistent blacklist once.
    Banned { retry_after: Duration, fresh: bool },
    /// Global verify rate limit exceeded — shed load.
    GlobalLimited,
}

#[derive(Clone)]
struct AttemptRecord {
    failures: u32,
    /// When the next attempt is permitted (backoff / ban release).
    locked_until: Option<Instant>,
    last_seen: Instant,
}

/// Per-source brute-force throttle for the TOTP `/verify` (+ `?code=` WS) path.
///
/// SECURITY (audit C1): the previous server applied *no* rate limiting to TOTP
/// verification — a 6-digit code with a ±1 window (3 live codes) is exhaustible
/// from the LAN in seconds. This tracks failed attempts per IP **and** per
/// device id (a key matches on either), applies exponential backoff after
/// `THROTTLE_SOFT_LIMIT`, and a 15-minute temp-ban after `THROTTLE_HARD_LIMIT`
/// (also surfaced so the caller can auto-blacklist). A coarse global limiter
/// caps total verify throughput so a distributed guess can't bypass per-key
/// state. Thread-safe via a single `Mutex` (verify is low-QPS, so contention is
/// a non-issue).
#[derive(Default)]
pub struct VerifyThrottle {
    by_ip: Mutex<HashMap<String, AttemptRecord>>,
    by_device: Mutex<HashMap<String, AttemptRecord>>,
    global: Mutex<Vec<Instant>>,
}

impl VerifyThrottle {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide whether a verify attempt from `(ip, device_id)` may proceed.
    /// Call BEFORE checking the TOTP code; call `record_failure` / `record_success`
    /// afterwards based on the result. `device_id` may be empty (not provided).
    pub fn check(&self, ip: &str, device_id: &str) -> ThrottleDecision {
        let now = Instant::now();
        // Per-key decision FIRST. A source already in backoff/ban is rejected here
        // WITHOUT touching the global limiter (audit M-1): otherwise a single
        // throttled attacker could flood the `/verify` path with attempts that are
        // doomed anyway, fill the global window, and shed legitimate users.
        // The strictest of the IP and device records governs.
        let ip_dec = self.key_decision(&self.by_ip, ip, now);
        let dev_dec = if device_id.is_empty() {
            ThrottleDecision::Allow
        } else {
            self.key_decision(&self.by_device, device_id, now)
        };
        let decision = strictest(ip_dec, dev_dec);
        if decision != ThrottleDecision::Allow {
            return decision;
        }
        // Only attempts that would actually reach the TOTP check consume global
        // budget — this still caps a distributed (many fresh IPs) guess, while no
        // longer letting throttled sources deny service to everyone else.
        if !self.global_allow() {
            return ThrottleDecision::GlobalLimited;
        }
        ThrottleDecision::Allow
    }

    /// Record a failed verify for both keys (advances backoff / ban state).
    /// Returns true if THIS failure freshly tripped the hard-cap ban on either
    /// key (so the caller can add to the persistent blacklist exactly once).
    pub fn record_failure(&self, ip: &str, device_id: &str) -> bool {
        let now = Instant::now();
        let mut banned = self.bump_failure(&self.by_ip, ip, now);
        if !device_id.is_empty() {
            banned |= self.bump_failure(&self.by_device, device_id, now);
        }
        banned
    }

    /// Clear throttle state for both keys after a successful verify, so a
    /// legitimate user who fat-fingered a few codes isn't punished afterwards.
    pub fn record_success(&self, ip: &str, device_id: &str) {
        self.by_ip.lock().remove(ip);
        if !device_id.is_empty() {
            self.by_device.lock().remove(device_id);
        }
    }

    fn global_allow(&self) -> bool {
        let now = Instant::now();
        let mut hits = self.global.lock();
        hits.retain(|t| now.duration_since(*t) < THROTTLE_GLOBAL_WINDOW);
        if hits.len() as u32 >= THROTTLE_GLOBAL_MAX {
            return false;
        }
        hits.push(now);
        true
    }

    fn key_decision(
        &self,
        map: &Mutex<HashMap<String, AttemptRecord>>,
        key: &str,
        now: Instant,
    ) -> ThrottleDecision {
        let mut guard = map.lock();
        let Some(rec) = guard.get_mut(key) else {
            return ThrottleDecision::Allow;
        };
        // Idle long enough → forget past failures entirely.
        if now.duration_since(rec.last_seen) >= THROTTLE_RESET_AFTER {
            guard.remove(key);
            return ThrottleDecision::Allow;
        }
        if let Some(until) = rec.locked_until {
            if until > now {
                let retry_after = until - now;
                return if rec.failures >= THROTTLE_HARD_LIMIT {
                    ThrottleDecision::Banned {
                        retry_after,
                        fresh: false,
                    }
                } else {
                    ThrottleDecision::Backoff { retry_after }
                };
            }
        }
        ThrottleDecision::Allow
    }

    fn bump_failure(
        &self,
        map: &Mutex<HashMap<String, AttemptRecord>>,
        key: &str,
        now: Instant,
    ) -> bool {
        let mut guard = map.lock();
        let rec = guard.entry(key.to_string()).or_insert(AttemptRecord {
            failures: 0,
            locked_until: None,
            last_seen: now,
        });
        // Reset stale counters before counting this failure.
        if now.duration_since(rec.last_seen) >= THROTTLE_RESET_AFTER {
            rec.failures = 0;
            rec.locked_until = None;
        }
        rec.failures = rec.failures.saturating_add(1);
        rec.last_seen = now;
        let was_banned = rec
            .locked_until
            .map(|u| u > now && rec.failures > THROTTLE_HARD_LIMIT)
            .unwrap_or(false);
        if rec.failures >= THROTTLE_HARD_LIMIT {
            rec.locked_until = Some(now + THROTTLE_BAN);
            // Fresh ban only on the transition into the hard cap.
            rec.failures == THROTTLE_HARD_LIMIT && !was_banned
        } else if rec.failures >= THROTTLE_SOFT_LIMIT {
            // Exponential backoff: base * 2^(failures - soft_limit), capped.
            let exp = rec.failures - THROTTLE_SOFT_LIMIT;
            let mult = 1u64.checked_shl(exp).unwrap_or(u64::MAX);
            let delay = THROTTLE_BACKOFF_BASE
                .checked_mul(mult.min(u32::MAX as u64) as u32)
                .unwrap_or(THROTTLE_BACKOFF_MAX)
                .min(THROTTLE_BACKOFF_MAX);
            rec.locked_until = Some(now + delay);
            false
        } else {
            false
        }
    }
}

/// Pick the more restrictive of two throttle decisions.
fn strictest(a: ThrottleDecision, b: ThrottleDecision) -> ThrottleDecision {
    use ThrottleDecision::*;
    let rank = |d: &ThrottleDecision| match d {
        Allow => 0u8,
        GlobalLimited => 1,
        Backoff { .. } => 2,
        Banned { .. } => 3,
    };
    if rank(&a) >= rank(&b) {
        a
    } else {
        b
    }
}

/// Generate a 256-bit session token, hex-encoded as 64 lowercase chars.
///
/// SECURITY (audit C2): sourced from the OS CSPRNG (`getrandom`). Aborts rather
/// than emitting a guessable token if the OS RNG is unavailable.
fn generate_session_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("OS CSPRNG unavailable for session token generation");
    let mut token = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(token, "{:02x}", b);
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_token_is_64_lowercase_hex_chars() {
        let token = generate_session_token();
        assert_eq!(token.len(), 64, "256-bit token = 64 hex chars");
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "token must be lowercase hex"
        );
    }

    #[test]
    fn session_tokens_are_distinct() {
        // CSPRNG output must not repeat across calls (would mean a broken RNG).
        let a = generate_session_token();
        let b = generate_session_token();
        assert_ne!(a, b);
    }

    #[test]
    fn ephemeral_auth_verifies_its_own_code() {
        let auth = RemoteAuth::ephemeral();
        let (code, uri) = auth.code_and_uri("My Machine");
        assert!(auth.verify(&code), "RemoteAuth 须能校验自己当前的 code");
        assert!(uri.starts_with("otpauth://totp/Ridge:"), "otpauth URI 形状正确");
    }

    #[test]
    fn store_create_validate_roundtrip() {
        let store = SessionStore::new();
        let token = store.create_session_bound("devX", "1.2.3.4");
        assert!(store.validate_token(&token));
        store.invalidate(&token);
        assert!(!store.validate_token(&token));
    }

    // ── SessionStore binding (audit H5) ──

    #[test]
    fn bound_token_requires_matching_ip() {
        let store = SessionStore::new();
        let token = store.create_session_bound("devX", "1.2.3.4");
        // Same identity → ok.
        assert!(store.validate_token_bound(&token, "devX", "1.2.3.4"));
        // Different IP (token replayed from another LAN host) → rejected.
        assert!(!store.validate_token_bound(&token, "devX", "9.9.9.9"));
    }

    #[test]
    fn bound_token_requires_matching_device_when_both_present() {
        let store = SessionStore::new();
        let token = store.create_session_bound("devX", "1.2.3.4");
        // Wrong device id from the SAME ip → rejected.
        assert!(!store.validate_token_bound(&token, "devY", "1.2.3.4"));
    }

    #[test]
    fn bound_token_allows_empty_presented_device_via_ip_pin() {
        // `/file` image requests can't send a device id; the IP pin still holds.
        let store = SessionStore::new();
        let token = store.create_session_bound("devX", "1.2.3.4");
        assert!(store.validate_token_bound(&token, "", "1.2.3.4"));
        assert!(!store.validate_token_bound(&token, "", "9.9.9.9"));
    }

    #[test]
    fn bound_token_unknown_is_rejected() {
        let store = SessionStore::new();
        assert!(!store.validate_token_bound("deadbeef", "devX", "1.2.3.4"));
    }

    // ── SessionStore device-strict path (audit L-3) ──

    #[test]
    fn strict_token_rejects_empty_or_wrong_device_when_bound() {
        let store = SessionStore::new();
        let token = store.create_session_bound("devX", "1.2.3.4");
        // Correct device + IP → ok.
        assert!(store.validate_token_device_strict(&token, "devX", "1.2.3.4"));
        // Empty device can no longer downgrade a device-bound token to the IP pin
        // (this is the L-3 escape the bound check would have allowed).
        assert!(!store.validate_token_device_strict(&token, "", "1.2.3.4"));
        // Wrong device → rejected.
        assert!(!store.validate_token_device_strict(&token, "devY", "1.2.3.4"));
        // IP is still always pinned.
        assert!(!store.validate_token_device_strict(&token, "devX", "9.9.9.9"));
    }

    #[test]
    fn strict_token_allows_ip_pin_for_deviceless_token() {
        // A token issued WITHOUT a device (legacy client / header-less flow) still
        // validates on the IP pin alone, so the strict path doesn't lock it out.
        let store = SessionStore::new();
        let token = store.create_session_bound("", "1.2.3.4");
        assert!(store.validate_token_device_strict(&token, "", "1.2.3.4"));
        assert!(store.validate_token_device_strict(&token, "whatever", "1.2.3.4"));
        // …but the IP pin still holds.
        assert!(!store.validate_token_device_strict(&token, "", "9.9.9.9"));
    }

    // ── VerifyThrottle (audit C1) ──

    #[test]
    fn throttle_allows_first_attempts() {
        let t = VerifyThrottle::new();
        assert_eq!(t.check("1.2.3.4", "devA"), ThrottleDecision::Allow);
    }

    #[test]
    fn throttle_backs_off_after_soft_limit() {
        let t = VerifyThrottle::new();
        // First 4 failures stay under the soft limit → still Allow.
        for _ in 0..(THROTTLE_SOFT_LIMIT - 1) {
            assert_eq!(t.record_failure("1.2.3.4", "devA"), false);
            assert_eq!(t.check("1.2.3.4", "devA"), ThrottleDecision::Allow);
        }
        // The soft-limit-th failure arms backoff.
        assert_eq!(t.record_failure("1.2.3.4", "devA"), false);
        assert!(matches!(
            t.check("1.2.3.4", "devA"),
            ThrottleDecision::Backoff { .. }
        ));
    }

    #[test]
    fn throttle_bans_after_hard_limit_exactly_once() {
        let t = VerifyThrottle::new();
        let mut fresh_count = 0;
        for _ in 0..THROTTLE_HARD_LIMIT {
            if t.record_failure("9.9.9.9", "devB") {
                fresh_count += 1;
            }
        }
        assert_eq!(fresh_count, 1, "fresh ban must trip exactly once");
        assert!(matches!(
            t.check("9.9.9.9", "devB"),
            ThrottleDecision::Banned { .. }
        ));
        // A further failure stays banned but does NOT re-trip `fresh`.
        assert_eq!(t.record_failure("9.9.9.9", "devB"), false);
    }

    #[test]
    fn throttle_success_clears_state() {
        let t = VerifyThrottle::new();
        for _ in 0..THROTTLE_SOFT_LIMIT {
            t.record_failure("5.5.5.5", "devC");
        }
        assert!(matches!(
            t.check("5.5.5.5", "devC"),
            ThrottleDecision::Backoff { .. }
        ));
        t.record_success("5.5.5.5", "devC");
        assert_eq!(t.check("5.5.5.5", "devC"), ThrottleDecision::Allow);
    }

    #[test]
    fn throttle_ip_and_device_are_independent_keys() {
        let t = VerifyThrottle::new();
        // Hammer one IP with an empty device id.
        for _ in 0..THROTTLE_HARD_LIMIT {
            t.record_failure("7.7.7.7", "");
        }
        // Same IP is banned…
        assert!(matches!(
            t.check("7.7.7.7", "freshdevice"),
            ThrottleDecision::Banned { .. }
        ));
        // …but a different IP with a fresh device is unaffected.
        assert_eq!(t.check("8.8.8.8", "freshdevice"), ThrottleDecision::Allow);
    }

    #[test]
    fn throttle_global_limit_sheds_load() {
        let t = VerifyThrottle::new();
        // Each distinct key passes per-key checks but consumes a global slot.
        for i in 0..THROTTLE_GLOBAL_MAX {
            let ip = format!("10.0.0.{i}");
            assert_eq!(t.check(&ip, ""), ThrottleDecision::Allow);
        }
        // One past the global window cap is shed.
        assert_eq!(t.check("10.0.99.99", ""), ThrottleDecision::GlobalLimited);
    }

    #[test]
    fn throttle_banned_source_does_not_consume_global_budget() {
        // audit M-1: a source already in backoff/ban must be rejected WITHOUT
        // eating a global-limit slot. Otherwise a single throttled attacker could
        // flood the global window with doomed attempts and shed legitimate users.
        let t = VerifyThrottle::new();
        // Drive one source into a hard-cap ban.
        for _ in 0..THROTTLE_HARD_LIMIT {
            t.record_failure("6.6.6.6", "devBan");
        }
        assert!(matches!(
            t.check("6.6.6.6", "devBan"),
            ThrottleDecision::Banned { .. }
        ));
        // Hammer the banned source far past the global window cap. None of these
        // doomed attempts should consume global budget…
        for _ in 0..(THROTTLE_GLOBAL_MAX * 3) {
            assert!(matches!(
                t.check("6.6.6.6", "devBan"),
                ThrottleDecision::Banned { .. }
            ));
        }
        // …so a legitimate fresh source still gets through (not GlobalLimited).
        assert_eq!(t.check("4.4.4.4", "devOk"), ThrottleDecision::Allow);
    }
}
