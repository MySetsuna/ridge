// src/lib/terminal/manager.ts
//
// TerminalManager — owns ridge-term wasm kernels and render handles for
// all panes that opted into the new renderer.
//
// ## Round 2.4 design (interim)
//
// Each `attach(paneId, container)` call:
//   1. Creates a fresh `<canvas>` inside `container`.
//   2. Spins up a `TerminalKernel` + `RenderHandle` paired with that canvas.
//   3. Registers them in a Map keyed by paneId.
//
// Round 2.5 will collapse the per-pane canvases into one global surface
// with scissor rectangles. This file's API is shaped so 2.5 won't change
// the call sites — only the implementation.
//
// ## Frame scheduling
//
// Single global rAF loop. Each frame walks all attached panes and calls
// renderHandle.render(kernel). Panes whose grid hasn't changed since
// last frame are no-ops inside wasm (the renderer's dirty-row tracker
// short-circuits). Cost of polling 10 idle panes is ~0.05ms.
//
// ## Lifecycle
//
//   const mgr = TerminalManager.instance();
//   await mgr.ready();                  // wait for wasm init
//   mgr.attach(paneId, divElement);
//   mgr.feed(paneId, ptyBytes);
//   mgr.onData(paneId, (bytes) => { /* send to PTY */ });
//   mgr.viewportChanged(paneId);        // call when container resizes
//   mgr.detach(paneId);                 // on pane unmount

import init, { TerminalKernel, RenderHandle } from '@ridge/term-wasm';
// Vite-native asset URL: this returns the bundled / dev-served path of
// the .wasm file at build time. Bypasses the "auto-locate next to .js"
// path that breaks under vite's pre-bundle (the cause of the
// /node_modules/.vite/deps/ridge_term_bg.wasm 404).
//
// **Required vite.config.js setting** for this to work:
//   optimizeDeps: { exclude: ['@ridge/term-wasm'] }
// Otherwise vite pre-bundles the package, splits it into anonymous chunks
// in node_modules/.vite/deps/, and 404s when init() tries to fetch the
// .wasm next to the .js.
import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';

export interface ManagerOptions {
	fontFamily: string;
	fontSizePx: number;
	scrollbackLines: number;
	/** xterm-style theme object. Keys: background/foreground/cursor/black/red/... */
	theme?: Record<string, string>;
	/** CSS padding (px) applied to each pane's container. Pushes the canvas
	 *  inward so glyphs aren't flush against the pane border. Default 0
	 *  preserves the original look; per-pane overrides via `setPadding`. */
	paddingPx?: number;
}

/** Tagged kernel event shape that mirrors `KernelEvent` in Rust. The
 *  wasm-bindgen serde tag-content config emits these as plain JS objects
 *  with `type` + (when applicable) `value` fields.
 *
 *  Note: OSC 8 hyperlinks do NOT show up here. Open/close transitions
 *  used to be emitted as `HyperlinkOpen` / `HyperlinkClose` but those
 *  variants were removed in TASKS §3.2 — every consumer reads the
 *  per-cell hyperlink state via `kernel.hyperlinkAt(row, col)` (used
 *  by the renderer's underline pass and the Ctrl+click hit-testing in
 *  this file), which made the event stream redundant. */
export type KernelEvent =
	| { type: 'TitleChanged'; value: string }
	| { type: 'IconNameChanged'; value: string }
	| { type: 'CwdChanged'; value: string }
	| { type: 'Bell' };

