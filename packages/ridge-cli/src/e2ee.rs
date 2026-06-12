//! 端到端加密（契约 §7）。
//!
//! relay / TURN 永远看不到明文：在 WebRTC DataChannel 之上再叠一层
//! X25519 + HKDF-SHA256 + ChaCha20-Poly1305(IETF, 96-bit nonce)。本模块与浏览器
//! 侧的 `@noble/*` 实现**字节级一致**（同一 info 串、同一 salt 排序规则、同一
//! nonce 布局）。
//!
//! - 握手（§7.1）：每端发 `0x01 || ephemeral_pub(32)`；
//!   `shared = X25519(my_priv, peer_pub)`；
//!   `salt = sort(my_pub, peer_pub) 字典序拼接`（64B）；
//!   `key = HKDF-SHA256(ikm=shared, salt=salt, info="ridge-e2ee-v1", L=32)`。
//! - 数据帧（§7.2）：`nonce(12) = [dir(1), 0,0,0, counter_u64_le(8)]`，
//!   `dir=0` host→controller，`dir=1` controller→host；线上 `nonce(12) || ct+tag`。
//!
//! cli 永远是 host：发送用 `dir=0`，接收校验对端 `dir=1` 且 counter 严格递增（防重放）。

use anyhow::{anyhow, bail, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

/// 握手首帧标签（§7.1）。任何一端收到非该标签的首帧立即断开。
pub const HANDSHAKE_TAG: u8 = 0x01;
/// HKDF info 串（§7.1）。双方必须完全一致。
const HKDF_INFO: &[u8] = b"ridge-e2ee-v1";
/// X25519 公钥长度。
pub const PUB_KEY_LEN: usize = 32;
/// 派生密钥长度。
const KEY_LEN: usize = 32;
/// IETF nonce 长度。
const NONCE_LEN: usize = 12;

/// nonce 方向字节（§7.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// host→controller
    HostToController = 0,
    /// controller→host
    ControllerToHost = 1,
}

impl Dir {
    /// 当前端的“对端方向”（接收校验用）。cli 是 host，对端是 controller→host=1。
    fn opposite(self) -> Dir {
        match self {
            Dir::HostToController => Dir::ControllerToHost,
            Dir::ControllerToHost => Dir::HostToController,
        }
    }
}

/// 一端的临时 X25519 私钥 + 公钥。握手前生成，发送公钥，收到对端公钥后派生会话。
pub struct Handshake {
    secret: StaticSecret,
    public: PublicKey,
}

impl Handshake {
    /// 生成临时密钥对。
    pub fn new() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// 本端公钥的 32 字节。
    pub fn public_bytes(&self) -> [u8; PUB_KEY_LEN] {
        *self.public.as_bytes()
    }

    /// 编码握手首帧 `0x01 || pub(32)`。
    /// 概念 4-cli 后 host 改发 0x02 设备签名帧（[`encode_signed_frame`]），本方法仅保留供
    /// 测试与 API 对称（解析对端 0x01 仍用 [`Handshake::parse_peer_frame`]）。
    #[allow(dead_code)]
    pub fn encode_frame(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + PUB_KEY_LEN);
        out.push(HANDSHAKE_TAG);
        out.extend_from_slice(&self.public_bytes());
        out
    }

    /// 解析对端握手帧，返回对端公钥。校验标签与长度。
    pub fn parse_peer_frame(frame: &[u8]) -> Result<[u8; PUB_KEY_LEN]> {
        if frame.len() != 1 + PUB_KEY_LEN {
            bail!(
                "handshake frame length {} != {}",
                frame.len(),
                1 + PUB_KEY_LEN
            );
        }
        if frame[0] != HANDSHAKE_TAG {
            bail!(
                "handshake frame tag {:#x} != {:#x}",
                frame[0],
                HANDSHAKE_TAG
            );
        }
        let mut pk = [0u8; PUB_KEY_LEN];
        pk.copy_from_slice(&frame[1..]);
        Ok(pk)
    }

    /// 用对端公钥完成 ECDH + HKDF，得到会话密钥。`my_dir` 是本端发送方向
    /// （cli=host 传 `Dir::HostToController`）。消费 `self`（私钥用后即弃）。
    pub fn into_session(self, peer_pub: [u8; PUB_KEY_LEN], my_dir: Dir) -> Result<Session> {
        let my_pub = self.public_bytes();
        let peer = PublicKey::from(peer_pub);
        let shared = self.secret.diffie_hellman(&peer);

        // salt = sort(my_pub, peer_pub) 字典序拼接（64B），双方一致。
        let mut salt = Vec::with_capacity(2 * PUB_KEY_LEN);
        if my_pub <= peer_pub {
            salt.extend_from_slice(&my_pub);
            salt.extend_from_slice(&peer_pub);
        } else {
            salt.extend_from_slice(&peer_pub);
            salt.extend_from_slice(&my_pub);
        }

        let hk = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
        let mut key = [0u8; KEY_LEN];
        hk.expand(HKDF_INFO, &mut key)
            .map_err(|_| anyhow!("HKDF expand failed (invalid length)"))?;

        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        Ok(Session {
            cipher,
            send_dir: my_dir,
            recv_dir: my_dir.opposite(),
            send_counter: 0,
            recv_counter: 0,
        })
    }
}

