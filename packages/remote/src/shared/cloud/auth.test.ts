import { describe, expect, it, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';

const storage: Record<string, string> = {};
const mockApi = {
  BASE_DOMAIN: 'ridge.example',
  setUnauthorizedHandler: vi.fn(),
  login: vi.fn(),
  session: vi.fn(),
  getMe: vi.fn(),
  forgotPassword: vi.fn(),
  resetPassword: vi.fn(),
  checkin: vi.fn(),
  activateKey: vi.fn(),
  deviceCode: vi.fn(),
  deviceActivate: vi.fn(),
  devicePoll: vi.fn(),
  authRequest: vi.fn(),
  authPoll: vi.fn(),
  ApiError: class ApiError extends Error {
    constructor(public code: string, message: string) { super(message); }
  },
};

vi.mock('./apiClient', () => mockApi);
vi.mock('./controllerIdentity', () => ({ clearControllerIdentity: vi.fn(async () => undefined) }));

vi.stubGlobal('window', {
  localStorage: {
    getItem: (key: string) => storage[key] ?? null,
    setItem: (key: string, value: string) => { storage[key] = value; },
    removeItem: (key: string) => { delete storage[key]; },
  },
  open: vi.fn(),
});

const auth = await import('./auth');

const user = {
  username: 'alice',
  plan: 'free',
  premiumActive: false,
  checkedInToday: false,
};

beforeEach(() => {
  for (const key of Object.keys(storage)) delete storage[key];
  mockApi.login.mockReset();
  mockApi.session.mockReset();
  mockApi.getMe.mockReset();
  mockApi.forgotPassword.mockReset();
  mockApi.resetPassword.mockReset();
  mockApi.checkin.mockReset();
  mockApi.activateKey.mockReset();
  mockApi.deviceCode.mockReset();
  mockApi.deviceActivate.mockReset();
  mockApi.devicePoll.mockReset();
  mockApi.authRequest.mockReset();
  mockApi.authPoll.mockReset();
  auth.cloudAuth.set({ userToken: null, user: null, deviceToken: null, deviceName: null });
});

describe('cloud auth state predicates and persistence', () => {
  it('reads safe defaults, derives account flags, and builds public entry domains', () => {
    const empty = auth.snapshot();
    expect(empty).toEqual({ userToken: null, user: null, deviceToken: null, deviceName: null });
    const state = { userToken: 'u', user: { ...user, plan: 'premium', premiumActive: true, checkedInToday: true }, deviceToken: 'd', deviceName: 'laptop' };
    expect(auth.isLoggedIn(state)).toBe(true);
    expect(auth.hasActiveTime(state)).toBe(true);
    expect(auth.isPremium(state)).toBe(true);
    expect(auth.hasCheckedInToday(state)).toBe(true);
    expect(auth.publicEntryDomain(state)).toBe('laptop-alice.ridge.example');
    expect(auth.publicEntryDomain({ ...state, deviceName: null })).toBeNull();
  });

  it('logs in, bootstraps cookie sessions, refreshes once, and logs out cleanly', async () => {
    mockApi.login.mockResolvedValue({ token: 'user-token', user });
    await expect(auth.login('a@example.com', 'pw')).resolves.toMatchObject({ userToken: 'user-token' });
    expect(storage['ridge.cloud.userToken']).toBe('user-token');
    expect(JSON.parse(storage['ridge.cloud.user'])).toEqual(user);

    mockApi.session.mockResolvedValue({ token: 'cookie-token', user: { ...user, premiumActive: true } });
    expect(await auth.bootstrapFromCookie()).toBe(true);
    expect(auth.snapshot().userToken).toBe('cookie-token');
    mockApi.session.mockRejectedValueOnce(new Error('offline'));
    expect(await auth.bootstrapFromCookie()).toBe(false);

    mockApi.getMe.mockResolvedValue({ user: { ...user, plan: 'premium' } });
    expect(await auth.refreshMe()).toMatchObject({ user: { plan: 'premium' } });
    auth.logout();
    expect(auth.snapshot()).toEqual({ userToken: null, user: null, deviceToken: null, deviceName: null });
  });

  it('coalesces access refresh and handles password/check-in/key flows', async () => {
    auth.cloudAuth.set({ userToken: 'u', user, deviceToken: null, deviceName: null });
    let release!: () => void;
    mockApi.session.mockImplementation(() => new Promise((resolve) => { release = () => resolve({ token: 'fresh', user }); }));
    const first = auth.refreshAccess();
    const second = auth.refreshAccess();
    expect(first).toBe(second);
    release();
    expect(await first).toBe(true);

    await auth.forgotPassword('a@example.com');
    mockApi.resetPassword.mockResolvedValue({ token: 'reset', user });
    await expect(auth.resetPassword('a@example.com', '123', 'new-pw')).resolves.toMatchObject({ userToken: 'reset' });
    mockApi.checkin.mockResolvedValue({ reason: 'granted', premiumExpiresAt: 123 });
    mockApi.getMe.mockResolvedValue({ user: { ...user, premiumActive: true } });
    await expect(auth.checkin()).resolves.toMatchObject({ reason: 'granted' });
    mockApi.activateKey.mockResolvedValue({ token: 'key-token', user: { ...user, plan: 'premium' } });
    await expect(auth.activateKey('KEY', 'alice')).resolves.toMatchObject({ userToken: 'key-token' });
    expect(mockApi.forgotPassword).toHaveBeenCalledWith('a@example.com');
  });

  it('rejects protected calls without user token and activates a device with progress', async () => {
    await expect(auth.refreshMe()).rejects.toMatchObject({ code: 'UNAUTHORIZED' });
    await expect(auth.checkin()).rejects.toMatchObject({ code: 'UNAUTHORIZED' });
    await expect(auth.activateKey('KEY')).rejects.toMatchObject({ code: 'UNAUTHORIZED' });

    auth.cloudAuth.set({ userToken: 'u', user, deviceToken: null, deviceName: null });
    storage['ridge.cloud.userToken'] = 'u';
    mockApi.deviceCode.mockResolvedValue({ pairing_code: 'PAIR', poll_token: 'POLL', expires_in: 60 });
    mockApi.deviceActivate.mockResolvedValue(undefined);
    mockApi.devicePoll.mockResolvedValue({ status: 'bound', token: 'device-token', device_name: 'laptop' });
    const progress: unknown[] = [];
    await expect(auth.activateThisDevice('laptop', (value) => progress.push(value))).resolves.toMatchObject({ deviceToken: 'device-token', deviceName: 'laptop' });
    expect(progress).toEqual([{ pairingCode: 'PAIR', expiresIn: 60 }]);
    expect(mockApi.deviceActivate).toHaveBeenCalledWith('u', 'PAIR', 'laptop');
    expect(get(auth.cloudAuth).deviceName).toBe('laptop');
  });

  it('completes browser login through a wake-up poll and rejects expired authorization', async () => {
    mockApi.authRequest.mockResolvedValue({
      request_code: 'REQ', poll_token: 'POLL', authorize_url: 'https://ridge.example/auth', expires_in: 60, interval: 1,
    });
    let wake!: () => void;
    mockApi.authPoll
      .mockImplementationOnce(async () => {
        setTimeout(() => wake?.(), 0);
        return { status: 'pending' };
      })
      .mockResolvedValueOnce({ status: 'approved', token: 'browser-token', user });
    const progress: unknown[] = [];
    await expect(auth.loginViaBrowser({
      onProgress: (value) => progress.push(value),
      onWake: (callback) => { wake = callback; return () => { wake = undefined!; }; },
    })).resolves.toMatchObject({ userToken: 'browser-token' });
    expect(progress).toEqual([{ authorizeUrl: 'https://ridge.example/auth', requestCode: 'REQ' }]);

    mockApi.authRequest.mockResolvedValue({
      request_code: 'REQ2', poll_token: 'POLL2', authorize_url: 'https://ridge.example/auth2', expires_in: 60, interval: 1,
    });
    mockApi.authPoll.mockResolvedValue({ status: 'expired' });
    await expect(auth.loginViaBrowser()).rejects.toMatchObject({ code: 'AUTH_REQUEST_EXPIRED' });
  });

  it('fills a missing cached username from device binding and handles malformed persisted user data', async () => {
    storage['ridge.cloud.user'] = '{malformed';
    expect(auth.snapshot().user).toBeNull();
    auth.cloudAuth.set({ userToken: 'u', user: { ...user, username: null }, deviceToken: null, deviceName: null });
    storage['ridge.cloud.userToken'] = 'u';
    mockApi.deviceCode.mockResolvedValue({ pairing_code: 'PAIR', poll_token: 'POLL', expires_in: 60 });
    mockApi.deviceActivate.mockResolvedValue(undefined);
    mockApi.devicePoll.mockResolvedValue({ status: 'bound', token: 'device-token', device_name: 'laptop', username: 'bound-user' });
    await expect(auth.activateThisDevice('laptop')).resolves.toMatchObject({
      deviceToken: 'device-token',
      user: { username: 'bound-user' },
    });
  });
});
