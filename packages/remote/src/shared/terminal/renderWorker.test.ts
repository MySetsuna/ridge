/**
 * P4.5 — Render-worker dispatch unit tests.
 *
 * Exercises the pure `handleRequest(state, request) -> response` core
 * of the worker without touching `self.postMessage` or wasm. The full
 * Worker bootstrap is guarded by an `isInWorkerScope()` check that
 * stays false under vitest's `node` environment, so importing this
 * file is safe.
 */

import { describe, expect, it, vi } from 'vitest';
import {
	getPaneState,
	handleRequest,
	makeWorkerState,
	type KernelAdapter,
	type KernelHandle,
	type RendererHandle,
} from './renderWorker';
import { isRenderWorkerRequest } from './renderWorker.protocol';
import { SYNC_OUTPUT_TIMEOUT_MS } from './renderTransaction';

const PANE = '00000000-0000-0000-0000-0000000000aa';
const PANE_B = '00000000-0000-0000-0000-0000000000bb';

function init(state = makeWorkerState(), paneId = PANE) {
	const ack = handleRequest(state, {
		type: 'init',
		paneId,
		dims: { rows: 24, cols: 80, dpr: 1 },
		backend: 'webgpu',
		scrollbackLines: 2000,
	});
	return { state, ack };
}

describe('renderWorker.protocol — isRenderWorkerRequest', () => {
	it('accepts every valid tag', () => {
		const tags: Array<{ type: string }> = [
			{ type: 'init' },
			{ type: 'bindCanvas' },
			{ type: 'applyDelta' },
			{ type: 'releaseCanvas' },
			{ type: 'feed' },
			{ type: 'clearTerminalPreservingPrompt' },
			{ type: 'resize' },
			{ type: 'destroy' },
			{ type: 'ping' },
			{ type: 'setFont' },
		];
		for (const t of tags) {
			expect(isRenderWorkerRequest(t)).toBe(true);
		}
	});

	it('rejects unknown / malformed shapes', () => {
		expect(isRenderWorkerRequest(null)).toBe(false);
		expect(isRenderWorkerRequest(undefined)).toBe(false);
		expect(isRenderWorkerRequest({})).toBe(false);
		expect(isRenderWorkerRequest('init')).toBe(false);
		expect(isRenderWorkerRequest({ type: 'evil' })).toBe(false);
		expect(isRenderWorkerRequest({ type: 42 })).toBe(false);
	});
});

describe('renderWorker.handleRequest — ping/pong', () => {
	it('echoes the optional token back', () => {
		const state = makeWorkerState();
		const r1 = handleRequest(state, { type: 'ping', token: 'abc' });
		expect(r1).toEqual({ type: 'pong', token: 'abc' });
		const r2 = handleRequest(state, { type: 'ping' });
		expect(r2).toEqual({ type: 'pong', token: undefined });
	});
});

describe('renderWorker.handleRequest — init', () => {
	it('creates per-pane state on first init and acks with the requested backend', () => {
		const { state, ack } = init();
		expect(ack).toEqual({ type: 'ready', paneId: PANE, backend: 'webgpu' });
		const pane = getPaneState(state, PANE);
		expect(pane).toBeTruthy();
		expect(pane).toMatchObject({
			rows: 24,
			cols: 80,
			dpr: 1,
			backend: 'webgpu',
			scrollbackLines: 2000,
			canvasBound: false,
		});
	});

	it('rejects double-init with pane_already_initialized', () => {
		const { state } = init();
		const dup = handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 50, cols: 100, dpr: 2 },
			backend: 'canvas2d',
			scrollbackLines: 5000,
		});
		expect(dup).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'pane_already_initialized',
		});
		// First init's state must NOT have been clobbered by the second.
		const pane = getPaneState(state, PANE);
		expect(pane).toMatchObject({ rows: 24, cols: 80, backend: 'webgpu' });
	});

	it('rejects init when the production wasm adapter is unavailable', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, null);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'apply_delta_failed',
		});
		expect(getPaneState(state, PANE)).toBeUndefined();
	});

	it('isolates state across panes', () => {
		const state = makeWorkerState();
		init(state, PANE);
		init(state, PANE_B);
		expect(getPaneState(state, PANE)).toBeTruthy();
		expect(getPaneState(state, PANE_B)).toBeTruthy();
		// Destroying one must not affect the other.
		handleRequest(state, { type: 'destroy', paneId: PANE });
		expect(getPaneState(state, PANE)).toBeUndefined();
		expect(getPaneState(state, PANE_B)).toBeTruthy();
	});
});

