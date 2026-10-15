import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const harness = vi.hoisted(() => {
	class FakeRemoteConnection {
		static mode: 'connected' | 'error' = 'connected';
		static instances: FakeRemoteConnection[] = [];
		private currentState: 'disconnected' | 'connected' | 'error' = 'disconnected';
		private listeners = new Set<(state: string) => void>();
		static failureCategory: 'user' | 'parked' | 'channel' = 'channel';

		constructor() {
			FakeRemoteConnection.instances.push(this);
		}

		connect = vi.fn(() => {
			if (FakeRemoteConnection.mode === 'error') {
				this.currentState = 'error';
				for (const listener of this.listeners) listener('error');
				return;
			}
			this.currentState = 'connected';
			for (const listener of this.listeners) listener('connected');
		});
		onStateChange = vi.fn((listener: (state: string) => void) => {
			this.listeners.add(listener);
			return () => this.listeners.delete(listener);
		});
		state = vi.fn(() => this.currentState);
		lastFailure = vi.fn(() => ({ category: FakeRemoteConnection.failureCategory, message: 'fake LAN rejection' }));
		disconnect = vi.fn(() => { this.currentState = 'disconnected'; });
		listWorkspaces = vi.fn(async () => ({
			workspaces: [{ id: 'workspace-remote', name: ' Remote ', active: true }],
		}));
		listWorkspacePanes = vi.fn(async () => [{ id: 'pane-remote', title: ' Agent ', cwd: 'C:/repo', isAgent: true }]);
		switchWorkspace = vi.fn(async () => true);
		createWorkspace = vi.fn(async () => 'workspace-created');
		renameWorkspace = vi.fn(async () => true);
		saveWorkspace = vi.fn(async () => true);
		createPane = vi.fn(async () => 'pane-created');
		closePane = vi.fn(async () => true);
		closeWorkspace = vi.fn(async () => true);
		onRawBytes = vi.fn(() => () => undefined);
        onTerminalFrame = vi.fn(() => () => undefined);
		subscribePane = vi.fn();
		sendStdin = vi.fn();
		refreshPane = vi.fn();
		getPaneOutput = vi.fn(() => []);
	}

	const isTauri = { value: false };
	let registered = false;
	const invoke = vi.fn(async (command: string): Promise<unknown> => {
		if (command === 'connect_host') { registered = true; return 'lan:127.0.0.1:9527'; }
		if (command === 'forget_host') registered = false;
		if (command === 'host_list_snapshot') return registered ? [{ id: 'lan:127.0.0.1:9527', kind: 'remote', label: 'LAN', addr: 'https://127.0.0.1:9527', status: FakeRemoteConnection.mode === 'error' ? 'error' : 'connected', detail: 'fake LAN rejection', sessions: [] }] : [];
		if (command === 'list_native_sessions' || command === 'host_list_snapshot') return [];
		return undefined;
	});
	const runtimeConnect = vi.fn(async () => {
        const connection = new FakeRemoteConnection(); connection.connect();
        return { connection, transport: { dispose: vi.fn() } };
    });
    return { FakeRemoteConnection, runtimeConnect, invoke, isTauri };
});

vi.mock('@tauri-apps/api/core', () => ({
	invoke: harness.invoke,
	isTauri: () => harness.isTauri.value,
}));

vi.mock('$lib/transport/runtimeClient', () => ({ connectPresentationRuntime: harness.runtimeConnect }));
vi.mock('$lib/remote/cloud/legacyControllerIdentity', () => ({ readLegacyControllerSeed: vi.fn(async () => null) }));
vi.mock('@ridge/remote/shared/cloud/auth', async importOriginal => ({
  ...await importOriginal<typeof import('@ridge/remote/shared/cloud/auth')>(),
  snapshot: () => ({ userToken: 'owned-controller', user: { username: 'owner' } }),
}));

const hosts = await import('./hosts');

beforeEach(() => {
	harness.FakeRemoteConnection.mode = 'connected';
	harness.FakeRemoteConnection.failureCategory = 'channel';
	harness.FakeRemoteConnection.instances.length = 0;
	harness.runtimeConnect.mockClear();
	harness.invoke.mockClear();
	harness.isTauri.value = false;
	hosts.hostsStore.set([]);
	hosts.hostConnectProgress.set(null);
});

