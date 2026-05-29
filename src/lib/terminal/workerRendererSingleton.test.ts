/**
 * P4.6 Part B — Worker singleton lifecycle.
 *
 * Tests use `__setWorkerFactory` to inject a `WorkerLike` stub so we
 * never need a real `Worker` constructor in the vitest node env.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	__setWorkerFactory,
	disposeWorkerRenderer,
	getWorkerRenderer,
	isWorkerRenderingEnabled,
} from './workerRendererSingleton';
import type { WorkerLike } from './workerHostedRenderer';

function makeFakeWorker(): WorkerLike & { terminated: boolean } {
	const fake: WorkerLike & { terminated: boolean } = {
		postMessage() {},
		onmessage: null,
		terminate() {
			fake.terminated = true;
		},
		terminated: false,
	};
	return fake;
}

function setFlag(value: boolean | undefined): void {
	(globalThis as unknown as { __RIDGE_USE_WORKER?: unknown }).__RIDGE_USE_WORKER =
		value;
}

describe('workerRendererSingleton', () => {
	beforeEach(() => {
		disposeWorkerRenderer();
		__setWorkerFactory(null);
		setFlag(undefined);
	});

	afterEach(() => {
		disposeWorkerRenderer();
		__setWorkerFactory(null);
		setFlag(undefined);
		vi.restoreAllMocks();
	});

	it('isWorkerRenderingEnabled defaults to true (P4.9)', () => {
		expect(isWorkerRenderingEnabled()).toBe(true);
	});

	it('reads the global flag', () => {
		setFlag(true);
		expect(isWorkerRenderingEnabled()).toBe(true);
		setFlag(false);
		expect(isWorkerRenderingEnabled()).toBe(false);
	});

	it('reads the localStorage fallback when global is unset', () => {
		// Build a stub localStorage that vitest's node env does NOT
		// provide. Assign it directly to globalThis so the helper's
		// `globalThis.localStorage?.getItem(...)` path executes.
		const store = new Map<string, string>();
		const ls = {
			getItem: (k: string) => store.get(k) ?? null,
			setItem: (k: string, v: string) => {
				store.set(k, v);
			},
			removeItem: (k: string) => {
				store.delete(k);
			},
			clear: () => store.clear(),
			get length() {
				return store.size;
			},
			key: () => null,
		};
		(globalThis as unknown as { localStorage: typeof ls }).localStorage = ls;
		try {
			expect(isWorkerRenderingEnabled()).toBe(true);
			ls.setItem('RIDGE_USE_WORKER', '1');
			expect(isWorkerRenderingEnabled()).toBe(true);
			ls.setItem('RIDGE_USE_WORKER', 'true');
			expect(isWorkerRenderingEnabled()).toBe(true);
			ls.setItem('RIDGE_USE_WORKER', '0');
			expect(isWorkerRenderingEnabled()).toBe(false);
			ls.removeItem('RIDGE_USE_WORKER');
			expect(isWorkerRenderingEnabled()).toBe(true);
		} finally {
			delete (globalThis as unknown as { localStorage?: unknown }).localStorage;
		}
	});

	it('global flag wins over a contradictory localStorage value', () => {
		const store = new Map<string, string>([['RIDGE_USE_WORKER', '0']]);
		(globalThis as unknown as {
			localStorage: { getItem(k: string): string | null };
		}).localStorage = {
			getItem: (k: string) => store.get(k) ?? null,
		};
		try {
			setFlag(true);
			expect(isWorkerRenderingEnabled()).toBe(true);
		} finally {
			delete (globalThis as unknown as { localStorage?: unknown }).localStorage;
		}
	});

	it('getWorkerRenderer returns null when the flag is off (opt-out)', () => {
		setFlag(false);
		__setWorkerFactory(() => makeFakeWorker());
		expect(getWorkerRenderer()).toBeNull();
	});

	it('getWorkerRenderer returns a singleton when the flag is on', () => {
		setFlag(true);
		const created: WorkerLike[] = [];
		__setWorkerFactory(() => {
			const w = makeFakeWorker();
			created.push(w);
			return w;
		});

		const a = getWorkerRenderer();
		const b = getWorkerRenderer();
		expect(a).not.toBeNull();
		expect(a).toBe(b);
		// Factory must have been called exactly once.
		expect(created.length).toBe(1);
	});

	it('disposeWorkerRenderer terminates the worker and allows recreation', () => {
		setFlag(true);
		const created: Array<WorkerLike & { terminated: boolean }> = [];
		__setWorkerFactory(() => {
			const w = makeFakeWorker();
			created.push(w);
			return w;
		});

		const first = getWorkerRenderer();
		expect(first).not.toBeNull();
		disposeWorkerRenderer();
		expect(created[0].terminated).toBe(true);

		// After dispose, getWorkerRenderer should lazily spawn a new one.
		const second = getWorkerRenderer();
		expect(second).not.toBeNull();
		expect(second).not.toBe(first);
		expect(created.length).toBe(2);
	});

	it('disposeWorkerRenderer is idempotent', () => {
		setFlag(true);
		__setWorkerFactory(() => makeFakeWorker());
		getWorkerRenderer();
		disposeWorkerRenderer();
		expect(() => disposeWorkerRenderer()).not.toThrow();
	});

	it('returns null when factory throws', () => {
		setFlag(true);
		const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
		__setWorkerFactory(() => {
			throw new Error('worker spawn denied');
		});
		expect(getWorkerRenderer()).toBeNull();
		expect(warnSpy).toHaveBeenCalled();
	});

	it('returns null when no Worker constructor exists and no factory override is set', () => {
		// The default vitest 'node' env has no `Worker` global, so this
		// exercises the production failure path naturally.
		setFlag(true);
		expect(typeof Worker).toBe('undefined');
		expect(getWorkerRenderer()).toBeNull();
	});
});
