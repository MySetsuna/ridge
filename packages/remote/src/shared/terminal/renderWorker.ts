/**
 * P4.5 (2026-05-21) — Render-worker entry (scaffold).
 *
 * This module ships in two halves:
 *
 *   1. `handleRequest(state, request) -> response` — a pure function
 *      that takes a `WorkerState` and a request, returns the response
 *      the worker would post back. No `self`/`postMessage` references
 *      and no wasm imports yet, so it is unit-testable in plain
 *      vitest under the `node` environment.
 *
 *   2. The Worker bootstrap at the bottom of the file (guarded by
 *      `isInWorkerScope()`) wires `self.onmessage` to `handleRequest`
 *      and `self.postMessage` for replies. When this file is loaded
 *      via `new Worker(new URL('./renderWorker.ts', import.meta.url),
 *      { type: 'module' })` the bootstrap runs; under vitest it does
 *      NOT, because `self.constructor.name` is not
 *      `DedicatedWorkerGlobalScope` outside a real worker.
 *
 * P4.5 scope is intentionally tiny: stand up the message-dispatch
 * skeleton with state tracking and per-pane bookkeeping, but DO NOT
 * import wasm yet. P4.6 will land the OffscreenCanvas transfer; P4.9
 * will land the wasm kernel ownership inside the worker.
 */

import {
	isRenderWorkerRequest,
	type RenderWorkerRequest,
	type RenderWorkerResponse,
	type RendererBackend,
} from './renderWorker.protocol';
import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';

/** Minimal slice of the wasm `TerminalKernel` the worker drives. Stays
 *  structural so tests can pass a mock without pulling in the real
 *  wasm module (which is unavailable in vitest's node env). */
export interface KernelHandle {
	feed(bytes: Uint8Array): void;
	clearTerminalPreservingPrompt(): void;
	applyDeltaFrame(bytes: Uint8Array): void;
	resize(rows: number, cols: number): void;
	free(): void;
}

/** Minimal slice of the wasm `RenderHandle` the worker drives. P4.8
 *  scaffolding (2026-05-22): only `render()` and `free()` are reachable
 *  from the worker so far — the protocol's `resize` carries rows/cols
 *  rather than pixel CSS dims, so the wasm-side `resize(w_css, h_css,
 *  dpr)` isn't wired yet. A future protocol extension will pass CSS
 *  dims through. Kept structural so tests can mock it. */
export interface RendererHandle {
	render(): void;
	resize(widthCss: number, heightCss: number, dpr: number): void;
	free(): void;
	/** Re-measure cell metrics with a new font config. Returns the
	 *  quantized cellW / cellH computed from the renderer's font
	 *  measurement. When absent the worker returns a `ready` ack
	 *  without cell metrics (the host treats it as no-op). */
	configure?(family: string, sizePx: number, dpr: number): { cellW: number; cellH: number };
}

/** Dependency-injection seam for the wasm kernel and (optionally) the
 *  per-pane renderer. When the bootstrap has finished loading
 *  `@ridge/term-wasm` it constructs a `KernelAdapter`; until then it's
 *  null and `handleRequest` still acks every request but skips kernel
 *  work. Tests inject a mock to drive the init/apply/destroy lifecycle
 *  without wasm.
 *
 *  P4.7 (2026-05-22): kernel side only — `create` populates per-pane
 *  `TerminalKernel`.
 *
 *  P4.8 (2026-05-22): optional `createRenderer` populates a per-pane
 *  `RenderHandle`. The production loader doesn't wire this yet because
 *  the wasm `RenderHandle` constructor only accepts `HtmlCanvasElement`
 *  (not `OffscreenCanvas`) — that needs a Rust-side change to
 *  `Canvas2dBackend::new` first. The JS surface is in place so the
 *  Rust change can land independently and the production adapter just
 *  starts returning a non-undefined `createRenderer`.
 */
export interface KernelAdapter {
	create(args: { rows: number; cols: number; scrollback: number }): KernelHandle;
	/** Optional renderer factory. When present and `bindCanvas` is
	 *  called with a canvas, the worker stores the returned
	 *  `RendererHandle` on the pane and drives it from `applyDelta`. */
	createRenderer?(args: {
		canvas: OffscreenCanvas;
		kernel: KernelHandle;
		backend: RendererBackend;
	}): RendererHandle;
}

