use std::time::{SystemTime, UNIX_EPOCH};

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

    /// Return an `otpauth://` URI suitable for QR-code generation.
    /// Uses RFC 4648 base32 (no padding) for the secret.
    pub fn otpauth_uri(&self) -> String {
        format!(
            "otpauth://totp/Ridge:remote?secret={}&issuer=Ridge&algorithm=SHA256&digits={}&period={}",
            base32_encode(&self.secret),
            TOTP_DIGITS,
            TOTP_PERIOD,
        )
    }
}

fn generate_secret() -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let mut rng = SimpleRng::new(seed as u64 ^ pid as u64);
    let mut buf = vec![0u8; 20];
    rng.fill(&mut buf);
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

/// Minimal non-cryptographic PRNG for secret generation.
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(0x9e3779b97f4a7c15),
        }
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x.wrapping_shl(13);
        x ^= x.wrapping_shr(7);
        x ^= x.wrapping_shl(17);
        self.state = x;
        x.wrapping_mul(0x9e3779b97f4a7c15)
    }
    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            let val = self.next_u64().to_le_bytes();
            for (d, s) in chunk.iter_mut().zip(val.iter()) {
                *d = *s;
            }
        }
    }
}
