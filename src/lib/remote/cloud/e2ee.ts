// Ridge Cloud — 端到端加密（E2EE），严格按契约 §7 实现。
//
// 算法套件（与 Rust 侧字节级一致）：
//   - 密钥协商：X25519（@noble/curves）
//   - 派生：HKDF-SHA256（@noble/hashes），info = "ridge-e2ee-v1"，L = 32
//   - 对称加密：ChaCha20-Poly1305（IETF，96-bit nonce，@noble/ciphers）
//
// WebCrypto 没有 ChaCha20，必须走 noble；X25519 也统一走 noble 以与 Rust
// （x25519-dalek + hkdf + sha2 + chacha20poly1305）保持字节级一致。

import { x25519, ed25519 } from '@noble/curves/ed25519.js';
import { hkdf } from '@noble/hashes/hkdf.js';
import { sha256 } from '@noble/hashes/sha2.js';
import { hmac } from '@noble/hashes/hmac.js';
import { chacha20poly1305 } from '@noble/ciphers/chacha.js';

/** 握手首帧 tag（契约 §7.1）：0x01 || ephemeral_pub(32B)。 */
export const HANDSHAKE_TAG = 0x01;
/** X25519 公钥长度。 */
export const PUBKEY_LEN = 32;
/** HKDF info（双方必须一致）。 */
export const HKDF_INFO = 'ridge-e2ee-v1';
/** 派生对称密钥长度。 */
export const KEY_LEN = 32;
/** ChaCha20-Poly1305 nonce 长度（IETF 96-bit）。 */
export const NONCE_LEN = 12;

/** 方向字节（契约 §7.2）。0 = host→controller，1 = controller→host。 */
export const DIR_HOST_TO_CONTROLLER = 0;
export const DIR_CONTROLLER_TO_HOST = 1;
export type Direction = typeof DIR_HOST_TO_CONTROLLER | typeof DIR_CONTROLLER_TO_HOST;

/** counter 接近 u64 上限时必须重建连接（防回绕）。预留安全余量。 */
const COUNTER_MAX = (1n << 64n) - 1n;

/** 本端临时密钥对。 */
export interface EphemeralKeyPair {
  readonly privateKey: Uint8Array;
  readonly publicKey: Uint8Array;
}

/** 生成临时 X25519 密钥对（契约 §7.1）。 */
export function generateEphemeralKeyPair(): EphemeralKeyPair {
  const privateKey = x25519.utils.randomSecretKey();
  const publicKey = x25519.getPublicKey(privateKey);
  return { privateKey, publicKey };
}

/** 构造握手首帧：0x01 || ephemeral_pub(32B)。 */
export function encodeHandshakeFrame(publicKey: Uint8Array): Uint8Array {
  if (publicKey.length !== PUBKEY_LEN) {
    throw new Error(`E2EE: 公钥长度必须为 ${PUBKEY_LEN}，实际 ${publicKey.length}`);
  }
  const out = new Uint8Array(1 + PUBKEY_LEN);
  out[0] = HANDSHAKE_TAG;
  out.set(publicKey, 1);
  return out;
}

/** 解析对端握手首帧，返回对端公钥；非握手帧抛错（调用方据此断开）。 */
export function decodeHandshakeFrame(frame: Uint8Array): Uint8Array {
  if (frame.length !== 1 + PUBKEY_LEN || frame[0] !== HANDSHAKE_TAG) {
    throw new Error('E2EE: 收到非法握手帧（首帧必须为 0x01 || pub32）');
  }
  return frame.slice(1, 1 + PUBKEY_LEN);
}

// ── B 层设备身份签名握手帧（0x02，零信任方案 #2 / 设计 §3.1, §7.1）────────────────
// tag/尺寸与 `ridge-signaling::tags::handshake`（DEVICE_BOUND=0x02）对齐。
// 帧布局：`0x02 || eph_pub(32) || id_pub(32) || sig(64)` = 129B。
// sig = Ed25519(device_id_priv, ID_BIND_DOMAIN || context)，context 见 buildIdBindContext。
// 用途：host→controller 方向，host 用长期设备身份私钥签本次临时公钥，controller 验签 +
// TOFU 固定 id_pub 指纹 → 被攻陷 relay 无设备私钥无法替换公钥做 MITM。

