import { describe, it, expect, beforeEach, vi } from 'vitest';
import { getOrCreateCli, _resetCliCacheForTest } from './controllerInstanceId';

// CI 环境差异钉子（iter-60 R-TESTGATE 实证）：Node ≤24 无全局 sessionStorage/
// Storage（Node 25 默认有 → 本地绿、CI 红）。补最小内存 stub，行为与浏览器
// sessionStorage 对齐（getItem 缺省 null；Storage.prototype 供 spyOn）。
if (typeof globalThis.sessionStorage === 'undefined') {
  class MemStorage {
    private m = new Map<string, string>();
    getItem(k: string): string | null { return this.m.has(k) ? this.m.get(k)! : null; }
    setItem(k: string, v: string): void { this.m.set(k, String(v)); }
    removeItem(k: string): void { this.m.delete(k); }
    clear(): void { this.m.clear(); }
    key(i: number): string | null { return [...this.m.keys()][i] ?? null; }
    get length(): number { return this.m.size; }
  }
  (globalThis as Record<string, unknown>).Storage = MemStorage;
  (globalThis as Record<string, unknown>).sessionStorage = new MemStorage();
}

describe('controllerInstanceId', () => {
  beforeEach(() => {
    _resetCliCacheForTest();
    sessionStorage.clear();
  });

  it('生成稳定 cli：同一会话多次调用返回同值', () => {
    const a = getOrCreateCli();
    const b = getOrCreateCli();
    expect(a).toBe(b);
    expect(a).toMatch(/^[A-Za-z0-9._-]{1,64}$/);
  });

  it('持久化到 sessionStorage：重置内存缓存后从 sessionStorage 复原同值', () => {
    const first = getOrCreateCli();
    _resetCliCacheForTest(); // 模拟同标签页刷新（sessionStorage 保留）
    const second = getOrCreateCli();
    expect(second).toBe(first);
  });

  it('sessionStorage 不可用时回退内存，仍返回稳定 cli', () => {
    const spy = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const a = getOrCreateCli();
    const b = getOrCreateCli();
    expect(a).toBe(b);
    expect(a).toMatch(/^[A-Za-z0-9._-]{1,64}$/);
    spy.mockRestore();
  });
});
