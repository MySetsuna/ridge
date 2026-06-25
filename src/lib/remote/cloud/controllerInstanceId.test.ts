import { describe, it, expect, beforeEach, vi } from 'vitest';
import { getOrCreateCli, _resetCliCacheForTest } from './controllerInstanceId';

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
