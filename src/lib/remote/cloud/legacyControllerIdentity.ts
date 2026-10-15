// Native migration reads existing browser storage only; it never creates a key.
function seed(value: unknown): Uint8Array {
  if (!(value instanceof Uint8Array) || value.byteLength !== 32) throw new Error('旧版 Controller 私钥格式无效，原数据保留');
  return value;
}
async function readIdb(): Promise<Uint8Array | null> {
  if (typeof indexedDB === 'undefined' || typeof indexedDB.databases !== 'function') throw new Error('无法核对旧版 Controller 身份库，原数据保留');
  if (!(await indexedDB.databases()).some(database => database.name === 'ridge-cloud')) return null;
  return new Promise((resolve, reject) => {
    let db: IDBDatabase | undefined;
    let settled = false;
    const finish = (value: Uint8Array | null, error?: Error) => {
      if (settled) return;
      settled = true; clearTimeout(timeout); db?.close();
      if (error) reject(error); else resolve(value);
    };
    const timeout = setTimeout(() => finish(null, new Error('读取旧版 Controller 身份超时，原数据保留')), 5000);
    let request: IDBOpenDBRequest;
    try { request = indexedDB.open('ridge-cloud'); }
    catch { finish(null, new Error('无法读取旧版 Controller 身份，原数据保留')); return; }
    // Deletion between databases() and open() must not create a replacement DB.
    request.onupgradeneeded = () => { request.transaction?.abort(); };
    request.onblocked = request.onerror = () => finish(null, new Error('无法读取旧版 Controller 身份，原数据保留'));
    request.onsuccess = () => {
      db = request.result;
      if (settled) { db.close(); return; }
      if (!db.objectStoreNames.contains('identity')) { finish(null); return; }
      try {
        const transaction = db.transaction('identity', 'readonly');
        const read = transaction.objectStore('identity').get('controller-ed25519-priv');
        let value: Uint8Array | null = null;
        read.onsuccess = () => {
          try { if (read.result !== undefined) value = seed(read.result); }
          catch { finish(null, new Error('旧版 Controller 私钥格式无效，原数据保留')); }
        };
        transaction.oncomplete = () => finish(value);
        transaction.onerror = transaction.onabort = () => finish(null, new Error('读取旧版 Controller 身份失败，原数据保留'));
      } catch { finish(null, new Error('读取旧版 Controller 身份失败，原数据保留')); }
    };
  });
}
export async function readLegacyControllerSeed(): Promise<number[] | null> {
  const encoded = localStorage.getItem('ridge_ctrl_identity');
  let local: Uint8Array | null = null;
  if (encoded !== null) {
    try {
      if (encoded.length > 128) throw new Error();
      local = seed(Uint8Array.from(atob(encoded), character => character.charCodeAt(0)));
    } catch { throw new Error('旧版 Controller 私钥格式无效，原数据保留'); }
  }
  const stored = await readIdb();
  if (stored && local && !stored.every((byte, index) => byte === local![index])) throw new Error('旧版 Controller 身份冲突，原数据保留');
  const existing = stored ?? local;
  return existing ? Array.from(existing) : null;
}
