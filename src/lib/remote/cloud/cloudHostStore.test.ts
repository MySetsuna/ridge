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
	const cloudHostBridge = vi.fn();
	const makeCloudHostPaneSource = vi.fn();
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
		cloudHostBridge,
		makeCloudHostPaneSource,
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
vi.mock('@ridge/remote/shared/cloud/cloudHostBridge', () => ({
	CloudHostBridge: class FakeCloudHostBridge {
		constructor(options: unknown) {
			mocks.cloudHostBridge(options);
		}
	},
}));
vi.mock('@ridge/remote/shared/cloud/cloudHostPaneSource', () => ({
	makeCloudHostPaneSource: mocks.makeCloudHostPaneSource,
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
		mocks.cloudHostBridge.mockReset();
		mocks.makeCloudHostPaneSource.mockReset();
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

	it('fails closed for inactive browser credentials and host startup failure', async () => {
		mocks.isTauri.mockReturnValue(false);
		mocks.snapshot.mockReturnValue({});
		await goOnline();
		expect(get(hostError)).toBe('cloud.errDeviceNotActivated');

		mocks.snapshot.mockReturnValue({
			deviceToken: 'token',
			deviceName: 'browser',
			user: { username: 'alice' },
		});
		mocks.invoke.mockRejectedValue(new Error('identity unavailable'));
		mocks.host.goOnline.mockRejectedValue(new Error('host down'));
		await goOnline();
		expect(get(hostError)).toBe('host down');
	});

	it('projects host callbacks and enforces scoped bridge operations', async () => {
		vi.resetModules();
		const store = await import('./cloudHostStore');
		mocks.isTauri.mockReturnValue(false);
		mocks.snapshot.mockReturnValue({
			deviceToken: 'token',
			deviceName: 'browser',
			user: { username: 'alice' },
		});
		mocks.invoke.mockImplementation(async (command: string) => {
			if (command === 'get_pane_layout_for') return { type: 'leaf', id: 'pane-1', cwd: '/workspace' };
			if (command === 'get_pane_cwd') return '/workspace';
			if (command === 'find_git_repo_root') return '/workspace';
			if (command === 'get_device_identity_pub') return [1, 2, 3];
			return undefined;
		});
		mocks.host.goOnline.mockResolvedValue(undefined);
		mocks.cloudHostBridge.mockReturnValue({ marker: true });
		const paneSource = vi.fn(() => () => {});
		mocks.makeCloudHostPaneSource.mockReturnValue(paneSource);
		const handlers = new Map<string, (event: { payload: unknown }) => void>();
		const unlisten = vi.fn();
		mocks.listen.mockImplementation(async (name: string, handler: (event: { payload: unknown }) => void) => {
			handlers.set(name, handler);
			return unlisten;
		});

		await store.goOnline();
		const callbacks = mocks.construct.mock.calls[0][1] as {
			onHostState: (state: 'offline' | 'connecting' | 'online' | 'error') => void;
			onSessions: (sessions: unknown[]) => void;
			onError: (message: string) => void;
			createBridge: (cid: string, send: () => void, transcript: Uint8Array | null, scope: unknown) => unknown;
		};
		callbacks.onHostState('online');
		callbacks.onSessions([{ cid: 'c1' }]);
		callbacks.onError('callback error');
		expect(get(store.hostState)).toBe('online');
		expect(get(store.cloudSessions)).toEqual([{ cid: 'c1' }]);
		expect(get(store.hostError)).toBe('callback error');

		callbacks.createBridge('plain', vi.fn(), new Uint8Array([1]), null);
		const plainOptions = mocks.cloudHostBridge.mock.calls[0][0] as Record<string, any>;
		expect(plainOptions.preauthorized).toBe(false);
		expect(plainOptions.paneOutputSource).toBe(paneSource);
		await expect(plainOptions.totpVerifier('123456')).resolves.toBeUndefined();

		const scope = {
			grantId: 'grant',
			granteeUserId: 'guest',
			ownerUserId: 'owner',
			deviceName: 'browser',
			workspaceId: 'shared',
			role: 'operator',
			delegable: false,
		};
		callbacks.createBridge('scoped', vi.fn(), new Uint8Array([2]), scope);
		const scopedOptions = mocks.cloudHostBridge.mock.calls[1][0] as Record<string, any>;
		expect(scopedOptions.preauthorized).toBe(true);
		expect(typeof scopedOptions.paneOutputSource).toBe('function');
		expect(mocks.makeCloudHostPaneSource).toHaveBeenCalledTimes(2);
		await expect(scopedOptions.invoke('get_active_workspace_id')).resolves.toBe('shared');
		await expect(scopedOptions.invoke('get_pane_layout', {})).resolves.toMatchObject({ id: 'pane-1' });
		await expect(scopedOptions.invoke('create_workspace', {})).rejects.toMatchObject({ code: -32003 });

		const events: unknown[] = [];
		const stop = scopedOptions.hostEventSource((name: string, payload: unknown) => events.push([name, payload]));
		await Promise.resolve();
		handlers.get('pane-meta-changed')?.({ payload: { workspaceId: 'shared', paneId: 'pane-1' } });
		handlers.get('pane-tree-changed')?.({ payload: { workspaceId: 'other' } });
		expect(events).toEqual([['pane-meta-changed', { workspaceId: 'shared', paneId: 'pane-1' }]]);
		stop();
		expect(unlisten).toHaveBeenCalledTimes(2);
	});
});