describe('hosts.connectHost transport onboarding', () => {
	it('connects LAN host, publishes topology, and clears progress', async () => {
		const progress: Array<unknown> = [];
		const original = hosts.hostConnectProgress.subscribe((value) => progress.push(value));

		await hosts.connectHost('remote', ' LAN ', 'https://127.0.0.1:9527', ' 123456 ', 'lan');

		const host = get(hosts.hostsStore).find((item) => item.id === 'lan:127.0.0.1:9527');
		expect(host).toMatchObject({
			kind: 'remote', status: 'connected', label: 'LAN', detail: 'wss://127.0.0.1:9527',
		});
		expect(host?.workspaces[0]).toMatchObject({ id: 'workspace-remote', name: 'Remote' });
		expect(host?.sessions[0]).toMatchObject({ remoteSessionId: 'pane-remote', cwd: 'C:/repo', isAgent: true });
		expect(progress.some((value) => (value as { phase?: string } | null)?.phase === 'connecting')).toBe(true);
		expect(progress.some((value) => (value as { phase?: string } | null)?.phase === 'loading-workspaces')).toBe(true);
		expect(get(hosts.hostConnectProgress)).toBeNull();
		expect(harness.invoke).toHaveBeenCalledWith('connect_host', { kind: 'remote', label: ' LAN ', addr: 'https://127.0.0.1:9527', token: '123456' });
        expect(harness.runtimeConnect).toHaveBeenCalledWith(undefined, 'lan:127.0.0.1:9527');
        expect(harness.invoke).not.toHaveBeenCalledWith('register_frontend_host', expect.anything());
		original();
	});

	it('rejects a LAN transport error and leaves an actionable progress state', async () => {
		harness.FakeRemoteConnection.mode = 'error';

		await expect(hosts.connectHost('remote', 'LAN', '127.0.0.1:9527', '123456', 'lan'))
			.rejects.toThrow('fake LAN rejection。若首次连接，请先在默认浏览器打开 https://127.0.0.1:9527/');
		expect(get(hosts.hostConnectProgress)).toEqual({
			phase: 'error',
			label: 'LAN',
			detail: 'fake LAN rejection。若首次连接，请先在默认浏览器打开 https://127.0.0.1:9527/，确认主机指纹并信任 Ridge 证书，再重试。',
		});
		expect(get(hosts.hostsStore)).toEqual([]);
	});

	it('forgets a live Tauri host from kernel persistence before dropping its link', async () => {
		await hosts.connectHost('remote', 'LAN', '127.0.0.1:9527', '123456', 'lan');
		harness.isTauri.value = true;

		await hosts.forgetHost('lan:127.0.0.1:9527');

		expect(harness.invoke).toHaveBeenCalledWith('forget_host', {
			hostId: 'lan:127.0.0.1:9527',
		});
		expect(harness.FakeRemoteConnection.instances[0]?.disconnect).toHaveBeenCalled();
		expect(get(hosts.hostsStore).find((host) => host.id === 'lan:127.0.0.1:9527')).toBeUndefined();
	});

	it('keeps insecure LAN errors free of certificate advice', () => {
		expect(hosts.lanConnectionError('127.0.0.1', 9527, false, 'refused')).toBe('refused');
	});

	it('keeps authentication failures free of certificate advice', () => {
		expect(hosts.lanConnectionError('127.0.0.1', 9527, true, 'bad code', 'user')).toBe('bad code');
	});

	it('connects Cloud host through the same topology projection and progress contract', async () => {
		harness.invoke.mockImplementation(async command => {
			if (command === 'connect_host') return 'cloud:device-a';
			if (command === 'host_list_snapshot') return [{ id: 'cloud:device-a', kind: 'rdg', label: 'device-a', addr: 'device-a', status: 'connected', detail: 'Kernel Cloud E2EE', sessions: [] }];
			return undefined;
		});
		await hosts.connectHost('rdg', '', 'device-a', '654321', 'public');

		expect(harness.invoke).toHaveBeenCalledWith('initialize_cloud_controller_identity', {legacySeed:null});
		const commands = harness.invoke.mock.calls.map(([command]) => command);
		expect(commands.indexOf('initialize_cloud_controller_identity')).toBeLessThan(commands.indexOf('connect_host'));
		expect(harness.runtimeConnect).toHaveBeenCalledWith(undefined, 'cloud:device-a');
		expect(harness.invoke).toHaveBeenCalledWith('connect_host', expect.objectContaining({ kind: 'rdg', addr: 'device-a', cloud: expect.objectContaining({ userToken: 'owned-controller', username: 'owner', deviceName: 'device-a', code: '654321' }) }));
		const host = get(hosts.hostsStore).find((item) => item.id === 'cloud:device-a');
		expect(host).toMatchObject({
			kind: 'rdg', status: 'connected', label: 'device-a',
			detail: 'Kernel Cloud E2EE',
		});
		expect(host?.sessions[0]).toMatchObject({ remoteSessionId: 'pane-remote', workspaceId: 'workspace-remote' });
		expect(get(hosts.hostConnectProgress)).toBeNull();
	});

	it('does not connect or import when the existing controller key cannot be read', async () => {
		const { readLegacyControllerSeed } = await import('$lib/remote/cloud/legacyControllerIdentity');
		vi.mocked(readLegacyControllerSeed).mockRejectedValueOnce(new Error('legacy controller storage unreadable'));
		await expect(hosts.connectHost('rdg', '', 'device-a', '654321', 'public')).rejects.toThrow('legacy controller storage unreadable');
		expect(harness.invoke).not.toHaveBeenCalledWith('initialize_cloud_controller_identity', expect.anything());
		expect(harness.invoke).not.toHaveBeenCalledWith('connect_host', expect.anything());
	});

	it('imports the existing Cloud pin without probing or writing browser storage', async () => {
		const getItem = vi.fn(() => '1111-2222-3333-4444');
		const setItem = vi.fn(() => { throw new Error('read-only browser storage'); });
		vi.stubGlobal('window', { localStorage: { getItem, setItem } });
		harness.invoke.mockImplementation(async command => command === 'connect_host' ? 'cloud:read-only' : command === 'host_list_snapshot' ? [{ id: 'cloud:read-only', kind: 'remote', label: 'read-only', addr: 'read-only', status: 'connected', detail: 'live', sessions: [] }] : []);
		try {
			await hosts.connectHost('remote', '', 'read-only', '654321', 'public');
			expect(getItem).toHaveBeenCalledWith('ridge.cloud.trust.read-only-owner');
			expect(setItem).not.toHaveBeenCalled();
			expect(harness.invoke).toHaveBeenCalledWith('connect_host', expect.objectContaining({ cloud: expect.objectContaining({ legacyFingerprint: '1111-2222-3333-4444' }) }));
		} finally { vi.unstubAllGlobals(); }
	});

	it('refuses empty-pin migration when old browser storage cannot be read', async () => {
		vi.stubGlobal('window', { localStorage: { getItem: () => { throw new Error('unreadable'); } } });
		try {
			await expect(hosts.connectHost('remote', '', 'unreadable', '654321', 'public')).rejects.toThrow('读取旧版 Cloud 设备指纹失败');
			expect(harness.invoke).not.toHaveBeenCalledWith('connect_host', expect.anything());
		} finally { vi.unstubAllGlobals(); }
	});
});


