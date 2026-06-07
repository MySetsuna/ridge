use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use sha2::{Digest, Sha256};

const TOTP_PERIOD: u64 = 30;
const TOTP_DIGITS: u64 = 6;
const TOTP_SKEW: i64 = 1;

/// Minimal TOTP (RFC 6238) implementation using SHA-256 HMAC.
///
/// No external crate dependencies — we use `sha2` (already in the dep tree)
/// to implement HMAC-SHA256 directly.
pub struct RemoteAuth {
    secret: Vec<u8>,
}

impl RemoteAuth {
    pub fn new() -> Self {
        Self {
            secret: generate_secret(),
        }
    }

    /// Generate the current 6-digit TOTP code.
    pub fn current_code(&self) -> String {
        let now = now_secs();
        totp_at(&self.secret, now)
    }

    /// Verify a user-supplied code, checking the current +-1 window.
    pub fn verify(&self, code: &str) -> bool {
        if code.len() != TOTP_DIGITS as usize {
            return false;
        }
        let now = now_secs();
        for offset in -TOTP_SKEW..=TOTP_SKEW {
            let ts = if offset >= 0 {
                now.saturating_add(offset as u64)
            } else {
                now.saturating_sub((-offset) as u64)
            };
            if constant_time_eq(totp_at(&self.secret, ts).as_bytes(), code.as_bytes()) {
                return true;
            }
        }
        false
    }

    /// Return current code + otpauth URI in one call (avoids race
    /// if the time step ticks between two separate calls).
    pub fn code_and_uri(&self, machine_name: &str) -> (String, String) {
        (self.current_code(), self.otpauth_uri(machine_name))
    }

    /// Return an `otpauth://` URI suitable for QR-code generation.
    /// Uses RFC 4648 base32 (no padding) for the secret.
    pub fn otpauth_uri(&self, machine_name: &str) -> String {
        // Simple manual encoding for the label part
        let label = machine_name.replace(' ', "%20");
        format!(
            "otpauth://totp/Ridge:{label}?secret={}&issuer=Ridge&algorithm=SHA256&digits={}&period={}",
            base32_encode(&self.secret),
            TOTP_DIGITS,
            TOTP_PERIOD,
        )
    }
}

/// Generate the 20-byte (160-bit) TOTP secret seed.
///
/// SECURITY (audit C2): the secret seed determines every past/future TOTP
/// code, so it MUST be unpredictable. We pull it from the OS CSPRNG
/// (`getrandom`, backed by `getrandom`/`BCryptGenRandom`/`/dev/urandom`).
/// The previous implementation seeded a non-cryptographic xorshift PRNG with
/// `nanos ^ pid` (low entropy, observable), letting a LAN attacker reconstruct
/// the seed and derive all codes. If the OS RNG ever fails we abort secret
/// generation rather than fall back to a weak source — a remote server with a
/// predictable TOTP secret is worse than one that fails to start its auth.
fn generate_secret() -> Vec<u8> {
    let mut buf = vec![0u8; 20];
    getrandom::getrandom(&mut buf).expect("OS CSPRNG unavailable for TOTP secret generation");
    buf
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a TOTP code for the given secret and time step.
fn totp_at(secret: &[u8], time_secs: u64) -> String {
    let counter = time_secs / TOTP_PERIOD;
    let counter_be = counter.to_be_bytes();
    let hmac_result = hmac_sha256(secret, &counter_be);
    let offset = (hmac_result[31] & 0x0f) as usize;
    let code = ((hmac_result[offset] & 0x7f) as u32) << 24
        | (hmac_result[offset + 1] as u32) << 16
        | (hmac_result[offset + 2] as u32) << 8
        | (hmac_result[offset + 3] as u32);
    let mod_val = 10u32.pow(TOTP_DIGITS as u32);
    let token = code % mod_val;
    format!("{:0width$}", token, width = TOTP_DIGITS as usize)
}

/// HMAC-SHA256 (RFC 2104).
fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 64;
    let mut k = key.to_vec();
    if k.len() > BLOCK_SIZE {
        k = Sha256::digest(&k).to_vec();
    }
    k.resize(BLOCK_SIZE, 0);
    let mut ipad = vec![0x36u8; BLOCK_SIZE];
    let mut opad = vec![0x5cu8; BLOCK_SIZE];
    for i in 0..k.len() {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::digest(&[&ipad[..], msg].concat());
    Sha256::digest(&[&opad[..], &inner[..]].concat()).to_vec()
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// RFC 4648 base32 encoding (no padding).
fn base32_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut result = String::new();
    let mut buffer: u64 = 0;
    let mut bits = 0;
    for &byte in input {
        buffer = (buffer << 8) | byte as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            let idx = ((buffer >> bits) & 0x1f) as usize;
            result.push(ALPHABET[idx] as char);
        }
    }
    if bits > 0 {
        let idx = ((buffer << (5 - bits)) & 0x1f) as usize;
        result.push(ALPHABET[idx] as char);
    }
    result
}

const SESSION_TTL: Duration = Duration::from_secs(3 * 24 * 60 * 60);

pub struct SessionStore {
    tokens: Mutex<HashMap<String, Instant>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            tokens: Mutex::new(HashMap::new()),
        }
    }

    pub fn create_session(&self) -> String {
        // SECURITY (audit C2): session tokens are bearer credentials for full
        // remote control, so they MUST be unguessable. Draw 32 bytes (256 bits)
        // from the OS CSPRNG and hex-encode (64 chars, same format as before).
        // The previous xorshift PRNG could be reverse-engineered from observed
        // tokens to forge new ones, bypassing TOTP entirely.
        let token = generate_session_token();
        self.tokens.lock().insert(token.clone(), Instant::now());
        self.cleanup_expired();
        token
    }

    pub fn validate_token(&self, token: &str) -> bool {
        let mut map = self.tokens.lock();
        if let Some(&created) = map.get(token) {
            if created.elapsed() < SESSION_TTL {
                return true;
            }
            map.remove(token);
        }
        false
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
            .retain(|_, created| created.elapsed() < SESSION_TTL);
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
        // Global limiter first: cheap, and sheds load before per-key work.
        if !self.global_allow() {
            return ThrottleDecision::GlobalLimited;
        }
        let now = Instant::now();
        // The strictest of the IP and device records governs.
        let ip_dec = self.key_decision(&self.by_ip, ip, now);
        let dev_dec = if device_id.is_empty() {
            ThrottleDecision::Allow
        } else {
            self.key_decision(&self.by_device, device_id, now)
        };
        strictest(ip_dec, dev_dec)
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
    fn totp_secret_is_20_bytes_and_varies() {
        let s1 = generate_secret();
        let s2 = generate_secret();
        assert_eq!(s1.len(), 20, "RFC 6238 / our otpauth uses a 160-bit seed");
        assert_ne!(s1, s2, "two CSPRNG draws must differ");
    }

    #[test]
    fn store_create_validate_roundtrip() {
        let store = SessionStore::new();
        let token = store.create_session();
        assert!(store.validate_token(&token));
        store.invalidate(&token);
        assert!(!store.validate_token(&token));
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
}
