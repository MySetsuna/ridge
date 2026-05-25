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
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { settingsStore } from '../stores/settings';
import { workerRendererBridge, workerLifecycleOnFit } from './workerRendererBridge';
import { getWorkerRenderer, isWorkerRenderingEnabled } from './workerRendererSingleton';
import { perfMark } from './perfTrace';

// Quantize a CSS-px cell dimension to match the renderer's device-px
// rounding. webgpu.rs draw_row_backgrounds/draw_row_texts compute
//   cell_dev = round(cell_css * dpr)
// so the renderer's effective per-column width in CSS px is `cell_dev /
// dpr`. JS-side hover (computeCell) and grid fit (fitPane) must use the
// same effective value — otherwise the sub-pixel error per column
// accumulates and the rightmost cell ends up outside the JS coordinate
// range. With dpr=1 this is a no-op. (Bug: "resize 后无法选中最右一列".)
function quantizeCellSize(raw: number, dpr: number): number {
    if (!Number.isFinite(raw) || raw <= 0 || !Number.isFinite(dpr) || dpr <= 0) return raw;
    return Math.round(raw * dpr) / dpr;
}
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
import { LinkSpanIndex } from '$lib/terminal/linkSpans';
// §1.32 Wave F: PTY-prompt suffix snapshot — reads shell-input from
// kernel cells instead of mirroring keystrokes. See module docstring.
import { reconstructInputSnapshot } from '$lib/terminal/shellInputSnapshot';
import type { InputBufferState } from '$lib/components/inputBufferTracker';
// §1.32 (2026-05-20): `linkResolver` transitively imports `monaco-editor`
// via `$lib/stores/fileEditor → $lib/utils/markdown`. Keeping it as a
// static top-level import drags monaco into every consumer of `manager.ts`,
// which made `paneTree.test.ts` crash on the `window` reference inside
// monaco's `window.js` when running in Vitest's node env. The functions
// are only needed inside a click handler — lazy-import them at the use
// site (around line 1185 below) instead.
import { paneCwdStore } from '$lib/stores/paneTree';

/** §A.4 — concatenate two Uint8Arrays without allocating a JS array. Used
 *  by the inline-TUI feed coalescer to grow `entry.feedBuffer` across
 *  ConPTY split-write fragments before flushing once to the kernel. */
function concatU8(a: Uint8Array, b: Uint8Array): Uint8Array {
	const out = new Uint8Array(a.length + b.length);
	out.set(a, 0);
	out.set(b, a.length);
	return out;
}

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
	/** §A.8 — workspace this pane belongs to. Set at attach time so
	 *  RAF tick / resize / viewport recompute can find the right per-
	 *  workspace SurfaceHost. */
	workspaceId: string;
	container: HTMLElement;
	canvas: HTMLCanvasElement;
	kernel: TerminalKernel;
	/** §p4 ITER 1c (2026-05-22) — null when the worker-renderer path
	 *  owns the canvas (worker has its own RenderHandle inside the
	 *  DedicatedWorker after `transferControlToOffscreen`). Read sites
	 *  must use optional chaining or an explicit null guard. */
	handle: RenderHandle | null;
	cellW: number;
	cellH: number;
	/** dpr that was passed into the most recent `handle.configure()` call.
	 *  fitPane re-configures whenever this drifts from the live
	 *  window.devicePixelRatio — covers user dragging the window between
	 *  monitors of different DPI without resizing the pane otherwise.
	 *  Without this, cellW/cellH would keep their old-DPR quantisation
	 *  while the renderer silently re-rounds against the new DPR. */
	lastConfiguredDpr: number;
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
	 *  and pointerup; `selectionStartAbs` is the (row,col) where drag began. */
	selecting: boolean;
	selectionStartAbs: { row: number; col: number } | null;
	selectionEndAbs: { row: number; col: number } | null;
	/** TUI mouse forwarding hot-path state — rAF batching + (row, col,
	 *  buttons, action) dedup so a single drag doesn't fire 60-120 wasm
	 *  encodeMouse calls per second. xterm.js / kitty / wezterm all use
	 *  this pattern; without it, hover / drag / wheel feel laggy in TUIs
	 *  because the kernel can't drain PTY writes fast enough. */
	lastMouseSent: { row: number; col: number; buttons: number; action: number } | null;
	pendingMouseMove: PointerEvent | null;
	mouseMoveRaf: number | null;
    /** Drag-selection auto-scroll timer. Non-null while the pointer is
     *  parked in the top/bottom edge band during a drag — the tick
     *  scrolls one row in `autoScrollDirection` and re-anchors the
     *  selection's far end to the new edge row so the highlight grows
     *  with the revealed content (xterm.js / iTerm2 / kitty contract). */
    autoScrollTimer: ReturnType<typeof setInterval> | null;
    autoScrollDirection: 'up' | 'down' | null;
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
	/** Actual CSS `padding` value (px) most recently written to the pane
	 *  container by `fitPane` — distinct from `lastAppliedPaddingPx`
	 *  which is the user's base preference set via `setPadding`. fitPane
	 *  reads the user preference as `basePad` (a floor), then computes
	 *  `padAll = (container - cells × cellW) / 2` and writes that to
	 *  CSS so the cell grid sits centred inside the content box. Pixel
	 *  position calculations (`pickAt`, `computeCell`,
	 *  `inputAnchorPixelPosition`) MUST read `lastFitPaddingPx` to
	 *  align with the visible cursor — using `lastAppliedPaddingPx`
	 *  (the user's basePad) would be off by `padAll - basePad`,
	 *  visible as e.g. the IME helper textarea anchored a few px to
	 *  the left of the cursor. `undefined` until the first fitPane
	 *  runs. */
	lastFitPaddingPx?: number;
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
	/** §A.4 (2026-05-08) — pending PTY bytes held back briefly while the
	 *  kernel is in inline-TUI mode (Ink/log-update emitting walk + new
	 *  frame across multiple ConPTY reads). Without coalescing, a rAF
	 *  tick can sample the kernel between an EL-walk event and the new-
	 *  frame write event, painting a partial state Canvas2D doesn't fully
	 *  overwrite next frame → "wrong word" jitter on the spinner row.
	 *  Null when no buffer is pending. */
	feedBuffer: Uint8Array | null;
	/** §A.4 — outstanding flush timer for `feedBuffer`. Coalesces ConPTY
	 *  fragment bursts within 8 ms into one `kernel.feed` call. */
	feedFlushTimer: ReturnType<typeof setTimeout> | null;
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
	/** §4a workspace keep-alive (2026-05-08): set true by the RAF tick when
	 *  this pane's container has 0×0 bbox (display:none ancestor — its
	 *  workspace tab is not active). Tracking this lets the next visible
	 *  tick detect the hidden→visible transition and run an explicit
	 *  fitPane, in case the per-pane ResizeObserver missed the change
	 *  (some browsers don't fire RO for display:none → display:flex
	 *  transitions reliably). */
	wasHiddenLastTick?: boolean;
	/** 终端纯文本链接 / 路径检测器。OSC 8 hyperlinkAt 之外的兜底：识别
	 *  https://、file://、绝对 / 相对路径，配合 Ctrl+click 路由到 ridge
	 *  编辑器或系统资源管理器。lazy 重建：feed / scroll / resize 后置 dirty
	 *  标志，仅在 ctrl+pointermove 或 ctrl+pointerdown 时同步扫一次。 */
	linkSpans: LinkSpanIndex;
	/** P1.3 (2026-05-19): last (offset, total) pair we surfaced via
	 *  `scrollStateHandler`. The RAF tick diffs against this and emits
	 *  only on change, so an idle pane never wakes the subscriber.
	 *  Initialised to `-1` so the first registration / first RAF tick
	 *  always emits a baseline event. Replaces the per-pane 250ms
	 *  `setInterval` poll RidgePane was running (§1.23). */
	lastScrollOffset: number;
	lastScrollTotal: number;
	/** P1.3: optional callback fired (at most once per RAF tick) when
	 *  `kernel.scrollOffset()` or `kernel.scrollbackLen()` differ from
	 *  the cached pair above. Single-consumer like `eventHandler` /
	 *  `dataHandler`; a fresh `onScrollState` registration replaces
	 *  the previous one. Cleared on detach. */
	scrollStateHandler: ((state: { offset: number; total: number }) => void) | null;
	/** P2.1 (2026-05-20): bytes that `_feedNow` chunked-and-yielded
	 *  out of when the per-call time budget was exhausted, plus any
	 *  later arrivals that landed while this queue was non-empty (so
	 *  byte order is preserved). The RAF tick drains this at the
	 *  start of each frame before invoking the renderer. Heavy output
	 *  on one pane (think `pnpm tauri dev` compile waterfall) can no
	 *  longer block input echo / render on its sibling panes for tens
	 *  of milliseconds. `null` when no bytes are deferred — the steady
	 *  state for an idle pane. */
	feedDeferred: Uint8Array | null;
	/** §1.32 Wave F (2026-05-20): row/col where the user's current
	 *  shell input started. Captured the first time the user types a
	 *  printable / paste / Tab event after a fresh prompt, cleared on
	 *  Enter (the shell submits and prints a new prompt next).
	 *  `readShellInputSnapshot` reads the kernel cells from this
	 *  point to `cursorRow / cursorCol` to reconstruct the actual
	 *  shell-input string — bypassing the keystroke mirror entirely
	 *  and so immune to Tab completion / $VAR expansion / Ctrl+R /
	 *  vi-mode drift.
	 *  `null` means "no input observed yet at the current prompt". */
	inputStartRow: number | null;
	inputStartCol: number | null;
}

/** Maximum hold time for `?2026` synchronous output mode. xterm uses 150ms;
 *  matching keeps Ink/lazygit/bottom behaviour consistent across terminals. */
const SYNC_OUTPUT_TIMEOUT_MS = 150;

/** Trailing-edge debounce window for container resize. The pane only
 *  re-fits (scissor + kernel grid + PTY SIGWINCH) after the user has
 *  paused this long without sending a new `viewportChanged` event,
 *  OR after a global `pointerup` fires (whichever comes first).
 *  500 ms — short enough that mouse-paused-mid-drag settles feel
 *  responsive, long enough that incidental layout twitches don't
 *  trip a mid-drag re-fit. `pointerup` is the dominant trigger; this
 *  is just the safety net when the release is missed. */