describe('renderWorker.handleRequest — bindCanvas', () => {
	it('flips canvasBound=true on a known pane', () => {
		const { state } = init();
		const ack = handleRequest(state, { type: 'bindCanvas', paneId: PANE });
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		expect(getPaneState(state, PANE)?.canvasBound).toBe(true);
	});

	it('renders immediately when bind follows feed', () => {
		const kernel = {
			feed: vi.fn(),
			applyDeltaFrame: vi.fn(),
			resize: vi.fn(),
			free: vi.fn(),
		};
		const renderer = {
			render: vi.fn(),
			resize: vi.fn(),
			free: vi.fn(),
		};
		const adapter: KernelAdapter = {
			create: () => kernel,
			createRenderer: () => renderer,
		};
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, adapter);
		handleRequest(state, {
			type: 'feed',
			paneId: PANE,
			bytes: new TextEncoder().encode('before-bind'),
		}, adapter);
		handleRequest(state, {
			type: 'bindCanvas',
			paneId: PANE,
			canvas: {} as OffscreenCanvas,
		}, adapter);
		expect(renderer.render).toHaveBeenCalledOnce();
	});

	it('returns pane_not_initialized when init never ran', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, { type: 'bindCanvas', paneId: PANE });
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'pane_not_initialized',
		});
	});
});

describe('renderWorker.handleRequest — applyDelta / feed', () => {
	it('applyDelta acks for an initialized pane', () => {
		const { state } = init();
		const ack = handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([1, 2, 3]),
		});
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
	});

	it('applyDelta on an unknown pane → pane_not_initialized', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([0]),
		});
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'pane_not_initialized',
		});
	});

	it('feed acks for an initialized pane', () => {
		const { state } = init();
		const ack = handleRequest(state, {
			type: 'feed',
			paneId: PANE,
			bytes: new TextEncoder().encode('hello'),
		});
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
	});

	it('feed on an unknown pane → pane_not_initialized', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, {
			type: 'feed',
			paneId: PANE,
			bytes: new Uint8Array([120]),
		});
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'pane_not_initialized',
		});
	});
});

describe('renderWorker.handleRequest — resize', () => {
	it('updates pane dims', () => {
		const { state } = init();
		const ack = handleRequest(state, {
			type: 'resize',
			paneId: PANE,
			rows: 40,
			cols: 132,
			dpr: 2,
		});
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		expect(getPaneState(state, PANE)).toMatchObject({
			rows: 40,
			cols: 132,
			dpr: 2,
		});
	});

	it('resize on an unknown pane → pane_not_initialized', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, {
			type: 'resize',
			paneId: PANE,
			rows: 10,
			cols: 10,
			dpr: 1,
		});
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'pane_not_initialized',
		});
	});
});

describe('renderWorker.handleRequest — destroy', () => {
	it('removes pane state and acks destroyed', () => {
		const { state } = init();
		const ack = handleRequest(state, { type: 'destroy', paneId: PANE });
		expect(ack).toEqual({ type: 'destroyed', paneId: PANE });
		expect(getPaneState(state, PANE)).toBeUndefined();
	});

	it('destroying an unknown pane is silent (still acks)', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, { type: 'destroy', paneId: PANE });
		expect(ack).toEqual({ type: 'destroyed', paneId: PANE });
	});
});

