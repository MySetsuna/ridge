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
			{ type: 'feed' },
			{ type: 'resize' },
			{ type: 'destroy' },
			{ type: 'ping' },
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
			data: 'hello',
		});
		expect(ack).toMatchObject({ type: 'ready', paneId: PANE });
	});

	it('feed on an unknown pane → pane_not_initialized', () => {
		const state = makeWorkerState();
		const ack = handleRequest(state, { type: 'feed', paneId: PANE, data: 'x' });
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

	it('omitting / nulling the adapter behaves like the pre-P4.7 shadow path', () => {
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
			null,
		);
		expect(ack).toEqual({ type: 'ready', paneId: PANE, backend: 'webgpu' });
		expect(getPaneState(state, PANE)?.kernel).toBeUndefined();

		const apply = handleRequest(
			state,
			{ type: 'applyDelta', paneId: PANE, bytes: new Uint8Array([0]) },
			null,
		);
		expect(apply).toMatchObject({ type: 'ready', paneId: PANE });

		const destroy = handleRequest(
			state,
			{ type: 'destroy', paneId: PANE },
			null,
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
			applyDeltaFrame: vi.fn<(bytes: Uint8Array) => void>(),
			free: vi.fn<() => void>(),
		};
	}
	function makeMockRenderer() {
		return {
			render: vi.fn<() => void>(),
			free: vi.fn<() => void>(),
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
