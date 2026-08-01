/**
 * P4.6 (2026-05-21) — Host-side wrapper around the render worker.
 *
 * This class is the main thread's view of the worker built in P4.5. It
 * owns one `Worker` instance and turns the postMessage protocol into a
 * normal-looking async API:
 *
 *   const renderer = new WorkerHostedRenderer(workerLike);
 *   await renderer.init({ paneId, dims, backend, scrollbackLines });
 *   await renderer.applyDelta(paneId, bytes);  // resolves on worker ack
 *   await renderer.resize(paneId, rows, cols, dpr);
 *   await renderer.destroy(paneId);
 *
 * Each call attaches a private `__reqId` to the outbound message and
 * waits for the matching response. The worker's bootstrap reflects the
 * id back, so the host can resolve the right Promise even when many
 * requests are in flight concurrently across multiple panes.
 *
 * Why a private id (and not `id` in the protocol types)
 * -----------------------------------------------------
 * The `RenderWorkerRequest` / `RenderWorkerResponse` types are kept
 * deliberately ID-less so the pure `handleRequest()` test fixture
 * doesn't have to thread ids through every assertion. The id only
 * exists on the wire between host and bootstrap.
 *
 * Why accept a `WorkerLike` instead of a real `Worker`
 * ----------------------------------------------------
 * In tests we pass a fake whose `postMessage` records the call and
 * exposes a `__deliver` helper. In production we pass a real `Worker`
 * spawned with `new Worker(new URL('./renderWorker.ts', import.meta.url),
 * { type: 'module' })`. The wrapper doesn't care which.
 */

import type {
	PaneInitDims,
	RendererBackend,
	RenderWorkerRequest,
	RenderWorkerResponse,
} from './renderWorker.protocol';

/**
 * The slice of `Worker` the wrapper actually needs. Tests pass a
 * fake; production passes a real `Worker`. Keeping the surface tiny
 * lets the fake stay tiny too.
 */
export interface WorkerLike {
	postMessage(message: unknown, transfer?: Transferable[]): void;
	onmessage: ((this: WorkerLike, ev: MessageEvent) => void) | null;
	onerror?: ((event: ErrorEvent) => unknown) | null;
	terminate(): void;
}

/** Internal — pending request entry. */
interface Pending {
	resolve: (response: RenderWorkerResponse) => void;
	reject: (err: Error) => void;
	timer: ReturnType<typeof setTimeout>;
	paneId?: string;
}

export const MAX_WORKER_PENDING = 256;
export const WORKER_REQUEST_TIMEOUT_MS = 5000;

/**
 * Errors thrown by the wrapper when the worker sends back an `error`
 * response or when the wrapper itself fails locally (e.g. terminate
 * was called with pending requests still in flight).
 */
export class WorkerRendererError extends Error {
	readonly code: RenderWorkerResponse extends infer R
		? R extends { type: 'error'; code: infer C }
			? C
			: never
		: never;
	readonly paneId?: string;
	constructor(
		message: string,
		code: WorkerRendererError['code'],
		paneId?: string,
	) {
		super(message);
		this.name = 'WorkerRendererError';
		this.code = code;
		this.paneId = paneId;
	}
}

/**
 * Wraps a `WorkerLike` and exposes typed async methods. Internal request
 * tracking via a monotonic `__reqId` counter; the bootstrap reflects the
 * id on every response, so concurrent calls don't cross wires.
 */
export class WorkerHostedRenderer {
	private worker: WorkerLike;
	private pending = new Map<number, Pending>();
	private ignoredReqIds = new Set<number>();
	private nextReqId = 1;
	private terminated = false;
	private onFatal?: (error: Error) => void;

	constructor(worker: WorkerLike, onFatal?: (error: Error) => void) {
		this.worker = worker;
		this.onFatal = onFatal;
		this.worker.onmessage = (event: MessageEvent) => this.onMessage(event);
		this.worker.onerror = (event: ErrorEvent) => {
			this.fail(
				new WorkerRendererError(
					event.message || 'render worker crashed',
					'apply_delta_failed',
				),
			);
		};
	}

