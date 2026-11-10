import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
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

afterEach(() => vi.useRealTimers());

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
  it('does not create credentials after the caller has already cancelled', async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(auth.loginViaBrowser({ signal: controller.signal })).rejects.toMatchObject({ code: 'INVALID_INPUT' });
    await expect(auth.activateThisDevice('laptop', undefined, controller.signal)).rejects.toMatchObject({ code: 'INVALID_INPUT' });
    expect(mockApi.authRequest).not.toHaveBeenCalled();
    expect(mockApi.deviceCode).not.toHaveBeenCalled();
    expect(mockApi.deviceActivate).not.toHaveBeenCalled();
  });

  it('stops before opening the browser or binding a device when progress cancels', async () => {
    mockApi.authRequest.mockResolvedValue({ request_code: 'REQ', poll_token: 'POLL', authorize_url: 'https://ridge.example/auth', expires_in: 60, interval: 2 });
    vi.mocked(window.open).mockClear();
    const login = new AbortController();
    await expect(auth.loginViaBrowser({ signal: login.signal, onProgress: () => login.abort() }))
      .rejects.toMatchObject({ code: 'INVALID_INPUT' });
    expect(window.open).not.toHaveBeenCalled();
    expect(mockApi.authPoll).not.toHaveBeenCalled();

    storage['ridge.cloud.userToken'] = 'u';
    mockApi.deviceCode.mockResolvedValue({ pairing_code: 'PAIR', poll_token: 'POLL', expires_in: 60 });
    const pairing = new AbortController();
    await expect(auth.activateThisDevice('laptop', () => pairing.abort(), pairing.signal))
      .rejects.toMatchObject({ code: 'INVALID_INPUT' });
    expect(mockApi.deviceActivate).not.toHaveBeenCalled();
    expect(mockApi.devicePoll).not.toHaveBeenCalled();
  });

  it('backs off a rate-limited login poll without issuing another authorization request', async () => {
    vi.useFakeTimers();
    mockApi.authRequest.mockResolvedValue({ request_code: 'REQ', poll_token: 'POLL', authorize_url: 'https://ridge.example/auth', expires_in: 60, interval: 2 });
    let firstPoll!: () => void;
    const started = new Promise<void>(resolve => { firstPoll = resolve; });
    mockApi.authPoll.mockImplementationOnce(async () => {
      firstPoll();
      throw new mockApi.ApiError('RATE_LIMITED', 'limited');
    }).mockResolvedValueOnce({ status: 'approved', token: 'recovered', user });
    let wake: (() => void) | undefined;
    const result = auth.loginViaBrowser({ onWake: callback => { wake = callback; return () => {}; } })
      .then(value => ({ value }), error => ({ error }));
    await started;
    await vi.advanceTimersByTimeAsync(0);
    wake?.();
    await vi.advanceTimersByTimeAsync(11_999);
    expect(mockApi.authPoll).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    await expect(result).resolves.toMatchObject({ value: { userToken: 'recovered' } });
    expect(mockApi.authRequest).toHaveBeenCalledTimes(1);
    expect(mockApi.authPoll.mock.calls).toEqual([['POLL'], ['POLL']]);
  });

  it.each(['abort', 'expiry'])('ends a limited login on %s without another poll', async reason => {
    vi.useFakeTimers();
    mockApi.authRequest.mockResolvedValue({ request_code: 'REQ', poll_token: 'POLL', authorize_url: 'https://ridge.example/auth', expires_in: 1, interval: 2 });
    let firstPoll!: () => void;
    const started = new Promise<void>(resolve => { firstPoll = resolve; });
    mockApi.authPoll.mockImplementation(async () => {
      firstPoll();
      throw new mockApi.ApiError('RATE_LIMITED', 'limited');
    });
    const controller = new AbortController();
    const result = auth.loginViaBrowser({ signal: controller.signal })
      .then(value => ({ value }), error => ({ error }));
    await started;
    await vi.advanceTimersByTimeAsync(0);
    if (reason === 'abort') controller.abort();
    else await vi.advanceTimersByTimeAsync(1_000);
    await expect(result).resolves.toMatchObject({ error: { code: reason === 'abort' ? 'INVALID_INPUT' : 'AUTH_REQUEST_EXPIRED' } });
    expect(mockApi.authPoll).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('retries only a denied device poll while preserving the original pairing', async () => {
    vi.useFakeTimers();
    auth.cloudAuth.set({ userToken: 'u', user, deviceToken: null, deviceName: null });
    storage['ridge.cloud.userToken'] = 'u';
    mockApi.deviceCode.mockResolvedValue({ pairing_code: 'PAIR', poll_token: 'POLL', expires_in: 60 });
    mockApi.deviceActivate.mockResolvedValue(undefined);
    let firstPoll!: () => void;
    const started = new Promise<void>(resolve => { firstPoll = resolve; });
    mockApi.devicePoll.mockImplementationOnce(async () => {
      firstPoll();
      throw new mockApi.ApiError('RATE_LIMITED', 'limited');
    }).mockResolvedValueOnce({ status: 'bound', token: 'device-token', device_name: 'laptop' });
    const result = auth.activateThisDevice('laptop').then(value => ({ value }), error => ({ error }));
    await started;
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(11_999);
    expect(mockApi.devicePoll).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    await expect(result).resolves.toMatchObject({ value: { deviceToken: 'device-token' } });
    expect(mockApi.deviceCode).toHaveBeenCalledTimes(1);
    expect(mockApi.deviceActivate).toHaveBeenCalledTimes(1);
    expect(mockApi.devicePoll.mock.calls).toEqual([['POLL'], ['POLL']]);
  });

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

  it('fails device activation on an expired code or an already-aborted signal', async () => {
    auth.cloudAuth.set({ userToken: 'u', user, deviceToken: null, deviceName: null });
    storage['ridge.cloud.userToken'] = 'u';
    mockApi.deviceCode.mockResolvedValue({ pairing_code: 'PAIR', poll_token: 'POLL', expires_in: 60 });
    mockApi.deviceActivate.mockResolvedValue(undefined);
    mockApi.devicePoll.mockResolvedValue({ status: 'expired' });
    await expect(auth.activateThisDevice('laptop')).rejects.toMatchObject({ code: 'PAIRING_EXPIRED' });

    const controller = new AbortController();
    controller.abort();
    await expect(auth.activateThisDevice('laptop', undefined, controller.signal))
      .rejects.toMatchObject({ code: 'INVALID_INPUT' });
  });
});