interface PaneEntry {
	paneId: string;
	container: HTMLElement;
	canvas: HTMLCanvasElement;
	kernel: TerminalKernel;
	handle: RenderHandle;
	cellW: number;
	cellH: number;
	dataHandler?: (bytes: Uint8Array) => void;
	resizeObserver: ResizeObserver;
	/** Last reported (rows, cols) — used to debounce IPC resize calls. */
	lastReportedRows: number;
	lastReportedCols: number;
	/** Optional callback fired when (rows, cols) changes — wired to PTY resize. */
	resizeHandler?: (rows: number, cols: number) => void;
	/** Debounce timer for fit. ResizeObserver fires many times during
	 *  splitpanes drag (or SvelteKit hydration). Each fit calls
	 *  `kernel.resize` AND triggers an async PTY resize via the handler.
	 *  If kernel size oscillates faster than PTY can catch up,
	 *  PSReadLine (which uses absolute cursor positions like CSI 39;18H)
	 *  loses track and emits land on the wrong row → "all output stacked
	 *  on the bottom row" bug. We debounce ~120ms: while the container
	 *  is animating, we don't resize at all; once it settles, fit once. */
	pendingFitTimer: ReturnType<typeof setTimeout> | null;
	/** Optional callback for typed kernel events (title, cwd, hyperlinks,
	 *  bell). Called once per event after each `feed()`. RidgePane wires
	 *  this to the relevant Svelte stores. */
	eventHandler?: (event: KernelEvent) => void;
	/** When the kernel transitions into `?2026` synchronous output mode,
	 *  we record `performance.now()` here. The rAF tick skips render until
	 *  either the kernel exits sync mode OR `SYNC_OUTPUT_TIMEOUT_MS`
	 *  elapses (timeout fallback so a misbehaving TUI can't freeze the
	 *  pane). Reset to null once sync ends. */
	syncStart: number | null;
	/** True once the rAF tick rendered the post-timeout "best-effort" frame
	 *  for a stuck `?2026` sync. Subsequent frames suspend rendering until
	 *  the kernel exits sync mode — without this, the tick would fall
	 *  through to `entry.handle.render(...)` every frame after the
	 *  timeout (since `now - syncStart` keeps exceeding the threshold),
	 *  burning CPU while the TUI is misbehaving. Cleared together with
	 *  `syncStart` when sync mode clears (TASKS §1.4). */
	syncTimeoutRendered: boolean;
	/** focusin listener bound to `container`. Held so detach() can remove
	 *  it cleanly. Emits `\x1b[I` to PTY when kernel.isFocusReporting(). */
	focusListener: (e: FocusEvent) => void;
	/** focusout listener; emits `\x1b[O`. */
	blurListener: (e: FocusEvent) => void;
	/** Mouse-drag selection state. `selecting` is true between pointerdown
	 *  and pointerup; `selectionStart` is the (row,col) where drag began. */
	selecting: boolean;
	selectionStart: { row: number; col: number } | null;
	pointerDownListener: (e: PointerEvent) => void;
	pointerMoveListener: (e: PointerEvent) => void;
	pointerUpListener: (e: PointerEvent) => void;
	/** Last `clamped` value passed to `setPadding`. Used to short-circuit
	 *  no-op calls (RidgePane wires setPadding into a $effect that fires
	 *  on every settings store update — without this, every font-size /
	 *  shell-pref / search-glob change would cascade to viewportChanged →
	 *  fitPane on every pane just to re-set padding to its current value).
	 *  `undefined` means "not yet set" — first call applies regardless. */
	lastAppliedPaddingPx?: number;
	/** Parking state (TASKS §5.1, Round 6).
	 *
	 *  When `parked = true`:
	 *   - `kernel` is alive (terminal grid, scrollback, attrs, modes,
	 *     scroll offset, current_link, IME composition state — everything
	 *     load-bearing for user-perceived continuity is preserved).
	 *   - `handle` has been `.free()`'d and `canvas` removed from DOM.
	 *   - All container event listeners are unbound.
	 *   - `dataHandler` / `eventHandler` / `resizeHandler` callbacks are
	 *     still wired so PTY bytes arriving during the park window land
	 *     in the kernel without loss.
	 *   - The render loop skips this pane (no handle to call render on).
	 *
	 *  Set true by `park(paneId)` and false by `unpark(paneId, container)`.
	 *  `detach(paneId)` works regardless of parked state — both code paths
	 *  release wasm resources at the end. */
	parked: boolean;
}

/** Maximum hold time for `?2026` synchronous output mode. xterm uses 150ms;
 *  matching keeps Ink/lazygit/bottom behaviour consistent across terminals. */
const SYNC_OUTPUT_TIMEOUT_MS = 150;

/**
 * Singleton. Created lazily on first `instance()` call. Held by the
 * `<RidgeTerminalRoot>` Svelte component for the entire app lifetime.
 */
export class TerminalManager {
	private static _instance: TerminalManager | null = null;

	private wasmReady = false;
	private wasmReadyPromise: Promise<void> | null = null;

	private opts: ManagerOptions;
	private panes = new Map<string, PaneEntry>();
	private rafHandle: number | null = null;

	private constructor(opts: ManagerOptions) {
		this.opts = opts;
	}

	static instance(opts?: ManagerOptions): TerminalManager {
		if (!TerminalManager._instance) {
			TerminalManager._instance = new TerminalManager(
				opts ?? {
					fontFamily:
						'"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", monospace',
					fontSizePx: 15,
					scrollbackLines: 2000,
				},
			);
			// Dev convenience: expose the singleton so `window.__rt` works in
			// the browser console without needing dynamic import. Removed in
			// production builds via Vite's import.meta.env.DEV guard.
			if (typeof window !== 'undefined' && import.meta.env?.DEV) {
				(window as unknown as { __rt: TerminalManager }).__rt = TerminalManager._instance;
			}
		}
		return TerminalManager._instance;
	}

	/**
	 * Resolves once the wasm module is initialized. Idempotent — multiple
	 * callers share the same in-flight promise.
	 *
	 * `init(wasmUrl)` is given an explicit URL rather than relying on
	 * the default `new URL('ridge_term_bg.wasm', import.meta.url)` —
	 * vite's dep pre-bundling moves the .js into `node_modules/.vite/deps/`
	 * but doesn't follow the side-loaded .wasm, producing a 404. The
	 * `?url` import (above) is vite's official asset-URL syntax and
	 * resolves to whatever path actually serves the file.
	 */
	ready(): Promise<void> {
		if (this.wasmReady) return Promise.resolve();
		if (this.wasmReadyPromise) return this.wasmReadyPromise;
		this.wasmReadyPromise = (async () => {
			await init(wasmUrl);
			this.wasmReady = true;
		})();
		return this.wasmReadyPromise;
	}