impl Default for Handshake {
    fn default() -> Self {
        Self::new()
    }
}

// ── B 层设备身份签名握手帧（0x02）+ 信道绑定 transcript（零信任 #1/#2，设计 §3.1/§2）──
// 与浏览器 `e2ee.ts` 字节级对齐。host(ridge-cli) 发送侧：用 `DeviceIdentity` 对
// `build_id_bind_context` 结果签名（签名方加 `ID_BIND_DOMAIN` 前缀，见 ridge-core
// device_identity），再 `encode_signed_frame` 组装 0x02 帧。tag/串常量与
// `ridge-signaling` 注册表一致。

/// 设备身份签名握手帧 tag（= `ridge-signaling::tags::handshake::DEVICE_BOUND`）。
pub const DEVICE_BOUND_TAG: u8 = 0x02;
/// Ed25519 设备身份公钥长度。
pub const ID_PUB_KEY_LEN: usize = 32;
/// Ed25519 签名长度。
pub const SIGNATURE_LEN: usize = 64;
/// 0x02 帧总长：1 + 32 + 32 + 64 = 129。
pub const SIGNED_HANDSHAKE_LEN: usize = 1 + PUB_KEY_LEN + ID_PUB_KEY_LEN + SIGNATURE_LEN;
/// 设备身份签名域分隔串（与 `e2ee.ts::ID_BIND_DOMAIN` / src-tauri `sign_device_identity` 一致）。
pub const ID_BIND_DOMAIN: &[u8] = b"ridge-id-bind-v1";
/// 信道绑定 transcript 域分隔串（与 `e2ee.ts::BIND_TRANSCRIPT_DOMAIN` 一致）。
pub const BIND_TRANSCRIPT_DOMAIN: &[u8] = b"ridge-e2ee-bind-v1";

/// 组装 0x02 帧：`0x02 || eph_pub(32) || id_pub(32) || sig(64)`。
pub fn encode_signed_frame(
    eph_pub: &[u8; PUB_KEY_LEN],
    id_pub: &[u8; ID_PUB_KEY_LEN],
    sig: &[u8; SIGNATURE_LEN],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(SIGNED_HANDSHAKE_LEN);
    out.push(DEVICE_BOUND_TAG);
    out.extend_from_slice(eph_pub);
    out.extend_from_slice(id_pub);
    out.extend_from_slice(sig);
    out
}

/// 解析 0x02 帧 → `(eph_pub, id_pub, sig)`。tag/长度非法报错。
/// （host=ridge-cli 通常**发送** 0x02、不解析；保留供测试与对称完整性。）
#[allow(dead_code)]
pub fn parse_signed_frame(
    frame: &[u8],
) -> Result<([u8; PUB_KEY_LEN], [u8; ID_PUB_KEY_LEN], [u8; SIGNATURE_LEN])> {
    if frame.len() != SIGNED_HANDSHAKE_LEN {
        bail!(
            "signed handshake frame length {} != {}",
            frame.len(),
            SIGNED_HANDSHAKE_LEN
        );
    }
    if frame[0] != DEVICE_BOUND_TAG {
        bail!(
            "signed handshake tag {:#x} != {:#x}",
            frame[0],
            DEVICE_BOUND_TAG
        );
    }
    let mut eph = [0u8; PUB_KEY_LEN];
    let mut id = [0u8; ID_PUB_KEY_LEN];
    let mut sig = [0u8; SIGNATURE_LEN];
    eph.copy_from_slice(&frame[1..1 + PUB_KEY_LEN]);
    id.copy_from_slice(&frame[1 + PUB_KEY_LEN..1 + PUB_KEY_LEN + ID_PUB_KEY_LEN]);
    sig.copy_from_slice(&frame[1 + PUB_KEY_LEN + ID_PUB_KEY_LEN..]);
    Ok((eph, id, sig))
}

