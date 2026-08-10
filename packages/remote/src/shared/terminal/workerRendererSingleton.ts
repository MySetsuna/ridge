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
 *   - `isWorkerRenderingEnabled()` — opt-in only; explicit true/1 enables the
 *     legacy worker path, while the main-thread WebGPU host remains default.
 *   - `getWorkerRenderer()` — returns the singleton, lazily creating it on
 *     first call. Returns `null` when the feature flag is off or when the
 *     environment has no `Worker` constructor (SSR / vitest node env).
 *   - `disposeWorkerRenderer()` — tears down the singleton. Idempotent;
 *     safe to call from HMR teardown or test cleanup.
 *
 * The factory itself is small enough to keep all production logic here
 * without introducing yet another abstraction.
 *
 * Integration status (see project memo):
 *   1. `TerminalManager` reads `isWorkerRenderingEnabled()` on pane attach.
 *   2. When enabled, the manager calls `getWorkerRenderer()` and proxies
 *      `applyDeltaFrame()`/`resize()` through it instead of running the
 *      kernel on the main thread.
 *   3. `RidgePane.svelte` calls `canvas.transferControlToOffscreen()` and
 *      ships the OffscreenCanvas to the renderer via `bindCanvas`.
 *   4. The main-thread WebGPU path stays the default while worker rendering
 *      remains an explicit compatibility opt-in.
 */

import { WorkerHostedRenderer, type WorkerLike } from './workerHostedRenderer';
import { unknownText } from '../transport/unknownText';

/** Read the opt-in flag in a type-safe way. Checks (in order):
 *    1. `globalThis.__RIDGE_USE_WORKER === true` — easiest at the JS console.
 *    2. `localStorage.RIDGE_USE_WORKER === '1' | 'true'` — survives reloads,
 *       and (most importantly) is settable BEFORE app boot by an e2e harness.
 *  Returns false otherwise. localStorage access is wrapped in try/catch
 *  because workers and SSR may not expose it. */
export function isWorkerRenderingEnabled(): boolean {
	const g = globalThis as unknown as { __RIDGE_USE_WORKER?: unknown };
	if (typeof g.__RIDGE_USE_WORKER === 'boolean') return g.__RIDGE_USE_WORKER;
	try {
		const v = globalThis.localStorage?.getItem('RIDGE_USE_WORKER');
		if (v === '1' || v === 'true') return true;
		if (v === '0' || v === 'false') return false;
	} catch {
		/* SSR / worker / sandboxed origin — localStorage unavailable */
	}
	// Worker OffscreenCanvas path currently calls `newFromOffscreen`, which is
	// Canvas2D-only. Keep it opt-in so desktop panes use shared WebGPU host.
	return false;
}

/** Returns true when the runtime exposes a real `Worker` constructor.
 *  Vitest's default `node` environment doesn't, and SSR doesn't either. */
function hasWorkerSupport(): boolean {
	return typeof Worker !== 'undefined';
}

let singleton: WorkerHostedRenderer | null = null;
let disabledAfterFailure = false;
const failureListeners = new Set<(error: Error) => void>();

function notifyFailure(error: Error): void {
	disabledAfterFailure = true;
	const failed = singleton;
	singleton = null;
	failed?.terminate();
	for (const listener of failureListeners) listener(error);
}

export function onWorkerRendererFailure(listener: (error: Error) => void): () => void {
	failureListeners.add(listener);
	return () => failureListeners.delete(listener);
}

export function failWorkerRenderer(error: unknown): void {
	notifyFailure(error instanceof Error ? error : new Error(unknownText(error)));
}

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
	if (disabledAfterFailure) return null;
	if (!isWorkerRenderingEnabled()) return null;
	if (factoryOverride) {
		try {
			singleton = new WorkerHostedRenderer(factoryOverride(), notifyFailure);
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
		singleton = new WorkerHostedRenderer(worker as unknown as WorkerLike, notifyFailure);
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
	if (singleton) {
		try {
			singleton.terminate();
		} catch {
			/* worker already dead */
		}
		singleton = null;
	}
	disabledAfterFailure = false;
}
