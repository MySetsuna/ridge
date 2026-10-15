/**
 * P4.3 — ptyBridge unit tests.
 *
 * The bridge wires three IPC channels together:
 *
 *   1. `listen('pty-output-{ws}-{pane}')` → manager.feed (string path)
 *   2. `listen('pane-pty-closed')` → invoke('create_pane' + 'activate_pane_pty')
 *   3. `new Channel<Uint8Array>()` registered via
 *      `invoke('register_pane_delta_channel')` → manager.enqueueDeltaFrame
 *
 * These tests mock the Tauri IPC surface and the TerminalManager, then
 * drive the bridge through:
 *   - happy-path delta dispatch (Uint8Array, ArrayBuffer, and number[]
 *     normalization)
 *   - delayed delta decode failure → R5 self-heal fallback toggle
 *   - register_pane_delta_channel invocation failure → bridge still wires
 *     the other listeners so the pane stays usable on the legacy path
 *   - idempotent ensurePtyBridge / teardown
 */

import {
	afterEach,
	beforeEach,
	describe,
	expect,
	it,
	vi,
} from 'vitest';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

/** All `new Channel<T>()` instances created during a test, in construction order. */
const channels: FakeChannel<unknown>[] = [];

class FakeChannel<T> {
	onmessage?: (data: T) => void;
	constructor() {
		channels.push(this as unknown as FakeChannel<unknown>);
	}
	/** Drive the onmessage handler from the test, as if the backend sent bytes. */
	__deliver(data: T): void {
		this.onmessage?.(data);
	}
}

const invokeMock = vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>();
const listenMock = vi.fn<(name: string, cb: (e: unknown) => void) => Promise<() => void>>();

vi.mock('@tauri-apps/api/core', () => ({
	invoke: (cmd: string, args?: unknown) => invokeMock(cmd, args),
	Channel: FakeChannel,
}));

vi.mock('@tauri-apps/api/event', () => ({
	listen: (name: string, cb: (e: unknown) => void) => listenMock(name, cb),
}));

// TerminalManager singleton mock — capture feed / enqueueDeltaFrame / rows /
// cols calls so tests can assert dispatch. `hostPorts` returns null: the
	// Runtime replacements only refresh the canonical projection.
const managerStub = {
	feed: vi.fn(),
    setNativeActivation: vi.fn(),
	enqueueDeltaFrame: vi.fn(),
	leaveAltScreen: vi.fn(),
	setLocalGridAuthority: vi.fn(),
	rows: vi.fn(() => 24),
	cols: vi.fn(() => 80),
	fitPaneNow: vi.fn(),
};
vi.mock('./manager', () => ({
	TerminalManager: { instance: () => managerStub, hostPorts: () => null },
}));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const WS = '00000000-0000-0000-0000-0000000000aa';
const WS2 = '00000000-0000-0000-0000-0000000000cc';
const PANE = '00000000-0000-0000-0000-0000000000bb';

