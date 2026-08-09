import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const harness = vi.hoisted(() => {
  const bridge = {
    attach: vi.fn(),
    detach: vi.fn(),
    invoke: vi.fn(),
  };
  const adapter = {
    connect: vi.fn(async () => {}),
    close: vi.fn(),
    dispose: vi.fn(),
  };
  class FakeDataProvider {
    constructor(readonly call: unknown) {}
  }
  class FakeProvider {
    static instances: FakeProvider[] = [];
    readonly config: unknown;
    readonly callbacks: unknown;
    readonly wakeUp = vi.fn();

    constructor(config: unknown, callbacks: unknown) {
      this.config = config;
      this.callbacks = callbacks;
      FakeProvider.instances.push(this);
    }
  }
  return {
    bridge,
    adapter,
    FakeDataProvider,
    FakeProvider,
    createTransport: vi.fn(),
    setTransport: vi.fn(),
    authSnapshot: vi.fn(),
    refreshAccess: vi.fn(async () => {}),
  };
});

vi.mock('$lib/transport/tauriShim/bridge', () => ({ bridge: harness.bridge }));
vi.mock('$lib/transport', () => ({ setTransport: harness.setTransport }));
vi.mock('$lib/transport/tauri', () => ({ TauriDataProvider: harness.FakeDataProvider }));
vi.mock('@ridge/remote', () => ({ createCloudWebrtcTransportWith: harness.createTransport }));
vi.mock('@ridge/remote/shared/cloud/controllerCloudProvider', () => ({ ControllerCloudProvider: harness.FakeProvider }));
vi.mock('@ridge/remote/shared/cloud/controllerIdentity', () => ({
  getControllerPub: vi.fn(async () => new Uint8Array(32)),
  signTrust: vi.fn(async () => new Uint8Array(64)),
}));
vi.mock('@ridge/remote/shared/cloud/e2ee', () => ({
  computeBindTag: vi.fn(() => new Uint8Array([1, 2, 3])),
  bytesToBase64: vi.fn(() => 'encoded'),
  base64ToBytes: vi.fn(() => null),
}));
vi.mock('@ridge/remote/shared/cloud/auth', () => ({
  cloudAuth: {},
  snapshot: harness.authSnapshot,
  refreshAccess: harness.refreshAccess,
}));

const boot = await import('./cloudControllerBoot');

type EventTargetStub = {
  hidden?: boolean;
  addEventListener: ReturnType<typeof vi.fn>;
  removeEventListener: ReturnType<typeof vi.fn>;
};

function makeEventTarget(hidden = false): EventTargetStub {
  return {
    hidden,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  };
}

describe('cloud controller boot wiring', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    harness.bridge.attach.mockReset();
    harness.bridge.detach.mockReset();
    harness.setTransport.mockReset();
    harness.createTransport.mockReset().mockImplementation((_device, factory) => {
      factory({ onState: vi.fn(), onFrame: vi.fn() });
      return harness.adapter;
    });
    harness.adapter.connect.mockClear();
    harness.adapter.close.mockClear();
    harness.adapter.dispose.mockClear();
    harness.authSnapshot.mockReset().mockReturnValue({
      userToken: 'auth-token',
      user: { username: 'auth-user' },
    });
    harness.refreshAccess.mockClear();
    harness.FakeProvider.instances = [];
  });

  afterEach(() => {
    const active = boot.activeCloudController();
    active?.disconnect();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('wires bridge, transport, callbacks, refresh timer, and foreground wake-up', async () => {
    const documentStub = makeEventTarget(false);
    const windowStub = makeEventTarget();
    vi.stubGlobal('document', documentStub);
    vi.stubGlobal('window', windowStub);

    const states: string[] = [];
    const errors: Array<[string, string | undefined]> = [];
    const handle = boot.startCloudControllerBoot({
      hostDevice: 'host-1',
      onState: (state) => states.push(state),
      onError: (message, code) => errors.push([message, code]),
    }, { useGlobalWorkspace: false });

    expect(handle.hostDevice).toBe('host-1');
    expect(harness.bridge.attach).toHaveBeenCalledWith(harness.adapter, { useGlobalWorkspace: false });
    expect(harness.setTransport).toHaveBeenCalledWith(expect.any(harness.FakeDataProvider));
    expect(harness.adapter.connect).toHaveBeenCalledOnce();
    expect(boot.activeCloudController()).toBe(handle);

    const provider = harness.FakeProvider.instances[0]!;
    const callbacks = provider.callbacks as { onState: (state: string) => void; onError: (message: string, code?: string) => void };
    callbacks.onState('connected');
    callbacks.onError('temporary', 'NETWORK');
    expect(states).toEqual(['connected']);
    expect(errors).toEqual([['temporary', 'NETWORK']]);

    await vi.advanceTimersByTimeAsync(10 * 60 * 1000);
    expect(harness.refreshAccess).toHaveBeenCalledOnce();

    const visibilityHandler = documentStub.addEventListener.mock.calls.find(([name]) => name === 'visibilitychange')?.[1] as (() => void);
    visibilityHandler();
    await Promise.resolve();
    expect(provider.wakeUp).toHaveBeenCalledOnce();

    expect(boot.startCloudControllerBoot({ hostDevice: 'other-host' })).toBe(handle);
    handle.disconnect();
    expect(harness.bridge.detach).toHaveBeenCalledOnce();
    expect(harness.adapter.close).toHaveBeenCalledOnce();
    expect(harness.adapter.dispose).toHaveBeenCalledOnce();
    expect(boot.activeCloudController()).toBeNull();
    expect(documentStub.removeEventListener).toHaveBeenCalledOnce();
    expect(windowStub.removeEventListener).toHaveBeenCalledTimes(3);
  });

  it('supports isolated fixed-token boot without global lifecycle hooks', () => {
    const documentStub = makeEventTarget(false);
    const windowStub = makeEventTarget();
    vi.stubGlobal('document', documentStub);
    vi.stubGlobal('window', windowStub);

    const handle = boot.startCloudControllerBoot({
      hostDevice: 'isolated-host',
      userToken: 'fixed-token',
      username: 'fixed-user',
      fixedToken: true,
    }, { isolated: true, installGlobalTransport: false });

    expect(boot.activeCloudController()).toBeNull();
    expect(harness.setTransport).not.toHaveBeenCalled();
    expect(documentStub.addEventListener).not.toHaveBeenCalled();
    expect(harness.FakeProvider.instances[0]?.config).toMatchObject({
      username: 'fixed-user',
      baseDomain: undefined,
    });
    handle.disconnect();
    expect(harness.bridge.detach).toHaveBeenCalledOnce();
  });
});