/** 设备身份签名握手帧 tag（= `ridge-signaling::tags::handshake::DEVICE_BOUND`）。 */
export const DEVICE_BOUND_TAG = 0x02;
/** Ed25519 设备身份公钥长度。 */
export const ID_PUBKEY_LEN = 32;
/** Ed25519 签名长度。 */
export const SIGNATURE_LEN = 64;
/** 0x02 帧总长：1 + 32 + 32 + 64 = 129。 */
export const SIGNED_HANDSHAKE_LEN = 1 + PUBKEY_LEN + ID_PUBKEY_LEN + SIGNATURE_LEN;
/** 签名域分隔串（与 Rust `src-tauri::sign_device_identity` / ridge-cli `e2ee.rs` 逐字一致）。 */
export const ID_BIND_DOMAIN = 'ridge-id-bind-v1';

/** 解析后的设备身份签名握手帧。 */
export interface SignedHandshake {
  /** 对端临时 X25519 公钥。 */
  ephPub: Uint8Array;
  /** 对端长期 Ed25519 设备身份公钥（待 TOFU 固定）。 */
  idPub: Uint8Array;
  /** Ed25519 签名（覆盖 `ID_BIND_DOMAIN || context`）。 */
  sig: Uint8Array;
}

/** 构造 0x02 帧：`0x02 || eph_pub(32) || id_pub(32) || sig(64)`。 */
export function encodeSignedHandshakeFrame(
  ephPub: Uint8Array,
  idPub: Uint8Array,
  sig: Uint8Array,
): Uint8Array {
  if (ephPub.length !== PUBKEY_LEN) throw new Error(`E2EE: eph_pub 长度必须 ${PUBKEY_LEN}`);
  if (idPub.length !== ID_PUBKEY_LEN) throw new Error(`E2EE: id_pub 长度必须 ${ID_PUBKEY_LEN}`);
  if (sig.length !== SIGNATURE_LEN) throw new Error(`E2EE: sig 长度必须 ${SIGNATURE_LEN}`);
  const out = new Uint8Array(SIGNED_HANDSHAKE_LEN);
  out[0] = DEVICE_BOUND_TAG;
  out.set(ephPub, 1);
  out.set(idPub, 1 + PUBKEY_LEN);
  out.set(sig, 1 + PUBKEY_LEN + ID_PUBKEY_LEN);
  return out;
}

/** 解析 0x02 帧；tag/长度非法抛错。 */
export function decodeSignedHandshakeFrame(frame: Uint8Array): SignedHandshake {
  if (frame.length !== SIGNED_HANDSHAKE_LEN || frame[0] !== DEVICE_BOUND_TAG) {
    throw new Error('E2EE: 收到非法签名握手帧（首帧必须为 0x02 || eph32 || id32 || sig64）');
  }
  return {
    ephPub: frame.slice(1, 1 + PUBKEY_LEN),
    idPub: frame.slice(1 + PUBKEY_LEN, 1 + PUBKEY_LEN + ID_PUBKEY_LEN),
    sig: frame.slice(1 + PUBKEY_LEN + ID_PUBKEY_LEN),
  };
}

/** 已解析的任意握手帧：legacy(0x01 裸公钥) 或 signed(0x02 带设备签名)。 */
export type AnyHandshake =
  | { kind: 'legacy'; ephPub: Uint8Array }
  | { kind: 'signed'; ephPub: Uint8Array; idPub: Uint8Array; sig: Uint8Array };

/** 按首字节分派解析握手帧（0x01→legacy，0x02→signed）；空帧/未知 tag 抛错。 */
export function decodeAnyHandshakeFrame(frame: Uint8Array): AnyHandshake {
  if (frame.length === 0) throw new Error('E2EE: 空握手帧');
  switch (frame[0]) {
    case HANDSHAKE_TAG:
      return { kind: 'legacy', ephPub: decodeHandshakeFrame(frame) };
    case DEVICE_BOUND_TAG: {
      const s = decodeSignedHandshakeFrame(frame);
      return { kind: 'signed', ephPub: s.ephPub, idPub: s.idPub, sig: s.sig };
    }
    default:
      throw new Error(`E2EE: 未知握手帧 tag 0x${frame[0].toString(16)}`);
  }
}

