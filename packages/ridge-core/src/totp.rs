//! 云/局域网远控二次验证：RFC 6238 TOTP（契约 §4）的**唯一权威实现**。
//!
//! 桌面（`src-tauri`）、无头 cli（`rdg`）、未来的 LAN host 三处共用本模块，确保对
//! 同一标准互通——不再各写一份（历史上 cli 与 desktop 各有近乎重复的实现，且
//! desktop 用弱 PRNG 生成 secret；此处统一为 `rand::OsRng`）：
//! - HMAC-**SHA256**（非默认 SHA1；otpauth URI 显式标 `algorithm=SHA256`）。
//! - 30s 时间步、6 位数字、验证 ±1 窗口（容忍时钟漂移）。
//! - 每进程随机生成 20 字节 secret（**绝不上线**：只在本机校验，URI 仅供展示）。
//!
//! 用法（§4）：会话启动时 `RemoteTotp::new()`，把 `current_code()` + `otpauth_uri()`
//! 展示给用户；E2EE 控制通道上对业务帧门控，收到 `{"t":"totp-verify","code":"…"}`
//! 时 `verify()`，回 `{"t":"totp-result","ok":…}`。

use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// 时间步长（秒）。
const TOTP_PERIOD: u64 = 30;
/// 验证码位数。
const TOTP_DIGITS: u32 = 6;
/// 验证窗口：当前步 ±SKEW 都接受（容忍时钟漂移）。
const TOTP_SKEW: i64 = 1;
/// secret 字节数（RFC 6238 推荐 ≥ 输出 HMAC 长度即可，取 20）。
const SECRET_LEN: usize = 20;

/// 一份 TOTP 上下文。`secret` 可由 `load_or_create` 从磁盘恢复（跨重启稳定），
/// `identity` 记住其归属身份（`"default"` 或云账号 username），供 reset/switch 用。
pub struct RemoteTotp {
    secret: Vec<u8>,
    identity: String,
}

impl RemoteTotp {
    /// 生成随机 secret 的新实例（OS 熵源），**不落盘**。供单测与不关心持久化处。
    pub fn new() -> Self {
        Self {
            secret: generate_secret(),
            identity: String::new(),
        }
    }

    /// 当前时间步对应的 6 位 TOTP。
    pub fn current_code(&self) -> String {
        totp_at(&self.secret, now_secs())
    }

    /// 校验用户输入的 code，检查当前 **±1 时间步**窗口（契约 §4「±1 窗口」）。
    /// 非 6 位直接拒绝；常量时间比较避免计时侧信道。
    pub fn verify(&self, code: &str) -> bool {
        if code.len() != TOTP_DIGITS as usize {
            return false;
        }
        let now = now_secs();
        for step in -TOTP_SKEW..=TOTP_SKEW {
            let ts = if step >= 0 {
                now.saturating_add((step as u64) * TOTP_PERIOD)
            } else {
                now.saturating_sub(((-step) as u64) * TOTP_PERIOD)
            };
            if constant_time_eq(totp_at(&self.secret, ts).as_bytes(), code.as_bytes()) {
                return true;
            }
        }
        false
    }

    /// 供 TUI 展示 / 二维码生成的 `otpauth://` URI。RFC 4648 base32（无填充）编码
    /// secret，并显式声明 `algorithm=SHA256`（默认是 SHA1，必须标注以对齐）。
    pub fn otpauth_uri(&self, label: &str) -> String {
        let label = label.replace(' ', "%20");
        format!(
            "otpauth://totp/Ridge:{label}?secret={}&issuer=Ridge&algorithm=SHA256&digits={}&period={}",
            base32_encode(&self.secret),
            TOTP_DIGITS,
            TOTP_PERIOD,
        )
    }

    /// 时间步长（秒），供展示「每 {period}s 刷新」提示。
    pub const fn period_secs() -> u64 {
        TOTP_PERIOD
    }

    /// 按身份加载持久化 secret；无则生成并落盘（跨重启稳定的入口）。
    pub fn load_or_create(identity: &str) -> Self {
        let secret = crate::seed_store::load(identity).unwrap_or_else(|| {
            let s = generate_secret();
            crate::seed_store::save(identity, &s);
            s
        });
        Self {
            secret,
            identity: identity.to_string(),
        }
    }

    /// 仅替换内存 secret（不落盘）——拆出来便于无磁盘单测。
    fn regenerate(&mut self) {
        self.secret = generate_secret();
    }

    /// 重置当前身份的 secret：新生成 + 覆盖落盘（旧验证器即失效）。
    pub fn reset(&mut self) {
        self.regenerate();
        crate::seed_store::save(&self.identity, &self.secret);
    }

