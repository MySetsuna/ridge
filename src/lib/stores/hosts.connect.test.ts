import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const harness = vi.hoisted(() => {
	class FakeRemoteConnection {
		static mode: 'connected' | 'error' = 'connected';
		static instances: FakeRemoteConnection[] = [];
		private currentState: 'disconnected' | 'connected' | 'error' = 'disconnected';
		private listeners = new Set<(state: string) => void>();

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
		lastFailure = vi.fn(() => ({ message: 'fake LAN rejection' }));
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
		subscribePane = vi.fn();
		sendStdin = vi.fn();
		refreshPane = vi.fn();
		getPaneOutput = vi.fn(() => []);
	}

	const cloudConnect = vi.fn(async () => {
		const link = new FakeRemoteConnection();
		link.connect();
		return link;
	});
	const invoke = vi.fn(async (command: string) => {
		if (command === 'list_native_sessions' || command === 'host_list_snapshot') return [];
		return undefined;
	});
	return { FakeRemoteConnection, cloudConnect, invoke };
});

vi.mock('@tauri-apps/api/core', () => ({
	invoke: harness.invoke,
	isTauri: () => false,
}));

vi.mock('@ridge/remote', () => ({ RemoteConnection: harness.FakeRemoteConnection }));
vi.mock('$lib/remote/cloud/cloudHostTopologyLink', () => ({
	connectCloudHostTopologyLink: harness.cloudConnect,
}));

const hosts = await import('./hosts');

beforeEach(() => {
	harness.FakeRemoteConnection.mode = 'connected';
	harness.FakeRemoteConnection.instances.length = 0;
	harness.cloudConnect.mockClear();
	harness.invoke.mockClear();
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
		expect(harness.FakeRemoteConnection.instances[0]?.connect).toHaveBeenCalledWith(
		'127.0.0.1', 9527, '123456', 'code', true,
		);
		original();
	});

	it('rejects a LAN transport error and leaves an actionable progress state', async () => {
		harness.FakeRemoteConnection.mode = 'error';

		await expect(hosts.connectHost('remote', 'LAN', '127.0.0.1:9527', '123456', 'lan'))
			.rejects.toThrow('fake LAN rejection');
		expect(get(hosts.hostConnectProgress)).toEqual({
			phase: 'error', label: 'LAN', detail: 'fake LAN rejection',
		});
		expect(get(hosts.hostsStore)).toEqual([]);
	});

	it('connects Cloud host through the same topology projection and progress contract', async () => {
		await hosts.connectHost('rdg', '', 'device-a', '654321', 'public');

		expect(harness.cloudConnect).toHaveBeenCalledWith('device-a', '654321');
		const host = get(hosts.hostsStore).find((item) => item.id === 'cloud:device-a');
		expect(host).toMatchObject({
			kind: 'rdg', status: 'connected', label: 'device-a',
			detail: '公网同账号 · Cloud E2EE',
		});
		expect(host?.sessions[0]).toMatchObject({ remoteSessionId: 'pane-remote', workspaceId: 'workspace-remote' });
		expect(get(hosts.hostConnectProgress)).toBeNull();
	});
});
