// Ridge Cloud — E2EE 公钥↔身份绑定的**校验核心**（D-GM-10 / B3）。
//
// 设计见 docs/plans/d-gm-10-e2ee-key-binding-design.md。这是该安全特性里**可在
// wind 单仓库自主实现并单测**的一半：host/controller 在 E2EE 握手收到对端临时
// 公钥后，用本模块判定「该公钥是否 == cloud 经**已认证信令**旁路转发回来的对端
// 公钥」。不一致 = 检测到 relay 层主动 MITM（给两端各发了攻击者自己的公钥），
// 拒绝会话。
//
// 威胁模型（与设计 §2 一致）：把"任何能转发 E2EE 握手的中间人都能 MITM"收窄到
// "须同时攻陷已认证 TLS 信令 + 持双方 deviceJWT 认证态"。完全攻陷 cloud auth 后端
// 不在防护内（既定边界）。
//
// 本模块**不**自己取"对端信令公钥"——那来自 ridge-cloud 的信令转发（`e2ee-peer-
// pubkey`，须先改契约 protocol.md §7，见设计 §3/§5，当前被用户的 protocol.md WIP
// 阻塞）。本模块只产出 `cloudHostBridge` 既有的 `KeyBindingVerifier` 钩子所需的
// **纯判定**，由 boot 层在拿到信令公钥后注入 `expectedPeerPublicKey`。
//
// 兼容（D9 能力协商）：仅当双方都公告 `e2ee-bind` 能力时才启用严格比对
// （`enabled=true`）；否则回落 relay-trust v1（`enabled=false` → 放行），避免老
// controller 回归。

import type { KeyBindingVerifier } from './cloudHostBridge';
import { PUBKEY_LEN } from './e2ee';

/**
 * 恒定时间比较两个字节数组是否相等（防计时侧信道）。长度不同立即返回 false
 * （长度本身非秘密）。等长时不提前退出，逐字节累积差异。
 */
export function constantTimeEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a[i] ^ b[i];
  }
  return diff === 0;
}

/** 构造 {@link makeKeyBindingVerifier} 的入参。 */
export interface KeyBindingOptions {
  /**
   * 是否启用严格绑定校验。仅当 D9 `$/hello` 双方都协商出 `e2ee-bind` 能力时为
   * true；否则 false（回落 relay-trust v1，放行——向后兼容）。
   */
  enabled: boolean;
  /**
   * cloud 经**已认证信令**转发回来的对端临时公钥（`e2ee-peer-pubkey`）。启用绑定
   * 但此值缺失 = 绑定被要求却拿不到旁路确认 → **fail-closed**（拒绝），不静默放行。
   */
  expectedPeerPublicKey: Uint8Array | null;
}

/**
 * 产出一个 `cloudHostBridge` 的 `KeyBindingVerifier`：把 E2EE 握手收到的对端公钥
 * 与信令旁路确认的对端公钥做恒定时间比对。
 *
 * 判定（与设计 §2/§3 一致）：
 *   - `enabled=false`（未协商 e2ee-bind）→ **放行**（relay-trust v1，兼容老端）。
 *   - `enabled=true` 且 `expectedPeerPublicKey` 缺失 → **拒绝**（fail-closed）。
 *   - `enabled=true` 且长度非法的握手公钥 → **拒绝**（防御性）。
 *   - `enabled=true` 且两公钥恒定时间相等 → **放行**；不等 → **拒绝**（MITM）。
 */
export function makeKeyBindingVerifier(opts: KeyBindingOptions): KeyBindingVerifier {
  const { enabled, expectedPeerPublicKey } = opts;
  return (peerPublicKey: Uint8Array): boolean => {
    if (!enabled) return true; // e2ee-bind 未协商：relay-trust v1
    if (!expectedPeerPublicKey) return false; // 要求绑定却无旁路确认 → fail-closed
    if (peerPublicKey.length !== PUBKEY_LEN) return false; // 防御性：非法长度
    return constantTimeEqual(peerPublicKey, expectedPeerPublicKey);
  };
}
