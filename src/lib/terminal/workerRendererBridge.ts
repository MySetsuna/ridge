/**
 * P4.6 Part B (2026-05-22) — Manager-side bridge to the render worker.
 *
 * Glues `TerminalManager` to the lazily-created `WorkerHostedRenderer`
 * singleton. While the feature flag (`window.__RIDGE_USE_WORKER`) stays
 * off (the default), every method here is a cheap no-op. When the flag
 * is on, the manager mirrors the pane lifecycle into the worker:
 *
 *   attach(paneId, rows, cols, dpr)   →  singleton.init(...)
 *   applyDelta(paneId, bytes)         →  singleton.applyDelta(...)
 *   resize(paneId, rows, cols, dpr)   →  singleton.resize(...)
 *   destroy(paneId)                   →  singleton.destroy(...)
 *
 * In Iter 7 the render worker is still a stub (it acks every request
 * without doing real rendering — see `renderWorker.ts`). So the mirror
 * is "shadow mode": the main-thread wasm kernel keeps painting, and the
 * worker just maintains its per-pane bookkeeping. P4.9 will move the
 * wasm kernel into the worker and switch the visible path over.
 *
 * Why shadow + slice() for the delta bytes
 * ----------------------------------------
 * `WorkerHostedRenderer.applyDelta` transfers `bytes.buffer` zero-copy.
 * The main-thread `TerminalManager.applyDeltaFrame` ALSO needs to feed
 * those bytes into the local wasm kernel. We can't transfer the same
 * buffer twice. Until the worker becomes authoritative, the bridge
 * sends a `.slice()` copy — the kernel keeps the original. The extra
 * allocation only happens when the flag is on, so the legacy default
 * path pays nothing.
 *
 * Why fire-and-forget
 * -------------------
 * The manager's hot paths (`applyDeltaFrame`, `fitPane`) cannot block
 * on a worker round-trip without ruining the per-frame budget. Each
 * call attaches a `.catch` so an `unhandledrejection` never leaks into
 * the console, but the caller does not await. Errors are surfaced as
 * dev-only `console.warn`.
 */

import { getWorkerRenderer } from './workerRendererSingleton';
import type { WorkerHostedRenderer } from './workerHostedRenderer';
import type { RendererBackend } from './renderWorker.protocol';

const DEFAULT_BACKEND: RendererBackend = 'webgpu';
const DEFAULT_SCROLLBACK = 5000;

function devMode(): boolean {
	try {
		return Boolean(import.meta.env?.DEV);
	} catch {
		return false;
	}
}

function warn(label: string, err: unknown): void {
	if (!devMode()) return;
	// eslint-disable-next-line no-console
	console.warn(`[ridge-term/worker-bridge] ${label}`, err);
}

/** Resolve the active worker renderer, swallowing factory errors. Tests
 *  can stub the singleton via `__setWorkerFactory` in
 *  `workerRendererSingleton.ts`. */
function active(): WorkerHostedRenderer | null {
	try {
		return getWorkerRenderer();
	} catch (err) {
		warn('getWorkerRenderer threw', err);
		return null;
	}
}

export interface WorkerRendererBridge {
	/** Test introspection — true iff the singleton is currently live. */
	isActive(): boolean;
	/** Number of host-to-worker requests still awaiting a response.
	 *  Returns 0 when the singleton isn't live (flag off, factory threw,
	 *  no Worker support). Wired through to `WorkerHostedRenderer.pendingCount`.
	 *  Useful for e2e specs verifying that the worker is keeping up. */
	pendingCount(): number;
	/** Mirror a new pane into the worker. Idempotent at the worker: a
	 *  duplicate attach for the same paneId becomes a
	 *  `pane_already_initialized` error response, which the bridge
	 *  swallows via its dev-only warn. */
	attach(
		paneId: string,
		rows: number,
		cols: number,
		dpr: number,
		opts?: { backend?: RendererBackend; scrollbackLines?: number },
	): void;
	/** Mirror a postcard delta frame. Sends a `.slice()` copy so the
	 *  main-thread kernel keeps the original bytes intact (see the
	 *  module-level note). */
	applyDelta(paneId: string, bytes: Uint8Array): void;
	/** Mirror a resize.
	 *  §p4 ITER 7 (2026-05-22) — `cssW` / `cssH` (CSS pixels) optional;
	 *  when supplied the worker also resizes its `RenderHandle` backing
	 *  buffer. Omit for kernel-grid-only resizes. */
	resize(
		paneId: string,
		rows: number,
		cols: number,
		dpr: number,
		cssW?: number,
		cssH?: number,
	): void;
	/** Mirror a font/size change.
	 *
	 *  §p4 ITER 8 (2026-05-22) — fires the worker `setFont` request
	 *  and, on a successful `ready` response with cell metrics,
	 *  invokes `onMetrics(cellW, cellH)` so the manager can re-seed
	 *  entry.cellW / cellH and trigger a fit. No-op when the worker
	 *  is inactive; never throws. */
	setFont(
		paneId: string,
		font: string,
		fontSizePx: number,
		dpr: number,
		onMetrics?: (cellW: number, cellH: number) => void,
	): void;

