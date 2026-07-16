import { describe, test, expect } from 'vitest';
import {
  fingerprintOf,
  checkOrPinDeviceIdentity,
  getPinnedFingerprint,
  clearDevicePin,
  type TrustStore,
} from './deviceTrust';

function memStore(): TrustStore {
  const m = new Map<string, string>();
  return {
    get: (k) => m.get(k) ?? null,
    set: (k, v) => {
      m.set(k, v);
    },
    remove: (k) => {
      m.delete(k);
    },
  };
}

describe('deviceTrust 指纹（与 Rust device_identity::fingerprint_of 对齐）', () => {
  test('fingerprintOf：确定性 + 形如 XXXX-XXXX-XXXX-XXXX 大写 hex', () => {
    const idPub = new Uint8Array(32).fill(0x11);
    expect(fingerprintOf(idPub)).toBe(fingerprintOf(idPub));
    expect(fingerprintOf(idPub)).toMatch(/^[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}$/);
  });

  test('golden：固定公钥 0x11*32 的指纹（跨 Rust 实现 conformance 锚点）', () => {
    // ridge-core device_identity::fingerprint_of 对同一公钥必产出同串。
    expect(fingerprintOf(new Uint8Array(32).fill(0x11))).toBe('02D4-49A3-1FBB-267C');
  });

  test('不同公钥 → 不同指纹', () => {
    expect(fingerprintOf(new Uint8Array(32).fill(1))).not.toBe(
      fingerprintOf(new Uint8Array(32).fill(2)),
    );
  });
});

describe('deviceTrust TOFU 固定', () => {
  const idPub = new Uint8Array(32).fill(0x11);

  test('首见 → pinned，再连一致 → match', () => {
    const store = memStore();
    expect(checkOrPinDeviceIdentity('laptop-alice', idPub, store).status).toBe('pinned');
    expect(checkOrPinDeviceIdentity('laptop-alice', idPub, store).status).toBe('match');
  });

  test('指纹变化（疑似 MITM / 换机）→ changed，含 pinned 与 actual', () => {
    const store = memStore();
    checkOrPinDeviceIdentity('laptop-alice', idPub, store);
    const other = new Uint8Array(32).fill(0x99);
    const r = checkOrPinDeviceIdentity('laptop-alice', other, store);
    expect(r.status).toBe('changed');
    if (r.status === 'changed') {
      expect(r.pinned).toBe(fingerprintOf(idPub));
      expect(r.actual).toBe(fingerprintOf(other));
    }
  });

  test('不同 host 独立 pin，互不影响', () => {
    const store = memStore();
    checkOrPinDeviceIdentity('host-a', idPub, store);
    expect(checkOrPinDeviceIdentity('host-b', new Uint8Array(32).fill(0x55), store).status).toBe(
      'pinned',
    );
    expect(getPinnedFingerprint('host-a', store)).toBe(fingerprintOf(idPub));
  });

  test('clearDevicePin 后重新 TOFU（再次 pinned）', () => {
    const store = memStore();
    checkOrPinDeviceIdentity('laptop-alice', idPub, store);
    clearDevicePin('laptop-alice', store);
    expect(getPinnedFingerprint('laptop-alice', store)).toBeNull();
    expect(checkOrPinDeviceIdentity('laptop-alice', idPub, store).status).toBe('pinned');
  });
});
