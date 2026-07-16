// controllerIdentity.ts
// 控制端持久化 Ed25519 身份模块（契约 §7.4）
//
// 每个浏览器生成一个长期 Ed25519 密钥对，存入 IndexedDB。
// 后端用公钥验签来识别"同一台受信控制端"，支持 24h TOTP grant。
//
// 导入路径与 e2ee.ts 完全一致，确保签名与宿主侧 @noble verify 互验。

import { ed25519 } from '@noble/curves/ed25519.js';

// ── IndexedDB 配置 ───────────────────────────────────────────────────────────
const IDB_NAME = 'ridge-cloud';
const IDB_STORE = 'identity';
const IDB_KEY = 'controller-ed25519-priv';
const IDB_VERSION = 1;

// ── 模块级内存缓存（避免重复读库；SSR/Node 下也作唯一存储） ──────────────
let cachedPriv: Uint8Array | null = null;

// ── IndexedDB 辅助（自包含，不依赖第三方 idb 库） ──────────────────────────

/** 打开（或初始化）IndexedDB，返回 IDBDatabase。失败时 reject。 */
function openIdb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(IDB_NAME, IDB_VERSION);
    req.onupgradeneeded = (e) => {
      const db = (e.target as IDBOpenDBRequest).result;
      if (!db.objectStoreNames.contains(IDB_STORE)) {
        db.createObjectStore(IDB_STORE);
      }
    };
    req.onsuccess = (e) => resolve((e.target as IDBOpenDBRequest).result);
    req.onerror = () => reject(req.error);
  });
}

/** 从 IndexedDB 读取存储的私钥；不存在时返回 null。 */
async function idbLoad(): Promise<Uint8Array | null> {
  const db = await openIdb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readonly');
    const req = tx.objectStore(IDB_STORE).get(IDB_KEY);
    req.onsuccess = () => {
      const val = req.result;
      resolve(val instanceof Uint8Array ? val : null);
    };
    req.onerror = () => reject(req.error);
    tx.oncomplete = () => db.close();
  });
}

/** 将私钥写入 IndexedDB。 */
async function idbSave(priv: Uint8Array): Promise<void> {
  const db = await openIdb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readwrite');
    const req = tx.objectStore(IDB_STORE).put(priv, IDB_KEY);
    req.onerror = () => reject(req.error);
    tx.oncomplete = () => { db.close(); resolve(); };
    tx.onerror = () => reject(tx.error);
  });
}

/** 从 IndexedDB 删除私钥。 */
async function idbDelete(): Promise<void> {
  const db = await openIdb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readwrite');
    const req = tx.objectStore(IDB_STORE).delete(IDB_KEY);
    req.onerror = () => reject(req.error);
    tx.oncomplete = () => { db.close(); resolve(); };
    tx.onerror = () => reject(tx.error);
  });
}

// ── IndexedDB 可用性检测 ─────────────────────────────────────────────────────

/** SSR / Node / Tauri-webview 若无 IndexedDB 则返回 false。 */
function hasIdb(): boolean {
  return typeof indexedDB !== 'undefined' && indexedDB !== null;
}

// ── 核心：懒加载 + 记忆化私钥 ───────────────────────────────────────────────

/**
 * 获取（或初始化）持久化私钥。
 * - 优先从内存缓存取（模块生命周期内只读库一次）。
 * - 其次从 IndexedDB 加载；若不存在则生成新密钥并持久化。
 * - 若 IndexedDB 不可用（SSR/Node/测试），退化为仅内存密钥并 warn。
 */
async function getOrCreatePrivKey(): Promise<Uint8Array> {
  // 内存命中
  if (cachedPriv !== null) return cachedPriv;

  if (!hasIdb()) {
    // SSR / Node 降级：生成一次性内存密钥，重载后会重新生成（可接受）
    console.warn('[controllerIdentity] IndexedDB 不可用，使用仅内存临时密钥（重载后失效）');
    cachedPriv = ed25519.utils.randomSecretKey();
    return cachedPriv;
  }

  try {
    // 尝试从 IndexedDB 加载
    let priv = await idbLoad();
    if (priv === null) {
      // 首次：生成并持久化
      priv = ed25519.utils.randomSecretKey();
      await idbSave(priv);
    }
    cachedPriv = priv;
    return cachedPriv;
  } catch (err) {
    // IndexedDB 打开/读写失败 → 内存降级
    console.warn('[controllerIdentity] IndexedDB 访问失败，使用仅内存临时密钥：', err);
    if (cachedPriv === null) {
      cachedPriv = ed25519.utils.randomSecretKey();
    }
    return cachedPriv;
  }
}

// ── 公开 API ─────────────────────────────────────────────────────────────────

/**
 * 返回控制端的 Ed25519 公钥（32 字节）。
 * 首次调用会触发密钥的生成或从 IndexedDB 加载。
 */
export async function getControllerPub(): Promise<Uint8Array> {
  const priv = await getOrCreatePrivKey();
  return ed25519.getPublicKey(priv);
}

/**
 * 用控制端持久化私钥对 message 签名，返回 64 字节 Ed25519 签名。
 * 签名可由宿主侧用 `ed25519.verify(sig, message, pub)` 验证（@noble 互验）。
 */
export async function signTrust(message: Uint8Array): Promise<Uint8Array> {
  const priv = await getOrCreatePrivKey();
  return ed25519.sign(message, priv);
}

/**
 * 清除控制端身份：删除 IndexedDB 存储的私钥，并清除内存缓存。
 * 登出时调用，下次调用 getControllerPub 会重新生成新密钥。
 */
export async function clearControllerIdentity(): Promise<void> {
  cachedPriv = null;
  if (!hasIdb()) return;
  try {
    await idbDelete();
  } catch (err) {
    console.warn('[controllerIdentity] 清除 IndexedDB 身份失败：', err);
  }
}

/**
 * 仅供测试使用：重置模块内存缓存（不触碰 IndexedDB）。
 * @internal
 */
export function _resetCacheForTest(): void {
  cachedPriv = null;
}
