// controllerIdentity 单元测试（契约 §7.4）
// 依赖 SSR 降级路径（Node 无 IndexedDB），无需 idb stub。
import { describe, test, expect, beforeEach, vi } from 'vitest';
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

function fakeIndexedDb() {
  const values = new Map<string, Uint8Array>();
  let hasStore = false;
  const db: any = {
    objectStoreNames: { contains: () => hasStore },
    createObjectStore: vi.fn(() => {
      hasStore = true;
      return {};
    }),
    transaction: vi.fn(() => {
      const tx: any = {
        oncomplete: null,
        onerror: null,
        error: null,
      };
      const complete = () => queueMicrotask(() => tx.oncomplete?.());
      tx.objectStore = () => ({
        get: (key: string) => {
          const request: any = { result: undefined, error: null, onsuccess: null, onerror: null };
          queueMicrotask(() => {
            request.result = values.get(key);
            request.onsuccess?.({ target: request });
            complete();
          });
          return request;
        },
        put: (value: Uint8Array, key: string) => {
          const request: any = { result: undefined, error: null, onsuccess: null, onerror: null };
          queueMicrotask(() => {
            values.set(key, value);
            request.onsuccess?.({ target: request });
            complete();
          });
          return request;
        },
        delete: (key: string) => {
          const request: any = { result: undefined, error: null, onsuccess: null, onerror: null };
          queueMicrotask(() => {
            values.delete(key);
            request.onsuccess?.({ target: request });
            complete();
          });
          return request;
        },
      });
      return tx;
    }),
    close: vi.fn(),
  };
  const indexedDB = {
    open: vi.fn(() => {
      const request: any = { result: db, error: null, onupgradeneeded: null, onsuccess: null, onerror: null };
      queueMicrotask(() => {
        if (!hasStore) request.onupgradeneeded?.({ target: request });
        request.onsuccess?.({ target: request });
      });
      return request;
    }),
  };
  return { indexedDB, db, values };
}

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

describe('IndexedDB persistence and failure fallback', () => {
  test('loads, saves, and deletes the durable private key', async () => {
    const fake = fakeIndexedDb();
    vi.stubGlobal('indexedDB', fake.indexedDB);

    const first = await getControllerPub();
    _resetCacheForTest();
    expect(await getControllerPub()).toEqual(first);
    await clearControllerIdentity();
    _resetCacheForTest();
    expect(await getControllerPub()).not.toEqual(first);
    expect(fake.db.close).toHaveBeenCalled();
  });

  test('falls back to memory when IndexedDB open fails', async () => {
    const open = vi.fn(() => {
      const request: any = { error: new Error('blocked'), onerror: null };
      queueMicrotask(() => request.onerror?.());
      return request;
    });
    vi.stubGlobal('indexedDB', { open });

    const pub = await getControllerPub();
    expect(pub).toHaveLength(32);
    await clearControllerIdentity();
    expect(open).toHaveBeenCalledTimes(2);
  });
});
