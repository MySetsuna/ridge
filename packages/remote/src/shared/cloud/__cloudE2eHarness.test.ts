import { beforeEach, describe, expect, it, vi } from 'vitest';

const harness = vi.hoisted(() => {
	class FakeHost {
		static instances: FakeHost[] = [];
		constructor(..._args: unknown[]) { FakeHost.instances.push(this); }
		goOnline = vi.fn(async () => undefined);
		goOffline = vi.fn();
	}

	class FakeControllerProvider {
		static instances: FakeControllerProvider[] = [];
		constructor(..._args: unknown[]) { FakeControllerProvider.instances.push(this); }
		getKeyBindingMode = vi.fn(() => 'enforced');
	}

	class FakeAdapter {
		static instances: FakeAdapter[] = [];
		static mode: 'connected' | 'error' = 'connected';
		private stateListener: ((state: string) => void) | null = null;
		private paneListener: ((paneId: string, bytes: Uint8Array) => void) | null = null;
		constructor() { FakeAdapter.instances.push(this); }
		onError = vi.fn((_listener: (message: string, code?: string) => void) => () => undefined);
		onStateChange = vi.fn((listener: (state: string) => void) => {
			this.stateListener = listener;
			return () => { this.stateListener = null; };
		});
		connect = vi.fn(async () => { this.stateListener?.(FakeAdapter.mode); });
		close = vi.fn();
		dispose = vi.fn();
		onPaneBytes = vi.fn((listener: (paneId: string, bytes: Uint8Array) => void) => {
			this.paneListener = listener;
			return () => { this.paneListener = null; };
		});
		emitPane(paneId: string, bytes: Uint8Array): void { this.paneListener?.(paneId, bytes); }
	}

	class FakeRpcClient {
		private negotiated: ((protocol: { capabilities: string[] }) => void) | null = null;
		constructor(private readonly adapter: FakeAdapter) {}
		onNegotiated = vi.fn((listener: (protocol: { capabilities: string[] }) => void) => {
			this.negotiated = listener;
			return () => { this.negotiated = null; };
		});
		notify = vi.fn((method: string, _params: unknown) => {
			if (method === '$/hello') this.negotiated?.({ capabilities: ['directory', 'pane-stream'] });
			if (method === 'subscribe-pane') this.adapter.emitPane('pane-a', new Uint8Array([0x41, 0x42]));
		});
		request = vi.fn(async (method: string, params: { offset?: number }) => {
			if (method === 'get_directory_children') {
				if (params.offset === 3) throw new Error('page failed');
				return { entries: [{ name: `entry-${params.offset ?? 0}` }], total: 7, has_more: true };
			}
			if (method === 'write_to_pty') return { accepted: true };
			if (method === 'get_remote_info') return { host: 'fixture' };
			throw new Error(`unsupported method: ${method}`);
		});
	}

	return {
		FakeHost,
		FakeControllerProvider,
		FakeAdapter,
		FakeRpcClient,
		invoke: vi.fn(async () => undefined),
		listen: vi.fn(async () => vi.fn()),
		createBridge: vi.fn(),
		makePaneSource: vi.fn(() => ({})),
	};
});

vi.mock('./ridgeCloudProvider', () => ({ RidgeCloudHost: harness.FakeHost }));
vi.mock('./controllerCloudProvider', () => ({ ControllerCloudProvider: harness.FakeControllerProvider }));
vi.mock('./cloudHostBridge', () => ({ CloudHostBridge: harness.createBridge }));
vi.mock('./cloudHostPaneSource', () => ({ makeCloudHostPaneSource: harness.makePaneSource }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: harness.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: harness.listen }));
vi.mock('@ridge/remote', () => ({
	CLIENT_CAPABILITIES: ['directory', 'pane-stream'],
	CLIENT_PROTOCOL_VERSION: 1,
	HELLO_METHOD: '$/hello',
	RpcClient: harness.FakeRpcClient,
	createCloudWebrtcTransportWith: vi.fn((_device: string, factory: (callback: () => void) => unknown) => {
		factory(() => undefined);
		return new harness.FakeAdapter();
	}),
}));

const { runCloudDirChildrenE2E } = await import('./__cloudE2eHarness');

beforeEach(() => {
	harness.FakeHost.instances.length = 0;
	harness.FakeControllerProvider.instances.length = 0;
	harness.FakeAdapter.instances.length = 0;
	harness.FakeAdapter.mode = 'connected';
	harness.invoke.mockReset().mockResolvedValue(undefined);
	harness.listen.mockReset().mockResolvedValue(vi.fn());
	harness.createBridge.mockReset();
	harness.makePaneSource.mockClear();
});

describe('cloud directory E2E harness orchestration', () => {
	it('collects negotiated capabilities, paginated failures, exploit result, and pane bytes', async () => {
		const result = await runCloudDirChildrenE2E({
			deviceToken: 'device-token',
			userToken: 'user-token',
			username: 'alice',
			device: 'laptop',
			path: 'C:/repo',
			offsets: [0, 3],
			limit: 1,
			timeoutMs: 100,
			exploit: { method: 'get_remote_info' },
			paneStream: { paneId: 'pane-a', write: 'x', waitMs: 0 },
			tamperBinding: true,
		});

		expect(result).toMatchObject({
			connected: true,
			capabilities: ['directory', 'pane-stream'],
			exploitResult: { method: 'get_remote_info', ok: true },
			keyBindingMode: 'enforced',
			paneStream: { paneId: 'pane-a', frames: 1, bytes: 2, sample: 'AB' },
		});
		expect(result.results).toEqual([
			{ offset: 0, ok: true, entries: 1, total: 7, hasMore: true, first: 'entry-0' },
			{ offset: 3, ok: false, error: 'page failed' },
		]);
		expect(harness.invoke).toHaveBeenCalledWith('set_cloud_remote_active', { active: true });
		expect(harness.invoke).toHaveBeenLastCalledWith('set_cloud_remote_active', { active: false });
		expect(harness.FakeAdapter.instances[0]?.close).toHaveBeenCalledOnce();
		expect(harness.FakeAdapter.instances[0]?.dispose).toHaveBeenCalledOnce();
		expect(harness.FakeHost.instances[0]?.goOffline).toHaveBeenCalledOnce();
		expect((globalThis as { __RIDGE_DEBUG_TAMPER_E2EE_SIG?: boolean }).__RIDGE_DEBUG_TAMPER_E2EE_SIG)
			.toBeUndefined();
	});

	it('returns a disconnected result on controller error and still closes every resource', async () => {
		harness.FakeAdapter.mode = 'error';

		const result = await runCloudDirChildrenE2E({
			deviceToken: 'device-token',
			userToken: 'user-token',
			username: 'alice',
			device: 'laptop',
			path: 'C:/repo',
			timeoutMs: 100,
		});

		expect(result).toMatchObject({
			connected: false,
			results: [],
			capabilities: null,
			exploitResult: null,
			keyBindingMode: 'enforced',
			paneStream: null,
		});
		expect(harness.FakeAdapter.instances[0]?.close).toHaveBeenCalledOnce();
		expect(harness.FakeAdapter.instances[0]?.dispose).toHaveBeenCalledOnce();
		expect(harness.FakeHost.instances[0]?.goOffline).toHaveBeenCalledOnce();
	});
});