	/**
	 * Bind a pane to the manager. Creates a `<canvas>` child of `container`,
	 * spins up the wasm kernel/renderer, starts observing the container
	 * for resize events.
	 *
	 * Throws if the manager isn't ready (caller must `await ready()` first)
	 * or if `paneId` is already attached.
	 */
	attach(paneId: string, container: HTMLElement): void {
		if (!this.wasmReady) {
			throw new Error('TerminalManager.attach: call ready() first');
		}
		if (this.panes.has(paneId)) {
			throw new Error(`TerminalManager.attach: pane ${paneId} already attached`);
		}

		const canvas = document.createElement('canvas');
		canvas.style.cssText = 'display:block; width:100%; height:100%;';
		canvas.setAttribute('aria-hidden', 'true');
		container.appendChild(canvas);

		// Apply initial padding to the container so the canvas (width:100%
		// inside content-box) starts inset by `opts.paddingPx`. Per-pane
		// updates after attach come through `setPadding(paneId, px)`.
		if (this.opts.paddingPx && this.opts.paddingPx > 0) {
			container.style.padding = `${this.opts.paddingPx}px`;
		}

		const handle = new RenderHandle(canvas);
		const dpr = window.devicePixelRatio || 1;

		// configure() returns [cellW, cellH] in CSS pixels at the supplied DPR.
		const [cellW, cellH] = handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as
			| [number, number]
			| Float32Array;
		const cellWnum = Number(cellW);
		const cellHnum = Number(cellH);

		// Seed kernel with default 24×80 — we'll resize to actual size right away.
		const kernel = new TerminalKernel(24, 80, this.opts.scrollbackLines);

		// Apply theme if provided.
		if (this.opts.theme) {
			handle.applyTheme(this.opts.theme);
		}

		// Focus reporting (`?1004`) — emit `\x1b[I` / `\x1b[O` to PTY when
		// the kernel says reporting is enabled. We use focusin/focusout
		// (vs focus/blur) so events bubble up from interactive descendants
		// (the canvas takes focus on click via the parent's tabIndex).
		// Captured into closures up-front so detach() can unbind cleanly.
		const focusListener = (_e: FocusEvent) => {
			const e = this.panes.get(paneId);
			if (!e || !e.dataHandler) return;
			if (!e.kernel.isFocusReporting()) return;
			e.dataHandler(new TextEncoder().encode('\x1b[I'));
		};
		const blurListener = (_e: FocusEvent) => {
			const e = this.panes.get(paneId);
			if (!e || !e.dataHandler) return;
			if (!e.kernel.isFocusReporting()) return;
			e.dataHandler(new TextEncoder().encode('\x1b[O'));
		};
		container.addEventListener('focusin', focusListener);
		container.addEventListener('focusout', blurListener);

		// Mouse-drag selection. The kernel already exposes setSelection /
		// getSelectionText; we just translate pointer coords → cell coords
		// and stream updates while dragging. Pointer capture on pointerdown
		// keeps moves flowing even when the cursor leaves the container.
		const computeCell = (e: PointerEvent): { row: number; col: number } | null => {
			const ent = this.panes.get(paneId);
			if (!ent || ent.cellW <= 0 || ent.cellH <= 0) return null;
			const rect = ent.container.getBoundingClientRect();
			const x = e.clientX - rect.left;
			const y = e.clientY - rect.top;
			const cols = ent.kernel.cols();
			const rows = ent.kernel.rows();
			if (cols === 0 || rows === 0) return null;
			const col = Math.max(0, Math.min(cols - 1, Math.floor(x / ent.cellW)));
			const row = Math.max(0, Math.min(rows - 1, Math.floor(y / ent.cellH)));
			return { row, col };
		};
		const pointerDownListener = (e: PointerEvent) => {
			// Only primary button (left). Right-click / middle should not
			// hijack selection — context menu handler in RidgePane owns those.
			if (e.button !== 0) return;
			const cell = computeCell(e);
			if (!cell) return;
			const ent = this.panes.get(paneId);
			if (!ent) return;
			// Ctrl/Cmd+click → if cell is inside an OSC 8 hyperlink span,
			// open it via the Tauri opener (or window.open as fallback).
			// Goes BEFORE selection branches so links beat selection on
			// modifier-click, matching iTerm/VSCode behaviour.
			const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
			const mod = e.ctrlKey || (isMac && e.metaKey);
			if (mod) {
				const link = ent.kernel.hyperlinkAt(cell.row, cell.col) as
					| { uri: string; id: string | null }
					| null;
				if (link && link.uri) {
					const uri = link.uri;
					if (typeof window !== 'undefined' && (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
						void import('@tauri-apps/plugin-opener')
							.then(({ openUrl }) => openUrl(uri))
							.catch((err) => console.warn('[ridge-term] openUrl failed', uri, err));
					} else {
						window.open(uri, '_blank', 'noopener,noreferrer');
					}
					e.preventDefault();
					return;
				}
				// Modifier-click without a link → fall through to normal
				// selection logic (respects Shift below).
			}
			// Shift-click extends the existing selection from its anchor
			// (last drag's start) to the clicked cell. If there's no
			// anchor yet, treat it as a normal click. Continues into drag
			// mode so subsequent move keeps extending — same as xterm.
			if (e.shiftKey && ent.selectionStart) {
				try { (e.target as Element | null)?.setPointerCapture?.(e.pointerId); } catch {}
				ent.selecting = true;
				ent.kernel.setSelection(
					ent.selectionStart.row, ent.selectionStart.col,
					cell.row, cell.col,
				);
				return;
			}
			// Multi-click: e.detail counts consecutive clicks within the
			// browser's double-click interval. Triple-click = full line,
			// double-click = word at cell. We do NOT enter drag mode for
			// these — a follow-up move shouldn't shrink/extend the multi-
			// click selection (matches xterm/iTerm behaviour).
			if (e.detail === 2) {
				ent.kernel.selectWordAt(cell.row, cell.col);
				return;
			}
			if (e.detail >= 3) {
				ent.kernel.selectLineAt(cell.row);
				return;
			}
			try { (e.target as Element | null)?.setPointerCapture?.(e.pointerId); } catch {}
			ent.selecting = true;
			ent.selectionStart = cell;
			// Empty range → kernel.setSelection.set treats as clear, which
			// is exactly what a single click should do (clear any prior
			// selection until the user actually drags).
			ent.kernel.setSelection(cell.row, cell.col, cell.row, cell.col);
		};
		const pointerMoveListener = (e: PointerEvent) => {
			const ent = this.panes.get(paneId);
			if (!ent) return;
			// Ctrl-hover over an OSC 8 hyperlink → pointer cursor as
			// affordance. Any other state resets cursor (when ctrl is
			// released or pointer moves off a link). Round-trips don't
			// fire on bare key events so the user must wiggle the mouse
			// once after releasing/pressing Ctrl — minor; round 5 can
			// add keydown/keyup hooks if needed.
			const hoverCell = computeCell(e);
			const isMacUA = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
			const mod = e.ctrlKey || (isMacUA && e.metaKey);
			if (hoverCell && mod) {
				const link = ent.kernel.hyperlinkAt(hoverCell.row, hoverCell.col);
				ent.container.style.cursor = link ? 'pointer' : '';
			} else if (ent.container.style.cursor === 'pointer') {
				ent.container.style.cursor = '';
			}

			// Continue with selection drag logic.
			if (!ent.selecting || !ent.selectionStart || !hoverCell) return;
			ent.kernel.setSelection(
				ent.selectionStart.row,
				ent.selectionStart.col,
				hoverCell.row,
				hoverCell.col,
			);
		};
		const pointerUpListener = (e: PointerEvent) => {
			const ent = this.panes.get(paneId);
			if (!ent) return;
			ent.selecting = false;
			try { (e.target as Element | null)?.releasePointerCapture?.(e.pointerId); } catch {}
		};
		container.addEventListener('pointerdown', pointerDownListener);
		container.addEventListener('pointermove', pointerMoveListener);
		container.addEventListener('pointerup', pointerUpListener);

		const entry: PaneEntry = {
			paneId,
			container,
			canvas,
			kernel,
			handle,
			cellW: cellWnum,
			cellH: cellHnum,
			resizeObserver: new ResizeObserver(() => this.viewportChanged(paneId)),
			lastReportedRows: -1,
			lastReportedCols: -1,
			pendingFitTimer: null,
			syncStart: null,
			syncTimeoutRendered: false,
			focusListener,
			blurListener,
			selecting: false,
			selectionStart: null,
			pointerDownListener,
			pointerMoveListener,
			pointerUpListener,
			parked: false,
		};
		entry.resizeObserver.observe(container);

		this.panes.set(paneId, entry);
		// Initial fit: do it once synchronously after layout settles. We
		// wait one rAF (so SvelteKit hydration finishes), then fit
		// directly without debounce so the PTY gets sized before any
		// shell output arrives.
		requestAnimationFrame(() => {
			if (this.panes.has(paneId)) this.fitPane(entry);
		});
		this.startRafLoop();
	}

	/**
	 * Tear down a pane completely. Frees the kernel, frees the render
	 * handle, removes the canvas, and drops the entry from the map.
	 *
	 * Idempotent against parking state: if the pane is currently parked,
	 * `entry.handle` and `entry.canvas` are already gone, so we just free
	 * the kernel and remove the entry. Caller must use `detach` for
	 * "the pane is permanently gone" (e.g. removed from paneTree) and
	 * `park` for "transient unmount across split / reparent" — see §5.1.
	 */
	detach(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		if (!entry.parked) {
			// Live entry — release DOM bindings before freeing wasm.
			entry.resizeObserver.disconnect();
			entry.container.removeEventListener('focusin', entry.focusListener);
			entry.container.removeEventListener('focusout', entry.blurListener);
			entry.container.removeEventListener('pointerdown', entry.pointerDownListener);
			entry.container.removeEventListener('pointermove', entry.pointerMoveListener);
			entry.container.removeEventListener('pointerup', entry.pointerUpListener);
			if (entry.pendingFitTimer !== null) {
				clearTimeout(entry.pendingFitTimer);
				entry.pendingFitTimer = null;
			}
			try {
				entry.canvas.remove();
			} catch {
				/* canvas already detached */
			}
			try { entry.handle.free(); } catch { /* ignore */ }
		}
		// Kernel always alive while in the map (parked or not).
		try { entry.kernel.free(); } catch { /* ignore */ }
		this.panes.delete(paneId);
		if (this.panes.size === 0) {
			this.stopRafLoop();
		}
	}

	/**
	 * Park a pane: release everything that's bound to the current DOM
	 * container (canvas, render handle, ResizeObserver, focus / pointer
	 * listeners) but keep the wasm kernel + dataHandler / eventHandler /
	 * resizeHandler closures alive.
	 *
	 * Used when a Svelte component unmounts due to a split or reparent —
	 * we don't know yet whether the pane is genuinely closing or about
	 * to remount. Parking is cheap to reverse via `unpark`.
	 *
	 * If the pane is already parked or unknown, this is a no-op.
	 */
	park(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;

		entry.resizeObserver.disconnect();
		entry.container.removeEventListener('focusin', entry.focusListener);
		entry.container.removeEventListener('focusout', entry.blurListener);
		entry.container.removeEventListener('pointerdown', entry.pointerDownListener);
		entry.container.removeEventListener('pointermove', entry.pointerMoveListener);
		entry.container.removeEventListener('pointerup', entry.pointerUpListener);
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
			entry.pendingFitTimer = null;
		}
		// Clear transient pointer drag state — if the user was mid-drag
		// when the unmount fired, the next attach should start fresh.
		entry.selecting = false;
		entry.selectionStart = null;

		try { entry.canvas.remove(); } catch { /* already detached */ }
		try { entry.handle.free(); } catch { /* ignore */ }

		entry.parked = true;
		// Don't stopRafLoop here — other panes may still need rendering.
		// The render-loop guards against parked entries by checking the
		// flag before calling `entry.handle.render(...)`.
	}

