// signaling/index.ts — wind 侧信令协议的唯一消费点（对应 Rust `messages.rs` 的 re-export）。
//
// 类型来源是 `generated/`（由 `pnpm sync:signaling` 从 ridge-signaling 的 ts-rs bindings
// vendor 而来，禁止手改）。两个 provider（host / controller）从这里取 `SignalMsg`/`SignalIn`/
// `Role` 并经 {@link parseSignal} 统一处理入站，从此不再手写信令类型。
//
// 漂移由 `drift.test.ts`（同级在场逐字节比对 + SOURCE_REV 校验）+ `conformance.test.ts`
// （fixtures 双向对照）共同钉死。

export type { SignalMsg } from './generated/SignalMsg';
export type { Role } from './generated/Role';
// serde_json::Value 的 TS 形：`ice.candidate` 的线类型。provider 在 WebRTC API 边界处用它收窄。
export type { JsonValue } from './generated/serde_json/JsonValue';

import type { SignalMsg } from './generated/SignalMsg';

/**
 * 两端入站子集：`kick` 是 host→relay 的出站帧（relay 用于踢人），按本协议约定任何一端都
 * 不在信令层「处理」收到的 kick（host 不收 kick；controller 即便收到也忽略，断开交由 RTC/
 * relay 收尾）。因此入站消费统一收窄到去掉 `kick` 的子集。
 */
export type SignalIn = Exclude<SignalMsg, { t: 'kick' }>;

/** 协议已知 tag 全集（含 kick）——SignalMsg 改 schema 时此处不更新会被 conformance 抓到。 */
const KNOWN_SIGNAL_TAGS: ReadonlySet<SignalMsg['t']> = new Set([
  'welcome',
  'peer-join',
  'peer-leave',
  'error',
  'offer',
  'answer',
  'ice',
  'kick',
  'e2ee-pubkey',
]);

/**
 * 把一条信令文本解析为消息对象，**集中处理未知 tag 的前向兼容**（对应 fixture
 * `unknown_forward_compat.json`）：
 *  - 已知 tag → 收窄为 {@link SignalMsg}（字段细节由 relay 权威，此处不做逐字段校验）。
 *  - 未知 tag / 非法 JSON / 非对象 / 无 string `t` → 保留 `{ t }`（未知时为原始 tag，其余为
 *    空串），由调用方静默忽略；**绝不抛**。
 *
 * 入站消费方应配合 {@link isInboundSignal} 收窄到 {@link SignalIn} 后再分派。
 */
export function parseSignal(text: string): SignalMsg | { t: string } {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return { t: '' }; // 非法 JSON：返回可被忽略的空 tag，不抛
  }
  if (typeof raw !== 'object' || raw === null) return { t: '' };
  const t = (raw as { t?: unknown }).t;
  if (typeof t !== 'string') return { t: '' };
  if ((KNOWN_SIGNAL_TAGS as ReadonlySet<string>).has(t)) return raw as SignalMsg;
  return { t }; // 未知 tag：仅保留 tag，调用方忽略（前向兼容）
}

/**
 * 入站类型守卫：仅当 tag 是「已知且非 kick」时为真，并把消息收窄为 {@link SignalIn}。
 * 未知 tag（前向兼容）与 kick（不在入站层处理）都返回 false → 调用方忽略。
 */
export function isInboundSignal(msg: SignalMsg | { t: string }): msg is SignalIn {
  return msg.t !== 'kick' && (KNOWN_SIGNAL_TAGS as ReadonlySet<string>).has(msg.t);
}
