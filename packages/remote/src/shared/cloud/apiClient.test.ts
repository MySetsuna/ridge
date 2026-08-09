import { describe, it, expect, vi, afterEach } from 'vitest';
import * as api from './apiClient';
import { isInsecureCloudDomain, cloudHttpScheme, cloudWsScheme } from './apiClient';

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => tauri);

// 这些是 cloud 连接 scheme 选择的依据：base 域指向本机回环时走明文 http/ws
// （本地自托管 ridge-cloud 无 TLS 反代），真实公网域名恒走 https/wss。

describe('isInsecureCloudDomain', () => {
  it('treats localhost (with/without port) as insecure', () => {
    expect(isInsecureCloudDomain('localhost')).toBe(true);
    expect(isInsecureCloudDomain('localhost:5050')).toBe(true);
    expect(isInsecureCloudDomain('LOCALHOST:443')).toBe(true);
  });

  it('treats *.localhost (tenant subdomain) as insecure', () => {
    // {device}-{username}.localhost 在 Chromium/WebView2 自动解析到 127.0.0.1。
    expect(isInsecureCloudDomain('mylaptop-alice.localhost')).toBe(true);
    expect(isInsecureCloudDomain('mylaptop-alice.localhost:5050')).toBe(true);
  });

  it('treats loopback IPs as insecure', () => {
    expect(isInsecureCloudDomain('127.0.0.1')).toBe(true);
    expect(isInsecureCloudDomain('127.0.0.1:5050')).toBe(true);
    expect(isInsecureCloudDomain('127.1.2.3')).toBe(true);
    expect(isInsecureCloudDomain('0.0.0.0')).toBe(true);
    expect(isInsecureCloudDomain('::1')).toBe(true);
    expect(isInsecureCloudDomain('[::1]')).toBe(true);
  });

  it('treats real public domains as secure', () => {
    expect(isInsecureCloudDomain('9527127.xyz')).toBe(false);
    expect(isInsecureCloudDomain('mylaptop-alice.9527127.xyz')).toBe(false);
    expect(isInsecureCloudDomain('example.com')).toBe(false);
    // 非回环 IP 与「localhost 仅作为子串」不应误判为回环。
    expect(isInsecureCloudDomain('192.168.0.10:5050')).toBe(false);
    expect(isInsecureCloudDomain('notlocalhost.example.com')).toBe(false);
    expect(isInsecureCloudDomain('localhost.evil.com')).toBe(false);
  });
});