/** 以 1 字节长度前缀编码一个变长字节段（长度必须 ≤ 255）。 */
function lenPrefixed(bytes: Uint8Array): Uint8Array {
  if (bytes.length > 255) throw new Error('E2EE: id-bind 变长字段超过 255 字节');
  const out = new Uint8Array(1 + bytes.length);
  out[0] = bytes.length;
  out.set(bytes, 1);
  return out;
}

/**
 * 构造设备身份签名的 **context**（**不含**域分隔前缀；前缀由签名方在签名时加）。
 *   context = host_eph_pub(32) || controller_eph_pub(32)
 *           || u8(len)||device_name || u8(len)||username
 * 变长字段(device_name/username)加 1 字节长度前缀，杜绝 `a||b` 与 `a'||b'` 的拼接歧义。
 * 双方临时公钥都纳入 → 签名绑定到本次具体会话，防 relay 把同一 host 签名重放到别的 controller。
 */
export function buildIdBindContext(
  hostEphPub: Uint8Array,
  controllerEphPub: Uint8Array,
  deviceName: string,
  username: string,
): Uint8Array {
  if (hostEphPub.length !== PUBKEY_LEN || controllerEphPub.length !== PUBKEY_LEN) {
    throw new Error('E2EE: id-bind context 的临时公钥长度非法');
  }
  const dev = lenPrefixed(new TextEncoder().encode(deviceName));
  const usr = lenPrefixed(new TextEncoder().encode(username));
  const out = new Uint8Array(PUBKEY_LEN * 2 + dev.length + usr.length);
  let off = 0;
  out.set(hostEphPub, off);
  off += PUBKEY_LEN;
  out.set(controllerEphPub, off);
  off += PUBKEY_LEN;
  out.set(dev, off);
  off += dev.length;
  out.set(usr, off);
  return out;
}

/**
 * 验证对端设备身份签名：`ed25519.verify(sig, ID_BIND_DOMAIN || context, idPub)`。
 *
 * 与 Rust 签名方逐字节对齐：签名方对 `b"ridge-id-bind-v1" || context` 做 Ed25519 签名
 * （src-tauri `sign_device_identity` 命令加同一前缀；ridge-cli `e2ee.rs` 同）。Ed25519 是
 * RFC 8032 标准，@noble 与 ed25519-dalek 互验。返回是否通过（**不抛**，便于调用方决策）。
 */
export function verifyIdBindSignature(
  idPub: Uint8Array,
  context: Uint8Array,
  sig: Uint8Array,
): boolean {
  if (idPub.length !== ID_PUBKEY_LEN || sig.length !== SIGNATURE_LEN) return false;
  const domain = new TextEncoder().encode(ID_BIND_DOMAIN);
  const msg = new Uint8Array(domain.length + context.length);
  msg.set(domain, 0);
  msg.set(context, domain.length);
  try {
    return ed25519.verify(sig, msg, idPub);
  } catch {
    return false;
  }
}

// ── C 层 TOTP 信道绑定（零信任方案 #1 / 设计 §2）─────────────────────────────────
// 把 controller 输入的 6 位 TOTP 码升级为「transcript 上的 MAC」：码不再明文上线；
// 被攻陷 relay 即便 MITM 了 X25519，也因**不知码**无法算出正确 tag → 被 host 拒。
// 在线爆破由 host 端单桥 5 次上限 + LAN throttle 限制（6 位低熵仅在线可猜）。

/** 信道绑定 transcript 域分隔串。 */
export const BIND_TRANSCRIPT_DOMAIN = 'ridge-e2ee-bind-v1';
/** 信道绑定 HKDF info。 */
export const BIND_HKDF_INFO = 'ridge-bind';

/**
 * 构造信道绑定 transcript：`BIND_TRANSCRIPT_DOMAIN || sorted(host_eph, ctrl_eph)`。
 * 双方临时公钥按字典序排序拼接 → 两端独立计算得同一 transcript（与 deriveSessionKey
 * 的 salt 排序同思路），且把绑定锚定到本次具体的 X25519 协商。
 */