const RESIZE_SETTLE_MS = 500;

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
	/** P4.6 Part B (2026-05-22) — paneIds that have been mirrored into
	 *  the render worker via `workerRendererBridge.attach(...)`. Only
	 *  populated when `window.__RIDGE_USE_WORKER` was on at the first
	 *  successful `fitPane` for the pane. Used to decide attach-vs-resize
	 *  on subsequent fits and to gate the per-frame delta mirror so we
	 *  don't spam `pane_not_initialized` errors after a mid-session
	 *  flag toggle. Cleared on `detach`. */
	private workerAttached = new Set<string>();
	private rafHandle: number | null = null;
	/** P2.2 (2026-05-20): id of the pane currently marked focused via
	 *  `setFocused(paneId, true)`. Used by the RAF tick to render the
	 *  focused pane FIRST each frame so its keystrokes / cursor blink
	 *  beat sibling panes' draws when the frame budget is tight. `null`
	 *  when no pane is focused (rare — usually one of the visible panes
	 *  carries the input focus). */
	private _focusedPaneId: string | null = null;
	/** Workspace id whose SplitContainer is currently `display:flex` (vs
	 *  `display:none`). Set by `onActiveWorkspaceChanged` whenever the UI
	 *  flips between workspace tabs. Used by `_isContainerHidden` to
	 *  short-circuit the per-RAF-tick `getBoundingClientRect()` call —
	 *  reading a DOM rect every tick was triggering ~63 ms of forced
	 *  reflows over a 5 s window in the perf trace, because Svelte
	 *  re-emits style updates on PTY output (cursor blink, scroll diff)
	 *  and the next layout query has to flush a fresh layout pass.
	 *
	 *  Comparing `entry.workspaceId === this._activeWorkspaceId` is a
	 *  plain string compare — no layout cost. `null` means "no workspace
	 *  has been declared active yet" (initial bootstrap window between
	 *  manager construction and the first `onActiveWorkspaceChanged`
	 *  call from +page.svelte); during that window
	 *  `_isContainerHidden` falls back to the bbox path so the very
	 *  first pane attach still renders. */
	private _activeWorkspaceId: string | null = null;
	/** P2.2: monotonic counter, bumped at the bottom of every RAF tick.
	 *  Used to rotate the order in which NON-focused panes are visited
	 *  for render so no single non-focused pane gets perpetually
	 *  starved at the tail of the order. `>>> 0` wrap keeps it bounded
	 *  to a u32 for the modulo arithmetic. */
	private _rafRotationIndex = 0;
	/** When set, the RAF loop is asleep; this timer is the next scheduled
	 *  wake-up (cursor-blink boundary or a 1s watchdog). Cleared and
	 *  fired by `wake()`. Independent of `rafHandle` — at any moment at
	 *  most ONE of `{rafHandle, idleTimer}` is non-null while panes are
	 *  attached. */
	private idleTimer: ReturnType<typeof setTimeout> | null = null;
	/** §A.9 (2026-05-08 follow-up) — single global host canvas, shared by
	 *  EVERY workspace's panes. Replaces the previous per-workspace
	 *  Map<wsId, {canvas, host}> design that forced a `surface.configure`
	 *  on every workspace switch (display:none → display:flex) and
	 *  produced visible black flashes while the swap chain reconfigured.
	 *
	 *  Single canvas means: pipeline + swap chain stay alive across
	 *  switches; switching workspaces is a CSS display flip that changes
	 *  which panes' container rects are non-zero, so `_recomputeViewport`
	 *  naturally drops scissors for inactive workspaces and the next RAF
	 *  paints the new active workspace into the existing surface — no
	 *  reconfigure, no clear, no black flash.
	 *
	 *  `null` until `attachHost(canvas)` lands at app boot. Once set, the
	 *  canvas/host pair is reused for the app lifetime. `detachHost()` is
	 *  only meaningful at shutdown / SSR teardown. */
	private globalHost: { canvas: HTMLCanvasElement; host: SurfaceHostHandle } | null = null;
	/** True between an `_invalidateHost()` call and the next RAF tick that
	 *  consumes it. The RAF idle-sleep gate uses this to decide whether
	 *  the upcoming tick is "real work" (a cache-replay pass over every
	 *  pane is required to refill the just-cleared swap chain) or "the
	 *  swap chain still holds the last presented frame, RAF can sleep
	 *  without painting anything". Without this flag, the loop opens a
	 *  host frame, runs `recordCachedOnly` for every visible pane, sets
	 *  `anyRendered = true`, and re-arms RAF — burning 60 fps of GPU
	 *  draw calls to repaint pixels identical to the last frame. With it,
	 *  steady-idle taps zero per-tick CPU and zero per-tick GPU work
	 *  between cursor-blink boundaries. */
	private _hostInvalidatePending: boolean = false;
	/** Mirror of the most recent `setPreedit` call per pane. RidgePane
	 *  writes the preedit overlay via `setPreedit(paneId, text, row, col)`;
	 *  the wasm side stores it but does not expose a getter, so we keep
	 *  this small JS-side mirror for E2E specs to assert that the overlay
	 *  cell matches the textarea cell + the kernel cursor. Cleared by
	 *  `clearPreedit`. */
	private _lastPreeditCall: Map<string, { row: number; col: number; text: string }> = new Map();
	/** §1.34 — JS-side mirror of the last `setHistoryOverlay` call per pane.
	 *  The wasm overlay state lives in `HistoryOverlay` (renderer.rs) and is
	 *  not exposed back to JS, so we keep this mirror purely for E2E specs
	 *  that previously inspected the (now-removed) DOM popup. Cleared by
	 *  `clearHistoryOverlay`. */
	private _lastHistoryOverlayCall: Map<string, {
		items: string[];
		selectedIndex: number;
		anchorRow: number;
		anchorCol: number;
		placeAbove: boolean;
	}> = new Map();
	/** In-flight `attachHost` init promise. Concurrent pane `attach()` /
	 *  `unpark()` calls await this so they don't race ahead of WebGPU
	 *  initialisation. Resolves (never rejects) — `attachHost` swallows
	 *  init errors internally and leaves `globalHost` null when WebGPU
	 *  isn't usable, falling back to per-pane Canvas2D for every pane. */
	private attachHostPromise: Promise<void> | null = null;
	/** Document `visibilitychange` listener installed once on first pane
	 *  attach; removed on last detach. Hidden tabs throttle RAF anyway,
	 *  but waking on visibility-restore avoids a lag the first time the
	 *  user comes back. */
	private visibilityListener: (() => void) | null = null;
	/** Document-level `pointerup` / `pointercancel` listener installed
	 *  lazily on first viewportChanged (= start of any drag session).
	 *  Triggers `_flushPendingFits`, so the moment the user releases the
	 *  mouse button the pending pane re-fits land immediately rather
	 *  than waiting out the trailing-edge `RESIZE_SETTLE_MS` window.
	 *  Removed in `stopRafLoop` to keep the singleton listener-clean
	 *  across detach-all → re-attach cycles. */
	private _resizeReleaseListener: (() => void) | null = null;

	private constructor(opts: ManagerOptions) {
		this.opts = opts;

		// Defensive `loadingdone` debounce. §4.6 used to bundle Noto
		// Color Emoji which fired ~10 `loadingdone` events as its
		// unicode-range subsets landed; without coalescing each event
		// invalidated the atlas, leaving WebGPU mid-frame with a mix
		// of pre-/post-invalidate glyphs and visibly "thick + ghost
		// echo" text. §A.7 dropped that webfont (system emoji fonts
		// are reliable; the bundled Noto failed to render via canvas
		// fillText on WebView2). The debounce stays so that any
		// future webfont addition can't reintroduce the storm.
		if (typeof document !== 'undefined' && 'fonts' in document) {
			let debounceTimer: ReturnType<typeof setTimeout> | null = null;
			document.fonts.addEventListener('loadingdone', () => {
				if (debounceTimer !== null) clearTimeout(debounceTimer);
				debounceTimer = setTimeout(() => {
					debounceTimer = null;
					this.invalidateAllPanes();
				}, 250);
			});
		}
	}

	/** Return the existing singleton without creating one. Used by
	 *  late-arriving callers (font loaders, theme watchers) that want
	 *  to invalidate panes only when the manager has actually spun up.
	 *  Returns null when no pane has attached yet — in which case the
	 *  next attach starts with a fresh atlas anyway. */
	static tryInstance(): TerminalManager | null {
		return TerminalManager._instance;
	}

	/** 终端链接路由器需要的 ctx：当前 pane 的 cwd（OSC 7 报告值）。 */
	static _currentPaneCwd(entry: PaneEntry): string | undefined {
		const map = get(paneCwdStore);
		return map[`${entry.workspaceId}:${entry.paneId}`];
	}

	/** 终端链接路由器需要的 ctx：所有 pane 当前 cwd 集合，用于"是否属于
	 *  任意 cwd 树"判断（多 workspace 多 pane 同时活跃时，落在任一 pane
	 *  CWD 内的文件都视为可在 ridge 编辑器打开）。 */
	static _knownCwds(): string[] {
		return Object.values(get(paneCwdStore)).filter((s): s is string => !!s);
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
					// Font-stack ordering: named monospace fonts first
					// (terminal text), then SYSTEM color-emoji fonts
					// (Apple / Segoe / system Noto), finally generic
					// `monospace` as the absolute fallback. The generic
					// family MUST stay last — per CSS, generic families
					// always match any codepoint (rendering .notdef
					// when the chosen font lacks the glyph), which
					// short-circuits the fallback chain. Putting
					// `monospace` earlier (e.g. between Consolas and
					// the emoji fonts) prevents the browser from ever
					// consulting the emoji fonts.
					//
					// §A.7 (2026-05-08): the @fontsource/noto-color-emoji
					// webfont was removed. WebView2 / Chromium failed to
					// render Noto's COLRv1 outlines via canvas fillText
					// (RIDGE_DIAG showed `non_zero_px=0` for every emoji
					// after the FontFace finished loading, even though
					// rendering with system Segoe UI Emoji worked before
					// Noto's unicode-range gate kicked in). System emoji
					// fonts (Segoe UI Emoji on Windows, Apple Color
					// Emoji on macOS, Noto Color Emoji where it's
					// installed system-wide on Linux) are reliable
					// across the runtimes we ship to and look identical
					// to the bundled Noto on Windows / macOS. "Noto
					// Color Emoji" stays in the chain as a SYSTEM font
					// lookup — harmless when not installed (the browser
					// just falls through), helpful on Linux distros
					// that ship it.
					fontFamily:
						'"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, "SimHei", "Heiti SC", "Microsoft YaHei", "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", monospace',
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
	private async _makeHandle(
		canvas: HTMLCanvasElement,
		surfaceHost?: SurfaceHostHandle,
	): Promise<RenderHandle> {
		const HandleCtor = RenderHandle as unknown as {
			newWithWebgpuFirst?: (
				c: HTMLCanvasElement,
				host?: SurfaceHostHandle,
			) => Promise<RenderHandle>;
		};
		if (this.opts.preferWebgpu && typeof HandleCtor.newWithWebgpuFirst === 'function') {
			try {
				// `newWithWebgpuFirst` consumes its `host` argument
				// (wasm-bindgen `Option<T>` moves the JS wrapper into
				// Rust and frees it on return). We need to keep the
				// manager's stored handle alive across multiple pane
				// attaches in the same workspace, so clone the JS
				// wrapper per call. The clone bumps the inner Rc
				// refcount; both wrappers share the same SurfaceHost.
				const hostArg =
					surfaceHost &&
					typeof (surfaceHost as unknown as { clone?: () => SurfaceHostHandle }).clone ===
						'function'
						? (surfaceHost as unknown as { clone: () => SurfaceHostHandle }).clone()
						: surfaceHost;
				return await HandleCtor.newWithWebgpuFirst(canvas, hostArg);
			} catch (err) {
				if (import.meta.env?.DEV) {
					console.warn('[ridge-term] newWithWebgpuFirst threw; falling back to Canvas2D', err);
				}
			}
		}
		return new RenderHandle(canvas);
	}

	/**
	 * §A.8 (2026-05-08) — bind one `wgpu::Surface` to `canvas` for the
	 * given workspace tab. Each workspace tab owns its own canvas so
	 * tab switching is just a CSS `display:flex/none` flip — the
	 * inactive tab's canvas keeps its last-painted pixels and the user
	 * sees no flash, no LoadOp::Clear, no re-rasterise.
	 *
	 * Idempotent per workspace: a second call for the same `workspaceId`
	 * is a no-op (so a SvelteKit HMR re-running mount can't double-init).
	 *
	 * Bails silently when the wasm bundle has no `SurfaceHostHandle`
	 * (Canvas2D-only build) or when WebGPU adapter / device acquisition
	 * fails. In those cases per-pane Canvas2D continues to work for the
	 * affected workspace — `attach()` falls back to creating a per-pane
	 * `<canvas>` inside each pane container.
	 */
	public attachHost(canvas: HTMLCanvasElement): Promise<void> {
		if (this.attachHostPromise) return this.attachHostPromise;
		if (this.globalHost) {
			// Re-attaching the SAME canvas is a no-op. Swapping to a
			// DIFFERENT canvas would require a full WebGPU surface
			// re-init — not supported in §A.9; the global canvas stays
			// for the app's lifetime.
			if (this.globalHost.canvas === canvas) return Promise.resolve();
			console.warn(
				'[ridge-term] attachHost called with a new canvas while one is already attached; ignoring',
			);
			return Promise.resolve();
		}
		const promise = (async () => {
			if (!this.wasmReady) await this.ready();
			const SHHCtor = SurfaceHostHandle as unknown as
				| { init: (c: HTMLCanvasElement) => Promise<SurfaceHostHandle> }
				| undefined;
			if (!SHHCtor || typeof SHHCtor.init !== 'function') {
				if (import.meta.env?.DEV) {
					console.warn(
						'[ridge-term] SurfaceHostHandle missing; bundle was built --no-webgpu',
					);
				}
				return;
			}
			let host: SurfaceHostHandle;
			try {
				host = await SHHCtor.init(canvas);
			} catch (err) {
				console.warn(
					'[ridge-term] SurfaceHost.init failed; per-pane Canvas2D will be used',
					err,
				);
				return;
			}
			this.globalHost = { canvas, host };
			this.resizeHost(); // initial swap-chain configure
		})();
		this.attachHostPromise = promise;
		return promise;
	}

	/** §A.9 — release the global SurfaceHost (only meaningful at app
	 *  shutdown / SSR teardown). All panes must be detached first;
	 *  surviving handles will no-op on render after the Rc drops. */
	public detachHost(): void {
		this.globalHost = null;
		this.attachHostPromise = null;
	}

	/** §A.9 — internal: global SurfaceHost lookup. The legacy
	 *  per-workspace `_hostFor(wsId)` API is gone; every pane shares
	 *  the same host now, so the wsId argument is meaningless. */
	private _globalHostHandle(): SurfaceHostHandle | null {
		return this.globalHost?.host ?? null;
	}

	/** Call `surfaceHost.invalidate()` AND mark `_hostInvalidatePending`
	 *  so the next RAF tick treats cache-replay passes as real work
	 *  (rather than letting the idle-sleep gate skip them and leave the
	 *  freshly-cleared swap chain blank). Every site that wipes the
	 *  shared canvas must go through here — direct
	 *  `_globalHostHandle()?.invalidate()` calls bypass the flag and
	 *  resurrect the "blank pane until next dirty event" symptom. */
	private _invalidateHost(): void {
		this._globalHostHandle()?.invalidate();
		this._hostInvalidatePending = true;
	}

	/** §A.9 — internal: global canvas lookup. */
	private _globalHostCanvas(): HTMLCanvasElement | null {
		return this.globalHost?.canvas ?? null;
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
	public resizeHost(dims?: { wCss: number; hCss: number }): void {
		const entry = this.globalHost;
		if (!entry) return;
		const { canvas, host } = entry;
		// Prefer dims passed in by a ResizeObserver callback (computed
		// from `entry.contentRect` — no layout query) over re-reading
		// `parent.getBoundingClientRect()`. The latter forces a sync
		// layout pass that the perf trace flagged at ~21 ms over a 5 s
		// window even though resizeHost itself only fires at most once
		// per RAF tick; Svelte's reactive style writes (cursor blink,
		// scroll diffs) invalidate layout between observer fires, so
		// each rect read pays the full reflow cost.
		let wCss: number;
		let hCss: number;
		if (dims) {
			if (dims.wCss <= 0 || dims.hCss <= 0) return;
			wCss = Math.max(1, Math.floor(dims.wCss));
			hCss = Math.max(1, Math.floor(dims.hCss));
		} else {
			const parent = canvas.parentElement;
			if (!parent) return;
			const rect = parent.getBoundingClientRect();
			// Defensive: parent may briefly measure 0×0 during initial mount
			// or while `display:none` is held by an ancestor. wgpu rejects
			// surface.configure(0, 0); skip and retry on the next observer
			// fire.
			if (rect.width <= 0 || rect.height <= 0) return;
			wCss = Math.max(1, Math.floor(rect.width));
			hCss = Math.max(1, Math.floor(rect.height));
		}
		const dpr = window.devicePixelRatio || 1;
		const wDev = Math.max(1, Math.round(wCss * dpr));
		const hDev = Math.max(1, Math.round(hCss * dpr));
		if (canvas.width === wDev && canvas.height === hDev) return;
		canvas.width = wDev;
		canvas.height = hDev;
		canvas.style.width = `${wCss}px`;
		canvas.style.height = `${hCss}px`;
		host.resize(wCss, hCss, dpr);
		for (const e of this.panes.values()) {
			if (e.parked) continue;
			this._recomputeViewport(e);
		}
		this._invalidateHost();
		this.wake();
	}

	/**
	 * §A.9 — call from the UI when `activeWorkspaceId` changes. With a
	 * shared global canvas we can't rely on canvas-level ResizeObserver
	 * to drive a redraw (the canvas itself doesn't resize on workspace
	 * switch). Instead, walk every newly-active pane, recompute its
	 * scissor against the (unchanged) host canvas, invalidate, and wake
	 * the RAF loop so the very next frame paints the new workspace.
	 *
	 * Inactive workspaces' panes naturally fall out via `_isContainerHidden`
	 * (their SplitContainer is `display:none`, container measures 0×0).
	 *
	 * No-op when WebGPU host isn't initialised (Canvas2D fallback path
	 * has no shared surface).
	 */
	public onActiveWorkspaceChanged(workspaceId: string): void {
		this._activeWorkspaceId = workspaceId;
		if (!this.globalHost) return;
		for (const e of this.panes.values()) {
			if (e.parked) continue;
			if (e.workspaceId !== workspaceId) continue;
			// Sync the host-canvas-relative scissor to the now-visible
			// pane container. Don't touch `wasHiddenLastTick` — the RAF
			// loop already sets it to true while the pane was hidden,
			// and §A.9 deliberately avoids the legacy "skip render this
			// tick" branch. Keeping the flag as-is means the next tick
			// runs a one-shot fitPane (idempotent if size unchanged) AND
			// renders this tick — no black flash, no missed kernel resize.
			this._recomputeViewport(e);
		}
		this._invalidateHost();
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
		const gh = this.globalHost;
		return gh !== null && entry.canvas === gh.canvas;
	}

	/**
	 * §4a workspace keep-alive (2026-05-08): true when the entry's pane
	 * container has 0 width or 0 height — the diagnostic for "this pane
	 * lives under a `display:none` ancestor (its workspace tab is not
	 * the active one)".
	 *
	 * Used by the RAF loop to skip render bookkeeping for hidden
	 * workspaces' panes. Their kernels keep being fed by PTY in the
	 * background (so scrollback / grid stays in sync), but no GPU work
	 * is paid for content the user can't see. On switch back to the
	 * workspace, the bbox returns non-zero next frame and isDirty=true
	 * fires a normal render — which is cheap because the RenderHandle
	 * stayed alive across the switch (no atlas re-warm, no Canvas re-
	 * mount).
	 *
	 * `getBoundingClientRect()` is cheap on a stable layout and the
	 * RAF loop runs at most 60 Hz, so the per-pane cost is negligible
	 * (~µs / pane / frame).
	 */
	private _isContainerHidden(entry: PaneEntry): boolean {
		// Fast path: when we know which workspace is active, a plain
		// string compare tells us if this pane lives under the visible
		// SplitContainer. Avoids the per-RAF-tick getBoundingClientRect
		// call that was burning ~63 ms of forced-reflow time over a
		// 5 s trace window (the worst hotspot in the perf insight).
		if (this._activeWorkspaceId !== null) {
			return entry.workspaceId !== this._activeWorkspaceId;
		}
		// Bootstrap fallback: no active workspace declared yet — fall
		// back to the layout-reading path so a pane attached before
		// the first `onActiveWorkspaceChanged` call still renders. This
		// branch is rare (only fires until +page.svelte's first reactive
		// dispatch lands, typically within one RAF after app mount).
		try {
			const rect = entry.container.getBoundingClientRect();
			return rect.width <= 0 || rect.height <= 0;
		} catch {
			return false;
		}
	}

	/**
	 * §4.3 Phase B: parse `opts.theme.background` (CSS hex string) into
	 * a 4-byte RGBA Uint8Array for `surfaceHost.beginFrame`. Defaults to
	 * opaque black on missing / unparseable input — matches how
	 * `Theme::default_dark` initialises `bg` in Rust.
	 *
	 * When the theme bridge has pushed a background via `setTheme`,
	 * `opts.theme.background` carries the `--rg-term-bg` value in
	 * `#RRGGBBAA` format (from `cssColor.ts::hex8`). By using it as
	 * the WebGPU clear color instead of transparent `[0,0,0,0]`, we
	 * ensure the global canvas always matches the terminal background.
	 * This prevents the page body's `--rg-bg` from showing through
	 * semi-transparent shell cells, which would appear as "unexpected
	 * black" when `--rg-bg` and `--rg-term-bg` visually differ.
	 *
	 * TUI panes paint their own opaque bg from Rust on top of this
	 * clear color, so TUI output is unaffected.
	 */
	private _currentThemeBgRgba(): Uint8Array {
		const bg = this.opts.theme?.background;
		if (bg && bg.length >= 7 && bg.startsWith('#')) {
			const r = parseInt(bg.slice(1, 3), 16);
			const g = parseInt(bg.slice(3, 5), 16);
			const b = parseInt(bg.slice(5, 7), 16);
			const a = bg.length >= 9 ? parseInt(bg.slice(7, 9), 16) : 255;
			if (!isNaN(r) && !isNaN(g) && !isNaN(b) && !isNaN(a)) {
				return new Uint8Array([r, g, b, a]);
			}
		}
		return new Uint8Array([0, 0, 0, 0]);
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
	 * No-op for Canvas2D-mode panes (per-pane DOM canvas, not host).
	 */
	private _recomputeViewport(entry: PaneEntry): void {
		const gh = this.globalHost;
		if (!gh || !this._isHostMode(entry)) return;
		const hostCanvas = gh.canvas;
		const cr = entry.container.getBoundingClientRect();
		// Hidden workspace tab → bbox 0×0 → degenerate scissor / kernel
		// resize. Skip; the next visible-tick ResizeObserver fire (or
		// the §A.8 host_canvas_rect that grows with the workspace
		// becoming visible) will redo this with the correct rect.
		if (cr.width <= 0 || cr.height <= 0) return;
		const hr = hostCanvas.getBoundingClientRect();
		const cs = window.getComputedStyle(entry.container);
		const padL = parseFloat(cs.paddingLeft) || 0;
		const padT = parseFloat(cs.paddingTop) || 0;
		const padR = parseFloat(cs.paddingRight) || 0;
		const padB = parseFloat(cs.paddingBottom) || 0;
		const dpr = window.devicePixelRatio || 1;
		const hostWDev = Math.round(hr.width * dpr);
		const hostHDev = Math.round(hr.height * dpr);
		let cssX = cr.left - hr.left + padL;
		let cssY = cr.top - hr.top + padT;
		let cssW = Math.max(0, cr.width - padL - padR);
		let cssH = Math.max(0, cr.height - padT - padB);
		// Shrink the scissor to cells-exact dimensions and re-center it
		// inside the content-box. Without this, `floor(cssH / cellH)`
		// leaves up to `cellH - 1` px of `term-bg` painted *below* the
		// last row inside the scissor — the user sees that as
		// "底部还有很多空余" because the renderer's bg fill is wider
		// than the actual rows. By collapsing the scissor to
		// `cellW*cols × cellH*rows`, the unused content-box area
		// reverts to whatever the host canvas was on (workspace bg,
		// since the canvas itself is transparent there), giving a tight
		// inset that visually matches the user's `paddingPx` setting.
		if (entry.cellW > 0 && entry.cellH > 0) {
			const cols = Math.max(1, Math.floor(cssW / entry.cellW));
			const rows = Math.max(1, Math.floor(cssH / entry.cellH));
			const cellsW = cols * entry.cellW;
			const cellsH = rows * entry.cellH;
			cssX += (cssW - cellsW) / 2;
			cssY += (cssH - cellsH) / 2;
			cssW = cellsW;
			cssH = cellsH;
		}
		// Add small epsilon to device-pixel width/height to avoid 1px
		// clipping on right/bottom edges due to sub-pixel rounding.
		const xDev = Math.max(0, Math.floor(cssX * dpr));
		const yDev = Math.max(0, Math.floor(cssY * dpr));
		const wDev = Math.max(0, Math.min(hostWDev - xDev, Math.ceil((cssX + cssW) * dpr) - xDev));
		const hDev = Math.max(0, Math.min(hostHDev - yDev, Math.ceil((cssY + cssH) * dpr) - yDev));
		entry.viewport = { x: xDev, y: yDev, w: wDev, h: hDev };

		// Push offset (x, y) and size (w, h) separately. `setViewportOffset`
		// is cheap (just updates two u32 fields); `resize` triggers
		// kernel grid resize + force redraw, so we only call it when
		// dims actually changed (it short-circuits internally).
		const handle = entry.handle;
		const handleVp = handle as unknown as {
			setViewportOffset?: (x: number, y: number) => void;
		} | null;
		if (handleVp !== null && typeof handleVp.setViewportOffset === 'function') {
			handleVp.setViewportOffset(xDev, yDev);
		}
		entry.handle?.resize(Math.round(cssW), Math.round(cssH), dpr);
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
	async attach(paneId: string, container: HTMLElement, workspaceId: string): Promise<void> {
		if (!this.wasmReady) {
			throw new Error('TerminalManager.attach: call ready() first');
		}
		if (this.panes.has(paneId)) {
			throw new Error(`TerminalManager.attach: pane ${paneId} already attached`);
		}
		// §A.9 — wait for the global SurfaceHost to settle (kicked off
		// by +page.svelte::onMount → manager.attachHost(canvas) which
		// races RidgePane mounts). Single global init now, so no
		// per-workspace lookup.
		if (this.attachHostPromise) {
			try { await this.attachHostPromise; } catch { /* attachHost handles errors internally */ }
		}

		const gh = this.globalHost;
		const useHost = gh !== null && this.opts.preferWebgpu;
		let canvas: HTMLCanvasElement;
		let hostHandle: SurfaceHostHandle | undefined;
		if (useHost && gh) {
			canvas = gh.canvas;
			hostHandle = gh.host;
			// Per-pane container must be transparent so the global
			// canvas (sitting BEHIND every workspace's SplitContainer
			// DOM tree) shows through. An opaque background would hide
			// every WebGPU pixel.
			container.style.background = 'transparent';
		} else {
			canvas = document.createElement('canvas');
			canvas.style.cssText = 'display:block; width:100%; height:100%; position:relative; z-index:0;';
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

		// §p4 ITER 1c-2 (2026-05-22) — when the worker-renderer path
		// owns the canvas, the main-thread `_makeHandle` is a no-op:
		// the worker has its own `RenderHandle::newFromOffscreen`
		// after the `transferControlToOffscreen` step below, and the
		// main thread never needs to call `render(...)` for this pane
		// again. Setting `handle = null` here makes the per-frame rAF
		// loop's `entry.handle?.render(...)` no-op and frees the main
		// thread from per-frame draw work entirely (the actual win of
		// the P4 ladder).
		//
		// Cell metrics fallback: without a main-thread handle we
		// cannot run `configure(...)` here, so we seed `entry.cellW /
		// cellH` from the wasm kernel's default 8 / 16 (matching the
		// `TerminalKernel::new(24, 80, ...)` seed below). The first
		// `fitPane` driven by the rAF after this attach will compute
		// real grid dimensions based on the container — slightly
		// wrong for one frame, then correct. Mouse-cell lookups
		// before that first fit briefly resolve to row 0 / col 0,
		// which is acceptable for an opt-in flag-gated path.
		const usingWorker = this.usingWorkerRenderer();
		const handle: RenderHandle | null = usingWorker
			? null
			: await this._makeHandle(canvas, hostHandle);
		const dpr = window.devicePixelRatio || 1;

		// configure() returns [cellW, cellH] in CSS pixels at the supplied DPR.
		// In worker-path mode `handle` is null — fall back to the kernel
		// seed dims (8 × 16); the first fitPane re-resolves real metrics.
		const [cellW, cellH] = handle
			? (handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as
					| [number, number]
					| Float32Array)
			: ([8, 16] as [number, number]);
		const cellWnum = quantizeCellSize(Number(cellW), dpr);
		const cellHnum = quantizeCellSize(Number(cellH), dpr);

		// §B.2 (2026-05-08) — read scrollback capacity from settings at
		// pane-attach time so the user's "终端 scrollback 行数" preference
		// (SettingsPanel slider, range 100..=10000) applies to every NEW
		// pane. Existing panes keep the capacity they were constructed
		// with (the wasm `Vec<Option<Row>>` is fixed-capacity). Falls
		// back to `this.opts.scrollbackLines` (constructor default 2000)
		// when the settings store hasn't been hydrated yet (SSR boot or
		// pre-first-attach).
		const settings = (() => {
			try {
				return get(settingsStore);
			} catch {
				return null;
			}
		})();
		const scrollbackLines =
			settings && Number.isFinite(settings.terminalScrollbackLines)
				? settings.terminalScrollbackLines
				: this.opts.scrollbackLines;
		// Seed kernel with default 24×80 — we'll resize to actual size right away.
		const kernel = new TerminalKernel(24, 80, scrollbackLines);

		// Apply theme if provided. Mirror the setTheme() pattern of
		// `applyDefaultTheme()` first so the kernel starts from a known
		// baseline before partial overrides land — otherwise any palette
		// entries not present in `opts.theme` retain whatever bits the
		// brand-new `Renderer::theme` ended up with, which has bitten us
		// when the wasm-side default doesn't match the bundled
		// `endless-dark` defaults (e.g. cursor color).
		if (this.opts.theme && handle) {
			handle.applyDefaultTheme();
			handle.applyTheme(this.opts.theme);
			if (typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_THEME_TRACE') === '1') {
				const t = this.opts.theme;
				// eslint-disable-next-line no-console
				console.debug(`[theme-trace] attach paneId=${paneId.slice(0,8)} bg=${t.background ?? '∅'} fg=${t.foreground ?? '∅'} cursor=${t.cursor ?? '∅'}`);
			}
		} else if (typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_THEME_TRACE') === '1') {
			// eslint-disable-next-line no-console
			console.debug(`[theme-trace] attach paneId=${paneId.slice(0,8)} ${handle ? 'NO_THEME (opts.theme is null — bridge hasn\'t fired yet)' : 'WORKER_PATH (theme applied by render worker)'}`);
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
			// §1.30: subtract container padding — canvas content starts at
			// `rect.top/left + pad`, not at the rect edge. See cellFromEvent
			// docstring for the full bug write-up.
			const pad = ent.lastFitPaddingPx ?? ent.lastAppliedPaddingPx ?? 0;
			const x = e.clientX - rect.left - pad;
			const y = e.clientY - rect.top - pad;
			const cols = ent.kernel.cols();
			const rows = ent.kernel.rows();
			if (cols === 0 || rows === 0) return null;
			const col = Math.max(0, Math.min(cols - 1, Math.floor(x / ent.cellW)));
			const row = Math.max(0, Math.min(rows - 1, Math.floor(y / ent.cellH)));
			return { row, col };
		};
		// JS-side mirror of selection.rs:22 — the abs-row encoding wasm
		// Selection uses is `0..sb_len` for scrollback rows and
		// `sb_len..sb_len+rows` for live grid rows, so the correct vp→abs
		// formula is `sb_len + vp - off`. A previous round of this code
		// used `vp + off`, which is only correct when sb_len = 0 — the
		// moment a pane accumulates any history (claude on first run),
		// stored abs landed below vp_first_abs and range_in_viewport
		// clipped the entire selection to None ("mouse selection
		// completely broken").
		const vpToAbsRow = (vpRow: number, kernel: TerminalKernel): number =>
			kernel.scrollbackLen() + vpRow - kernel.scrollOffset();
		// The custom scrollbar (RidgePane.svelte) renders as a sibling DOM
		// overlay on top of the canvas. Pointerdown on the track's empty
		// space has no element-local handler, so it bubbles into the
		// container and kicks off terminal selection — the user sees a
		// stray drag-select fire whenever they grab the scrollbar near
		// (but not on) the thumb. The thumb itself calls stopPropagation,
		// but a belt-and-suspenders target check at the listener entry
		// keeps thumb / track / and any future child of the scrollbar
		// fully isolated from selection/hover logic.
		const isInScrollbar = (e: PointerEvent): boolean => {
			const tgt = e.target as Element | null;
			return !!tgt?.closest?.('.rg-scrollbar-track, .rg-scrollbar-thumb');
		};
		// Mouse mode bitmask (kernel.mouseReportingModes()):
		//   bit 0 = ?1000 (normal), bit 1 = ?1002 (button-event / drag),
		//   bit 2 = ?1003 (any-event / motion), bit 3 = ?1006 (SGR).
		// One wasm call replaces 3 separate boolean getters on every
		// pointer event — a measurable saving at 60-120 Hz pointermove.
		const MOUSE_BTN_EVT = 0x2;
		const MOUSE_ANY_EVT = 0x4;
		const flushPointerMove = () => {
			const ent = this.panes.get(paneId);
			if (!ent) return;
			const pending = ent.pendingMouseMove;
			ent.pendingMouseMove = null;
			ent.mouseMoveRaf = null;
			if (!pending) return;

			const hoverCell = computeCell(pending);
			const modes = ent.kernel.mouseReportingModes();

			// ★ TUI mouse motion forwarding: when ?1002 (button-event /
			// drag) or ?1003 (any-event / all motion) is active, encode
			// and send each move to the application. No Alt escape hatch
			// — symmetric with pointerdown above (TUI takes priority for
			// every event, modifier-aware encoding still flows through).
			const isMouseMotion = (modes & (MOUSE_BTN_EVT | MOUSE_ANY_EVT)) !== 0;
			if (isMouseMotion && hoverCell) {
				// ?1003: forward ALL motion (no drag required).
				// ?1002: only forward while a mouse button is held —
				// read PointerEvent.buttons directly instead of relying
				// on `ent.selecting`, which is the host drag-select
				// flag. Conflating the two used to leak stale host
				// selection: a TUI mouse-mode pointerdown set
				// selecting=true to gate ?1002 motion, and if the TUI
				// then disabled mouse reporting (or exited back to
				// shell) mid-press, the next move would fall into the
				// host selection block with selecting still true and
				// `selectionStartAbs` carrying residue from a prior
				// host drag, silently extending that selection.
				if ((modes & MOUSE_ANY_EVT) !== 0 || pending.buttons !== 0) {
					const isMacUA = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
					const mod = pending.ctrlKey || (isMacUA && pending.metaKey);
					const btn = pending.buttons & 1 ? 0 : pending.buttons & 2 ? 2 : pending.buttons & 4 ? 1 : 0;
					const buttons = pending.buttons;
					const action = 2; // motion
					// Dedup: same cell + same buttons + same action → skip
					// the wasm encode + dataHandler. A single slow drag can
					// fire thousands of pointermoves within one cell; the
					// TUI only needs one motion per cell transition.
					const last = ent.lastMouseSent;
					if (
						!last ||
						last.row !== hoverCell.row ||
						last.col !== hoverCell.col ||
						last.buttons !== buttons ||
						last.action !== action
					) {
						const bytes = ent.kernel.encodeMouse(hoverCell.row, hoverCell.col, btn, action, pending.shiftKey, mod, pending.altKey);
						if (bytes.length > 0) {
							ent.dataHandler?.(bytes);
							ent.lastMouseSent = { row: hoverCell.row, col: hoverCell.col, buttons, action };
						}
					}
					return;
				}
			}

			// Ctrl-hover over an OSC 8 hyperlink → pointer cursor as
			// affordance. Any other state resets cursor (when ctrl is
			// released or pointer moves off a link). Round-trips don't
			// fire on bare key events so the user must wiggle the mouse
			// once after releasing/pressing Ctrl — minor; round 5 can
			// add keydown/keyup hooks if needed.
			const isMacUA2 = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
			const mod2 = pending.ctrlKey || (isMacUA2 && pending.metaKey);
			if (hoverCell && mod2) {
				const link = ent.kernel.hyperlinkAt(hoverCell.row, hoverCell.col);
				const span = link
					? null
					: ent.linkSpans.hitTest(ent.kernel, hoverCell.row, hoverCell.col);
				ent.container.style.cursor = link || span ? 'pointer' : '';
			} else if (ent.container.style.cursor === 'pointer') {
				ent.container.style.cursor = '';
			}

			// Continue with selection drag logic.
			if (!ent.selecting || !ent.selectionStartAbs || !hoverCell) return;
			ent.selectionEndAbs = { row: vpToAbsRow(hoverCell.row, ent.kernel), col: hoverCell.col };
			this._syncSelection(ent);
		};
		const pointerDownListener = (e: PointerEvent) => {
			if (isInScrollbar(e)) return;
			const cell = computeCell(e);
			if (!cell) return;
			const ent = this.panes.get(paneId);
			if (!ent) return;

			// Drop any pointermove that's still queued for the next rAF —
			// otherwise it fires AFTER this pointerdown's encoded press
			// reaches the TUI, and the TUI reads it as "the user pressed
			// at A then immediately dragged to A_prev", extending the
			// selection / visual range backwards by one frame's worth of
			// cursor history. Symptom: "selection starts from where the
			// cursor was a moment ago, not where I clicked."
			if (ent.mouseMoveRaf !== null) {
				cancelAnimationFrame(ent.mouseMoveRaf);
				ent.mouseMoveRaf = null;
			}
			ent.pendingMouseMove = null;

			const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
			const mod = e.ctrlKey || (isMac && e.metaKey);

			// ★ TUI mouse reporting takes absolute priority: when the
			// TUI app has enabled DEC mouse mode (?1000/?1002/?1003),
			// forward ALL button clicks (left, middle, right) to the
			// application. No modifier-key escape hatch — the user's
			// stated intent ("以 TUI 设置为准") is that an app which
			// asked for mouse events keeps them. To use host text
			// selection inside a TUI, the user disables mouse reporting
			// in the app (vim: `:set mouse=`, tmux: enter copy mode)
			// — the standard xterm contract. The Alt modifier is still
			// encoded into the SGR sequence (input.rs `encode_mouse` |8)
			// so the TUI can react to Alt+click in its own bindings.
			if (ent.kernel.mouseReportingModes() !== 0) {
				const btn = e.button; // 0=left, 1=middle, 2=right
				const bytes = ent.kernel.encodeMouse(cell.row, cell.col, btn, 0, e.shiftKey, mod, e.altKey);
				if (bytes.length > 0) {
					ent.dataHandler?.(bytes);
					// Deliberately do NOT set ent.selecting = true here.
					// `selecting` is the host drag-select state machine;
					// the ?1002 motion gate now reads PointerEvent.buttons
					// instead so this branch stays fully isolated from
					// host selection state — preventing residue leakage
					// across mid-press mouse-reporting changes.
					// Seed dedup baseline so the first motion in this cell is
					// suppressed (the TUI already knows the button is down here).
					ent.lastMouseSent = { row: cell.row, col: cell.col, buttons: e.buttons, action: 0 };
					try { (e.target as Element | null)?.setPointerCapture?.(e.pointerId); } catch {}
					return;
				}
			}

			// Only primary button (left) for selection. Right-click /
			// middle-click handled by context menu in RidgePane.
			if (e.button !== 0) return;

			// Ctrl/Cmd+click → if cell is inside an OSC 8 hyperlink span,
			// open it via the Tauri opener (or window.open as fallback).
			// Goes BEFORE selection branches so links beat selection on
			// modifier-click, matching iTerm/VSCode behaviour.
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
				// 纯文本路径 / URL 兜底（OSC 8 没标记的情况）：linkSpans
				// 命中即用统一的 resolveLink → executeAction 路由（CWD 内
				// 文件 → ridge 编辑器；外链 → 系统浏览器；外部路径/目录
				// → 系统资源管理器）。
				const span = ent.linkSpans.hitTest(ent.kernel, cell.row, cell.col);
				if (span) {
					const cwd = TerminalManager._currentPaneCwd(ent);
					const known = TerminalManager._knownCwds();
					const spanText = span.text;
					// §1.32: dynamic import keeps linkResolver (and its
					// transitive monaco-editor dependency) out of this
					// module's load graph. Click handlers tolerate the
					// extra microtask; tests in node env no longer crash.
					void import('$lib/utils/linkResolver').then(({ resolveLink, executeAction }) => {
						const action = resolveLink(spanText, { cwd, knownCwds: known });
						void executeAction(action);
					});
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
			if (e.shiftKey && ent.selectionStartAbs) {
				try { (e.target as Element | null)?.setPointerCapture?.(e.pointerId); } catch {}
				ent.selecting = true;
				const absEndRow = vpToAbsRow(cell.row, ent.kernel);
				ent.selectionEndAbs = { row: absEndRow, col: cell.col };
				ent.kernel.setSelectionAbs(
					ent.selectionStartAbs.row, ent.selectionStartAbs.col,
					absEndRow, cell.col,
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
			const absRow = vpToAbsRow(cell.row, ent.kernel);
			ent.selectionStartAbs = { row: absRow, col: cell.col };
			ent.selectionEndAbs = { row: absRow, col: cell.col };
			ent.kernel.setSelectionAbs(absRow, cell.col, absRow, cell.col);
			this.wake();
		};
		// pointermove is batched on requestAnimationFrame so a single
		// drag can't fire 60-120 wasm encodeMouse calls per second. The
		// flushPointerMove helper above does the actual work + cell-dedup
		// when the rAF tick runs; here we just record the latest event
		// and schedule one tick if none is queued. Last-event-wins is fine
		// (TUIs only react to the current cursor position, not
		// intermediate samples).
		// Drag-selection auto-scroll: when the user holds the left button
		// and drags past the viewport's top/bottom edge during a host
		// selection, scroll one row in that direction at a fixed rate
		// and re-pin the selection's moving end to the freshly-revealed
		// edge row. Without this the drag stalls at the viewport limit
		// even though the scrollback content the user wants to select
		// sits one row away. Same contract as xterm.js / iTerm2 / kitty.
		const AUTO_SCROLL_EDGE_PX = 24;
		const AUTO_SCROLL_INTERVAL_MS = 30;
		const stopAutoScroll = (ent: PaneEntry) => {
			if (ent.autoScrollTimer !== null) {
				clearInterval(ent.autoScrollTimer);
				ent.autoScrollTimer = null;
			}
			ent.autoScrollDirection = null;
		};
		const updateAutoScrollFromEdge = (ent: PaneEntry, e: PointerEvent) => {
			// Only auto-scroll during an active host drag-select. TUI
			// mouse-reporting paths and idle hover don't trigger it.
			if (!ent.selecting || !ent.selectionStartAbs) { stopAutoScroll(ent); return; }
			const rect = ent.container.getBoundingClientRect();
			const y = e.clientY - rect.top;
			const dir: 'up' | 'down' | null =
				y < AUTO_SCROLL_EDGE_PX ? 'up'
				: y > rect.height - AUTO_SCROLL_EDGE_PX ? 'down'
				: null;
			if (dir === null) { stopAutoScroll(ent); return; }
			// Same direction already ticking → keep going. Direction
			// flipped → reset so the new tick fires immediately rather
			// than waiting out the old interval.
			if (ent.autoScrollTimer !== null && ent.autoScrollDirection === dir) return;
			if (ent.autoScrollTimer !== null) clearInterval(ent.autoScrollTimer);
			ent.autoScrollDirection = dir;
			ent.autoScrollTimer = setInterval(() => {
				const cur = this.panes.get(paneId);
				if (!cur || !cur.selecting || !cur.selectionStartAbs) {
					if (cur) stopAutoScroll(cur);
					return;
				}
				if (dir === 'up') this.scrollUp(paneId, 1);
				else this.scrollDown(paneId, 1);
				const rowsCount = cur.kernel.rows();
				const colsCount = cur.kernel.cols();
				if (rowsCount === 0 || colsCount === 0) return;
				// Re-pin selection end to the new edge row. Use the last
				// pending pointer event's X for the column (the user's
				// hand may still be hovering off-edge after the initial
				// crossing) and the just-revealed top/bottom row in vp
				// coords. Convert to abs via *current* scroll_offset —
				// already shifted by the scrollUp/Down call above.
				const lastEvt = cur.pendingMouseMove ?? e;
				const r2 = cur.container.getBoundingClientRect();
				const xCol = Math.max(0, Math.min(colsCount - 1,
					Math.floor((lastEvt.clientX - r2.left) / cur.cellW)));
				const vpRow = dir === 'up' ? 0 : rowsCount - 1;
				const absRow = vpToAbsRow(vpRow, cur.kernel);
				cur.selectionEndAbs = { row: absRow, col: xCol };
				this._syncSelection(cur);
			}, AUTO_SCROLL_INTERVAL_MS);
		};
		const pointerMoveListener = (e: PointerEvent) => {
			if (isInScrollbar(e)) return;
			const ent = this.panes.get(paneId);
			if (!ent) return;
			ent.pendingMouseMove = e;
			if (ent.mouseMoveRaf == null) {
				ent.mouseMoveRaf = requestAnimationFrame(flushPointerMove);
			}
			// Edge auto-scroll runs synchronously off the raw move event
			// — coupling it to the rAF tick would make the initial
			// cross-into-edge feel laggy by up to 16ms.
			updateAutoScrollFromEdge(ent, e);
		};
		const pointerUpListener = (e: PointerEvent) => {
			if (isInScrollbar(e)) return;
			const ent = this.panes.get(paneId);
			if (!ent) return;

			// ★ TUI mouse release forwarding: send button release event
			// when mouse reporting is active, so the TUI app doesn't
			// get stuck in a pressed state.
			if (ent.kernel.mouseReportingModes() !== 0) {
				const cell = computeCell(e);
				if (cell) {
					const isMacUA = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
					const ctrl = e.ctrlKey || (isMacUA && e.metaKey);
					const bytes = ent.kernel.encodeMouse(cell.row, cell.col, 3, 1, e.shiftKey, ctrl, e.altKey);
					// btn=3=release, action=1=release → ESC [ <3 ; row ; col m
					if (bytes.length > 0) {
						ent.dataHandler?.(bytes);
					}
				}
			}

			ent.selecting = false;
			// Release ends the press; the next motion may come with no
			// button held → reset dedup baseline so the first such motion
			// (button=0) is not suppressed against the press baseline.
			ent.lastMouseSent = null;
			// Drag is done — kill any auto-scroll ticker that may be
			// running because pointer-up arrived while the cursor still
			// sat in the edge band.
			stopAutoScroll(ent);
			try { (e.target as Element | null)?.releasePointerCapture?.(e.pointerId); } catch {}
		};
		container.addEventListener('pointerdown', pointerDownListener);
		container.addEventListener('pointermove', pointerMoveListener);
		container.addEventListener('pointerup', pointerUpListener);

		const entry: PaneEntry = {
			paneId,
			workspaceId,
			container,
			canvas,
			kernel,
			handle,
			cellW: cellWnum,
			cellH: cellHnum,
			lastConfiguredDpr: dpr,
			resizeObserver: new ResizeObserver(() => this.viewportChanged(paneId)),
			lastReportedRows: -1,
			lastReportedCols: -1,
			pendingFitTimer: null,
			syncStart: null,
			syncTimeoutRendered: false,
			focusListener,
			blurListener,
			selecting: false,
			selectionStartAbs: null,
			selectionEndAbs: null,
			lastMouseSent: null,
			pendingMouseMove: null,
			mouseMoveRaf: null,
			autoScrollTimer: null,
			autoScrollDirection: null,
			pointerDownListener,
			pointerMoveListener,
			pointerUpListener,
			parked: false,
			imeAnchor: null,
			imeAnchorRaf: null,
			feedBuffer: null,
			feedFlushTimer: null,
			linkSpans: new LinkSpanIndex(),
			lastScrollOffset: -1,
			lastScrollTotal: -1,
			scrollStateHandler: null,
			feedDeferred: null,
			inputStartRow: null,
			inputStartCol: null,
		};
		entry.resizeObserver.observe(container);

		this.panes.set(paneId, entry);

		// §p4 ITER 1b/1d (2026-05-22) — worker-renderer hand-off.
		// When the worker-renderer flag is on AND the worker singleton
		// is alive, transfer the canvas to the worker via
		// `transferControlToOffscreen()` and post `bindCanvas` so the
		// worker's per-pane `RenderHandle` (newFromOffscreen) can paint
		// it directly. After ITER 1c-2 the main-thread `_makeHandle`
		// is skipped (entry.handle === null) when this branch fires,
		// so there's no detached-canvas render attempt. Fire-and-forget
		// bindCanvas — a worker hiccup must not block pane attach.
		//
		// ITER 1d: also set `pointerEvents: 'none'` on the (now
		// transferred) canvas so the pane container's existing
		// pointer/keyboard handlers continue to receive events even
		// after the canvas detaches. Without this, browsers can
		// route pointer events to the canvas element first; once
		// detached its event behavior is undefined and we'd lose
		// pointer capture during mouse-mode TUI rendering.
		if (usingWorker && typeof canvas.transferControlToOffscreen === 'function') {
			try {
				canvas.style.pointerEvents = 'none';
				const offscreen = canvas.transferControlToOffscreen();
				const wr = getWorkerRenderer();
				if (wr) {
					// §p4 ITER 5 (2026-05-22) — pass measure args so the
					// worker can `configure()` the new RenderHandle and
					// return real cell metrics in the `ready` response.
					// On success we update `entry.cellW / cellH` (still
					// quantized to dev-pixel grid) and trigger a fit so
					// the first visible frame sees the right rows/cols.
					wr.bindCanvas(paneId, offscreen, {
						font: this.opts.fontFamily,
						fontSizePx: this.opts.fontSizePx,
						dpr,
					})
						.then((response) => {
							if (
								response.type === 'ready' &&
								typeof response.cellW === 'number' &&
								typeof response.cellH === 'number' &&
								response.cellW > 0 &&
								response.cellH > 0
							) {
								const ent = this.panes.get(paneId);
								if (!ent) return;
								ent.cellW = quantizeCellSize(response.cellW, dpr);
								ent.cellH = quantizeCellSize(response.cellH, dpr);
								ent.lastConfiguredDpr = dpr;
								this.fitPaneNow(paneId);
							}
						})
						.catch((err) => {
							if (import.meta.env?.DEV) {
								// eslint-disable-next-line no-console
								console.warn('[ridge-term] worker bindCanvas rejected', err);
							}
						});
				}
			} catch (err) {
				if (import.meta.env?.DEV) {
					// eslint-disable-next-line no-console
					console.warn('[ridge-term] transferControlToOffscreen failed', err);
				}
			}
		}

		// Initial fit: do it once synchronously after layout settles. We
		// wait one rAF (so SvelteKit hydration finishes), then fit
		// directly without debounce so the PTY gets sized before any
		// shell output arrives.
		requestAnimationFrame(() => {
			if (this.panes.has(paneId)) void this.fitPane(entry);
		});
		// Expose a debug-dump entry point on `window` so we can inspect
		// what characters a TUI actually wrote into a row from DevTools
		// console — no module import required. Read-only beyond a brief
		// selection state mutation that the dump path itself clears.
		if (typeof window !== 'undefined') {
			(window as unknown as { __windDumpRows?: TerminalManager['debugDumpRows'] }).__windDumpRows =
				(pId: string, from: number, to: number) => this.debugDumpRows(pId, from, to);
			// P3.14 (2026-05-20) — e2e harness hook. The tauri-driver +
			// WebdriverIO suite (tests/e2e-shell/) needs an in-process
			// way to (a) feed PTY bytes into a pane and (b) inspect the
			// resulting visible grid without going through a real shell
			// (which would be flaky and platform-specific). Expose two
			// small helpers on window so the WebDriver client can
			// `executeAsync` them.
			//
			// Pure read-only / pass-through over the public manager API;
			// no production code path consults `__windE2E`. Gating on a
			// URL flag (`?e2e=1`) here is unnecessary because both
			// helpers are no-ops when called against a non-existent
			// paneId. The names follow the existing `__windDumpRows`
			// convention to keep the dev surface coherent.
			(window as unknown as {
				__windE2E?: {
					feedPty: (paneId: string, data: string) => void;
					writePty: (paneId: string, data: string) => Promise<void>;
					visibleText: (paneId: string) => string[];
					rows: (paneId: string) => number;
					cols: (paneId: string) => number;
					scrollbackLen: (paneId: string) => number;
					themeSnapshot: () => Record<string, string> | null;
					kernelCursor: (paneId: string) => { row: number; col: number } | null;
					kernelThemeProbe: (paneId: string) =>
						| { bg: string; fg: string; cursor: string; tuiBg: string }
						| { error: string }
						| null;
					setTheme: (theme: Record<string, string>) => void;
					sampleHostPixel: (
						relX?: number,
						relY?: number,
					) => { r: number; g: number; b: number; a: number } | null;
					inputAnchorResolved: (paneId: string) =>
						| { row: number; col: number; x: number; y: number; cellW: number; cellH: number; fontSizePx: number }
						| null;
					lastPreeditCall: (paneId: string) =>
						| { row: number; col: number; text: string }
						| null;
					/** §1.33 / §P5.IME — snapshot the kernel's live DEC-private
					 *  mode bits so e2e specs can prove they landed where
					 *  intended (`?1049h`/`?1h`/`?1000h`/`?25l` etc.) before
					 *  asserting on the popup gate or the IME-anchor follow.
					 *  Pure read of wasm-side getters; never touches state. */
					kernelDecState: (paneId: string) =>
						| {
								isAltScreen: boolean;
								isAppCursorKeys: boolean;
								isCursorVisible: boolean;
								isInlineTuiMode: boolean;
								mouseReportingModes: number;
						  }
						| null;
					/** §1.34 — wasm shell-history overlay state. Mirror of the
					 *  most-recent `setHistoryOverlay` call. Replaces DOM
					 *  `.rg-history-popup` querying after the wasm migration. */
					historyOverlayState: (paneId: string) => {
						open: boolean;
						items: string[];
						selectedIndex: number;
						anchorRow: number;
						anchorCol: number;
						placeAbove: boolean;
					};
					/** §1.34 perf harness — drive the overlay directly from a
					 *  spec without going through Svelte's onkeydown chain, so
					 *  the timing reflects ONLY the wasm + JS-mirror cost. */
					setHistoryOverlay: (
						paneId: string,
						items: string[],
						selectedIndex: number,
						anchorRow: number,
						anchorCol: number,
						placeAbove: boolean,
					) => void;
					clearHistoryOverlay: (paneId: string) => void;
					setSelectionAbs: (
						paneId: string,
						startAbsRow: number,
						startCol: number,
						endAbsRow: number,
						endCol: number,
					) => void;
					getSelectionText: (paneId: string) => string;
					hasSelection: (paneId: string) => boolean;
					applyDeltaFrameRaw: (paneId: string, bytes: Uint8Array) => void;
					encodeCursorDeltaFrame: (
						paneId: string,
						seq: number,
						row: number,
						col: number,
					) => Uint8Array | null;
					installPtyWriteSpy: (paneId: string) => void;
					ptyWriteLog: (paneId: string) => Array<{ data: string; at: number }>;
					clearPtyWriteLog: (paneId: string) => void;
					/** P4.6 Part B (Iter 17, 2026-05-22) — diagnostic
					 *  surface for the render-worker mirror. Lets e2e
					 *  specs verify the worker actually spun up and is
					 *  keeping up with messages. `active` reflects
					 *  whether `getWorkerRenderer()` returned non-null at
					 *  call time. `pending` is the in-flight request
					 *  count (0 when not active). */
					workerBridge: () => { active: boolean; pending: number };
				};
			}).__windE2E = {
				feedPty: (paneId, data) => this.feed(paneId, data),
				// P3.14 perf harness (2026-05-20) — writePty drives bytes
				// INTO the real PTY (same Tauri command the pane's key
				// encoder uses), so shell output flows back through
				// whichever parserBackend is active. Use this — not
				// feedPty — when the test needs to exercise the actual
				// Rust producer vs wasm consumer pipeline end-to-end.
				// feedPty short-circuits to kernel.feed and is therefore
				// useless for backend comparison.
				writePty: (paneId, data) => invoke('write_to_pty', { paneId, data }),
				visibleText: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return [];
					// kernel.dumpVisibleText returns Vec<String> as JsValue[]
					return (e.kernel.dumpVisibleText() as string[]).map((s) => String(s));
				},
				rows: (paneId) => this.rows(paneId) ?? 0,
				cols: (paneId) => this.cols(paneId) ?? 0,
				scrollbackLen: (paneId) => {
					const e = this.panes.get(paneId);
					return e ? e.kernel.scrollbackLen() : 0;
				},
				// Theme bridge regression guard: the bridge pushes a Record
				// of xterm.js-shape keys (background / foreground / cursor /
				// ANSI 16 / …) into `opts.theme`. If the boot order is
				// broken so `setupTerminalThemeBridge`'s RAF runs before the
				// first pane attaches AND attach() doesn't see opts.theme
				// either, the snapshot stays null and the kernel keeps its
				// compile-time defaults — that's the bug this hook surfaces.
				themeSnapshot: () => this.opts.theme ?? null,
				// Cursor probe for input-echo regression specs. The kernel's
				// `cursorRow / cursorCol` track the VT cursor; comparing
				// before / after a typed sequence catches any flicker /
				// misalignment in the delta path (P3.x rust parser) that
				// would otherwise only show up as a visual artefact.
				kernelCursor: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return null;
					const k = e.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
					return { row: k.cursorRow(), col: k.cursorCol() };
				},
				// Wasm-side theme probe — returns the renderer's currently
				// active `Theme::{bg, fg, cursor_color, tui_bg}` as four
				// `#rrggbbaa` hex strings. Lets JS verify the kernel-side
				// state independently of `opts.theme`, which only reflects
				// what the manager *sent*, not what the wasm renderer
				// actually accepted. The hex strings are reconstructed
				// from a 16-byte Uint8Array the wasm export returns.
				kernelThemeProbe: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return null;
					const h = e.handle as unknown as { currentThemeProbe?: () => Uint8Array };
					if (typeof h.currentThemeProbe !== 'function') {
						return { error: 'currentThemeProbe not exported — rebuild wasm pkg' };
					}
					const bytes = h.currentThemeProbe();
					if (!bytes || bytes.length < 16) return { error: 'short probe response' };
					const toHex = (off: number) => {
						const hex = (n: number) => n.toString(16).padStart(2, '0');
						return `#${hex(bytes[off])}${hex(bytes[off+1])}${hex(bytes[off+2])}${hex(bytes[off+3])}`;
					};
					return {
						bg: toHex(0),
						fg: toHex(4),
						cursor: toHex(8),
						tuiBg: toHex(12),
					};
				},
				// Theme-rotation regression probe: drive `setTheme` from a
				// spec without needing dev-server module imports (release
				// bundle hides /src/* URLs). Pairs with `sampleHostPixel`
				// to verify GPU output, not just the kernel Theme struct.
				setTheme: (theme) => this.setTheme(theme),
				// Read one device pixel from the global host canvas via
				// drawImage + getImageData. `relX / relY` are 0..1
				// fractions of the canvas backing size (default 0.5,0.85
				// = bottom-mid, almost always empty bg below the PS
				// prompt). Returns null if no host canvas or the
				// drawImage path can't sample the WebGPU swap chain.
				// IME alignment regression probes (P5.IME): expose the unified
				// anchor resolver + the JS-side mirror of the last setPreedit
				// call so specs can verify textarea cell == overlay cell ==
				// kernel cursor for shell/TUI/wrap scenarios.
				inputAnchorResolved: (paneId) => this.inputAnchorResolved(paneId),
				lastPreeditCall: (paneId) => this.lastPreeditCall(paneId),
				kernelDecState: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return null;
					const k = e.kernel as unknown as {
						isAltScreen: () => boolean;
						isAppCursorKeys: () => boolean;
						isCursorVisible: () => boolean;
						isInlineTuiMode: () => boolean;
						mouseReportingModes: () => number;
					};
					return {
						isAltScreen: k.isAltScreen(),
						isAppCursorKeys: k.isAppCursorKeys(),
						isCursorVisible: k.isCursorVisible(),
						isInlineTuiMode: k.isInlineTuiMode(),
						mouseReportingModes: k.mouseReportingModes(),
					};
				},
				historyOverlayState: (paneId) => this.historyOverlayState(paneId),
				setHistoryOverlay: (paneId, items, selectedIndex, anchorRow, anchorCol, placeAbove) =>
					this.setHistoryOverlay(paneId, items, selectedIndex, anchorRow, anchorCol, placeAbove),
				clearHistoryOverlay: (paneId) => this.clearHistoryOverlay(paneId),
				sampleHostPixel: (relX = 0.5, relY = 0.85) => {
					const host = this.globalHost?.canvas ?? null;
					if (!host) return null;
					const x = Math.max(0, Math.min(host.width - 1, Math.floor(host.width * relX)));
					const y = Math.max(0, Math.min(host.height - 1, Math.floor(host.height * relY)));
					const tmp = document.createElement('canvas');
					tmp.width = 1;
					tmp.height = 1;
					const ctx = tmp.getContext('2d', { willReadFrequently: true });
					if (!ctx) return null;
					try {
						ctx.drawImage(host, x, y, 1, 1, 0, 0, 1, 1);
					} catch {
						return null;
					}
					const d = ctx.getImageData(0, 0, 1, 1).data;
					return { r: d[0], g: d[1], b: d[2], a: d[3] };
				},
				// Selection regression hooks. These are thin pass-throughs to
				// the wasm kernel; the active spec is
				// `tests/e2e-shell/selection-tui-refresh.spec.ts`, which
				// drives the same code path the user hit when reporting
				// "selection flashes / can't copy text from claude TUI"
				// — see lib.rs::apply_delta_frame docstring for the §B.2
				// follow-up that locks down the invariant.
				setSelectionAbs: (paneId, startAbsRow, startCol, endAbsRow, endCol) => {
					const e = this.panes.get(paneId);
					if (!e) return;
					e.kernel.setSelectionAbs(startAbsRow, startCol, endAbsRow, endCol);
					this.wake();
				},
				getSelectionText: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return '';
					const k = e.kernel as unknown as { getSelectionText?: () => string };
					return k.getSelectionText?.() ?? '';
				},
				hasSelection: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return false;
					const k = e.kernel as unknown as { hasSelection?: () => boolean };
					return !!k.hasSelection?.();
				},
				applyDeltaFrameRaw: (paneId, bytes) => this.applyDeltaFrame(paneId, bytes),
				encodeCursorDeltaFrame: (paneId, seq, row, col) => {
					const e = this.panes.get(paneId);
					if (!e) return null;
					const k = e.kernel as unknown as {
						e2eEncodeCursorDeltaFrame?: (seq: number, row: number, col: number) => Uint8Array;
					};
					return k.e2eEncodeCursorDeltaFrame?.(seq, row, col) ?? null;
				},
				// §1.33 (2026-05-22) — PTY write spy used by the shell-
				// history-gate / ArrowRight e2e specs to assert the
				// exact bytes the popup-onSelect path produced (e.g.
				// "command without trailing '\r' on ArrowRight"). The
				// spy wraps the entry's dataHandler in place; calling
				// `installPtyWriteSpy` is idempotent per pane.
				installPtyWriteSpy: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e || !e.dataHandler) return;
					const ent = e as unknown as { _e2ePtyWriteLog?: Array<{ data: string; at: number }> };
					if (ent._e2ePtyWriteLog) return;
					const log: Array<{ data: string; at: number }> = [];
					ent._e2ePtyWriteLog = log;
					const original = e.dataHandler;
					const decoder = new TextDecoder();
					e.dataHandler = (bytes: Uint8Array) => {
						try {
							log.push({ data: decoder.decode(bytes), at: performance.now() });
						} catch {
							// Decoder errors must NOT block the real write — spy is observation-only.
						}
						original(bytes);
					};
				},
				ptyWriteLog: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return [];
					const ent = e as unknown as { _e2ePtyWriteLog?: Array<{ data: string; at: number }> };
					return ent._e2ePtyWriteLog ? [...ent._e2ePtyWriteLog] : [];
				},
				clearPtyWriteLog: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return;
					const ent = e as unknown as { _e2ePtyWriteLog?: Array<{ data: string; at: number }> };
					if (ent._e2ePtyWriteLog) ent._e2ePtyWriteLog.length = 0;
				},
				workerBridge: () => ({
					active: workerRendererBridge.isActive(),
					pending: workerRendererBridge.pendingCount(),
				}),
			};
		}
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
			// pointermove batches on rAF; cancel any in-flight tick so the
			// flush callback doesn't reach into a freed kernel via
			// kernel.encodeMouse / dataHandler.
			if (entry.mouseMoveRaf !== null) {
				cancelAnimationFrame(entry.mouseMoveRaf);
				entry.mouseMoveRaf = null;
			}
			entry.pendingMouseMove = null;
			entry.lastMouseSent = null;
			// Auto-scroll ticker holds a closure over the pane id; once
			// the pane is torn down its setInterval callback would call
			// scrollUp/Down against a freed kernel. Stop it here.
			if (entry.autoScrollTimer !== null) {
				clearInterval(entry.autoScrollTimer);
				entry.autoScrollTimer = null;
			}
			entry.autoScrollDirection = null;
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
				this._invalidateHost();
			} else {
				try {
					entry.canvas.remove();
				} catch {
					/* canvas already detached */
				}
			}
			try { entry.handle?.free(); } catch { /* ignore */ }
		}
		// §1.27: cancel any pending IME-anchor rAF before freeing the
		// kernel — the rAF body would otherwise call cursorRow() on a
		// freed kernel and crash.
		if (entry.imeAnchorRaf !== null) {
			cancelAnimationFrame(entry.imeAnchorRaf);
			entry.imeAnchorRaf = null;
		}
		// §A.4: flush any pending coalesced PTY bytes to the kernel BEFORE
		// `kernel.free()` so we don't drop end-of-stream bytes on a tab
		// close. After flush, drop the timer/buffer so no setTimeout
		// callback fires against a freed kernel.
		if (entry.feedFlushTimer !== null) {
			clearTimeout(entry.feedFlushTimer);
			entry.feedFlushTimer = null;
		}
		if (entry.feedBuffer !== null && entry.feedBuffer.length > 0) {
			try { entry.kernel.feed(entry.feedBuffer); } catch { /* kernel already freed elsewhere */ }
		}
		entry.feedBuffer = null;
		// P2.1 (2026-05-20): also drain any time-budget-deferred bytes
		// into the kernel before it's freed, so a tab-close that races
		// a high-output burst doesn't drop the trailing chunk. Best-
		// effort: the kernel may already be wedged if free() was called
		// elsewhere — swallow to keep the close path idempotent.
		if (entry.feedDeferred !== null && entry.feedDeferred.length > 0) {
			try { entry.kernel.feed(entry.feedDeferred); } catch { /* kernel already freed elsewhere */ }
		}
		entry.feedDeferred = null;
		// Kernel always alive while in the map (parked or not).
		try { entry.kernel.free(); } catch { /* ignore */ }
		// P4.6 Part B (2026-05-22) — mirror teardown into the render
		// worker. `destroy` is idempotent on the worker side: an unknown
		// paneId just deletes nothing. Skip the Set lookup; we want the
		// destroy to fire even when the attached-set is empty (e.g.
		// flag flipped off mid-session).
		workerRendererBridge.destroy(paneId);
		this.workerAttached.delete(paneId);
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
		entry.selectionStartAbs = null;
		entry.selectionEndAbs = null;
		if (entry.mouseMoveRaf !== null) {
			cancelAnimationFrame(entry.mouseMoveRaf);
			entry.mouseMoveRaf = null;
		}
		entry.pendingMouseMove = null;
		entry.lastMouseSent = null;
		// Same reason as in detach — kill the auto-scroll ticker so its
		// next tick doesn't fire against a parked pane (kernel still
		// alive but UI bindings are gone, and re-attach should resume
		// from a clean slate anyway).
		if (entry.autoScrollTimer !== null) {
			clearInterval(entry.autoScrollTimer);
			entry.autoScrollTimer = null;
		}
		entry.autoScrollDirection = null;

		// §A.9: host-mode panes share the global canvas; just mark for
		// clear so departed pixels don't linger. Canvas2D mode owns its
		// per-pane DOM canvas — clean up.
		if (this._isHostMode(entry)) {
			this._invalidateHost();
		} else {
			try { entry.canvas.remove(); } catch { /* already detached */ }
		}
		try { entry.handle?.free(); } catch { /* ignore */ }

		// §A.4: kernel stays alive while parked, but the flush timer is
		// tied to setTimeout — cancel it and replay any buffered bytes
		// directly into the live kernel so background PTY output that
		// arrives during the parked window doesn't get lost.
		this._flushFeedBuffer(entry);

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
		if (this.attachHostPromise) {
			try { await this.attachHostPromise; } catch { /* ignore */ }
		}

		const gh = this.globalHost;
		const useHost = gh !== null && this.opts.preferWebgpu;
		let canvas: HTMLCanvasElement;
		let hostHandle: SurfaceHostHandle | undefined;
		if (useHost && gh) {
			canvas = gh.canvas;
			hostHandle = gh.host;
			container.style.background = 'transparent';
		} else {
			canvas = document.createElement('canvas');
			canvas.style.cssText = 'display:block; width:100%; height:100%; position:relative; z-index:0;';
			canvas.setAttribute('aria-hidden', 'true');
			container.appendChild(canvas);
		}

		if (this.opts.paddingPx && this.opts.paddingPx > 0) {
			container.style.padding = `${this.opts.paddingPx}px`;
		}

		const handle = await this._makeHandle(canvas, hostHandle);
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
		// Force a Clear on this workspace's surface so any pre-park
		// pixels in this slot don't bleed through during the first fit.
		if (useHost && hostHandle) {
			this._invalidateHost();
		}
		entry.cellW = quantizeCellSize(Number(cellW), dpr);
		entry.cellH = quantizeCellSize(Number(cellH), dpr);
		entry.lastConfiguredDpr = dpr;
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

		// §A.4 (2026-05-08) — coalesce sub-frame ConPTY split-writes during
		// inline-TUI so a rAF tick can't sample the kernel mid-walk. ConPTY
		// on Windows splits a single application write across 2-3 reads when
		// the byte stream straddles its internal buffer; Ink's
		// `(walk)+\x1b[G+frame` then arrives as separate `pty-output` events.
		// Without batching, the renderer paints a partial state (cells erased
		// but new content not yet fed). 8 ms is well below Ink's ~30 ms
		// spinner cadence and well above ConPTY's 1-2 ms inter-fragment gap.
		// Disabled when not inline-TUI so ordinary shells feed immediately.
		let inlineTui = false;
		try {
			inlineTui = entry.kernel.isInlineTuiMode();
		} catch {
			// Older wasm bundle without isInlineTuiMode export → bypass coalescing.
			inlineTui = false;
		}
		// §A.4 — tick trace: correlate feed timing with rAF sampling.
		if (typeof localStorage !== 'undefined' && localStorage.RIDGE_TICK_TRACE === '1') {
			const ts = performance.now().toFixed(1);
			const id = paneId.slice(0, 8);
			const buffered = entry.feedBuffer ? entry.feedBuffer.length : 0;
			const pending = entry.feedFlushTimer !== null ? 'pending' : 'idle';
			// eslint-disable-next-line no-console
			// console.debug(
			// 	`[tick-trace][${ts}ms][${id}][feed +${bytes.length}B inlineTui=${inlineTui} buffered=${buffered}B flush=${pending}]`,
			// );
		}
		if (inlineTui) {
			// §4c (2026-05-08) — burst-only coalescing. The §A.4 8 ms
			// gate was applied to ALL inline-TUI bytes, including the
			// shell's echo of typed characters. That added 8 ms to
			// every keystroke's perceived latency in panes hosting an
			// inline TUI (Claude Code's `claude`, lazygit, …) — Jack
			// reported it as "typing feels laggy in multi-pane".
			//
			// Echo bytes are pure ASCII (no ESC sequences). Ink-style
			// redraws are densely packed CSI sequences (\x1b[..). Gate
			// the buffer on "bytes contain ESC" so:
			//   - keystroke echoes feed immediately (zero added lag);
			//   - any CSI/escape stream still buffers and the §A.4
			//     coalesce protects mid-walk sampling.
			// Mixed packets (echo + CSI in one chunk, e.g. PSReadLine
			// emitting echo + CHA reposition) take the buffer path —
			// safe because the first byte before the ESC is implicitly
			// fed in-order when the buffer flushes.
			const hasEsc = bytes.indexOf(0x1b) !== -1;
			if (hasEsc) {
				// §4d (2026-05-19) — first-chunk-fast-path.
				//
				// §4c above always buffered every ESC-bearing chunk for
				// 8 ms so ConPTY's split-writes coalesce into one feed.
				// That added 8 ms (plus up to one rAF) to every user-
				// input response in TUI apps: an ArrowUp inside vim
				// only echoes `\x1b[A`, a single chunk that never
				// needed coalescing, yet the buffer held it back.
				// Symptom in claude code TUI: rapid arrow keys feel
				// like they only register every other press because
				// two adjacent responses collapse into one frame.
				//
				// Fix: feed the FIRST esc chunk immediately, then open
				// the 8 ms coalesce window so any FOLLOW-UP fragments
				// from a ConPTY split-write still rejoin the same
				// frame. ConPTY's split always arrives as
				// (head chunk now) + (tail chunk a few ms later), so
				// catching only the tail is sufficient — single-chunk
				// responses (the vast majority of user-input echoes)
				// pay zero added latency.
				if (entry.feedBuffer === null && entry.feedFlushTimer === null) {
					this._feedNow(entry, bytes);
					entry.feedFlushTimer = setTimeout(() => {
						entry.feedFlushTimer = null;
						this._flushFeedBuffer(entry);
					}, 8);
					return;
				}
				// Inside a coalesce window — append for the tail of a
				// split-write. Same back-pressure cap as before.
				entry.feedBuffer = entry.feedBuffer
					? concatU8(entry.feedBuffer, bytes)
					: bytes;
				if (entry.feedBuffer.length >= 8192) {
					this._flushFeedBuffer(entry);
				}
				return;
			}
			// No ESC → pure text echo path. Flush any pending buffer
			// FIRST so byte order with prior CSI activity is preserved,
			// then feed the echo bytes immediately.
			if (entry.feedBuffer !== null) {
				this._flushFeedBuffer(entry);
			}
			this._feedNow(entry, bytes);
			return;
		}

		// Non-inline path: preserve byte order if a buffer was just left
		// over from a recent inline-TUI window (cursor became visible
		// between events) — flush it first, then feed the new bytes.
		if (entry.feedBuffer !== null) {
			this._flushFeedBuffer(entry);
		}
		this._feedNow(entry, bytes);
	}

	/** §A.4 — feed bytes to the kernel synchronously, including PTY trace,
	 *  reply / event drain, and rAF wake. Extracted from `feed()` so the
	 *  inline-TUI coalescer can call it once per flush instead of once per
	 *  PTY event. Always feeds — does NOT consult the inline-TUI gate.
	 *
	 *  P2.1 (2026-05-20): the wasm `kernel.feed(bytes)` call is synchronous
	 *  and runs the VTE state machine byte-by-byte; on a 200 KB compile
	 *  burst from a single pane it would block the JS main thread for
	 *  ~50 ms, starving keystrokes on every other pane plus the RAF loop
	 *  itself. We now chunk the input into ~16 KB pieces and stop after
	 *  `FEED_PER_CALL_BUDGET_MS` of wall-clock; leftover bytes spill into
	 *  `entry.feedDeferred` and the RAF tick drains them at the top of
	 *  the next frame (after preserving order with any later arrivals).
	 *  vte::Parser carries its own state across feed calls so byte-level
	 *  chunking is safe — even mid-CSI / mid-OSC. */
	private _feedNow(entry: PaneEntry, bytes: Uint8Array): void {
		// §1.24 PTY trace (Phase 1.2): when `localStorage.RIDGE_PTY_TRACE === '1'`,
		// log every PTY-to-wasm byte chunk with a high-res timestamp so a live
		// resize-while-claude repro can be replayed in devtools to confirm
		// whether ConPTY's reflow noise leaks past the silence skip.
		if (typeof localStorage !== 'undefined' && localStorage.RIDGE_PTY_TRACE === '1') {
			const ts = performance.now().toFixed(1);
			const id = entry.paneId.slice(0, 8);
			const hex = Array.from(bytes.slice(0, 256))
				.map((b) => b.toString(16).padStart(2, '0'))
				.join('');
			const more = bytes.length > 256 ? `…+${bytes.length - 256}B` : '';
			// eslint-disable-next-line no-console
			console.debug(`[pty-trace][${ts}ms][${id}][${bytes.length}B] ${hex}${more}`);
		}

		// P2.1: if a previous _feedNow already deferred bytes for this
		// pane, the new arrivals MUST queue behind them — otherwise vte
		// would see them in shuffled order and emit garbage. Append and
		// let the next RAF tick drain (which calls _feedNow with the
		// whole queue, no overflow check needed for the queued half).
		if (entry.feedDeferred) {
			entry.feedDeferred = concatU8(entry.feedDeferred, bytes);
			this.wake();
			return;
		}

		// P2.1: budget-aware chunked feed. Each call gets at most
		// FEED_PER_CALL_BUDGET_MS of wall-clock to push into the kernel
		// before yielding. 4 ms ≈ a quarter of one 60 fps frame — plenty
		// for typical PTY arrivals, generous enough that small bursts
		// (the common case) finish in one chunk, strict enough that a
		// 200 KB compile waterfall doesn't freeze the main thread.
		const FEED_PER_CALL_BUDGET_MS = 4;
		const FEED_CHUNK_BYTES = 16 * 1024;
		let offset = 0;
		const start = performance.now();
		const traceCursor = typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_CURSOR_TRACE') === '1';
		const k = entry.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
		const pre = traceCursor ? `(${k.cursorRow()},${k.cursorCol()})` : '';
		while (offset < bytes.length) {
			const end = Math.min(offset + FEED_CHUNK_BYTES, bytes.length);
			entry.kernel.feed(bytes.subarray(offset, end));
			offset = end;
			if (performance.now() - start >= FEED_PER_CALL_BUDGET_MS) break;
		}
		if (traceCursor) {
			const ts = performance.now().toFixed(1);
			// eslint-disable-next-line no-console
			console.debug(`[cursor-trace][${ts}ms] feed paneId=${entry.paneId.slice(0,8)} bytes=${bytes.length} consumed=${offset} cursor ${pre}→(${k.cursorRow()},${k.cursorCol()})`);
		}
		if (offset < bytes.length) {
			// Copy via `slice` so the deferred queue doesn't pin the
			// possibly-much-larger original ArrayBuffer through its
			// subarray view (would waste memory on every spill).
			entry.feedDeferred = bytes.slice(offset);
		}

		// 屏幕内容变化 → 链接索引失效，下次 ctrl+hover/click 时再 lazy 重建。
		entry.linkSpans.markDirty();
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
				entry.paneId,
				'— events discarded; check onEvent() registration order',
			);
		}
	}

	/** P3.9 (2026-05-20) — apply one postcard-encoded `DeltaFrame` from the
	 *  Rust-side `engine::parser::PaneParser` (produced when this pane's
	 *  backend `delta_mode` is on). Mirror counterpart to `feed()`:
	 *  apply diff → drain pending_response back to PTY → drain
	 *  pending_events → wake render loop.
	 *
	 *  Designed to be called by `ptyBridge.ts`'s `pty-delta-{ws}-{pane}`
	 *  listener; never invoked directly by RidgePane. Bytes is the raw
	 *  postcard payload; the wasm bridge decodes + applies in one shot.
	 *  Throws (JsValue → JS Error) on decode failure or protocol-version
	 *  mismatch; caller logs and triggers a `set_pane_delta_mode(false)`
	 *  fallback as the self-heal path (P3 R5 mitigation).
	 */
	applyDeltaFrame(paneId: string, bytes: Uint8Array): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		// Re-throw decode / version errors so ptyBridge can trigger the
		// self-heal `set_pane_delta_mode(false)` invoke. manager is host-
		// agnostic (no Tauri imports) — recovery routing lives in
		// ptyBridge where the invoke surface is available.
		const traceCursor = typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_CURSOR_TRACE') === '1';
		const k = entry.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
		const pre = traceCursor ? `(${k.cursorRow()},${k.cursorCol()})` : '';
		entry.kernel.applyDeltaFrame(bytes);
		if (traceCursor) {
			const ts = performance.now().toFixed(1);
			// eslint-disable-next-line no-console
			console.debug(`[cursor-trace][${ts}ms] applyDeltaFrame paneId=${paneId.slice(0,8)} bytes=${bytes.length} cursor ${pre}→(${k.cursorRow()},${k.cursorCol()})`);
		}
		// P4.6 Part B (2026-05-22) — shadow-mirror the delta into the
		// render worker when the feature flag is on AND this pane has
		// been attached over there. Bridge handles the .slice() copy so
		// the kernel call above still owns the original bytes.
		if (this.workerAttached.has(paneId)) {
			workerRendererBridge.applyDelta(paneId, bytes);
		}
		// Pump DSR/DA replies the mirror produced via apply_delta back to
		// the PTY. Symmetric with feed()'s take_pending_response drain.
		const reply = entry.kernel.takePendingResponse();
		if (reply.length > 0 && entry.dataHandler) {
			entry.dataHandler(reply);
		}
		// Drain semantic events (title / cwd / bell). apply_delta pushes
		// them onto the same pending_events queue feed() uses so the
		// existing eventHandler routing applies unchanged.
		const events = entry.kernel.takePendingEvents() as KernelEvent[];
		if (entry.eventHandler) {
			for (const ev of events) entry.eventHandler(ev);
		}
		entry.linkSpans.markDirty();
		this.wake();
	}

	/** P2.1 (2026-05-20): drain any per-pane bytes that prior `_feedNow`
	 *  calls spilled out of when their time budget ran out. Called at
	 *  the top of every RAF tick BEFORE the dirty-detection pre-pass,
	 *  so the next frame sees whatever the kernel ends up consuming on
	 *  this tick. The drain itself re-enters `_feedNow` which applies
	 *  its own budget — so a perpetually-bursting pane consumes one
	 *  chunk per frame and never blocks the loop for more than ~4 ms,
	 *  while other panes keep their own budget intact.
	 *
	 *  P2.2: takes the same focus-first + rotated-others order as the
	 *  render pass so the focused pane recovers from a burst fastest,
	 *  while non-focused panes still see progress every frame via
	 *  the rotation. */
	private _drainDeferredFeeds(order: readonly PaneEntry[]): void {
		for (const entry of order) {
			if (!entry.feedDeferred) continue;
			const buf = entry.feedDeferred;
			// Clear BEFORE _feedNow — the call will re-set it if the new
			// budget runs out before consuming the whole queue.
			entry.feedDeferred = null;
			this._feedNow(entry, buf);
		}
	}

	/** §A.4 — flush any pending coalesced bytes to the kernel and clear the
	 *  timer. Safe to call when the buffer is empty (no-op). */
	private _flushFeedBuffer(entry: PaneEntry): void {
		if (entry.feedFlushTimer !== null) {
			clearTimeout(entry.feedFlushTimer);
			entry.feedFlushTimer = null;
		}
		const buf = entry.feedBuffer;
		if (buf === null || buf.length === 0) {
			entry.feedBuffer = null;
			return;
		}
		entry.feedBuffer = null;
		this._feedNow(entry, buf);
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

		// Handle OS native Copy on Ctrl+C / Cmd+C when text is selected.
		const isCtrlC = ctrl && ev.key.toLowerCase() === 'c';
		if (isCtrlC) {
			const sel = entry.kernel.getSelectionText();
			if (sel && sel.length > 0) {
				// Don't encode \x03, instead copy and clear selection
				void navigator.clipboard.writeText(sel);
				entry.kernel.clearSelection();
				this.wake();
				return true;
			}
		}

		const bytes = entry.kernel.encodeKey(ev.key, ctrl, ev.altKey, ev.shiftKey, ev.metaKey);
		if (bytes.length === 0) return false;
		// Real Ctrl+C (no selection → falling through to ETX `\x03`):
		// arm the kernel's inline-TUI grace window so the IME helper /
		// shell-history popup can re-enable after the foreground TUI
		// dies. Without this, PSReadLine's per-keystroke CHA `\x1b[G`
		// keeps the inline-TUI heuristic stuck on forever (cursor
		// stayed hidden because the killed TUI never got to emit ?25h).
		if (isCtrlC) {
			const k = entry.kernel as unknown as { noteCtrlCSent?: () => void };
			k.noteCtrlCSent?.();
		}
		if (typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_CURSOR_TRACE') === '1') {
			const ts = performance.now().toFixed(1);
			const hex = Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join(' ');
			const k = entry.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
			// eslint-disable-next-line no-console
			console.debug(`[cursor-trace][${ts}ms] keydown key=${JSON.stringify(ev.key)} → bytes(${bytes.length})=${hex} kernel-cursor(pre)=(${k.cursorRow()},${k.cursorCol()})`);
		}
		entry.dataHandler(bytes);
		this.scheduleImeAnchorCapture(entry);
		return true;
	}

	/**
	 * Forward a wheel event to the TUI application when DEC mouse reporting
	 * is active. Encodes the scroll as an SGR mouse sequence (button 64/65
	 * for up/down) and sends it through the data handler.
	 */
	handleWheel(paneId: string, ev: WheelEvent): boolean {
		const entry = this.panes.get(paneId);
		if (!entry || !entry.dataHandler) return false;
		if (entry.kernel.mouseReportingModes() === 0) return false;

		const rect = entry.container.getBoundingClientRect();
		// §1.30: subtract container padding before dividing — TUIs that
		// receive a wheel-as-mouse SGR report deserve the same accurate
		// row/col as click handlers. Otherwise wheel-over-cell-N gets
		// reported as cell-N+1 once `pad > 0`.
		const pad = entry.lastFitPaddingPx ?? entry.lastAppliedPaddingPx ?? 0;
		const x = ev.clientX - rect.left - pad;
		const y = ev.clientY - rect.top - pad;
		if (entry.cellW <= 0 || entry.cellH <= 0) return false;
		const cols = entry.kernel.cols();
		const rows = entry.kernel.rows();
		if (cols === 0 || rows === 0) return false;
		const col = Math.max(0, Math.min(cols - 1, Math.floor(x / entry.cellW)));
		const row = Math.max(0, Math.min(rows - 1, Math.floor(y / entry.cellH)));

		const delta = ev.deltaY;
		if (delta === 0) return false;

		const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
		const ctrl = ev.ctrlKey || (isMac && ev.metaKey);
		const btn = delta < 0 ? 64 : 65; // 64=up, 65=down
		const bytes = entry.kernel.encodeMouse(row, col, btn, 0, ev.shiftKey, ctrl, ev.altKey);
		if (bytes.length > 0) {
			entry.dataHandler(bytes);
			return true;
		}
		return false;
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

	/** Dev-only: dump cell characters + Unicode codepoints for a range of
	 *  viewport rows. Used to diagnose which characters / attributes a TUI
	 *  is actually drawing when procedural / atlas rendering produces
	 *  visible artefacts. Exposed as `window.__windDumpRows(paneId, from,
	 *  to)` at attach time. Returns `[]` for unknown pane. */
	debugDumpRows(paneId: string, fromVpRow: number, toVpRow: number): Array<{
		row: number;
		nonSpace: Array<{ col: number; ch: string; hex: string; attrId: number; fg: string; bg: string; dim: boolean; bold: boolean; inverse: boolean }>;
	}> {
		const ent = this.panes.get(paneId);
		if (!ent) return [];
		const cols = ent.kernel.cols();
		const rows = ent.kernel.rows();
		if (cols === 0 || rows === 0) return [];
		const lo = Math.max(0, Math.min(rows - 1, Math.floor(fromVpRow)));
		const hi = Math.max(0, Math.min(rows - 1, Math.floor(toVpRow)));
		const out: Array<{ row: number; nonSpace: Array<{ col: number; ch: string; hex: string; attrId: number; fg: string; bg: string; dim: boolean; bold: boolean; inverse: boolean }> }> = [];
		for (let r = lo; r <= hi; r++) {
			const cells = ent.kernel.cellsAt(r, 0, cols) as Array<{
				col: number; ch: string; codepoint: number; width: number;
				attrId: number; dim: boolean; bold: boolean; italic: boolean;
				underline: boolean; inverse: boolean; hidden: boolean;
				fg: string; bg: string;
			}>;
			const nonSpace = cells
				.filter((c) => c.ch !== ' ' || c.fg !== 'default' || c.bg !== 'default')
				.map((c) => ({
					col: c.col,
					ch: c.ch,
					hex: 'U+' + c.codepoint.toString(16).toUpperCase().padStart(4, '0'),
					attrId: c.attrId,
					fg: c.fg,
					bg: c.bg,
					dim: c.dim,
					bold: c.bold,
					inverse: c.inverse,
				}));
			out.push({ row: r, nonSpace });
		}
		return out;
	}

	/** Compute viewport cell coordinates from a mouse/pointer event.
	 *  Returns null if the pane is unknown or cell metrics aren't ready.
	 *
	 *  §1.30 (2026-05-19): the pane container has CSS `padding` (set by
	 *  `setPadding`) and the canvas paints inside the content-box, so the
	 *  drawn rows start at `rect.top + pad`. Without subtracting pad,
	 *  every click further than `cellH - pad` from the canvas top maps
	 *  to the row BELOW its visual cell, producing the "mouse appears
	 *  higher than the selection start" symptom. Symmetric pad subtraction
	 *  on x fixes the same off-by-one on the column axis.
	 *  `inputAnchorPixelPosition` (this file, ~line 2286) already applies
	 *  the same pad correction in the opposite direction. */
	cellFromEvent(paneId: string, e: { clientX: number; clientY: number }): { row: number; col: number } | null {
		const ent = this.panes.get(paneId);
		if (!ent || ent.cellW <= 0 || ent.cellH <= 0) return null;
		const rect = ent.container.getBoundingClientRect();
		const pad = ent.lastFitPaddingPx ?? ent.lastAppliedPaddingPx ?? 0;
		const x = e.clientX - rect.left - pad;
		const y = e.clientY - rect.top - pad;
		const cols = ent.kernel.cols();
		const rows = ent.kernel.rows();
		if (cols === 0 || rows === 0) return null;
		const col = Math.max(0, Math.min(cols - 1, Math.floor(x / ent.cellW)));
		const row = Math.max(0, Math.min(rows - 1, Math.floor(y / ent.cellH)));
		return { row, col };
	}

	/** Write raw bytes to the pane's PTY via dataHandler. */
	sendData(paneId: string, data: Uint8Array): void {
		const ent = this.panes.get(paneId);
		if (!ent || !ent.dataHandler) return;
		ent.dataHandler(data);
	}

	/** Get the wasm kernel for a pane. Used for direct kernel method calls
	 *  (e.g. encodeMouse) from component event handlers. */
	getKernel(paneId: string): TerminalKernel | null {
		return this.panes.get(paneId)?.kernel ?? null;
	}

    isSelecting(paneId: string): boolean {
        return this.panes.get(paneId)?.selecting ?? false;
    }

    getMousePosition(paneId: string): { row: number, col: number } {
        return this.panes.get(paneId)?.selectionEndAbs ?? { row: 0, col: 0 };
    }

    private _syncSelection(ent: PaneEntry): void {
        if (!ent.selectionStartAbs || !ent.selectionEndAbs) return;
        // selectionStart/EndAbs are stored in **absolute-row coords**
        // (vp_row + scroll_offset captured at point of input). Forward
        // them through the abs entry point so the wasm side doesn't
        // re-translate vp→abs against the current — possibly different —
        // scroll_offset (the bug that made highlights drift after every
        // scroll: vp→abs ran in JS and again in wasm, so the stored
        // abs_row landed at vp_row + 2*scroll_offset).
        ent.kernel.setSelectionAbs(
            ent.selectionStartAbs.row, ent.selectionStartAbs.col,
            ent.selectionEndAbs.row, ent.selectionEndAbs.col
        );
        this.wake();
    }

    /** 滚动时扩展选择 */
    updateSelection(paneId: string, endAbs: { row: number, col: number }) {
        const ent = this.panes.get(paneId);
        if (!ent || !ent.selectionStartAbs) return;
        ent.selectionEndAbs = endAbs;
        this._syncSelection(ent);
    }

	clearSelection(paneId: string): void {
		this.panes.get(paneId)?.kernel.clearSelection();
		this.wake();
	}

	/**
	 * §B.2 (2026-05-08) — drop the in-kernel scrollback ring buffer
	 * (physical clear) and snap viewport to live grid. Mirrors the
	 * xterm `\x1b[3J` sequence at the JS API level so the right-click
	 * "清空" handler can wipe both screen + saved lines without a PTY
	 * round trip (and without depending on the active shell to translate
	 * Ctrl+L into ED 3 — most don't).
	 *
	 * Use-case: user hits the right-click "清空" menu after a verbose
	 * session and expects ALL evidence gone. Pre-fix this only sent
	 * Ctrl+L which bash/PowerShell handle by emitting ED 2 + cursor
	 * home — visible grid clears but pageUp still resurrects everything
	 * the user wanted gone (the documented "clear 不能完全清理" symptom).
	 */
	clearScrollback(paneId: string): void {
		this.panes.get(paneId)?.kernel.clearScrollback();
		this.wake();
	}

	/** Tell the wasm renderer whether this pane is the focused one. Only the
	 *  truly focused pane should blink its cursor; unfocused panes hide it
	 *  entirely. RidgePane wires this to the global `activePaneId` store so
	 *  switching panes flips the cursor visibility on both sides instantly. */
	setFocused(paneId: string, focused: boolean): void {
		this.panes.get(paneId)?.handle?.setFocused(focused);
		// P2.2 (2026-05-20): also mirror the focus bit at the manager
		// level so the RAF tick can order the focused pane FIRST each
		// frame. Without this the renderer-side `set_focused` is the
		// only signal, and the tick has no way to peek at it from
		// outside the wasm bridge. Clear when the currently-tracked
		// pane loses focus and nothing else has claimed it yet —
		// otherwise the stale id would push a parked / departing pane
		// to the head of the order.
		if (focused) {
			this._focusedPaneId = paneId;
		} else if (this._focusedPaneId === paneId) {
			this._focusedPaneId = null;
		}
		// Cursor visibility changed → cursor row dirties → wake.
		this.wake();
	}

	/** P2.2 (2026-05-20): build the per-frame pane visit order for the
	 *  RAF tick — focused pane first, then non-focused entries rotated
	 *  by `_rafRotationIndex` so over many frames every non-focused
	 *  pane gets first-of-the-rest treatment in turn. Parked entries
	 *  are filtered out (the render loop already skips them, but
	 *  excluding here keeps the rotation index meaningful — otherwise
	 *  a parked pane's slot would shift the cadence). Workspace
	 *  visibility is handled later (the existing `_isContainerHidden`
	 *  check) — this helper just orders the candidates. */
	private _renderOrder(): PaneEntry[] {
		const live: PaneEntry[] = [];
		for (const entry of this.panes.values()) {
			if (!entry.parked) live.push(entry);
		}
		if (live.length <= 1) return live;
		const focusedId = this._focusedPaneId;
		let focused: PaneEntry | undefined;
		const others: PaneEntry[] = [];
		for (const e of live) {
			if (e.paneId === focusedId) focused = e;
			else others.push(e);
		}
		if (others.length > 1) {
			const rot = this._rafRotationIndex % others.length;
			if (rot > 0) {
				const rotated = others.slice(rot).concat(others.slice(0, rot));
				others.length = 0;
				others.push(...rotated);
			}
		}
		return focused ? [focused, ...others] : others;
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
		const e = this.panes.get(paneId);
		if (!e) return;
		e.kernel.scrollToBottom();
		e.linkSpans.markDirty();
	}

	scrollUp(paneId: string, lines: number): void {
		const e = this.panes.get(paneId);
		if (!e) return;
		e.kernel.scrollUp(lines);
		e.linkSpans.markDirty();
		// Every other state-mutating manager method ends with wake(); these
		// two were the only holes. Without it the rAF loop stays idle after
		// the viewport offset moves, so the screen sits on the pre-scroll
		// frame until some unrelated event (next PTY byte, keystroke, …)
		// happens to wake it. User-perceptible symptom: "wheel feels laggy."
		this.wake();
	}

	scrollDown(paneId: string, lines: number): void {
		const e = this.panes.get(paneId);
		if (!e) return;
		e.kernel.scrollDown(lines);
		e.linkSpans.markDirty();
		this.wake();
	}

	/** Returns scroll offset (0 = at bottom) and scrollback length, for UI hints. */
	scrollState(paneId: string): { offset: number; total: number } {
		const e = this.panes.get(paneId);
		if (!e) return { offset: 0, total: 0 };
		return { offset: e.kernel.scrollOffset(), total: e.kernel.scrollbackLen() };
	}

	/** P1.3 (2026-05-19): subscribe to scroll-state changes for one pane.
	 *  The handler fires at most once per RAF tick when `kernel.scrollOffset`
	 *  or `kernel.scrollbackLen` differ from the previous emit, and once
	 *  immediately with the current snapshot so the subscriber doesn't
	 *  also need an initial read.
	 *
	 *  Replaces the 250ms `setInterval(refreshScrollState, …)` RidgePane
	 *  used to run per pane (§1.23). Sleeping panes pay nothing — emits
	 *  ride on the existing RAF loop that PTY feed / scrollUp / scrollDown
	 *  already wake.
	 *
	 *  Single-consumer: a fresh registration replaces the previous one,
	 *  matching `eventHandler` / `dataHandler` semantics. Returns an
	 *  unsubscribe that no-ops if the pane has been detached. */
	onScrollState(
		paneId: string,
		handler: (state: { offset: number; total: number }) => void,
	): () => void {
		const e = this.panes.get(paneId);
		if (!e) return () => {};
		e.scrollStateHandler = handler;
		// Baseline emit so the subscriber's UI doesn't sit on its initial
		// `$state` default until the next PTY byte / scroll event.
		try {
			const off = e.kernel.scrollOffset();
			const tot = e.kernel.scrollbackLen();
			e.lastScrollOffset = off;
			e.lastScrollTotal = tot;
			handler({ offset: off, total: tot });
		} catch {
			// kernel may have been freed between get() and the call; the
			// next RAF tick will pick the subscriber up.
		}
		return () => {
			const cur = this.panes.get(paneId);
			if (cur && cur.scrollStateHandler === handler) cur.scrollStateHandler = null;
		};
	}

	/** P1.3: diff each subscribed pane's scroll state against its cached
	 *  pair and fire the handler when it changed. Called from the RAF tick
	 *  after the per-pane render loop so the emit reflects the same
	 *  kernel state the user just saw painted. */
	private _emitScrollStateChanges(): void {
		for (const entry of this.panes.values()) {
			if (entry.parked) continue;
			const h = entry.scrollStateHandler;
			if (!h) continue;
			let off: number;
			let tot: number;
			try {
				off = entry.kernel.scrollOffset();
				tot = entry.kernel.scrollbackLen();
			} catch {
				continue; // kernel freed mid-tick — skip this pane
			}
			if (off === entry.lastScrollOffset && tot === entry.lastScrollTotal) continue;
			entry.lastScrollOffset = off;
			entry.lastScrollTotal = tot;
			try {
				h({ offset: off, total: tot });
			} catch (err) {
				console.error('[ridge-term] scrollStateHandler error', entry.paneId, err);
			}
		}
	}

	rows(paneId: string): number { return this.panes.get(paneId)?.kernel.rows() ?? 0; }
	cols(paneId: string): number { return this.panes.get(paneId)?.kernel.cols() ?? 0; }

	/** Whether the pane is currently in alt-screen mode (TUI app active). */
	isAltScreen(paneId: string): boolean { return this.panes.get(paneId)?.kernel.isAltScreen() ?? false; }

	/** Install an IME preedit overlay on the pane's renderer (a layer
	 *  painted on top of the cell grid each frame, NOT a feed into the
	 *  kernel cells). RidgePane calls this on `compositionupdate` so
	 *  CJK preedit text appears inline at the cursor without disturbing
	 *  any underlying TUI content — Ink redraws can't clobber preedit,
	 *  preedit can't clobber Ink's frame. Empty `text` is treated as
	 *  `clearPreedit`. */
	setPreedit(paneId: string, text: string, row: number, col: number): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		const h = entry.handle as unknown as { setPreedit?: (t: string, r: number, c: number) => void };
		h.setPreedit?.(text, row, col);
		this._lastPreeditCall.set(paneId, { row, col, text });
		this.wake();
	}

	/** Remove the IME preedit overlay. Called on `compositionend` after
	 *  the committed string has been shipped to the PTY. */
	clearPreedit(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		const h = entry.handle as unknown as { clearPreedit?: () => void };
		h.clearPreedit?.();
		this._lastPreeditCall.delete(paneId);
		this.wake();
	}

	/** §1.34 (2026-05-22): install the shell-history popup overlay on
	 *  this pane's canvas. Replaces any prior overlay state. JS owns
	 *  the filter / dedup logic; this just ships the snapshot to the
	 *  wasm renderer which paints it on top of the cell grid every
	 *  frame. `items` is shipped as a JS array directly — the wasm
	 *  side calls `js_sys::Array::iter().filter_map(as_string)` so
	 *  non-string entries are silently dropped (a defence in depth
	 *  for filter/dedup bugs upstream). */
	setHistoryOverlay(
		paneId: string,
		items: string[],
		selectedIndex: number,
		anchorRow: number,
		anchorCol: number,
		placeAbove: boolean,
	): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		const h = entry.handle as unknown as {
			setHistoryOverlay?: (
				items: string[],
				selectedIndex: number,
				anchorRow: number,
				anchorCol: number,
				placeAbove: boolean,
			) => void;
		};
		h.setHistoryOverlay?.(items, selectedIndex, anchorRow, anchorCol, placeAbove);
		this._lastHistoryOverlayCall.set(paneId, {
			items: [...items],
			selectedIndex,
			anchorRow,
			anchorCol,
			placeAbove,
		});
		this.wake();
	}

	/** §1.34 — remove the shell-history overlay. Called from RidgePane
	 *  on Enter / ArrowRight / Escape / focus loss. */
	clearHistoryOverlay(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		const h = entry.handle as unknown as { clearHistoryOverlay?: () => void };
		h.clearHistoryOverlay?.();
		this._lastHistoryOverlayCall.delete(paneId);
		this.wake();
	}

	/** E2E probe — current wasm shell-history overlay state (mirror of the
	 *  most-recent `setHistoryOverlay` call). Returns `open: false` and
	 *  empty items when the overlay isn't visible. Replaces the prior
	 *  DOM-querySelector approach used while the popup was a Svelte
	 *  `<TerminalHistoryPopup>` element. */
	historyOverlayState(paneId: string): {
		open: boolean;
		items: string[];
		selectedIndex: number;
		anchorRow: number;
		anchorCol: number;
		placeAbove: boolean;
	} {
		const last = this._lastHistoryOverlayCall.get(paneId);
		if (!last) {
			return {
				open: false,
				items: [],
				selectedIndex: -1,
				anchorRow: 0,
				anchorCol: 0,
				placeAbove: true,
			};
		}
		return {
			open: true,
			items: [...last.items],
			selectedIndex: last.selectedIndex,
			anchorRow: last.anchorRow,
			anchorCol: last.anchorCol,
			placeAbove: last.placeAbove,
		};
	}

	/** E2E probe — last `setPreedit` call for the given pane, or `null`
	 *  if `clearPreedit` was the most recent call (or no preedit yet).
	 *  Specs use this to assert overlay-cell == textarea-cell == anchor. */
	lastPreeditCall(paneId: string): { row: number; col: number; text: string } | null {
		return this._lastPreeditCall.get(paneId) ?? null;
	}

	/** Whether the pane has DEC mouse reporting enabled (?1000/?1002/?1003).
	 *  When true, pointer events should be forwarded to the TUI instead of
	 *  being consumed by ridge's selection/link handlers. */
	isMouseReporting(paneId: string): boolean {
		const e = this.panes.get(paneId);
		if (!e) return false;
		try {
			return (e.kernel as unknown as { isMouseReporting?: () => boolean }).isMouseReporting?.() ?? false;
		} catch {
			return false;
		}
	}

	/** Whether the pane is in inline-TUI mode (Ink-style app on primary screen,
	 *  e.g. opencode). Like alt-screen mode, wheel events should pass through to
	 *  the PTY so the TUI can handle its own scrolling. */
	isInlineTuiActive(paneId: string): boolean {
		const e = this.panes.get(paneId);
		if (!e) return false;
		try {
			return (e.kernel as unknown as { isInlineTuiMode?: () => boolean }).isInlineTuiMode?.() ?? false;
		} catch {
			return false;
		}
	}

	/** §1.35 — force-leave alt screen on the kernel when the PTY process
	 *  exits while a TUI is still in alt screen mode. Called from the
	 *  `pane-pty-closed` handler before spawning a new shell so the new
	 *  shell's output goes to the primary screen, not the alt buffer. */
	leaveAltScreen(paneId: string): void {
		const e = this.panes.get(paneId);
		if (!e || e.parked) return;
		try {
			(e.kernel as unknown as { leaveAltScreen?: () => void }).leaveAltScreen?.();
		} catch {
			// kernel gone — nothing to clear
		}
	}

	/** §1.31 (2026-05-19): DECCKM application-cursor-keys mode (`?1`).
	 *  When true, the running program has explicitly declared "I own the
	 *  arrow keys" — Ink, vim, less, GNU readline-with-vi-mode, PSReadLine
	 *  all set this when their line editor is active. Unlike the inline-TUI
	 *  heuristic this signal has NO time decay; it stays on until the app
	 *  resets it (or the terminal is reset). Used by tuiGate.isTuiActive
	 *  as the protocol-level dominant signal for arrow-key ownership. */
	isAppCursorKeys(paneId: string): boolean {
		const e = this.panes.get(paneId);
		if (!e) return false;
		try {
			return (e.kernel as unknown as { isAppCursorKeys?: () => boolean }).isAppCursorKeys?.() ?? false;
		} catch {
			return false;
		}
	}

	/** §1.31 (2026-05-19): DEC text-cursor-enable mode (`?25`).
	 *  Returns true when the cursor is visible (the default). A hidden
	 *  cursor (`?25l`) is a strong "app is doing custom rendering" hint
	 *  used by the sticky branch of tuiGate.isTuiActive to decide whether
	 *  the user is still inside a TUI or genuinely back at a shell prompt.
	 *  Defaults to true on a missing pane so attach races don't false-
	 *  positive as TUI. */
	isCursorVisible(paneId: string): boolean {
		const e = this.panes.get(paneId);
		if (!e) return true;
		try {
			return (e.kernel as unknown as { isCursorVisible?: () => boolean }).isCursorVisible?.() ?? true;
		} catch {
			return true;
		}
	}

	/** §p4 (2026-05-22): does the worker-renderer path own panes' canvases
	 *  on this app instance? When true, RidgePane should call
	 *  `canvas.transferControlToOffscreen()` + `WorkerHostedRenderer.bindCanvas`
	 *  at mount instead of letting `attach()` construct (and the rAF loop
	 *  drive) a main-thread `RenderHandle`. The decision is process-wide
	 *  for now — the flag and the singleton are both global — so callers
	 *  can query without a `paneId`. Mid-session flag toggles take effect
	 *  on the next pane attach; already-attached panes keep their initial
	 *  decision until detach. */
	usingWorkerRenderer(): boolean {
		return isWorkerRenderingEnabled() && getWorkerRenderer() !== null;
	}

	/** §1.33 (2026-05-22): kernel-side gate for the shell-history popup.
	 *  Returns true ONLY when the wasm kernel is confident a normal shell
	 *  prompt owns the input line on this pane — every known TUI signal
	 *  short-circuits to false, AND a 2-second sticky window holds the
	 *  gate closed after any signal clears so the popup can't race in
	 *  between TUI repaints (the original Claude-Code-arrow-key-hijack
	 *  symptom). Replaces the JS-side `tuiGate` for the popup decision;
	 *  the wheel handler still uses `tuiGate` because its semantics
	 *  (mouse reporting forwarding) differ. Defaults to `false` on a
	 *  missing pane so attach races never open the popup. */
	shouldAllowShellHistory(paneId: string): boolean {
		const e = this.panes.get(paneId);
		if (!e) return false;
		try {
			return (
				e.kernel as unknown as { shouldAllowShellHistory?: () => boolean }
			).shouldAllowShellHistory?.() ?? false;
		} catch {
			return false;
		}
	}

	/** §1.32 Wave F (2026-05-20): mark the start of the user's current
	 *  shell-input line by capturing the kernel cursor position. Called
	 *  by `RidgePane` the first time the user types a printable / paste
	 *  / Tab event after the previous line was submitted. Idempotent:
	 *  subsequent calls while `inputStartRow` is already set are no-ops,
	 *  so spamming this from every keystroke is safe. */
	markInputStart(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		if (entry.inputStartRow != null) return;
		entry.inputStartRow = entry.kernel.cursorRow();
		entry.inputStartCol = entry.kernel.cursorCol();
	}

	/** §1.32 Wave F: clear the input-start marker. Called on Enter
	 *  (line submitted; the shell will print a new prompt and the
	 *  next typing will re-mark). Also safe to call defensively
	 *  whenever the pane state is reset. */
	clearInputStart(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.inputStartRow = null;
		entry.inputStartCol = null;
	}

	/** §1.32 Wave F: read the actual shell-input string from the
	 *  kernel grid — the ground truth that the keystroke mirror can
	 *  only approximate. Returns null when no input has been observed
	 *  yet at the current prompt, when the cursor has moved to a row
	 *  other than the input-start row (multi-row input / wrap not
	 *  modelled), or when the cursor jumped behind the input start
	 *  (prompt redrew, e.g. Ctrl+L clear).
	 *
	 *  Callers (notably the history-pick replay path) should fall back
	 *  to the keystroke mirror when this returns null. */
	readShellInputSnapshot(paneId: string): InputBufferState | null {
		const entry = this.panes.get(paneId);
		if (!entry) return null;
		const startRow = entry.inputStartRow;
		const startCol = entry.inputStartCol;
		if (startRow == null || startCol == null) return null;

		const k = entry.kernel as unknown as {
			cursorRow: () => number;
			cursorCol: () => number;
			cols: () => number;
			cellsAt: (row: number, col: number, len: number) => Array<{ ch: string; width: number }>;
		};
		const cursorRow = k.cursorRow();
		const cursorCol = k.cursorCol();
		// Multi-row input (long command that wrapped, or cursor moved
		// to another row via PageUp etc.) — not handled. Caller falls
		// back to keystroke mirror.
		if (cursorRow !== startRow) return null;
		// Cursor jumped behind input start — prompt was redrawn under
		// us (Ctrl+L, screen clear). Snapshot is invalid.
		if (cursorCol < startCol) return null;

		const totalCols = k.cols();
		const preCells = k.cellsAt(startRow, startCol, cursorCol - startCol);
		const postCells = k.cellsAt(startRow, cursorCol, totalCols - cursorCol);
		const snap = reconstructInputSnapshot(preCells, postCells);
		return { text: snap.text, cursorCol: snap.cursorCol };
	}

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
		// Container has CSS `padding: Npx` (set by setPadding); absolute-
		// positioned IME helper measures `left/top` from the padding-box
		// while the canvas lays out inside the content-box. Add `pad` so
		// (col=0, row=0) returns the canvas top-left, not N px above-left.
		const pad = e.lastFitPaddingPx ?? e.lastAppliedPaddingPx ?? 0;
		return {
			x: Math.round(col * e.cellW) + pad,
			y: Math.round(row * e.cellH) + pad,
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

		// Container CSS `padding: Npx` shifts the canvas inward into the
		// content-box; the IME helper is `position: absolute` and measures
		// `left/top` from the padding-box edge. Compensate by adding pad
		// to both axes so the returned coords land over the canvas cursor
		// instead of N px above-left of it.
		const pad = e.lastFitPaddingPx ?? e.lastAppliedPaddingPx ?? 0;
		const pickAt = (row: number, col: number) => {
			const r = Math.min(row, Math.max(0, rows - 1));
			const c = Math.min(col, Math.max(0, cols - 1));
			return {
				x: Math.round(c * e.cellW) + pad,
				y: Math.round(r * e.cellH) + pad,
				cellW: e.cellW,
				cellH: e.cellH,
				fontSizePx: this.opts.fontSizePx,
			};
		};

		const k = e.kernel as unknown as {
			lastAbsCsiPosition?: () => { row: number; col: number; atMs: number } | null;
			isAltScreen?: () => boolean;
			isInlineTuiMode?: () => boolean;
		};
		const isAlt = k.isAltScreen?.() === true;
		const isInlineTui = k.isInlineTuiMode?.() === true;

		// Priority for TUI scenarios (alt-screen / inline-TUI like Ink
		// based apps — opencode, claude code): `lastAbsCsiPosition`
		// wins over `imeAnchor`. Ink's frame ends with a CHA `\x1b[G`
		// or CUP that parks the cursor at the user's input column;
		// that's the stable "where the next character lands" signal.
		// `imeAnchor` reads `kernel.cursor{Row,Col}` after a RAF, but
		// in Ink the live cursor may have walked through a spinner /
		// hint row mid-frame and the RAF picked up the wrong cell —
		// so the preedit textarea anchored on `imeAnchor` no longer
		// tracks the visible input position. Fall back to `imeAnchor`
		// (post-PSReadLine-echo cursor) only when we're NOT inside a
		// TUI, where it correctly tracks shell typing.
		if ((isAlt || isInlineTui) && typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi && Date.now() - csi.atMs < ABS_CSI_DECAY_MS) {
				return pickAt(csi.row, csi.col);
			}
		}

		const anchor = e.imeAnchor;
		if (anchor) return pickAt(anchor.row, anchor.col);

		// Non-TUI fallback: try lastAbsCsiPosition even outside the
		// TUI gate, then live cursor. Older wasm bundles without
		// `lastAbsCsiPosition` fall through cleanly.
		if (typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi && Date.now() - csi.atMs < ABS_CSI_DECAY_MS) {
				return pickAt(csi.row, csi.col);
			}
		}
		return this.cursorPixelPosition(paneId);
	}

	/** Row/col version of `inputAnchorPixelPosition`. Resolved with the
	 *  SAME fallback chain (alt-screen / inline-TUI → recent
	 *  `lastAbsCsiPosition` → `imeAnchor` → live cursor) so the IME
	 *  preedit-overlay code in RidgePane lands its CUP at the exact
	 *  cell the user is "really" typing at — not wherever a mid-frame
	 *  Ink spinner has parked the kernel cursor for the moment. */
	inputAnchorCell(paneId: string): { row: number; col: number } | null {
		const e = this.panes.get(paneId);
		if (!e) return null;
		const ABS_CSI_DECAY_MS = 2_000;
		const k = e.kernel as unknown as {
			cursorRow: () => number;
			cursorCol: () => number;
			lastAbsCsiPosition?: () => { row: number; col: number; atMs: number } | null;
			isAltScreen?: () => boolean;
			isInlineTuiMode?: () => boolean;
		};
		const isAlt = k.isAltScreen?.() === true;
		const isInlineTui = k.isInlineTuiMode?.() === true;
		// Alt-screen / inline-TUI: the LAST absolute-positioning CSI is
		// the most reliable input-cell signal, even if it's "old". Ink /
		// claude / opencode park the cursor at the user's input column
		// at the end of every render frame; when the app goes idle (no
		// spinner, no animation), it stops emitting CSI but the cursor
		// stays exactly where the last frame left it — at the input
		// cell. The 2 s decay used to demote a stale CSI in favour of
		// the live `kernel.cursor*()`, but in alt-screen the live
		// cursor and the last CSI position are the SAME thing (no
		// other writes happen between user keystrokes), so age doesn't
		// matter. Skip the decay so a quiet Ink TUI still gets the
		// right anchor.
		if ((isAlt || isInlineTui) && typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi) return { row: csi.row, col: csi.col };
		}
		if (e.imeAnchor) return { row: e.imeAnchor.row, col: e.imeAnchor.col };
		if (typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi && Date.now() - csi.atMs < ABS_CSI_DECAY_MS) {
				return { row: csi.row, col: csi.col };
			}
		}
		return { row: k.cursorRow(), col: k.cursorCol() };
	}

	/** Unified IME anchor — single source for textarea position AND
	 *  preedit overlay cell. Returns the resolved (row, col) from
	 *  `inputAnchorCell` together with the matching pixel rect computed
	 *  with the same `lastFitPaddingPx` compensation the pixel resolver
	 *  uses. Both consumers (DOM textarea, wasm preedit overlay) must
	 *  read from this so they can't drift apart by even a cell. */
	inputAnchorResolved(
		paneId: string,
	): {
		row: number;
		col: number;
		x: number;
		y: number;
		cellW: number;
		cellH: number;
		fontSizePx: number;
	} | null {
		const e = this.panes.get(paneId);
		if (!e || e.cellW <= 0 || e.cellH <= 0) return null;
		const cell = this.inputAnchorCell(paneId);
		if (!cell) return null;
		const rows = e.kernel.rows();
		const cols = e.kernel.cols();
		const r = Math.min(cell.row, Math.max(0, rows - 1));
		const c = Math.min(cell.col, Math.max(0, cols - 1));
		const pad = e.lastFitPaddingPx ?? e.lastAppliedPaddingPx ?? 0;
		return {
			row: r,
			col: c,
			x: Math.round(c * e.cellW) + pad,
			y: Math.round(r * e.cellH) + pad,
			cellW: e.cellW,
			cellH: e.cellH,
			fontSizePx: this.opts.fontSizePx,
		};
	}

	/** Force a full-frame redraw on the next rAF tick (§1.27 fix). Used
	 *  by `RidgePane::onCompositionEnd` to repaint cells underneath the
	 *  IME helper textarea — without this, Canvas2D's per-row hash diff
	 *  may skip redrawing rows whose `cells` are unchanged but whose
	 *  pixels were smeared by the opaque `.is-composing` overlay. WebGPU
	 *  already redraws every row per tick, so this is a no-op there
	 *  beyond a single extra wake. */
	/** Refresh specific panes by id — invalidates render cache and wakes
	 *  the rAF loop. Used after split resize to redraw affected panes. */
	forceFullRedrawFor(ids: string[]): void {
		for (const id of ids) {
			const entry = this.panes.get(id);
			if (!entry || entry.parked) continue;
			entry.handle?.invalidateAll();
		}
		if (ids.length) this.wake();
	}

	forceFullRedraw(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		entry.handle?.invalidateAll();
		this.wake();
	}

	/** Same as `forceFullRedraw` but applied across every attached pane.
	 *  Used when a global font event lands — e.g. Twemoji finishes loading
	 *  AFTER panes have already been streaming output. Each pane's
	 *  `invalidateAll` clears the WebGPU `GlyphAtlas` LRU and resets the
	 *  Canvas2D row-hash snapshot so the next frame re-rasterizes from
	 *  scratch against the new font stack. Parked panes are skipped (their
	 *  handles have been freed); they pick up the new font on unpark. */
	/** Invalidate all panes in a specific workspace. Called after split
	 *  resize drag completes to refresh all affected panes. */
	invalidateWorkspace(workspaceId: string): void {
		for (const entry of this.panes.values()) {
			if (entry.parked) continue;
			if (entry.workspaceId !== workspaceId) continue;
			entry.handle?.invalidateAll();
		}
		this.wake();
	}

	invalidateAllPanes(): void {
		for (const entry of this.panes.values()) {
			if (entry.parked) continue;
			entry.handle?.invalidateAll();
		}
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
		// Expose the terminal's actual font stack as a CSS custom
		// property so DOM overlays positioned over the canvas (the IME
		// helper textarea, in particular) can render their text in the
		// same typeface as the canvas glyphs. Without this the
		// preedit text sits in the page's default Inter sans-serif
		// while the surrounding terminal cells are JetBrains Mono /
		// Cascadia Code etc., so the in-progress IME text looks
		// nothing like an inline input field — visibly mismatched
		// character widths, weights, and baselines.
		if (typeof document !== 'undefined') {
			document.documentElement.style.setProperty('--rg-term-font-family', family);
			document.documentElement.style.setProperty('--rg-term-font-size', `${sizePx}px`);
		}
		const dpr = window.devicePixelRatio || 1;
		for (const entry of this.panes.values()) {
			// Skip parked entries — their handle has been freed. They'll
			// pick up the new font on the next unpark via this.opts.
			if (entry.parked) continue;
			// §p4 ITER 1c / ITER 8 — when the worker-renderer owns this
			// pane's canvas, the main-thread handle is null. Push the
			// font into the worker so its `RenderHandle.configure`
			// re-measures, and re-seed entry.cellW / cellH from the
			// metrics it returns (then refit so the new column count
			// reaches the kernel + PTY).
			if (!entry.handle) {
				const paneId = entry.paneId;
				workerRendererBridge.setFont(paneId, family, sizePx, dpr, (cellW, cellH) => {
					const ent = this.panes.get(paneId);
					if (!ent) return;
					ent.cellW = quantizeCellSize(cellW, dpr);
					ent.cellH = quantizeCellSize(cellH, dpr);
					ent.lastConfiguredDpr = dpr;
					void this.fitPane(ent);
				});
				continue;
			}
			const [w, h] = entry.handle.configure(family, sizePx, dpr) as
				| [number, number]
				| Float32Array;
			entry.cellW = quantizeCellSize(Number(w), dpr);
			entry.cellH = quantizeCellSize(Number(h), dpr);
			entry.lastConfiguredDpr = dpr;
			entry.handle.invalidateAll();
			void this.fitPane(entry);
		}
		this.wake();
	}

	/** Apply theme overrides to all panes. */
	setTheme(theme: Record<string, string>): void {
		this.opts.theme = theme;
		let applied = 0;
		let parked = 0;
		for (const entry of this.panes.values()) {
			// Parked panes pick up the theme on the next unpark via this.opts.
			if (entry.parked) { parked++; continue; }
			entry.handle?.applyDefaultTheme();
			entry.handle?.applyTheme(theme);
			// Theme change doesn't bump kernel dirty, so the next frame's
			// `dirty=false` branch would call `recordCachedOnly()` which
			// replays the previous frame's CellInstance buffer — that
			// buffer has the OLD theme's bg/fg baked into every quad.
			// Drop the cache so the next frame goes through full render
			// and re-reads `theme.bg` for the clear quad and per-cell bgs.
			entry.handle?.invalidateAll();
			applied++;
		}
		// Surface-host LoadOp::Clear color is sampled from JS `themeBg`
		// every begin_frame, but only painted when `needs_initial_clear`.
		// Force one initial-clear so the gutter pixels around per-pane
		// scissors also get repainted with the new bg.
		this._invalidateHost();
		if (typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_THEME_TRACE') === '1') {
			// eslint-disable-next-line no-console
			console.debug(`[theme-trace] setTheme applied=${applied} parked=${parked} totalKeys=${Object.keys(theme).length} bg=${theme.background ?? '∅'}`);
		}
		this.wake();
	}

	/**
	 * Bypass the trailing-edge debounce and run a fit synchronously.
	 *
	 * Used after a discrete layout-changing operation (split / dock /
	 * close) where the caller already knows the container's new size is
	 * what the kernel grid must match — there's no further `viewportChanged`
	 * coming, so waiting out `RESIZE_SETTLE_MS` only delays the right
	 * answer. Cancels any pending debounced fit so we don't run twice.
	 *
	 * No-op when the pane is unknown or parked; the next attach/unpark
	 * will fire its own initial fit. Fire-and-forget: the underlying
	 * `fitPane` is async (awaits backend `resize_pane`) but callers
	 * generally don't need to await — the kernel + PTY sync happens on
	 * the same frame the next render reads from.
	 */
	fitPaneNow(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
			entry.pendingFitTimer = null;
		}
		void this.fitPane(entry);
	}

	/**
	 * Container-size changed. Trailing-edge debounce: hold the actual
	 * fit until the user stops resizing.
	 *
	 * Trigger to "settle now" is either of:
	 *   a. `RESIZE_SETTLE_MS` (1000 ms) elapses with no further
	 *      viewportChanged events — user paused mid-drag.
	 *   b. A global `pointerup` lands — user released the splitter /
	 *      window-edge handle (see `_ensureResizeReleaseListener`).
	 *
	 * Until one of those fires we do NOTHING — no scissor update, no
	 * kernel grid resize, no PTY SIGWINCH. The visual terminal stays
	 * exactly where it was at drag start while CSS reflows the
	 * container around it. This matches the explicit UX ask: "during
	 * resize the terminal content should not follow in real time; only
	 * when the mouse pauses for 1 s OR the user releases the button
	 * should the content snap into place against the divider".
	 *
	 * The previous 120 ms debounce eagerly fit on every brief pause
	 * mid-drag, producing the "TUI drawing 错位 / 不完整" symptom: a
	 * partial re-fit landed during continuous motion, then drift
	 * accumulated as the user kept dragging.
	 *
	 * Kernel + PTY race-correctness still applies: in-flight bytes
	 * carry absolute cursor positions valid only under one given grid,
	 * so collapsing the whole drag into a single end-of-drag fit is
	 * strictly safer than the prior behaviour.
	 *
	 * Initial fit at attach() bypasses the debounce — synchronous
	 * resize, no concurrent in-flight bytes.
	 */
	viewportChanged(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		this._ensureResizeReleaseListener();
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
		}
		entry.pendingFitTimer = setTimeout(() => {
			entry.pendingFitTimer = null;
			const e = this.panes.get(paneId);
			// Re-check parked: a park() call could have come in during
			// the debounce window, freeing entry.handle.
			if (!e || e.parked) return;
			void this.fitPane(e);
		}, RESIZE_SETTLE_MS);
	}

	/** Install a document-level `pointerup` listener (once) that flushes
	 *  every pane's pending fit timer the moment the user releases the
	 *  mouse button — so drag-end snaps immediately without waiting out
	 *  the full `RESIZE_SETTLE_MS`. Idempotent; teardown happens in
	 *  `stopRafLoop` so the singleton doesn't leak listeners between
	 *  detach-all → re-attach cycles.
	 *
	 *  Critical: the listener MUST NOT flush when the pointerup is the
	 *  release end of a click inside a pane. Flushing there fires
	 *  `kernel.resize` + PTY SIGWINCH between the pane's own pointerdown
	 *  and pointerup handlers — which delivers the TUI mouse-release
	 *  byte against a freshly-resized grid, so the release cell snaps
	 *  to wrong coordinates. In opencode / Claude Code / other Ink TUIs
	 *  the symptom is "TUI mouse capture is broken: clicks land in the
	 *  wrong place, or selection menus react oddly". We gate via
	 *  `e.target.closest('[data-rg-pane-id]')`: pane-internal pointerups
	 *  belong to the pane's own handler chain; only splitter / window-
	 *  edge / sidebar releases (DOM outside any pane) drive the flush. */
	private _ensureResizeReleaseListener(): void {
		if (this._resizeReleaseListener !== null) return;
		if (typeof document === 'undefined') return;
		this._resizeReleaseListener = (e?: Event) => {
			const tgt = (e as PointerEvent | undefined)?.target as Element | null | undefined;
			if (tgt && tgt.closest && tgt.closest('[data-rg-pane-id]')) {
				// Pane-internal release — leave the pane's own pointerup
				// handler to do its TUI mouse-release forwarding without
				// a concurrent grid resize racing it.
				return;
			}
			this._flushPendingFits();
		};
		document.addEventListener('pointerup', this._resizeReleaseListener, { passive: true });
		document.addEventListener('pointercancel', this._resizeReleaseListener, { passive: true });
	}

	/** Run any pane's pending fit immediately, clearing its timer.
	 *  Called from the `pointerup` listener and from `stopRafLoop`. */
	private _flushPendingFits(): void {
		for (const entry of this.panes.values()) {
			if (entry.pendingFitTimer === null) continue;
			clearTimeout(entry.pendingFitTimer);
			entry.pendingFitTimer = null;
			if (entry.parked) continue;
			void this.fitPane(entry);
		}
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
		// Track the container rect (host mode only) so we can later
		// redistribute the rounding leftover into symmetric padding —
		// see the "center cells in content-box" step below.
		let containerWCss = 0;
		let containerHCss = 0;
		if (this._isHostMode(entry)) {
			const cr = entry.container.getBoundingClientRect();
			containerWCss = cr.width;
			containerHCss = cr.height;
			// Use the user-configured base padding as a floor — never read
			// the live CSS padding here. The live value gets rewritten at
			// the end of fitPane to absorb the cell rounding leftover, and
			// reading it back would feed a slightly inflated padding into
			// the next col/row computation, slowly drifting the grid size
			// on every fit. opts.paddingPx is the single source of truth
			// for "how much margin should we *at least* leave around the
			// grid"; the actual on-screen padding ends up >= that.
			const basePad = Math.max(
				0,
				Math.min(64, Math.round((entry.lastAppliedPaddingPx ?? this.opts.paddingPx) || 0)),
			);
			wCss = Math.max(0, Math.floor(cr.width - 2 * basePad));
			hCss = Math.max(0, Math.floor(cr.height - 2 * basePad));
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

		const dpr = window.devicePixelRatio || 1;

		// DPR drift since the last configure() (typical cause: user dragged
		// the window across monitors of different DPI). The renderer
		// rounds `cell_css * dpr` to a whole device-px every frame against
		// the *new* dpr, so the JS-side cellW/cellH would silently
		// disagree — `Math.floor(x / oldCellW)` then snaps the rightmost
		// hover column out of range. Re-configure so both sides stay in
		// lock-step.
		if (entry.lastConfiguredDpr !== dpr && entry.handle) {
			const [w, h] = entry.handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as
				| [number, number]
				| Float32Array;
			entry.cellW = quantizeCellSize(Number(w), dpr);
			entry.cellH = quantizeCellSize(Number(h), dpr);
			entry.lastConfiguredDpr = dpr;
		}

		// Cells fit into the container; round DOWN to avoid drawing past
		// the right/bottom edge.
		const cols = Math.max(1, Math.floor(wCss / entry.cellW));
		let rows = Math.max(1, Math.floor(hCss / entry.cellH));

		// Equal padding on all four sides, using the horizontal-center
		// value as the canonical inset. Splitting horizontal and
		// vertical leftover independently made the vertical padding
		// visibly thicker than the horizontal one (cellH > cellW means
		// a larger vertical rounding remainder). For a single-pane
		// terminal we want symmetric top/bottom/left/right insets, so
		// the horizontal-centered pad is reused for the vertical sides
		// and `rows` is re-floored to fit inside the now-shrunk
		// content-box. Any residual vertical leftover (< cellH px) sits
		// below the cells and gets painted as bg by the scissor —
		// visually the bottom padding is at most one half-cell taller
		// than the top, well below the perceptual threshold the user
		// flagged. Canvas2D mode skips this — its canvas is sized to
		// the container directly with no padding budget to redistribute.
		if (this._isHostMode(entry)) {
			const cellsW = cols * entry.cellW;
			const padAll = Math.max(0, (containerWCss - cellsW) / 2);
			rows = Math.max(1, Math.floor((containerHCss - 2 * padAll) / entry.cellH));
			entry.container.style.padding = `${padAll}px`;
			// Record the ACTUAL written CSS padding separately from the
			// user-preference value. `pickAt` etc. need the on-screen
			// value to align overlays with the visible cursor, while
			// the NEXT fitPane needs the user's basePad as a floor
			// (otherwise basePad drifts toward padAll on every fit and
			// the container shifts a few px each run — visible as the
			// "shell prompt loads then nudges downward" jolt at startup).
			entry.lastFitPaddingPx = padAll;
			this._recomputeViewport(entry);
		} else {
			entry.handle?.resize(wCss, hCss, dpr);
		}

		// Self-healing: also compare against the kernel's actual grid. If a
		// prior fitPane ran while the pty-delta Channel wasn't wired up yet
		// (race: attach() schedules rAF fit BEFORE RidgePane's ensurePtyBridge
		// + setPaneDeltaMode complete), the backend's Resize delta was dropped
		// and the kernel stays at its compile-time default (80×24) while
		// lastReportedRows/Cols already cached the intended size. Without this
		// extra guard, sizeChanged stays false on every subsequent fit and
		// the new pane never reaches the correct grid → visible as the
		// "split 后新终端不填满分区" bug.
		const kernelRows = entry.kernel.rows();
		const kernelCols = entry.kernel.cols();
		const sizeChanged =
			rows !== entry.lastReportedRows ||
			cols !== entry.lastReportedCols ||
			rows !== kernelRows ||
			cols !== kernelCols;
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

		// P4.6 Part B (2026-05-22) — mirror sizing into the render worker.
		// First fit: attach (init). Subsequent fits: resize. The decision
		// is delegated to a pure helper so it is independently
		// unit-testable (see `workerRendererBridge.test.ts` — Iter 14).
		const workerAction = workerLifecycleOnFit({
			paneId: entry.paneId,
			rows,
			cols,
			dpr: entry.lastConfiguredDpr,
			attached: this.workerAttached,
			isActive: workerRendererBridge.isActive(),
		});
		switch (workerAction.kind) {
			case 'attach':
				workerRendererBridge.attach(
					entry.paneId,
					workerAction.rows,
					workerAction.cols,
					workerAction.dpr,
				);
				this.workerAttached.add(entry.paneId);
				break;
			case 'resize':
				// §p4 ITER 7 (2026-05-22) — also pass CSS dims so the
				// worker can resize its `RenderHandle` backing buffer.
				// fitPane's local `wCss` / `hCss` (computed above from
				// the container's bounding rect minus padding) are the
				// right values.
				workerRendererBridge.resize(
					entry.paneId,
					workerAction.rows,
					workerAction.cols,
					workerAction.dpr,
					wCss,
					hCss,
				);
				break;
			case 'noop':
				break;
		}

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

		// §pane-resize-reflow (2026-05-09): Enhanced diagnostic for resize
		// behavior debugging. Logs the key decision factors so we can
		// correlate visible symptoms (truncation, cursor drift) with the
		// internal state at resize time.
		if (import.meta.env?.DEV && typeof console.debug === 'function') {
			const lastAbsCsiPos = entry.kernel.lastAbsCsiPosition();
			const diag = {
				paneId: entry.paneId,
				old: { rows: entry.lastReportedRows, cols: entry.lastReportedCols },
				new: { rows, cols },
				isAlt,
				isInlineTui,
				wipeBeforePty,
				cursorVisible: entry.kernel.isCursorVisible(),
				heuristic: lastAbsCsiPos ? {
					absCsiRow: lastAbsCsiPos.row,
					absCsiCol: lastAbsCsiPos.col,
					absCsiAt: lastAbsCsiPos.atMs,
				} : null,
			};
			console.debug('[ridge-term] resize decision', diag);
		}

		// P3.9.r (2026-05-20) → P4.4 (2026-05-21) — Rust parser is the
		// only mode now, so the mirror's kernel resize is always driven
		// by apply_delta(Resize) after the Rust-side PaneParser emits
		// the Resize delta. We must NOT call `entry.kernel.resize` here
		// directly — that would race the frame and risk Cells deltas
		// referencing the old/new grid mix. `entry.resizeHandler`
		// (resize_pane Tauri command) takes care of PTY master.resize +
		// PaneParser.resize + delta emit in one atomic sequence. The
		// `wipeBeforePty` knob from the WASM path is no longer
		// reachable; the Rust path's `set_pane_delta_mode` already
		// covers the equivalent "clean snapshot on resize" scenario via
		// force_full_reframe.
		void wipeBeforePty;
		await entry.resizeHandler?.(rows, cols, isAlt, isInlineTui);
		// Mirror resize will follow via apply_delta(Resize) in the
		// next pty-delta frame — handler emits it synchronously.
		// Drop the renderer's per-row hash snapshot the instant the kernel
		// grid changed shape. `entry.handle.resize(wCss,hCss,dpr)` above
		// only invalidates when the CSS surface dimensions changed; a
		// rows/cols change that doesn't move pixels (e.g. font drift
		// changes cell metrics without changing container px) would leave
		// stale row hashes and produce the "色块错位" symptom on a TUI
		// resize. Calling invalidateAll unconditionally here makes the
		// next sync render below — and the 150ms forceFullRedraw — both
		// start from a clean snapshot regardless of which path got us
		// here.
		entry.handle?.invalidateAll();
		entry.linkSpans.markDirty();

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
			entry.handle?.render(entry.kernel);
		} catch (err) {
			console.error('[ridge-term] post-resize render error', entry.paneId, err);
		}
		// §pane-resize-reflow (2026-05-09): ALL resize paths need a delayed
		// refresh, not just TUI. The synchronous inline render above paints
		// with whatever kernel state existed at the moment of resize, but
		// ConPTY's SIGWINCH delivery is async on Windows: both TUI apps
		// AND normal shells (PSReadLine, zsh, fish) emit prompt redraws
		// 30-100 ms after resize_pane returns. Without a delayed refresh,
		// Canvas2D's per-row dirty diff may show stale content.
		// 150ms covers both TUI redraws and shell prompt repaints.
		// `alive`-guarded against the pane being closed during the wait.
		{
			const targetPaneId = entry.paneId;
			setTimeout(() => {
				const e = this.panes.get(targetPaneId);
				if (!e || e.parked) return;
				this.forceFullRedraw(targetPaneId);
			}, 150);
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
				if (!document.hidden) {
					// On visibility-restore the swap chain may have been
					// recycled by Chromium / WebView2 while we were
					// hidden — force a full cache replay on the next
					// tick so the user doesn't briefly see fresh-zero
					// pixels (transparent → DOM parent) where rendered
					// terminal content was. Pairs with the idle-watchdog
					// invalidate so any code path that suspends the loop
					// repaints on resume.
					this._hostInvalidatePending = true;
					this.wake();
				}
			};
			document.addEventListener('visibilitychange', this.visibilityListener);
		}
		const tick = () => {
			// §P4 attribution — wrap the per-frame render body so the
			// `frame-time-attribution` spec can measure how much of the
			// rAF interval is paint (this measure) vs PTY event handlers
			// (rg.ptyText.feed / rg.ptyDelta.apply in ptyBridge.ts).
			// `perfMark` is a no-op unless `window.__RIDGE_PERF_TRACE`
			// is true, so production / dev pays one branch.
			perfMark('rg.frame.tick', () => {
			this.rafHandle = null;
			const perfNow = performance.now();
			// Use Date.now() for the dirty / blink queries: `RenderHandle.render`
			// reads `js_sys::Date::now()` internally, so the renderer's blink
			// phase and our pre-render `isDirty` must use the same epoch.
			const dateNow = Date.now();
			// Consume the host-invalidate flag at the start of the tick so
			// the upcoming cache-replay pass over every visible pane counts
			// as "real work" (the swap chain was wiped by LoadOp::Clear in
			// SurfaceHost::begin_frame; without per-pane repaints those
			// regions stay blank). Cleared so the next tick sleeps if no
			// new invalidate / dirty event lands in the meantime.
			const surfaceJustWiped = this._hostInvalidatePending;
			this._hostInvalidatePending = false;
			// P2.2 (2026-05-20): compute the per-frame order ONCE here so
			// the deferred-feed drain and the main render loop agree on
			// who goes first. Focused pane heads the list; remaining
			// panes rotate by `_rafRotationIndex` so no non-focused pane
			// gets perpetually starved at the tail.
			const frameOrder = this._renderOrder();
			// P2.1 (2026-05-20): drain bytes that prior `_feedNow` calls
			// spilled out of when their per-call time budget ran out. The
			// drain itself re-enters `_feedNow` with its own budget, so a
			// pane bursting 200 KB/sec consumes one ~16 KB chunk per frame
			// instead of monopolising the main thread for tens of ms. Run
			// BEFORE the dirty pre-pass so the kernel state the pre-pass
			// hashes against reflects this frame's freshly-fed bytes.
			this._drainDeferredFeeds(frameOrder);
			let anyRendered = false;
			let minDeadlineMs = Infinity;
			// §4b per-pane increment cache (2026-05-08): this pre-pass
			// collects PER-PANE dirty state (used to be one
			// `forceHostRenderAll` boolean for the whole frame). For
			// each visible host pane:
			//   - dirty=true  → main loop calls full `render()` (kernel
			//     traversal + cell instance encode + GPU upload + draw).
			//   - dirty=false → main loop calls `recordCachedOnly()`,
			//     which re-records the pane's previously-uploaded
			//     instance buffer with NO kernel traversal. Falls back
			//     to `render()` if the wasm bundle is too old to expose
			//     the export OR the cache was invalidated mid-frame.
			//
			// `LoadOp::Clear` on the host's first record wipes the
			// entire host canvas to bg, but per-pane scissor + every
			// pane (dirty or cached) records a draw inside its scissor,
			// so non-dirty pane regions get repainted from cached
			// instances — no flash. The pre-§4b "when ANY pane dirty,
			// EVERY pane re-encodes" multiplier is gone: typing in one
			// pane no longer makes 7 other panes traverse their grids.
			//
			// Open host frame iff any visible host pane needs to draw
			// (= any pane exists AND is visible). Idle ticks (no pane
			// dirty AND nothing in their caches changes) still skip
			// `beginFrame` because all-cached + no clear = nothing to
			// present — but that's a rare optimisation; the typical
			// case is "at least one pane dirty per tick".
			// §A.9 single-canvas (2026-05-08 follow-up): only one
			// workspace tab is visible at a time → exactly one
			// workspace's panes have non-zero bbox each tick. We pin
			// `activeWsId` to the first visible host pane found, then
			// render only that workspace's panes onto the shared global
			// host. Inactive workspaces' panes are skipped via the 0×0
			// bbox check (`_isContainerHidden`) — their kernels keep
			// receiving PTY output but no GPU work is paid for unseen
			// pixels. Switching workspaces simply changes WHICH panes
			// pass the bbox gate; the host's swap chain / pipeline never
			// reconfigures, so no black flash on switch.
			const dirtyByPane = new Map<string, boolean>();
			let activeWsId: string | null = null;
			let anyDirty = false;
			for (const entry of this.panes.values()) {
				if (entry.parked) continue;
				if (!this._isHostMode(entry)) continue;
				if (this._isContainerHidden(entry)) continue;
				// First visible host pane wins: pin to its workspace.
				if (activeWsId === null) activeWsId = entry.workspaceId;
				// Sanity: skip any pane from a different workspace that
				// somehow has non-zero bbox (shouldn't happen in normal
				// CSS toggle, but defensive against intermediate layout
				// states).
				if (entry.workspaceId !== activeWsId) continue;
				const handleAny = entry.handle as unknown as {
					isDirty?: (k: TerminalKernel, t: number) => boolean;
				};
				let d = true;
				if (typeof handleAny.isDirty === 'function') {
					try {
						d = handleAny.isDirty(entry.kernel, dateNow);
					} catch {
						d = true;
					}
				}
				dirtyByPane.set(entry.paneId, d);
				if (d) anyDirty = true;
			}
			let hostFrameOpen = false;
			let activeHost: SurfaceHostHandle | null = null;
			const themeBg = this._currentThemeBgRgba();
			if (activeWsId !== null) {
				activeHost = this._globalHostHandle();
				// §A.9: don't open the host frame eagerly here. With one
				// shared canvas, calling beginFrame + endFrame without
				// drawing any pane (e.g. every visible pane is in sync
				// mode, or every visible pane just unhid and would have
				// taken the §4a fitPane skip) submits a LoadOp::Clear-only
				// frame and the user sees a black flash. Open lazily
				// inside the pane loop, when we know at least one host
				// pane is about to draw.
			}
			void anyDirty;
			const ensureHostFrame = (): boolean => {
				if (hostFrameOpen) return true;
				if (!activeHost) return false;
				hostFrameOpen = activeHost.beginFrame(themeBg);
				return hostFrameOpen;
			};
			// P2.2 (2026-05-20): use the frame's ordered list (focused
			// pane first, then rotated non-focused) so the focused pane's
			// dirty rows get encoded + presented before any sibling pane
			// — visible win when the frame budget is tight (the focused
			// cursor doesn't stutter behind a busy non-focused pane).
			for (const entry of frameOrder) {
				// `frameOrder` already filtered parked entries, but the
				// kernel-freed dereference would crash hard so keep the
				// belt-and-suspenders guard.
				if (entry.parked) continue;
				// §4a workspace keep-alive: skip panes whose container has
				// 0 bbox (display:none on hidden workspace's wrapper).
				// Kernel still received PTY bytes; on the next tick where
				// container becomes visible, isDirty=true and a normal
				// render fires — atlas/RenderHandle already warm so it's
				// a single sub-16 ms frame, no black flash.
				if (this._isContainerHidden(entry)) {
					entry.wasHiddenLastTick = true;
					continue;
				}
				// §4a hidden→visible transition: ResizeObserver doesn't
				// reliably fire for display:none → display:flex flips on
				// every browser. Belt-and-suspenders: when this tick is
				// the first visible one after a hidden run, schedule a
				// fitPane so the kernel grid matches the (possibly
				// different) container size. We DO NOT skip render this
				// tick (§A.9): the host frame would otherwise open with
				// LoadOp::Clear and submit black with no scissors drawn.
				// fitPane's _recomputeViewport call sets the scissor
				// synchronously; the kernel-resize side effect is async
				// but only matters if cell grid actually changed (rare on
				// pure workspace switches — typically same layout).
				if (entry.wasHiddenLastTick) {
					entry.wasHiddenLastTick = false;
					void this.fitPane(entry);
				}
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
				// §4b per-pane increment cache: the pre-pass already
				// computed `dirty` for each visible host pane and stored
				// it in `dirtyByPane`. Re-reading would double the
				// `isDirty` cost for no benefit. Canvas2D panes still
				// compute dirty inline below — they don't participate in
				// the cache path (Canvas2D's per-row diff is already the
				// equivalent fast-path).
			const isHost = this._isHostMode(entry);
			// §P4.9 — worker-owned panes have `handle === null`; skip dirty
			// check and inline render — the worker's `createRenderer` handles
			// per-frame paint inside its own `applyDelta` handler.
		const hasHandle = entry.handle !== null;
		const handleAny = entry.handle as unknown as {
			isDirty?: (k: TerminalKernel, t: number) => boolean;
			nextBlinkDeadlineMs?: (k: TerminalKernel, t: number) => number;
			recordCachedOnly?: () => boolean;
		} | null;
		let dirty = true;
		if (isHost) {
			dirty = dirtyByPane.get(entry.paneId) ?? true;
		} else if (hasHandle && handleAny !== null && typeof handleAny.isDirty === 'function') {
			try {
				dirty = handleAny.isDirty(entry.kernel, dateNow);
			} catch {
				dirty = true;
			}
		}
				// §4.3 Phase B + §4b: host-mode panes participate in the
				// frame whenever ANY visible host pane needs to draw —
				// not just this one. WebView2's LoadOp::Load is not
				// reliable enough to preserve neighbour-pane pixels
				// across a present: when pane A renders (cursor blink,
				// PTY input, focus change), pane B's scissor region
				// frequently comes back as fresh-zero in the next
				// presented texture. So whenever the host frame opens
				// at all, every visible pane re-records its content —
				// dirty panes via full `render()`, others via the
				// cheap `recordCachedOnly()` path (one buffered GPU
				// draw call, no kernel grid sweep, sub-100μs each).
				// `surfaceJustWiped` covers the case where ALL panes
				// are non-dirty but a manual invalidate (theme change,
				// resize, park/unpark) wiped the canvas.
				// Canvas2D-mode panes still gate on per-pane `dirty`.
				// §A.9: `hostFrameOpen` is no longer the gate — the
				// frame is opened lazily inside the render path so we
				// don't submit a clear-only frame when nothing draws.
				const shouldRender = isHost
					? (activeHost !== null && (dirty || anyDirty || surfaceJustWiped))
					: dirty;
				// §A.4 — tick trace: dump cursor row's cells per frame. Lets us
				// answer "what does the kernel grid hold at the moment Canvas2D
				// samples it" — if the cells are right but render is wrong, it's
				// a render bug; if cells are wrong, it's a parser/feed bug.
				if (
					typeof localStorage !== 'undefined' &&
					localStorage.RIDGE_TICK_TRACE === '1'
				) {
					try {
						const kAny = entry.kernel as unknown as {
							cursorRow?: () => number;
							cursorCol?: () => number;
							isInlineTuiMode?: () => boolean;
							cellsAt?: (row: number, col: number, len: number) => Array<{
								col: number;
								ch: string;
								width: number;
							}>;
						};
						const cr = typeof kAny.cursorRow === 'function' ? kAny.cursorRow() : -1;
						const cc = typeof kAny.cursorCol === 'function' ? kAny.cursorCol() : -1;
						const inTui =
							typeof kAny.isInlineTuiMode === 'function'
								? kAny.isInlineTuiMode()
								: false;
						let rowDump = '';
						if (cr >= 0 && typeof kAny.cellsAt === 'function') {
							const cells = kAny.cellsAt(cr, 0, 80);
							rowDump = cells
								.map((c) => (c.ch === ' ' ? '·' : c.ch))
								.join('');
						}
						const ts = performance.now().toFixed(1);
						const id = entry.paneId.slice(0, 8);
						// eslint-disable-next-line no-console
						// console.debug(
						// 	`[tick-trace][${ts}ms][${id}][cur=(${cr},${cc}) inlineTui=${inTui} dirty=${dirty} render=${shouldRender}] row${cr}="${rowDump}"`,
						// );
					} catch {
						/* missing wasm export → skip the trace silently */
					}
				}
				if (shouldRender) {
					// §A.9: open the global host frame lazily, on the
					// first host pane that's actually about to draw this
					// tick. If every pane skips (sync mode, etc.), the
					// frame stays unopened and the canvas keeps its last
					// presented pixels — no LoadOp::Clear flash.
					if (isHost && !ensureHostFrame()) {
						// Surface lost / not yet inited — skip this pane.
						continue;
					}
					try {
						// §4b per-pane increment cache: visible host
						// pane that the pre-pass marked NOT dirty →
						// re-record cached instances without kernel
						// traversal. Canvas2D panes always go through
						// full render (their fast-path is the per-row
						// dirty diff inside `render` itself).
						let usedCache = false;
						if (
							isHost &&
							!dirty &&
							handleAny !== null &&
							typeof handleAny.recordCachedOnly === 'function'
						) {
							try {
								usedCache = handleAny.recordCachedOnly();
							} catch {
								usedCache = false;
							}
						}
						if (!usedCache) {
							entry.handle?.render(entry.kernel);
						}
						anyRendered = true;
					} catch (err) {
						// Don't let one pane's render error kill the whole loop.
						console.error('[ridge-term] render error', entry.paneId, err);
					}
				}
				if (hasHandle && handleAny !== null && typeof handleAny.nextBlinkDeadlineMs === 'function') {
					try {
						const d = handleAny.nextBlinkDeadlineMs(entry.kernel, dateNow);
						if (Number.isFinite(d) && d < minDeadlineMs) minDeadlineMs = d;
					} catch {
						// ignore — watchdog cap below covers us
					}
				}
			}
			// §A.8: close the active workspace's host frame.
			if (hostFrameOpen && activeHost) {
				try {
					activeHost.endFrame();
				} catch (err) {
					console.error('[ridge-term] surfaceHost.endFrame error', err);
				}
			}
			// P1.3 (2026-05-19): surface scroll-state diffs to RidgePane
			// subscribers AFTER the render is committed, so the scrollbar
			// thumb position the user reads matches the kernel state that
			// just painted. Sleeping panes pay nothing — emits only run on
			// ticks the RAF loop is already executing.
			this._emitScrollStateChanges();
			// P2.2 (2026-05-20): advance the rotation cursor so the next
			// frame visits non-focused panes in a different order. `>>> 0`
			// wraps to u32 so the counter never grows unbounded across a
			// long-running session.
			this._rafRotationIndex = (this._rafRotationIndex + 1) >>> 0;
			if (this.panes.size === 0) return;
			if (anyRendered) {
				// Likely more work soon — stay on RAF cadence.
				this.rafHandle = requestAnimationFrame(tick);
				return;
			}
			// All idle. Sleep until the next blink boundary (or a 1s
			// watchdog so a missed wake-up path can't hang a pane longer
			// than that). Min 1ms keeps `setTimeout(0)` semantics off the
			// hot path. When `document.visibilityState === 'hidden'`
			// (window minimised or another tab active) we don't even
			// arm the watchdog — the OS won't show our pixels until
			// visibility flips back, and that fires the
			// `visibilitychange` listener which wakes us via
			// `this.wake()`. Skipping the watchdog here is the
			// "彻底停 RAF" idle-optimisation pass: it brings hidden-
			// state CPU down to literal zero.
			if (typeof document !== 'undefined' && document.visibilityState === 'hidden') {
				return;
			}
			const sleepMs = Math.min(Math.max(minDeadlineMs, 1), 1000);
			this.idleTimer = setTimeout(() => {
				this.idleTimer = null;
				// Defensive repaint on every idle-watchdog wake. WebView2
				// occasionally returns a fresh-zero swap-chain texture
				// after extended idle — under the previous "wake +
				// nothing dirty → skip draws" path, the user saw stale
				// or torn content (TUI rendering 错位) until the next
				// real dirty event landed. Marking the host invalidated
				// here forces a full cache replay across every pane on
				// the upcoming tick at the cost of one GPU draw per
				// second; sub-millisecond GPU work, invisible to CPU.
				this._hostInvalidatePending = true;
				this.startRafLoop();
			}, sleepMs);
			}); // §P4 close perfMark('rg.frame.tick', ...)
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
		if (this._resizeReleaseListener !== null && typeof document !== 'undefined') {
			document.removeEventListener('pointerup', this._resizeReleaseListener);
			document.removeEventListener('pointercancel', this._resizeReleaseListener);
			this._resizeReleaseListener = null;
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