	/**
	 * Tear down the worker. Pending Promises reject with a
	 * `WorkerRendererError` so callers don't hang forever after a
	 * `destroy()` race. Safe to call twice.
	 */
	terminate(): void {
		if (this.terminated) return;
		this.terminated = true;
		const pending = Array.from(this.pending.values());
		this.pending.clear();
		this.ignoredReqIds.clear();
		this.worker.onmessage = null;
		this.worker.onerror = null;
		try {
			this.worker.terminate();
		} catch {
			/* Worker may already be dead — ignore. */
		}
		for (const p of pending) {
			clearTimeout(p.timer);
			p.reject(
				new WorkerRendererError(
					'render worker terminated with pending requests',
					'apply_delta_failed',
				),
			);
		}
	}

	// --- protocol-typed methods --------------------------------------------

	/** Initialize a pane in the worker. Resolves on the `ready` ack. */
	init(args: {
		paneId: string;
		dims: PaneInitDims;
		backend: RendererBackend;
		scrollbackLines: number;
	}): Promise<RenderWorkerResponse> {
		return this.send({ type: 'init', ...args });
	}

	/**
	 * Hand the OffscreenCanvas to the worker. The transferable list is
	 * the caller's responsibility — pass `[canvas]` so the underlying
	 * surface actually moves into the worker. P4.6 keeps this as a no-op
	 * on the worker side; P4.7 will wire wasm renderer attachment.
	 *
	 * `fontOpts` (optional) supplies font measurement args that the worker
	 * passes to `RenderHandle.configure()` on the newly-bound canvas,
	 * returning real cell metrics in the `ready` response.
	 */
	bindCanvas(
		paneId: string,
		canvas?: OffscreenCanvas,
		fontOpts?: { font?: string; fontSizePx?: number; dpr?: number },
	): Promise<RenderWorkerResponse> {
		const transfer = canvas ? ([canvas] as Transferable[]) : undefined;
		return this.send(
			{
				type: 'bindCanvas',
				paneId,
				canvas,
				font: fontOpts?.font,
				fontSizePx: fontOpts?.fontSizePx,
				dpr: fontOpts?.dpr,
			},
			transfer,
		);
	}

	/**
	 * Apply a postcard-encoded GridDelta frame. The byte buffer is
	 * transferred (zero-copy) so the main side MUST NOT touch
	 * `bytes.buffer` after this call returns. The caller passes a fresh
	 * Uint8Array per call (the Channel path in `ptyBridge.ts` already
	 * does this — each message creates a new Uint8Array view).
	 */
	applyDelta(paneId: string, bytes: Uint8Array): Promise<RenderWorkerResponse> {
		// Some test environments may pass a Uint8Array backed by a
		// SharedArrayBuffer (can't be transferred). Detect and skip
		// transferList when that's the case.
		const transferable =
			bytes.buffer instanceof ArrayBuffer ? [bytes.buffer] : undefined;
		return this.send({ type: 'applyDelta', paneId, bytes }, transferable);
	}

	feed(paneId: string, bytes: Uint8Array): Promise<RenderWorkerResponse> {
		const transferable =
			bytes.buffer instanceof ArrayBuffer ? [bytes.buffer] : undefined;
		return this.send({ type: 'feed', paneId, bytes }, transferable);
	}

	resize(
		paneId: string,
		rows: number,
		cols: number,
		dpr: number,
		wCss?: number,
		hCss?: number,
	): Promise<RenderWorkerResponse> {
		return this.send({ type: 'resize', paneId, rows, cols, dpr, wCss, hCss });
	}

	/**
	 * Reconfigure the font on an already-initialized pane. The worker
	 * calls `RenderHandle.configure(family, sizePx, dpr)` and returns
	 * cell metrics in the `ready` response (cellW / cellH).
	 */
	setFont(
		paneId: string,
		family: string,
		sizePx: number,
		dpr: number,
	): Promise<RenderWorkerResponse> {
		return this.send({ type: 'setFont', paneId, family, sizePx, dpr });
	}

	destroy(paneId: string): Promise<RenderWorkerResponse> {
		this.cancelPane(paneId);
		return this.send({ type: 'destroy', paneId });
	}

	/** Healthcheck. Useful for tests and for "are you still alive?" probes. */
	ping(token?: string): Promise<RenderWorkerResponse> {
		return this.send({ type: 'ping', token });
	}

