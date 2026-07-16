// signaling/conformance.test.ts — TS 侧跨语言信令对照（设计 §4，决策 B）。
//
// 读 vendored golden fixtures（来源 ridge-signaling，由 `pnpm sync:signaling` 落地），逐条与
// 一张**强类型字面量表**双向对照，把线形钉死：camelCase（`peerPresent`）、`cid` 取舍、
// kebab tag（`peer-join`/`peer-leave`/`e2ee-pubkey`）、`ice` 的 `candidate:null`。
//
// 字面量表标注为 `SignalMsg`：ridge-signaling 改 schema 重新 vendor 后——字段改名 → **本文件
// 编译报错**；值/形状变更 → **测试失败**。这是 CI 零依赖那一半（不需要同级 ridge-signaling
// 在场，对照的是 vendored 副本）；另一半「在场即比对源」见 drift.test.ts。

import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { parseSignal, isInboundSignal } from './index';
import type { SignalMsg } from './index';

const FIXTURES_DIR = join(dirname(fileURLToPath(import.meta.url)), 'fixtures');

/** 读一条 fixture 的原始文本（保留以便 JSON.parse + 序列化双向对照）。 */
function readFixture(name: string): string {
  return readFileSync(join(FIXTURES_DIR, `${name}.json`), 'utf8');
}

/** 列出 fixtures/ 下全部 fixture 基名（去 .json）。 */
function fixtureBaseNames(): string[] {
  return readdirSync(FIXTURES_DIR)
    .filter((f) => f.endsWith('.json'))
    .map((f) => f.replace(/\.json$/, ''))
    .sort();
}

// `unknown_forward_compat` 不在字面量表（它本就不是已知变体），单独断言。
const UNKNOWN_FIXTURE = 'unknown_forward_compat';

/**
 * 强类型字面量表：fixture 基名 → 期望的 `SignalMsg`。标注为 `SignalMsg`，故任一字面量与
 * 生成类型不符即编译报错（schema drift 的第一道闸）。
 */
const TYPED: Readonly<Record<string, SignalMsg>> = {
  answer_cid: { t: 'answer', sdp: 'v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\n', cid: 'cK7MQ2X9' },
  e2ee_pubkey_cid: {
    t: 'e2ee-pubkey',
    pubkey: 'c2lnbmFsaW5nLXRlc3QtZXBoLXB1YmtleS0zMmI9',
    cid: 'cK7MQ2X9',
  },
  e2ee_pubkey_no_cid: { t: 'e2ee-pubkey', pubkey: 'c2lnbmFsaW5nLXRlc3QtZXBoLXB1YmtleS0zMmI9' },
  error: { t: 'error', code: 'KICKED', message: 'kicked by host' },
  ice_candidate_cid: {
    t: 'ice',
    candidate: {
      candidate: 'candidate:1 1 udp 2122260223 192.168.1.2 54321 typ host',
      sdpMid: '0',
      sdpMLineIndex: 0,
    },
    cid: 'cK7MQ2X9',
  },
  ice_null: { t: 'ice', candidate: null },
  kick: { t: 'kick', cid: 'cK7MQ2X9' },
  offer_cid: { t: 'offer', sdp: 'v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\n', cid: 'cK7MQ2X9' },
  offer_no_cid: { t: 'offer', sdp: 'v=0\r\no=- 0 0 IN IP4 0.0.0.0\r\n' },
  peer_join_controller: { t: 'peer-join', role: 'controller', cid: 'cK7MQ2X9' },
  peer_join_host: { t: 'peer-join', role: 'host' },
  peer_leave_controller: { t: 'peer-leave', role: 'controller', cid: 'cK7MQ2X9' },
  peer_leave_host: { t: 'peer-leave', role: 'host' },
  welcome_controller: {
    t: 'welcome',
    room: 'my-laptop-alice',
    role: 'controller',
    cid: 'cK7MQ2X9',
    peerPresent: true,
  },
  welcome_host: { t: 'welcome', room: 'my-laptop-alice', role: 'host', peerPresent: false },
};

describe('signaling conformance — golden fixtures ↔ generated SignalMsg (双向)', () => {
  for (const [name, typed] of Object.entries(TYPED)) {
    it(`${name}: 解析向 — JSON.parse(fixture) 等于强类型字面量`, () => {
      const parsed = JSON.parse(readFixture(name));
      expect(parsed).toEqual(typed);
    });

    it(`${name}: 序列化向 — JSON.stringify(字面量) 还原回 fixture`, () => {
      expect(JSON.parse(JSON.stringify(typed))).toEqual(JSON.parse(readFixture(name)));
    });

    it(`${name}: parseSignal 识别为已知入站信令并保形`, () => {
      const msg = parseSignal(readFixture(name));
      expect(msg).toEqual(typed);
      // kick 是出站帧，不算入站；其余 14 条都应被 isInboundSignal 接纳。
      expect(isInboundSignal(msg)).toBe(name !== 'kick');
    });
  }

  it('字面量表 key 集合 === fixtures 文件名集合（减 unknown_forward_compat）', () => {
    const onDisk = new Set(fixtureBaseNames());
    const expected = new Set([...Object.keys(TYPED), UNKNOWN_FIXTURE]);
    expect(onDisk).toEqual(expected);
  });
});

describe('signaling conformance — 未知 tag 前向兼容（unknown_forward_compat）', () => {
  it('parseSignal 不抛、保留 t、且不被误判为已知变体', () => {
    const text = readFixture(UNKNOWN_FIXTURE);
    expect(() => parseSignal(text)).not.toThrow();
    const msg = parseSignal(text);
    expect(msg.t).toBe('future-message-type');
    // 未知 tag 不应被收窄为入站已知变体（绝不误判）。
    expect(isInboundSignal(msg)).toBe(false);
  });

  it('parseSignal 对非法 JSON / 空串也不抛（返回可忽略的空 tag）', () => {
    expect(parseSignal('').t).toBe('');
    expect(parseSignal('not json').t).toBe('');
    expect(parseSignal('123').t).toBe('');
    expect(isInboundSignal(parseSignal(''))).toBe(false);
  });
});
