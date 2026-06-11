// E2EE 单元测试（契约 §7）：验证 seal→open 往返、方向 nonce、重放被拒。
import { describe, test, expect } from 'vitest';
import { ed25519 } from '@noble/curves/ed25519.js';
import {
  generateEphemeralKeyPair,
  encodeHandshakeFrame,
  decodeHandshakeFrame,
  deriveSessionKey,
  buildNonce,
  nonceDirection,
  nonceCounter,
  E2eeSession,
  DIR_HOST_TO_CONTROLLER,
  DIR_CONTROLLER_TO_HOST,
  HANDSHAKE_TAG,
  PUBKEY_LEN,
  NONCE_LEN,
  KEY_LEN,
  encodeSignedHandshakeFrame,
  decodeSignedHandshakeFrame,
  decodeAnyHandshakeFrame,
  buildIdBindContext,
  verifyIdBindSignature,
  DEVICE_BOUND_TAG,
  SIGNED_HANDSHAKE_LEN,
  SIGNATURE_LEN,
  ID_BIND_DOMAIN,
  buildBindTranscript,
  computeBindTag,
  BIND_TRANSCRIPT_DOMAIN,
} from './e2ee';

/** 模拟一次完整的 host/controller 握手，返回双方会话。 */
function establishSessions(): { host: E2eeSession; controller: E2eeSession } {
  const hostKp = generateEphemeralKeyPair();
  const ctrlKp = generateEphemeralKeyPair();

  // 双方交换握手帧并解析对端公钥。
  const hostFrame = encodeHandshakeFrame(hostKp.publicKey);
  const ctrlFrame = encodeHandshakeFrame(ctrlKp.publicKey);
  const hostSeesCtrlPub = decodeHandshakeFrame(ctrlFrame);
  const ctrlSeesHostPub = decodeHandshakeFrame(hostFrame);

  const hostKey = deriveSessionKey(hostKp.privateKey, hostKp.publicKey, hostSeesCtrlPub);
  const ctrlKey = deriveSessionKey(ctrlKp.privateKey, ctrlKp.publicKey, ctrlSeesHostPub);

  // 双方派生的 key 必须一致。
  expect(hostKey).toEqual(ctrlKey);

  return {
    host: new E2eeSession(hostKey, DIR_HOST_TO_CONTROLLER),
    controller: new E2eeSession(ctrlKey, DIR_CONTROLLER_TO_HOST),
  };
}

describe('E2EE 握手帧编解码', () => {
  test('encode 产生 0x01 || pub32', () => {
    // Arrange
    const kp = generateEphemeralKeyPair();
    // Act
    const frame = encodeHandshakeFrame(kp.publicKey);
    // Assert
    expect(frame.length).toBe(1 + PUBKEY_LEN);
    expect(frame[0]).toBe(HANDSHAKE_TAG);
    expect(frame.slice(1)).toEqual(kp.publicKey);
  });

  test('decode 还原对端公钥', () => {
    const kp = generateEphemeralKeyPair();
    const frame = encodeHandshakeFrame(kp.publicKey);
    expect(decodeHandshakeFrame(frame)).toEqual(kp.publicKey);
  });

  test('decode 对非握手帧（首字节非 0x01）抛错', () => {
    const bad = new Uint8Array(1 + PUBKEY_LEN);
    bad[0] = 0x02;
    expect(() => decodeHandshakeFrame(bad)).toThrow();
  });

  test('decode 对长度错误的帧抛错', () => {
    const bad = new Uint8Array(10);
    bad[0] = HANDSHAKE_TAG;
    expect(() => decodeHandshakeFrame(bad)).toThrow();
  });
});

describe('E2EE 密钥派生', () => {
  test('双方独立派生出相同会话密钥，且 salt 排序无关连接顺序', () => {
    const a = generateEphemeralKeyPair();
    const b = generateEphemeralKeyPair();
    const keyFromA = deriveSessionKey(a.privateKey, a.publicKey, b.publicKey);
    const keyFromB = deriveSessionKey(b.privateKey, b.publicKey, a.publicKey);
    expect(keyFromA.length).toBe(KEY_LEN);
    expect(keyFromA).toEqual(keyFromB);
  });

  test('不同对端公钥派生出不同密钥', () => {
    const a = generateEphemeralKeyPair();
    const b = generateEphemeralKeyPair();
    const c = generateEphemeralKeyPair();
    const k1 = deriveSessionKey(a.privateKey, a.publicKey, b.publicKey);
    const k2 = deriveSessionKey(a.privateKey, a.publicKey, c.publicKey);
    expect(k1).not.toEqual(k2);
  });
});