describe('cloudHttpScheme / cloudWsScheme', () => {
  // 前提：RIDGE_CLOUD_DEV_PLAINTEXT 未注入（或注入空串）时 DEV_PLAINTEXT=false，下方“默认”用例据此走 TLS。
  it('returns TLS schemes for loopback bases by default (dev TLS)', () => {
    expect(cloudHttpScheme('localhost:5050')).toBe('https');
    expect(cloudWsScheme('localhost:5050')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.localhost:5050')).toBe('wss');
  });

  it('returns TLS schemes for public bases', () => {
    expect(cloudHttpScheme('9527127.xyz')).toBe('https');
    expect(cloudWsScheme('9527127.xyz')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.9527127.xyz')).toBe('wss');
  });

  it('downgrades loopback bases to plaintext when plaintext flag set (escape hatch)', () => {
    expect(cloudHttpScheme('localhost:5050', true)).toBe('http');
    expect(cloudWsScheme('localhost:5050', true)).toBe('ws');
  });

  it('keeps public bases on TLS even with plaintext flag (never downgrade prod)', () => {
    expect(cloudHttpScheme('9527127.xyz', true)).toBe('https');
    expect(cloudWsScheme('9527127.xyz', true)).toBe('wss');
  });
});

describe('cloud API envelope, retry, and public request boundaries', () => {
  afterEach(() => {
    api.setUnauthorizedHandler(null);
    tauri.invoke.mockReset();
    vi.unstubAllGlobals();
  });

  it('routes the public API surface through JSON envelopes with auth and cookie headers', async () => {
    const fetchMock = vi.fn(async () => ({
      status: 200,
      json: async () => ({ ok: true, data: {} }),
    }));
    vi.stubGlobal('fetch', fetchMock);
    await Promise.all([
      api.login('a@example.com', 'pw'),
      api.register('a@example.com', 'pw'),
      api.getMe('user-token'),
      api.setUsername('user-token', 'alice'),
      api.createWorkspaceShare('user-token', { deviceName: 'host', workspaceId: 'ws', grantee: 'bob' }),
      api.listWorkspaceShares('user-token'),
      api.listSharedWithMe('user-token'),
      api.acceptWorkspaceShare('user-token', 'grant/1'),
      api.declineWorkspaceShare('user-token', 'grant/1'),
      api.revokeWorkspaceShare('user-token', 'grant/1'),
      api.getWorkspaceShareToken('user-token', 'grant/1'),
      api.verifyWorkspaceShareAccess('device-token', 'share-token'),
      api.session(),
      api.checkin('user-token'),
      api.activateKey('user-token', 'KEY', 'alice'),
      api.authRequest('desktop'),
      api.authPoll('poll-token'),
      api.deviceCode(),
      api.devicePoll('poll-token'),
      api.deviceActivate('user-token', 'PAIR', 'host'),
      api.listDevices('user-token'),
      api.deleteDevice('user-token', 'host/name'),
      api.forgotPassword('a@example.com'),
      api.resetPassword('a@example.com', '123', 'new-pw'),
      api.getIceServers('user-token'),
    ]);
    expect(fetchMock).toHaveBeenCalledTimes(25);
    expect(fetchMock.mock.calls[2][1]).toMatchObject({
      headers: { Authorization: 'Bearer user-token' },
    });
    expect(fetchMock.mock.calls[12][1]).toMatchObject({ credentials: 'include' });
    expect(fetchMock.mock.calls[7][0]).toContain('/workspace-shares/grant%2F1/accept');
  });

  it('retries one unauthorized bearer request, then fails closed on network and bad envelopes', async () => {
    const fetchMock = vi.fn()
      .mockResolvedValueOnce({ status: 401, json: async () => ({ ok: false, error: { code: 'UNAUTHORIZED', message: 'expired' } }) })
      .mockResolvedValueOnce({ status: 200, json: async () => ({ ok: true, data: { user: {} } }) });
    vi.stubGlobal('fetch', fetchMock);
    api.setUnauthorizedHandler(async () => 'fresh-token');
    await expect(api.getMe('old-token')).resolves.toEqual({ user: {} });
    expect(fetchMock.mock.calls[1][1]).toMatchObject({ headers: { Authorization: 'Bearer fresh-token' } });

    fetchMock.mockRejectedValueOnce(new Error('offline'));
    await expect(api.getMe('fresh-token')).rejects.toMatchObject({ code: 'NETWORK' });
    fetchMock.mockResolvedValueOnce({ status: 502, json: async () => ({ ok: false, error: { code: 'UNKNOWN', message: 'bad gateway' } }) });
    await expect(api.getMe('fresh-token')).rejects.toMatchObject({ code: 'INTERNAL', message: 'bad gateway' });
    fetchMock.mockResolvedValueOnce({ status: 200, json: async () => { throw new Error('not json'); } });
    await expect(api.getMe('fresh-token')).rejects.toMatchObject({ code: 'BAD_RESPONSE' });
  });

  it('uses the Tauri cloud_http proxy and rejects proxy transport or JSON failures', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} });
    tauri.invoke.mockResolvedValueOnce({ status: 200, body: JSON.stringify({ ok: true, data: { user: {} } }) });
    await expect(api.getMe('desktop-token')).resolves.toEqual({ user: {} });
    expect(tauri.invoke).toHaveBeenCalledWith('cloud_http', expect.objectContaining({
      method: 'GET',
      headers: { Authorization: 'Bearer desktop-token' },
      body: null,
    }));

    tauri.invoke.mockRejectedValueOnce(new Error('proxy offline'));
    await expect(api.getMe('desktop-token')).rejects.toMatchObject({ code: 'NETWORK', message: 'proxy offline' });
    tauri.invoke.mockResolvedValueOnce({ status: 502, body: '{broken' });
    await expect(api.getMe('desktop-token')).rejects.toMatchObject({ code: 'BAD_RESPONSE' });
  });
});