async function freshBridge() {
	// Reset module state so the in-module `bridges` Map starts empty per test.
	vi.resetModules();
	channels.length = 0;
	invokeMock.mockReset();
	listenMock.mockReset();
	managerStub.feed.mockReset();
	managerStub.enqueueDeltaFrame.mockReset();
	managerStub.leaveAltScreen.mockReset();
	managerStub.setLocalGridAuthority.mockReset();
	managerStub.rows.mockReturnValue(24);
	managerStub.cols.mockReturnValue(80);
	managerStub.fitPaneNow.mockReset();

	// Default behavior: listen returns a no-op unlisten, invoke resolves.
	listenMock.mockImplementation(async () => () => {});
	invokeMock.mockResolvedValue(undefined);

	// Re-import after resetModules so the in-module mocks bind fresh.
	const mod = await import('./ptyBridge');
	return mod;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('ptyBridge.ensurePtyBridge — delta Channel wiring', () => {
	beforeEach(() => {
		vi.useRealTimers();
	});
	afterEach(() => {
		vi.restoreAllMocks();
	});

	it('registers a Channel via invoke("register_pane_delta_channel")', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);

		const registerCall = invokeMock.mock.calls.find(
			([cmd]) => cmd === 'register_pane_delta_channel',
		);
		expect(registerCall).toBeTruthy();
		expect(registerCall![1]).toMatchObject({ workspaceId: WS, paneId: PANE });
		expect(registerCall![1]).toHaveProperty('channel');
		expect(registerCall![1]).toEqual(
			expect.objectContaining({ channel: expect.any(FakeChannel) }),
		);
		// Exactly one Channel must have been created for this pane.
		expect(channels).toHaveLength(1);
	});

	it('queues Uint8Array payload unchanged for the manager frame hub', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);

		const payload = new Uint8Array([1, 2, 3, 4, 5]);
		channels[0].__deliver(payload);

		expect(managerStub.enqueueDeltaFrame).toHaveBeenCalledTimes(1);
		const [paneArg, bytesArg] = managerStub.enqueueDeltaFrame.mock.calls[0];
		expect(paneArg).toBe(PANE);
		expect(bytesArg).toBe(payload); // same reference, no copy on the fast path
	});

	it('pulls one merged frame after a zero-byte mailbox wake', async () => {
		const { ensurePtyBridge } = await freshBridge();
		const merged = new Uint8Array([4, 5, 6]);
		invokeMock.mockImplementation(async (cmd: string) =>
			cmd === 'take_terminal_frames' ? [merged] : undefined,
		);
		await ensurePtyBridge(PANE, WS);

		channels[0].__deliver([]);
		await vi.waitFor(() => {
			expect(invokeMock).toHaveBeenCalledWith('take_terminal_frames', {
				workspaceId: WS,
				paneId: PANE,
			});
			expect(managerStub.enqueueDeltaFrame).toHaveBeenCalledWith(
				PANE,
				merged,
				expect.any(Function),
			);
		});
	});

	it('wraps an ArrayBuffer payload into a Uint8Array view', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);

		const buf = new Uint8Array([7, 8, 9]).buffer;
		channels[0].__deliver(buf);

		expect(managerStub.enqueueDeltaFrame).toHaveBeenCalledTimes(1);
		const bytesArg = managerStub.enqueueDeltaFrame.mock.calls[0][1] as Uint8Array;
		expect(bytesArg).toBeInstanceOf(Uint8Array);
		expect(Array.from(bytesArg)).toEqual([7, 8, 9]);
	});

	it('handles a number[] payload (legacy IPC path)', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);

		channels[0].__deliver([10, 20, 30]);

		expect(managerStub.enqueueDeltaFrame).toHaveBeenCalledTimes(1);
		const bytesArg = managerStub.enqueueDeltaFrame.mock.calls[0][1] as Uint8Array;
		expect(bytesArg).toBeInstanceOf(Uint8Array);
		expect(Array.from(bytesArg)).toEqual([10, 20, 30]);
	});

	it('requests a new activation and snapshot when queued decode fails', async () => {
		const { ensurePtyBridge } = await freshBridge();
		managerStub.enqueueDeltaFrame.mockImplementation((_pane, _bytes, onError) => {
			onError?.(new Error('decode failed'));
		});
		// Silence the warn the bridge emits on error.
		const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

		await ensurePtyBridge(PANE, WS);
		channels[0].__deliver(new Uint8Array([0xff]));
		await Promise.resolve();

        expect(invokeMock).toHaveBeenCalledWith('register_pane_delta_channel', expect.objectContaining({ workspaceId: WS, paneId: PANE, activationId: expect.any(Number) }));
        expect(invokeMock.mock.calls.some(([cmd, args]) => cmd === 'set_pane_delta_mode' && args?.enabled === false)).toBe(false);

		// warn was called at least once with the diagnostic context object.
		expect(warnSpy).toHaveBeenCalled();
		warnSpy.mockRestore();
	});

	it('leaves exit and restart decisions in Runtime', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		invokeMock.mockClear();
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		closed[1]({ payload: { workspaceId: WS, paneId: PANE, replacementEpoch: null } });
		expect(invokeMock).not.toHaveBeenCalled();
		expect(managerStub.leaveAltScreen).not.toHaveBeenCalled();
	});

	it('refreshes the canonical projection once for a Runtime replacement', async () => {
		const { ensurePtyBridge } = await freshBridge();
		invokeMock.mockImplementation(async command => command === 'get_terminal_snapshot' ? btoa(String.fromCharCode(0x13)) : undefined);
		await ensurePtyBridge(PANE, WS);
		invokeMock.mockClear();
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		const event = { payload: { workspaceId: WS, paneId: PANE, replacementEpoch: 'new-terminal' } };
		closed[1](event); closed[1](event);
		await vi.waitFor(() => expect(managerStub.enqueueDeltaFrame).toHaveBeenCalled());
		expect(invokeMock.mock.calls.map(([method]) => method)).toEqual(['register_pane_delta_channel', 'get_terminal_snapshot']);
		expect(invokeMock).toHaveBeenCalledWith('get_terminal_snapshot', expect.objectContaining({ workspaceId: WS, paneId: PANE, activationId: expect.any(Number) }));
		expect(managerStub.leaveAltScreen).not.toHaveBeenCalled();
		expect(managerStub.setLocalGridAuthority).not.toHaveBeenCalled();
	});

	it('coalesces a replacement received before the recovery snapshot starts', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		invokeMock.mockClear();
		let release!: () => void;
		let registrations = 0;
		invokeMock.mockImplementation(command => {
			if (command === 'register_pane_delta_channel' && registrations++ === 0) return new Promise<void>(resolve => { release = resolve; });
			return Promise.resolve(command === 'get_terminal_snapshot' ? btoa(String.fromCharCode(0x13)) : undefined);
		});
		managerStub.enqueueDeltaFrame.mockImplementationOnce((_pane: string, _bytes: Uint8Array, onError?: (error: unknown) => void) => onError?.(new Error('old frame')));
		channels[0].__deliver(new Uint8Array([0xff]));
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		closed[1]({ payload: { workspaceId: WS, paneId: PANE, replacementEpoch: 'new-terminal' } });
		release();
		await vi.waitFor(() => expect(invokeMock.mock.calls.filter(([method]) => method === 'get_terminal_snapshot')).toHaveLength(1));
		expect(invokeMock.mock.calls.filter(([method]) => method === 'register_pane_delta_channel')).toHaveLength(1);
		expect(invokeMock.mock.calls.some(([method]) => method === 'create_pane' || method === 'activate_pane_pty')).toBe(false);
	});

	it('keeps canonical stream frames arriving before the explicit recovery snapshot returns', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		let release!: (snapshot: string) => void;
		invokeMock.mockImplementation(command => command === 'get_terminal_snapshot'
			? new Promise<string>(resolve => { release = resolve; }) : Promise.resolve());
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		closed[1]({payload:{workspaceId:WS,paneId:PANE,replacementEpoch:'new'}});
		await vi.waitFor(() => expect(release).toBeTypeOf('function'));
		channels[0].__deliver(new Uint8Array([7]));
		channels[0].__deliver(new Uint8Array([8]));
		expect(managerStub.enqueueDeltaFrame.mock.calls.map(call => call[1])).toEqual([new Uint8Array([7]),new Uint8Array([8])]);
		release(btoa(String.fromCharCode(0x13)));
	});

	it('discards an older recovery snapshot if a later lifetime arrives while it is pending', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		invokeMock.mockClear(); managerStub.enqueueDeltaFrame.mockClear();
		let release!: (snapshot: string) => void;
		let snapshots = 0;
		invokeMock.mockImplementation(command => command === 'get_terminal_snapshot'
			? ++snapshots === 1 ? new Promise<string>(resolve => { release = resolve; }) : Promise.resolve(btoa(String.fromCharCode(0x14)))
			: Promise.resolve());
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		closed[1]({payload:{workspaceId:WS,paneId:PANE,replacementEpoch:'first'}});
		await vi.waitFor(() => expect(release).toBeTypeOf('function'));
		closed[1]({payload:{workspaceId:WS,paneId:PANE,replacementEpoch:'second'}});
		release(btoa(String.fromCharCode(0x13)));
		await vi.waitFor(() => expect(managerStub.enqueueDeltaFrame).toHaveBeenCalledOnce());
		expect(managerStub.enqueueDeltaFrame.mock.calls[0][1]).toEqual(new Uint8Array([0x14]));
	});

	it('ignores a queued frame failure from an activation already replaced', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		channels[0].__deliver(new Uint8Array([0xff]));
		const oldFailure = managerStub.enqueueDeltaFrame.mock.calls.at(-1)![2];
		invokeMock.mockImplementation(async command => command === 'get_terminal_snapshot' ? btoa(String.fromCharCode(0x13)) : undefined);
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		closed[1]({payload:{workspaceId:WS,paneId:PANE,replacementEpoch:'new'}});
		await vi.waitFor(() => expect(invokeMock.mock.calls.some(([method]) => method === 'get_terminal_snapshot')).toBe(true));
		invokeMock.mockClear(); oldFailure(new Error('late old frame'));
		expect(invokeMock).not.toHaveBeenCalled();
	});

	it('ignores a replacement for another workspace or a torn-down view', async () => {
		const { ensurePtyBridge, teardownPtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed')!;
		invokeMock.mockClear();
		closed[1]({ payload: { workspaceId: WS2, paneId: PANE, replacementEpoch: 'other' } });
		expect(invokeMock).not.toHaveBeenCalled();
		teardownPtyBridge(PANE, WS);
		invokeMock.mockClear();
		closed[1]({ payload: { workspaceId: WS, paneId: PANE, replacementEpoch: 'late' } });
		expect(invokeMock).not.toHaveBeenCalled();
	});

	it('releases a failed registration and permits a later canonical retry', async () => {
		const { ensurePtyBridge, hasPtyBridge } = await freshBridge();
		const stopClosed = vi.fn();
		listenMock.mockImplementationOnce(async () => stopClosed);
		invokeMock.mockRejectedValueOnce(new Error('pane not found'));
		await expect(ensurePtyBridge(PANE, WS)).rejects.toThrow('pane not found');
		expect(hasPtyBridge(PANE)).toBe(false);
		expect(stopClosed).toHaveBeenCalledOnce();
		expect(listenMock.mock.calls.map(([name]) => name)).toEqual(['pane-pty-closed', 'pane-meta-changed', 'pane-terminal-error']);
		invokeMock.mockResolvedValue(undefined);
		await ensurePtyBridge(PANE, WS);
		expect(hasPtyBridge(PANE)).toBe(true);
		expect(invokeMock.mock.calls.filter(([name]) => name === 'register_pane_delta_channel')).toHaveLength(2);
	});

	it('is idempotent — second ensurePtyBridge call is a no-op', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		const channelsAfterFirst = channels.length;
		const registerCallsAfterFirst = invokeMock.mock.calls.filter(
			([cmd]) => cmd === 'register_pane_delta_channel',
		).length;

		await ensurePtyBridge(PANE, WS);

		expect(channels).toHaveLength(channelsAfterFirst);
		const registerCallsAfterSecond = invokeMock.mock.calls.filter(
			([cmd]) => cmd === 'register_pane_delta_channel',
		).length;
		expect(registerCallsAfterSecond).toBe(registerCallsAfterFirst);
	});

	it('single-flights concurrent attaches for the same pane', async () => {
		const { ensurePtyBridge } = await freshBridge();
		const first = ensurePtyBridge(PANE, WS);
		const second = ensurePtyBridge(PANE, WS);
		expect(second).toBe(first);
		await Promise.all([first, second]);

		expect(channels).toHaveLength(1);
		expect(listenMock).toHaveBeenCalledTimes(3);
		expect(invokeMock.mock.calls.filter(([cmd]) => cmd === 'register_pane_delta_channel'))
			.toHaveLength(1);
	});

	it('retains a stopped projection for a remounted view and rejects old child metadata', async () => {
		const { ensurePtyBridge, observePtyView } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		const metadata=listenMock.mock.calls.find(([name])=>name==='pane-meta-changed')!;
		const state={workspaceId:WS,paneId:PANE,runtimeEpoch:'runtime',terminalEpoch:'current',ptyGeneration:2,status:'Exited',error:'outside Runtime roots'};
		metadata[1]({payload:state});
		const receive=vi.fn(); const stop=observePtyView(PANE,WS,receive);
		expect(receive).toHaveBeenLastCalledWith(expect.objectContaining({lifecycle:state}));
		stop();
		metadata[1]({payload:{...state,ptyGeneration:1,terminalEpoch:'old',status:'Idle'}});
		const remount=vi.fn(); observePtyView(PANE,WS,remount);
		expect(remount).toHaveBeenLastCalledWith(expect.objectContaining({lifecycle:state}));
	});

	it('keeps same-named panes independent across workspaces', async () => {
		const { ensurePtyBridge, teardownPtyBridge, hasPtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		await ensurePtyBridge(PANE, WS2);

		expect(channels).toHaveLength(2);
		expect(hasPtyBridge(PANE, WS)).toBe(true);
		expect(hasPtyBridge(PANE, WS2)).toBe(true);

		teardownPtyBridge(PANE, WS);
		expect(hasPtyBridge(PANE, WS)).toBe(false);
		expect(hasPtyBridge(PANE, WS2)).toBe(true);
	});
});

