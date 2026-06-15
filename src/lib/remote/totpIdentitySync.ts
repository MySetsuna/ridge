// §totp-persist：把云登录态实时同步给 Rust 侧 TOTP 种子选择。
//
// 订阅 cloudAuth store，仅在 username **真正变化**时调 `remote_set_totp_identity`
// （登录→账号专属种子；登出→默认种子）。只在真实桌面 host 启用（见 +layout 守卫），
// 绝不在 web-remote controller 跑——否则会把「控制端」的登录态隧道到 host，污染 host 种子。

import type { CloudAuthState } from './cloud/auth';

type InvokeFn = <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type StoreLike = { subscribe: (run: (s: CloudAuthState) => void) => () => void };

/**
 * 启动同步。返回取消订阅函数。
 *
 * `store` 显式传入（而非默认全局）：本模块只 `import type` cloud/auth，绝不在顶层
 * **值导入** 它——否则单测一加载就会触发 auth.ts 的 localStorage 初始化（node 环境
 * 下会抛）。调用方（+layout）传真实 `cloudAuth` store，测试传注入的 writable。
 *
 * @param invoke 真实 Tauri `invoke`（测试可注入 mock）。
 * @param store  cloudAuth store。
 */
export function startTotpIdentitySync(invoke: InvokeFn, store: StoreLike): () => void {
  // undefined 哨兵：确保首次订阅必触发一次（与任何真实 username 都不等）。
  let last: string | null | undefined = undefined;
  return store.subscribe((s) => {
    const username = s.user?.username ?? null;
    if (username === last) return;
    last = username;
    void invoke('remote_set_totp_identity', { username });
  });
}
