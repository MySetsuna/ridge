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

import init, { TerminalKernel, RenderHandle, SurfaceHostHandle } from '@ridge/term-wasm';
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
	/** Try the WebGPU render backend first, fall back to Canvas2D on
	 *  adapter miss / device-creation failure (TASKS §4.5.e).
	 *
	 *  When the wasm bundle was built without `--features webgpu`, the
	 *  `RenderHandle.newWithWebgpuFirst` static method does not exist;
	 *  `_makeHandle` detects this via `typeof` and falls back to the
	 *  synchronous `new RenderHandle(canvas)` constructor. Setting this
	 *  flag in a Canvas2D-only build is therefore a no-op, not an error.
	 *
	 *  Default: read from `localStorage.RIDGE_WEBGPU === '1'` so users can
	 *  flip it from the browser console without rebuilding. */
	preferWebgpu?: boolean;
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
	/** Optional callback fired when (rows, cols) changes — wired to PTY resize.
	 *  `isAlt` is the kernel's alt-screen state at resize time; the backend
	 *  uses it to skip the ConPTY resize-silence window when an alt-screen
	 *  app (claude / vim / lazygit) is in the foreground (§1.24, 2026-05-06).
	 *  `isInlineTui` is the §A.3 heuristic snapshot — true when an Ink-style
	 *  app is rendering inline on primary (Claude Code's input box). The
	 *  backend treats it as another reason to skip the silence window so
	 *  the foreground app's SIGWINCH redraw lands promptly.
	 *  Returns a Promise so `fitPane` can await the backend's PTY resize
	 *  before narrowing the kernel grid — eliminates the in-flight byte
	 *  race that caused border characters to wrap on shrink. */
	resizeHandler?: (
		rows: number,
		cols: number,
		isAlt: boolean,
		isInlineTui: boolean,
	) => Promise<void> | void;
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
	/** Stable user-input anchor for the IME helper textarea (§1.27 fix).
	 *
	 *  Reading the *live* kernel cursor every time `compositionupdate`
	 *  fires is unsafe when an Ink-based CLI (Claude Code, lazygit, …) is
	 *  redrawing its frame: log-update walks the cursor up through every
	 *  previously-rendered row via `(\x1b[2K\x1b[1A)*N + \x1b[G` before
	 *  writing the new frame. If the user starts typing pinyin during one
	 *  of those walks, the helper teleports to the spinner row and its
	 *  opaque background covers the loading area.
	 *
	 *  Instead we snapshot the kernel cursor *after* each user-initiated
	 *  write (`handleKeyDown` / `paste` / `write`) on the next animation
	 *  frame — by then the shell has echoed the typed bytes and the
	 *  cursor sits at its real post-input position. Background PTY
	 *  output (spinner ticks) does NOT update this anchor.
	 *
	 *  `null` until the first user-initiated write. `RidgePane` falls back
	 *  to the live cursor in that case. */
	imeAnchor: { row: number; col: number } | null;
	/** rAF id for the pending anchor-capture frame. Coalesces multiple
	 *  rapid writes into a single capture: at most one rAF outstanding
	 *  per pane. Cleared by the rAF callback. */
	imeAnchorRaf: number | null;
	/** §4.3 Phase B: pane's rectangle on the host canvas in device
	 *  pixels. Set by `_recomputeViewport` whenever the splitter drag /
	 *  workspace resize / DPR change moves the container. Forwarded to
	 *  `entry.handle.setViewportOffset(x, y)` and (via
	 *  `entry.handle.resize(wCss, hCss, dpr)`) into the WebGPU pane
	 *  backend's `viewport: ScissorRect`.
	 *
	 *  Undefined for Canvas2D-backed panes (and for WebGPU panes before
	 *  the first `_recomputeViewport` runs). Host pane lookups treat
	 *  `undefined` the same as a zero-size rect — the pane is parked-by-
	 *  clip until JS computes a real viewport. */
	viewport?: { x: number; y: number; w: number; h: number };
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
	/** When set, the RAF loop is asleep; this timer is the next scheduled
	 *  wake-up (cursor-blink boundary or a 1s watchdog). Cleared and
	 *  fired by `wake()`. Independent of `rafHandle` — at any moment at
	 *  most ONE of `{rafHandle, idleTimer}` is non-null while panes are
	 *  attached. */
	private idleTimer: ReturnType<typeof setTimeout> | null = null;
	/** §4.3 Phase B: process-wide host canvas the WebGPU `SurfaceHost`
	 *  binds its swap chain to. Mounted in `+page.svelte` and registered
	 *  via `attachHost` exactly once per app lifetime. `null` until then,
	 *  or permanently `null` on Canvas2D-only builds / WebGPU adapter
	 *  miss. Per-pane Canvas2D panes ignore this field entirely — they
	 *  still create their own per-pane `<canvas>` inside their container.
	 */
	private hostCanvas: HTMLCanvasElement | null = null;
	/** §4.3 Phase B: handle to the shared swap chain. Drives
	 *  `beginFrame` / `endFrame` once per RAF tick (wrapping all WebGPU
	 *  pane renders), plus `resize` from the host-parent ResizeObserver
	 *  in `+page.svelte` and `invalidate` after detach / park /
	 *  splitter settle so departed-pane pixels don't linger. */
	private surfaceHost: SurfaceHostHandle | null = null;
	/** §4.3 Phase B: in-flight attachHost promise. Used so `attach()` /
	 *  `unpark()` can `await` host initialisation (driven by
	 *  `+page.svelte::onMount`) before deciding host vs Canvas2D mode.
	 *  Without this de-dup, the very first RidgePane to mount would
	 *  race ahead of attachHost and end up on the Canvas2D fallback
	 *  path even when WebGPU is fully available. Resolves (never
	 *  rejects) — `attachHost` swallows init errors internally and
	 *  leaves `surfaceHost` null when WebGPU isn't usable. */
	private attachHostPromise: Promise<void> | null = null;
	/** Document `visibilitychange` listener installed once on first pane
	 *  attach; removed on last detach. Hidden tabs throttle RAF anyway,
	 *  but waking on visibility-restore avoids a lag the first time the
	 *  user comes back. */
	private visibilityListener: (() => void) | null = null;

	private constructor(opts: ManagerOptions) {
		this.opts = opts;
	}

	static instance(opts?: ManagerOptions): TerminalManager {
		if (!TerminalManager._instance) {
			// WebGPU is the default backend (user feedback 2026-05-05).
			// `_makeHandle` runtime-detects whether the wasm bundle exposes
			// `newWithWebgpuFirst`; if it does, that path internally calls
			// `navigator.gpu.requestAdapter()` and falls back to Canvas2D
			// in Rust on adapter miss. If the wasm bundle was built without
			// the webgpu feature (a future Canvas-only profile), the `typeof
			// === 'function'` check skips the upgrade and goes straight to
			// Canvas2D. Either way, no opt-in build flag or storage gate.
			//
			// Escape hatch (debugging only):
			//   localStorage.RIDGE_WEBGPU = '0'; location.reload()
			let preferWebgpu = true;
			try {
				if (typeof localStorage !== 'undefined') {
					const v = localStorage.getItem('RIDGE_WEBGPU');
					if (v === '0' || v === 'false') preferWebgpu = false;
				}
			} catch {
				// LS denied (private mode / SSR) — keep the WebGPU default.
			}
			TerminalManager._instance = new TerminalManager(
				opts ?? {
					fontFamily:
						'"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", monospace',
					fontSizePx: 15,
					scrollbackLines: 2000,
					preferWebgpu,
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
	 * Construct a `RenderHandle` for `canvas`. When `opts.preferWebgpu`
	 * is true AND the wasm bundle exposes the async `newWithWebgpuFirst`
	 * static (i.e. it was built with `--features webgpu`), use that —
	 * it tries WebGPU first and falls back to Canvas2D on adapter miss
	 * inside Rust. Otherwise build a Canvas2D handle synchronously.
	 *
	 * Returns a Promise either way so `attach` / `unpark` can `await`
	 * uniformly. The Canvas2D-only path resolves on the same tick.
	 */
	private async _makeHandle(canvas: HTMLCanvasElement): Promise<RenderHandle> {
		// `RenderHandle.newWithWebgpuFirst` only exists when the wasm
		// bundle was built with `--features webgpu`. Detect via `typeof`;
		// no static type from `@ridge/term-wasm` declares it, so cast.
		const HandleCtor = RenderHandle as unknown as {
			newWithWebgpuFirst?: (c: HTMLCanvasElement) => Promise<RenderHandle>;
		};
		if (this.opts.preferWebgpu && typeof HandleCtor.newWithWebgpuFirst === 'function') {
			try {
				return await HandleCtor.newWithWebgpuFirst(canvas);
			} catch (err) {
				// WebGPU path failed catastrophically (rare — the Rust
				// constructor already retries Canvas2D inside on adapter
				// miss). Fall through to JS-side Canvas2D as a final
				// safety net so attach() never rejects.
				if (import.meta.env?.DEV) {
					console.warn('[ridge-term] newWithWebgpuFirst threw; falling back to Canvas2D', err);
				}
			}
		}
		return new RenderHandle(canvas);
	}

	/**
	 * §4.3 Phase B: bind a single `wgpu::Surface` to `canvas` so every
	 * WebGPU pane composites through one swap chain instead of one
	 * surface per pane. JS calls this once at app boot from
	 * `+page.svelte::onMount` AFTER the host canvas DOM is mounted.
	 *
	 * Idempotent: a second call on the same instance is a no-op (so a
	 * SvelteKit hot-module-reload re-running onMount can't double-init).
	 *
	 * Bails silently when the wasm bundle has no `SurfaceHostHandle`
	 * (Canvas2D-only build) or when WebGPU adapter / device acquisition
	 * fails inside the Rust `SurfaceHost::init`. In those cases per-pane
	 * Canvas2D continues to work — `attach()` falls back to creating a
	 * `<canvas>` inside each pane container.
	 */
	public attachHost(canvas: HTMLCanvasElement): Promise<void> {
		if (this.attachHostPromise) return this.attachHostPromise;
		if (this.surfaceHost) return Promise.resolve(); // already done
		this.attachHostPromise = (async () => {
			if (!this.wasmReady) await this.ready();
			// Runtime-check the symbol so canvas-only wasm bundles
			// (built with `--no-webgpu`) don't crash here. The static
			// import above resolves to `undefined` on bundles missing
			// the export, and wasm-bindgen sets the binding to
			// undefined accordingly.
			const SHHCtor = SurfaceHostHandle as unknown as
				| { init: (c: HTMLCanvasElement) => Promise<SurfaceHostHandle> }
				| undefined;
			if (!SHHCtor || typeof SHHCtor.init !== 'function') {
				if (import.meta.env?.DEV) {
					console.warn('[ridge-term] SurfaceHostHandle missing; bundle was built --no-webgpu');
				}
				return;
			}
			try {
				this.surfaceHost = await SHHCtor.init(canvas);
			} catch (err) {
				// Adapter miss / device-creation failure. Per-pane
				// Canvas2D path keeps working; just log so DevTools
				// shows why WebGPU didn't come up.
				console.warn('[ridge-term] SurfaceHost.init failed; per-pane Canvas2D will be used', err);
				return;
			}
			this.hostCanvas = canvas;
			this.resizeHost(); // initial swap-chain configure
		})();
		return this.attachHostPromise;
	}

	/**
	 * §4.3 Phase B: reconfigure the shared swap chain when the host
	 * canvas's parent (workspace content area) changes size — window
	 * resize, sidebar collapse, FileEditor toggle. Drives
	 * `surface.configure` once on the host, then walks every attached
	 * pane to recompute its host-canvas-relative scissor.
	 *
	 * Cheap on no-op (manager.ts + Rust side both short-circuit on
	 * unchanged dims), so spurious ResizeObserver fires are harmless.
	 *
	 * No-op when `surfaceHost` is null (Canvas2D-only deployment).
	 */
	public resizeHost(): void {
		if (!this.surfaceHost || !this.hostCanvas) return;
		const parent = this.hostCanvas.parentElement;
		if (!parent) return;
		const rect = parent.getBoundingClientRect();
		const dpr = window.devicePixelRatio || 1;
		const wCss = Math.max(1, Math.floor(rect.width));
		const hCss = Math.max(1, Math.floor(rect.height));
		const wDev = Math.max(1, Math.round(wCss * dpr));
		const hDev = Math.max(1, Math.round(hCss * dpr));
		// Keep the canvas element's intrinsic size in lockstep with the
		// surface configure so the GPU output isn't scaled.
		if (this.hostCanvas.width !== wDev) this.hostCanvas.width = wDev;
		if (this.hostCanvas.height !== hDev) this.hostCanvas.height = hDev;
		this.hostCanvas.style.width = `${wCss}px`;
		this.hostCanvas.style.height = `${hCss}px`;
		this.surfaceHost.resize(wCss, hCss, dpr);
		// Pane scissor rects on the host are recomputed in host-canvas
		// device-pixel coords — host size change shifts every pane's
		// (x, y) so we must redo them all.
		for (const entry of this.panes.values()) {
			if (!entry.parked) this._recomputeViewport(entry);
		}
		// New surface backing pixels are undefined — make sure the next
		// frame seeds bg before any LoadOp::Load can leak driver garbage.
		this.surfaceHost.invalidate();
		this.wake();
	}

	/**
	 * §4.3 Phase B: predicate. True when this entry is rendering through
	 * the shared SurfaceHost (WebGPU host mode); false when it owns its
	 * per-pane DOM `<canvas>` (Canvas2D fallback). Callers branch on
	 * this to know whether to read `entry.canvas` or `entry.container`
	 * for layout, and whether to call `setViewportOffset` /
	 * `surfaceHost.invalidate()`.
	 */
	private _isHostMode(entry: PaneEntry): boolean {
		return this.surfaceHost !== null && entry.canvas === this.hostCanvas;
	}

	/**
	 * §4.3 Phase B: parse `opts.theme.background` (CSS hex string) into
	 * a 4-byte RGBA Uint8Array for `surfaceHost.beginFrame`. Defaults to
	 * opaque black on missing / unparseable input — matches how
	 * `Theme::default_dark` initialises `bg` in Rust.
	 *
	 * Accepts `#rgb`, `#rrggbb`, `#rrggbbaa`. Whitespace + casing
	 * tolerated. Anything else falls back to `[0, 0, 0, 255]`.
	 */
	private _currentThemeBgRgba(): Uint8Array {
		const out = new Uint8Array([0, 0, 0, 255]);
		const raw = this.opts.theme?.background;
		if (typeof raw !== 'string') return out;
		const hex = raw.trim().replace(/^#/, '');
		const parseByte = (s: string) => {
			const n = parseInt(s, 16);
			return Number.isFinite(n) ? n & 0xff : 0;
		};
		if (hex.length === 3) {
			out[0] = parseByte(hex[0] + hex[0]);
			out[1] = parseByte(hex[1] + hex[1]);
			out[2] = parseByte(hex[2] + hex[2]);
			out[3] = 255;
		} else if (hex.length === 6) {
			out[0] = parseByte(hex.slice(0, 2));
			out[1] = parseByte(hex.slice(2, 4));
			out[2] = parseByte(hex.slice(4, 6));
			out[3] = 255;
		} else if (hex.length === 8) {
			out[0] = parseByte(hex.slice(0, 2));
			out[1] = parseByte(hex.slice(2, 4));
			out[2] = parseByte(hex.slice(4, 6));
			out[3] = parseByte(hex.slice(6, 8));
		}
		return out;
	}

	/**
	 * §4.3 Phase B: translate `entry.container`'s DOM bounding rect into
	 * a device-pixel scissor on the host canvas, push the (x, y) to the
	 * pane backend via `setViewportOffset`, and push the (w, h) via the
	 * existing `entry.handle.resize` (which the WebGPU backend now
	 * routes to `WebGpuPaneBackend::resize_surface` — a no-surface
	 * variant that just records the new size).
	 *
	 * Reads the container's content-box (rect minus computed padding)
	 * so the per-pane padding of `opts.paddingPx` correctly insets the
	 * scissor from the splitter / pane border. Without padding
	 * subtraction the scissor would extend over the gutter strip and
	 * the pane's bg color would visibly bleed past the visual gap.
	 *
	 * Clamped to the host canvas bounds: a pane dragged to zero width
	 * or off-canvas resolves to `{ w: 0, h: 0 }` and the host's
	 * `record_pane` skips it entirely (parked-by-clip).
	 *
	 * No-op for Canvas2D-mode panes (`entry.canvas !== this.hostCanvas`).
	 */
	private _recomputeViewport(entry: PaneEntry): void {
		if (!this.hostCanvas || !this._isHostMode(entry)) return;
		const cr = entry.container.getBoundingClientRect();
		const hr = this.hostCanvas.getBoundingClientRect();
		const cs = window.getComputedStyle(entry.container);
		const padL = parseFloat(cs.paddingLeft) || 0;
		const padT = parseFloat(cs.paddingTop) || 0;
		const padR = parseFloat(cs.paddingRight) || 0;
		const padB = parseFloat(cs.paddingBottom) || 0;
		const dpr = window.devicePixelRatio || 1;
		// Container's content-box, then host-canvas-relative.
		const cssX = cr.left - hr.left + padL;
		const cssY = cr.top - hr.top + padT;
		const cssW = Math.max(0, cr.width - padL - padR);
		const cssH = Math.max(0, cr.height - padT - padB);
		const xDev = Math.max(0, Math.round(cssX * dpr));
		const yDev = Math.max(0, Math.round(cssY * dpr));
		const hostWDev = this.hostCanvas.width;
		const hostHDev = this.hostCanvas.height;
		const wDev = Math.max(0, Math.min(hostWDev - xDev, Math.round(cssW * dpr)));
		const hDev = Math.max(0, Math.min(hostHDev - yDev, Math.round(cssH * dpr)));
		entry.viewport = { x: xDev, y: yDev, w: wDev, h: hDev };
		// Push offset (x, y) and size (w, h) separately. `setViewportOffset`
		// is cheap (just updates two u32 fields); `resize` triggers
		// kernel grid resize + force redraw, so we only call it when
		// dims actually changed (it short-circuits internally).
		const handle = entry.handle as unknown as {
			setViewportOffset?: (x: number, y: number) => void;
		};
		if (typeof handle.setViewportOffset === 'function') {
			handle.setViewportOffset(xDev, yDev);
		}
		entry.handle.resize(Math.round(cssW), Math.round(cssH), dpr);
	}

	/**
	 * Bind a pane to the manager. Creates a `<canvas>` child of `container`,
	 * spins up the wasm kernel/renderer, starts observing the container
	 * for resize events.
	 *
	 * Throws if the manager isn't ready (caller must `await ready()` first)
	 * or if `paneId` is already attached.
	 *
	 * Async because the optional WebGPU upgrade path (`opts.preferWebgpu`)
	 * needs to await the Rust adapter request. Canvas2D-only builds resolve
	 * on the same tick — call sites should still `await` to stay consistent.
	 */
	async attach(paneId: string, container: HTMLElement): Promise<void> {
		if (!this.wasmReady) {
			throw new Error('TerminalManager.attach: call ready() first');
		}
		if (this.panes.has(paneId)) {
			throw new Error(`TerminalManager.attach: pane ${paneId} already attached`);
		}
		// §4.3 Phase B: if `+page.svelte` kicked off attachHost just
		// before this RidgePane mounted, wait for it to settle so we
		// pick the correct host-vs-Canvas2D path (`useHost` below).
		if (this.attachHostPromise) {
			try { await this.attachHostPromise; } catch { /* ignore — handled inside attachHost */ }
		}

		// §4.3 Phase B: when the global SurfaceHost is alive, every WebGPU
		// pane composites through it — no per-pane DOM canvas needed.
		// Canvas2D fallback (no host, or `--no-webgpu` build) keeps the
		// legacy per-pane canvas path so its 2D context has a render
		// target.
		const useHost = this.surfaceHost !== null && this.opts.preferWebgpu;
		let canvas: HTMLCanvasElement;
		if (useHost && this.hostCanvas) {
			// Sentinel: store the host canvas reference so `_isHostMode`
			// can detect this entry. Per-pane DOM stays canvas-free —
			// the pane's container is a layout box only.
			canvas = this.hostCanvas;
			// §4.3 Phase B: RidgePane.svelte's container has
			// `background: var(--rg-term-bg)` so per-pane Canvas2D's
			// `<canvas>` child sat on a matching backdrop. In host mode
			// there is no per-pane canvas — the GPU draws into the host
			// canvas BEHIND the container, so an opaque container
			// background would hide every drawn pixel (the original
			// "black screen" symptom on the first Phase B build).
			// Override to transparent so the host canvas shows through.
			container.style.background = 'transparent';
		} else {
			canvas = document.createElement('canvas');
			canvas.style.cssText = 'display:block; width:100%; height:100%;';
			canvas.setAttribute('aria-hidden', 'true');
			container.appendChild(canvas);
		}

		// Apply initial padding to the container. In legacy mode this
		// inset the per-pane canvas; in host mode the pane's scissor
		// reads `getComputedStyle().padding*` (see `_recomputeViewport`)
		// to mirror the same visual inset on the host canvas.
		if (this.opts.paddingPx && this.opts.paddingPx > 0) {
			container.style.padding = `${this.opts.paddingPx}px`;
		}

		const handle = await this._makeHandle(canvas);
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
				this.wake();
				return;
			}
			// Multi-click: e.detail counts consecutive clicks within the
			// browser's double-click interval. Triple-click = full line,
			// double-click = word at cell. We do NOT enter drag mode for
			// these — a follow-up move shouldn't shrink/extend the multi-
			// click selection (matches xterm/iTerm behaviour).
			if (e.detail === 2) {
				ent.kernel.selectWordAt(cell.row, cell.col);
				this.wake();
				return;
			}
			if (e.detail >= 3) {
				ent.kernel.selectLineAt(cell.row);
				this.wake();
				return;
			}
			try { (e.target as Element | null)?.setPointerCapture?.(e.pointerId); } catch {}
			ent.selecting = true;
			ent.selectionStart = cell;
			// Empty range → kernel.setSelection.set treats as clear, which
			// is exactly what a single click should do (clear any prior
			// selection until the user actually drags).
			ent.kernel.setSelection(cell.row, cell.col, cell.row, cell.col);
			this.wake();
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
			this.wake();
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
			imeAnchor: null,
			imeAnchorRaf: null,
		};
		entry.resizeObserver.observe(container);

		this.panes.set(paneId, entry);
		// Initial fit: do it once synchronously after layout settles. We
		// wait one rAF (so SvelteKit hydration finishes), then fit
		// directly without debounce so the PTY gets sized before any
		// shell output arrives.
		requestAnimationFrame(() => {
			if (this.panes.has(paneId)) void this.fitPane(entry);
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
			// §4.3 Phase B: only Canvas2D-mode panes own a per-pane DOM
			// canvas to remove. Host-mode panes share the global host
			// canvas; tearing down their entry leaves the host canvas
			// alive but we ask the host to clear next frame so the
			// pane's last pixels don't outlive its slot.
			if (this._isHostMode(entry)) {
				this.surfaceHost?.invalidate();
			} else {
				try {
					entry.canvas.remove();
				} catch {
					/* canvas already detached */
				}
			}
			try { entry.handle.free(); } catch { /* ignore */ }
		}
		// §1.27: cancel any pending IME-anchor rAF before freeing the
		// kernel — the rAF body would otherwise call cursorRow() on a
		// freed kernel and crash.
		if (entry.imeAnchorRaf !== null) {
			cancelAnimationFrame(entry.imeAnchorRaf);
			entry.imeAnchorRaf = null;
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

		// §4.3 Phase B: same canvas-ownership branch as detach. Host
		// mode shares the global canvas (don't remove); Canvas2D mode
		// owns its per-pane DOM canvas and must clean up.
		if (this._isHostMode(entry)) {
			this.surfaceHost?.invalidate();
		} else {
			try { entry.canvas.remove(); } catch { /* already detached */ }
		}
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
	async unpark(paneId: string, container: HTMLElement): Promise<void> {
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
		// §4.3 Phase B: same await as attach. Mostly defensive — by
		// the time any pane is parked, attachHost has long settled.
		if (this.attachHostPromise) {
			try { await this.attachHostPromise; } catch { /* ignore */ }
		}

		// §4.3 Phase B: same host-vs-legacy branch as `attach`. Host
		// mode reuses the global host canvas (no per-pane DOM canvas);
		// Canvas2D fallback creates a fresh one inside the container.
		const useHost = this.surfaceHost !== null && this.opts.preferWebgpu;
		let canvas: HTMLCanvasElement;
		if (useHost && this.hostCanvas) {
			canvas = this.hostCanvas;
			// Same transparent override as attach — without this the
			// re-mounted RidgePane's container bg hides the host canvas.
			container.style.background = 'transparent';
		} else {
			canvas = document.createElement('canvas');
			canvas.style.cssText = 'display:block; width:100%; height:100%;';
			canvas.setAttribute('aria-hidden', 'true');
			container.appendChild(canvas);
		}

		if (this.opts.paddingPx && this.opts.paddingPx > 0) {
			container.style.padding = `${this.opts.paddingPx}px`;
		}

		const handle = await this._makeHandle(canvas);
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
		// §4.3 Phase B: a freshly-mounted pane region on the host canvas
		// may still hold pixels from whichever pane lived there before
		// the workspace switch (or whichever pane was parked from the
		// same slot). Force a global Clear next frame so we don't see
		// flickered stale content during the first fit.
		if (useHost && this.surfaceHost) {
			this.surfaceHost.invalidate();
		}
		entry.cellW = Number(cellW);
		entry.cellH = Number(cellH);
		// Force a resize-handler emit on the next fit so PTY rows/cols
		// resync — in particular if the new container has different
		// dimensions from the parked one.
		entry.lastReportedRows = -1;
		entry.lastReportedCols = -1;
		// Reset the padding cache: the fresh container has no inline
		// padding, but the cache still holds the pre-park value. Without
		// this, RidgePane's onMount setPadding(paneId, samePx) sees
		// `cached === clamped` and short-circuits — leaving the new
		// container at zero padding after every split / reparent.
		entry.lastAppliedPaddingPx = undefined;

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
			if (e && !e.parked) void this.fitPane(e);
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
		// §1.24 PTY trace (Phase 1.2): when `localStorage.RIDGE_PTY_TRACE === '1'`,
		// log every PTY-to-wasm byte chunk with a high-res timestamp so a live
		// resize-while-claude repro can be replayed in devtools to confirm
		// whether ConPTY's reflow noise leaks past the silence skip.
		if (typeof localStorage !== 'undefined' && localStorage.RIDGE_PTY_TRACE === '1') {
			const ts = performance.now().toFixed(1);
			const id = paneId.slice(0, 8);
			const hex = Array.from(bytes.slice(0, 256))
				.map((b) => b.toString(16).padStart(2, '0'))
				.join('');
			const more = bytes.length > 256 ? `…+${bytes.length - 256}B` : '';
			// eslint-disable-next-line no-console
			console.debug(`[pty-trace][${ts}ms][${id}][${bytes.length}B] ${hex}${more}`);
		}
		entry.kernel.feed(bytes);
		// PTY bytes mutated kernel state — wake the RAF loop if it was
		// sleeping. No-op when already running.
		this.wake();

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
		this.wake();
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
		if (bytes.length > 0) {
			entry.dataHandler(bytes);
			this.scheduleImeAnchorCapture(entry);
		}
	}

	/** Register a callback for (rows, cols) changes — wire to PTY resize.
	 *  The third arg is the kernel's alt-screen state at resize time
	 *  (§1.24, 2026-05-06); the backend uses it to skip ConPTY's resize-
	 *  silence window for alt-screen panes so the foreground TUI's
	 *  SIGWINCH-driven redraw isn't dropped. The fourth arg is the §A.3
	 *  inline-TUI heuristic — same skip-silence treatment for Ink-style
	 *  apps (Claude Code's input box) running on primary. The callback
	 *  may return a Promise; `fitPane` awaits it on plain primary so the
	 *  backend ConPTY resize completes before the kernel grid narrows. */
	onResize(
		paneId: string,
		cb: (
			rows: number,
			cols: number,
			isAlt: boolean,
			isInlineTui: boolean,
		) => Promise<void> | void,
	): void {
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
		this.scheduleImeAnchorCapture(entry);
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
		this.scheduleImeAnchorCapture(entry);
	}

	/** Programmatic select-all. */
	selectAll(paneId: string): void {
		this.panes.get(paneId)?.kernel.selectAll();
		this.wake();
	}

	/** Get currently selected text (empty string if no selection). */
	getSelectionText(paneId: string): string {
		return this.panes.get(paneId)?.kernel.getSelectionText() ?? '';
	}

	clearSelection(paneId: string): void {
		this.panes.get(paneId)?.kernel.clearSelection();
		this.wake();
	}

	/** Tell the wasm renderer whether this pane is the focused one. Only the
	 *  truly focused pane should blink its cursor; unfocused panes hide it
	 *  entirely. RidgePane wires this to the global `activePaneId` store so
	 *  switching panes flips the cursor visibility on both sides instantly. */
	setFocused(paneId: string, focused: boolean): void {
		this.panes.get(paneId)?.handle.setFocused(focused);
		// Cursor visibility changed → cursor row dirties → wake.
		this.wake();
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

	/** Schedule a single rAF that snapshots the kernel cursor as the new
	 *  IME anchor. Coalesces rapid writes — at most one outstanding rAF
	 *  per pane (`imeAnchorRaf` guard). The rAF gives the shell echo time
	 *  to land before we read, so the snapshot reflects the cursor's
	 *  *post-input* position rather than its position at the moment we
	 *  forwarded the bytes (which on Windows ConPTY can be one frame
	 *  behind the echo). See `PaneEntry.imeAnchor` doc-comment for the
	 *  motivating §1.27 bug. */
	private scheduleImeAnchorCapture(entry: PaneEntry): void {
		if (entry.parked) return;
		if (entry.imeAnchorRaf !== null) return;
		entry.imeAnchorRaf = requestAnimationFrame(() => {
			entry.imeAnchorRaf = null;
			if (entry.parked) return;
			entry.imeAnchor = {
				row: entry.kernel.cursorRow(),
				col: entry.kernel.cursorCol(),
			};
		});
	}

	/** Pixel position of the kernel cursor relative to the pane container's
	 *  top-left, plus the cell height (so callers can place a one-line
	 *  helper element BELOW the current cursor row). Returns null when
	 *  the pane is unknown or cell metrics aren't ready yet. */
	cursorPixelPosition(
		paneId: string,
	): { x: number; y: number; cellW: number; cellH: number; fontSizePx: number } | null {
		const e = this.panes.get(paneId);
		if (!e || e.cellW <= 0 || e.cellH <= 0) return null;
		const row = e.kernel.cursorRow();
		const col = e.kernel.cursorCol();
		return {
			x: Math.round(col * e.cellW),
			y: Math.round(row * e.cellH),
			cellW: e.cellW,
			cellH: e.cellH,
			fontSizePx: this.opts.fontSizePx,
		};
	}

	/** Pixel position of the IME helper anchor (§1.27 fix) — uses the
	 *  stable user-input snapshot (`PaneEntry.imeAnchor`) instead of the
	 *  live kernel cursor, so background PTY redraws (Ink/log-update
	 *  spinner walks) don't drag the helper.
	 *
	 *  §1.27-tail fallback chain when `imeAnchor` is null (user clicked
	 *  into a pane and started composing without typing any ASCII first):
	 *    1. `kernel.lastAbsCsiPosition()` if recent (≤ 2 s) — for an Ink-
	 *       style inline TUI, the LAST absolute-positioning CSI of any
	 *       frame parks the cursor at the input row, so this reflects
	 *       the Ink-stable input position even when the live cursor is
	 *       mid-walk in some intermediate spinner state.
	 *    2. Live `cursorPixelPosition` — for plain shells the live
	 *       cursor sits at end-of-prompt and is a fine anchor.
	 *  This avoids the live-cursor teleport bug for inline TUIs while
	 *  preserving correct behaviour for plain shells. */
	inputAnchorPixelPosition(
		paneId: string,
	): { x: number; y: number; cellW: number; cellH: number; fontSizePx: number } | null {
		const e = this.panes.get(paneId);
		if (!e || e.cellW <= 0 || e.cellH <= 0) return null;
		const rows = e.kernel.rows();
		const cols = e.kernel.cols();
		// §1.27-tail decay window — must match `INLINE_TUI_DECAY_MS` in
		// `packages/ridge-term/src/term/grid.rs` so a stale CSI from a
		// long-quiet shell (>2 s) isn't preferred over the live cursor.
		const ABS_CSI_DECAY_MS = 2_000;

		const pickAt = (row: number, col: number) => {
			const r = Math.min(row, Math.max(0, rows - 1));
			const c = Math.min(col, Math.max(0, cols - 1));
			return {
				x: Math.round(c * e.cellW),
				y: Math.round(r * e.cellH),
				cellW: e.cellW,
				cellH: e.cellH,
				fontSizePx: this.opts.fontSizePx,
			};
		};

		const anchor = e.imeAnchor;
		if (anchor) return pickAt(anchor.row, anchor.col);

		// Try lastAbsCsiPosition for inline-TUI scenarios where the live
		// cursor is unreliable. Defensive feature-detection so an older
		// wasm bundle without the method still falls through cleanly.
		const k = e.kernel as unknown as {
			lastAbsCsiPosition?: () => { row: number; col: number; atMs: number } | null;
		};
		if (typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi && Date.now() - csi.atMs < ABS_CSI_DECAY_MS) {
				return pickAt(csi.row, csi.col);
			}
		}
		return this.cursorPixelPosition(paneId);
	}

	/** Force a full-frame redraw on the next rAF tick (§1.27 fix). Used
	 *  by `RidgePane::onCompositionEnd` to repaint cells underneath the
	 *  IME helper textarea — without this, Canvas2D's per-row hash diff
	 *  may skip redrawing rows whose `cells` are unchanged but whose
	 *  pixels were smeared by the opaque `.is-composing` overlay. WebGPU
	 *  already redraws every row per tick, so this is a no-op there
	 *  beyond a single extra wake. */
	forceFullRedraw(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		entry.handle.invalidateAll();
		this.wake();
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
			void this.fitPane(entry);
		}
		this.wake();
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
		this.wake();
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
			void this.fitPane(e);
		}, 120);
	}

	private async fitPane(entry: PaneEntry): Promise<void> {
		// §4.3 Phase B: in host mode there is no per-pane canvas — the
		// entry.canvas reference is the shared host canvas, which spans
		// the whole workspace. Read the CONTAINER's content-box instead
		// so the cell grid matches the visible pane region.
		// Legacy Canvas2D mode keeps reading the per-pane canvas rect
		// (which is `width:100%; height:100%` inside the container, so
		// equivalent to the content-box).
		let wCss: number;
		let hCss: number;
		if (this._isHostMode(entry)) {
			const cr = entry.container.getBoundingClientRect();
			const cs = window.getComputedStyle(entry.container);
			const padL = parseFloat(cs.paddingLeft) || 0;
			const padT = parseFloat(cs.paddingTop) || 0;
			const padR = parseFloat(cs.paddingRight) || 0;
			const padB = parseFloat(cs.paddingBottom) || 0;
			wCss = Math.max(0, Math.floor(cr.width - padL - padR));
			hCss = Math.max(0, Math.floor(cr.height - padT - padB));
		} else {
			const rect = entry.canvas.getBoundingClientRect();
			wCss = Math.floor(rect.width);
			hCss = Math.floor(rect.height);
		}

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

		// Resize the render target. In host mode, _recomputeViewport
		// recomputes the host-canvas-relative scissor (which depends on
		// container x/y as well as w/h) AND calls entry.handle.resize
		// internally, so we dispatch to it. In Canvas2D mode, the per-
		// pane canvas owns its own size and handle.resize is sufficient.
		if (this._isHostMode(entry)) {
			this._recomputeViewport(entry);
		} else {
			entry.handle.resize(wCss, hCss, dpr);
		}

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

		// Critical ordering — depends on which screen is active.
		//
		// PRIMARY screen WITHOUT inline TUI (shell prompt at PSReadLine /
		// fish-zle / zsh-zle / cmd.exe): PTY first, then kernel.
		//   Shells emit absolute cursor positions (e.g. CSI 39;18 H).
		//   When the kernel resizes BEFORE the PTY knows about the new
		//   size, in-flight bytes (emitted under the OLD size) land on
		//   the new (smaller) grid and the cursor clamps to the new
		//   last row. We `await` the handler so the backend ConPTY
		//   resize completes before the kernel grid narrows — this
		//   eliminates the millisecond-scale in-flight byte race that
		//   used to be the dominant source of the "ghost characters
		//   past prompt end" symptom on shrink.
		//
		// ALT screen OR primary with inline TUI (Claude Code's Ink input
		// box, lazygit, vim, less, htop): kernel first, then PTY.
		//   The §1.22 alt wipe (or the §A.3 inline-TUI primary full
		//   wipe) inside `kernel.resize` must land BEFORE the foreground
		//   TUI receives SIGWINCH and begins its diff redraw. Otherwise
		//   the redraw bytes flow into the kernel while the visible
		//   region still holds OLD content; the subsequent wipe then
		//   erases the partial redraw, and Ink-style differential
		//   repaint never refills the gaps (it only updates cells that
		//   differ from its own model of the previous frame, so wiped
		//   cells stay blank). Switching the order makes the wipe land
		//   first, the PTY resize fire after the canvas is clean, and
		//   the SIGWINCH-driven redraw paints onto blanks every time.
		//
		// §1.24 / §A.3: `isAlt` AND `isInlineTui` snapshots both let the
		// backend skip the ConPTY resize-silence window so the foreground
		// app's redraw isn't dropped — see
		// src-tauri/src/commands/terminal.rs::resize_pane_inner.
		//
		// §1.25 (2026-05-06): the kernel itself never reflows on resize
		// (Grid::resize always uses naive truncate/pad on both screens),
		// so this ordering only governs the wipe vs. the TUI's own
		// redraw — there is no kernel-side rewrap that could race with
		// the application's repaint on either path.
		const isAlt = entry.kernel.isAltScreen();
		const isInlineTui = !isAlt && entry.kernel.isInlineTuiMode();
		const wipeBeforePty = isAlt || isInlineTui;
		if (wipeBeforePty) {
			entry.kernel.resize(rows, cols);
			await entry.resizeHandler?.(rows, cols, isAlt, isInlineTui);
		} else {
			await entry.resizeHandler?.(rows, cols, isAlt, isInlineTui);
			entry.kernel.resize(rows, cols);
		}

		// Synchronous first frame after resize. The next rAF tick is
		// up to ~16ms away, and a TUI like `claude` that's continuously
		// emitting bytes can land several PTY chunks in that window —
		// chunks that get parsed against the new grid size while the
		// renderer is still showing the previous frame's pixels.
		// Driving one frame here closes the gap so the very next visible
		// pixel reflects the new metrics + freshly-cleared atlas,
		// instead of a stale composite. Wrapped in try/catch so a
		// transient render error never blocks the resize from
		// completing — the rAF loop will pick it up next tick.
		try {
			entry.handle.render(entry.kernel);
		} catch (err) {
			console.error('[ridge-term] post-resize render error', entry.paneId, err);
		}
		// Resize may have fired while the loop was sleeping; even though
		// we drew one frame inline, subsequent SIGWINCH-driven redraw
		// bytes from the TUI need the loop awake to catch them.
		this.wake();
	}

	// ---- frame loop -------------------------------------------------

	private startRafLoop(): void {
		if (this.rafHandle !== null) return;
		// Install the visibility listener lazily on first start. Removed in
		// stopRafLoop so the singleton doesn't leak listeners between
		// detach-all → re-attach cycles.
		if (this.visibilityListener === null && typeof document !== 'undefined') {
			this.visibilityListener = () => {
				if (!document.hidden) this.wake();
			};
			document.addEventListener('visibilitychange', this.visibilityListener);
		}
		const tick = () => {
			this.rafHandle = null;
			const perfNow = performance.now();
			// Use Date.now() for the dirty / blink queries: `RenderHandle.render`
			// reads `js_sys::Date::now()` internally, so the renderer's blink
			// phase and our pre-render `isDirty` must use the same epoch.
			const dateNow = Date.now();
			let anyRendered = false;
			let minDeadlineMs = Infinity;
			// §4.3 Phase B (2026-05-08 fix): when ANY host-mode pane
			// reports dirty this tick, EVERY host-mode pane must render.
			// `SurfaceHost::begin_frame` always issues `LoadOp::Clear`
			// (multi-buffer swap-chain needs a deterministic seed each
			// frame), so a pane whose `isDirty` returned false would
			// have its scissor region wiped to bg without a redraw —
			// the visible "other panes flash blank when I type in one
			// pane" symptom.
			//
			// Pre-pass: cheap row-hash check per host pane. If any
			// dirty, set `forceHostRenderAll = true` and all host panes
			// re-encode below. Idle ticks (no host pane dirty) skip the
			// host frame entirely so RAF can sleep.
			let forceHostRenderAll = false;
			for (const entry of this.panes.values()) {
				if (entry.parked) continue;
				if (!this._isHostMode(entry)) continue;
				const handleAny = entry.handle as unknown as {
					isDirty?: (k: TerminalKernel, t: number) => boolean;
				};
				if (typeof handleAny.isDirty !== 'function') {
					forceHostRenderAll = true;
					break;
				}
				try {
					if (handleAny.isDirty(entry.kernel, dateNow)) {
						forceHostRenderAll = true;
						break;
					}
				} catch {
					forceHostRenderAll = true;
					break;
				}
			}
			let hostFrameOpen = false;
			const themeBg = this._currentThemeBgRgba();
			if (forceHostRenderAll && this.surfaceHost) {
				hostFrameOpen = this.surfaceHost.beginFrame(themeBg);
				// On surface lost, hostFrameOpen=false; host pane
				// renders below skip via the `hostFrameOpen` guard,
				// and the next RAF tick retries (begin_frame already
				// re-asserted needs_initial_clear=true on entry).
			}
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
					if (entry.syncStart === null) entry.syncStart = perfNow;
					const elapsed = perfNow - entry.syncStart;
					if (elapsed < SYNC_OUTPUT_TIMEOUT_MS) {
						// Hold the frame; schedule a wake at the timeout boundary.
						const remaining = SYNC_OUTPUT_TIMEOUT_MS - elapsed;
						if (remaining < minDeadlineMs) minDeadlineMs = remaining;
						continue;
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
				// Per-pane dirty check (Phase B 2026-05-07). When the wasm
				// bundle was built before the isDirty export shipped (older
				// pkg/), the property is undefined → fall back to render
				// unconditionally so we never freeze a pane on a downgrade.
				const handleAny = entry.handle as unknown as {
					isDirty?: (k: TerminalKernel, t: number) => boolean;
					nextBlinkDeadlineMs?: (k: TerminalKernel, t: number) => number;
				};
				let dirty = true;
				if (typeof handleAny.isDirty === 'function') {
					try {
						dirty = handleAny.isDirty(entry.kernel, dateNow);
					} catch {
						dirty = true;
					}
				}
				// §4.3 Phase B: host-mode panes render whenever the host
				// frame is open (so cleared regions get repainted).
				// Canvas2D-mode panes still gate on per-pane `dirty`.
				const shouldRender = this._isHostMode(entry)
					? hostFrameOpen
					: dirty;
				if (shouldRender) {
					try {
						entry.handle.render(entry.kernel);
						anyRendered = true;
					} catch (err) {
						// Don't let one pane's render error kill the whole loop.
						console.error('[ridge-term] render error', entry.paneId, err);
					}
				}
				if (typeof handleAny.nextBlinkDeadlineMs === 'function') {
					try {
						const d = handleAny.nextBlinkDeadlineMs(entry.kernel, dateNow);
						if (Number.isFinite(d) && d < minDeadlineMs) minDeadlineMs = d;
					} catch {
						// ignore — watchdog cap below covers us
					}
				}
			}
			// §4.3 Phase B: close the host frame if any pane drew.
			// `endFrame` finishes the encoder + queue.submit + present;
			// safe to skip when no pane drew (idle frame).
			if (hostFrameOpen && this.surfaceHost) {
				try {
					this.surfaceHost.endFrame();
				} catch (err) {
					console.error('[ridge-term] surfaceHost.endFrame error', err);
				}
			}
			if (this.panes.size === 0) return;
			if (anyRendered) {
				// Likely more work soon — stay on RAF cadence.
				this.rafHandle = requestAnimationFrame(tick);
				return;
			}
			// All idle. Sleep until the next blink boundary (or a 1s
			// watchdog so a missed wake-up path can't hang a pane longer
			// than that). Min 1ms keeps `setTimeout(0)` semantics off the
			// hot path.
			const sleepMs = Math.min(Math.max(minDeadlineMs, 1), 1000);
			this.idleTimer = setTimeout(() => {
				this.idleTimer = null;
				this.startRafLoop();
			}, sleepMs);
		};
		this.rafHandle = requestAnimationFrame(tick);
	}

	private stopRafLoop(): void {
		if (this.rafHandle !== null) {
			cancelAnimationFrame(this.rafHandle);
			this.rafHandle = null;
		}
		if (this.idleTimer !== null) {
			clearTimeout(this.idleTimer);
			this.idleTimer = null;
		}
		if (this.visibilityListener !== null && typeof document !== 'undefined') {
			document.removeEventListener('visibilitychange', this.visibilityListener);
			this.visibilityListener = null;
		}
	}

	/** Wake the RAF loop if it's currently asleep (idleTimer pending) or
	 *  not running at all. Idempotent — harmless to call from any state-
	 *  mutating path: `feed` (PTY bytes arrived), `setFocused` (cursor
	 *  visibility flip), theme/font/resize, selection drag, etc. Cheap
	 *  enough to call generously; the cost is one branch + (when sleep
	 *  is pending) one `clearTimeout` + one `requestAnimationFrame`. */
	private wake(): void {
		if (this.idleTimer !== null) {
			clearTimeout(this.idleTimer);
			this.idleTimer = null;
		}
		if (this.rafHandle === null && this.panes.size > 0) {
			this.startRafLoop();
		}
	}
}