/** Per-pane state the worker tracks. Future iterations grow this to
 *  include the offscreen canvas reference and the per-row hash cache
 *  currently in manager.ts. */
export interface PaneWorkerState {
	rows: number;
	cols: number;
	dpr: number;
	backend: RendererBackend;
	scrollbackLines: number;
	canvasBound: boolean;
	/** P4.7: wasm kernel mirror, present iff a `KernelAdapter` was
	 *  available when `init` ran. */
	kernel?: KernelHandle;
	/** P4.8: per-pane renderer, present iff `bindCanvas` arrived AND
	 *  the adapter exposed `createRenderer`. Drawn from on every
	 *  successful `applyDelta`. */
	renderer?: RendererHandle;
	/** Last accepted render generation. Replayed/late feed or delta frames must not revive old rows. */
	lastAppliedFrameId: number;
}

/** The whole worker's state is a Map keyed by paneId. Stays in JS
 *  closure of the bootstrap; exported as a type so the test fixture
 *  can stub it. */
export type WorkerState = Map<string, PaneWorkerState>;

export function makeWorkerState(): WorkerState {
	return new Map();
}

/**
 * Pure request → response dispatcher. No I/O, no postMessage. The
 * Worker bootstrap calls this and forwards the response with
 * `self.postMessage`. Tests call it directly.
 *
 * The optional `adapter` is the bridge to the real wasm kernel. When
 * provided, `init` constructs a per-pane `KernelHandle`, `applyDelta`
 * feeds bytes into it, and `destroy` frees it. Tests omit the adapter
 * (or pass a mock) to exercise the dispatch protocol without wasm.
 */
type RequestOf<T extends RenderWorkerRequest['type']> = Extract<RenderWorkerRequest, { type: T }>;
type ErrorCode = Extract<RenderWorkerResponse, { type: 'error' }>['code'];

function workerError(
	paneId: string | undefined,
	code: ErrorCode,
	message: string,
): RenderWorkerResponse {
	return { type: 'error', paneId, code, message };
}

function paneMissing(
	paneId: string,
	operation: string,
): RenderWorkerResponse {
	return workerError(paneId, 'pane_not_initialized', operation + ' before init for pane ' + paneId);
}

function frameGuard(
	pane: PaneWorkerState,
	paneId: string,
	frameId: number | undefined,
	code: Extract<ErrorCode, 'apply_delta_failed' | 'feed_failed' | 'clear_failed'>,
): RenderWorkerResponse | null {
	if (frameId === undefined) return null;
	if (!Number.isSafeInteger(frameId) || frameId <= 0) {
		return workerError(paneId, code, 'invalid frameId: ' + frameId);
	}
	if (frameId <= pane.lastAppliedFrameId) {
		return { type: 'ready', paneId, backend: pane.backend };
	}
	return null;
}

function handleInit(
	state: WorkerState,
	request: RequestOf<'init'>,
	adapter?: KernelAdapter | null,
): RenderWorkerResponse {
	if (state.has(request.paneId)) {
		return workerError(
			request.paneId,
			'pane_already_initialized',
			'pane ' + request.paneId + ' already initialized',
		);
	}
	if (adapter === null) {
		return workerError(
			request.paneId,
			'apply_delta_failed',
			'render worker wasm adapter unavailable',
		);
	}
	let kernel: KernelHandle | undefined;
	if (adapter) {
		try {
			kernel = adapter.create({
				rows: request.dims.rows,
				cols: request.dims.cols,
				scrollback: request.scrollbackLines,
			});
		} catch (error) {
			return workerError(
				request.paneId,
				'apply_delta_failed',
				'kernel.create failed: ' + (error instanceof Error ? error.message : String(error)),
			);
		}
	}
	state.set(request.paneId, {
		rows: request.dims.rows,
		cols: request.dims.cols,
		dpr: request.dims.dpr,
		backend: request.backend,
		scrollbackLines: request.scrollbackLines,
		canvasBound: false,
		kernel,
		lastAppliedFrameId: 0,
	});
	return { type: 'ready', paneId: request.paneId, backend: request.backend };
}