	/**
	 * Reverse of `park`: bind the existing kernel to a new container.
	 * Creates a fresh canvas + RenderHandle, re-installs the previously
	 * captured listener closures (which look up `this.panes.get(paneId)`
	 * and so naturally see the updated `entry.container`), and rejoins
	 * the render loop.
	 *
	 * Throws if the paneId isn't in the map at all (caller bug — should
	 * have called `attach` instead) or if the entry is already attached
	 * (double-unpark indicates a lifecycle ordering bug).
	 */
	unpark(paneId: string, container: HTMLElement): void {
		if (!this.wasmReady) {
			throw new Error('TerminalManager.unpark: call ready() first');
		}
		const entry = this.panes.get(paneId);
		if (!entry) {
			throw new Error(`TerminalManager.unpark: pane ${paneId} not parked (use attach for new panes)`);
		}
		if (!entry.parked) {
			throw new Error(`TerminalManager.unpark: pane ${paneId} is already attached`);
		}

		const canvas = document.createElement('canvas');
		canvas.style.cssText = 'display:block; width:100%; height:100%;';
		canvas.setAttribute('aria-hidden', 'true');
		container.appendChild(canvas);

		if (this.opts.paddingPx && this.opts.paddingPx > 0) {
			container.style.padding = `${this.opts.paddingPx}px`;
		}

		const handle = new RenderHandle(canvas);
		const dpr = window.devicePixelRatio || 1;
		const [cellW, cellH] = handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as
			| [number, number]
			| Float32Array;
		if (this.opts.theme) {
			handle.applyTheme(this.opts.theme);
		}

		entry.container = container;
		entry.canvas = canvas;
		entry.handle = handle;
		entry.cellW = Number(cellW);
		entry.cellH = Number(cellH);
		// Force a resize-handler emit on the next fit so PTY rows/cols
		// resync — in particular if the new container has different
		// dimensions from the parked one.
		entry.lastReportedRows = -1;
		entry.lastReportedCols = -1;

		container.addEventListener('focusin', entry.focusListener);
		container.addEventListener('focusout', entry.blurListener);
		container.addEventListener('pointerdown', entry.pointerDownListener);
		container.addEventListener('pointermove', entry.pointerMoveListener);
		container.addEventListener('pointerup', entry.pointerUpListener);
		entry.resizeObserver = new ResizeObserver(() => this.viewportChanged(paneId));
		entry.resizeObserver.observe(container);

		entry.parked = false;

		requestAnimationFrame(() => {
			const e = this.panes.get(paneId);
			if (e && !e.parked) this.fitPane(e);
		});
		this.startRafLoop();
	}

