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

/** Minimal slice of the wasm `TerminalKernel` the worker drives. Stays
 *  structural so tests can pass a mock without pulling in the real
 *  wasm module (which is unavailable in vitest's node env). */
export interface KernelHandle {
	applyDeltaFrame(bytes: Uint8Array): void;
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
	free(): void;
	/** §p4 ITER 5 (2026-05-22) — measure cell metrics for `font` /
	 *  `sizePx` at `dpr`. Returns `[cellW, cellH]` in CSS pixels.
	 *  Optional so mock handles in tests don't have to stub it; the
	 *  bindCanvas handler skips the measure step when this method
	 *  isn't present. */
	configure?(font: string, sizePx: number, dpr: number): readonly [number, number];
	/** §p4 ITER 7 (2026-05-22) — resize the renderer's backing
	 *  surface. `wCss` / `hCss` are CSS pixels at `dpr`. Optional —
	 *  mock handles in tests can skip it and the resize handler
	 *  treats the call as a wasm-kernel-only resize. */
	resize?(wCss: number, hCss: number, dpr: number): void;
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
export function handleRequest(
	state: WorkerState,
	request: RenderWorkerRequest,
	adapter?: KernelAdapter | null,
): RenderWorkerResponse {
	switch (request.type) {
		case 'ping':
			return { type: 'pong', token: request.token };

		case 'init': {
			if (state.has(request.paneId)) {
				return {
					type: 'error',
					paneId: request.paneId,
					code: 'pane_already_initialized',
					message: `pane ${request.paneId} already initialized`,
				};
			}
			let kernel: KernelHandle | undefined;
			if (adapter) {
				try {
					kernel = adapter.create({
						rows: request.dims.rows,
						cols: request.dims.cols,
						scrollback: request.scrollbackLines,
					});
				} catch (err) {
					// Kernel construction failed (wasm OOM, bad args, etc.).
					// Surface as a structured error so the host can decide
					// whether to retry or fall back to the legacy path.
					return {
						type: 'error',
						paneId: request.paneId,
						code: 'apply_delta_failed',
						message: `kernel.create failed: ${err instanceof Error ? err.message : String(err)}`,
					};
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
			});
			return {
				type: 'ready',
				paneId: request.paneId,
				// P4.5 doesn't actually load WebGPU yet, so we honor the
				// requested backend in the ack. P4.9 swaps this for the
				// real `try webgpu else canvas2d` probe.
				backend: request.backend,
			};
		}

		case 'bindCanvas': {
			const pane = state.get(request.paneId);
			if (!pane) {
				return {
					type: 'error',
					paneId: request.paneId,
					code: 'pane_not_initialized',
					message: `bindCanvas before init for pane ${request.paneId}`,
				};
			}
			pane.canvasBound = true;
			// P4.8 (2026-05-22): when the adapter exposes `createRenderer`
			// AND the request carries a canvas AND we have a kernel,
			// construct the per-pane RendererHandle. All three guards must
			// pass — otherwise we silently keep the pre-P4.8 behavior
			// (canvasBound flag only). The renderer factory may throw
			// (WebGPU adapter miss, malformed canvas); surface as
			// structured error so the host can decide to retry or fall
			// back. Pane state retains `canvasBound=true` even on factory
			// failure so a follow-up retry doesn't re-trigger pane init.
			if (adapter?.createRenderer && request.canvas && pane.kernel) {
				try {
					pane.renderer = adapter.createRenderer({
						canvas: request.canvas,
						kernel: pane.kernel,
						backend: pane.backend,
					});
				} catch (err) {
					return {
						type: 'error',
						paneId: request.paneId,
						code: 'apply_delta_failed',
						message: `createRenderer failed: ${err instanceof Error ? err.message : String(err)}`,
					};
				}
			}
			// §p4 ITER 5 (2026-05-22) — measure cell metrics inside
			// the worker so the host can seed `entry.cellW / cellH`
			// from real font metrics instead of the 8 × 16 placeholder.
			// Only fires when the host supplied font/size/dpr AND the
			// renderer exposes `configure`. Failures here are
			// non-fatal — the bindCanvas ack still goes through
			// without cell metrics; the next host-side fitPane will
			// recover via the worker's own `resize` accounting.
			let cellW: number | undefined;
			let cellH: number | undefined;
			if (
				pane.renderer?.configure &&
				request.font !== undefined &&
				request.fontSizePx !== undefined &&
				request.dpr !== undefined
			) {
				try {
					const [w, h] = pane.renderer.configure(
						request.font,
						request.fontSizePx,
						request.dpr,
					);
					cellW = Number(w);
					cellH = Number(h);
				} catch {
					// Swallow — fall through with undefined metrics.
				}
			}
			return {
				type: 'ready',
				paneId: request.paneId,
				backend: pane.backend,
				cellW,
				cellH,
			};
		}

		case 'applyDelta': {
			const pane = state.get(request.paneId);
			if (!pane) {
				return {
					type: 'error',
					paneId: request.paneId,
					code: 'pane_not_initialized',
					message: `applyDelta before init for pane ${request.paneId}`,
				};
			}
			// P4.7 (2026-05-22): when the wasm kernel adapter loaded
			// successfully at bootstrap, drive the per-pane kernel here.
			// When it failed to load (or the adapter wasn't provided,
			// e.g. tests), we silently ack — protocol surface is the same.
			// Drawing still lives on the main thread until p4.8 transfers
			// the OffscreenCanvas; this branch only keeps the worker's
			// kernel state in sync.
			if (pane.kernel) {
				try {
					pane.kernel.applyDeltaFrame(request.bytes);
				} catch (err) {
					return {
						type: 'error',
						paneId: request.paneId,
						code: 'apply_delta_failed',
						message: `kernel.applyDeltaFrame failed: ${err instanceof Error ? err.message : String(err)}`,
					};
				}
			}
			// P4.8 (2026-05-22): if a renderer is bound, drive a frame
			// from the kernel's just-updated grid. Errors here do NOT
			// invalidate the kernel state (the delta already landed) —
			// surface as `apply_delta_failed` so the host knows to retry
			// or fall back, but pane state stays intact.
			if (pane.renderer) {
				try {
					pane.renderer.render();
				} catch (err) {
					return {
						type: 'error',
						paneId: request.paneId,
						code: 'apply_delta_failed',
						message: `renderer.render failed: ${err instanceof Error ? err.message : String(err)}`,
					};
				}
			}
			return {
				type: 'ready',
				paneId: request.paneId,
				backend: pane.backend,
			};
		}

		case 'feed': {
			const pane = state.get(request.paneId);
			if (!pane) {
				return {
					type: 'error',
					paneId: request.paneId,
					code: 'pane_not_initialized',
					message: `feed before init for pane ${request.paneId}`,
				};
			}
			// P4.9: wasmKernel.feed(request.data) — but the text path is
			// scheduled for full removal once the Channel path is
			// proven, so this branch may be deleted before it ever
			// gets wired up.
			return {
				type: 'ready',
				paneId: request.paneId,
				backend: pane.backend,
			};
		}

		case 'resize': {
			const pane = state.get(request.paneId);
			if (!pane) {
				return {
					type: 'error',
					paneId: request.paneId,
					code: 'pane_not_initialized',
					message: `resize before init for pane ${request.paneId}`,
				};
			}
			pane.rows = request.rows;
			pane.cols = request.cols;
			pane.dpr = request.dpr;
			// §p4 ITER 7 (2026-05-22) — when the host supplied CSS dims
			// AND a renderer is bound AND it exposes `resize`, drive the
			// wasm RenderHandle resize so its backing buffer + atlas
			// re-quantize against the new DPR / cell box. Failures are
			// non-fatal; the wasm kernel grid mutation above always
			// happens so subsequent applyDelta frames still land.
			if (
				pane.renderer?.resize &&
				typeof request.cssW === 'number' &&
				typeof request.cssH === 'number'
			) {
				try {
					pane.renderer.resize(request.cssW, request.cssH, request.dpr);
				} catch {
					// Swallow — wasm-kernel side already resized.
				}
			}
			return {
				type: 'ready',
				paneId: request.paneId,
				backend: pane.backend,
			};
		}

		case 'setFont': {
			// §p4 ITER 8 (2026-05-22) — propagate a font/size change
			// to the worker-owned RenderHandle. Re-measures cell
			// metrics via `configure` and returns them in `ready`
			// so the host can re-seed entry.cellW / cellH and refit.
			const pane = state.get(request.paneId);
			if (!pane) {
				return {
					type: 'error',
					paneId: request.paneId,
					code: 'pane_not_initialized',
					message: `setFont before init for pane ${request.paneId}`,
				};
			}
			let cellW: number | undefined;
			let cellH: number | undefined;
			if (pane.renderer?.configure) {
				try {
					const [w, h] = pane.renderer.configure(
						request.font,
						request.fontSizePx,
						request.dpr,
					);
					cellW = Number(w);
					cellH = Number(h);
				} catch {
					// Swallow — wasm-side configure failed; host falls
					// back to its current metrics.
				}
			}
			return {
				type: 'ready',
				paneId: request.paneId,
				backend: pane.backend,
				cellW,
				cellH,
			};
		}

		case 'destroy': {
			const pane = state.get(request.paneId);
			if (pane?.renderer) {
				// Free the renderer FIRST — it holds the GPU resources
				// (Canvas2D context / WebGPU swap chain) that need to be
				// released before the kernel they reference goes away.
				try {
					pane.renderer.free();
				} catch {
					/* renderer already freed elsewhere — destroy must be idempotent */
				}
			}
			if (pane?.kernel) {
				// Free the wasm kernel BEFORE dropping the state entry so a
				// `free()` exception doesn't leave the kernel handle
				// dangling on a still-mapped pane.
				try {
					pane.kernel.free();
				} catch {
					/* kernel already freed elsewhere — destroy must be idempotent */
				}
			}
			state.delete(request.paneId);
			return { type: 'destroyed', paneId: request.paneId };
		}
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
		await wasm.default();
		return {
			create({ rows, cols, scrollback }) {
				return new wasm.TerminalKernel(rows, cols, scrollback);
			},
			// §p4.9 (2026-05-22) — worker-side renderer factory. Now
			// that the Rust `RenderHandle::new_from_offscreen` exists
			// (built into the wasm pkg as `newFromOffscreen`), the
			// worker can paint directly onto the transferred canvas
			// without going back to the main thread.
			//
			// The wasm `RenderHandle.render(kernel)` takes the per-pane
			// kernel as an argument (so the rust renderer can read the
			// grid + cursor state). We capture the same kernel the
			// `create` method just minted in a closure so the worker's
			// `pane.renderer.render()` call site stays argument-free.
			// `kernel` here is structurally typed as `KernelHandle` —
			// in production it's always the `wasm.TerminalKernel` we
			// just constructed, so the cast is sound.
			createRenderer({ canvas, kernel }) {
				const handle = wasm.RenderHandle.newFromOffscreen(canvas);
				const wasmKernel = kernel as unknown as InstanceType<typeof wasm.TerminalKernel>;
				return {
					render: () => {
						handle.render(wasmKernel);
					},
					free: () => handle.free(),
					// §p4 ITER 5 (2026-05-22) — surface the wasm-side
					// configure so the worker can measure cell metrics
					// during bindCanvas. The wasm method returns either
					// a `[number, number]` tuple OR a `Float32Array`
					// depending on the wasm-bindgen variant; normalize
					// to a tuple here so callers can destructure
					// uniformly.
					configure: (font, sizePx, dpr) => {
						const raw = handle.configure(font, sizePx, dpr) as
							| [number, number]
							| Float32Array;
						return [Number(raw[0]), Number(raw[1])] as const;
					},
					// §p4 ITER 7 (2026-05-22) — surface wasm-side
					// resize so the worker's resize handler can keep
					// the OffscreenCanvas backing buffer + atlas in
					// sync when the host pane shrinks / grows.
					resize: (wCss, hCss, dpr) => {
						handle.resize(wCss, hCss, dpr);
					},
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

if (isInWorkerScope()) {
	const state = makeWorkerState();
	const scope = self as unknown as WorkerScopeLike;
	// The browser queues incoming `message` events on the worker's
	// internal port until we install a listener. We finish wasm load
	// first so the very first message a pane sends (typically `init`)
	// already has the adapter available — no per-pane race.
	let adapter: KernelAdapter | null = null;

	function installListener(): void {
		scope.addEventListener('message', (event: MessageEvent<unknown>) => {
			// `__reqId` is a host-side correlation id (see WorkerHostedRenderer).
			// It's NOT part of the typed RenderWorkerRequest — it travels alongside
			// the payload as an extra property. The bootstrap reads it from the
			// raw event.data and reflects it on every response so the host can
			// resolve the matching pending promise. If the host didn't include
			// one (e.g. a malformed message), `id` is undefined and responses
			// won't be correlated — same as the legacy direct-postMessage path.
			const id = (event.data as { __reqId?: number } | null)?.__reqId;
			if (!isRenderWorkerRequest(event.data)) {
				const response: RenderWorkerResponse = {
					type: 'error',
					code: 'unknown_message',
					message: `unknown request shape: ${JSON.stringify(event.data)}`,
				};
				scope.postMessage({ ...response, __reqId: id });
				return;
			}
			const response = handleRequest(state, event.data, adapter);
			scope.postMessage({ ...response, __reqId: id });
		});
	}

	// P4.7 + Iter 15 (2026-05-22) — install the listener even if wasm
	// load throws unexpectedly. `loadKernelAdapter` already wraps the
	// imports in try/catch and returns null on failure, but a defensive
	// outer `.catch` ensures any UNCAUGHT failure (e.g. browser refused
	// to honor `import()`, scope cast surprise, etc.) still installs
	// the listener — otherwise host postMessage queues forever and the
	// host's `WorkerHostedRenderer.pending` Map leaks.
	loadKernelAdapter()
		.then((a) => {
			adapter = a;
			installListener();
		})
		.catch((err) => {
			// eslint-disable-next-line no-console
			console.warn(
				'[ridge-term/worker] unexpected loadKernelAdapter rejection — installing listener without an adapter',
				err,
			);
			installListener();
		});
}
