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
// pubkey`）。本模块只产出 B3 在线判定所需的**纯函数**（decideKeyBinding），
// 由双 provider 在握手/信令公钥到达时调用。
//
// 兼容（D9 能力协商）：仅当双方都公告 `e2ee-bind` 能力时才启用严格比对
// （`enabled=true`）；否则回落 relay-trust v1（`enabled=false` → 放行），避免老
// controller 回归。

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

// makeKeyBindingVerifier / KeyBindingOptions 已随 §5.5 桥钩子退役删除（S1-F5，
// iteration 8 G4）：其目标场景由下方 decideKeyBinding（B3 在线判定）+ 0x02 帧覆盖。

/**
 * 一个连接的 B3 绑定模式（诊断/测试可读）。
 * - `pending`：尚未判定。
 * - `enforced`：收到信令旁路公钥并比对一致（严格绑定生效）。
 * - `relay-trust`：宽限期内未收到信令公钥 → 回落 v1（对端疑为旧端）。
 */
export type KeyBindingMode = 'pending' | 'enforced' | 'relay-trust';

/** {@link decideKeyBinding} 的三态判定。 */
export type KeyBindingDecision =
  | 'accept' // 校验通过（或回落 relay-trust）：可标记 connected
  | 'reject' // 检测到 MITM：握手公钥 ≠ 信令旁路公钥 → 必须断开
  | 'wait'; //  信令旁路公钥尚未到达且宽限期未过 → 暂缓决定（既不放行也不拒绝）

/**
 * B3 在线判定：用「**信令旁路确认是否到达**」作启用门，而非 D9 `$/hello` 能力位。
 *
 * 为何不用 `$/hello`：`$/hello` 在 E2EE 握手**完成后**才由 L2 发出（见 rpcClient/
 * bridge），那时连接已 `connected`，太晚——无法在握手时据它决定是否拒绝。改用更稳健
 * 的**信令公钥到达性**：DataChannel 网络中间人**无法**篡改另一条已认证 TLS 信令上转发
 * 的公钥，故"收到了信令公钥"即可强制比对；"宽限期内未收到"暂缓；"宽限期过仍未收到"判
 * 定对端为不发信令公钥的旧端 → 回落 relay-trust（向后兼容）。DataChannel MITM 无法
 * 借此回落逃逸——它无法阻止信令公钥经独立 TLS 通道到达。
 *
 * @param handshakePub   E2EE DataChannel 握手帧里的对端公钥（0x01||pub32 解出）。
 * @param signalingPub   cloud 经已认证信令转发回来的对端公钥；尚未到达为 null。
 * @param graceExpired   宽限期是否已过（仍未收到 signalingPub 时用于回落判定）。
 */
export function decideKeyBinding(
  handshakePub: Uint8Array,
  signalingPub: Uint8Array | null,
  graceExpired: boolean,
): KeyBindingDecision {
  if (signalingPub) {
    if (signalingPub.length !== PUBKEY_LEN || handshakePub.length !== PUBKEY_LEN) {
      return 'reject'; // 防御性：任一公钥长度非法 → 拒绝
    }
    return constantTimeEqual(handshakePub, signalingPub) ? 'accept' : 'reject';
  }
  // 尚无信令公钥：宽限期内等待；过期则回落 relay-trust（对端疑为旧端，不发信令公钥）。
  return graceExpired ? 'accept' : 'wait';
}