function handleBindCanvas(
	state: WorkerState,
	request: RequestOf<'bindCanvas'>,
	adapter?: KernelAdapter | null,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	if (!pane) return paneMissing(request.paneId, 'bindCanvas');
	pane.canvasBound = true;
	let cellW: number | undefined;
	let cellH: number | undefined;
	if (adapter?.createRenderer && request.canvas && pane.kernel) {
		try {
			try {
				pane.renderer?.free();
			} catch {
				// A detached renderer is best effort during replacement.
			}
			pane.renderer = adapter.createRenderer({
				canvas: request.canvas,
				kernel: pane.kernel,
				backend: pane.backend,
			});
			if (
				pane.renderer.configure &&
				request.font &&
				typeof request.fontSizePx === 'number' &&
				typeof request.dpr === 'number'
			) {
				const metrics = pane.renderer.configure(request.font, request.fontSizePx, request.dpr);
				cellW = metrics.cellW;
				cellH = metrics.cellH;
			}
			pane.renderer.render();
		} catch (error) {
			return workerError(
				request.paneId,
				'apply_delta_failed',
				'createRenderer failed: ' + (error instanceof Error ? error.message : String(error)),
			);
		}
	}
	return { type: 'ready', paneId: request.paneId, backend: pane.backend, cellW, cellH };
}

function handleApplyDelta(
	state: WorkerState,
	request: RequestOf<'applyDelta'>,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	if (!pane) return paneMissing(request.paneId, 'applyDelta');
	const guard = frameGuard(pane, request.paneId, request.frameId, 'apply_delta_failed');
	if (guard) return guard;
	try {
		pane.kernel?.applyDeltaFrame(request.bytes);
		pane.renderer?.render();
	} catch (error) {
		return workerError(
			request.paneId,
			'apply_delta_failed',
			'applyDelta failed: ' + (error instanceof Error ? error.message : String(error)),
		);
	}
	if (request.frameId !== undefined) pane.lastAppliedFrameId = request.frameId;
	return { type: 'ready', paneId: request.paneId, backend: pane.backend };
}

function handleFeed(
	state: WorkerState,
	request: RequestOf<'feed'>,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	if (!pane) return paneMissing(request.paneId, 'feed');
	const guard = frameGuard(pane, request.paneId, request.frameId, 'feed_failed');
	if (guard) return guard;
	try {
		pane.kernel?.feed(request.bytes);
		pane.renderer?.render();
	} catch (error) {
		return workerError(
			request.paneId,
			'feed_failed',
			'kernel.feed failed: ' + (error instanceof Error ? error.message : String(error)),
		);
	}
	if (request.frameId !== undefined) pane.lastAppliedFrameId = request.frameId;
	return { type: 'ready', paneId: request.paneId, backend: pane.backend };
}

function handleClearTerminalPreservingPrompt(
	state: WorkerState,
	request: RequestOf<'clearTerminalPreservingPrompt'>,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	if (!pane) return paneMissing(request.paneId, 'clearTerminalPreservingPrompt');
	const guard = frameGuard(pane, request.paneId, request.frameId, 'clear_failed');
	if (guard) return guard;
	try {
		pane.kernel?.clearTerminalPreservingPrompt();
		pane.renderer?.render();
	} catch (error) {
		return workerError(
			request.paneId,
			'clear_failed',
			'clearTerminalPreservingPrompt failed: ' + (error instanceof Error ? error.message : String(error)),
		);
	}
	if (request.frameId !== undefined) pane.lastAppliedFrameId = request.frameId;
	return { type: 'ready', paneId: request.paneId, backend: pane.backend };
}

