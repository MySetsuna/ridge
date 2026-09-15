import { describe, expect, it, vi } from 'vitest';
import {
  enqueuePtyInput,
  enqueuePtyWrite,
  PtyWriteQueueFullError,
  PtyWriteQueueRetiredError,
  retirePtyWriteQueue,
  retirePtyWriteQueuesForPane,
} from './ptyWriteQueue';

describe('enqueuePtyWrite', () => {
	it('coalesces an optional short burst before the first IPC write', async () => {
		vi.useFakeTimers();
		try {
			const sent: string[] = [];
			const write = async (data: string) => { sent.push(data); };
			expect(enqueuePtyInput('ws:burst', '\x03', write, { coalesceWindowMs: 8 })).toBe(true);
			expect(enqueuePtyInput('ws:burst', '\x03', write, { coalesceWindowMs: 8 })).toBe(true);
			expect(sent).toEqual([]);
			await vi.advanceTimersByTimeAsync(8);
			expect(sent).toEqual(['\x03\x03']);
			retirePtyWriteQueue('ws:burst');
		} finally {
			vi.useRealTimers();
		}
	});

	it('coalesces keys queued behind a slow desktop write', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const sent: string[] = [];
    const write = async (data: string) => {
      sent.push(data);
      if (data === 'a') await gate;
    };

    expect(enqueuePtyInput('ws:input', 'a', write)).toBe(true);
    expect(enqueuePtyInput('ws:input', 'b', write)).toBe(true);
    expect(enqueuePtyInput('ws:input', 'c', write)).toBe(true);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sent).toEqual(['a']);
    release();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(sent).toEqual(['a', 'bc']);
    retirePtyWriteQueue('ws:input');
  });

  it('bounds coalesced input bytes', () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    // 同一实例同一闭包：mountToken 保持同一对象身份；三次 enqueue 累计到 lane.queued
    // 超过 maxQueuedBytes=2 时第三次返回 false。
    const inst = { tag: 'cap' };
    const sameWrite = async (data: string) => { await gate; };
    expect(enqueuePtyInput('ws:input-cap', 'a', sameWrite, { maxQueuedBytes: 2, mountToken: inst })).toBe(true);
    expect(enqueuePtyInput('ws:input-cap', 'b', sameWrite, { maxQueuedBytes: 2, mountToken: inst })).toBe(true);
    expect(enqueuePtyInput('ws:input-cap', 'c', sameWrite, { maxQueuedBytes: 2, mountToken: inst })).toBe(false);
    release();
    retirePtyWriteQueue('ws:input-cap');
  });

  it('keeps multiline paste before later input for the same pane', async () => {
    const sent: string[] = [];
    let releasePaste!: () => void;
    const pasteGate = new Promise<void>((resolve) => { releasePaste = resolve; });
    const paste = enqueuePtyWrite('ws:pane', async () => {
      await pasteGate;
      sent.push('one\ntwo\nthree');
    });
    const key = enqueuePtyWrite('ws:pane', async () => { sent.push('x'); });

    await Promise.resolve();
    await Promise.resolve();
    expect(sent).toEqual([]);
    releasePaste();
    await Promise.all([paste, key]);
    expect(sent).toEqual(['one\ntwo\nthree', 'x']);
  });

  it('does not strand later input after a write failure', async () => {
    const sent: string[] = [];
    await expect(enqueuePtyWrite('ws:failure', async () => { throw new Error('closed'); })).rejects.toThrow('closed');
    await enqueuePtyWrite('ws:failure', async () => { sent.push('retry'); });
    expect(sent).toEqual(['retry']);
  });

  it('bounds pending operations before a slow write can grow memory', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const first = enqueuePtyWrite('ws:bounded', () => gate, { maxPending: 2 });
    const second = enqueuePtyWrite('ws:bounded', async () => undefined, { maxPending: 2 });

    await expect(
      enqueuePtyWrite('ws:bounded', async () => undefined, { maxPending: 2 }),
    ).rejects.toBeInstanceOf(PtyWriteQueueFullError);
    release();
    await Promise.all([first, second]);
  });

  it('retires queued writes when the pane closes', async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const sent: string[] = [];
    const first = enqueuePtyWrite('ws:retire', async () => {
      await gate;
      sent.push('first');
    });
    const queued = enqueuePtyWrite('ws:retire', async () => {
      sent.push('stale');
    });

    await Promise.resolve();
    await Promise.resolve();
    retirePtyWriteQueue('ws:retire');
    release();
    await first;
    await expect(queued).rejects.toBeInstanceOf(PtyWriteQueueRetiredError);
    expect(sent).toEqual(['first']);
  });
});

