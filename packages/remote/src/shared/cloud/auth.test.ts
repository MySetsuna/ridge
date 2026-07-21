import { describe, it, expect, vi, afterEach } from 'vitest';
import { get } from 'svelte/store';
import { bootstrapFromCookie, cloudAuth } from './auth';

// 父域 cookie bootstrap（设计 2026-06-12-cloud-domain-sso）：子域调 GET /auth/session
// （credentials:'include' 带父域 ridge_sso cookie）换短 access token，免重登。
// 走真实 apiClient.request 的 §2 信封解包路径，仅 stub 全局 fetch。

afterEach(() => {
  vi.unstubAllGlobals();
  cloudAuth.set({ userToken: null, user: null, deviceToken: null, deviceName: null });
});

describe('bootstrapFromCookie', () => {
  it('session 命中 → 写入 access token + user，返回 true，且以 credentials:include 调 /auth/session', async () => {
    const user = { id: 'u1', email: 'e@x', username: 'jack', plan: 'free', devices: [] };
    const fetchMock = vi.fn().mockResolvedValue({
      status: 200,
      json: async () => ({ ok: true, data: { token: 'ACCESS', user } }),
    });
    vi.stubGlobal('fetch', fetchMock);

    const okk = await bootstrapFromCookie();

    expect(okk).toBe(true);
    expect(get(cloudAuth).userToken).toBe('ACCESS');
    expect(get(cloudAuth).user?.username).toBe('jack');
    // 必须带父域 cookie（credentials:'include'）调 /auth/session。
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(String(url)).toContain('/auth/session');
    expect(init).toMatchObject({ credentials: 'include' });
  });

  it('后端 401（无 cookie / 失效）→ 返回 false，不改登录态', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({
        status: 401,
        json: async () => ({ ok: false, error: { code: 'UNAUTHORIZED', message: '无效会话' } }),
      }),
    );
    expect(await bootstrapFromCookie()).toBe(false);
    expect(get(cloudAuth).userToken).toBeNull();
  });

  it('网络异常 → 返回 false', async () => {
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new Error('boom')));
    expect(await bootstrapFromCookie()).toBe(false);
  });
});