function handleResize(
	state: WorkerState,
	request: RequestOf<'resize'>,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	if (!pane) return paneMissing(request.paneId, 'resize');
	pane.rows = request.rows;
	pane.cols = request.cols;
	pane.dpr = request.dpr;
	try {
		pane.kernel?.resize(request.rows, request.cols);
		if (
			pane.renderer &&
			typeof request.wCss === 'number' &&
			typeof request.hCss === 'number'
		) {
			pane.renderer.resize(request.wCss, request.hCss, request.dpr);
			pane.renderer.render();
		}
	} catch (error) {
		return workerError(
			request.paneId,
			'resize_failed',
			'resize failed: ' + (error instanceof Error ? error.message : String(error)),
		);
	}
	return { type: 'ready', paneId: request.paneId, backend: pane.backend };
}

function handleDestroy(
	state: WorkerState,
	request: RequestOf<'destroy'>,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	try {
		pane?.renderer?.free();
	} catch {
		// Destroy is idempotent when the renderer was already released.
	}
	try {
		pane?.kernel?.free();
	} catch {
		// Destroy is idempotent when the kernel was already released.
	}
	state.delete(request.paneId);
	return { type: 'destroyed', paneId: request.paneId };
}

function handleSetFont(
	state: WorkerState,
	request: RequestOf<'setFont'>,
): RenderWorkerResponse {
	const pane = state.get(request.paneId);
	if (!pane) return paneMissing(request.paneId, 'setFont');
	if (pane.renderer?.configure) {
		try {
			pane.renderer.configure(request.family, request.sizePx, request.dpr);
		} catch {
			return workerError(
				request.paneId,
				'resize_failed',
				'setFont configure failed for pane ' + request.paneId,
			);
		}
	}
	return { type: 'ready', paneId: request.paneId, backend: pane.backend };
}

export function handleRequest(
	state: WorkerState,
	request: RenderWorkerRequest,
	adapter?: KernelAdapter | null,
): RenderWorkerResponse {
	switch (request.type) {
		case 'ping':
			return { type: 'pong', token: request.token };
		case 'init':
			return handleInit(state, request, adapter);
		case 'bindCanvas':
			return handleBindCanvas(state, request, adapter);
		case 'applyDelta':
			return handleApplyDelta(state, request);
		case 'releaseCanvas': {
			const pane = state.get(request.paneId);
			if (!pane) return paneMissing(request.paneId, 'releaseCanvas');
			try {
				pane.renderer?.free();
			} catch {
				// Release remains safe when the renderer is already freed.
			}
			pane.renderer = undefined;
			pane.canvasBound = false;
			return { type: 'ready', paneId: request.paneId, backend: pane.backend };
		}
		case 'feed':
			return handleFeed(state, request);
		case 'clearTerminalPreservingPrompt':
			return handleClearTerminalPreservingPrompt(state, request);
		case 'resize':
			return handleResize(state, request);
		case 'destroy':
			return handleDestroy(state, request);
		case 'setFont':
			return handleSetFont(state, request);
	}
}


/**
 * Look up the per-pane state. Test-only helper; the worker bootstrap
 * doesn't need it. Kept exported so tests don't have to reach into the
 * Map themselves.
 */
export function getPaneState(state: WorkerState, paneId: string): PaneWorkerState | undefined {
	return state.get(paneId);
}

// ---------------------------------------------------------------------------
// Worker bootstrap. Skipped under vitest / SSR.
// ---------------------------------------------------------------------------

/**
 * Returns true when this module is loaded inside a real DedicatedWorker.
 * Under vitest (`environment: 'node'`) and during SSR there is no
 * `WorkerGlobalScope`, so the bootstrap stays inert.
 */
function isInWorkerScope(): boolean {
	if (typeof self === 'undefined') return false;
	// `self.constructor` is `DedicatedWorkerGlobalScope` inside a worker
	// and `Window` or `Object` everywhere else. Check by name to avoid
	// referencing a global the test environment doesn't define.
	const name =
		(self as { constructor?: { name?: string } }).constructor?.name ?? '';
	return (
		name === 'DedicatedWorkerGlobalScope' ||
		name === 'SharedWorkerGlobalScope'
	);
}

/** Minimal slice of `DedicatedWorkerGlobalScope` we actually touch in
 *  the bootstrap. Declared structurally so we don't need to include
 *  `lib: ['webworker']` in tsconfig — the file is compiled with the DOM
 *  lib (because vitest test files share the tsconfig) and the named
 *  worker globals are not in that lib. */
