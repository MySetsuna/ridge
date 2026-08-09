import { beforeEach, describe, expect, it, vi } from 'vitest';

const h = vi.hoisted(() => {
	const state = { userToken: 'user-token', adapterState: 'connected', authState: 'authorized' };
	let stateHandler: (() => void) | undefined;
	let authHandler: (() => void) | undefined;
	let messageHandler: ((message: unknown) => void) | undefined;
	let metadataHandler: ((pane: { paneId: string }, title?: string, cwd?: string) => void) | undefined;
	const adapter = {
		state: () => state.adapterState,
		authState: () => state.authState,
		onStateChange: (cb: () => void) => { stateHandler = cb; return () => { stateHandler = undefined; }; },
		onAuthChange: (cb: () => void) => { authHandler = cb; return () => { authHandler = undefined; }; },
	};
	const handle = { adapter, disconnect: vi.fn() };
	const connection = {
		notifyState: vi.fn(),
		notifyError: vi.fn(),
		init: vi.fn(),
		listWorkspaces: vi.fn(async () => ({ workspaces: [{ id: 'workspace-1', name: 'Remote workspace' }] })),
		listWorkspacePanes: vi.fn(async () => [{ id: 'pane-1', title: 'shell', cwd: '/repo' }]),
		onMessage: vi.fn((cb: (message: unknown) => void) => { messageHandler = cb; return vi.fn(); }),
		onMetadata: vi.fn((cb: (pane: { paneId: string }, title?: string, cwd?: string) => void) => { metadataHandler = cb; return vi.fn(); }),
		disconnect: vi.fn(),
	};
	const scoped = { grantId: 'grant-1', workspaceId: 'workspace-1', deviceName: 'host-1', delegable: false, token: 'scoped-token' };
	class FakeCloudRemoteConnection {
		constructor() { return connection as unknown as FakeCloudRemoteConnection; }
	}
	class FakeTauriBridge {
		invoke = vi.fn();
	}
	class FakeTauriDataProvider {
		invoke = vi.fn();
	}
	return {
		state, adapter, handle, connection, scoped,
		getWorkspaceShareToken: vi.fn(async () => scoped),
		authSnapshot: vi.fn(() => ({ userToken: state.userToken })),
		startCloudControllerBoot: vi.fn(() => handle),
		CloudRemoteConnection: FakeCloudRemoteConnection,
		TauriDataProvider: FakeTauriDataProvider,
		TauriBridge: FakeTauriBridge,
		getStateHandler: () => stateHandler,
		getAuthHandler: () => authHandler,
		getMessageHandler: () => messageHandler,
		getMetadataHandler: () => metadataHandler,
	};
});

vi.mock('@ridge/remote/shared/cloud/apiClient', () => ({ getWorkspaceShareToken: h.getWorkspaceShareToken }));
vi.mock('@ridge/remote/shared/cloud/auth', () => ({ snapshot: h.authSnapshot }));
vi.mock('$lib/transport/tauri', () => ({ TauriDataProvider: h.TauriDataProvider }));
vi.mock('$lib/transport/tauriShim/bridge', () => ({ TauriBridge: h.TauriBridge }));
vi.mock('./cloudControllerBoot', () => ({ startCloudControllerBoot: h.startCloudControllerBoot }));
vi.mock('../../../remote/lib/cloudRemote', () => ({ CloudRemoteConnection: h.CloudRemoteConnection }));

import {
	activeSharedWorkspaceProjection,
	assertShareTokenScope,
	closeSharedWorkspaceProjection,
	currentSharedWorkspaceProjection,
	openSharedWorkspaceProjection,
} from './sharedWorkspaceProjection';

const input = {
	grantId: 'grant-1',
	workspaceId: 'workspace-1',
	name: 'Requested name',
	ownerUsername: 'owner',
	deviceName: 'host-1',
};

beforeEach(() => {
	closeSharedWorkspaceProjection();
	h.state.userToken = 'user-token';
	h.state.adapterState = 'connected';
	h.state.authState = 'authorized';
	h.scoped.grantId = 'grant-1';
	h.scoped.workspaceId = 'workspace-1';
	h.scoped.deviceName = 'host-1';
	h.handle.disconnect.mockClear();
	h.connection.disconnect.mockClear();
	h.connection.init.mockClear();
	h.connection.listWorkspaces.mockImplementation(async () => ({ workspaces: [{ id: 'workspace-1', name: 'Remote workspace' }] }));
	h.connection.listWorkspacePanes.mockImplementation(async () => [{ id: 'pane-1', title: 'shell', cwd: '/repo' }]);
});

describe('shared workspace projection lifecycle', () => {
	it('opens a scoped projection and applies pane/message metadata updates', async () => {
		await openSharedWorkspaceProjection(input);
		const projection = currentSharedWorkspaceProjection();
		expect(projection).toMatchObject({ grantId: 'grant-1', workspaceId: 'workspace-1', name: 'Remote workspace' });
		expect(projection?.panes).toEqual([{ id: 'pane-1', title: 'shell', cwd: '/repo' }]);
		expect(activeSharedWorkspaceProjection).toBeDefined();
		expect(h.connection.init).toHaveBeenCalledOnce();

		h.getMessageHandler()?.({ type: 'panes', panes: [{ id: 'pane-2', title: 'editor', cwd: '/tmp' }] });
		expect(currentSharedWorkspaceProjection()?.panes[0]?.id).toBe('pane-2');
	h.getMetadataHandler()?.({ paneId: 'pane-2' }, 'updated', '/workspace');
		expect(currentSharedWorkspaceProjection()?.panes[0]).toMatchObject({ title: 'updated', cwd: '/workspace' });

		closeSharedWorkspaceProjection();
		expect(currentSharedWorkspaceProjection()).toBeNull();
		expect(h.connection.disconnect).toHaveBeenCalledOnce();
	});

	it('fails before host contact without login and rejects scope mismatch', async () => {
		h.state.userToken = null;
		await expect(openSharedWorkspaceProjection(input)).rejects.toThrow('Ridge Cloud');

		h.state.userToken = 'user-token';
		h.scoped.deviceName = 'other-host';
		await expect(openSharedWorkspaceProjection(input)).rejects.toThrow();
	});

	it('disconnects a failed boot when the host returns an unauthorized workspace', async () => {
		h.connection.listWorkspaces.mockResolvedValue({ workspaces: [{ id: 'other-workspace', name: 'wrong' }] });
		await expect(openSharedWorkspaceProjection(input)).rejects.toThrow();
		expect(h.handle.disconnect).toHaveBeenCalledOnce();
	});

	it('rejects authorization errors and cleans the waiting listeners', async () => {
		h.state.adapterState = 'connecting';
		h.state.authState = 'pending';
		const pending = openSharedWorkspaceProjection(input);
		h.state.adapterState = 'error';
		h.getStateHandler()?.();
		await expect(pending).rejects.toThrow();
	});

	it('validates the non-delegable share scope contract', () => {
		expect(() => assertShareTokenScope(input, { ...input, delegable: false })).not.toThrow();
		expect(() => assertShareTokenScope(input, { ...input, delegable: true })).toThrow();
	});
});