it('restores Kernel-owned host and original foreign view without another TOTP connection', async () => {
  const { workspacePaneTrees } = await import('./paneTree');
  const { remotePaneBinding, unbindRemotePane } = await import('$lib/hosts/remotePaneBindings');
  const id = 'lan:retained-host';
  workspacePaneTrees.set(new Map([['local-workspace', { type: 'leaf', id: 'retained-view',
    origin: { kind: 'remote', host_id: id, host_label: 'Retained', session_id: 'original-remote-pane', workspace_id: 'original-remote-workspace' } }]]));
  harness.invoke.mockImplementation(async command => command === 'host_list_snapshot' ? [{
    id, kind: 'remote', label: 'Retained', addr: 'https://127.0.0.1:9527', status: 'connected', detail: 'live', runtimeManaged: true, sessions: [],
  }] : []);
  await hosts.refreshHosts();
  expect(harness.runtimeConnect).toHaveBeenCalledWith(undefined, id);
  expect(harness.invoke).not.toHaveBeenCalledWith('connect_host', expect.anything());
  expect(remotePaneBinding('retained-view')).toMatchObject({ hostId: id, remotePaneId: 'original-remote-pane', workspaceId: 'original-remote-workspace' });
  const link = remotePaneBinding('retained-view')?.link;
  await hosts.refreshHosts();
  expect(remotePaneBinding('retained-view')?.link).toBe(link);
  expect(harness.runtimeConnect).toHaveBeenCalledTimes(1);
  unbindRemotePane('retained-view');
  workspacePaneTrees.set(new Map());
});
