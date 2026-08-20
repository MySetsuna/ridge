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
// `pane-pty-closed` rebuild branch reads `defaultShell` via it; null degrades
// gracefully through `?.`.
const managerStub = {
	feed: vi.fn(),
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
			cmd === 'take_pane_delta_frame' ? merged : undefined,
		);
		await ensurePtyBridge(PANE, WS);

		channels[0].__deliver([]);
		await vi.waitFor(() => {
			expect(invokeMock).toHaveBeenCalledWith('take_pane_delta_frame', {
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

	it('falls back to set_pane_delta_mode(false) when queued decode fails (R5 self-heal)', async () => {
		const { ensurePtyBridge } = await freshBridge();
		managerStub.enqueueDeltaFrame.mockImplementation((_pane, _bytes, onError) => {
			onError?.(new Error('decode failed'));
		});
		// Silence the warn the bridge emits on error.
		const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

		await ensurePtyBridge(PANE, WS);
		channels[0].__deliver(new Uint8Array([0xff]));
		await Promise.resolve();

		const fallback = invokeMock.mock.calls.find(
			([cmd]) => cmd === 'set_pane_delta_mode',
		);
		expect(fallback).toBeTruthy();
		expect(fallback![1]).toMatchObject({
			workspaceId: WS,
			paneId: PANE,
			enabled: false,
		});

		// warn was called at least once with the diagnostic context object.
		expect(warnSpy).toHaveBeenCalled();
		warnSpy.mockRestore();
	});

	it('does not log a destroyed pane when backend casing differs', async () => {
		const { ensurePtyBridge } = await freshBridge();
		invokeMock.mockImplementation(async (cmd: string) => {
			if (cmd === 'activate_pane_pty') throw new Error('pane not found: closed');
			return undefined;
		});
		await ensurePtyBridge(PANE, WS);
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed');
		expect(closed).toBeTruthy();
		const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
		await (closed![1] as (event: unknown) => Promise<void>)({
			payload: { workspaceId: WS, paneId: PANE },
		});
		expect(errorSpy).not.toHaveBeenCalledWith(
			'activate_pane_pty (rebuild) failed',
			expect.anything(),
		);
		errorSpy.mockRestore();
	});

	it('does not log a late rebuild when the pane was already destroyed', async () => {
		const { ensurePtyBridge } = await freshBridge();
		invokeMock.mockImplementation(async (cmd: string) => {
			if (cmd === 'create_pane') throw new Error('Pane not found: closed');
			return undefined;
		});
		await ensurePtyBridge(PANE, WS);
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed');
		expect(closed).toBeTruthy();
		const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
		await (closed![1] as (event: unknown) => Promise<void>)({
			payload: { workspaceId: WS, paneId: PANE },
		});
		expect(errorSpy).not.toHaveBeenCalledWith(
			'create_pane (rebuild) failed',
			expect.anything(),
		);
		errorSpy.mockRestore();
	});

	it('rebuilds a closed pane within its composite workspace identity', async () => {
		const { ensurePtyBridge } = await freshBridge();
		await ensurePtyBridge(PANE, WS);
		const closed = listenMock.mock.calls.find(([name]) => name === 'pane-pty-closed');
		expect(closed).toBeTruthy();

		await (closed![1] as (event: unknown) => Promise<void>)({
			payload: { workspaceId: WS, paneId: PANE },
		});

		expect(invokeMock).toHaveBeenCalledWith('create_pane', {
			workspaceId: WS,
			paneId: PANE,
			shell: null,
		});
		expect(invokeMock).toHaveBeenCalledWith('activate_pane_pty', expect.objectContaining({
			workspaceId: WS,
			paneId: PANE,
		}));
		expect(invokeMock).toHaveBeenCalledWith('set_pane_delta_mode', {
			workspaceId: WS,
			paneId: PANE,
			enabled: true,
		});
		expect(managerStub.setLocalGridAuthority).toHaveBeenNthCalledWith(1, PANE, true);
		expect(managerStub.setLocalGridAuthority).toHaveBeenNthCalledWith(2, PANE, false);
		expect(managerStub.fitPaneNow).toHaveBeenCalledWith(PANE, true);
	});

	it('still installs listeners when register_pane_delta_channel fails', async () => {
		// freshBridge() resets the invoke mock, so the rejection has to be
		// configured AFTER the module is loaded.
		const { ensurePtyBridge, hasPtyBridge } = await freshBridge();

		// Backend rejects the registration (e.g. pane not yet activated).
		// The bridge must keep the listen() paths so the pane is usable on
		// legacy pty-output-* events.
		invokeMock.mockImplementation(async (cmd: string) => {
			if (cmd === 'register_pane_delta_channel') {
				throw new Error('pane not found');
			}
			return undefined;
		});
		const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});

		await ensurePtyBridge(PANE, WS);

		// Bridge entry is present even though registration failed.
		expect(hasPtyBridge(PANE)).toBe(true);
		// Both listen() subscriptions still ran (pty-output + pane-pty-closed).
		const listenedNames = listenMock.mock.calls.map(([n]) => n);
		expect(listenedNames).toEqual(
			expect.arrayContaining([
				`pty-output-${WS}-${PANE}`,
				'pane-pty-closed',
			]),
		);
		// A diagnostic warning surfaced (not silently swallowed).
		expect(warnSpy).toHaveBeenCalled();
		warnSpy.mockRestore();
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
		expect(listenMock).toHaveBeenCalledTimes(2);
		expect(invokeMock.mock.calls.filter(([cmd]) => cmd === 'register_pane_delta_channel'))
			.toHaveLength(1);
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

	it('releases the output listener when the close listener fails', async () => {
		const { ensurePtyBridge, hasPtyBridge } = await freshBridge();
		const outUnlisten = vi.fn();
		listenMock
			.mockImplementationOnce(async () => outUnlisten)
			.mockImplementationOnce(async () => { throw new Error('event bus closed'); });

		await expect(ensurePtyBridge(PANE, WS)).rejects.toThrow('event bus closed');
		expect(outUnlisten).toHaveBeenCalledOnce();
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