// P4.7 (2026-05-22) — wasm kernel adapter wiring. Uses a mock
// KernelAdapter to drive the init/applyDelta/destroy lifecycle without
// loading the real wasm module (which is unavailable in vitest's node
// env). Verifies both happy-path dispatch and structured error
// propagation when the kernel itself throws.
describe('renderWorker.handleRequest — wasm KernelAdapter wiring', () => {
	function makeMockKernel() {
		return {
			feed: vi.fn<(bytes: Uint8Array) => void>(),
			clearTerminalPreservingPrompt: vi.fn<() => void>(),
			applyDeltaFrame: vi.fn<(bytes: Uint8Array) => void>(),
			free: vi.fn<() => void>(),
		};
	}

	type MockKernel = ReturnType<typeof makeMockKernel>;
	type MockAdapter = KernelAdapter & {
		create: ReturnType<typeof vi.fn>;
		kernel: MockKernel;
	};

	function makeMockAdapter(kernel: MockKernel = makeMockKernel()): MockAdapter {
		return {
			kernel,
			create: vi.fn(() => kernel as unknown as KernelHandle),
		};
	}

	it('init calls adapter.create with the requested geometry and stores the kernel', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		const ack = handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 30, cols: 90, dpr: 2 },
				backend: 'webgpu',
				scrollbackLines: 7777,
			},
			adapter,
		);
		expect(ack).toEqual({ type: 'ready', paneId: PANE, backend: 'webgpu' });
		expect(adapter.create).toHaveBeenCalledOnce();
		expect(adapter.create).toHaveBeenCalledWith({
			rows: 30,
			cols: 90,
			scrollback: 7777,
		});
		expect(getPaneState(state, PANE)?.kernel).toBe(adapter.kernel);
	});

	it('applyDelta forwards the bytes into the kernel', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'canvas2d',
				scrollbackLines: 2000,
			},
			adapter,
		);
		const bytes = new Uint8Array([7, 8, 9]);
		const ack = handleRequest(
			state,
			{ type: 'applyDelta', paneId: PANE, bytes },
			adapter,
		);
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		expect(adapter.kernel.applyDeltaFrame).toHaveBeenCalledOnce();
		expect(adapter.kernel.applyDeltaFrame).toHaveBeenCalledWith(bytes);
	});

	it('accepts increasing frame ids and ignores stale replays', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, adapter);
		const accepted = handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([1]),
			frameId: 2,
		}, adapter);
		const callsAfterAccepted = adapter.kernel.applyDeltaFrame.mock.calls.length;
		const stale = handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([0]),
			frameId: 1,
		}, adapter);
		expect(accepted).toMatchObject({ type: 'ready', paneId: PANE });
		expect(stale).toMatchObject({ type: 'ready', paneId: PANE });
		expect(adapter.kernel.applyDeltaFrame).toHaveBeenCalledTimes(callsAfterAccepted);
		expect(getPaneState(state, PANE)?.lastAppliedFrameId).toBe(2);
	});

	it('rejects invalid frame ids before touching the kernel', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, adapter);
		const ack = handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([1]),
			frameId: 0,
		}, adapter);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'apply_delta_failed',
		});
		expect(adapter.kernel.applyDeltaFrame).not.toHaveBeenCalled();
	});

	it('feed forwards raw PTY bytes and renders the bound canvas', () => {
		const adapter = makeMockAdapter();
		const renderer = {
			render: vi.fn(),
			resize: vi.fn(),
			free: vi.fn(),
		} satisfies RendererHandle;
		adapter.createRenderer = vi.fn(() => renderer);
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, adapter);
		handleRequest(state, {
			type: 'bindCanvas',
			paneId: PANE,
			canvas: {} as OffscreenCanvas,
		}, adapter);
		renderer.render.mockClear();
		const bytes = new Uint8Array([0x1b, 0x5b, 0x48]);
		const ack = handleRequest(state, { type: 'feed', paneId: PANE, bytes }, adapter);
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		expect(adapter.kernel.feed).toHaveBeenCalledWith(bytes);
		expect(renderer.render).toHaveBeenCalledOnce();
	});

	it('feed shares render generations with delta frames and ignores stale replays', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, adapter);

		const first = handleRequest(state, {
			type: 'feed',
			paneId: PANE,
			bytes: new Uint8Array([1]),
			frameId: 2,
		}, adapter);
		const delta = handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([2]),
			frameId: 3,
		}, adapter);
		const stale = handleRequest(state, {
			type: 'feed',
			paneId: PANE,
			bytes: new Uint8Array([0]),
			frameId: 1,
		}, adapter);

		expect(first).toMatchObject({ type: 'ready', paneId: PANE });
		expect(delta).toMatchObject({ type: 'ready', paneId: PANE });
		expect(stale).toMatchObject({ type: 'ready', paneId: PANE });
		expect(adapter.kernel.feed).toHaveBeenCalledTimes(1);
		expect(adapter.kernel.applyDeltaFrame).toHaveBeenCalledTimes(1);
		expect(getPaneState(state, PANE)?.lastAppliedFrameId).toBe(3);
	});

	it('rejects invalid feed frame ids before touching the kernel', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, adapter);
		const ack = handleRequest(state, {
			type: 'feed',
			paneId: PANE,
			bytes: new Uint8Array([1]),
			frameId: 0,
		}, adapter);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'feed_failed',
		});
		expect(adapter.kernel.feed).not.toHaveBeenCalled();
	});

	it('destroy frees the kernel before removing pane state', () => {
		const adapter = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 100,
			},
			adapter,
		);
		const ack = handleRequest(
			state,
			{ type: 'destroy', paneId: PANE },
			adapter,
		);
		expect(ack).toEqual({ type: 'destroyed', paneId: PANE });
		expect(adapter.kernel.free).toHaveBeenCalledOnce();
		expect(getPaneState(state, PANE)).toBeUndefined();
	});

	it('adapter.create throwing → init returns apply_delta_failed error', () => {
		const adapter: KernelAdapter = {
			create: vi.fn(() => {
				throw new Error('wasm OOM');
			}),
		};
		const state = makeWorkerState();
		const ack = handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			adapter,
		);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'apply_delta_failed',
		});
		// State must NOT have been populated on a failed init.
		expect(getPaneState(state, PANE)).toBeUndefined();
	});

	it('kernel.applyDeltaFrame throwing → returns apply_delta_failed error', () => {
		const kernel = makeMockKernel();
		kernel.applyDeltaFrame.mockImplementation(() => {
			throw new Error('postcard decode failed');
		});
		const adapter = makeMockAdapter(kernel);
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			adapter,
		);
		const ack = handleRequest(
			state,
			{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([0]) },
			adapter,
		);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'apply_delta_failed',
		});
		// Pane state survives a delta error — kernel may recover on the next frame.
		expect(getPaneState(state, PANE)).toBeTruthy();
	});

	it('kernel.free throwing → destroy still acks destroyed (idempotent)', () => {
		const kernel = makeMockKernel();
		kernel.free.mockImplementation(() => {
			throw new Error('kernel already freed');
		});
		const adapter = makeMockAdapter(kernel);
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			adapter,
		);
		const ack = handleRequest(
			state,
			{ type: 'destroy', paneId: PANE },
			adapter,
		);
		expect(ack).toEqual({ type: 'destroyed', paneId: PANE });
		expect(getPaneState(state, PANE)).toBeUndefined();
	});

	it('omitting the adapter keeps the unit-test shadow path', () => {
		const state = makeWorkerState();
		const ack = handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
		);
		expect(ack).toEqual({ type: 'ready', paneId: PANE, backend: 'webgpu' });
		expect(getPaneState(state, PANE)?.kernel).toBeUndefined();

		const apply = handleRequest(
			state,
			{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([0]) },
		);
		expect(apply).toMatchObject({ type: 'ready', paneId: PANE });

		const destroy = handleRequest(
			state,
			{ type: 'destroy', paneId: PANE },
		);
		expect(destroy).toEqual({ type: 'destroyed', paneId: PANE });
	});
});