interface WorkerScopeLike {
	postMessage(message: unknown, transfer?: Transferable[]): void;
	addEventListener(
		type: 'message',
		listener: (event: MessageEvent<unknown>) => void,
	): void;
}

/**
 * P4.7 (2026-05-22) — async wasm-kernel loader. Imports `@ridge/term-wasm`
 * once (the module-level singleton), awaits its async init, and returns
 * a `KernelAdapter` that constructs `TerminalKernel` instances on
 * demand. If anything goes wrong (network, OOM, missing wasm file),
 * returns null and the bootstrap installs the listener without an
 * adapter — handleRequest then acks every request but creates no
 * kernel, matching the P4.5/P4.6 shadow-mode behavior.
 *
 * Kept inside the bootstrap so the import only runs in a real worker;
 * vitest's node env never reaches it because `isInWorkerScope()` is
 * false.
 */
async function loadKernelAdapter(): Promise<KernelAdapter | null> {
	try {
		const wasm = await import('@ridge/term-wasm');
		// Keep worker startup aligned with TerminalManager.ready(). The
		// package's default URL is relative to the worker chunk and can point
		// at a non-existent dev/remote asset, leaving the bootstrap awaiting
		// WASM forever before it installs its message listener.
		await wasm.default(wasmUrl);
		return {
			create({ rows, cols, scrollback }) {
				return new wasm.TerminalKernel(rows, cols, scrollback);
			},
			createRenderer({ canvas, kernel }) {
				const handle = wasm.RenderHandle.newFromOffscreen(canvas);
				const typedKernel = kernel as InstanceType<typeof wasm.TerminalKernel>;
				return {
					render: () => {
						handle.render(typedKernel);
					},
					resize: (widthCss, heightCss, dpr) => {
						handle.resize(widthCss, heightCss, dpr);
					},
					configure: (family, sizePx, dpr) => {
						const metrics = handle.configure(family, sizePx, dpr);
						return {
							cellW: Number(metrics[0]),
							cellH: Number(metrics[1]),
						};
					},
					free: () => handle.free(),
				};
			},
		};
	} catch (err) {
		// eslint-disable-next-line no-console
		console.warn(
			'[ridge-term/worker] wasm kernel adapter failed to load — running in shadow mode',
			err,
		);
		return null;
	}
}

function handleWorkerMessage(
	state: WorkerState,
	scope: WorkerScopeLike,
	event: MessageEvent<unknown>,
	adapter: KernelAdapter | null | undefined,
): void {
	const id = (event.data as { __reqId?: number } | null)?.__reqId;
	if (!isRenderWorkerRequest(event.data)) {
		scope.postMessage({
			type: 'error',
			code: 'unknown_message',
			message: `unknown request shape: ${JSON.stringify(event.data)}`,
			__reqId: id,
		});
		return;
	}
	if (event.data.type === 'ping') {
		scope.postMessage({ ...handleRequest(state, event.data), __reqId: id });
		return;
	}
	if (adapter === undefined) {
		scope.postMessage({
			type: 'error',
			paneId: 'paneId' in event.data ? event.data.paneId : undefined,
			code: 'apply_delta_failed',
			message: 'render worker wasm adapter is still loading',
			__reqId: id,
		});
		return;
	}
	scope.postMessage({ ...handleRequest(state, event.data, adapter), __reqId: id });
}

if (isInWorkerScope()) {
	const state = makeWorkerState();
	const scope = self as unknown as WorkerScopeLike;
	// Install the control-plane listener before WASM starts loading. WebView2
	// can spend seconds compiling the cold kernel; delaying the listener makes
	// the host's first `init` request look like a hung worker and blocks the
	// main-thread fallback. `undefined` means loading, `null` means failed.
	let adapter: KernelAdapter | null | undefined;

	scope.addEventListener('message', (event) => handleWorkerMessage(state, scope, event, adapter));

	// P4.7 + Iter 15 (2026-05-22) — adapter failure remains explicit: an
	// adapter of `null` makes `init` fail, causing the host to restore the
	// live main-thread mirror. The listener above stays available throughout.
	adapter = await loadKernelAdapter();
}
