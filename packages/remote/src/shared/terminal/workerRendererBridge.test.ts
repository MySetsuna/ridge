/**
 * P4.6 Part B — Manager-side bridge unit tests.
 *
 * Verifies fire-and-forget semantics, flag gating, and per-method
 * dispatch into the WorkerHostedRenderer. The fake `WorkerLike` lets us
 * drive the real `WorkerHostedRenderer` end-to-end (so the test sees
 * what `__reqId`-tagged messages the bridge actually emits) without
 * standing up a real `Worker`.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { workerRendererBridge, workerLifecycleOnFit } from './workerRendererBridge';
import {
	__setWorkerFactory,
	disposeWorkerRenderer,
} from './workerRendererSingleton';
import type { WorkerLike } from './workerHostedRenderer';

type Captured = { wire: Record<string, unknown>; transfer: Transferable[] };

interface FakeWorker extends WorkerLike {
	posted: Captured[];
	deliver(response: Record<string, unknown>): void;
	terminated: boolean;
}

function makeFakeWorker(): FakeWorker {
	const fake = {
		posted: [] as Captured[],
		terminated: false,
		onmessage: null as WorkerLike['onmessage'],
		postMessage(message: unknown, transfer?: Transferable[]) {
			this.posted.push({
				wire: message as Record<string, unknown>,
				transfer: transfer ?? [],
			});
		},
		terminate() {
			this.terminated = true;
		},
		deliver(response: Record<string, unknown>) {
			if (!this.onmessage) return;
			const event = { data: response } as unknown as MessageEvent;
			this.onmessage.call(this as unknown as WorkerLike, event);
		},
	} satisfies FakeWorker;
	return fake;
}

function setFlag(value: boolean | undefined): void {
	(globalThis as unknown as { __RIDGE_USE_WORKER?: unknown }).__RIDGE_USE_WORKER =
		value;
}

describe('workerRendererBridge', () => {
	let fake: FakeWorker;

	beforeEach(() => {
		disposeWorkerRenderer();
		fake = makeFakeWorker();
		__setWorkerFactory(() => fake);
		setFlag(undefined);
	});

	afterEach(() => {
		disposeWorkerRenderer();
		__setWorkerFactory(null);
		setFlag(undefined);
		vi.restoreAllMocks();
	});

	describe('flag off (default)', () => {
		it('isActive returns false', () => {
			expect(workerRendererBridge.isActive()).toBe(false);
		});

		it('attach/applyDelta/resize/destroy do nothing', () => {
			workerRendererBridge.attach('pane-a', 24, 80, 2);
			workerRendererBridge.applyDelta('pane-a', new Uint8Array([1, 2, 3]));
			workerRendererBridge.resize('pane-a', 40, 100, 2);
			workerRendererBridge.destroy('pane-a');
			expect(fake.posted).toHaveLength(0);
		});

		it('pendingCount returns 0 when bridge is inactive', () => {
			expect(workerRendererBridge.pendingCount()).toBe(0);
		});
	});

	describe('flag on', () => {
		beforeEach(() => {
			setFlag(true);
		});

		it('isActive returns true', () => {
			expect(workerRendererBridge.isActive()).toBe(true);
		});

		it('attach posts a typed init request', () => {
			workerRendererBridge.attach('pane-a', 24, 80, 2);
			expect(fake.posted).toHaveLength(1);
			const w = fake.posted[0].wire;
			expect(w.type).toBe('init');
			expect(w.paneId).toBe('pane-a');
			expect(w.dims).toEqual({ rows: 24, cols: 80, dpr: 2 });
			expect(w.backend).toBe('webgpu');
			expect(w.scrollbackLines).toBe(5000);
			expect(typeof w.__reqId).toBe('number');
		});

		it('attach honors backend/scrollback overrides', () => {
			workerRendererBridge.attach('pane-a', 30, 90, 1.5, {
				backend: 'canvas2d',
				scrollbackLines: 1234,
			});
			const w = fake.posted[0].wire;
			expect(w.backend).toBe('canvas2d');
			expect(w.scrollbackLines).toBe(1234);
		});

		it('applyDelta posts a COPY of the bytes (kernel can still read original)', () => {
			const original = new Uint8Array([10, 20, 30, 40]);
			workerRendererBridge.applyDelta('pane-a', original);
			expect(fake.posted).toHaveLength(1);
			const sent = fake.posted[0].wire.bytes as Uint8Array;
			expect(sent).toBeInstanceOf(Uint8Array);
			expect(Array.from(sent)).toEqual([10, 20, 30, 40]);
			// Critical: the wire bytes must be a DIFFERENT buffer than the
			// kernel's original, otherwise the transferList would detach
			// what the kernel is about to consume.
			expect(sent.buffer).not.toBe(original.buffer);
			expect(fake.posted[0].transfer).toEqual([sent.buffer]);
			// Original is still readable on the main thread.
			expect(Array.from(original)).toEqual([10, 20, 30, 40]);
		});

		it('applyDelta swallows a sync throw from bytes.slice() (Iter 16 contract guard)', () => {
			// Simulate a detached buffer or other pathological state by
			// passing an object whose `.slice()` throws. The bridge MUST
			// NOT propagate the throw to the caller — see the
			// `manager.applyDeltaFrame` → `ptyBridge.onmessage` chain in
			// the bridge module's comment for why this matters.
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			const evil = {
				slice() {
					throw new Error('buffer detached');
				},
			} as unknown as Uint8Array;
			expect(() =>
				workerRendererBridge.applyDelta('pane-a', evil),
			).not.toThrow();
			expect(fake.posted).toHaveLength(0);
			warnSpy.mockRestore();
		});

		it('resize posts a typed resize request', () => {
			workerRendererBridge.resize('pane-b', 50, 120, 2);
			const w = fake.posted[0].wire;
			expect(w).toMatchObject({
				type: 'resize',
				paneId: 'pane-b',
				rows: 50,
				cols: 120,
				dpr: 2,
			});
		});

		it('destroy posts a typed destroy request', () => {
			workerRendererBridge.destroy('pane-c');
			expect(fake.posted[0].wire).toMatchObject({
				type: 'destroy',
				paneId: 'pane-c',
			});
		});

		it('does not throw and does not reject the caller when the worker errors', async () => {
			workerRendererBridge.attach('pane-a', 24, 80, 2);
			expect(fake.posted).toHaveLength(1);
			const id = fake.posted[0].wire.__reqId;
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			// Deliver an error response — the bridge's internal .catch
			// should swallow it; the caller never sees a rejected Promise.
			fake.deliver({
				type: 'error',
				code: 'apply_delta_failed',
				message: 'simulated worker failure',
				__reqId: id,
			});
			await Promise.resolve();
			await Promise.resolve();
			warnSpy.mockRestore();
		});

		it('subsequent calls reuse the same worker (singleton semantics)', () => {
			workerRendererBridge.attach('a', 1, 1, 1);
			workerRendererBridge.attach('b', 1, 1, 1);
			expect(fake.posted).toHaveLength(2);
			expect(fake.terminated).toBe(false);
		});

		it('pendingCount tracks in-flight requests on the singleton', () => {
			expect(workerRendererBridge.pendingCount()).toBe(0);
			workerRendererBridge.attach('pane-1', 24, 80, 2);
			expect(workerRendererBridge.pendingCount()).toBe(1);
			workerRendererBridge.attach('pane-2', 24, 80, 2);
			expect(workerRendererBridge.pendingCount()).toBe(2);
			// Resolve the first by delivering an ack with the matching __reqId.
			const id1 = fake.posted[0].wire.__reqId;
			fake.deliver({ type: 'ready', paneId: 'pane-1', backend: 'webgpu', __reqId: id1 });
			expect(workerRendererBridge.pendingCount()).toBe(1);
			const id2 = fake.posted[1].wire.__reqId;
			fake.deliver({ type: 'ready', paneId: 'pane-2', backend: 'webgpu', __reqId: id2 });
			expect(workerRendererBridge.pendingCount()).toBe(0);
		});
	});
});

// P4.6 Part B / Iter 14 — pure `workerLifecycleOnFit` decision helper.
// Extracted from `TerminalManager.fitPane`'s inline branch so the
// invariant is testable without standing up the manager fixture.
describe('workerLifecycleOnFit', () => {
	const paneId = 'pane-x';
	const rows = 30;
	const cols = 100;
	const dpr = 2;

	it('returns resize when the pane is already in the attached set', () => {
		const action = workerLifecycleOnFit({
			paneId,
			rows,
			cols,
			dpr,
			attached: new Set([paneId]),
			isActive: true, // irrelevant when attached.has — covered below
		});
		expect(action).toEqual({ kind: 'resize', rows, cols, dpr });
	});

	it('attached.has is checked BEFORE isActive (resize wins even if bridge went inactive)', () => {
		// If the flag was flipped OFF mid-session, isActive returns false
		// but the pane is still in the attached set (we don't clean on
		// flag flip). The right behavior is still 'resize' — sending a
		// resize to a dead worker is a no-op at the bridge level. We
		// must NOT downgrade to attach (which would re-add the pane to
		// the set and call init on a now-disposed singleton).
		const action = workerLifecycleOnFit({
			paneId,
			rows,
			cols,
			dpr,
			attached: new Set([paneId]),
			isActive: false,
		});
		expect(action).toEqual({ kind: 'resize', rows, cols, dpr });
	});

	it('returns attach when not attached AND bridge is active', () => {
		const action = workerLifecycleOnFit({
			paneId,
			rows,
			cols,
			dpr,
			attached: new Set(),
			isActive: true,
		});
		expect(action).toEqual({ kind: 'attach', rows, cols, dpr });
	});

	it('returns noop when not attached AND bridge is inactive (flag off, default)', () => {
		const action = workerLifecycleOnFit({
			paneId,
			rows,
			cols,
			dpr,
			attached: new Set(),
			isActive: false,
		});
		expect(action).toEqual({ kind: 'noop' });
	});

	it('attached set isolation: other panes do not trigger this pane\'s resize', () => {
		const action = workerLifecycleOnFit({
			paneId,
			rows,
			cols,
			dpr,
			attached: new Set(['pane-y', 'pane-z']),
			isActive: true,
		});
		expect(action).toEqual({ kind: 'attach', rows, cols, dpr });
	});
});
