/**
 * P4.6 — WorkerHostedRenderer unit tests.
 *
 * Drives the host-side wrapper with a fake `WorkerLike` that captures
 * outgoing postMessage calls and lets the test deliver responses
 * inline. The wrapper attaches a private `__reqId` to each outgoing
 * payload; the fake reflects it back on the simulated response so the
 * wrapper resolves the right Promise.
 *
 * Together with `renderWorker.test.ts` (P4.5), these tests cover both
 * sides of the worker boundary without needing a real `Worker` runtime.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
	WorkerHostedRenderer,
	type WorkerLike,
} from './workerHostedRenderer';
import {
	handleRequest,
	makeWorkerState,
	type WorkerState,
} from './renderWorker';
import type { RenderWorkerRequest } from './renderWorker.protocol';

const PANE = '00000000-0000-0000-0000-0000000000aa';

interface CapturedMessage {
	message: RenderWorkerRequest & { __reqId: number };
	transfer: Transferable[];
}

class FakeWorker implements WorkerLike {
	onmessage: ((this: WorkerLike, ev: MessageEvent) => void) | null = null;
	terminated = false;
	calls: CapturedMessage[] = [];
	postMessage(message: unknown, transfer?: Transferable[]): void {
		this.calls.push({
			message: message as RenderWorkerRequest & { __reqId: number },
			transfer: transfer ?? [],
		});
	}
	terminate(): void {
		this.terminated = true;
	}

	/** Deliver a response to the host, reflecting the __reqId off the
	 *  Nth outbound message. Tests use this to simulate the worker's
	 *  bootstrap echo. */
	deliverResponseFor(index: number, body: object): void {
		const reqId = this.calls[index].message.__reqId;
		this.onmessage?.call(
			this,
			new MessageEvent('message', { data: { ...body, __reqId: reqId } }),
		);
	}

	/** Deliver an arbitrary message (used for unsolicited / malformed
	 *  message tests where there is no matching outbound request). */
	deliverRaw(data: unknown): void {
		this.onmessage?.call(this, new MessageEvent('message', { data }));
	}
}

/**
 * Build a FakeWorker that's pre-wired with the actual worker dispatcher.
 * Useful for end-to-end tests that verify both halves agree on the
 * protocol without spinning up an actual Worker.
 */
function dispatchedFake(): { worker: FakeWorker; state: WorkerState } {
	const worker = new FakeWorker();
	const state = makeWorkerState();
	worker.postMessage = (message: unknown) => {
		worker.calls.push({
			message: message as RenderWorkerRequest & { __reqId: number },
			transfer: [],
		});
		const m = message as RenderWorkerRequest & { __reqId?: number };
		const id = m.__reqId;
		// Run on the next microtask so the host's pending map is set
		// before we deliver — mirrors real Worker timing.
		queueMicrotask(() => {
			const response = handleRequest(state, m);
			worker.onmessage?.call(
				worker,
				new MessageEvent('message', {
					data: { ...response, __reqId: id },
				}),
			);
		});
	};
	return { worker, state };
}

describe('WorkerHostedRenderer — basic dispatch', () => {
	afterEach(() => {
		vi.restoreAllMocks();
	});

	it('init resolves with the worker ack and includes __reqId on the wire', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);

		const promise = renderer.init({
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'webgpu',
			scrollbackLines: 2000,
		});
		// Synchronously after the call: the wire message must already be
		// in calls[0], carrying a numeric __reqId.
		expect(worker.calls).toHaveLength(1);
		expect(worker.calls[0].message.type).toBe('init');
		expect(typeof worker.calls[0].message.__reqId).toBe('number');

		// Now deliver the response.
		worker.deliverResponseFor(0, {
			type: 'ready',
			paneId: PANE,
			backend: 'webgpu',
		});

		const response = await promise;
		expect(response).toMatchObject({
			type: 'ready',
			paneId: PANE,
			backend: 'webgpu',
		});
		expect(renderer.pendingCount()).toBe(0);
	});

	it('ping returns the pong with the same token', async () => {
		const { worker } = dispatchedFake();
		const renderer = new WorkerHostedRenderer(worker);
		const response = await renderer.ping('hello');
		expect(response).toMatchObject({ type: 'pong', token: 'hello' });
	});

	it('uses zero-copy transfer for applyDelta bytes', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);
		const bytes = new Uint8Array([1, 2, 3, 4]);

		const promise = renderer.applyDelta(PANE, bytes);
		expect(worker.calls[0].transfer).toEqual([bytes.buffer]);
		expect(worker.calls[0].message.type).toBe('applyDelta');

		worker.deliverResponseFor(0, {
			type: 'ready',
			paneId: PANE,
			backend: 'webgpu',
		});
		await promise;
	});
});

