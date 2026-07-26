import { describe, it, expect } from 'vitest';
import { pendingKey } from './wsRemote';

// iter-63 回归钉：LAN 腿的在途请求表原本只按 `responseType` 作键，于是任何两条
// 并发 `invoke-request` 都挤在 `'invoke-result'` 这一个键上，后注册的直接顶掉先
// 注册的——先发那条永远等不到回包（5s 超时抛错），活下来那条还可能收到**另一条
// 命令**的结果。手机端花名册正是三连发（topology / hitlPending / health），因此恒
// 定失败、只显示一个「—」，而后端数据一直是对的。
describe('pendingKey（LAN 在途请求的相关性键）', () => {
  it('同类型不同 _reqId 必须是不同的键 —— 否则并发请求互相顶掉', () => {
    expect(pendingKey('invoke-result', 1)).not.toBe(pendingKey('invoke-result', 2));
  });

  it('同类型同 _reqId 是同一个键 —— 回包才认得领它的那条请求', () => {
    expect(pendingKey('invoke-result', 7)).toBe(pendingKey('invoke-result', 7));
  });

  it('没有 _reqId 的老式请求仍按类型作键（行为逐字不变）', () => {
    for (const missing of [undefined, null, '3', {}]) {
      expect(pendingKey('workspaces', missing)).toBe('workspaces');
    }
  });

  it('不同类型即使 _reqId 相同也不串（两条协议各自的计数器互不相干）', () => {
    expect(pendingKey('invoke-result', 1)).not.toBe(pendingKey('switch-workspace-result', 1));
  });
});
