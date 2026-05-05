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
import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';

export interface ManagerOptions {
	fontFamily: string;
	fontSizePx: number;
	scrollbackLines: number;
	/** xterm-style theme object. Keys: background/foreground/cursor/black/red/... */
	theme?: Record<string, string>;
}

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
}

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
		};
		entry.resizeObserver.observe(container);

		this.panes.set(paneId, entry);
		// Initial fit in the next frame, after the container has settled.
		requestAnimationFrame(() => this.viewportChanged(paneId));
		this.startRafLoop();
	}

	/**
	 * Tear down a pane. Removes the canvas, drops the wasm kernel/handle,
	 * and stops observing.
	 */
	detach(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.resizeObserver.disconnect();
		try {
			entry.canvas.remove();
		} catch {
			/* canvas already detached */
		}
		// Free wasm-side resources. wasm-bindgen generates `.free()` on each
		// exported class; calling it explicitly drops the Rust-side state.
		try { entry.handle.free(); } catch { /* ignore */ }
		try { entry.kernel.free(); } catch { /* ignore */ }
		this.panes.delete(paneId);
		if (this.panes.size === 0) {
			this.stopRafLoop();
		}
	}

	/** Feed PTY bytes into the pane's kernel. Accepts string or Uint8Array. */
	feed(paneId: string, data: string | Uint8Array): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
		entry.kernel.feed(bytes);
	}

	/** Register a callback for keyboard-encoded bytes that should be sent
	 *  to the PTY. Manager calls this from its key event handler. */
	onData(paneId: string, cb: (bytes: Uint8Array) => void): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.dataHandler = cb;
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
			entry.handle.applyDefaultTheme();
			entry.handle.applyTheme(theme);
		}
	}

	/**
	 * Container-size changed — recalc rows/cols, resize canvas surface,
	 * resize wasm kernel, fire onResize callback if (rows, cols) changed.
	 *
	 * Called by ResizeObserver; safe to call manually too.
	 */
	viewportChanged(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		this.fitPane(entry);
	}

	private fitPane(entry: PaneEntry): void {
		const rect = entry.container.getBoundingClientRect();
		const wCss = Math.max(1, Math.floor(rect.width));
		const hCss = Math.max(1, Math.floor(rect.height));

		// Cells fit into the container minus a 1-cell safety margin to
		// avoid clipping the rightmost column when DPR rounding produces
		// a sub-pixel boundary.
		const cols = Math.max(1, Math.floor(wCss / entry.cellW));
		const rows = Math.max(1, Math.floor(hCss / entry.cellH));

		const dpr = window.devicePixelRatio || 1;
		entry.handle.resize(wCss, hCss, dpr);

		entry.kernel.resize(rows, cols);

		if (rows !== entry.lastReportedRows || cols !== entry.lastReportedCols) {
			entry.lastReportedRows = rows;
			entry.lastReportedCols = cols;
			entry.resizeHandler?.(rows, cols);
		}
	}

	// ---- frame loop -------------------------------------------------

	private startRafLoop(): void {
		if (this.rafHandle !== null) return;
		const tick = () => {
			this.rafHandle = null;
			for (const entry of this.panes.values()) {
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