describe('ptyBridge.teardownPtyBridge', () => {
	it('removes the bridge entry; subsequent hasPtyBridge returns false', async () => {
		const { ensurePtyBridge, teardownPtyBridge, hasPtyBridge } =
			await freshBridge();
		await ensurePtyBridge(PANE, WS);
		expect(hasPtyBridge(PANE)).toBe(true);
		teardownPtyBridge(PANE);
		expect(hasPtyBridge(PANE)).toBe(false);
	});

	it('is safe to call when the pane has no bridge', async () => {
		const { teardownPtyBridge } = await freshBridge();
		// Should not throw.
		expect(() => teardownPtyBridge('unknown-pane')).not.toThrow();
	});

	it('cancels an attach that is still waiting for listener registration', async () => {
		const { ensurePtyBridge, teardownPtyBridge, hasPtyBridge } = await freshBridge();
		let resolveListen!: (unlisten: () => void) => void;
		const unlisten = vi.fn();
		listenMock.mockImplementationOnce(() => new Promise((resolve) => {
			resolveListen = resolve;
		}));

		const pending = ensurePtyBridge(PANE, WS);
		teardownPtyBridge(PANE);
		resolveListen(unlisten);
		await pending;

		expect(hasPtyBridge(PANE)).toBe(false);
		expect(unlisten).toHaveBeenCalledOnce();
		expect(invokeMock.mock.calls.some(([cmd]) => cmd === 'register_pane_delta_channel'))
			.toBe(false);
	});

	it('does not register a channel when the lifecycle listener fails', async () => {
		const { ensurePtyBridge, hasPtyBridge } = await freshBridge();
		listenMock.mockRejectedValueOnce(new Error('event bus closed'));
		await expect(ensurePtyBridge(PANE, WS)).rejects.toThrow('event bus closed');
		expect(invokeMock).not.toHaveBeenCalled();
		expect(hasPtyBridge(PANE)).toBe(false);
	});
});

