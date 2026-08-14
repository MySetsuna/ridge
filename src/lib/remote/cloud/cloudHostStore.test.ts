import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const mocks = vi.hoisted(() => {
  const host = {
    goOnline: vi.fn(),
    goOffline: vi.fn(),
    kick: vi.fn(),
    blacklist: vi.fn(),
  };
  const construct = vi.fn();
  class FakeRidgeCloudHost {
    constructor(...args: unknown[]) {
      construct(...args);
      return host;
    }
  }
  return {
    host,
    invoke: vi.fn(),
    listen: vi.fn(),
    snapshot: vi.fn(),
    construct,
    RidgeCloudHost: FakeRidgeCloudHost,
  };
});

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('$lib/i18n', () => ({ tr: (key: string) => key }));
vi.mock('@ridge/remote/shared/cloud/auth', () => ({ snapshot: mocks.snapshot }));
vi.mock('@ridge/remote/shared/cloud/ridgeCloudProvider', () => ({
  RidgeCloudHost: mocks.RidgeCloudHost,
}));
vi.mock('@ridge/remote/shared/cloud/cloudHostBridge', () => ({
  CloudHostBridge: class FakeCloudHostBridge {},
}));
vi.mock('@ridge/remote/shared/cloud/cloudHostPaneSource', () => ({
  makeCloudHostPaneSource: vi.fn(),
}));
vi.mock('@ridge/remote/shared/cloud/apiClient', () => ({
  verifyWorkspaceShareAccess: vi.fn(),
}));

import {
  cloudSessions,
  goOffline,
  goOnline,
  hostError,
  hostState,
  isHostOnline,
} from './cloudHostStore';

describe('cloudHostStore lifecycle', () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.snapshot.mockReset();
    mocks.construct.mockClear();
    for (const method of Object.values(mocks.host)) method.mockReset();
    mocks.snapshot.mockReturnValue({});
    hostState.set('offline');
    hostError.set('');
    cloudSessions.set([]);
  });

  it('fails closed when device credentials are absent', async () => {
    await goOnline();
    expect(get(hostError)).toBe('cloud.errDeviceNotActivated');
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it('uses Ridge-owned WebRTC host on desktop and browser paths', async () => {
    mocks.snapshot.mockReturnValue({
      deviceToken: 'token',
      deviceName: 'desktop',
      user: { username: 'alice' },
    });
    mocks.invoke.mockImplementation(async (command: string) =>
      command === 'get_device_identity_pub' ? [1, 2, 3] : undefined,
    );
    mocks.host.goOnline.mockImplementation(async () => {
      const callbacks = mocks.construct.mock.calls[0]?.[1] as { onHostState?: (state: string) => void };
      callbacks?.onHostState?.('online');
    });
    mocks.host.goOffline.mockResolvedValue(undefined);

    await goOnline();
    expect(mocks.construct).toHaveBeenCalledOnce();
    expect(mocks.host.goOnline).toHaveBeenCalledWith('desktop');
    expect(get(hostState)).toBe('online');
    expect(mocks.invoke).toHaveBeenCalledWith('set_cloud_remote_active', { active: true });
    const commands = mocks.invoke.mock.calls.map(([command]) => command);
    expect(commands).not.toEqual(expect.arrayContaining([
      'sync_cloud_remote_credentials',
      'ensure_cloud_remote_host',
      'disable_cloud_remote_host',
    ]));

    await goOffline();
    expect(mocks.host.goOffline).toHaveBeenCalledOnce();
    expect(get(hostState)).toBe('offline');
    expect(mocks.invoke).toHaveBeenLastCalledWith('set_cloud_remote_active', { active: false });
  });

  it('surfaces host startup and shutdown failures', async () => {
    mocks.snapshot.mockReturnValue({
      deviceToken: 'token',
      deviceName: 'desktop',
      user: { username: 'alice' },
    });
    mocks.invoke.mockResolvedValue([1, 2, 3]);
    mocks.host.goOnline.mockRejectedValue(new Error('host down'));
    mocks.host.goOffline.mockRejectedValue(new Error('offline'));

    await goOnline();
    expect(get(hostState)).toBe('error');
    expect(get(hostError)).toBe('host down');
    await goOffline();
    expect(get(hostError)).toBe('offline');
  });

  it('keeps controller actions on the shared host instance', async () => {
    mocks.snapshot.mockReturnValue({
      deviceToken: 'token',
      deviceName: 'desktop',
      user: { username: 'alice' },
    });
    mocks.invoke.mockResolvedValue([1, 2, 3]);
    mocks.host.goOnline.mockResolvedValue(undefined);

    await goOnline();
    const { kickController, blacklistController } = await import('./cloudHostStore');
    kickController('controller-1');
    blacklistController('controller-2');
    expect(mocks.host.kick).toHaveBeenCalledWith('controller-1');
    expect(mocks.host.blacklist).toHaveBeenCalledWith('controller-2');
  });
});