describe('WorkerHostedRenderer — concurrency', () => {
	it('keeps two concurrent requests from crossing wires', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);

		const pA = renderer.ping('A');
		const pB = renderer.ping('B');

		expect(worker.calls.length).toBe(2);
		expect(renderer.pendingCount()).toBe(2);

		// Deliver out of order — B first, then A. Each must resolve to its
		// own token; the __reqId reflection keeps them paired correctly.
		worker.deliverResponseFor(1, { type: 'pong', token: 'B' });
		worker.deliverResponseFor(0, { type: 'pong', token: 'A' });

		const [respA, respB] = await Promise.all([pA, pB]);
		expect(respA).toMatchObject({ token: 'A' });
		expect(respB).toMatchObject({ token: 'B' });
		expect(renderer.pendingCount()).toBe(0);
	});

	it('end-to-end against the real dispatcher: init → applyDelta → destroy', async () => {
		const { worker, state } = dispatchedFake();
		const renderer = new WorkerHostedRenderer(worker);

		await renderer.init({
			paneId: PANE,
			dims: { rows: 24, cols: 80, dpr: 1 },
			backend: 'webgpu',
			scrollbackLines: 1000,
		});
		expect(state.get(PANE)).toBeTruthy();

		const ack = await renderer.applyDelta(PANE, new Uint8Array([9, 9, 9]));
		expect(ack.type).toBe('ready');

		await renderer.destroy(PANE);
		expect(state.get(PANE)).toBeUndefined();
	});
});

describe('WorkerHostedRenderer — error paths', () => {
	let warnSpy: ReturnType<typeof vi.spyOn>;
	beforeEach(() => {
		warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
	});
	afterEach(() => {
		warnSpy.mockRestore();
	});

	it('rejects with WorkerRendererError when the worker sends an error response', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);

		const promise = renderer.applyDelta(PANE, new Uint8Array([1]));
		worker.deliverResponseFor(0, {
			type: 'error',
			paneId: PANE,
			code: 'pane_not_initialized',
			message: 'applyDelta before init',
		});

		await expect(promise).rejects.toMatchObject({
			name: 'WorkerRendererError',
			code: 'pane_not_initialized',
			paneId: PANE,
		});
	});

	it('drops unsolicited messages with no __reqId (and warns)', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);
		// No outbound call → no pending. Deliver a stray message.
		worker.deliverRaw({ type: 'pong' });
		expect(warnSpy).toHaveBeenCalled();
		expect(renderer.pendingCount()).toBe(0);
	});

	it('drops messages whose __reqId does not match any pending request', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);
		worker.deliverRaw({ type: 'pong', __reqId: 999 });
		expect(warnSpy).toHaveBeenCalled();
	});

	it('terminate rejects all pending requests', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);
		const p = renderer.applyDelta(PANE, new Uint8Array([0]));
		renderer.terminate();
		await expect(p).rejects.toMatchObject({ name: 'WorkerRendererError' });
		expect(worker.terminated).toBe(true);
	});

	it('rejects new requests after terminate without touching the worker', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);
		renderer.terminate();
		const callsBefore = worker.calls.length;
		await expect(renderer.ping()).rejects.toMatchObject({
			name: 'WorkerRendererError',
		});
		expect(worker.calls.length).toBe(callsBefore);
	});

	it('terminate is idempotent', async () => {
		const worker = new FakeWorker();
		const renderer = new WorkerHostedRenderer(worker);
		renderer.terminate();
		renderer.terminate();
		expect(worker.terminated).toBe(true);
	});
});