// §B 代际保护 (Goal #1): 旧挂起输入不得跟随新 props 发给新终端。
// Svelte 组件 unmount→remount 之间，mountToken（每实例唯一对象身份）变化，
// 新实例必须从空 lane 开始；同一实例内 mountToken 共享，合并 coalesce 窗口。
describe('B — enqueuePtyInput generation guard (unmount→remount race)', () => {
  it('A→B→A: mountToken 变化 → 旧 lane 清空，新 lane 独立', async () => {
    const oldSent: string[] = [];
    const newSent: string[] = [];
    // 旧组件 enqueue 1 字节，不 drain
    const instA = { tag: 'A' };
    expect(enqueuePtyInput('ws:race', 'OLD', async (d) => { oldSent.push(d); }, { mountToken: instA })).toBe(true);
    // 新组件 remount：相同 key 但 mountToken 是新对象身份
    const instB = { tag: 'B' };
    expect(enqueuePtyInput('ws:race', 'NEW', async (d) => { newSent.push(d); }, { mountToken: instB })).toBe(true);
    // 触发 drain：应只看到 NEW；OLD 字节被 generation 守卫丢弃
    await new Promise((r) => setTimeout(r, 0));
    expect(newSent).toEqual(['NEW']);
    expect(oldSent).toEqual([]);
    retirePtyWriteQueue('ws:race');
  });

  it('A→A: mountToken 不变 → 同一 lane 合并，bytes 保留', async () => {
    // 模拟同一实例内多次 enqueue（mountToken 同一对象身份）
    const sent: string[] = [];
    const inst = { tag: 'same' };
    expect(enqueuePtyInput('ws:same-inst', 'a', async (d) => { sent.push(d); }, { mountToken: inst, coalesceWindowMs: 4 })).toBe(true);
    expect(enqueuePtyInput('ws:same-inst', 'b', async (d) => { sent.push(d); }, { mountToken: inst, coalesceWindowMs: 4 })).toBe(true);
    expect(enqueuePtyInput('ws:same-inst', 'c', async (d) => { sent.push(d); }, { mountToken: inst, coalesceWindowMs: 4 })).toBe(true);
    // 等 coalesce 窗口 + drain 完成（多 flush 几次微任务）
    await new Promise((r) => setTimeout(r, 10));
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect(sent).toEqual(['abc']);
    retirePtyWriteQueue('ws:same-inst');
  });

  it('旧响应晚到：drain 期间 mountToken 变 → 旧 bytes 不再 flush 到新闭包', async () => {
    const newSent: string[] = [];
    // 实例 A 写入
    const instA = { tag: 'A' };
    let release!: () => void;
    const gate = new Promise<void>((r) => { release = r; });
    const oldWrite = async (d: string) => { await gate; };
    expect(enqueuePtyInput('ws:late', 'A', oldWrite, { mountToken: instA })).toBe(true);
    await new Promise((r) => setTimeout(r, 0));
    // 实例 B remount → 旧 lane 清理
    const instB = { tag: 'B' };
    expect(enqueuePtyInput('ws:late', 'B', async (d) => { newSent.push(d); }, { mountToken: instB })).toBe(true);
    await new Promise((r) => setTimeout(r, 0));
    release();
    await new Promise((r) => setTimeout(r, 0));
    expect(newSent).toEqual(['B']);
    retirePtyWriteQueue('ws:late');
  });

  it('重连同时切换：retirePtyWriteQueuesForPane 硬清，再 enqueue → 旧挂起清空', async () => {
    const sentA: string[] = [];
    const instA = { tag: 'A' };
    expect(enqueuePtyInput('ws:recon:foo', 'pre', async (d) => { sentA.push(d); }, { mountToken: instA })).toBe(true);
    // 模拟重连：cleanupPaneRuntime 路径
    retirePtyWriteQueuesForPane('foo');
    const sentB: string[] = [];
    const instB = { tag: 'B' };
    expect(enqueuePtyInput('ws:recon:foo', 'post', async (d) => { sentB.push(d); }, { mountToken: instB })).toBe(true);
    await new Promise((r) => setTimeout(r, 0));
    expect(sentB).toEqual(['post']);
    expect(sentA).toEqual([]); // retire 后旧 lane 不再被 drain 触发
  });

  it('write 闭包身份变化但 mountToken 相同 → 不触发 generation guard（合法快键）', async () => {
    // 关键场景：RidgePane 每次 keystroke 都新建 arrow function (`(data) => invoke(...)`)。
    // 如果用 write 身份作为信号，合法连击会被误判成 remount。mountToken 是真正的代次。
    //
    // 期望：drain 在 enqueue1 同步启动；enqueue1 data 已快照为 'a'，drain iter 1 发
    // 'a'；iter 2 看到 enqueue2+3 在 await 期间累计的 'bc'。两次 IPC 写都跑到
    // 同一 PTY（mountToken 不变 → lane 不被换）。新闭包能被调用（不是写时快照
    // 的旧写）。
    const sent: string[] = [];
    const inst = { tag: 'instance' };
    expect(enqueuePtyInput('ws:fn-identity', 'a', async (d) => { sent.push(d); }, { mountToken: inst })).toBe(true);
    // 不同箭头函数，相同 mountToken
    expect(enqueuePtyInput('ws:fn-identity', 'b', async (d) => { sent.push(d); }, { mountToken: inst })).toBe(true);
    expect(enqueuePtyInput('ws:fn-identity', 'c', async (d) => { sent.push(d); }, { mountToken: inst })).toBe(true);
    await new Promise((r) => setTimeout(r, 0));
    for (let i = 0; i < 5; i++) await Promise.resolve();
    // 关键断言：内容合并 = 'a' + 'bc' = 'abc'；闭包全 3 个都被调用过。
    expect(sent.join('')).toBe('abc');
    expect(sent.length).toBeGreaterThanOrEqual(1);
    retirePtyWriteQueue('ws:fn-identity');
  });
});
