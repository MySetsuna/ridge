import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const harness = vi.hoisted(() => ({
	invoke: vi.fn(),
	snapshot: vi.fn(),
	listSharedWithMe: vi.fn(),
	listWorkspaceShares: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({
	invoke: harness.invoke,
	isTauri: () => false,
}));

vi.mock('@ridge/remote/shared/cloud/auth', () => ({
	snapshot: harness.snapshot,
}));

vi.mock('@ridge/remote/shared/cloud/apiClient', () => ({
	acceptWorkspaceShare: vi.fn(),
	listSharedWithMe: harness.listSharedWithMe,
	listWorkspaceShares: harness.listWorkspaceShares,
	revokeWorkspaceShare: vi.fn(),
}));

const hosts = await import('./hosts');

beforeEach(() => {
	harness.invoke.mockReset();
	harness.snapshot.mockReset();
	harness.listSharedWithMe.mockReset();
	harness.listWorkspaceShares.mockReset();
	harness.snapshot.mockReturnValue({ userToken: null, user: null, deviceToken: null, deviceName: null });
	harness.invoke.mockResolvedValue([]);
	harness.listSharedWithMe.mockResolvedValue({ shares: [] });
	harness.listWorkspaceShares.mockResolvedValue({ shares: [] });
	hosts.hostsStore.set([]);
	hosts.hostsError.set('');
	hosts.hostsLoading.set(false);
});

describe('hosts.refreshHosts source aggregation', () => {
	it('keeps remote records when native enumeration fails and projects cloud shares', async () => {
		harness.invoke.mockImplementation(async (command: string) => {
			if (command === 'list_native_sessions') throw new Error('native unavailable');
			if (command === 'host_list_snapshot') {
				return [{
					id: 'lan:one', kind: 'remote', label: 'LAN one', addr: '127.0.0.1',
					status: 'connected', detail: 'online',
					sessions: [{ id: 'pane-1', title: 'Terminal', attached: true }],
				}];
			}
			return [];
		});
		harness.snapshot.mockReturnValue({ userToken: 'user-token', user: null, deviceToken: null, deviceName: null });
		harness.listSharedWithMe.mockResolvedValue({ shares: [
			{
				id: 'grant-pending', ownerUserId: 'owner-1', ownerUsername: 'Alice', deviceId: 'device-1',
				deviceName: 'laptop', workspaceId: 'workspace-pending', granteeUserId: 'me',
				granteeEmail: 'me@example.com', role: 'operator', status: 'pending', createdAt: 'now',
			},
			{
				id: 'grant-active', ownerUserId: 'owner-1', ownerUsername: 'Alice', deviceId: 'device-1',
				deviceName: 'laptop', workspaceId: 'workspace-active', granteeUserId: 'me',
				granteeEmail: 'me@example.com', role: 'operator', status: 'active', createdAt: 'now',
			},
			{
				id: 'grant-revoked', ownerUserId: 'owner-2', deviceId: 'device-2',
				deviceName: 'old-laptop', workspaceId: 'workspace-revoked', granteeUserId: 'me',
				granteeEmail: 'me@example.com', role: 'operator', status: 'revoked', createdAt: 'now',
			},
		] });
		harness.listWorkspaceShares.mockResolvedValue({ shares: [
			{
				id: 'outgoing-active', ownerUserId: 'me', deviceId: 'device-local', deviceName: 'desktop',
				workspaceId: 'workspace-local', granteeUserId: 'guest', granteeUsername: 'Bob',
				granteeEmail: 'bob@example.com', role: 'operator', status: 'active', createdAt: 'now',
			},
			{
				id: 'outgoing-revoked', ownerUserId: 'me', deviceId: 'device-local', deviceName: 'desktop',
				workspaceId: 'workspace-old', granteeUserId: 'guest', granteeEmail: 'bob@example.com',
				role: 'operator', status: 'revoked', createdAt: 'now',
			},
		] });

		await hosts.refreshHosts();

		const projected = get(hosts.hostsStore);
		expect(projected).toHaveLength(3);
		expect(projected.find((host) => host.id === 'lan:one')).toMatchObject({
			status: 'connected',
			sessions: [{ remoteSessionId: 'pane-1', name: 'Terminal' }],
		});
		const shared = projected.find((host) => host.id === 'shared:owner-1:laptop');
		expect(shared).toMatchObject({ kind: 'shared', status: 'connected' });
		expect(shared?.sessions).toEqual(expect.arrayContaining([
			expect.objectContaining({ shareGrantId: 'grant-pending', attached: false, windows: 1 }),
			expect.objectContaining({ shareGrantId: 'grant-active', attached: false, windows: 1 }),
		]));
		expect(projected.find((host) => host.id === 'sharing:outgoing')).toMatchObject({
			kind: 'sharing',
			sessions: [expect.objectContaining({ shareGrantId: 'outgoing-active', granteeLabel: 'Bob' })],
		});
		expect(get(hosts.hostsError)).toContain('native unavailable');
		expect(get(hosts.hostsLoading)).toBe(false);
	});

	it('does not replace the snapshot with a stale generation after a slow refresh', async () => {
		let releaseFirst!: () => void;
		const first = new Promise<never>((_, reject) => {
			releaseFirst = () => reject(new Error('first refresh superseded'));
		});
		let calls = 0;
		harness.invoke.mockImplementation(async (command: string) => {
			if (command === 'list_native_sessions') {
				calls += 1;
				if (calls === 1) return first;
				return [{ socket: 'tmux', name: 'new', windows: 1, panes: 1, width: 80, height: 24, attached: false }];
			}
			return [];
		});

		const stale = hosts.refreshHosts();
		const fresh = hosts.refreshHosts();
		releaseFirst();
		await Promise.all([stale, fresh]);

		expect(get(hosts.hostsStore)).toEqual([expect.objectContaining({
			id: 'headless',
			sessions: [expect.objectContaining({ name: 'new' })],
		})]);
	});
});