describe('ptyBridge.setPaneDeltaMode', () => {
	it('invokes set_pane_delta_mode with the registered workspaceId', async () => {
		const { ensurePtyBridge, setPaneDeltaMode } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		invokeMock.mockClear();
		invokeMock.mockResolvedValue(undefined);

		expect(await setPaneDeltaMode(PANE, true)).toBe(true);
		expect(invokeMock).toHaveBeenCalledWith('set_pane_delta_mode', {
			workspaceId: WS,
			paneId: PANE,
			enabled: true,
		});
		expect(managerStub.setLocalGridAuthority).toHaveBeenCalledWith(PANE, false);
	});

	it('is silent for a pane that has no bridge', async () => {
		const { setPaneDeltaMode } = await freshBridge();
		invokeMock.mockClear();
		expect(await setPaneDeltaMode('unknown-pane', true)).toBe(false);
		// The bridge is gone, so no invoke should fire.
		expect(invokeMock).not.toHaveBeenCalled();
		expect(managerStub.setLocalGridAuthority).not.toHaveBeenCalled();
	});

	it('keeps raw-grid authority when the backend switch fails', async () => {
		const { ensurePtyBridge, setPaneDeltaMode } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		invokeMock.mockRejectedValueOnce(new Error('backend unavailable'));

		expect(await setPaneDeltaMode(PANE, true)).toBe(false);
		expect(managerStub.setLocalGridAuthority).toHaveBeenCalledWith(PANE, true);
	});

	it('awaits delta enable before the forced post-activation fit', async () => {
		const { enableDeltaModeThenFit, ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		const order: string[] = [];
		invokeMock.mockImplementation(async (cmd: string) => {
			if (cmd === 'set_pane_delta_mode') order.push('enable');
			return undefined;
		});

		await enableDeltaModeThenFit(PANE, async () => { order.push('fit'); }, WS);
		expect(order).toEqual(['enable', 'fit']);
	});
});