	/** Number of in-flight requests. Exposed for tests / diagnostics. */
	pendingCount(): number {
		return this.pending.size;
	}

	// --- internals ---------------------------------------------------------

	private send(
		request: RenderWorkerRequest,
		transfer?: Transferable[],
	): Promise<RenderWorkerResponse> {
		if (this.terminated) {
			return Promise.reject(
				new WorkerRendererError(
					'render worker is terminated',
					'apply_delta_failed',
					'paneId' in request ? request.paneId : undefined,
				),
			);
		}
		if (this.pending.size >= MAX_WORKER_PENDING) {
			const error = new WorkerRendererError(
				`render worker pending limit exceeded (${MAX_WORKER_PENDING})`,
				'apply_delta_failed',
				'paneId' in request ? request.paneId : undefined,
			);
			this.fail(error);
			return Promise.reject(error);
		}
		const id = this.nextReqId++;
		return new Promise<RenderWorkerResponse>((resolve, reject) => {
			const timer = setTimeout(() => {
				const pending = this.pending.get(id);
				if (!pending) return;
				this.pending.delete(id);
				const error = new WorkerRendererError(
					`render worker request timed out after ${WORKER_REQUEST_TIMEOUT_MS}ms`,
					'apply_delta_failed',
					'paneId' in request ? request.paneId : undefined,
				);
				pending.reject(error);
				this.fail(error);
			}, WORKER_REQUEST_TIMEOUT_MS);
			this.pending.set(id, {
				resolve,
				reject,
				timer,
				paneId: 'paneId' in request ? request.paneId : undefined,
			});
			const wire = { ...request, __reqId: id };
			try {
				this.worker.postMessage(wire, transfer ?? []);
			} catch (err) {
				const pending = this.pending.get(id);
				if (pending) clearTimeout(pending.timer);
				this.pending.delete(id);
				reject(err instanceof Error ? err : new Error(String(err)));
			}
		});
	}

	/** Reject and forget every request for a pane before its destroy command
	 * enters the worker queue. Late replies are tombstoned so they do not look
	 * like protocol errors or keep the pane alive through retained callbacks. */
	private cancelPane(paneId: string): void {
		for (const [id, pending] of this.pending) {
			if (pending.paneId !== paneId) continue;
			this.pending.delete(id);
			clearTimeout(pending.timer);
			this.ignoredReqIds.add(id);
			pending.reject(
				new WorkerRendererError(
					'pane destroyed; request cancelled',
					'apply_delta_failed',
					paneId,
				),
			);
		}
		// A pane can produce many frame requests during teardown. Keep late-reply
		// tombstones bounded so cancellation itself cannot become a leak.
		while (this.ignoredReqIds.size > MAX_WORKER_PENDING * 2) {
			const oldest = this.ignoredReqIds.values().next().value as number | undefined;
			if (oldest === undefined) break;
			this.ignoredReqIds.delete(oldest);
		}
	}

	releaseCanvas(paneId: string): Promise<RenderWorkerResponse> {
		return this.send({ type: 'releaseCanvas', paneId });
	}

	private fail(error: Error): void {
		if (this.terminated) return;
		const notify = this.onFatal;
		this.terminate();
		notify?.(error);
	}

	private onMessage(event: MessageEvent): void {
		const data = event.data as
			| (RenderWorkerResponse & { __reqId?: number })
			| undefined;
		const id = data?.__reqId;
		if (id == null) {
			// Unsolicited message — log and drop. The protocol doesn't
			// have server-pushed events yet (P4.9 may add some).
			console.warn('[ridge-term] worker sent message without __reqId', {
				data,
			});
			return;
		}
		const pending = this.pending.get(id);
		if (!pending) {
			if (this.ignoredReqIds.delete(id)) return;
			console.warn(`[ridge-term] worker reply for unknown req id ${id}`, {
				data,
			});
			return;
		}
		this.pending.delete(id);
		clearTimeout(pending.timer);
		if (data && data.type === 'error') {
			pending.reject(
				new WorkerRendererError(data.message, data.code, data.paneId),
			);
			return;
		}
		// Resolve with the typed response; callers can refine via the
		// discriminator if they need the body.
		pending.resolve(data as RenderWorkerResponse);
	}
}
