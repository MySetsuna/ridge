import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  mount: vi.fn(() => ({ type: 'app' })),
  registerSW: vi.fn(),
  setHostPorts: vi.fn(),
  applyUpdate: vi.fn(async () => {}),
}));

vi.mock('svelte', () => ({ mount: mocks.mount }));
vi.mock('./App.svelte', () => ({ default: {} }));
vi.mock('virtual:pwa-register', () => ({ registerSW: mocks.registerSW }));
vi.mock('@ridge/remote/shared/terminal/fontStack', () => ({ REMOTE_TERM_FONT: 'remote-mono' }));
vi.mock('@ridge/remote/shared/terminal/manager', () => ({
  TerminalManager: { setHostPorts: mocks.setHostPorts },
}));

const state = vi.hoisted(() => ({
  visibility: 'visible' as DocumentVisibilityState,
  visibilityHandler: undefined as (() => void) | undefined,
  messageHandler: undefined as ((event: any) => void) | undefined,
  reload: vi.fn(),
  register: vi.fn(async () => ({ scope: '/' })),
  swAddEventListener: vi.fn(),
  clearLocal: vi.fn(),
  clearSession: vi.fn(),
  deleteDatabase: vi.fn(),
  dispatched: [] as Event[],
}));

beforeEach(() => {
  mocks.mount.mockClear();
  mocks.registerSW.mockReset().mockReturnValue(mocks.applyUpdate);
  mocks.setHostPorts.mockClear();
  mocks.applyUpdate.mockClear();
  state.visibility = 'visible';
  state.visibilityHandler = undefined;
  state.messageHandler = undefined;
  state.reload.mockClear();
  state.register.mockClear();
  state.swAddEventListener.mockClear();
  state.clearLocal.mockClear();
  state.clearSession.mockClear();
  state.deleteDatabase.mockClear();
  state.dispatched = [];
});

describe('Remote bootstrap and service-worker recovery', () => {
  it('mounts the app, injects mobile host ports, and marks standalone PWA', async () => {
    const serviceWorker = {
      register: state.register,
      addEventListener: vi.fn((type: string, handler: (event: any) => void) => {
        if (type === 'message') state.messageHandler = handler;
      }),
    };
    const documentMock = {
      visibilityState: state.visibility,
      documentElement: { dataset: {} as Record<string, string> },
      getElementById: vi.fn(() => ({ id: 'app' })),
      addEventListener: vi.fn((type: string, handler: () => void) => {
        if (type === 'visibilitychange') state.visibilityHandler = handler;
      }),
    };
    const windowMock = {
      matchMedia: vi.fn((query: string) => ({ matches: query.includes('standalone') })),
      location: { reload: state.reload },
      dispatchEvent: vi.fn((event: Event) => { state.dispatched.push(event); }),
    };
    const navigatorMock = { standalone: false, serviceWorker };
    vi.stubGlobal('document', documentMock);
    vi.stubGlobal('window', windowMock);
    vi.stubGlobal('navigator', navigatorMock);
    vi.stubGlobal('localStorage', { clear: state.clearLocal });
    vi.stubGlobal('sessionStorage', { clear: state.clearSession });
    vi.stubGlobal('indexedDB', {
      databases: vi.fn(async () => [{ name: 'ridge' }, { name: undefined }]),
      deleteDatabase: state.deleteDatabase,
    });
    vi.stubGlobal('CustomEvent', class CustomEventMock { constructor(public type: string, public init: any) {} });
    vi.stubGlobal('setTimeout', ((callback: () => void) => { callback(); return 1; }) as any);

    await import('./main');
    await new Promise<void>((resolve) => setImmediate(resolve));
    expect(mocks.mount).toHaveBeenCalledWith({}, { target: { id: 'app' } });
    expect(mocks.setHostPorts).toHaveBeenCalledWith(expect.objectContaining({
      settings: expect.objectContaining({ get: expect.any(Function), subscribe: expect.any(Function) }),
      openTextLink: expect.any(Function),
    }));
    expect(documentMock.documentElement.dataset.ridgePwa).toBe('standalone');
    const ports = mocks.setHostPorts.mock.calls[0][0];
    const unsubscribe = ports.settings.subscribe(vi.fn());
    expect(unsubscribe()).toBeUndefined();
    ports.openTextLink('url', { cwd: '/repo', knownCwds: ['/repo'] });
    expect(state.dispatched).toHaveLength(1);

    const options = mocks.registerSW.mock.calls[0][0];
    options.onRegisteredSW('/sw.js', { scope: '/wrong' });
    options.onNeedRefresh();
    state.visibility = 'hidden';
    documentMock.visibilityState = 'hidden';
    state.visibilityHandler?.();
    expect(mocks.applyUpdate).toHaveBeenCalledWith(true);
  });

  it('clears all client stores and reloads after a version mismatch message', async () => {
    const documentMock = {
      visibilityState: 'visible',
      documentElement: { dataset: {} as Record<string, string> },
      getElementById: vi.fn(() => ({ id: 'app' })),
      addEventListener: vi.fn(),
    };
    const serviceWorker = {
      register: state.register,
      addEventListener: vi.fn((type: string, handler: (event: any) => void) => {
        if (type === 'message') state.messageHandler = handler;
      }),
    };
    vi.stubGlobal('document', documentMock);
    vi.stubGlobal('window', { matchMedia: () => ({ matches: false }), location: { reload: state.reload } });
    vi.stubGlobal('navigator', { serviceWorker });
    vi.stubGlobal('localStorage', { clear: state.clearLocal });
    vi.stubGlobal('sessionStorage', { clear: state.clearSession });
    vi.stubGlobal('indexedDB', { databases: vi.fn(async () => [{ name: 'ridge' }]), deleteDatabase: state.deleteDatabase });
    vi.resetModules();
    await import('./main');
    state.messageHandler?.({ data: { type: 'CLEAR_STORAGE', version: 'next' } });
    await Promise.resolve();
    expect(state.clearLocal).toHaveBeenCalled();
    expect(state.clearSession).toHaveBeenCalled();
    expect(state.deleteDatabase).toHaveBeenCalledWith('ridge');
    expect(state.reload).toHaveBeenCalled();
  });
});
