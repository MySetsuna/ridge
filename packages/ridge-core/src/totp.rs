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

    /// C 层 TOTP 信道绑定校验（零信任 #1）：对端用当前 6 位码在 `transcript` 上算
    /// `tag = HMAC(HKDF(code, transcript), transcript)` 发来；本端用本机种子在 **±1
    /// 时间步**窗口各算一遍比对（恒定时间）。与浏览器 `e2ee.ts::computeBindTag` 字节对齐。
    /// `transcript` 由 e2ee 层构造（domain‖sorted(双方临时公钥)，见 e2ee.* build_bind_transcript）。
    pub fn verify_bind_tag(&self, transcript: &[u8], tag: &[u8]) -> bool {
        if tag.len() != 32 {
            return false;
        }
        let now = now_secs();
        for step in -TOTP_SKEW..=TOTP_SKEW {
            let ts = if step >= 0 {
                now.saturating_add((step as u64) * TOTP_PERIOD)
            } else {
                now.saturating_sub(((-step) as u64) * TOTP_PERIOD)
            };
            let code = totp_at(&self.secret, ts);
            let expected = bind_tag_at(&code, transcript);
            if constant_time_eq(&expected, tag) {
                return true;
            }
        }
        false
    }

    /// 本端用**当前**时间步的码对 `transcript` 算绑定 tag（controller 侧/对称测试用）。
    pub fn current_bind_tag(&self, transcript: &[u8]) -> [u8; 32] {
        bind_tag_at(&self.current_code(), transcript)
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

    /// 返回当前 TOTP 归属身份（`"default"` 或云账号 username）。
    /// 供 `grant_store` 按身份隔离授权记录使用。
    pub fn identity(&self) -> String {
        self.identity.clone()
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

/// C 层 TOTP 信道绑定 HKDF info（与 `e2ee.ts::BIND_HKDF_INFO` 一致）。
const BIND_HKDF_INFO: &[u8] = b"ridge-bind";

/// RFC 5869 HKDF-SHA256，单块 expand（L=32 = HashLen，一块足够）。复用手写 `hmac_sha256`。
fn hkdf_sha256_32(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    // extract：PRK = HMAC-SHA256(salt, ikm)。
    let prk = hmac_sha256(salt, ikm);
    // expand：T(1) = HMAC-SHA256(PRK, info || 0x01)，取前 32 字节。
    let mut block_input = info.to_vec();
    block_input.push(0x01);
    let okm = hmac_sha256(&prk, &block_input);
    let mut out = [0u8; 32];
    out.copy_from_slice(&okm[..32]);
    out
}

/// 用给定 code 对 transcript 算信道绑定 tag（32B）：
///   K   = HKDF-SHA256(ikm=code_ascii, salt=transcript, info="ridge-bind", L=32)
///   tag = HMAC-SHA256(K, transcript)
/// 与 `e2ee.ts::computeBindTag` 逐字节对齐（HKDF/HMAC 均 RFC 标准 + 相同输入）。
fn bind_tag_at(code: &str, transcript: &[u8]) -> [u8; 32] {
    let k = hkdf_sha256_32(code.as_bytes(), transcript, BIND_HKDF_INFO);
    let mac = hmac_sha256(&k, transcript);
    let mut out = [0u8; 32];
    out.copy_from_slice(&mac[..32]);
    out
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

    // ── C 层 TOTP 信道绑定（零信任 #1）─────────────────────────────────────────

    /// 与 e2ee.ts buildBindTranscript 一致：`domain || sorted(host_pub, ctrl_pub)`。
    fn bind_transcript(host_pub: &[u8; 32], ctrl_pub: &[u8; 32]) -> Vec<u8> {
        let mut t = b"ridge-e2ee-bind-v1".to_vec();
        let (first, second) = if host_pub <= ctrl_pub {
            (host_pub, ctrl_pub)
        } else {
            (ctrl_pub, host_pub)
        };
        t.extend_from_slice(first);
        t.extend_from_slice(second);
        t
    }

    #[test]
    fn bind_tag_golden_matches_browser_e2ee() {
        // 跨实现 conformance：与 e2ee.test.ts 'golden' 同一输入必产出同一 tag
        // （host_pub=0x11*32, ctrl_pub=0x22*32, code="123456"）。任一端改算法即红。
        let transcript = bind_transcript(&[0x11u8; 32], &[0x22u8; 32]);
        let tag = bind_tag_at("123456", &transcript);
        let hex: String = tag.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            "d694a5285b3e8eaff2a0e53216ac003f6e79fbab207fbaf4db605efa6ffdaa64",
            "Rust 绑定 tag 必须与浏览器 e2ee.ts computeBindTag 字节对齐"
        );
    }

    #[test]
    fn verify_bind_tag_accepts_own_current_tag() {
        let totp = RemoteTotp::new();
        let transcript = bind_transcript(&[1u8; 32], &[2u8; 32]);
        let tag = totp.current_bind_tag(&transcript);
        assert!(
            totp.verify_bind_tag(&transcript, &tag),
            "本机当前码算的 tag 须能自洽校验"
        );
        // 长度非法直接拒。
        assert!(!totp.verify_bind_tag(&transcript, &[0u8; 16]));
    }

    #[test]
    fn verify_bind_tag_accepts_previous_step_within_skew() {
        let totp = RemoteTotp::new();
        let transcript = bind_transcript(&[3u8; 32], &[4u8; 32]);
        let now = now_secs();
        let prev_code = totp_at(&totp.secret, now.saturating_sub(TOTP_PERIOD));
        let tag = bind_tag_at(&prev_code, &transcript);
        assert!(
            totp.verify_bind_tag(&transcript, &tag),
            "±1 窗口内上一步码的 tag 须接受"
        );
    }

    #[test]
    fn verify_bind_tag_rejects_different_transcript() {
        let totp = RemoteTotp::new();
        let t1 = bind_transcript(&[1u8; 32], &[2u8; 32]);
        let t2 = bind_transcript(&[9u8; 32], &[2u8; 32]); // MITM 换 host 公钥
        let tag = totp.current_bind_tag(&t1);
        assert!(
            !totp.verify_bind_tag(&t2, &tag),
            "transcript 不同（换公钥）→ 拒绝"
        );
    }
}