export function buildBindTranscript(
  hostEphPub: Uint8Array,
  controllerEphPub: Uint8Array,
): Uint8Array {
  if (hostEphPub.length !== PUBKEY_LEN || controllerEphPub.length !== PUBKEY_LEN) {
    throw new Error('E2EE: bind transcript 公钥长度非法');
  }
  const [first, second] =
    compareBytes(hostEphPub, controllerEphPub) <= 0
      ? [hostEphPub, controllerEphPub]
      : [controllerEphPub, hostEphPub];
  const domain = new TextEncoder().encode(BIND_TRANSCRIPT_DOMAIN);
  const out = new Uint8Array(domain.length + PUBKEY_LEN * 2);
  out.set(domain, 0);
  out.set(first, domain.length);
  out.set(second, domain.length + PUBKEY_LEN);
  return out;
}

/**
 * 计算信道绑定 MAC：
 *   K   = HKDF-SHA256(ikm = ascii(totp_code), salt = transcript, info = BIND_HKDF_INFO, L = 32)
 *   tag = HMAC-SHA256(K, transcript)
 * controller 用当前 6 位码算 tag 发给 host；host 用本机种子在 ±1 窗口各算一遍比对。
 * 返回 32 字节 tag。
 */
export function computeBindTag(totpCode: string, transcript: Uint8Array): Uint8Array {
  const ikm = new TextEncoder().encode(totpCode);
  const info = new TextEncoder().encode(BIND_HKDF_INFO);
  const k = hkdf(sha256, ikm, transcript, info, KEY_LEN);
  return hmac(sha256, k, transcript);
}

/** 把字节数组编码为 base64（信令 JSON 传公钥用，B3）。 */
export function bytesToBase64(bytes: Uint8Array): string {
  let s = '';
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
  return btoa(s);
}

/** 解析 base64 为字节数组；非法输入返回 null（不抛，调用方据此忽略坏帧）。 */
export function base64ToBytes(b64: string): Uint8Array | null {
  try {
    const s = atob(b64);
    const out = new Uint8Array(s.length);
    for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
    return out;
  } catch {
    return null;
  }
}

/**
 * 字典序比较两个等长字节数组。返回 <0 / 0 / >0。
 * 用于 salt 排序（契约 §7.1：双方按字典序排序保证一致）。
 */
function compareBytes(a: Uint8Array, b: Uint8Array): number {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return a.length - b.length;
}

/**
 * 完成握手并派生会话密钥（契约 §7.1）。
 *   shared = X25519(my_priv, peer_pub)
 *   salt   = sort(my_pub, peer_pub) 后拼接（64B，字典序）
 *   key    = HKDF-SHA256(ikm=shared, salt=salt, info="ridge-e2ee-v1", L=32)
 */
export function deriveSessionKey(
  myPrivateKey: Uint8Array,
  myPublicKey: Uint8Array,
  peerPublicKey: Uint8Array,
): Uint8Array {
  if (peerPublicKey.length !== PUBKEY_LEN) {
    throw new Error('E2EE: 对端公钥长度非法');
  }
  const shared = x25519.getSharedSecret(myPrivateKey, peerPublicKey);
  // 字典序排序保证两端 salt 一致。
  const [first, second] =
    compareBytes(myPublicKey, peerPublicKey) <= 0
      ? [myPublicKey, peerPublicKey]
      : [peerPublicKey, myPublicKey];
  const salt = new Uint8Array(PUBKEY_LEN * 2);
  salt.set(first, 0);
  salt.set(second, PUBKEY_LEN);
  return hkdf(sha256, shared, salt, new TextEncoder().encode(HKDF_INFO), KEY_LEN);
}

/**
 * 按方向构造 nonce（契约 §7.2）：
 *   nonce(12) = [ dir(1) , 0,0,0 , counter_u64_le(8) ]
 */