/// 以 1 字节长度前缀编码变长字节段（长度必须 ≤ 255）。
fn push_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    if bytes.len() > 255 {
        bail!("id-bind 变长字段超过 255 字节");
    }
    out.push(bytes.len() as u8);
    out.extend_from_slice(bytes);
    Ok(())
}

/// 构造设备身份签名 **context**（**不含**域分隔前缀；签名方加前缀）：
///   `context = host_eph(32) || ctrl_eph(32) || u8(len)||device || u8(len)||username`
/// 与 `e2ee.ts::buildIdBindContext` 字节对齐（变长字段 1B 长度前缀防拼接歧义）。
pub fn build_id_bind_context(
    host_eph: &[u8; PUB_KEY_LEN],
    ctrl_eph: &[u8; PUB_KEY_LEN],
    device_name: &str,
    username: &str,
) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(PUB_KEY_LEN * 2 + device_name.len() + username.len() + 2);
    out.extend_from_slice(host_eph);
    out.extend_from_slice(ctrl_eph);
    push_len_prefixed(&mut out, device_name.as_bytes())?;
    push_len_prefixed(&mut out, username.as_bytes())?;
    Ok(out)
}

/// 构造信道绑定 transcript：`BIND_TRANSCRIPT_DOMAIN || sorted(host_eph, ctrl_eph)`。
/// 与 `e2ee.ts::buildBindTranscript` 字节对齐（字典序排序保证两端独立计算一致）。
pub fn build_bind_transcript(
    host_eph: &[u8; PUB_KEY_LEN],
    ctrl_eph: &[u8; PUB_KEY_LEN],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(BIND_TRANSCRIPT_DOMAIN.len() + PUB_KEY_LEN * 2);
    out.extend_from_slice(BIND_TRANSCRIPT_DOMAIN);
    let (first, second) = if host_eph <= ctrl_eph {
        (host_eph, ctrl_eph)
    } else {
        (ctrl_eph, host_eph)
    };
    out.extend_from_slice(first);
    out.extend_from_slice(second);
    out
}

/// 派生出会话密钥后的对称加密上下文。方向分离 nonce + 单调 counter（防重放）。
pub struct Session {
    cipher: ChaCha20Poly1305,
    send_dir: Dir,
    recv_dir: Dir,
    send_counter: u64,
    recv_counter: u64,
}

/// counter 上限：u64::MAX 视为耗尽，接近时调用方须重建连接（§7.2 严禁回绕）。
const COUNTER_EXHAUSTED: u64 = u64::MAX;

fn build_nonce(dir: Dir, counter: u64) -> [u8; NONCE_LEN] {
    let mut n = [0u8; NONCE_LEN];
    n[0] = dir as u8;
    // n[1..4] 保持 0
    n[4..12].copy_from_slice(&counter.to_le_bytes());
    n
}

