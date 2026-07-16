import { describe, it, expect, vi, afterEach } from 'vitest';
import { getMe, session, setUnauthorizedHandler, ApiError } from './apiClient';

// 401 静默刷新（设计 2026-06-12-cloud-domain-sso Task 8）：带 token 的请求收 401 →
// 调刷新钩子换新 token → 用新 token 重试一次。无 token 的请求不触发（防 /auth/session 递归）。

afterEach(() => {
  vi.unstubAllGlobals();
  setUnauthorizedHandler(null);
});

const env401 = {
  status: 401,
  json: async () => ({ ok: false, error: { code: 'UNAUTHORIZED', message: 'expired' } }),
};

describe('request 401 静默刷新', () => {
  it('带 token 收 401 → 刷新换新 token 重试一次成功', async () => {
    const handler = vi.fn().mockResolvedValue('NEW');
    setUnauthorizedHandler(handler);
    let n = 0;
    const fetchMock = vi.fn().mockImplementation(async () => {
      n++;
      return n === 1
        ? env401
        : { status: 200, json: async () => ({ ok: true, data: { user: { username: 'jack' } } }) };
    });
    vi.stubGlobal('fetch', fetchMock);

    const res = await getMe('OLD');
    expect(res.user.username).toBe('jack');
    expect(handler).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    // 重试请求带新 token。
    const retryInit = fetchMock.mock.calls[1][1] as RequestInit;
    expect((retryInit.headers as Record<string, string>)['Authorization']).toBe('Bearer NEW');
  });

  it('刷新失败（钩子返 null）→ 抛原 401', async () => {
    setUnauthorizedHandler(vi.fn().mockResolvedValue(null));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(env401));
    await expect(getMe('OLD')).rejects.toBeInstanceOf(ApiError);
    await expect(getMe('OLD')).rejects.toMatchObject({ code: 'UNAUTHORIZED' });
  });

  it('无 token 的请求 401 → 不触发刷新（防 /auth/session 自身递归）', async () => {
    const handler = vi.fn().mockResolvedValue('NEW');
    setUnauthorizedHandler(handler);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(env401));
    await expect(session()).rejects.toMatchObject({ code: 'UNAUTHORIZED' });
    expect(handler).not.toHaveBeenCalled();
  });
});
