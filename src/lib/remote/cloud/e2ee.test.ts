// E2EE 单元测试（契约 §7）：验证 seal→open 往返、方向 nonce、重放被拒。
import { describe, test, expect } from 'vitest';
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
