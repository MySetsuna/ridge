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
		isTauri: vi.fn(),
		listen: vi.fn(),
		snapshot: vi.fn(),
		construct,
		RidgeCloudHost: FakeRidgeCloudHost,
	};
});

vi.mock('@tauri-apps/api/core', () => ({
	invoke: mocks.invoke,
	isTauri: mocks.isTauri,
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('$lib/i18n', () => ({ tr: (key: string) => key }));
vi.mock('@ridge/remote/shared/cloud/auth', () => ({ snapshot: mocks.snapshot }));
vi.mock('@ridge/remote/shared/cloud/ridgeCloudProvider', () => ({
	RidgeCloudHost: mocks.RidgeCloudHost,
}));
vi.mock('@ridge/remote/shared/cloud/cloudHostBridge', () => ({ CloudHostBridge: vi.fn() }));
vi.mock('@ridge/remote/shared/cloud/cloudHostPaneSource', () => ({
	makeCloudHostPaneSource: vi.fn(),
}));
vi.mock('@ridge/remote/shared/cloud/apiClient', () => ({
	verifyWorkspaceShareAccess: vi.fn(),
}));

import {
	blacklistController,
	cloudSessions,
	hostError,
	hostState,
	isHostOnline,
	kickController,
	goOffline,
	goOnline,
} from './cloudHostStore';

describe('cloudHostStore lifecycle', () => {
	beforeEach(() => {
		mocks.invoke.mockReset();
		mocks.isTauri.mockReset();
		mocks.snapshot.mockReset();
		mocks.construct.mockClear();
		for (const method of Object.values(mocks.host)) method.mockReset();
		mocks.isTauri.mockReturnValue(true);
		mocks.snapshot.mockReturnValue({});
		hostState.set('offline');
		hostError.set('');
		cloudSessions.set([]);
	});

	it('fails closed when desktop credentials are absent', async () => {
		await goOnline();
		expect(get(hostError)).toBe('cloud.errDeviceNotActivated');
		expect(mocks.invoke).not.toHaveBeenCalled();
	});

	it('starts and stops the detached desktop host', async () => {
		mocks.snapshot.mockReturnValue({
			deviceToken: 'token',
			deviceName: 'desktop',
			user: { username: 'alice' },
		});
		mocks.invoke.mockResolvedValue(undefined);

		await goOnline();
		expect(mocks.invoke).toHaveBeenNthCalledWith(1, 'sync_cloud_remote_credentials', {
			deviceToken: 'token',
			deviceName: 'desktop',
			username: 'alice',
		});
		expect(mocks.invoke).toHaveBeenNthCalledWith(2, 'ensure_cloud_remote_host');
		expect(isHostOnline()).toBe(true);

		await goOffline();
		expect(mocks.invoke).toHaveBeenLastCalledWith('disable_cloud_remote_host');
		expect(isHostOnline()).toBe(false);
	});

	it('surfaces desktop startup and shutdown failures', async () => {
		mocks.snapshot.mockReturnValue({
			deviceToken: 'token',
			deviceName: 'desktop',
			user: { username: 'alice' },
		});
		mocks.invoke.mockRejectedValue(new Error('offline'));

		await goOnline();
		expect(get(hostError)).toBe('offline');
		await goOffline();
		expect(get(hostError)).toBe('offline');
	});

	it('keeps browser host ownership outside panel mount lifecycle', async () => {
		mocks.isTauri.mockReturnValue(false);
		mocks.snapshot.mockReturnValue({
			deviceToken: 'token',
			deviceName: 'browser',
			user: { username: 'alice' },
		});
		mocks.invoke.mockResolvedValue([1, 2, 3]);
		mocks.host.goOnline.mockResolvedValue(undefined);

		await goOnline();
		expect(mocks.construct).toHaveBeenCalledOnce();
		expect(mocks.host.goOnline).toHaveBeenCalledWith('browser');
		kickController('controller-1');
		blacklistController('controller-2');
		expect(mocks.host.kick).toHaveBeenCalledWith('controller-1');
		expect(mocks.host.blacklist).toHaveBeenCalledWith('controller-2');

		await goOffline();
		expect(mocks.host.goOffline).toHaveBeenCalledOnce();
		expect(mocks.invoke).toHaveBeenLastCalledWith('set_cloud_remote_active', { active: false });
	});
});
