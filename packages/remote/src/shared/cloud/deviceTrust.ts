// src/lib/remote/cloud/deviceTrust.ts
//
// 零信任方案 #2 的 controller 侧 **TOFU（首次使用即信任）** 固定。
// controller 首次连某 host 时记录其 Ed25519 设备身份公钥指纹（类 SSH known_hosts）；
// 再次连接比对，指纹变化即判定「疑似 MITM 或换机」→ 由调用方按策略告警/拒绝
// （fail-closed 翻闸是 P3；本期默认告警、不强拒）。
//
// 指纹算法与 Rust `ridge-core::device_identity::fingerprint_of` **逐字节一致**：
//   SHA-256(id_pub) 前 8 字节，大写 hex，每 2 字节一组用 '-' 分隔（XXXX-XXXX-XXXX-XXXX）。
// 这样 host（桌面面板 / cli TUI）与 controller 显示同一指纹，用户可带外核对（SAS）。
//
// pin 只存**公钥指纹**（非秘密），localStorage 即可；设备私钥永不在浏览器侧。

import { sha256 } from '@noble/hashes/sha2.js';

/** 抽象存储（生产 = localStorage；测试注入内存 mock）。 */
export interface TrustStore {
  get(key: string): string | null;
  set(key: string, value: string): void;
  remove(key: string): void;
}

/** localStorage 后端；SSR / 无 localStorage 时退化为进程内存（与 auth.ts 同策略）。 */
export function localStorageTrustStore(): TrustStore {
  const ls = typeof localStorage !== 'undefined' ? localStorage : null;
  if (ls) {
    return {
      get: (k) => ls.getItem(k),
      set: (k, v) => ls.setItem(k, v),
      remove: (k) => ls.removeItem(k),
    };
  }
  const mem = new Map<string, string>();
  return {
    get: (k) => mem.get(k) ?? null,
    set: (k, v) => {
      mem.set(k, v);
    },
    remove: (k) => {
      mem.delete(k);
    },
  };
}

/**
 * 设备身份公钥指纹（与 Rust `device_identity::fingerprint_of` 一致）：
 * SHA-256(id_pub) 前 8 字节，大写 hex，每 2 字节一组用 '-' 分隔。
 */
export function fingerprintOf(idPub: Uint8Array): string {
  const digest = sha256(idPub);
  let s = '';
  for (let i = 0; i < 8; i++) {
    if (i > 0 && i % 2 === 0) s += '-';
    s += digest[i].toString(16).padStart(2, '0').toUpperCase();
  }
  return s;
}

/** TOFU 判定结果。 */
export type TofuResult =
  | { status: 'pinned'; fingerprint: string } // 首见 → 已固定
  | { status: 'match'; fingerprint: string } // 与已固定一致
  | { status: 'changed'; pinned: string; actual: string }; // 变化（疑似 MITM / 换机）

/** pin 的 localStorage key 前缀。 */
const PIN_PREFIX = 'ridge.cloud.trust.';

/**
 * TOFU 校验或固定：首见记录指纹返回 `pinned`；一致返回 `match`；不一致返回 `changed`
 * （调用方据策略决定告警还是拒绝）。`hostKey` = 稳定 host 标识（如 `device-username`）。
 */
export function checkOrPinDeviceIdentity(
  hostKey: string,
  idPub: Uint8Array,
  store: TrustStore = localStorageTrustStore(),
): TofuResult {
  const actual = fingerprintOf(idPub);
  const key = PIN_PREFIX + hostKey;
  const pinned = store.get(key);
  if (pinned === null || pinned === '') {
    store.set(key, actual);
    return { status: 'pinned', fingerprint: actual };
  }
  if (pinned === actual) return { status: 'match', fingerprint: actual };
  return { status: 'changed', pinned, actual };
}

/** 取某 host 已固定的指纹（无则 null）。 */
export function getPinnedFingerprint(
  hostKey: string,
  store: TrustStore = localStorageTrustStore(),
): string | null {
  return store.get(PIN_PREFIX + hostKey) || null;
}

/** 清除某 host 的 pin（用户主动「重新信任」/ 换机后重新 TOFU）。 */
export function clearDevicePin(hostKey: string, store: TrustStore = localStorageTrustStore()): void {
  store.remove(PIN_PREFIX + hostKey);
}