impl Session {
    /// 封包：返回 `nonce(12) || ciphertext+tag`。每次发送 counter 自增。
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        if self.send_counter == COUNTER_EXHAUSTED {
            bail!("send counter exhausted — reconnect required (no nonce reuse)");
        }
        let counter = self.send_counter;
        let nonce_bytes = build_nonce(self.send_dir, counter);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ct = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: &[],
                },
            )
            .map_err(|_| anyhow!("AEAD seal failed"))?;
        self.send_counter += 1;
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// 拆包：校验 nonce 的 `dir` == 对端方向，且 counter 严格递增（防重放）。
    /// 成功返回明文并把 `recv_counter` 推进到 `nonce.counter + 1`。
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>> {
        if frame.len() < NONCE_LEN {
            bail!("frame too short: {} < {}", frame.len(), NONCE_LEN);
        }
        let (nonce_bytes, ct) = frame.split_at(NONCE_LEN);

        // dir 校验：必须是对端方向（cli=host 时对端=controller→host=1）。
        let dir_byte = nonce_bytes[0];
        if dir_byte != self.recv_dir as u8 {
            bail!(
                "nonce dir {} != expected peer dir {}",
                dir_byte,
                self.recv_dir as u8
            );
        }
        // 中间 3 字节必须为 0（防 nonce 走私额外熵）。
        if nonce_bytes[1] != 0 || nonce_bytes[2] != 0 || nonce_bytes[3] != 0 {
            bail!("nonce reserved bytes must be zero");
        }
        let mut ctr = [0u8; 8];
        ctr.copy_from_slice(&nonce_bytes[4..12]);
        let counter = u64::from_le_bytes(ctr);

        // 重放/乱序拒绝：counter 必须 >= 期望值（严格递增、不接受历史值）。
        if counter < self.recv_counter {
            bail!(
                "replay/out-of-order: counter {} < expected {}",
                counter,
                self.recv_counter
            );
        }

        let nonce = Nonce::from_slice(nonce_bytes);
        let pt = self
            .cipher
            .decrypt(nonce, Payload { msg: ct, aad: &[] })
            .map_err(|_| anyhow!("AEAD open failed (auth tag mismatch)"))?;

        // 仅在解密成功后推进，避免伪造帧污染计数器。
        self.recv_counter = counter
            .checked_add(1)
            .ok_or_else(|| anyhow!("recv counter overflow — reconnect required"))?;
        Ok(pt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建立一对 host/controller 会话（模拟双向握手）。
    fn pair() -> (Session, Session) {
        let host_hs = Handshake::new();
        let ctrl_hs = Handshake::new();
        let host_pub = host_hs.public_bytes();
        let ctrl_pub = ctrl_hs.public_bytes();

        let host = host_hs
            .into_session(ctrl_pub, Dir::HostToController)
            .unwrap();
        let ctrl = ctrl_hs
            .into_session(host_pub, Dir::ControllerToHost)
            .unwrap();
        (host, ctrl)
    }

    #[test]
    fn handshake_frame_roundtrip() {
        let hs = Handshake::new();
        let frame = hs.encode_frame();
        assert_eq!(frame.len(), 1 + PUB_KEY_LEN);
        assert_eq!(frame[0], HANDSHAKE_TAG);
        let parsed = Handshake::parse_peer_frame(&frame).unwrap();
        assert_eq!(parsed, hs.public_bytes());
    }

    #[test]
    fn parse_rejects_bad_tag_and_length() {
        let mut bad = vec![0x02u8];
        bad.extend_from_slice(&[0u8; PUB_KEY_LEN]);
        assert!(Handshake::parse_peer_frame(&bad).is_err());
        assert!(Handshake::parse_peer_frame(&[HANDSHAKE_TAG, 1, 2]).is_err());
    }

    #[test]
    fn both_sides_derive_same_key() {
        // 同一明文双向加解密成功即证明派生密钥一致。
        let (mut host, mut ctrl) = pair();
        let sealed = host.seal(b"hello from host").unwrap();
        let opened = ctrl.open(&sealed).unwrap();
        assert_eq!(opened, b"hello from host");

        let sealed2 = ctrl.seal(b"hi from controller").unwrap();
        let opened2 = host.open(&sealed2).unwrap();
        assert_eq!(opened2, b"hi from controller");
    }

    #[test]
    fn salt_sorting_is_order_independent() {
        // 不论谁先生成，salt 排序一致 → 双方派生同一 key。上面的双向测试已覆盖；
        // 这里额外断言 nonce 方向字节确实落在密文头。
        let (mut host, _ctrl) = pair();
        let sealed = host.seal(b"x").unwrap();
        assert_eq!(sealed[0], Dir::HostToController as u8);
        assert_eq!(&sealed[1..4], &[0, 0, 0]);
    }

    #[test]
    fn counter_increments_per_send() {
        let (mut host, mut ctrl) = pair();
        for i in 0..5u64 {
            let sealed = host.seal(format!("msg{i}").as_bytes()).unwrap();
            // counter 编码在 nonce[4..12]。
            let mut ctr = [0u8; 8];
            ctr.copy_from_slice(&sealed[4..12]);
            assert_eq!(u64::from_le_bytes(ctr), i);
            let opened = ctrl.open(&sealed).unwrap();
            assert_eq!(opened, format!("msg{i}").as_bytes());
        }
    }

    #[test]
    fn replay_is_rejected() {
        let (mut host, mut ctrl) = pair();
        let f0 = host.seal(b"first").unwrap();
        let f1 = host.seal(b"second").unwrap();
        // 正常顺序消费。
        assert_eq!(ctrl.open(&f0).unwrap(), b"first");
        assert_eq!(ctrl.open(&f1).unwrap(), b"second");
        // 重放 f0（counter=0 < 期望=2）→ 拒绝。
        let err = ctrl.open(&f0).unwrap_err();
        assert!(err.to_string().contains("replay"), "got: {err}");
        // 重放 f1（counter=1 < 期望=2）→ 拒绝。
        assert!(ctrl.open(&f1).is_err());
    }

    #[test]
    fn wrong_direction_is_rejected() {
        // host 加密的帧 dir=0，再喂回 host.open（host 期望对端 dir=1）→ 拒绝。
        let (mut host, _ctrl) = pair();
        let sealed = host.seal(b"loopback").unwrap();
        let err = host.open(&sealed).unwrap_err();
        assert!(err.to_string().contains("dir"), "got: {err}");
    }

    #[test]
    fn tampered_ciphertext_fails_auth() {
        let (mut host, mut ctrl) = pair();
        let mut sealed = host.seal(b"authentic").unwrap();
        // 翻转密文最后一字节（tag 区）。
        let last = sealed.len() - 1;
        sealed[last] ^= 0xff;
        assert!(ctrl.open(&sealed).is_err());
    }

    #[test]
    fn nonzero_reserved_bytes_rejected() {
        let (mut host, mut ctrl) = pair();
        let mut sealed = host.seal(b"data").unwrap();
        sealed[1] = 0x01; // 篡改保留字节
        assert!(ctrl.open(&sealed).is_err());
    }

    #[test]
    fn out_of_order_forward_jump_is_accepted() {
        // 若中间帧丢失，counter 跳跃前进（>= 期望）应被接受并重置期望值。
        let (mut host, mut ctrl) = pair();
        let _f0 = host.seal(b"a").unwrap();
        let f1 = host.seal(b"b").unwrap(); // counter=1
                                           // 直接消费 f1（跳过 f0）。
        assert_eq!(ctrl.open(&f1).unwrap(), b"b");
        // 之后 f2 (counter=2) 仍可消费。
        let f2 = host.seal(b"c").unwrap();
        assert_eq!(ctrl.open(&f2).unwrap(), b"c");
    }

    // ── B 层 0x02 设备签名握手帧 + 信道绑定 transcript（零信任 #1/#2）──────────────

    #[test]
    fn signed_frame_roundtrip() {
        let eph = [0x11u8; PUB_KEY_LEN];
        let id = [0x22u8; ID_PUB_KEY_LEN];
        let sig = [0x33u8; SIGNATURE_LEN];
        let frame = encode_signed_frame(&eph, &id, &sig);
        assert_eq!(frame.len(), SIGNED_HANDSHAKE_LEN);
        assert_eq!(frame.len(), 129);
        assert_eq!(frame[0], DEVICE_BOUND_TAG);
        let (e, i, s) = parse_signed_frame(&frame).unwrap();
        assert_eq!(e, eph);
        assert_eq!(i, id);
        assert_eq!(s, sig);
    }

    #[test]
    fn parse_signed_frame_rejects_bad_tag_and_len() {
        let frame = encode_signed_frame(&[1u8; 32], &[2u8; 32], &[3u8; 64]);
        let mut bad = frame.clone();
        bad[0] = 0x01;
        assert!(parse_signed_frame(&bad).is_err());
        assert!(parse_signed_frame(&frame[..128]).is_err());
    }

    #[test]
    fn id_bind_context_golden_matches_browser() {
        // 跨实现 conformance：与 e2ee.test.ts 'id-bind context golden' 同输入同 hex
        // （host_pub=0x11*32, ctrl_pub=0x22*32, device="dev", username="alice"）。
        let ctx = build_id_bind_context(&[0x11u8; 32], &[0x22u8; 32], "dev", "alice").unwrap();
        let hex: String = ctx.iter().map(|b| format!("{:02x}", b)).collect();
        assert_eq!(
            hex,
            concat!(
                "1111111111111111111111111111111111111111111111111111111111111111",
                "2222222222222222222222222222222222222222222222222222222222222222",
                "0364657605616c696365"
            ),
            "Rust id-bind context 必须与浏览器 e2ee.ts buildIdBindContext 字节对齐"
        );
    }

    #[test]
    fn id_bind_context_length_prefix_disambiguates() {
        let h = [1u8; 32];
        let c = [2u8; 32];
        // ("ab","c") 与 ("a","bc") 无长度前缀会拼成同一串；这里必须不同。
        assert_ne!(
            build_id_bind_context(&h, &c, "ab", "c").unwrap(),
            build_id_bind_context(&h, &c, "a", "bc").unwrap(),
        );
    }

    #[test]
    fn bind_transcript_sort_order_independent() {
        let h = [5u8; 32];
        let c = [9u8; 32];
        assert_eq!(build_bind_transcript(&h, &c), build_bind_transcript(&c, &h));
        assert!(build_bind_transcript(&h, &c).starts_with(BIND_TRANSCRIPT_DOMAIN));
    }
}
