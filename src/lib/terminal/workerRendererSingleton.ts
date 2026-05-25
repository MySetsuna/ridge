/**
 * P4.6 Part B (2026-05-22) — production singleton for the render worker.
 *
 * `WorkerHostedRenderer` (P4.6 Part A) wraps a `WorkerLike`. For production
 * we want exactly ONE worker shared across all panes — the protocol from
 * P4.5 is already per-pane addressable, so spinning one worker per pane
 * would just waste memory (each worker re-loads the wasm module).
 *
 * This module exposes:
 *
 *   - `isWorkerRenderingEnabled()` — reads the `window.__RIDGE_USE_WORKER`
 *     feature flag. Default off; opt-in by setting the global to `true`
 *     before any pane mounts.
 *   - `getWorkerRenderer()` — returns the singleton, lazily creating it on
 *     first call. Returns `null` when the feature flag is off or when the
 *     environment has no `Worker` constructor (SSR / vitest node env).
 *   - `disposeWorkerRenderer()` — tears down the singleton. Idempotent;
 *     safe to call from HMR teardown or test cleanup.
 *
 * The factory itself is small enough to keep all production logic here
 * without introducing yet another abstraction.
 *
 * Integration status (2026-05-24) — ALL four steps implemented:
 *   1. ✅ `TerminalManager` reads `isWorkerRenderingEnabled()` on pane attach.
 *   2. ✅ When enabled, manager proxies through `workerRendererBridge`.
 *   3. ✅ `manager.attach` calls `canvas.transferControlToOffscreen()`
 *      and ships the OffscreenCanvas to the worker via `bindCanvas`.
 *   4. ✅ Legacy main-thread path stays as fallback when flag is off
 *      or worker fails to spin up.
 */

import { WorkerHostedRenderer, type WorkerLike } from './workerHostedRenderer';

/** Read the opt-in flag in a type-safe way. Checks (in order):
 *    1. `globalThis.__RIDGE_USE_WORKER === true` — explicit opt-in.
 *    2. `globalThis.__RIDGE_USE_WORKER === false` — explicit opt-out.
 *    3. `localStorage.RIDGE_USE_WORKER === '0'` — persistent opt-out.
 *  Returns true otherwise (P4.9: enabled by default). localStorage access
 *  is wrapped in try/catch because workers and SSR may not expose it. */
export function isWorkerRenderingEnabled(): boolean {
	const g = globalThis as unknown as { __RIDGE_USE_WORKER?: unknown };
	if (g.__RIDGE_USE_WORKER === true) return true;
	if (g.__RIDGE_USE_WORKER === false) return false;
	try {
		const v = globalThis.localStorage?.getItem('RIDGE_USE_WORKER');
		if (v === '0') return false;
	} catch {
		/* SSR / worker / sandboxed origin — localStorage unavailable */
	}
	// P4.9: enabled by default. Users who hit performance issues before
	// the worker path was stable can still opt out via the global or
	// localStorage.
	return true;
}

/** Returns true when the runtime exposes a real `Worker` constructor.
 *  Vitest's default `node` environment doesn't, and SSR doesn't either. */
function hasWorkerSupport(): boolean {
	return typeof Worker !== 'undefined';
}

let singleton: WorkerHostedRenderer | null = null;

/**
 * Optional injection seam used by tests. Production callers leave this
 * unset; tests call `__setWorkerFactory(() => fakeWorkerLike)` before
 * `getWorkerRenderer()` to bypass the real `new Worker(...)` and feed
 * a `WorkerLike` stub instead.
 */
type WorkerFactory = () => WorkerLike;
let factoryOverride: WorkerFactory | null = null;

export function __setWorkerFactory(factory: WorkerFactory | null): void {
	factoryOverride = factory;
}

/**
 * Lazily create (or return) the singleton renderer. Returns `null` when
 * the feature flag is off, when no Worker support exists, or when an
 * earlier creation threw.
 */
export function getWorkerRenderer(): WorkerHostedRenderer | null {
	if (singleton) return singleton;
	if (!isWorkerRenderingEnabled()) return null;
	if (factoryOverride) {
		try {
			singleton = new WorkerHostedRenderer(factoryOverride());
			return singleton;
		} catch (err) {
			console.warn('[ridge-term] worker renderer factory threw', err);
			return null;
		}
	}
	if (!hasWorkerSupport()) return null;
	try {
		const worker = new Worker(new URL('./renderWorker.ts', import.meta.url), {
			type: 'module',
		});
		// Native `Worker.onmessage` has a slightly stricter `this: Worker`
		// signature than our minimal `WorkerLike`. The wrapper only ever
		// assigns an arrow-function listener (no `this` reliance), so the
		// cast is safe — and unavoidable without pulling the full DOM
		// `Worker` type into the wrapper's interface.
		singleton = new WorkerHostedRenderer(worker as unknown as WorkerLike);
		return singleton;
	} catch (err) {
		// Bundler couldn't resolve the URL pattern (e.g. environment lacks
		// import.meta.url support), or the OS denied worker spawn. Fall
		// back to the legacy main-thread path by returning null; the
		// caller treats this the same as the feature flag being off.
		console.warn('[ridge-term] failed to spawn render worker', err);
		return null;
	}
}

/**
 * Tear down the singleton. After this call, `getWorkerRenderer()` will
 * lazily spawn a new one (assuming the flag still says yes). Idempotent.
 */
export function disposeWorkerRenderer(): void {
	if (!singleton) return;
	try {
		singleton.terminate();
	} catch {
		/* worker already dead */
	}
	singleton = null;
}
