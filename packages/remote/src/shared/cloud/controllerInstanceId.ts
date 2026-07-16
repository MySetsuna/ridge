// controllerInstanceId.ts
// 控制端「实例 id」(cli，契约 §5.3)：每标签页一个，用于信令同实例顶替快速重连。
//
// 与 controllerIdentity.ts(跨会话持久 Ed25519 身份)语义不同：cli 是**标签页级**实例标识，
// 存 sessionStorage(刷新保留、新标签页独立)，仅用于「让换网/刷新的新连接顶替自己的旧连接」。
// 非鉴权凭据：服务端只做卫生校验，房间隔离由 user_id + token 保证。

const SS_KEY = 'ridge_cli';

/** 模块级内存缓存：sessionStorage 不可用(SSR/隐私模式)时的唯一存储。 */
let cached: string | null = null;

/** 生成新的 cli：优先 crypto.randomUUID()，回退到时间+随机的 base36（仅在无 crypto 时）。 */
function genCli(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
  } catch {
    /* fallthrough */
  }
  // 极端回退：字符集仍落在 [A-Za-z0-9._-]，长度 ≤ 64。
  return `c-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
}

/**
 * 取/建本标签页的 cli：
 * - 内存命中直接返回；
 * - 否则从 sessionStorage 读；不存在则生成并写回；
 * - sessionStorage 读写抛错(隐私模式/SSR)时回退纯内存(本次加载内稳定)。
 */
export function getOrCreateCli(): string {
  if (cached !== null) return cached;
  try {
    const existing = sessionStorage.getItem(SS_KEY);
    if (existing) {
      cached = existing;
      return cached;
    }
    const fresh = genCli();
    sessionStorage.setItem(SS_KEY, fresh);
    cached = fresh;
    return cached;
  } catch {
    if (cached === null) cached = genCli();
    return cached;
  }
}

/** 仅供测试：清空内存缓存(不触碰 sessionStorage)。 */
export function _resetCliCacheForTest(): void {
  cached = null;
}