	/** True if a pane is in the manager but currently parked.
	 *  Useful for the RidgePane onMount path to decide attach vs unpark. */
	isParked(paneId: string): boolean {
		const entry = this.panes.get(paneId);
		return entry !== undefined && entry.parked;
	}

	/** Feed PTY bytes into the pane's kernel. Accepts string or Uint8Array.
	 *
	 *  After consuming `bytes`, drain TWO outbound queues:
	 *   1. `pending_response` (raw bytes) → ship back to PTY via dataHandler
	 *      (DSR/DA query responses; needed so PSReadLine can re-anchor).
	 *   2. `pending_events` (typed events) → dispatch to `eventHandler`
	 *      (title / cwd / hyperlinks / bell → relevant Svelte stores). */
	feed(paneId: string, data: string | Uint8Array): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
		entry.kernel.feed(bytes);

		const reply = entry.kernel.takePendingResponse();
		if (reply.length > 0 && entry.dataHandler) {
			entry.dataHandler(reply);
		}

		// Always drain pending_events, even when no handler is registered.
		// If we gate the drain on `entry.eventHandler`, events emitted before
		// the consumer wires its handler accumulate in the wasm queue and
		// then arrive batched on a later feed() — out of sync with the
		// screen state at the moment they were originally emitted (CWD /
		// title flicker, hyperlinks pointing at the wrong cell). Drain
		// unconditionally to bound the queue; warn in dev when events are
		// discarded so ordering bugs surface during development.
		const events = entry.kernel.takePendingEvents() as KernelEvent[];
		if (entry.eventHandler) {
			for (const ev of events) entry.eventHandler(ev);
		} else if (events.length > 0 && import.meta.env?.DEV) {
			console.warn(
				'[ridge-term] feed() drained',
				events.length,
				'kernel events but no eventHandler registered for pane',
				paneId,
				'— events discarded; check onEvent() registration order',
			);
		}
	}

	/** Prepend older history bytes at the OLDEST end of this pane's
	 *  scrollback ring. The bytes go through an isolated sandbox terminal
	 *  in wasm so the live grid / cursor / attrs / pending queues are
	 *  untouched (see `Terminal::prepend_scrollback` in Rust).
	 *
	 *  Caller is responsible for fetching the bytes from wherever they
	 *  live (the Tauri `get_pane_scrollback_before` IPC, in Ridge's case)
	 *  and tracking the seq cursor for paged "load older" UX. Manager
	 *  itself stays host-agnostic — it doesn't know about Tauri. */
	prependScrollback(paneId: string, data: string | Uint8Array): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
		if (bytes.length === 0) return;
		entry.kernel.prependScrollback(bytes);
		// No selection / search clear here: prepend grows the scrollback
		// at its older end and leaves all existing rows in place, so any
		// currently-active selection or search anchor is still valid.
		// Likewise no pending_response / pending_events to drain — the
		// kernel discards both for prepend-mode bytes by design.
	}

	/** Subscribe to typed kernel events (title, cwd, hyperlinks, bell).
	 *  Replaces any previously-registered handler for the same pane. */
	onEvent(paneId: string, cb: (event: KernelEvent) => void): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.eventHandler = cb;
	}

	/** Register a callback for keyboard-encoded bytes that should be sent
	 *  to the PTY. Manager calls this from its key event handler. */
	onData(paneId: string, cb: (bytes: Uint8Array) => void): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.dataHandler = cb;
	}

	/** Send arbitrary bytes (or a string, UTF-8-encoded) to the PTY through
	 *  the registered dataHandler. Used for IME composition results, paste,
	 *  and any other path that produces text that should reach the shell
	 *  as if typed. */
	write(paneId: string, data: string | Uint8Array): void {
		const entry = this.panes.get(paneId);
		if (!entry || !entry.dataHandler) return;
		const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
		if (bytes.length > 0) entry.dataHandler(bytes);
	}

	/** Register a callback for (rows, cols) changes — wire to PTY resize. */
	onResize(paneId: string, cb: (rows: number, cols: number) => void): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.resizeHandler = cb;
	}

	/**
	 * Forward a keyboard event to the kernel's encoder, push the encoded
	 * bytes through the registered onData callback. Returns true if the
	 * event was consumed (caller should preventDefault).
	 */
	handleKeyDown(paneId: string, ev: KeyboardEvent): boolean {
		const entry = this.panes.get(paneId);
		if (!entry || !entry.dataHandler) return false;

		// macOS: treat Cmd as Ctrl for terminal apps.
		const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
		const ctrl = ev.ctrlKey || (isMac && ev.metaKey);

		const bytes = entry.kernel.encodeKey(ev.key, ctrl, ev.altKey, ev.shiftKey, ev.metaKey);
		if (bytes.length === 0) return false;
		entry.dataHandler(bytes);
		return true;
	}

	/**
	 * Paste text into the pane. Wraps in bracketed-paste markers if mode 2004
	 * is active. Pushes through onData.
	 */
	paste(paneId: string, text: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || !entry.dataHandler) return;
		const bytes = entry.kernel.encodePaste(text);
		entry.dataHandler(bytes);
	}

	/** Programmatic select-all. */
	selectAll(paneId: string): void {
		this.panes.get(paneId)?.kernel.selectAll();
	}

	/** Get currently selected text (empty string if no selection). */
	getSelectionText(paneId: string): string {
		return this.panes.get(paneId)?.kernel.getSelectionText() ?? '';
	}

	clearSelection(paneId: string): void {
		this.panes.get(paneId)?.kernel.clearSelection();
	}

	/** Tell the wasm renderer whether this pane is the focused one. Only the
	 *  truly focused pane should blink its cursor; unfocused panes hide it
	 *  entirely. RidgePane wires this to the global `activePaneId` store so
	 *  switching panes flips the cursor visibility on both sides instantly. */
	setFocused(paneId: string, focused: boolean): void {
		this.panes.get(paneId)?.handle.setFocused(focused);
	}

	/** Apply CSS padding (px) to a pane's container. Pushes the canvas inward
	 *  so glyphs aren't flush against the pane border. The change triggers a
	 *  fit on the next animation frame (ResizeObserver picks it up); for an
	 *  immediate effect we also call `viewportChanged(paneId)` synchronously.
	 *
	 *  No-op when the resolved px hasn't changed since the last call for
	 *  this pane — RidgePane wires this from a `$effect` keyed on
	 *  `$settingsStore.terminalPaddingPx`, and Svelte's $effect re-runs on
	 *  any settings store fire (font, shell, search globs, …). Without
	 *  this guard a font-size change would cascade to a viewportChanged
	 *  → fitPane on every pane just to re-set padding to its current value. */
	setPadding(paneId: string, px: number): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const clamped = Math.max(0, Math.min(64, Math.round(px)));
		if (entry.lastAppliedPaddingPx === clamped) return;
		entry.lastAppliedPaddingPx = clamped;
		entry.container.style.padding = clamped > 0 ? `${clamped}px` : '';
		this.viewportChanged(paneId);
	}

	// ---- search forwarders -----------------------------------------

	/** Run a viewport search. Returns match count. The active match (first
	 *  one) is highlighted via the selection overlay automatically. */
	searchSetQuery(paneId: string, query: string, caseSensitive: boolean): number {
		return this.panes.get(paneId)?.kernel.searchSetQuery(query, caseSensitive) ?? 0;
	}

	searchNext(paneId: string): number {
		return this.panes.get(paneId)?.kernel.searchNext() ?? Number.MAX_SAFE_INTEGER;
	}

	searchPrev(paneId: string): number {
		return this.panes.get(paneId)?.kernel.searchPrev() ?? Number.MAX_SAFE_INTEGER;
	}

	searchClear(paneId: string): void {
		this.panes.get(paneId)?.kernel.searchClear();
	}

	searchInfo(paneId: string): { count: number; activeIndex: number } {
		const e = this.panes.get(paneId);
		if (!e) return { count: 0, activeIndex: -1 };
		const idx = e.kernel.searchActiveIndex();
		return {
			count: e.kernel.searchMatchCount(),
			// kernel returns usize::MAX (~9007199254740991 on 64-bit JS)
			// when there's no active match; normalise to -1 for JS callers.
			activeIndex: idx >= Number.MAX_SAFE_INTEGER ? -1 : idx,
		};
	}

	/** Snap viewport to bottom (live grid). */
	scrollToBottom(paneId: string): void {
		this.panes.get(paneId)?.kernel.scrollToBottom();
	}

	scrollUp(paneId: string, lines: number): void {
		this.panes.get(paneId)?.kernel.scrollUp(lines);
	}

	scrollDown(paneId: string, lines: number): void {
		this.panes.get(paneId)?.kernel.scrollDown(lines);
	}

	/** Returns scroll offset (0 = at bottom) and scrollback length, for UI hints. */
	scrollState(paneId: string): { offset: number; total: number } {
		const e = this.panes.get(paneId);
		if (!e) return { offset: 0, total: 0 };
		return { offset: e.kernel.scrollOffset(), total: e.kernel.scrollbackLen() };
	}

	rows(paneId: string): number { return this.panes.get(paneId)?.kernel.rows() ?? 0; }
	cols(paneId: string): number { return this.panes.get(paneId)?.kernel.cols() ?? 0; }

	/** Pixel position of the kernel cursor relative to the pane container's
	 *  top-left, plus the cell height (so callers can place a one-line
	 *  helper element BELOW the current cursor row). Returns null when
	 *  the pane is unknown or cell metrics aren't ready yet. */
	cursorPixelPosition(paneId: string): { x: number; y: number; cellH: number } | null {
		const e = this.panes.get(paneId);
		if (!e || e.cellW <= 0 || e.cellH <= 0) return null;
		const row = e.kernel.cursorRow();
		const col = e.kernel.cursorCol();
		return {
			x: Math.round(col * e.cellW),
			y: Math.round(row * e.cellH),
			cellH: e.cellH,
		};
	}

	/**
	 * Apply font family/size globally. Re-measures cells and triggers fit
	 * for every attached pane. (round 2.5 will store this once per surface
	 * rather than per-pane.)
	 */
	setFont(family: string, sizePx: number): void {
		this.opts.fontFamily = family;
		this.opts.fontSizePx = sizePx;
		const dpr = window.devicePixelRatio || 1;
		for (const entry of this.panes.values()) {
			// Skip parked entries — their handle has been freed. They'll
			// pick up the new font on the next unpark via this.opts.
			if (entry.parked) continue;
			const [w, h] = entry.handle.configure(family, sizePx, dpr) as
				| [number, number]
				| Float32Array;
			entry.cellW = Number(w);
			entry.cellH = Number(h);
			entry.handle.invalidateAll();
			this.fitPane(entry);
		}
	}

	/** Apply theme overrides to all panes. */
	setTheme(theme: Record<string, string>): void {
		this.opts.theme = theme;
		for (const entry of this.panes.values()) {
			// Parked panes pick up the theme on the next unpark via this.opts.
			if (entry.parked) continue;
			entry.handle.applyDefaultTheme();
			entry.handle.applyTheme(theme);
		}
	}

	/**
	 * Container-size changed. Trailing-edge debounce 120ms: while
	 * splitpanes is being dragged (or any continuous container resize is
	 * happening), `viewportChanged` may fire dozens of times per second.
	 * Each call would resize the kernel AND fire an async PTY resize.
	 * If kernel size oscillates faster than the PTY catches up, in-flight
	 * shell bytes (which were emitted under the OLD viewport) land on
	 * a smaller grid and PSReadLine's absolute-cursor positioning (e.g.
	 * `CSI 39;18 H`) clamps to the new last row → "everything on bottom
	 * row" bug.
	 *
	 * 120ms is short enough to feel instant after drag ends, long enough
	 * to skip continuous drag frames. Initial fit at attach() bypasses
	 * the debounce.
	 */
	viewportChanged(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
		}
		entry.pendingFitTimer = setTimeout(() => {
			entry.pendingFitTimer = null;
			const e = this.panes.get(paneId);
			// Re-check parked: a park() call could have come in during
			// the 120 ms debounce window, freeing entry.handle.
			if (!e || e.parked) return;
			this.fitPane(e);
		}, 120);
	}

	private fitPane(entry: PaneEntry): void {
		// Read CANVAS dimensions (not container) so any padding applied to the
		// container is correctly excluded. Canvas is `width:100%; height:100%`
		// inside the container's content-box, so its rect is the actual
		// drawing area regardless of `paddingPx`.
		const rect = entry.canvas.getBoundingClientRect();
		const wCss = Math.floor(rect.width);
		const hCss = Math.floor(rect.height);

		// Skip fit until the container actually has size. splitpanes (and
		// SvelteKit hydration in general) frequently mount a Pane whose
		// containing flex/grid hasn't laid out yet — `getBoundingClientRect`
		// returns 0×0 for one or two frames. If we proceed with `Math.max(1,
		// Math.floor(0 / cellW))` we'd resize the kernel to 1×1, the PTY
		// fires SIGWINCH for 1×1, and now every byte the shell emits scrolls
		// off-screen instantly (looks like "all output stacked on one line").
		// Better: bail. The ResizeObserver will call us again when the layout
		// settles.
		if (wCss <= 0 || hCss <= 0) return;
		if (entry.cellW <= 0 || entry.cellH <= 0) return;

		// Cells fit into the container; round DOWN to avoid drawing past
		// the right/bottom edge.
		const cols = Math.max(1, Math.floor(wCss / entry.cellW));
		const rows = Math.max(1, Math.floor(hCss / entry.cellH));

		const dpr = window.devicePixelRatio || 1;

		// Always resize the canvas surface (cheap, immediate).
		entry.handle.resize(wCss, hCss, dpr);

		const sizeChanged = rows !== entry.lastReportedRows || cols !== entry.lastReportedCols;
		if (!sizeChanged) {
			// Surface size changed (different DPR or container dimensions
			// that don't translate to a different cell grid) but cells stay
			// the same — kernel resize would be a no-op, skip.
			return;
		}
		// Dev-only diagnostic: which paneIds are actually firing a real
		// rows×cols change. Lets the user (and us) verify in DevTools
		// console which panes a splitter drag actually triggers, to
		// disambiguate "non-adjacent panes are resizing" reports — if
		// only the two adjacent paneIds log here on a single drag, the
		// CSS layer is doing the right thing; if more panes log, it's
		// a real fan-out bug. Production builds gate on import.meta.env.DEV
		// so this never reaches end users.
		if (import.meta.env?.DEV) {
			console.debug(
				'[ridge-term] fit',
				entry.paneId,
				`${cols}×${rows}`,
				`(was ${entry.lastReportedCols < 0 ? '?' : entry.lastReportedCols}×${entry.lastReportedRows < 0 ? '?' : entry.lastReportedRows})`,
			);
		}
		entry.lastReportedRows = rows;
		entry.lastReportedCols = cols;

		// Critical ordering: tell PTY first, then resize kernel.
		//
		// Why: PSReadLine and other shells emit absolute cursor positions
		// (e.g. CSI 39;18 H to put cursor on row 39). When the kernel
		// resizes BEFORE the PTY knows about the new size, in-flight bytes
		// emitted under the old size land on the new (smaller) grid and
		// the cursor clamps to the new last row.
		entry.resizeHandler?.(rows, cols);
		entry.kernel.resize(rows, cols);
	}

	// ---- frame loop -------------------------------------------------

	private startRafLoop(): void {
		if (this.rafHandle !== null) return;
		const tick = () => {
			this.rafHandle = null;
			const now = performance.now();
			for (const entry of this.panes.values()) {
				// Skip parked entries — kernel is alive but handle was
				// freed by park(); render would dereference a dead pointer.
				if (entry.parked) continue;
				// Synchronous output mode (?2026): hold rendering while the
				// TUI emits a multi-step redraw, so the user never sees a
				// torn intermediate frame. Timeout (150ms) prevents a stuck
				// app from freezing the pane forever.
				const sync = entry.kernel.isSyncOutput();
				if (sync) {
					if (entry.syncStart === null) entry.syncStart = now;
					if (now - entry.syncStart < SYNC_OUTPUT_TIMEOUT_MS) {
						continue; // hold the frame
					}
					// Timeout reached. Render the best-effort frame ONCE,
					// then suspend further renders until the kernel exits
					// sync. Without this guard, every subsequent rAF tick
					// satisfies `now - syncStart >= TIMEOUT` and falls
					// through to `handle.render(...)` — burst-rendering
					// at 60 fps for as long as the TUI's sync stays stuck
					// (TASKS §1.4).
					if (entry.syncTimeoutRendered) {
						continue;
					}
					entry.syncTimeoutRendered = true;
				} else if (entry.syncStart !== null) {
					// Clean exit from sync mode — reset for next cycle.
					entry.syncStart = null;
					entry.syncTimeoutRendered = false;
				}
				try {
					entry.handle.render(entry.kernel);
				} catch (err) {
					// Don't let one pane's render error kill the whole loop.
					console.error('[ridge-term] render error', entry.paneId, err);
				}
			}
			if (this.panes.size > 0) {
				this.rafHandle = requestAnimationFrame(tick);
			}
		};
		this.rafHandle = requestAnimationFrame(tick);
	}

	private stopRafLoop(): void {
		if (this.rafHandle !== null) {
			cancelAnimationFrame(this.rafHandle);
			this.rafHandle = null;
		}
	}
}
