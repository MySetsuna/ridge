// controllerIdentity 单元测试（契约 §7.4）
// 依赖 SSR 降级路径（Node 无 IndexedDB），无需 idb stub。
import { describe, test, expect, beforeEach } from 'vitest';
import { ed25519 } from '@noble/curves/ed25519.js';
import {
  getControllerPub,
  signTrust,
  clearControllerIdentity,
  _resetCacheForTest,
} from './controllerIdentity.js';

// Node 环境下 typeof indexedDB === 'undefined'，模块自动退化到仅内存密钥。
// 每个 test 前重置内存缓存，确保隔离。
beforeEach(() => {
  _resetCacheForTest();
});

describe('getControllerPub', () => {
  test('返回 32 字节公钥', async () => {
    const pub = await getControllerPub();
    expect(pub).toBeInstanceOf(Uint8Array);
    expect(pub.byteLength).toBe(32);
  });

  test('多次调用返回同一公钥（内存记忆化）', async () => {
    const pub1 = await getControllerPub();
    const pub2 = await getControllerPub();
    // 同一密钥对应同一公钥（字节相等）
    expect(pub1).toEqual(pub2);
  });
});

describe('signTrust', () => {
  test('返回 64 字节签名', async () => {
    const msg = new TextEncoder().encode('ridge-cloud-trust-v1');
    const sig = await signTrust(msg);
    expect(sig).toBeInstanceOf(Uint8Array);
    expect(sig.byteLength).toBe(64);
  });

  test('签名可被 @noble ed25519.verify 验证（与宿主侧互验）', async () => {
    const msg = new TextEncoder().encode('ridge-cloud-trust-v1');
    const pub = await getControllerPub();
    const sig = await signTrust(msg);
    const ok = ed25519.verify(sig, msg, pub);
    expect(ok).toBe(true);
  });

  test('篡改消息后验签失败', async () => {
    const msg = new TextEncoder().encode('ridge-cloud-trust-v1');
    const pub = await getControllerPub();
    const sig = await signTrust(msg);
    // 修改 msg 第一字节
    const tampered = new Uint8Array(msg);
    tampered[0] ^= 0xff;
    const ok = ed25519.verify(sig, tampered, pub);
    expect(ok).toBe(false);
  });
});

describe('clearControllerIdentity', () => {
  test('清除后下次 getControllerPub 生成新密钥（内存缓存已清）', async () => {
    const pub1 = await getControllerPub();
    await clearControllerIdentity();   // 清除缓存（IndexedDB 不可用无影响）
    // 重置缓存后重新生成——两次生成的随机密钥极大概率不同
    const pub2 = await getControllerPub();
    // 因为 Node 环境每次都重新随机生成，所以两个公钥不相等
    expect(pub1).not.toEqual(pub2);
  });
});