// P4.8 (2026-05-22) — Renderer adapter wiring. `bindCanvas` constructs
// the per-pane RendererHandle via the adapter; `applyDelta` drives the
// renderer alongside the kernel; `destroy` frees the renderer before
// the kernel. Mock OffscreenCanvas is a typed-cast stub — node env has
// no real OffscreenCanvas, and the worker never inspects it beyond
// passing it through to the adapter factory.
describe('renderWorker.handleRequest — Renderer adapter wiring (p4.8)', () => {
	function makeMockKernel() {
		return {
			applyDeltaFrame: vi.fn<(bytes: Uint8Array) => boolean | void>(),
			isSyncOutput: vi.fn(() => false),
			resize: vi.fn<(rows: number, cols: number) => void>(),
			free: vi.fn<() => void>(),
		};
	}
	function makeMockRenderer() {
		return {
			render: vi.fn<() => void>(),
			setPresentationCursorSuppressed: vi.fn<(suppressed: boolean) => void>(),
			resize: vi.fn<(widthCss: number, heightCss: number, dpr: number) => void>(),
			free: vi.fn<() => void>(),
			configure: vi.fn(() => ({ cellW: 9, cellH: 18 })),
		};
	}
	type MockKernel = ReturnType<typeof makeMockKernel>;
	type MockRenderer = ReturnType<typeof makeMockRenderer>;

	function makeMockAdapter(): {
		adapter: KernelAdapter;
		kernel: MockKernel;
		renderer: MockRenderer;
		createSpy: ReturnType<typeof vi.fn>;
		createRendererSpy: ReturnType<typeof vi.fn>;
	} {
		const kernel = makeMockKernel();
		const renderer = makeMockRenderer();
		const createSpy = vi.fn(() => kernel as unknown as KernelHandle);
		const createRendererSpy = vi.fn(() => renderer as unknown as RendererHandle);
		return {
			adapter: { create: createSpy, createRenderer: createRendererSpy },
			kernel,
			renderer,
			createSpy,
			createRendererSpy,
		};
	}

	// Fake OffscreenCanvas — only its identity matters here.
	const fakeCanvas = {} as unknown as OffscreenCanvas;

	function initAndBind(): {
		state: ReturnType<typeof makeWorkerState>;
		mocks: ReturnType<typeof makeMockAdapter>;
	} {
		const mocks = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 2 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			mocks.adapter,
		);
		handleRequest(
			state,
			{ type: 'bindCanvas', paneId: PANE, canvas: fakeCanvas },
			mocks.adapter,
		);
		return { state, mocks };
	}

	it('bindCanvas creates the renderer via the adapter and stores it', () => {
		const { state, mocks } = initAndBind();
		expect(mocks.createRendererSpy).toHaveBeenCalledOnce();
		const arg = mocks.createRendererSpy.mock.calls[0][0] as {
			canvas: OffscreenCanvas;
			kernel: KernelHandle;
			backend: string;
		};
		expect(arg.canvas).toBe(fakeCanvas);
		expect(arg.kernel).toBe(mocks.kernel);
		expect(arg.backend).toBe('webgpu');
		const pane = getPaneState(state, PANE);
		expect(pane?.renderer).toBe(mocks.renderer);
		expect(pane?.canvasBound).toBe(true);
	});

	it('applyDelta drives both kernel.applyDeltaFrame and renderer.render', () => {
		const { state, mocks } = initAndBind();
		mocks.renderer.render.mockClear();
		const bytes = new Uint8Array([4, 5, 6]);
		const ack = handleRequest(
			state,
			{ type: 'applyDelta', paneId: PANE, bytes },
			mocks.adapter,
		);
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		expect(mocks.kernel.applyDeltaFrame).toHaveBeenCalledWith(bytes);
		expect(mocks.renderer.render).toHaveBeenCalledOnce();
		// Ordering: kernel BEFORE renderer (so render sees the new grid).
		const kernelOrder = mocks.kernel.applyDeltaFrame.mock.invocationCallOrder[0];
		const rendererOrder = mocks.renderer.render.mock.invocationCallOrder[0];
		expect(kernelOrder).toBeLessThan(rendererOrder);
	});

	it('uses standard synchronized-output boundaries without an output delay', () => {
		vi.useFakeTimers();
		try {
			const { state, mocks } = initAndBind();
			mocks.renderer.render.mockClear();
			mocks.kernel.isSyncOutput.mockReturnValue(true);

			handleRequest(
				state,
				{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([6]) },
				mocks.adapter,
			);
			expect(mocks.renderer.render).not.toHaveBeenCalled();

			mocks.kernel.isSyncOutput.mockReturnValue(false);
			handleRequest(
				state,
				{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([7]) },
				mocks.adapter,
			);
			expect(mocks.renderer.render).toHaveBeenCalledOnce();
			expect(getPaneState(state, PANE)?.syncStart).toBeNull();
		} finally {
			vi.useRealTimers();
		}
	});

	it('renders only one safety frame for a stuck synchronized-output transaction', () => {
		vi.useFakeTimers();
		try {
			const { state, mocks } = initAndBind();
			mocks.renderer.render.mockClear();
			mocks.kernel.isSyncOutput.mockReturnValue(true);

			handleRequest(
				state,
				{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([6]) },
				mocks.adapter,
			);
			vi.advanceTimersByTime(SYNC_OUTPUT_TIMEOUT_MS);
			expect(mocks.renderer.render).toHaveBeenCalledOnce();

			handleRequest(
				state,
				{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([7]) },
				mocks.adapter,
			);
			expect(mocks.renderer.render).toHaveBeenCalledOnce();
		} finally {
			vi.useRealTimers();
		}
	});

	it('settles a native repaint cursor without delaying grid paints', () => {
		vi.useFakeTimers();
		try {
			const { state, mocks } = initAndBind();
			mocks.renderer.render.mockClear();
			mocks.kernel.applyDeltaFrame.mockReturnValue(true);

			handleRequest(
				state,
				{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([7]) },
				mocks.adapter,
			);
			expect(mocks.renderer.setPresentationCursorSuppressed).toHaveBeenCalledWith(true);
			expect(mocks.renderer.render).toHaveBeenCalledOnce();

			vi.advanceTimersByTime(23);
			expect(mocks.renderer.setPresentationCursorSuppressed).not.toHaveBeenCalledWith(false);
			vi.advanceTimersByTime(1);
			expect(mocks.renderer.setPresentationCursorSuppressed).toHaveBeenLastCalledWith(false);
			expect(mocks.renderer.render).toHaveBeenCalledTimes(2);

			mocks.renderer.render.mockClear();
			mocks.kernel.applyDeltaFrame.mockReturnValue(false);
			handleRequest(
				state,
				{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([8]) },
				mocks.adapter,
			);
			expect(mocks.renderer.render).toHaveBeenCalledOnce();
		} finally {
			vi.useRealTimers();
		}
	});

	it('does not repaint a replayed frame after a newer frame was accepted', () => {
		const { state, mocks } = initAndBind();
		mocks.renderer.render.mockClear();
		const trace = [
			{ frameId: 1, byte: 1 },
			{ frameId: 2, byte: 2 },
			{ frameId: 4, byte: 4 },
			{ frameId: 3, byte: 3 },
			{ frameId: 4, byte: 4 },
		];
		const replies = trace.map(({ frameId, byte }) => handleRequest(state, {
			type: 'applyDelta',
			paneId: PANE,
			bytes: new Uint8Array([byte]),
			frameId,
		}, mocks.adapter));
		const renderCallsAfterTrace = mocks.renderer.render.mock.calls.length;
		expect(replies).toHaveLength(trace.length);
		expect(replies.every((reply) => reply.type === 'ready' && reply.paneId === PANE)).toBe(true);
		expect(renderCallsAfterTrace).toBe(3);
		expect(mocks.kernel.applyDeltaFrame).toHaveBeenCalledTimes(3);
		expect(getPaneState(state, PANE)?.lastAppliedFrameId).toBe(4);
	});

	it('bind configures real metrics; resize drives kernel, surface, then render', () => {
		const mocks = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(state, {
			type: 'init',
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 2 },
			backend: 'canvas2d',
			scrollbackLines: 2000,
		}, mocks.adapter);
		const bind = handleRequest(state, {
			type: 'bindCanvas',
			paneId: PANE,
			canvas: fakeCanvas,
			font: 'monospace',
			fontSizePx: 15,
			dpr: 2,
		}, mocks.adapter);
		expect(bind).toMatchObject({ type: 'ready', cellW: 9, cellH: 18 });
		mocks.renderer.render.mockClear();

		const resized = handleRequest(state, {
			type: 'resize',
			paneId: PANE,
			rows: 30,
			cols: 100,
			dpr: 2,
			wCss: 800,
			hCss: 480,
		}, mocks.adapter);
		expect(resized).toMatchObject({ type: 'ready', paneId: PANE });
		expect(mocks.kernel.resize).toHaveBeenCalledWith(30, 100);
		expect(mocks.renderer.resize).toHaveBeenCalledWith(800, 480, 2);
		expect(mocks.renderer.render).toHaveBeenCalledOnce();
	});

	it('releaseCanvas frees only the renderer and keeps the kernel for parked output', () => {
		const { state, mocks } = initAndBind();
		const released = handleRequest(
			state,
			{ type: 'releaseCanvas', paneId: PANE },
			mocks.adapter,
		);
		expect(released).toMatchObject({ type: 'ready', paneId: PANE });
		expect(mocks.renderer.free).toHaveBeenCalledOnce();
		expect(mocks.kernel.free).not.toHaveBeenCalled();
		expect(getPaneState(state, PANE)).toMatchObject({
			canvasBound: false,
			kernel: mocks.kernel,
		});
		expect(getPaneState(state, PANE)?.renderer).toBeUndefined();
	});

	it('destroy frees renderer BEFORE kernel', () => {
		const { state, mocks } = initAndBind();
		handleRequest(state, { type: 'destroy', paneId: PANE }, mocks.adapter);
		expect(mocks.renderer.free).toHaveBeenCalledOnce();
		expect(mocks.kernel.free).toHaveBeenCalledOnce();
		const rendererFreeOrder = mocks.renderer.free.mock.invocationCallOrder[0];
		const kernelFreeOrder = mocks.kernel.free.mock.invocationCallOrder[0];
		expect(rendererFreeOrder).toBeLessThan(kernelFreeOrder);
	});

	it('createRenderer throwing → bindCanvas returns apply_delta_failed', () => {
		const kernel = makeMockKernel();
		const adapter: KernelAdapter = {
			create: vi.fn(() => kernel as unknown as KernelHandle),
			createRenderer: vi.fn(() => {
				throw new Error('WebGPU adapter unavailable');
			}),
		};
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			adapter,
		);
		const ack = handleRequest(
			state,
			{ type: 'bindCanvas', paneId: PANE, canvas: fakeCanvas },
			adapter,
		);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'apply_delta_failed',
		});
		// canvasBound still flips so the host knows the message reached
		// the dispatcher (matches the pre-existing P4.5/P4.6 semantics).
		expect(getPaneState(state, PANE)?.canvasBound).toBe(true);
	});

	it('renderer.render throwing → applyDelta returns apply_delta_failed; kernel already applied', () => {
		const { state, mocks } = initAndBind();
		mocks.renderer.render.mockImplementation(() => {
			throw new Error('canvas context lost');
		});
		const bytes = new Uint8Array([0]);
		const ack = handleRequest(
			state,
			{ type: 'applyDelta', paneId: PANE, bytes },
			mocks.adapter,
		);
		expect(ack).toMatchObject({
			type: 'error',
			paneId: PANE,
			code: 'apply_delta_failed',
		});
		// Kernel still consumed the bytes before the renderer failed.
		expect(mocks.kernel.applyDeltaFrame).toHaveBeenCalledWith(bytes);
	});

	it('bindCanvas with no createRenderer on adapter → pre-P4.8 behavior (just flips canvasBound)', () => {
		// Use kernel-only adapter (no createRenderer)
		const kernel = makeMockKernel();
		const adapter: KernelAdapter = {
			create: vi.fn(() => kernel as unknown as KernelHandle),
		};
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			adapter,
		);
		const ack = handleRequest(
			state,
			{ type: 'bindCanvas', paneId: PANE, canvas: fakeCanvas },
			adapter,
		);
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		const pane = getPaneState(state, PANE);
		expect(pane?.canvasBound).toBe(true);
		expect(pane?.renderer).toBeUndefined();
	});

	it('bindCanvas without canvas payload → no renderer created (e.g. legacy host calls)', () => {
		const mocks = makeMockAdapter();
		const state = makeWorkerState();
		handleRequest(
			state,
			{
				type: 'init',
				paneId: PANE,
				dims: { rows: 24, cols: 80, dpr: 1 },
				backend: 'webgpu',
				scrollbackLines: 2000,
			},
			mocks.adapter,
		);
		const ack = handleRequest(
			state,
			{ type: 'bindCanvas', paneId: PANE }, // no canvas field
			mocks.adapter,
		);
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
		expect(mocks.createRendererSpy).not.toHaveBeenCalled();
		expect(getPaneState(state, PANE)?.renderer).toBeUndefined();
	});
});
