/**
 * P4.5 (2026-05-21) — Render-worker postMessage protocol.
 *
 * The render worker owns the wasm kernel mirror and (after P4.6) the
 * OffscreenCanvas paint surface for every pane. Main thread keeps input
 * collection, layout, and PTY IPC; it does NOT touch the wasm kernel or
 * paint commands directly. All communication goes through the messages
 * below.
 *
 * Design notes
 * ------------
 *
 * 1. Per-pane addressing. Every request carries a `paneId`; the worker
 *    maintains a `Map<paneId, PaneState>` internally so a single worker
 *    can drive every pane in the workbench. This avoids spinning up one
 *    worker per pane (which would blow up the wasm module load count
 *    and cost N × ~MB of heap for typical splits).
 *
 * 2. Transferables. `applyDelta` carries a `Uint8Array`; both the byte
 *    payload and the underlying `ArrayBuffer` are eligible for
 *    `postMessage(..., [buf])` zero-copy transfer. Callers MUST pass
 *    the buffer in the transferList — the worker assumes it owns the
 *    bytes after the call (the buffer is detached on the main side).
 *
 * 3. Errors. The worker reports failures via `error` responses so the
 *    main thread can show diagnostics. The R5 self-heal path (force
 *    reframe on decode error) lives on the main thread for now —
 *    `ptyBridge.ts` already owns it via `set_pane_delta_mode(false)`.
 *
 * 4. No transferControlToOffscreen yet. That's P4.6. The protocol has
 *    a `bindCanvas` request slot reserved but the worker currently
 *    no-ops it; the actual paint commands still come back to the main
 *    thread until P4.6 lands.
 *
 * Backwards-compat / forward-compat
 * ---------------------------------
 * The discriminated unions below use `type` as the tag. New messages
 * are added by extending the union. Old workers receiving an unknown
 * `type` reply with `{type:'error', code:'unknown_message'}` rather
 * than throwing, so a main-thread roll-forward doesn't brick existing
 * workers in flight.
 */

/** Renderer backend selection. The worker honors the request when the
 *  underlying `@ridge/term-wasm` supports it; otherwise it falls back
 *  to `canvas2d` and surfaces the downgrade in the `ready` ack. */
export type RendererBackend = 'webgpu' | 'canvas2d';

/** Initial dimensions a pane is created with. The worker uses these to
 *  size the wasm kernel grid; the real `resize` will follow as the
 *  Svelte component reports its measured CSS box. */
export interface PaneInitDims {
	rows: number;
	cols: number;
	dpr: number;
}

/** Requests sent from the main thread to the worker. */
export type RenderWorkerRequest =
	| {
			type: 'init';
			paneId: string;
			dims: PaneInitDims;
			backend: RendererBackend;
			scrollbackLines: number;
	  }
	| {
			type: 'bindCanvas';
			paneId: string;
			// P4.6 will populate this with the OffscreenCanvas transferable.
			// For P4.5 the worker accepts the message but no-ops it.
			canvas?: OffscreenCanvas;
			// Font measurement args: the worker calls `RenderHandle.configure`
			// on the newly-bound canvas and returns real cell metrics in the
			// `ready` response (cellW / cellH).
			font?: string;
			fontSizePx?: number;
			dpr?: number;
	  }
	| {
			type: 'applyDelta';
			paneId: string;
			bytes: Uint8Array;
	  }
	| {
			type: 'feed';
			paneId: string;
			data: string;
	  }
	| {
			type: 'resize';
			paneId: string;
			rows: number;
			cols: number;
			dpr: number;
			// CSS dimensions of the container (clientWidth / clientHeight).
			// The worker uses these to resize its backing buffer to match
			// the actual DOM box before painting.
			wCss?: number;
			hCss?: number;
	  }
	| {
			type: 'destroy';
			paneId: string;
	  }
	| {
			type: 'ping';
			// Optional opaque token the worker echoes back in the pong.
			// Useful for latency / healthcheck probes.
			token?: string;
	  }
	| {
			type: 'setFont';
			paneId: string;
			// Font family, font size in px, and device-pixel-ratio for
			// the worker's `RenderHandle.configure()` call.
			family: string;
			sizePx: number;
			dpr: number;
	  };

/** Responses sent from the worker back to the main thread. */
export type RenderWorkerResponse =
	| {
			type: 'ready';
			paneId: string;
			// What the worker actually wired up — may differ from the
			// requested backend if e.g. WebGPU was unavailable.
			backend: RendererBackend;
			// Real cell metrics measured by `RenderHandle.configure()`.
			// Present when the worker was given font/fontSizePx/dpr
			// (typically via `bindCanvas`). The bridge reads these back
			// to update `entry.cellW / cellH` and trigger a fit.
			cellW?: number;
			cellH?: number;
	  }
	| {
			type: 'destroyed';
			paneId: string;
	  }
	| {
			type: 'pong';
			token?: string;
	  }
	| {
			type: 'error';
			paneId?: string;
			code:
				| 'unknown_message'
				| 'pane_not_initialized'
				| 'pane_already_initialized'
				| 'apply_delta_failed'
				| 'feed_failed'
				| 'resize_failed';
			message: string;
	  };

/**
 * Narrow predicate for request validation. The worker entry uses this
 * to decide whether to ACK or to emit `unknown_message`. Centralizing
 * the list of valid tags here keeps the worker in sync with the
 * protocol when new messages are added.
 */
export function isRenderWorkerRequest(value: unknown): value is RenderWorkerRequest {
	if (!value || typeof value !== 'object') return false;
	const t = (value as { type?: unknown }).type;
	return (
		t === 'init' ||
		t === 'bindCanvas' ||
		t === 'applyDelta' ||
		t === 'feed' ||
		t === 'resize' ||
		t === 'destroy' ||
		t === 'ping' ||
		t === 'setFont'
	);
}