    /// 切换到另一身份的 secret（无则现生成并落盘）。
    pub fn switch_identity(&mut self, identity: &str) {
        let next = Self::load_or_create(identity);
        self.secret = next.secret;
        self.identity = next.identity;
    }
}

impl Default for RemoteTotp {
    fn default() -> Self {
        Self::new()
    }
}

/// 随机 20 字节 secret（OS 熵源）。
fn generate_secret() -> Vec<u8> {
    use rand::RngCore;
    let mut buf = vec![0u8; SECRET_LEN];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    buf
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 给定 secret + 绝对时间（秒）算出该时间步的 TOTP（RFC 6238 截断）。
fn totp_at(secret: &[u8], time_secs: u64) -> String {
    let counter = time_secs / TOTP_PERIOD;
    let counter_be = counter.to_be_bytes();
    let mac = hmac_sha256(secret, &counter_be);
    // 动态截断（RFC 4226 §5.3）：取末字节低 4 位作偏移，截 4 字节。
    let offset = (mac[mac.len() - 1] & 0x0f) as usize;
    let code = ((mac[offset] & 0x7f) as u32) << 24
        | (mac[offset + 1] as u32) << 16
        | (mac[offset + 2] as u32) << 8
        | (mac[offset + 3] as u32);
    let modulo = 10u32.pow(TOTP_DIGITS);
    format!("{:0width$}", code % modulo, width = TOTP_DIGITS as usize)
}

/// HMAC-SHA256（RFC 2104）。手写以避免引额外 crate。
fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 64;
    let mut k = key.to_vec();
    if k.len() > BLOCK_SIZE {
        k = Sha256::digest(&k).to_vec();
    }
    k.resize(BLOCK_SIZE, 0);
    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..k.len() {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::digest([&ipad[..], msg].concat());
    Sha256::digest([&opad[..], &inner[..]].concat()).to_vec()
}

/// 常量时间比较，防计时侧信道泄露匹配进度。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// RFC 4648 base32 编码（无填充）。仅用于 otpauth URI 展示。
fn base32_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut buffer: u64 = 0;
    let mut bits = 0;
    for &byte in input {
        buffer = (buffer << 8) | byte as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 已知向量：RFC 6238 附录 B 的 SHA256 行（secret = ASCII
    /// "12345678901234567890123456789012"，T = 59s → 8 位 code 46119246）。
    /// 本实现取 6 位，即末 6 位 `119246`。锁死算法/截断/位数与标准一致，
    /// 桌面端与浏览器 controller 才能对得上。
    #[test]
    fn rfc6238_known_vector_sha256() {
        let secret = b"12345678901234567890123456789012";
        assert_eq!(totp_at(secret, 59), "119246");
    }

    #[test]
    fn current_code_verifies() {
        let totp = RemoteTotp::new();
        let code = totp.current_code();
        assert!(totp.verify(&code), "freshly generated code must verify");
    }

    #[test]
    fn wrong_code_rejected() {
        let totp = RemoteTotp::new();
        assert!(!totp.verify("12"), "non-6-digit rejected");
        assert!(!totp.verify("abcdef"), "non-numeric mismatch rejected");
    }

    #[test]
    fn previous_step_within_skew_verifies() {
        let totp = RemoteTotp::new();
        let now = now_secs();
        let prev_code = totp_at(&totp.secret, now.saturating_sub(TOTP_PERIOD));
        assert!(
            totp.verify(&prev_code),
            "previous-step code within ±1 must verify"
        );
    }

    #[test]
    fn far_step_rejected() {
        let totp = RemoteTotp::new();
        let now = now_secs();
        let far = totp_at(&totp.secret, now.saturating_add(TOTP_PERIOD * 3));
        assert!(!totp.verify(&far) || far == totp.current_code());
    }

    #[test]
    fn otpauth_uri_shape() {
        let totp = RemoteTotp::new();
        let uri = totp.otpauth_uri("my host");
        assert!(uri.starts_with("otpauth://totp/Ridge:my%20host?"));
        assert!(uri.contains("algorithm=SHA256"));
        assert!(uri.contains("digits=6"));
        assert!(uri.contains("period=30"));
        assert!(uri.contains("secret="));
    }

    #[test]
    fn base32_known_vector() {
        // RFC 4648 §10：base32("foobar") = "MZXW6YTBOI"（无填充）。
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn regenerate_changes_secret_and_self_verifies() {
        let mut totp = RemoteTotp::new();
        let before = totp.secret.clone();
        totp.regenerate();
        assert_ne!(totp.secret, before, "regenerate 必须换掉 secret");
        let code = totp.current_code();
        assert!(totp.verify(&code), "regenerate 后的码须能自洽校验");
    }
}