export function buildNonce(dir: Direction, counter: bigint): Uint8Array {
  if (counter < 0n || counter > COUNTER_MAX) {
    throw new Error('E2EE: counter 超出 u64 范围');
  }
  const nonce = new Uint8Array(NONCE_LEN);
  nonce[0] = dir;
  // nonce[1..4] 保持 0
  // counter 小端写入 nonce[4..12]
  const view = new DataView(nonce.buffer, nonce.byteOffset, nonce.byteLength);
  view.setBigUint64(4, counter, true /* little-endian */);
  return nonce;
}

/** 从 nonce 读出方向字节。 */
export function nonceDirection(nonce: Uint8Array): number {
  return nonce[0];
}

/** 从 nonce 读出 counter（u64 LE）。 */
export function nonceCounter(nonce: Uint8Array): bigint {
  const view = new DataView(nonce.buffer, nonce.byteOffset, nonce.byteLength);
  return view.getBigUint64(4, true);
}

/**
 * E2EE 会话。握手完成后持有对称 key，提供方向分离的 seal/open。
 *
 *   - sendDir：本端发出帧用的方向（host 端为 0，controller 端为 1）。
 *   - recvDir：期望从对端收到帧的方向（与 sendDir 相反）。
 *   - 接收端严格校验 nonce.dir == recvDir 且 counter 严格递增（防重放）。
 */
export class E2eeSession {
  private readonly key: Uint8Array;
  private readonly sendDir: Direction;
  private readonly recvDir: Direction;
  private sendCounter = 0n;
  // 已成功 open 的最大接收 counter。要求严格递增（> 此值），首帧从 0 起。
  private lastRecvCounter: bigint | null = null;

  constructor(key: Uint8Array, sendDir: Direction) {
    if (key.length !== KEY_LEN) {
      throw new Error('E2EE: 会话密钥长度非法');
    }
    this.key = key;
    this.sendDir = sendDir;
    this.recvDir = sendDir === DIR_HOST_TO_CONTROLLER ? DIR_CONTROLLER_TO_HOST : DIR_HOST_TO_CONTROLLER;
  }

  /**
   * 加密一帧明文，返回线上帧：nonce(12) || ciphertext_with_tag。
   * counter 单调自增，接近上限时抛错（契约要求重建连接）。
   */
  seal(plaintext: Uint8Array): Uint8Array {
    if (this.sendCounter >= COUNTER_MAX) {
      throw new Error('E2EE: 发送 counter 接近上限，必须重建连接');
    }
    const nonce = buildNonce(this.sendDir, this.sendCounter);
    this.sendCounter += 1n;
    const cipher = chacha20poly1305(this.key, nonce);
    const ct = cipher.encrypt(plaintext); // tag 附于密文尾（库默认）
    const out = new Uint8Array(NONCE_LEN + ct.length);
    out.set(nonce, 0);
    out.set(ct, NONCE_LEN);
    return out;
  }

  /**
   * 解密一帧线上数据。校验：
   *   1. 长度 >= nonce(12) + tag(16)
   *   2. nonce.dir == 期望的对端方向
   *   3. counter 严格递增（防重放）
   *   4. Poly1305 tag 校验（noble decrypt 失败抛错）
   * 任一失败抛错（调用方据此断开 / 丢弃）。
   */
  open(frame: Uint8Array): Uint8Array {
    if (frame.length < NONCE_LEN + 16) {
      throw new Error('E2EE: 密文帧过短');
    }
    const nonce = frame.slice(0, NONCE_LEN);
    const dir = nonceDirection(nonce);
    if (dir !== this.recvDir) {
      throw new Error(`E2EE: nonce 方向非法（期望 ${this.recvDir}，实际 ${dir}）`);
    }
    const counter = nonceCounter(nonce);
    if (this.lastRecvCounter !== null && counter <= this.lastRecvCounter) {
      throw new Error('E2EE: counter 未严格递增（疑似重放）');
    }
    const ct = frame.slice(NONCE_LEN);
    const cipher = chacha20poly1305(this.key, nonce);
    const plaintext = cipher.decrypt(ct); // tag 不符会抛错
    // 仅在解密成功后推进接收计数，避免被无效帧推高计数器。
    this.lastRecvCounter = counter;
    return plaintext;
  }
}