describe('E2EE 方向 nonce（§7.2）', () => {
  test('nonce = [dir, 0,0,0, counter_u64_le]', () => {
    const nonce = buildNonce(DIR_CONTROLLER_TO_HOST, 0x0102030405060708n);
    expect(nonce.length).toBe(NONCE_LEN);
    expect(nonce[0]).toBe(DIR_CONTROLLER_TO_HOST);
    expect(nonce[1]).toBe(0);
    expect(nonce[2]).toBe(0);
    expect(nonce[3]).toBe(0);
    // 小端：最低字节在前
    expect(Array.from(nonce.slice(4))).toEqual([0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
  });

  test('nonceDirection / nonceCounter 往返', () => {
    const nonce = buildNonce(DIR_HOST_TO_CONTROLLER, 42n);
    expect(nonceDirection(nonce)).toBe(DIR_HOST_TO_CONTROLLER);
    expect(nonceCounter(nonce)).toBe(42n);
  });
});

describe('E2EE seal/open 往返', () => {
  test('host seal → controller open，明文一致', () => {
    const { host, controller } = establishSessions();
    const msg = new TextEncoder().encode('postcard-frame-payload');
    const sealed = host.seal(msg);
    // 线上帧 = nonce(12) || ciphertext_with_tag
    expect(sealed.length).toBe(NONCE_LEN + msg.length + 16);
    const opened = controller.open(sealed);
    expect(opened).toEqual(msg);
  });

  test('controller seal → host open，明文一致', () => {
    const { host, controller } = establishSessions();
    const msg = new Uint8Array([1, 2, 3, 4, 5, 0, 255, 128]);
    const sealed = controller.seal(msg);
    const opened = host.open(sealed);
    expect(opened).toEqual(msg);
  });

  test('双向多帧连续往返，counter 自增不互相干扰', () => {
    const { host, controller } = establishSessions();
    for (let i = 0; i < 5; i++) {
      const fromHost = new TextEncoder().encode(`h${i}`);
      expect(controller.open(host.seal(fromHost))).toEqual(fromHost);
      const fromCtrl = new TextEncoder().encode(`c${i}`);
      expect(host.open(controller.seal(fromCtrl))).toEqual(fromCtrl);
    }
  });

  test('空帧也可往返', () => {
    const { host, controller } = establishSessions();
    const empty = new Uint8Array(0);
    expect(controller.open(host.seal(empty))).toEqual(empty);
  });
});

describe('E2EE 方向校验', () => {
  test('host 不能 open 自己 seal 的帧（方向相同被拒）', () => {
    const { host } = establishSessions();
    const sealed = host.seal(new TextEncoder().encode('x'));
    // host 期望收到 controller→host(dir=1) 的帧，但 sealed 是 host→controller(dir=0)
    expect(() => host.open(sealed)).toThrow(/方向/);
  });
});

describe('E2EE 重放防护', () => {
  test('重放同一帧被拒（counter 未严格递增）', () => {
    const { host, controller } = establishSessions();
    const sealed = host.seal(new TextEncoder().encode('once'));
    // 第一次正常解密
    expect(controller.open(sealed)).toEqual(new TextEncoder().encode('once'));
    // 重放同一帧（相同 counter）→ 被拒
    expect(() => controller.open(sealed)).toThrow(/重放|递增/);
  });

  test('乱序旧帧（counter 回退）被拒', () => {
    const { host, controller } = establishSessions();
    const f0 = host.seal(new TextEncoder().encode('f0')); // counter 0
    const f1 = host.seal(new TextEncoder().encode('f1')); // counter 1
    // 先收 f1，推进 lastRecvCounter=1
    expect(controller.open(f1)).toEqual(new TextEncoder().encode('f1'));
    // 再收更旧的 f0（counter 0 <= 1）→ 被拒
    expect(() => controller.open(f0)).toThrow(/重放|递增/);
  });

  test('被篡改的密文（tag 不符）被拒', () => {
    const { host, controller } = establishSessions();
    const sealed = host.seal(new TextEncoder().encode('tamper-me'));
    // 翻转密文区一个字节
    const tampered = sealed.slice();
    tampered[tampered.length - 1] ^= 0xff;
    expect(() => controller.open(tampered)).toThrow();
  });

  test('被篡改后失败的帧不会推进接收计数（后续合法帧仍可解）', () => {
    const { host, controller } = establishSessions();
    const f0 = host.seal(new TextEncoder().encode('f0')); // counter 0
    const tampered = f0.slice();
    tampered[tampered.length - 1] ^= 0xff;
    expect(() => controller.open(tampered)).toThrow();
    // counter 0 的合法帧仍应可解（失败帧未推高 lastRecvCounter）
    expect(controller.open(f0)).toEqual(new TextEncoder().encode('f0'));
  });
});

describe('B 层 0x02 设备签名握手帧（零信任 #2）', () => {
  // 测试用固定 Ed25519 设备身份种子（**非生产密钥**）。
  const idPriv = new Uint8Array(32).fill(7);
  const idPub = ed25519.getPublicKey(idPriv);

  /** 模拟 Rust 签名方：对 `ID_BIND_DOMAIN || context` 做 Ed25519 签名。 */
  function signContext(priv: Uint8Array, context: Uint8Array): Uint8Array {
    const domain = new TextEncoder().encode(ID_BIND_DOMAIN);
    const msg = new Uint8Array(domain.length + context.length);
    msg.set(domain, 0);
    msg.set(context, domain.length);
    return ed25519.sign(msg, priv);
  }

  test('encode 产生 0x02 || eph32 || id32 || sig64（129B）', () => {
    const eph = generateEphemeralKeyPair().publicKey;
    const sig = new Uint8Array(SIGNATURE_LEN).fill(9);
    const frame = encodeSignedHandshakeFrame(eph, idPub, sig);
    expect(frame.length).toBe(SIGNED_HANDSHAKE_LEN);
    expect(frame.length).toBe(129);
    expect(frame[0]).toBe(DEVICE_BOUND_TAG);
    expect(frame.slice(1, 33)).toEqual(eph);
    expect(frame.slice(33, 65)).toEqual(idPub);
    expect(frame.slice(65)).toEqual(sig);
  });

  test('decodeSignedHandshakeFrame 还原三段', () => {
    const eph = generateEphemeralKeyPair().publicKey;
    const sig = new Uint8Array(SIGNATURE_LEN).fill(3);
    const parsed = decodeSignedHandshakeFrame(encodeSignedHandshakeFrame(eph, idPub, sig));
    expect(parsed.ephPub).toEqual(eph);
    expect(parsed.idPub).toEqual(idPub);
    expect(parsed.sig).toEqual(sig);
  });

  test('decodeSignedHandshakeFrame 对错误 tag / 长度抛错', () => {
    const good = encodeSignedHandshakeFrame(
      generateEphemeralKeyPair().publicKey,
      idPub,
      new Uint8Array(SIGNATURE_LEN),
    );
    const wrongTag = good.slice();
    wrongTag[0] = 0x01;
    expect(() => decodeSignedHandshakeFrame(wrongTag)).toThrow();
    expect(() => decodeSignedHandshakeFrame(good.slice(0, 128))).toThrow();
  });

  test('decodeAnyHandshakeFrame 分派 legacy(0x01) / signed(0x02) / 未知抛错', () => {
    const eph = generateEphemeralKeyPair().publicKey;
    const a = decodeAnyHandshakeFrame(encodeHandshakeFrame(eph));
    expect(a.kind).toBe('legacy');
    if (a.kind === 'legacy') expect(a.ephPub).toEqual(eph);
    const b = decodeAnyHandshakeFrame(
      encodeSignedHandshakeFrame(eph, idPub, new Uint8Array(SIGNATURE_LEN)),
    );
    expect(b.kind).toBe('signed');
    if (b.kind === 'signed') expect(b.idPub).toEqual(idPub);
    expect(() => decodeAnyHandshakeFrame(new Uint8Array([0x09, 1, 2, 3]))).toThrow();
    expect(() => decodeAnyHandshakeFrame(new Uint8Array(0))).toThrow();
  });

  test('buildIdBindContext：长度前缀消除拼接歧义', () => {
    const h = generateEphemeralKeyPair().publicKey;
    const c = generateEphemeralKeyPair().publicKey;
    // ("ab","c") 与 ("a","bc") 无长度前缀时会拼成同一串；这里必须不同。
    expect(buildIdBindContext(h, c, 'ab', 'c')).not.toEqual(buildIdBindContext(h, c, 'a', 'bc'));
    // 确定性：同输入同输出。
    expect(buildIdBindContext(h, c, 'dev', 'alice')).toEqual(
      buildIdBindContext(h, c, 'dev', 'alice'),
    );
  });

  test('verifyIdBindSignature：正确签名通过', () => {
    const host = generateEphemeralKeyPair().publicKey;
    const ctrl = generateEphemeralKeyPair().publicKey;
    const context = buildIdBindContext(host, ctrl, 'my-laptop', 'alice');
    expect(verifyIdBindSignature(idPub, context, signContext(idPriv, context))).toBe(true);
  });

  test('verifyIdBindSignature：篡改 context（换 username）被拒', () => {
    const host = generateEphemeralKeyPair().publicKey;
    const ctrl = generateEphemeralKeyPair().publicKey;
    const sig = signContext(idPriv, buildIdBindContext(host, ctrl, 'my-laptop', 'alice'));
    expect(verifyIdBindSignature(idPub, buildIdBindContext(host, ctrl, 'my-laptop', 'mallory'), sig)).toBe(
      false,
    );
  });

  test('verifyIdBindSignature：换 host 临时公钥（疑似 MITM）被拒', () => {
    const host = generateEphemeralKeyPair().publicKey;
    const ctrl = generateEphemeralKeyPair().publicKey;
    const sig = signContext(idPriv, buildIdBindContext(host, ctrl, 'd', 'u'));
    const mitmHost = generateEphemeralKeyPair().publicKey;
    expect(verifyIdBindSignature(idPub, buildIdBindContext(mitmHost, ctrl, 'd', 'u'), sig)).toBe(false);
  });

  test('verifyIdBindSignature：错误设备身份公钥被拒', () => {
    const host = generateEphemeralKeyPair().publicKey;
    const ctrl = generateEphemeralKeyPair().publicKey;
    const context = buildIdBindContext(host, ctrl, 'd', 'u');
    const sig = signContext(idPriv, context);
    const otherPub = ed25519.getPublicKey(new Uint8Array(32).fill(8));
    expect(verifyIdBindSignature(otherPub, context, sig)).toBe(false);
  });

  test('verifyIdBindSignature：篡改签名 / 非法长度被拒且不抛', () => {
    const host = generateEphemeralKeyPair().publicKey;
    const ctrl = generateEphemeralKeyPair().publicKey;
    const context = buildIdBindContext(host, ctrl, 'd', 'u');
    const sig = signContext(idPriv, context);
    const bad = sig.slice();
    bad[0] ^= 0xff;
    expect(verifyIdBindSignature(idPub, context, bad)).toBe(false);
    expect(verifyIdBindSignature(idPub, context, new Uint8Array(10))).toBe(false);
  });
});

describe('C 层 TOTP 信道绑定 MAC（零信任 #1）', () => {
  test('buildBindTranscript：排序无关连接顺序（两端独立计算一致）', () => {
    const h = generateEphemeralKeyPair().publicKey;
    const c = generateEphemeralKeyPair().publicKey;
    expect(buildBindTranscript(h, c)).toEqual(buildBindTranscript(c, h));
  });

  test('buildBindTranscript：以域分隔串开头，长度 = domain + 64', () => {
    const h = generateEphemeralKeyPair().publicKey;
    const c = generateEphemeralKeyPair().publicKey;
    const t = buildBindTranscript(h, c);
    const domain = new TextEncoder().encode(BIND_TRANSCRIPT_DOMAIN);
    expect(t.length).toBe(domain.length + 64);
    expect(t.slice(0, domain.length)).toEqual(domain);
  });

  test('computeBindTag：确定性 + 双端同输入同 tag，长度 32', () => {
    const transcript = buildBindTranscript(
      generateEphemeralKeyPair().publicKey,
      generateEphemeralKeyPair().publicKey,
    );
    const a = computeBindTag('123456', transcript);
    expect(a).toEqual(computeBindTag('123456', transcript));
    expect(a.length).toBe(32);
  });

  test('computeBindTag：错误 6 位码算出不同 tag（中继不知码无法伪造）', () => {
    const transcript = buildBindTranscript(
      generateEphemeralKeyPair().publicKey,
      generateEphemeralKeyPair().publicKey,
    );
    expect(computeBindTag('123456', transcript)).not.toEqual(computeBindTag('654321', transcript));
  });

  test('computeBindTag：不同 transcript（MITM 换公钥）→ 不同 tag（即便码相同）', () => {
    const h = generateEphemeralKeyPair().publicKey;
    const c = generateEphemeralKeyPair().publicKey;
    const mitm = generateEphemeralKeyPair().publicKey;
    expect(computeBindTag('123456', buildBindTranscript(h, c))).not.toEqual(
      computeBindTag('123456', buildBindTranscript(mitm, c)),
    );
  });
});