	/** Mirror a pane teardown. No-op if the pane was never attached. */
	destroy(paneId: string): void;
}

/**
 * Pure decision function for `TerminalManager::fitPane`'s worker
 * lifecycle hook. Extracted (2026-05-22 Iter 14) so the invariant
 *
 *     pane already mirrored → resize
 *     not yet mirrored AND bridge live → attach (and the caller
 *                                                 should register
 *                                                 the paneId)
 *     not yet mirrored AND bridge dead → noop
 *
 * is independently unit-testable without standing up a wasm-laden
 * TerminalManager fixture. The caller (manager.ts) consumes the
 * tagged-union result and (a) calls the matching bridge method and
 * (b) updates its private `workerAttached: Set<string>` on `attach`.
 *
 * Keeping this here (next to the bridge itself) rather than in the
 * manager keeps the worker plumbing in one place.
 */
export type WorkerLifecycleAction =
	| { kind: 'attach'; rows: number; cols: number; dpr: number }
	| { kind: 'resize'; rows: number; cols: number; dpr: number }
	| { kind: 'noop' };

export function workerLifecycleOnFit(args: {
	paneId: string;
	rows: number;
	cols: number;
	dpr: number;
	attached: ReadonlySet<string>;
	isActive: boolean;
}): WorkerLifecycleAction {
	const { paneId, rows, cols, dpr, attached, isActive } = args;
	if (attached.has(paneId)) {
		return { kind: 'resize', rows, cols, dpr };
	}
	if (isActive) {
		return { kind: 'attach', rows, cols, dpr };
	}
	return { kind: 'noop' };
}

export const workerRendererBridge: WorkerRendererBridge = {
	isActive(): boolean {
		return active() !== null;
	},

	pendingCount(): number {
		const r = active();
		return r ? r.pendingCount() : 0;
	},

	attach(paneId, rows, cols, dpr, opts): void {
		const r = active();
		if (!r) return;
		r.init({
			paneId,
			dims: { rows, cols, dpr },
			backend: opts?.backend ?? DEFAULT_BACKEND,
			scrollbackLines: opts?.scrollbackLines ?? DEFAULT_SCROLLBACK,
		}).catch((err) => warn(`init ${paneId}`, err));
	},

	applyDelta(paneId, bytes): void {
		const r = active();
		if (!r) return;
		// Iter 16 (2026-05-22) — guard the entire body. `bytes.slice()`
		// is synchronous; in the rare event the buffer is in a weird
		// state (e.g. already-detached from a previous transfer because
		// of a caller bug), it would throw to the caller. The bridge's
		// contract is fire-and-forget never-throw — `manager.applyDeltaFrame`
		// has no try/catch around our call, so a sync throw would bubble
		// up to `ptyBridge.onmessage` and incorrectly trip the R5
		// self-heal `set_pane_delta_mode(false)` invoke.
		try {
			// .slice() so the kernel can still consume the original bytes.
			// Cheap because the buffer is typically ≤ tens of KB per frame.
			r.applyDelta(paneId, bytes.slice()).catch((err) =>
				warn(`applyDelta ${paneId}`, err),
			);
		} catch (err) {
			warn(`applyDelta ${paneId} (sync)`, err);
		}
	},

	resize(paneId, rows, cols, dpr, cssW, cssH): void {
		const r = active();
		if (!r) return;
		r.resize(paneId, rows, cols, dpr, cssW, cssH).catch((err) =>
			warn(`resize ${paneId}`, err),
		);
	},

	setFont(paneId, font, fontSizePx, dpr, onMetrics): void {
		const r = active();
		if (!r) return;
		r.setFont(paneId, font, fontSizePx, dpr)
			.then((response) => {
				if (
					onMetrics &&
					response.type === 'ready' &&
					typeof response.cellW === 'number' &&
					typeof response.cellH === 'number' &&
					response.cellW > 0 &&
					response.cellH > 0
				) {
					try {
						onMetrics(response.cellW, response.cellH);
					} catch (err) {
						warn(`setFont onMetrics ${paneId}`, err);
					}
				}
			})
			.catch((err) => warn(`setFont ${paneId}`, err));
	},

	destroy(paneId): void {
		const r = active();
		if (!r) return;
		r.destroy(paneId).catch((err) => warn(`destroy ${paneId}`, err));
	},
};
