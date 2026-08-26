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
import type { HostPorts } from './ports';
import { invoke } from '@tauri-apps/api/core';
import type { ActiveWallpaperGpu, InputBufferState } from './types';
import { perfMark } from './perfTrace';
import { unknownText } from '../transport/unknownText';
import { DEFAULT_TERM_FONT } from './fontStack';
import { loadTerminalFonts, type FontDataInstaller } from './fontDataService';
import { imeHelperCssPosition, type ImeAnchorInput } from './imeAnchor';
import {
	cellFromVisualClientPoint,
	computePaneGeometry,
	type PaneGeometry,
} from './paneGeometry';
import {
	isBrowserHeapUnderPressure,
	planTerminalMemoryReclaim,
	terminalScrollbackBudgetRows,
	TERMINAL_MEMORY_SWEEP_MS,
	type BrowserHeapSnapshot,
} from './terminalMemoryPolicy';
import {
	dropPendingFeedBuffers,
	enqueueDeferredFeed,
	hasDeferredFeed,
	prependDeferredFeed,
	takeDeferredFeed,
} from './terminalFeedPolicy';
import {
	INITIAL_FIT_RETRY_DELAYS_MS,
	needsInitialPaneFit,
	type InitialFitMeasurement,
} from './initialPaneFit';
import { shouldWipeHostOnPaneRemount } from './hostRemountPolicy';
import { shouldForwardPointerMotion, sgrReleaseButton } from './mouseForwardPolicy';
import { SYNC_OUTPUT_TIMEOUT_MS, TUI_CURSOR_SETTLE_MS } from './renderTransaction';

function isMacPlatform(): boolean {
	return typeof navigator !== 'undefined'
		&& /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
}

function linkModifierHeld(event: Pick<KeyboardEvent, 'ctrlKey' | 'metaKey'>): boolean {
	return isMacPlatform() ? event.metaKey : event.ctrlKey;
}

function copySelectionIfPresent(entry: PaneEntry): boolean {
	const selection = entry.kernel.getSelectionText();
	if (!selection) return false;
	if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
		void navigator.clipboard.writeText(selection);
	}
	entry.kernel.clearSelection();
	entry.selecting = false;
	entry.selectionStartAbs = null;
	entry.selectionEndAbs = null;
	return true;
}

function traceKeydown(entry: PaneEntry, ev: KeyboardEvent, bytes: Uint8Array): void {
	if (typeof localStorage === 'undefined' || localStorage.getItem('RIDGE_CURSOR_TRACE') !== '1') return;
	const ts = performance.now().toFixed(1);
	const hex = Array.from(bytes).map((b) => b.toString(16).padStart(2, '0')).join(' ');
	const kernel = entry.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
	// eslint-disable-next-line no-console
	console.debug(`[cursor-trace][${ts}ms] keydown key=${JSON.stringify(ev.key)} →bytes(${bytes.length})=${hex} kernel-cursor(pre)=(${kernel.cursorRow()},${kernel.cursorCol()})`);
}

function isExpectedWorkerLifecycleCancellation(error: unknown): boolean {
	const message = error instanceof Error ? error.message : unknownText(error);
	return (
		message === 'pane destroyed; request cancelled' ||
		message === 'render worker terminated with pending requests'
	);
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
import { LinkSpanIndex, type LinkSpan } from './linkSpans';
import {
	decideHoverUnderline,
	decideLinkClick,
	osc8UnderlineRegions,
	underlineRegionsFromSpan,
} from './linkAffordance';
import {
	buildOpenPlanFromHit,
	encodeUnderlineDataset,
	planHostOpen,
	underlineCssTokens,
	type HostOpenAction,
} from './linkOpenHost';
import type { TerminalLinkOpenRequest } from './ports';

type LinkUnderlineRegion = { row: number; c0: number; c1: number };

function createLinkUnderlineOverlay(container: HTMLElement): HTMLDivElement {
	const el = document.createElement('div');
	el.setAttribute('aria-hidden', 'true');
	el.dataset.ridgeLinkUnderline = 'true';
	el.style.cssText = [
		'position:absolute',
		'display:none',
		'pointer-events:none',
		'z-index:3',
		'height:1px',
		'border-radius:0',
		'background:var(--rg-accent,#58a6ff)',
		'box-sizing:border-box',
	].join(';');
	container.appendChild(el);
	return el;
}

function linkOpenHintText(isMac = isMacPlatform()): string {
	return isMac ? '⌘+点击打开' : 'Ctrl+点击打开';
}

function createLinkHintOverlay(container: HTMLElement): HTMLDivElement {
	const el = document.createElement('div');
	el.setAttribute('aria-hidden', 'true');
	el.dataset.ridgeLinkHint = 'true';
	el.textContent = linkOpenHintText();
	el.style.cssText = [
		'position:absolute',
		'display:none',
		'pointer-events:none',
		'z-index:4',
		'padding:2px 6px',
		'border:1px solid color-mix(in srgb,var(--rg-fg,#fff) 18%,transparent)',
		'border-radius:4px',
		'background:var(--rg-bg-elevated,#20252d)',
		'color:var(--rg-fg,#fff)',
		'font:12px/1.2 system-ui,sans-serif',
		'white-space:nowrap',
		'box-shadow:0 2px 8px rgba(0,0,0,.24)',
		'box-sizing:border-box',
	].join(';');
	container.appendChild(el);
	return el;
}
// §1.32 Wave F: PTY-prompt suffix snapshot — reads shell-input from
// kernel cells instead of mirroring keystrokes. See module docstring.

// §1.32 Wave F: pure shell-input reconstruction from kernel cells. The
// kernel reads + pane-level start marker live here; the reconstruction
// math is the tested pure function in `shellInputSnapshot`.
import { reconstructInputSnapshot } from './shellInputSnapshot';
// §1.32 (2026-05-20): `linkResolver` transitively imports `monaco-editor`
// via `$lib/stores/fileEditor → $lib/utils/markdown`. Keeping it as a
// static top-level import drags monaco into every consumer of `manager.ts`,
// which made `paneTree.test.ts` crash on the `window` reference inside
// monaco's `window.js` when running in Vitest's node env. The functions
// are only needed inside a click handler — lazy-import them at the use
// site (around line 1185 below) instead.

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

interface QueuedDeltaFrame {
	bytes: Uint8Array;
	onError?: (error: unknown) => void;
}

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
	/** iter-60 G3：raw 字节模式。Remote/Host 谁主动刷新，谁发送带 owner 的
	 *  `pty-resized` canonical grid；另一端只应用该网格，不再次 claim。仅
	 *  `localGridAuthority` pane 在 fit 时主动计算并刷新 PTY。TerminalCanvas
	 *  attach/unpark 后置 true。 */
	localGridAuthority?: boolean;
	/** Debounce timer for fit. ResizeObserver fires many times during
	 *  splitpanes drag (or SvelteKit hydration). Each fit calls
	 *  `kernel.resize` AND triggers an async PTY resize via the handler.
	 *  If kernel size oscillates faster than PTY can catch up,
	 *  PSReadLine (which uses absolute cursor positions like CSI 39;18H)
	 *  loses track and emits land on the wrong row → "all output stacked
	 *  on the bottom row" bug. We debounce ~120ms: while the container
	 *  is animating, we don't resize at all; once it settles, fit once. */
	pendingFitTimer: ReturnType<typeof setTimeout> | null;
	/** Bounded cold-mount fit retries. Layout/font/WebGPU setup can briefly
	 * report 0×0 or stale metrics; keep retry state cancellable so a parked or
	 * destroyed pane cannot retain a timer or re-enter a freed kernel. */
	initialFitTimer: ReturnType<typeof setTimeout> | null;
	initialFitAttempt: number;
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
	/** Kernel/output or renderer invalidation already requires a paint. */
	renderPending: boolean;
	/** Monotonic delta generation mirrored into the worker renderer. */
	/** Native delta frames wait here until the next compositor turn. Keeping
	 * their parse/apply work out of the Tauri Channel callback prevents a burst
	 * from monopolising the input event loop. Head indexing avoids O(n) shifts. */
	deltaQueue: QueuedDeltaFrame[];
	deltaQueueHead: number;
	deltaQueuedBytes: number;
	/** Short inline-TUI presentation transaction. Grid cells keep painting;
	 * the cursor stays at its last presented cell until the walk is quiet. */
	tuiCursorSuppressUntil: number;
	tuiCursorSuppressed: boolean;
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
	pointerCancelListener: (e: PointerEvent) => void;
	pointerLeaveListener: (e: PointerEvent) => void;
	modifierKeyListener: (e: KeyboardEvent) => void;
	lastPointerPoint: {
		clientX: number;
		clientY: number;
		buttons: number;
		shiftKey: boolean;
		altKey: boolean;
		ctrlKey: boolean;
		metaKey: boolean;
	} | null;
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
	/** Component switches retain a ready renderer for instant rebind. Memory
	 * parks and final detach still free it, so this is not an unbounded leak. */
	rendererRetained: boolean;
	/** Why the renderer is parked. Component parks protect a transient DOM
	 * unmount; memory parks may be restored automatically when visible again. */
	parkReason: 'component' | 'memory' | null;
	/** LRU signal for aggregate scrollback reclamation. PTY output does not
	 * refresh it: a noisy background pane must remain reclaimable. */
	lastForegroundAt: number;
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
	/** DOM bridge for the focused IME sink. Fired after an input anchor is
	 *  captured, including the delayed PTY-echo capture. */
	imeAnchorHandler: ((anchor: { row: number; col: number } | null) => void) | null;
	/** Composition is live. Shell mode follows its live cursor; TUI mode uses
	 *  `imeAnchor` as a redraw-resistant snapshot until compositionend. */
	imeCompositionActive: boolean;
	/** §A.4 (2026-05-08) — pending PTY bytes held back briefly while the
	 *  kernel is in inline-TUI mode (Ink/log-update emitting walk + new
	 *  frame across multiple ConPTY reads). Without coalescing, a rAF
	 *  tick can sample the kernel between an EL-walk event and the new-
	 *  frame write event, painting a partial state that the next frame does
	 *  not fully overwrite → "wrong word" jitter on the spinner row.
	 *  Null when no buffer is pending. */
	feedBuffer: Uint8Array | null;
	/** FIFO fragments accumulated during the short inline-TUI window. */
	feedBufferChunks: Uint8Array[];
	/** Total bytes in `feedBuffer` plus `feedBufferChunks`. */
	feedBufferBytes: number;
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
	 *  Undefined before the first `_recomputeViewport` runs. Host pane lookups treat
	 *  `undefined` the same as a zero-size rect — the pane is parked-by-
	 *  clip until JS computes a real viewport. */
	viewport?: { x: number; y: number; w: number; h: number };
	/** CSS/device geometry used by renderer, pointer, wheel and selection. */
	geometry?: PaneGeometry;
	/** Stage visual offset captured alongside `geometry`; pointer mapping uses
	 *  the delta from this snapshot to the event's current offset. */
	geometryVisualOffsetY?: number;
	/** Visual-only stage translateY; layout/grid geometry intentionally stays fixed. */
	visualOffsetY?: number;
	/** §shared-remote (2026-06-14): the kernel (rows, cols) the last
	 *  `_recomputeViewport` sized the centered letterbox for. In
	 *  `sharedRemoteMode` the scissor tracks the SHARED PTY grid (not the
	 *  container), so the RAF pre-pass watches these to re-letterbox the
	 *  moment a host/controller claim grows or shrinks the kernel grid via
	 *  the broadcast Resize delta. -1 until the first shared-mode compute. */
	lastViewportKernelRows: number;
	lastViewportKernelCols: number;
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
	 *  标志，在 hover/click 时按需同步扫一次。 */
	linkSpans: LinkSpanIndex;
	/** Real DOM affordance for link hover. Dataset-only state is not painted by
	 * WebView2, so keep pointer-events-free overlays beside the canvas and
	 * position them in the same CSS grid coordinates. */
	linkUnderlineEls: HTMLDivElement[];
	linkUnderlineRegions: { row: number; c0: number; c1: number }[];
	linkHintEl: HTMLDivElement | null;
	linkHintRegion: { row: number; c0: number; c1: number } | null;
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
	/** FIFO chunks queued behind `feedDeferred`; avoids O(n²) concat growth. */
	feedDeferredChunks: Uint8Array[];
	/** Total bytes retained by `feedDeferred` plus `feedDeferredChunks`. */
	feedDeferredBytes: number;
	/** Cumulative render-only bytes shed after the bounded queue filled. */
	feedDroppedBytes: number;
	feedDropCount: number;
	/** True until the owner re-subscribes / otherwise repairs the render gap. */
	feedNeedsResync: boolean;
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

interface RafFrameState {
	frameOrder: PaneEntry[];
	/** All live panes still receive bounded kernel/delta work, including hidden workspaces. */
	feedOrder: PaneEntry[];
	dateNow: number;
	perfNow: number;
	surfaceJustWiped: boolean;
	dirtyByPane: Map<string, boolean>;
	activeWsId: string | null;
	activeHost: SurfaceHostHandle | null;
	hostFrameOpen: boolean;
	frameFailed: boolean;
	anyRendered: boolean;
	/** Dirty panes left for the next compositor turn by the paint budget. */
	renderDeferred: boolean;
	renderDeadlineMs: number;
	minDeadlineMs: number;
}

interface FitGeometry {
	wCss: number;
	hCss: number;
	rows: number;
	cols: number;
}

interface AttachCanvas {
	canvas: HTMLCanvasElement;
	hostHandle: SurfaceHostHandle;
}

interface AttachRenderState {
	handle: RenderHandle;
	dpr: number;
	cellW: number;
	cellH: number;
}

/** Trailing-edge debounce window for container resize. The pane only
 *  re-fits (scissor + kernel grid + PTY SIGWINCH) after the user has
 *  paused this long without sending a new `viewportChanged` event,
 *  OR after a global `pointerup` fires (whichever comes first).
 *  500 ms — short enough that mouse-paused-mid-drag settles feel
 *  responsive, long enough that incidental layout twitches don't
 *  trip a mid-drag re-fit. `pointerup` is the dominant trigger; this
 *  is just the safety net when the release is missed. */
const RESIZE_SETTLE_MS = 500;
const FEED_PER_CALL_BUDGET_MS = 4;
/** Keep render back-pressure bounded without allowing one burst to monopolize
 * a frame. A focused pane may drain two chunks, then siblings get a turn. */
const FEED_FRAME_BUDGET_MS = 6;
const MAX_DEFERRED_CHUNKS_PER_FRAME = 2;
/** Leave a turn for input/layout after parser work. The focused pane is always
 * allowed one paint; siblings yield once this deadline is reached. */
const RENDER_FRAME_BUDGET_MS = 8;
/** Public fast-path flushes may never bypass the frame budget. */
export const MAX_PANE_FEED_FLUSH_BUDGET_MS = FEED_FRAME_BUDGET_MS;
/** Delta frames are already VTE-parsed natively, but decoding/applying a
 * microburst in the Channel callback can still starve WebView input. */
const DELTA_FRAME_BUDGET_MS = 4;

/**
 * Singleton. Created lazily on first `instance()` call. Held by the
 * `<RidgeTerminalRoot>` Svelte component for the entire app lifetime.
 */

/** §P2 端口注入（R0 范式）：manager 迁入 @ridge/remote 后不再直接 import 主 app 的
 *  store/util，改由主 app 启动时经 setHostPorts 注入。模块级单持有者，static 与实例
 *  方法共读；缺失（SSR / 手机未注入 / 预启动期）时 manager 优雅降级。 */
let _hostPorts: HostPorts | null = null;

function mouseButtonFromButtons(buttons: number): number {
	if (buttons & 1) return 0;
	if (buttons & 2) return 2;
	if (buttons & 4) return 1;
	// SGR motion uses button 3 when no physical button is held. Using 0
	// makes DECSET ?1003 hover look like a left-button drag to TUIs.
	return 3;
}

export class TerminalManager {
	private static _instance: TerminalManager | null = null;

	private wasmReady = false;
	private wasmReadyPromise: Promise<void> | null = null;
	private fontInstaller: FontDataInstaller | null = null;
	private readonly loadedFontStacks = new Set<string>();
	private readonly fontLoadPromises = new Map<string, Promise<void>>();

	private readonly opts: ManagerOptions;
	private readonly panes = new Map<string, PaneEntry>();
	/** Active-frame lookup. RAF paints one workspace, so avoid walking hidden
	 * workspace panes on every compositor turn. The main map remains the source
	 * of truth for lifecycle and feed bookkeeping. */
	private readonly paneIdsByWorkspace = new Map<string, Set<string>>();
	/** Pane ids with parser work waiting for a compositor turn. Keeping this
	 * sparse avoids scanning every hidden workspace on each RAF tick. Parked
	 * panes stay indexed so their backlog resumes when they are unparked. */
	private readonly pendingFrameWorkPanes = new Set<string>();
	/** P4.6 Part B (2026-05-22) — paneIds that have been mirrored into
	*/
	private rafHandle: number | null = null;
	/** P2.2 (2026-05-20): focus/cursor owner per workspace. Keeping this
	 *  keyed by workspace prevents a focus claim in one split tree from
	 *  stealing the cursor owner of another tree. `setFocused(true)` also
	 *  clears the previous owner's renderer before installing the new one. */
	private readonly _focusedPaneByWorkspace = new Map<string, string>();
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
	/** Theme updates for hidden keep-alive workspaces are spread across turns so
	 * a settings click only pays the visible panes' atlas invalidation cost. */
	private _themeGeneration = 0;
	private _themeDeferredTimer: ReturnType<typeof setTimeout> | null = null;
	/** §shared-remote: shared-grid mode for the desktop browser controller.
	 *  Enabled only on the desktop-in-browser controller (WEB_REMOTE). One PTY
	 *  has one grid; multiple viewers of different sizes can't all fill it. In
	 *  this mode:
	 *   - live ResizeObserver frames only re-letterbox;
	 *   - the existing trailing-edge/pointerup fit claims the settled size once;
	 *   - `_recomputeViewport` sizes the scissor to the KERNEL's current grid
	 *     (the shared size, driven by Resize deltas) and CENTERS it in the pane,
	 *     so the surplus area is intentional terminal-bg letterbox, not a dead
	 *     zone;
	 *   - `claimPaneSize` remains an immediate manual recovery path.
	 *  Off (normal desktop): byte-for-byte the prior behaviour. */
	private _sharedRemoteMode = false;
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
	/** Consecutive host/render failures. Prevents a broken WebGPU surface or
	 * renderer exception from becoming a 1 ms RAF spin. */
	private frameFailureCount = 0;
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
	 *  consumes it. That tick performs one atlas-preserving full repaint of
	 *  every visible pane into the newly-seeded compositor backing store. */
	private _hostInvalidatePending: boolean = false;
	/** Mirror of the most recent `setPreedit` call per pane. RidgePane
	 *  writes the preedit overlay via `setPreedit(paneId, text, row, col)`;
	 *  the wasm side stores it but does not expose a getter, so we keep
	 *  this small JS-side mirror for E2E specs to assert that the overlay
	 *  cell matches the textarea cell + the kernel cursor. Cleared by
	 *  `clearPreedit`. */
	private readonly _lastPreeditCall: Map<string, { row: number; col: number; text: string }> = new Map();
	/** In-flight `attachHost` init promise. Concurrent pane `attach()` /
	 *  `unpark()` calls await this so they don't race ahead of WebGPU
	 *  initialisation. Initialization failures reject only after wgpu's
	 *  WebGPU-first, WebGL2-second selection cannot produce a device. */
	private attachHostPromise: Promise<void> | null = null;
	/** Document `visibilitychange` listener installed once on first pane
	 *  attach; removed on last detach. Hidden tabs throttle RAF anyway,
	 *  but waking on visibility-restore avoids a lag the first time the
	 *  user comes back. */
	private visibilityListener: (() => void) | null = null;
	private _lastMemorySweepAt = 0;
	private readonly _memoryRestorePending = new Set<string>();
	/**
	 * WebGPU SurfaceHost is shared by every pane.  Adapter/device creation is
	 * asynchronous and the browser can deadlock when several RenderHandles are
	 * constructed against that canvas in the same turn.  Keep renderer creation
	 * single-file even when several workspace panes mount/unpark together.
	 */
	private _rendererCreateQueue: Promise<void> = Promise.resolve();
	/** Latest active-workspace notification. Older paint/restore work must not
	 * wake the renderer after a newer tab selection. */
	private _activeWorkspaceChangeGeneration = 0;
	private _memoryRestoreQueue: Promise<void> = Promise.resolve();
	/**
	 * Defer shared-surface invalidation while memory-parked panes are being
	 * restored. `unpark()` creates the renderer asynchronously; invalidating
	 * before it is attached makes the host clear with no active draw region,
	 * which presents as a black frame during workspace-tab switches.
	 */
	private _hostInvalidateSuspendDepth = 0;
	private _deferredHostInvalidate = false;
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

		if (typeof window !== 'undefined') {
			try {
				if (window.localStorage?.RIDGE_DIAG === '1') {
					const diagWindow = window as typeof window & {
						__RIDGE_TERMINAL_GEOMETRY?: () => unknown;
					};
					diagWindow.__RIDGE_TERMINAL_GEOMETRY = () => this.debugGeometry();
				}
			} catch {
				// Storage may be unavailable in hardened browser contexts.
			}
		}

	}

	/** Return the existing singleton without creating one. Used by
	 *  late-arriving callers (theme watchers) that want
	 *  to invalidate panes only when the manager has actually spun up.
	 *  Returns null when no pane has attached yet — in which case the
	 *  next attach starts with a fresh atlas anyway. */
	static tryInstance(): TerminalManager | null {
		return TerminalManager._instance;
	}

	/** §P2 注入主 app 能力（settings/cwd/链接路由）。app 启动时调用一次（+page
	 *  onMount），须早于首个 pane attach 与链接点击。手机端可注入部分或不注入。 */
	static setHostPorts(ports: HostPorts | null): void {
		_hostPorts = ports;
	}

	/** 读回已注入的 HostPorts（themeBridge/ptyBridge/paneShell 经此取 settings/
	 *  termSettings/themes 等端口）。未注入时 null。 */
	static hostPorts(): HostPorts | null {
		return _hostPorts;
	}

	/** 终端链接路由器需要的 ctx：当前 pane 的 cwd（OSC 7 报告值）。 */
	static _currentPaneCwd(entry: PaneEntry): string | undefined {
		return _hostPorts?.cwd?.current(entry.workspaceId, entry.paneId);
	}

	/** 终端链接路由器需要的 ctx：所有 pane 当前 cwd 集合，用于"是否属于
	 *  任意 cwd 树"判断（多 workspace 多 pane 同时活跃时，落在任一 pane
	 *  CWD 内的文件都视为可在 ridge 编辑器打开）。 */
	static _knownCwds(): string[] {
		return _hostPorts?.cwd?.all() ?? [];
	}

	static _workspaceRoot(entry: PaneEntry): string | undefined {
		return _hostPorts?.cwd?.workspaceRoot?.(entry.workspaceId, entry.paneId);
	}

	/**
	 * C51 product path: execute a HostOpenAction from linkOpenHost.
	 * URL → opener / window.open; path/file → openTextLink host port.
	 */
	static _executeOpenPlan(
		plan: HostOpenAction,
		entry: PaneEntry,
		fallbackText: string,
	): boolean {
		if (plan.type === 'noop') {
			console.warn('[ridge-term] open plan noop', plan.reason, fallbackText);
			return false;
		}
		if (!_hostPorts?.openTextLink) return false;
		const cwd = TerminalManager._currentPaneCwd(entry);
		const origin = {
			kind: 'local' as const,
			workspaceId: entry.workspaceId,
			paneId: entry.paneId,
		};
		if (plan.type === 'open_url') {
			const request: TerminalLinkOpenRequest = {
				type: 'url',
				href: plan.href,
				cwd,
				workspaceRoot: TerminalManager._workspaceRoot(entry),
				origin,
			};
			void Promise.resolve(_hostPorts.openTextLink(request));
			return true;
		}
		// open_file | reveal_in_tree → host port (editor / explorer)
		const request: TerminalLinkOpenRequest = {
			type: 'path',
			path: plan.path,
			...(plan.type === 'open_file'
				? { line: plan.line, col: plan.col }
				: { directoryHint: true }),
			cwd,
			workspaceRoot: TerminalManager._workspaceRoot(entry),
			origin,
		};
		void Promise.resolve(_hostPorts.openTextLink(request));
		return true;
	}

	static instance(opts?: ManagerOptions): TerminalManager {
		if (!TerminalManager._instance) {
			// One wgpu renderer serves both browser backends: WebGPU first,
			// WebGL2 when WebGPU is unavailable.
			//
			TerminalManager._instance = new TerminalManager(
				opts ?? {
					// Host resolves this stack and supplies bounded font bytes to
					// the shared Swash/WebGPU renderer before pane attachment.
					fontFamily: DEFAULT_TERM_FONT,
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
	private _ensureFontStack(stack: string): Promise<void> {
		const key = stack.trim();
		if (this.loadedFontStacks.has(key)) return Promise.resolve();
		const active = this.fontLoadPromises.get(key);
		if (active) return active;
		if (!this.fontInstaller) {
			return Promise.reject(new Error('FONT_DATA_MISSING: wasm font installer is unavailable'));
		}
		const pending = loadTerminalFonts(key, this.fontInstaller).then(
			() => {
				this.loadedFontStacks.add(key);
				this.fontLoadPromises.delete(key);
			},
			(error) => {
				this.fontLoadPromises.delete(key);
				throw error;
			},
		);
		this.fontLoadPromises.set(key, pending);
		return pending;
	}

	ready(): Promise<void> {
		if (this.wasmReady) return Promise.resolve();
		if (this.wasmReadyPromise !== null) return this.wasmReadyPromise;
		const pending = (async () => {
			await init(wasmUrl);
			const fontModule = (await import('@ridge/term-wasm')) as unknown as {
				installFontData?: (data: Uint8Array) => boolean;
			};
			if (typeof fontModule.installFontData !== 'function') {
				throw new Error('FONT_DATA_MISSING: wasm bundle has no system-font installer');
			}
			this.fontInstaller = fontModule.installFontData;
			await this._ensureFontStack(this.opts.fontFamily);
			this.wasmReady = true;
			// §atlas-race forensics (2026-06-22): expose detector counters on
			// window.__ridgeAtlasRace() for release console / CDP polling. A value
			// of -1 means the wasm export is missing (running an OLD bundle) —
			// confirms the installed app actually has this build.
			try {
				const rmod = (await import('@ridge/term-wasm')) as unknown as {
					atlasOverwriteAfterCiteCount?: () => number;
				};
				if (typeof window !== 'undefined') {
					(window as unknown as Record<string, unknown>).__ridgeAtlasRace = () => ({
						overwriteAfterCite:
							typeof rmod.atlasOverwriteAfterCiteCount === 'function'
								? rmod.atlasOverwriteAfterCiteCount()
								: -1,
					});
				}
			} catch {
				/* old wasm bundle without the exports */
			}
			// §present-fast (2026-06-22): opt the WebGPU renderer into the
			// dirty-row fast path (vs. the always-full-frame correctness
			// default) when `localStorage.RIDGE_PRESENT_FAST === '1'`. On a
			// release WebView2 that reliably preserves swap-chain pixels under
			// LoadOp::Load this kills the per-frame full Clear behind IME-
			// composition / selection flicker AND cuts the per-frame glyph
			// re-admission that amplifies switch-workspace atlas-eviction
			// garble. Default off = zero behaviour change; reversible by
			// unsetting the flag. typeof-guarded for wasm bundles built before
			// the `setPresentFast` export existed.
			try {
				if (
					typeof localStorage !== 'undefined' &&
					localStorage.getItem('RIDGE_PRESENT_FAST') === '1'
				) {
					const mod = (await import('@ridge/term-wasm')) as unknown as {
						setPresentFast?: (on: boolean) => void;
					};
					if (typeof mod.setPresentFast === 'function') mod.setPresentFast(true);
				}
			} catch {
				/* old wasm bundle without the export → skip */
			}
		})();
		this.wasmReadyPromise = pending;
		void pending.catch(() => {
			if (this.wasmReadyPromise === pending) this.wasmReadyPromise = null;
		});
		return pending;
	}

	/** Construct a WebGPU handle. Missing host/constructor and adapter errors
	 *  are surfaced after the WebGPU/WebGL2 selection fails. */
	private async _makeHandle(
		canvas: HTMLCanvasElement,
		surfaceHost?: SurfaceHostHandle,
	): Promise<RenderHandle> {
		if (!surfaceHost) {
			const error = new Error('WEBGPU_INIT_FAILED: SurfaceHostHandle is required');
			console.error('[ridge-term] terminal WebGPU initialization failed', error);
			throw error;
		}
		const HandleCtor = RenderHandle as unknown as {
			newWithWebgpuFirst?: (
				c: HTMLCanvasElement,
				host: SurfaceHostHandle,
			) => Promise<RenderHandle>;
		};
		if (typeof HandleCtor.newWithWebgpuFirst !== 'function') {
			const error = new Error('WEBGPU_INIT_FAILED: wasm bundle lacks newWithWebgpuFirst');
			console.error('[ridge-term] terminal WebGPU initialization failed', error);
			throw error;
		}
		try {
			const hostArg =
				typeof (surfaceHost as unknown as { clone?: () => SurfaceHostHandle }).clone ===
					'function'
					? (surfaceHost as unknown as { clone: () => SurfaceHostHandle }).clone()
					: surfaceHost;
			const handle = await HandleCtor.newWithWebgpuFirst(canvas, hostArg);
			const name = (handle as unknown as { backendName?: () => string }).backendName?.();
			if (name && !['webgpu', 'webgl2'].includes(name.toLowerCase())) {
				throw new Error(`WEBGPU_INIT_FAILED: unexpected backend ${name}`);
			}
			return handle;
		} catch (error) {
			const failure = error instanceof Error
				? error
				: new Error(`WEBGPU_INIT_FAILED: ${unknownText(error)}`);
			console.error('[ridge-term] terminal WebGPU initialization failed', failure);
			throw failure;
		}
	}

	private async _makeHandleSerialized(
		canvas: HTMLCanvasElement,
		surfaceHost?: SurfaceHostHandle,
	): Promise<RenderHandle> {
		const predecessor = this._rendererCreateQueue;
		let release!: () => void;
		this._rendererCreateQueue = new Promise<void>((resolve) => {
			release = resolve;
		});
		await predecessor;
		try {
			return await this._makeHandle(canvas, surfaceHost);
		} finally {
			release();
		}
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
	 * Missing WebGPU exports or adapter/device acquisition failures reject
	 * with a structured `WEBGPU_INIT_FAILED` error. The pane remains
	 * unrendered until the host can be initialized.
	 */
	public attachHost(canvas: HTMLCanvasElement): Promise<void> {
		if (this.attachHostPromise !== null) return this.attachHostPromise;
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
				throw new Error('WEBGPU_INIT_FAILED: SurfaceHostHandle missing from wasm bundle');
			}
			let host: SurfaceHostHandle;
			try {
				host = await SHHCtor.init(canvas);
			} catch (error) {
				const failure = error instanceof Error
					? error
					: new Error(`WEBGPU_INIT_FAILED: ${unknownText(error)}`);
				console.error('[ridge-term] terminal WebGPU host initialization failed', failure);
				throw failure;
			}
			this.globalHost = { canvas, host };
			try {
				this.resizeHost(); // initial swap-chain configure
			} catch (error) {
				this.globalHost = null;
				const failure = error instanceof Error
					? error
					: new Error(`WEBGPU_INIT_FAILED: ${unknownText(error)}`);
				console.error('[ridge-term] terminal GPU surface resize failed', failure);
				throw failure;
			}
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

	/** §wallpaper — 将 activeWallpaperGpu 信号的最新值应用到 GPU 壁纸。
	 *  由 +page.svelte 订阅 activeWallpaperGpu store 并在每次 emission 时调用。
	 *  gpu=null 时调用 clearWallpaper 回退纯色；host 未就绪时 no-op（信号晚到
	 *  不丢失——host 就绪后 attachHost 调用方须补一次 applyWallpaperGpu）。 */
	public applyWallpaperGpu(gpu: ActiveWallpaperGpu | null): void {
		const host = this._globalHostHandle();
		if (!host) return;
		if (gpu) {
			host.setWallpaper(gpu.rgba, gpu.width, gpu.height, gpu.opacity);
		} else {
			host.clearWallpaper();
		}
	}

	/** §A.9 — internal: global SurfaceHost lookup. The legacy
	 *  per-workspace `_hostFor(wsId)` API is gone; every pane shares
	 *  the same host now, so the wsId argument is meaningless. */
	private _globalHostHandle(): SurfaceHostHandle | null {
		return this.globalHost?.host ?? null;
	}

	/**
	 * Start the DOM host when a pane wins the mount race.  Desktop keeps the
	 * shared host canvas after the workspace tree for stacking, so the first
	 * RidgePane can attach before the canvas action has called attachHost().
	 * If the canvas is already in the DOM, claim it here; the action's later
	 * call is idempotent. Without this bridge the pane receives no host and
	 * attach reports the structured WebGPU initialization error.
	 */
	private async _ensureDomHostStarted(): Promise<void> {
		if (this.globalHost !== null || this.attachHostPromise !== null) return;
		if (typeof document === 'undefined') return;
		const hostCanvas = (document as Document & {
			querySelector?: (selectors: string) => Element | null;
		}).querySelector?.('canvas[data-rg-host]');
		if (!(hostCanvas instanceof HTMLCanvasElement)) return;
		await this.attachHost(hostCanvas);
	}

	/** Call `surfaceHost.invalidate()` AND mark `_hostInvalidatePending`
	 *  so the next RAF tick treats the required full repaint as real work
	 *  (rather than letting the idle-sleep gate skip them and leave the
	 *  freshly-cleared swap chain blank). Every site that wipes the
	 *  shared canvas must go through here — direct
	 *  `_globalHostHandle()?.invalidate()` calls bypass the flag and
	 *  resurrect the "blank pane until next dirty event" symptom. */
	private _invalidateHost(): void {
		this._hostInvalidatePending = true;
		if (this._hostInvalidateSuspendDepth > 0) {
			this._deferredHostInvalidate = true;
			return;
		}
		this._globalHostHandle()?.invalidate();
	}

	private _beginHostInvalidateBatch(): void {
		this._hostInvalidateSuspendDepth += 1;
	}

	private _endHostInvalidateBatch(): void {
		if (this._hostInvalidateSuspendDepth === 0) return;
		this._hostInvalidateSuspendDepth -= 1;
		if (this._hostInvalidateSuspendDepth !== 0 || !this._deferredHostInvalidate) return;
		this._deferredHostInvalidate = false;
		this._globalHostHandle()?.invalidate();
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
	 * No-op when the shared WebGPU host is not attached.
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
		const previous = {
			width: canvas.width,
			height: canvas.height,
			styleWidth: canvas.style.width,
			styleHeight: canvas.style.height,
		};
		canvas.width = wDev;
		canvas.height = hDev;
		canvas.style.width = `${wCss}px`;
		canvas.style.height = `${hCss}px`;
		try {
			host.resize(wCss, hCss, dpr);
		} catch (error) {
			canvas.width = previous.width;
			canvas.height = previous.height;
			canvas.style.width = previous.styleWidth;
			canvas.style.height = previous.styleHeight;
			throw error instanceof Error
				? error
				: new Error(`WEBGPU_INIT_FAILED: ${unknownText(error)}`);
		}
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
	 * No-op when the shared WebGPU host isn't initialized.
	 */
	public onActiveWorkspaceChanged(workspaceId: string): void {
		this._activeWorkspaceId = workspaceId;
		const generation = ++this._activeWorkspaceChangeGeneration;
		const shouldRestore = typeof document === 'undefined' || !document.hidden;
		if (!this.globalHost) {
			if (shouldRestore) void this._restoreMemoryParked(workspaceId);
			return;
		}

		if (shouldRestore) this._beginHostInvalidateBatch();
		const restore = shouldRestore
			? this._restoreMemoryParked(workspaceId)
			: Promise.resolve();
		const paint = () => {
			if (shouldRestore) this._endHostInvalidateBatch();
			// A newer tab selection owns the shared host. The stale restore may
			// finish later, but it must not recompute or wake the old workspace.
			if (
				generation !== this._activeWorkspaceChangeGeneration ||
				!this.globalHost ||
				this._activeWorkspaceId !== workspaceId
			) return;
			for (const e of this.panes.values()) {
				if (e.parked) continue;
				if (e.workspaceId !== workspaceId) continue;
				// Sync the host-canvas-relative scissor to the now-visible pane
				// container. Keeping wasHiddenLastTick lets this frame fit and
				// render without the legacy one-tick black gap.
				this._recomputeViewport(e);
				// A display:none→flex transition does not reliably emit ResizeObserver.
				// Re-run the kernel fit as well as the visual scissor projection; this
				// is what moves a cold pane off its 80×24 attach seed immediately.
				this._scheduleInitialFit(e);
			}
			this._invalidateHost();
			this.wake();
		};
		void restore.then(paint, paint);
	}

	private _restoreMemoryParked(workspaceId: string | null): Promise<void> {
		// Do not fan out N browser GPU adapter/device creations from one tab click.
		// A serial queue also lets a newer active workspace supersede a stale
		// restore before the next pane is touched.
		const run = async (): Promise<void> => {
			const entries = [...this.panes.values()].filter((entry) =>
				entry.parked &&
				entry.parkReason === 'memory' &&
				(workspaceId === null || entry.workspaceId === workspaceId) &&
				entry.container.isConnected &&
				!this._memoryRestorePending.has(entry.paneId),
			);
			for (const entry of entries) {
				if (
					workspaceId !== null &&
					this._activeWorkspaceId !== workspaceId
				) break;
				if (!entry.container.isConnected || !entry.parked) continue;
				this._memoryRestorePending.add(entry.paneId);
				try {
					await this.unpark(entry.paneId, entry.container);
				} catch (error) {
					console.warn('[ridge-term] memory-park restore failed', entry.paneId, error);
				} finally {
					this._memoryRestorePending.delete(entry.paneId);
				}
			}
		};
		const queued = this._memoryRestoreQueue.then(run, run);
		this._memoryRestoreQueue = queued.then(() => undefined, () => undefined);
		return queued;
	}

	/**
	 * Release cold terminal memory without interrupting PTY streams. Renderer
	 * resources are parked while hidden/pressured; aggregate scrollback is
	 * bounded across panes and remains available in the host's raw replay store.
	 */
	reclaimTerminalMemory(args: { documentHidden?: boolean; forceHeapPressure?: boolean } = {}): {
		clearedPaneIds: string[];
		parkedPaneIds: string[];
		retainedRowsBefore: number;
		retainedRowsAfter: number;
		heapPressure: boolean;
	} {
		const documentHidden = args.documentHidden ??
			(typeof document !== 'undefined' && document.hidden);
		const perfMemory = typeof performance !== 'undefined'
			? (performance as Performance & { memory?: BrowserHeapSnapshot }).memory
			: undefined;
		const heapPressure = args.forceHeapPressure ?? isBrowserHeapUnderPressure(perfMemory);
		const deviceMemory = typeof navigator !== 'undefined'
			? (navigator as Navigator & { deviceMemory?: number }).deviceMemory
			: undefined;
		const candidates = [...this.panes.values()].map((entry) => ({
			paneId: entry.paneId,
			scrollbackRows: entry.kernel.scrollbackLen(),
			focused: entry.paneId === this._focusedPaneByWorkspace.get(entry.workspaceId),
			hidden: this._activeWorkspaceId !== null && entry.workspaceId !== this._activeWorkspaceId,
			parked: entry.parked,
			lastForegroundAt: entry.lastForegroundAt,
		}));
		const plan = planTerminalMemoryReclaim({
			candidates,
			rowBudget: terminalScrollbackBudgetRows(deviceMemory),
			heapPressure,
			documentHidden,
		});
		for (const paneId of plan.clearScrollbackPaneIds) {
			const entry = this.panes.get(paneId);
			if (entry) this._releaseScrollback(entry);
		}
		const parkedRendererIds = new Set(plan.parkRendererPaneIds);
		// Component switches keep a renderer warm, but that cache must yield to
		// an explicit background/heap-pressure reclaim even though the pane is
		// already marked parked and has no connected DOM container.
		if (documentHidden || heapPressure) {
			for (const entry of this.panes.values()) {
				if (entry.parked && entry.rendererRetained) parkedRendererIds.add(entry.paneId);
			}
		}
		for (const paneId of parkedRendererIds) this.park(paneId, 'memory');
		return {
			clearedPaneIds: plan.clearScrollbackPaneIds,
			parkedPaneIds: [...parkedRendererIds],
			retainedRowsBefore: plan.retainedRowsBefore,
			retainedRowsAfter: plan.retainedRowsAfter,
			heapPressure: plan.heapPressure,
		};
	}

	/** Restore renderers parked by an explicit native hide/reclaim event. */
	restoreTerminalMemory(): void {
		this._restoreMemoryParked(this._activeWorkspaceId);
		this._invalidateHost();
		this.wake();
	}

	/**
	 * §4.3 Phase B: predicate. True when this entry is rendering through
	 * the shared SurfaceHost; false before the host viewport is ready.
	 * Callers use this to decide whether host scissor geometry is available.
	 */
	private _isHostMode(entry: PaneEntry): boolean {
		const gh = this.globalHost;
		return gh !== null && entry.canvas === gh.canvas &&
			typeof (gh.host as unknown as { beginFrame?: unknown }).beginFrame === 'function';
	}

	private _clearLinkUnderline(entry: PaneEntry): void {
		entry.linkUnderlineRegions = [];
		for (const el of entry.linkUnderlineEls) el.style.display = 'none';
		this._clearLinkHint(entry);
	}

	private _positionLinkUnderline(entry: PaneEntry): void {
		const regions = entry.linkUnderlineRegions;
		const els = entry.linkUnderlineEls;
		if (regions.length === 0) {
			for (const el of els) el.style.display = 'none';
			return;
		}
		const cellW = entry.geometry?.cellWidthCss ?? entry.cellW;
		const cellH = entry.geometry?.cellHeightCss ?? entry.cellH;
		if (!Number.isFinite(cellW) || !Number.isFinite(cellH) || cellW <= 0 || cellH <= 0) {
			for (const el of els) el.style.display = 'none';
			return;
		}
		const rect = entry.container.getBoundingClientRect();
		const gridLeft = entry.geometry
			? entry.geometry.gridClientXCss - rect.left
			: (entry.lastFitPaddingPx ?? entry.lastAppliedPaddingPx ?? 0);
		const gridTop = entry.geometry
			? entry.geometry.gridClientYCss - rect.top
			: (entry.lastFitPaddingPx ?? entry.lastAppliedPaddingPx ?? 0);
		for (let i = 0; i < els.length; i++) {
			const el = els[i]!;
			const region = regions[i];
			if (!region || region.c1 <= region.c0) {
				el.style.display = 'none';
				continue;
			}
			const left = gridLeft + region.c0 * cellW;
			const top = gridTop + (region.row + 1) * cellH - 1;
			if (![left, top].every(Number.isFinite)) {
				el.style.display = 'none';
				continue;
			}
			el.style.left = `${left}px`;
			el.style.top = `${top}px`;
			el.style.width = `${Math.max(1, (region.c1 - region.c0) * cellW)}px`;
			el.style.display = 'block';
		}
	}

	private _showLinkUnderlines(entry: PaneEntry, regions: LinkUnderlineRegion[]): void {
		this._clearLinkHint(entry);
		entry.linkUnderlineRegions = regions.filter((region) => region.c1 > region.c0);
		while (entry.linkUnderlineEls.length < entry.linkUnderlineRegions.length) {
			entry.linkUnderlineEls.push(createLinkUnderlineOverlay(entry.container));
		}
		while (entry.linkUnderlineEls.length > entry.linkUnderlineRegions.length) {
			entry.linkUnderlineEls.pop()?.remove();
		}
		this._positionLinkUnderline(entry);
	}

	private _clearLinkHint(entry: PaneEntry): void {
		entry.linkHintRegion = null;
		if (entry.linkHintEl) entry.linkHintEl.style.display = 'none';
	}

	private _positionLinkHint(entry: PaneEntry): void {
		const el = entry.linkHintEl;
		const region = entry.linkHintRegion;
		if (!el || !region) return;
		const cellW = entry.geometry?.cellWidthCss ?? entry.cellW;
		const cellH = entry.geometry?.cellHeightCss ?? entry.cellH;
		if (!Number.isFinite(cellW) || !Number.isFinite(cellH) || cellW <= 0 || cellH <= 0) {
			el.style.display = 'none';
			return;
		}
		const rect = entry.container.getBoundingClientRect();
		const gridLeft = entry.geometry
			? entry.geometry.gridClientXCss - rect.left
			: (entry.lastFitPaddingPx ?? entry.lastAppliedPaddingPx ?? 0);
		const gridTop = entry.geometry
			? entry.geometry.gridClientYCss - rect.top
			: (entry.lastFitPaddingPx ?? entry.lastAppliedPaddingPx ?? 0);
		const left = gridLeft + region.c0 * cellW;
		const top = Math.max(2, gridTop + region.row * cellH - 26);
		if (![left, top].every(Number.isFinite)) {
			el.style.display = 'none';
			return;
		}
		el.style.left = `${left}px`;
		el.style.top = `${top}px`;
		el.style.display = 'block';
	}

	private _showLinkHint(entry: PaneEntry, region: LinkUnderlineRegion): void {
		entry.linkHintEl ??= createLinkHintOverlay(entry.container);
		entry.linkHintEl.textContent = linkOpenHintText();
		entry.linkHintRegion = region;
		this._positionLinkHint(entry);
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
			const r = Number.parseInt(bg.slice(1, 3), 16);
			const g = Number.parseInt(bg.slice(3, 5), 16);
			const b = Number.parseInt(bg.slice(5, 7), 16);
			const a = bg.length >= 9 ? Number.parseInt(bg.slice(7, 9), 16) : 255;
			if (!Number.isNaN(r) && !Number.isNaN(g) && !Number.isNaN(b) && !Number.isNaN(a)) {
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
	 * `queue_pane` skips it entirely (parked-by-clip).
	 *
	 * Every attached pane uses this host-relative geometry; the shared
	 * WebGPU surface is the sole presentation target.
	 */
	private _recomputeViewport(entry: PaneEntry): void {
		const gh = this.globalHost;
		const cr = entry.container.getBoundingClientRect();
		// Hidden workspace tab → bbox 0×0 → degenerate scissor / kernel
		// resize. Skip; the next visible-tick ResizeObserver fire (or
		// the §A.8 host_canvas_rect that grows with the workspace
		// becoming visible) will redo this with the correct rect.
		if (cr.width <= 0 || cr.height <= 0) return;
		const hostCanvas = gh?.canvas;
		const hostMode = gh !== null && this._isHostMode(entry);
		if (!hostMode || !hostCanvas) return;
		const hr = hostCanvas.getBoundingClientRect();
		const cs = window.getComputedStyle(entry.container);
		const padL = Number.parseFloat(cs.paddingLeft) || 0;
		const padT = Number.parseFloat(cs.paddingTop) || 0;
		const padR = Number.parseFloat(cs.paddingRight) || 0;
		const padB = Number.parseFloat(cs.paddingBottom) || 0;
		const dpr = window.devicePixelRatio || 1;
		const geometry = computePaneGeometry({
			container: cr,
			host: hr,
			padding: { left: padL, top: padT, right: padR, bottom: padB },
			cellWidthCss: entry.cellW,
			cellHeightCss: entry.cellH,
			dpr,
			sharedGrid: this._sharedRemoteMode
				? { rows: entry.kernel.rows(), cols: entry.kernel.cols() }
				: undefined,
		});
		if (!geometry) return;
		entry.geometry = geometry;
		entry.geometryVisualOffsetY = this._sharedRemoteMode
			? (entry.visualOffsetY ?? 0)
			: 0;
		entry.viewport = geometry.viewportDevice;
		this._positionLinkUnderline(entry);
		this._positionLinkHint(entry);
		if (this._sharedRemoteMode) {
			entry.lastViewportKernelRows = geometry.rows;
			entry.lastViewportKernelCols = geometry.cols;
		}

		// Push offset (x, y) and size (w, h) separately. `setViewportOffset`
		// is cheap (just updates two u32 fields); `resize` triggers
		// kernel grid resize + force redraw, so we only call it when
		// dims actually changed (it short-circuits internally).
		const handle = entry.handle;
		const handleVp = handle as unknown as {
			setViewportOffset?: (x: number, y: number) => void;
		} | null;
		if (handleVp !== null && typeof handleVp.setViewportOffset === 'function') {
			handleVp.setViewportOffset(geometry.viewportDevice.x, geometry.viewportDevice.y);
		}
		entry.handle?.resize(
			Math.round(geometry.gridWidthCss),
			Math.round(geometry.gridHeightCss),
			dpr,
		);
	}

	/**
	 * Bind a pane to the manager. Binds the shared host canvas, spins up the
	 * wasm kernel/renderer, and starts observing the container
	 * for resize events.
	 *
	 * Throws if the manager isn't ready (caller must `await ready()` first)
	 * or if `paneId` is already attached.
	 *
	 * Async because browser GPU adapter/device selection is asynchronous.
	 */
	private _forwardPointerMotion(
		entry: PaneEntry,
		pending: PointerEvent,
		hoverCell: { row: number; col: number } | null,
		modes: number,
	): boolean {
		if (!shouldForwardPointerMotion(modes, pending.buttons) || !hoverCell) return false;
		const isMac = isMacPlatform();
		const last = entry.lastMouseSent;
		const buttons = pending.buttons;
		if (last?.row === hoverCell.row && last?.col === hoverCell.col && last.buttons === buttons && last.action === 2) return true;
		const bytes = entry.kernel.encodeMouse(
			hoverCell.row,
			hoverCell.col,
			mouseButtonFromButtons(buttons),
			2,
			pending.shiftKey,
			pending.ctrlKey || (isMac && pending.metaKey),
			pending.altKey,
		);
		if (bytes.length > 0) {
			entry.dataHandler?.(bytes);
			entry.lastMouseSent = { row: hoverCell.row, col: hoverCell.col, buttons, action: 2 };
		}
		this._clearLinkUnderline(entry);
		entry.container.style.cursor = '';
		delete entry.container.dataset.linkUnderline;
		delete entry.container.dataset.linkUnderlineClass;
		return true;
	}

	private _applySpanHover(entry: PaneEntry, hoverCell: { row: number; col: number }, span: LinkSpan, showHint: boolean): void {
		const regions = entry.linkSpans.regionsForSpan(entry.kernel, span).map(underlineRegionsFromSpan);
		const region = regions[0];
		if (!region) return;
		entry.container.dataset.linkUnderline = encodeUnderlineDataset(region.row, region.c0, region.c1);
		this._showLinkUnderlines(entry, regions);
		if (showHint) this._showLinkHint(entry, region);
	}

	private _applyOscHover(entry: PaneEntry, hoverCell: { row: number; col: number }, uri: string | null, showHint: boolean): void {
		const region = osc8UnderlineRegions(entry.kernel, hoverCell.row, hoverCell.col, uri)[0];
		if (!region) return;
		entry.container.dataset.linkUnderline = encodeUnderlineDataset(region.row, region.c0, region.c1);
		this._showLinkUnderlines(entry, [region]);
		if (showHint) this._showLinkHint(entry, region);
	}

		private _clearPointerHover(entry: PaneEntry): void {
			if (entry.container.style.cursor !== 'pointer' && !entry.container.dataset.linkUnderline && entry.linkUnderlineRegions.length === 0 && !entry.linkHintRegion) return;
			entry.container.style.cursor = '';
			delete entry.container.dataset.linkUnderline;
			delete entry.container.dataset.linkUnderlineClass;
			this._clearLinkUnderline(entry);
		}

		private _applyHoverDecision(
			entry: PaneEntry,
			cell: { row: number; col: number },
			link: { uri?: string } | null,
			span: LinkSpan | null,
			decision: ReturnType<typeof decideHoverUnderline>,
		): void {
			entry.container.style.cursor = decision.cursor;
			const tokens = underlineCssTokens({ show: decision.showUnderline, kind: span?.kind ?? (link ? 'osc8' : null) });
			if (tokens.length) entry.container.dataset.linkUnderlineClass = tokens.join(' ');
			else delete entry.container.dataset.linkUnderlineClass;
			if (decision.showUnderline && span) return this._applySpanHover(entry, cell, span, decision.showHint);
			if (decision.showUnderline && link) return this._applyOscHover(entry, cell, link.uri ?? null, decision.showHint);
			delete entry.container.dataset.linkUnderline;
			this._clearLinkUnderline(entry);
			if (decision.showHint && span) this._showLinkHint(entry, underlineRegionsFromSpan(span));
			if (decision.showHint && link) {
				const region = osc8UnderlineRegions(entry.kernel, cell.row, cell.col, link.uri ?? null)[0];
				if (region) this._showLinkHint(entry, region);
			}
		}

		private _updatePointerHover(entry: PaneEntry, pending: PointerEvent, hoverCell: { row: number; col: number } | null): void {
			if (!hoverCell) return this._clearPointerHover(entry);
			const link = entry.kernel.hyperlinkAt(hoverCell.row, hoverCell.col) as { uri?: string } | null;
			const span = link ? null : entry.linkSpans.hitTest(entry.kernel, hoverCell.row, hoverCell.col);
			const decision = decideHoverUnderline({
				hasLinkHit: !!(link || span),
				modifierHeld: linkModifierHeld(pending),
				isMac: isMacPlatform(),
				spanText: link?.uri ?? span?.text ?? null,
			});
			this._applyHoverDecision(entry, hoverCell, link, span, decision);
		}

	private _flushPointerMove(paneId: string, pending: PointerEvent): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const hoverCell = this.cellFromEvent(paneId, pending);
		const modes = entry.kernel.mouseReportingModes();
		if (this._forwardPointerMotion(entry, pending, hoverCell, modes)) return;
		this._updatePointerHover(entry, pending, hoverCell);
		if (entry.selecting && entry.selectionStartAbs && hoverCell) {
			entry.selectionEndAbs = { row: entry.kernel.scrollbackLen() + hoverCell.row - entry.kernel.scrollOffset(), col: hoverCell.col };
			this._syncSelection(entry);
		}
	}

	private _sendPointerDown(entry: PaneEntry, cell: { row: number; col: number }, event: PointerEvent, mod: boolean): boolean {
		const bytes = entry.kernel.encodeMouse(cell.row, cell.col, event.button, 0, event.shiftKey, mod, event.altKey);
		if (bytes.length === 0) return false;
		entry.dataHandler?.(bytes);
		entry.lastMouseSent = { row: cell.row, col: cell.col, buttons: event.buttons, action: 0 };
		try { (event.target as Element | null)?.setPointerCapture?.(event.pointerId); } catch { /* best effort */ }
		return true;
	}

	private _extendPointerSelection(entry: PaneEntry, cell: { row: number; col: number }, event: PointerEvent): boolean {
		if (!event.shiftKey || !entry.selectionStartAbs) return false;
		try { (event.target as Element | null)?.setPointerCapture?.(event.pointerId); } catch { /* best effort */ }
		entry.selecting = true;
		const row = entry.kernel.scrollbackLen() + cell.row - entry.kernel.scrollOffset();
		entry.selectionEndAbs = { row, col: cell.col };
		entry.kernel.setSelectionAbs(entry.selectionStartAbs.row, entry.selectionStartAbs.col, row, cell.col);
		this.wake();
		return true;
	}

	private _handleMultiClick(entry: PaneEntry, cell: { row: number; col: number }, detail: number): boolean {
		if (detail === 2) entry.kernel.selectWordAt(cell.row, cell.col);
		else if (detail >= 3) entry.kernel.selectLineAt(cell.row);
		else return false;
		this.wake();
		return true;
	}

	private _handlePointerDown(paneId: string, event: PointerEvent): void {
		const cell = this.cellFromEvent(paneId, event);
		const entry = this.panes.get(paneId);
		if (!cell || !entry) return;
		if (entry.mouseMoveRaf !== null) cancelAnimationFrame(entry.mouseMoveRaf);
		entry.mouseMoveRaf = null;
		entry.pendingMouseMove = null;
		const isMac = isMacPlatform();
		const linkMod = linkModifierHeld(event);
		const terminalMod = event.ctrlKey || (isMac && event.metaKey);
		const mouseReportingOn = entry.kernel.mouseReportingModes() !== 0;
		const link = entry.kernel.hyperlinkAt(cell.row, cell.col) as { uri: string; id: string | null } | null;
		const span = link ? null : entry.linkSpans.hitTest(entry.kernel, cell.row, cell.col);
		const decision = decideLinkClick({
			mouseReportingOn,
			modifierHeld: linkMod,
			hasLinkHit: !!(link?.uri || span),
			primaryButton: event.button === 0,
		});
		if (decision.openLink && this.openLinkAt(paneId, cell.row, cell.col)) {
			event.preventDefault();
			return;
		}
		if (decision.forwardToProgram && mouseReportingOn && this._sendPointerDown(entry, cell, event, terminalMod)) return;
		if (event.button !== 0 || this._extendPointerSelection(entry, cell, event)) return;
		if (this._handleMultiClick(entry, cell, event.detail)) return;
		try { (event.target as Element | null)?.setPointerCapture?.(event.pointerId); } catch { /* best effort */ }
		entry.selecting = true;
		const row = entry.kernel.scrollbackLen() + cell.row - entry.kernel.scrollOffset();
		entry.selectionStartAbs = { row, col: cell.col };
		entry.selectionEndAbs = { row, col: cell.col };
		entry.kernel.setSelectionAbs(row, cell.col, row, cell.col);
		this.wake();
	}

	private _stopAttachAutoScroll(entry: PaneEntry): void {
		if (entry.autoScrollTimer !== null) clearInterval(entry.autoScrollTimer);
		entry.autoScrollTimer = null;
		entry.autoScrollDirection = null;
	}

	private _startAttachAutoScroll(paneId: string, entry: PaneEntry, event: PointerEvent, direction: 'up' | 'down'): void {
		this._stopAttachAutoScroll(entry);
		entry.autoScrollDirection = direction;
		entry.autoScrollTimer = setInterval(() => {
			const current = this.panes.get(paneId);
			if (!current || !current.selecting || !current.selectionStartAbs) {
				if (current) this._stopAttachAutoScroll(current);
				return;
			}
			if (direction === 'up') this.scrollUp(paneId, 1);
			else this.scrollDown(paneId, 1);
			const rows = current.kernel.rows();
			const cols = current.kernel.cols();
			if (rows === 0 || cols === 0) return;
			const point = current.pendingMouseMove ?? event;
			const rect = current.container.getBoundingClientRect();
			const col = Math.max(0, Math.min(cols - 1, Math.floor((point.clientX - rect.left) / current.cellW)));
			const row = current.kernel.scrollbackLen() + (direction === 'up' ? 0 : rows - 1) - current.kernel.scrollOffset();
			current.selectionEndAbs = { row, col };
			this._syncSelection(current);
		}, 30);
	}

	private _updateAttachAutoScroll(paneId: string, entry: PaneEntry, event: PointerEvent): void {
		if (!entry.selecting || !entry.selectionStartAbs) {
			this._stopAttachAutoScroll(entry);
			return;
		}
		const rect = entry.container.getBoundingClientRect();
		const y = event.clientY - rect.top;
		let direction: 'up' | 'down' | null = null;
		// Do not treat the first/last terminal row as an auto-scroll zone.
		// The old 24px inset covers a whole short shell row, so merely moving
		// the selection inside that row started a 30ms scroll loop and made the
		// selection endpoint alternate with the viewport. Pointer capture keeps
		// delivering moves after the cursor leaves the pane, which is the only
		// point at which auto-scroll is needed.
		if (y < 0) direction = 'up';
		else if (y > rect.height) direction = 'down';
		if (!direction) {
			this._stopAttachAutoScroll(entry);
			return;
		}
		if (entry.autoScrollTimer !== null && entry.autoScrollDirection === direction) return;
		this._startAttachAutoScroll(paneId, entry, event, direction);
	}

	private _createAttachBindings(paneId: string, container: HTMLElement): Pick<PaneEntry, 'focusListener' | 'blurListener' | 'pointerDownListener' | 'pointerMoveListener' | 'pointerUpListener' | 'pointerCancelListener' | 'pointerLeaveListener' | 'modifierKeyListener'> & { linkHintEl: HTMLDivElement } {
		const focusListener = () => {
			const entry = this.panes.get(paneId);
			if (entry?.dataHandler && entry.kernel.isFocusReporting()) entry.dataHandler(new TextEncoder().encode('\x1b[I'));
		};
		const blurListener = () => {
			const entry = this.panes.get(paneId);
			if (entry?.dataHandler && entry.kernel.isFocusReporting()) entry.dataHandler(new TextEncoder().encode('\x1b[O'));
		};
		const isScrollbar = (event: PointerEvent) => !!(event.target as Element | null)?.closest?.('.rg-scrollbar-track, .rg-scrollbar-thumb');
		const flushPointerMove = () => {
			const entry = this.panes.get(paneId);
			if (!entry) return;
			const pending = entry.pendingMouseMove;
			entry.pendingMouseMove = null;
			entry.mouseMoveRaf = null;
			if (pending) this._flushPointerMove(paneId, pending);
		};
		const pointerDownListener = (event: PointerEvent) => {
			if (!isScrollbar(event)) this._handlePointerDown(paneId, event);
		};
		const pointerMoveListener = (event: PointerEvent) => {
			if (isScrollbar(event)) return;
			const entry = this.panes.get(paneId);
			if (!entry) return;
			entry.lastPointerPoint = { clientX: event.clientX, clientY: event.clientY, buttons: event.buttons, shiftKey: event.shiftKey, altKey: event.altKey, ctrlKey: event.ctrlKey, metaKey: event.metaKey };
			entry.pendingMouseMove = event;
			entry.mouseMoveRaf ??= requestAnimationFrame(flushPointerMove);
			this._updateAttachAutoScroll(paneId, entry, event);
		};
		const modifierKeyListener = (event: KeyboardEvent) => {
			if (event.key !== 'Control' && event.key !== 'Meta') return;
			const entry = this.panes.get(paneId);
			const point = entry?.lastPointerPoint;
			if (!entry || !point) return;
			const mac = isMacPlatform();
			const held = linkModifierHeld(event);
			entry.pendingMouseMove = { ...point, ctrlKey: mac ? held || event.metaKey : held, metaKey: event.metaKey } as PointerEvent;
			entry.mouseMoveRaf ??= requestAnimationFrame(flushPointerMove);
		};
		const pointerUpListener = (event: PointerEvent, force = false) => {
			if (!force && isScrollbar(event)) return;
			const entry = this.panes.get(paneId);
			if (!entry) return;
			if (entry.kernel.mouseReportingModes() !== 0) {
				const cell = this.cellFromEvent(paneId, event);
				if (cell) {
					const mac = isMacPlatform();
					const bytes = entry.kernel.encodeMouse(cell.row, cell.col, sgrReleaseButton(event.button, entry.lastMouseSent?.buttons ?? 0), 1, event.shiftKey, event.ctrlKey || (mac && event.metaKey), event.altKey);
					if (bytes.length > 0) entry.dataHandler?.(bytes);
				}
			}
			entry.selecting = false;
			entry.lastMouseSent = null;
			this._stopAttachAutoScroll(entry);
			try { (event.target as Element | null)?.releasePointerCapture?.(event.pointerId); } catch { /* best effort */ }
		};
		const pointerCancelListener = (event: PointerEvent) => pointerUpListener(event, true);
		const pointerLeaveListener = () => {
			const entry = this.panes.get(paneId);
			if (!entry) return;
			entry.lastPointerPoint = null;
			this._clearPointerHover(entry);
		};
		container.addEventListener('focusin', focusListener);
		container.addEventListener('focusout', blurListener);
		container.addEventListener('pointerdown', pointerDownListener);
		container.addEventListener('pointermove', pointerMoveListener);
		container.addEventListener('pointerup', pointerUpListener);
		container.addEventListener('pointercancel', pointerCancelListener);
		container.addEventListener('pointerleave', pointerLeaveListener);
		container.addEventListener('keydown', modifierKeyListener);
		container.addEventListener('keyup', modifierKeyListener);
		return { focusListener, blurListener, pointerDownListener, pointerMoveListener, pointerUpListener, pointerCancelListener, pointerLeaveListener, modifierKeyListener, linkHintEl: createLinkHintOverlay(container) };
	}

	private _installAttachDebugHooks(paneId: string): void {
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
			// (which would be flaky and platform-specific). Expose small
			// helpers on window so the WebDriver client can
			// `executeAsync` them.
			//
			// Production launches omit RIDGE_E2E, so writePty / feedPty /
			// installPtyWriteSpy stay inaccessible from DevTools or an XSS.
			const processE2e = (window as typeof window & { __RIDGE_E2E__?: boolean }).__RIDGE_E2E__ === true;
			if (import.meta.env.DEV || processE2e) {
				(window as unknown as {
				__windE2E?: {
					feedPty: (paneId: string, data: string) => void;
					writePty: (paneId: string, data: string) => Promise<void>;
					visibleText: (paneId: string) => string[];
					rows: (paneId: string) => number;
					cols: (paneId: string) => number;
					backendName: (paneId: string) => string | null;
					scrollbackLen: (paneId: string) => number;
					themeSnapshot: () => Record<string, string> | null;
					kernelCursor: (paneId: string) => { row: number; col: number } | null;
					presentedCursor: (paneId: string) => { row: number; col: number } | null;
					kernelThemeProbe: (paneId: string) =>
						| { bg: string; fg: string; cursor: string; tuiBg: string }
						| { error: string }
						| null;
					setTheme: (theme: Record<string, string>) => void;
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
				*/
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
				writePty: (paneId, data) => invoke('write_to_pty', {
					workspaceId: this.panes.get(paneId)?.workspaceId,
					paneId,
					data,
				}),
				visibleText: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return [];
					// kernel.dumpVisibleText returns Vec<String> as JsValue[]
					return (e.kernel.dumpVisibleText() as string[]).map(String);
				},
				rows: (paneId) => this.rows(paneId) ?? 0,
				cols: (paneId) => this.cols(paneId) ?? 0,
				backendName: (paneId) => this.backendName(paneId),
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
				presentedCursor: (paneId) => {
					const e = this.panes.get(paneId);
					if (!e) return null;
					const handle = e.handle as unknown as {
						presentedCursorRow?: () => number;
						presentedCursorCol?: () => number;
					};
					if (typeof handle.presentedCursorRow !== 'function') return null;
					const row = handle.presentedCursorRow();
					if (row < 0) return null;
					return { row, col: handle.presentedCursorCol?.() ?? 0 };
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
				// spec without needing dev-server module imports.
				setTheme: (theme) => this.setTheme(theme),
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
					if (!e?.dataHandler) return;
					const ent = e as unknown as {
						_e2ePtyWriteLog?: Array<{ data: string; at: number }>;
						_e2ePtyWriteWrappedHandler?: (bytes: Uint8Array) => void;
					};
					if (ent._e2ePtyWriteWrappedHandler === e.dataHandler) return;
					const log = ent._e2ePtyWriteLog ?? [];
					ent._e2ePtyWriteLog = log;
					const original = e.dataHandler;
					const decoder = new TextDecoder();
					const wrapped = (bytes: Uint8Array) => {
						try {
							log.push({ data: decoder.decode(bytes), at: performance.now() });
						} catch {
							// Decoder errors must NOT block the real write — spy is observation-only.
						}
						original(bytes);
					};
					ent._e2ePtyWriteWrappedHandler = wrapped;
					e.dataHandler = wrapped;
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
			};
			}
		}
	}

	private async _awaitAttachHost(): Promise<void> {
		await this._ensureDomHostStarted();
		if (this.attachHostPromise === null) return;
		await this.attachHostPromise;
	}

	private _createAttachCanvas(container: HTMLElement): AttachCanvas {
		const gh = this.globalHost;
		if (!gh) throw new Error('WEBGPU_INIT_FAILED: SurfaceHost is not attached');
		container.style.background = 'transparent';
		return { canvas: gh.canvas, hostHandle: gh.host };
	}

	private async _createAttachRenderState(canvas: HTMLCanvasElement, hostHandle: SurfaceHostHandle): Promise<AttachRenderState> {
		const handle = await this._makeHandleSerialized(canvas, hostHandle);
		const dpr = window.devicePixelRatio || 1;
		const [cellW, cellH] = handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as [number, number] | Float32Array;
		return { handle, dpr, cellW: Number(cellW), cellH: Number(cellH) };
	}

	private _attachScrollbackLines(): number {
		const settings = _hostPorts?.settings?.get() ?? null;
		return settings && Number.isFinite(settings.terminalScrollbackLines)
			? settings.terminalScrollbackLines
			: this.opts.scrollbackLines;
	}

	private _applyAttachTheme(paneId: string, handle: RenderHandle | null): void {
		const trace = typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_THEME_TRACE') === '1';
		if (this.opts.theme && handle) {
			handle.applyDefaultTheme();
			handle.applyTheme(this.opts.theme);
			if (trace) {
				const t = this.opts.theme;
				console.debug(`[theme-trace] attach paneId=${paneId.slice(0,8)} bg=${t.background ?? '∅'} fg=${t.foreground ?? '∅'} cursor=${t.cursor ?? '∅'}`);
			}
			return;
		}
		if (trace) console.debug(`[theme-trace] attach paneId=${paneId.slice(0,8)} ${handle ? 'NO_THEME (opts.theme is null — bridge hasn\'t fired yet)' : 'WORKER_PATH (theme applied by render worker)'}`);
	}

	async attach(paneId: string, container: HTMLElement, workspaceId: string): Promise<void> {
		if (!this.wasmReady) {
			throw new Error('TerminalManager.attach: call ready() first');
		}
		if (this.panes.has(paneId)) {
			throw new Error(`TerminalManager.attach: pane ${paneId} already attached`);
		}
		await this._awaitAttachHost();

		const { canvas, hostHandle } = this._createAttachCanvas(container);
		if (this.opts.paddingPx && this.opts.paddingPx > 0) {
			container.style.padding = `${this.opts.paddingPx}px`;
		}
		const { handle, dpr, cellW: cellWnum, cellH: cellHnum } =
			await this._createAttachRenderState(canvas, hostHandle);

		const scrollbackLines = this._attachScrollbackLines();
		const kernel = new TerminalKernel(24, 80, scrollbackLines);

		this._applyAttachTheme(paneId, handle);


		const bindings = this._createAttachBindings(paneId, container);

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
			lastViewportKernelRows: -1,
			lastViewportKernelCols: -1,
			pendingFitTimer: null,
			initialFitTimer: null,
			initialFitAttempt: 0,
			syncStart: null,
			syncTimeoutRendered: false,
			renderPending: true,
			deltaQueue: [],
			deltaQueueHead: 0,
			deltaQueuedBytes: 0,
			tuiCursorSuppressUntil: 0,
			tuiCursorSuppressed: false,
			focusListener: bindings.focusListener,
			blurListener: bindings.blurListener,
			selecting: false,
			selectionStartAbs: null,
			selectionEndAbs: null,
			lastMouseSent: null,
			pendingMouseMove: null,
			mouseMoveRaf: null,
			autoScrollTimer: null,
			autoScrollDirection: null,
			pointerDownListener: bindings.pointerDownListener,
			pointerMoveListener: bindings.pointerMoveListener,
			pointerUpListener: bindings.pointerUpListener,
			pointerCancelListener: bindings.pointerCancelListener,
			pointerLeaveListener: bindings.pointerLeaveListener,
			modifierKeyListener: bindings.modifierKeyListener,
			lastPointerPoint: null,
			parked: false,
			rendererRetained: false,
			parkReason: null,
			lastForegroundAt: Date.now(),
			imeAnchor: null,
			imeAnchorRaf: null,
			imeAnchorHandler: null,
			imeCompositionActive: false,
			feedBuffer: null,
			feedBufferChunks: [],
			feedBufferBytes: 0,
			feedFlushTimer: null,
			linkSpans: new LinkSpanIndex(),
			linkUnderlineEls: [],
			linkUnderlineRegions: [],
			linkHintEl: bindings.linkHintEl,
			linkHintRegion: null,
			lastScrollOffset: -1,
			lastScrollTotal: -1,
			scrollStateHandler: null,
			feedDeferred: null,
			feedDeferredChunks: [],
			feedDeferredBytes: 0,
			feedDroppedBytes: 0,
			feedDropCount: 0,
			feedNeedsResync: false,
			inputStartRow: null,
			inputStartCol: null,
		};
		entry.resizeObserver.observe(container);

		this.panes.set(paneId, entry);
		let workspacePaneIds = this.paneIdsByWorkspace.get(workspaceId);
		if (!workspacePaneIds) {
			workspacePaneIds = new Set<string>();
			this.paneIdsByWorkspace.set(workspaceId, workspacePaneIds);
		}
		workspacePaneIds.add(paneId);

		// The worker mirrors kernel state only; presentation stays on the
		// main-thread WebGPU host, so attach keeps the canvas local.

		// Initial fit: do it once synchronously after layout settles. We
		// wait one rAF (so SvelteKit hydration finishes), then fit
		// directly without debounce so the PTY gets sized before any
		// shell output arrives.
		// iter-60 G2: in shared-remote (desktop-in-browser) mode the initial
		// fit CLAIMS the PTY at this viewer's size — without this the pane
		// letterboxes the host's grid forever ("shell 渲染区不填满 pane" bug).
		// Passive fits afterwards still only re-letterbox (multi-viewer safe).
		this._scheduleInitialFit(entry);
		this._installAttachDebugHooks(paneId);
		this.startRafLoop();
	}

	/**
	 * Tear down a pane completely. Frees the kernel, frees the render
	 * handle, removes the canvas, and drops the entry from the map.
	 *
	 * Idempotent against parking state: if the pane is currently parked,
	 * its kernel stays alive and a component switch may still retain the
	 * renderer; both are released here. Caller must use `detach` for
	 * "the pane is permanently gone" (e.g. removed from paneTree) and
	 * `park` for "transient unmount across split / reparent" — see §5.1.
	 */
	private _removeLinkOverlays(entry: PaneEntry): void {
		this._clearLinkUnderline(entry);
		for (const el of entry.linkUnderlineEls) {
			try { el.remove(); } catch { /* already detached */ }
		}
		entry.linkUnderlineEls = [];
		try { entry.linkHintEl?.remove(); } catch { /* already detached */ }
		entry.linkHintEl = null;
	}

	private _detachDomBindings(entry: PaneEntry): void {
		entry.resizeObserver.disconnect();
		entry.container.removeEventListener('focusin', entry.focusListener);
		entry.container.removeEventListener('focusout', entry.blurListener);
		entry.container.removeEventListener('pointerdown', entry.pointerDownListener);
		entry.container.removeEventListener('pointermove', entry.pointerMoveListener);
		entry.container.removeEventListener('pointerup', entry.pointerUpListener);
		entry.container.removeEventListener('pointercancel', entry.pointerCancelListener);
		entry.container.removeEventListener('pointerleave', entry.pointerLeaveListener);
		entry.container.removeEventListener('keydown', entry.modifierKeyListener);
		entry.container.removeEventListener('keyup', entry.modifierKeyListener);
		if (entry.mouseMoveRaf !== null) cancelAnimationFrame(entry.mouseMoveRaf);
		entry.mouseMoveRaf = null;
		entry.pendingMouseMove = null;
		entry.lastMouseSent = null;
		if (entry.autoScrollTimer !== null) clearInterval(entry.autoScrollTimer);
		entry.autoScrollTimer = null;
		entry.autoScrollDirection = null;
		if (entry.pendingFitTimer !== null) clearTimeout(entry.pendingFitTimer);
		entry.pendingFitTimer = null;
		this._cancelInitialFit(entry);
		entry.imeAnchorHandler = null;
	}

	private _releaseRenderer(entry: PaneEntry): void {
		this._releaseTuiCursorSuppression(entry);
		if (this._isHostMode(entry)) this._invalidateHost();
		else {
			try { entry.canvas.remove(); } catch { /* already detached */ }
		}
		const handle = entry.handle;
		entry.handle = null;
		try { handle?.free(); } catch { /* ignore */ }
		entry.rendererRetained = false;
	}

	private _releaseDetachedResources(entry: PaneEntry): void {
		if (!entry.parked) this._detachDomBindings(entry);
		if (entry.rendererRetained || !entry.parked) this._releaseRenderer(entry);
		if (entry.imeAnchorRaf !== null) cancelAnimationFrame(entry.imeAnchorRaf);
		entry.imeAnchorRaf = null;
		entry.imeAnchorHandler = null;
		dropPendingFeedBuffers(entry);
		this._syncPendingFrameWork(entry);
		try { entry.kernel.free(); } catch { /* ignore */ }
	}

	detach(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		this._memoryRestorePending.delete(paneId);
		this._removeLinkOverlays(entry);
		this._releaseDetachedResources(entry);
		if (this._focusedPaneByWorkspace.get(entry.workspaceId) === paneId) {
			this._focusedPaneByWorkspace.delete(entry.workspaceId);
		}
		this.panes.delete(paneId);
		const workspacePaneIds = this.paneIdsByWorkspace.get(entry.workspaceId);
		workspacePaneIds?.delete(paneId);
		if (workspacePaneIds?.size === 0) this.paneIdsByWorkspace.delete(entry.workspaceId);
		this.pendingFrameWorkPanes.delete(paneId);
		if (this.panes.size === 0) this.stopRafLoop();
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
	park(paneId: string, reason: 'component' | 'memory' = 'component'): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		if (entry.parked) {
			this._updateParkedEntry(entry, reason);
			return;
		}
		if (this._focusedPaneByWorkspace.get(entry.workspaceId) === paneId) {
			this._focusedPaneByWorkspace.delete(entry.workspaceId);
			entry.handle?.setFocused(false);
		}
		this._releaseTuiCursorSuppression(entry);
		this._detachParkedBindings(entry);
		entry.imeCompositionActive = false;
		entry.imeAnchor = null;
		const retainRenderer = this._shouldRetainRenderer(entry, reason);
		this._releaseParkedCanvas(entry, retainRenderer);
		this._flushFeedBuffer(entry);
		this._replaceParkedContainer(entry, reason);
		entry.rendererRetained = retainRenderer;
		entry.parked = true;
		entry.parkReason = reason;
		// Don't stopRafLoop here — other panes may still need rendering.
		// The render-loop guards against parked entries by checking the
		// flag before calling `entry.handle.render(...)`.
	}

	private _updateParkedEntry(entry: PaneEntry, reason: 'component' | 'memory'): void {
		if (reason === 'component') {
			entry.parkReason = 'component';
			return;
		}
		if (!entry.rendererRetained) return;
		this._releaseRenderer(entry);
		entry.parkReason = 'memory';
	}

	private _detachParkedBindings(entry: PaneEntry): void {
		this._detachDomBindings(entry);
		this._removeLinkOverlays(entry);
		if (entry.pendingFitTimer !== null) clearTimeout(entry.pendingFitTimer);
		entry.pendingFitTimer = null;
		this._cancelInitialFit(entry);
		entry.selecting = false;
		entry.selectionStartAbs = null;
		entry.selectionEndAbs = null;
	}

	private _shouldRetainRenderer(entry: PaneEntry, reason: 'component' | 'memory'): boolean {
		return reason === 'component' && entry.handle !== null;
	}

	private _releaseParkedCanvas(entry: PaneEntry, retainRenderer: boolean): void {
		if (this._isHostMode(entry)) {
			if (shouldWipeHostOnPaneRemount(retainRenderer)) this._invalidateHost();
		} else {
			try { entry.canvas.remove(); } catch { /* already detached */ }
		}
		if (retainRenderer) return;
		const handle = entry.handle;
		entry.handle = null;
		try { handle?.free(); } catch { /* ignore */ }
	}

	private _replaceParkedContainer(entry: PaneEntry, reason: 'component' | 'memory'): void {
		if (reason === 'component' && typeof document !== 'undefined') {
			entry.container = document.createElement('div');
		}
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
		await this._ensureDomHostStarted();
		await this._waitForHostAttach();
		if (this.panes.get(paneId) !== entry || !entry.parked) return;
		const resources = await this._prepareUnparkResources(container, entry);
		this._commitUnpark(paneId, container, entry, resources);
	}

	private async _waitForHostAttach(): Promise<void> {
		if (this.attachHostPromise === null) return;
		await this.attachHostPromise;
	}

	private _selectUnparkCanvas(
		container: HTMLElement,
	): { canvas: HTMLCanvasElement; hostHandle: SurfaceHostHandle } {
		const gh = this.globalHost;
		if (!gh) throw new Error('WEBGPU_INIT_FAILED: SurfaceHost is not attached');
		container.style.background = 'transparent';
		return { canvas: gh.canvas, hostHandle: gh.host };
	}

	private async _prepareUnparkResources(
		container: HTMLElement,
		entry: PaneEntry,
	): Promise<{ canvas: HTMLCanvasElement; hostHandle: SurfaceHostHandle; handle: RenderHandle | null; dpr: number }> {
		const selected = this._selectUnparkCanvas(container);
		if (this.opts.paddingPx && this.opts.paddingPx > 0) container.style.padding = `${this.opts.paddingPx}px`;
		const retained = selected.canvas === entry.canvas && entry.rendererRetained === true && entry.handle !== null;
		if (entry.rendererRetained && !retained) {
			const oldHandle = entry.handle;
			entry.handle = null;
			try { oldHandle?.free(); } catch { /* stale retained renderer */ }
			entry.rendererRetained = false;
		}
		const handle = retained ? entry.handle : await this._makeHandleSerialized(selected.canvas, selected.hostHandle);
		return { ...selected, handle, dpr: window.devicePixelRatio || 1 };
	}

	private _commitUnpark(
		paneId: string,
		container: HTMLElement,
		entry: PaneEntry,
		resources: { canvas: HTMLCanvasElement; hostHandle: SurfaceHostHandle; handle: RenderHandle | null; dpr: number },
	): void {
		if (this.panes.get(paneId) !== entry || !entry.parked) {
			try { resources.handle?.free(); } catch { /* best-effort abandoned restore cleanup */ }
			return;
		}
		const { handle, dpr, canvas, hostHandle } = resources;
		const wipeHost = shouldWipeHostOnPaneRemount(entry.rendererRetained);
		const linkHintEl = createLinkHintOverlay(container);
		const [cellW, cellH] = handle
			? (handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as [number, number] | Float32Array)
			: ([entry.cellW, entry.cellH] as [number, number]);
		if (handle && this.opts.theme) handle.applyTheme(this.opts.theme);
		Object.assign(entry, {
			container, canvas, handle, rendererRetained: false, linkUnderlineEls: [],
			linkUnderlineRegions: [], linkHintEl, linkHintRegion: null,
			cellW: Number(cellW), cellH: Number(cellH),
			lastConfiguredDpr: dpr, lastReportedRows: -1, lastReportedCols: -1, lastAppliedPaddingPx: undefined,
			renderPending: true,
		});
		if (wipeHost) this._invalidateHost();
		this._bindUnparkListeners(paneId, container, entry);
		entry.parked = false;
		entry.parkReason = null;
		entry.lastForegroundAt = Date.now();
		entry.wasHiddenLastTick = true;
		this._scheduleInitialFit(entry);
		this.startRafLoop();
	}

	private _bindUnparkListeners(paneId: string, container: HTMLElement, entry: PaneEntry): void {
		container.addEventListener('focusin', entry.focusListener);
		container.addEventListener('focusout', entry.blurListener);
		container.addEventListener('pointerdown', entry.pointerDownListener);
		container.addEventListener('pointermove', entry.pointerMoveListener);
		container.addEventListener('pointerup', entry.pointerUpListener);
		container.addEventListener('pointercancel', entry.pointerCancelListener);
		container.addEventListener('pointerleave', entry.pointerLeaveListener);
		container.addEventListener('keydown', entry.modifierKeyListener);
		container.addEventListener('keyup', entry.modifierKeyListener);
		entry.resizeObserver = new ResizeObserver(() => this.viewportChanged(paneId));
		entry.resizeObserver.observe(container);
	}

	/** True if a pane is in the manager but currently parked.
	 *  Useful for the RidgePane onMount path to decide attach vs unpark. */
	isParked(paneId: string): boolean {
		const entry = this.panes.get(paneId);
		return entry?.parked ?? false;
	}

	private _isInlineTui(entry: PaneEntry): boolean {
		try { return entry.kernel.isInlineTuiMode(); }
		catch { return false; }
	}

	private _feedInline(entry: PaneEntry, bytes: Uint8Array): void {
		// VTE parsing is stateful across calls, so CSI/OSC fragments do not
		// need a timer-backed buffer. Feed immediately; the manager's single
		// RAF wake still coalesces the paint, without adding an 8 ms input/output
		// cadence to inline TUIs such as Codex.
		if (entry.feedBuffer !== null || entry.feedBufferChunks.length > 0) {
			this._flushFeedBuffer(entry);
		}
		this._feedNow(entry, bytes);
	}

	private _feedImmediate(entry: PaneEntry, bytes: Uint8Array): void {
		if (entry.feedBuffer !== null || entry.feedBufferChunks.length > 0) this._flushFeedBuffer(entry);
		this._feedNow(entry, bytes);
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
		if (this._isInlineTui(entry)) this._feedInline(entry, bytes);
		else this._feedImmediate(entry, bytes);
	}

	/** Queue external/remote PTY bytes for the shared compositor budget. This
	 * keeps several remote split panes from parsing back-to-back network frames
	 * inside one callback; local PTY paths retain the synchronous `feed()` API. */
	enqueueFeed(paneId: string, data: string | Uint8Array): boolean {
		const entry = this.panes.get(paneId);
		if (!entry) return false;
		const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
		if (bytes.byteLength === 0) return true;
		if (entry.feedBuffer !== null || entry.feedBufferChunks.length > 0) this._flushFeedBuffer(entry);
		const result = enqueueDeferredFeed(entry, bytes);
		this._syncPendingFrameWork(entry);
		entry.renderPending = true;
		this.wake();
		return result.droppedBytes === 0;
	}

	/** Read bounded render-queue diagnostics without exposing kernel internals. */
	feedStats(paneId: string): {
		queuedBytes: number;
		droppedBytes: number;
		dropCount: number;
		needsResync: boolean;
	} | null {
		const entry = this.panes.get(paneId);
		if (!entry) return null;
		return {
			queuedBytes: entry.feedDeferredBytes,
			droppedBytes: entry.feedDroppedBytes,
			dropCount: entry.feedDropCount,
			needsResync: entry.feedNeedsResync,
		};
	}

	/**
	 * Fast-path a pane that just became visible. Normal PTY back-pressure drains
	 * deferred chunks over RAF ticks so a noisy pane cannot monopolise the UI;
	 * after a user switch, however, showing the latest tail is more important
	 * than preserving that background fairness. Spend one bounded slice now,
	 * then let the regular rotated drain finish the remainder.
	 */
	flushPaneFeed(paneId: string, budgetMs = MAX_PANE_FEED_FLUSH_BUDGET_MS): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked || !hasDeferredFeed(entry)) return;
		const boundedBudgetMs = Number.isFinite(budgetMs)
			? Math.min(Math.max(0, budgetMs), MAX_PANE_FEED_FLUSH_BUDGET_MS)
			: MAX_PANE_FEED_FLUSH_BUDGET_MS;
		const start = performance.now();
		while (hasDeferredFeed(entry)) {
			const elapsed = performance.now() - start;
			if (elapsed >= boundedBudgetMs) break;
			const chunk = takeDeferredFeed(entry);
			if (!chunk) break;
			this._feedNow(entry, chunk, Math.max(1, boundedBudgetMs - elapsed), true);
		}
		this._syncPendingFrameWork(entry);
		this.wake();
	}

	/** Cancel queued render bytes before a transport-level full resync. */
	clearPendingFeed(paneId: string): number {
		const entry = this.panes.get(paneId);
		if (!entry) return 0;
		const dropped = dropPendingFeedBuffers(entry);
		this._syncPendingFrameWork(entry);
		return dropped;
	}

	/** §A.4 — feed bytes to the kernel synchronously, including PTY trace,
	 *  reply / event drain, and rAF wake. Extracted from `feed()` so the
	 *  inline-TUI path can call it directly without consulting the gate.
	 *  Always feeds — does NOT consult the inline-TUI gate; paint remains
	 *  coalesced by the manager's RAF wake.
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
		private _traceFeed(entry: PaneEntry, bytes: Uint8Array): void {
			if (typeof localStorage === 'undefined' || localStorage.RIDGE_PTY_TRACE !== '1') return;
			const hex = Array.from(bytes.slice(0, 256)).map((byte) => byte.toString(16).padStart(2, '0')).join('');
			const more = bytes.length > 256 ? `…${bytes.length - 256}B` : '';
			console.debug(`[pty-trace][${performance.now().toFixed(1)}ms][${entry.paneId.slice(0, 8)}][${bytes.length}B] ${hex}${more}`);
		}

		private _feedChunks(
			entry: PaneEntry,
			bytes: Uint8Array,
			budgetMs: number,
		): { offset: number; requiresRenderSettle: boolean } {
			const chunkBytes = 16 * 1024;
			let offset = 0;
			let requiresRenderSettle = false;
			const start = performance.now();
			const traceCursor = typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_CURSOR_TRACE') === '1';
			const kernel = entry.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
			const before = traceCursor ? `(${kernel.cursorRow()},${kernel.cursorCol()})` : '';
			while (offset < bytes.length) {
				const end = Math.min(offset + chunkBytes, bytes.length);
				const chunk = bytes.subarray(offset, end);
				requiresRenderSettle = (
					entry.kernel as unknown as { feed: (data: Uint8Array) => boolean | void }
				).feed(chunk) === true || requiresRenderSettle;
				offset = end;
				if (performance.now() - start >= budgetMs) break;
			}
			if (traceCursor) console.debug(`[cursor-trace][${performance.now().toFixed(1)}ms] feed paneId=${entry.paneId.slice(0, 8)} bytes=${bytes.length} consumed=${offset} cursor ${before}→${kernel.cursorRow()},${kernel.cursorCol()})`);
			return { offset, requiresRenderSettle };
		}

		private _drainFeedOutputs(entry: PaneEntry): void {
			const reply = entry.kernel.takePendingResponse();
			if (reply.length > 0 && entry.dataHandler) entry.dataHandler(reply);
			const events = entry.kernel.takePendingEvents() as KernelEvent[];
			if (entry.eventHandler) {
				for (const event of events) entry.eventHandler(event);
			} else if (events.length > 0 && import.meta.env?.DEV) {
				console.warn('[ridge-term] feed() drained kernel events without an eventHandler', entry.paneId, events.length);
			}
		}

		private _feedNow(
			entry: PaneEntry,
			bytes: Uint8Array,
			budgetMs = FEED_PER_CALL_BUDGET_MS,
			drainingDeferred = false,
		): void {
			this._traceFeed(entry, bytes);
			entry.renderPending = true;
			if (!drainingDeferred && hasDeferredFeed(entry)) {
				enqueueDeferredFeed(entry, bytes.slice());
				this._syncPendingFrameWork(entry);
				this.wake();
				return;
			}
			const { offset, requiresRenderSettle } = this._feedChunks(entry, bytes, budgetMs);
			if (offset < bytes.length) {
				const remainder = bytes.slice(offset);
				if (drainingDeferred) prependDeferredFeed(entry, remainder);
				else enqueueDeferredFeed(entry, remainder);
			}
			this._syncPendingFrameWork(entry);
			entry.linkSpans.markDirty();
			if (requiresRenderSettle) this._noteTuiCursorSettle(entry, performance.now());
			this.wake();
			this._drainFeedOutputs(entry);
		}

	/** Queue one native parser delta for the next compositor turn. The Tauri
	 * Channel callback does O(1) work only; decoding and rendering remain under
	 * the same focus-first frame budget as raw PTY feeds. */
	enqueueDeltaFrame(
		paneId: string,
		bytes: Uint8Array,
		onError?: (error: unknown) => void,
	): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.deltaQueue.push({ bytes, onError });
		entry.deltaQueuedBytes += bytes.byteLength;
		this._syncPendingFrameWork(entry);
		entry.renderPending = true;
		this.wake();
	}

	/** Public immediate path retained for DEV hooks and direct callers. Desktop
	 * Channel delivery uses `enqueueDeltaFrame` above, so it cannot run parser
	 * work in a burst of IPC callbacks. */
	applyDeltaFrame(paneId: string, bytes: Uint8Array): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		if (this._applyDeltaFrame(entry, bytes)) this._noteTuiCursorSettle(entry, performance.now());
	}

	private _applyDeltaFrame(entry: PaneEntry, bytes: Uint8Array): boolean {
		return perfMark('rg.ptyDelta.apply', () => {
			if ((globalThis as { __RIDGE_PERF_TRACE?: unknown }).__RIDGE_PERF_TRACE === true) {
				const traceHost = globalThis as { __ridgePtyDeltaTrace?: number[] };
				traceHost.__ridgePtyDeltaTrace?.push(bytes.byteLength);
			}
			const paneId = entry.paneId;
			const traceCursor = typeof localStorage !== 'undefined' && localStorage.getItem('RIDGE_CURSOR_TRACE') === '1';
			const k = entry.kernel as unknown as { cursorRow: () => number; cursorCol: () => number };
			const pre = traceCursor ? `(${k.cursorRow()},${k.cursorCol()})` : '';
			// The v5 return value is a native-parser repaint hint. Cast keeps this
			// source compatible with an already-running v4 dev bundle until wasm
			// hot-reloads; stale bundles simply yield `undefined` (immediate path).
			const requiresRenderSettle = (
				entry.kernel as unknown as { applyDeltaFrame: (frame: Uint8Array) => boolean | void }
			).applyDeltaFrame(bytes) === true;
			if (traceCursor) {
				const ts = performance.now().toFixed(1);
				// eslint-disable-next-line no-console
				console.debug(`[cursor-trace][${ts}ms] applyDeltaFrame paneId=${paneId.slice(0,8)} bytes=${bytes.length} cursor ${pre}→(${k.cursorRow()},${k.cursorCol()})`);
			}
			const reply = entry.kernel.takePendingResponse();
			if (reply.length > 0 && entry.dataHandler) entry.dataHandler(reply);
			const events = entry.kernel.takePendingEvents() as KernelEvent[];
			if (entry.eventHandler) {
				for (const ev of events) entry.eventHandler(ev);
			}
			entry.linkSpans.markDirty();
			entry.renderPending = true;
			this.wake();
			return requiresRenderSettle;
		});
	}

	private _takeQueuedDeltaFrame(entry: PaneEntry): QueuedDeltaFrame | null {
		const frame = entry.deltaQueue[entry.deltaQueueHead] ?? null;
		if (frame === null) return null;
		entry.deltaQueueHead += 1;
		entry.deltaQueuedBytes = Math.max(0, entry.deltaQueuedBytes - frame.bytes.byteLength);
		if (entry.deltaQueueHead === entry.deltaQueue.length) {
			entry.deltaQueue.length = 0;
			entry.deltaQueueHead = 0;
		} else if (entry.deltaQueueHead >= 64 && entry.deltaQueueHead * 2 >= entry.deltaQueue.length) {
			entry.deltaQueue = entry.deltaQueue.slice(entry.deltaQueueHead);
			entry.deltaQueueHead = 0;
		}
		this._syncPendingFrameWork(entry);
		return frame;
	}

	private _clearQueuedDeltaFrames(entry: PaneEntry): void {
		entry.deltaQueue.length = 0;
		entry.deltaQueueHead = 0;
		entry.deltaQueuedBytes = 0;
		this._syncPendingFrameWork(entry);
	}

	private _inputPending(): boolean {
		if (typeof navigator === 'undefined') return false;
		try {
			const scheduling = (navigator as unknown as {
				scheduling?: { isInputPending?: (options?: { includeContinuous?: boolean }) => boolean };
			}).scheduling;
			return scheduling?.isInputPending?.({ includeContinuous: true }) === true;
		} catch {
			return false;
		}
	}

	private _setPresentationCursorSuppressed(entry: PaneEntry, suppressed: boolean): void {
		if (entry.tuiCursorSuppressed === suppressed) return;
		entry.tuiCursorSuppressed = suppressed;
		const handle = entry.handle as unknown as {
			setPresentationCursorSuppressed?: (value: boolean) => void;
		} | null;
		try { handle?.setPresentationCursorSuppressed?.(suppressed); }
		catch { /* an already-running wasm bundle keeps its prior safe behavior */ }
	}

	private _noteTuiCursorSettle(entry: PaneEntry, now: number): void {
		// The worker owns its own renderer transaction. Main-thread panes paint
		// text immediately and freeze only the cursor during the rewind burst.
		if (entry.handle === null) return;
		entry.tuiCursorSuppressUntil = now + TUI_CURSOR_SETTLE_MS;
		this._setPresentationCursorSuppressed(entry, true);
	}

	private _releaseTuiCursorSuppression(entry: PaneEntry): void {
		entry.tuiCursorSuppressUntil = 0;
		this._setPresentationCursorSuppressed(entry, false);
	}

	private _drainQueuedDeltaFrames(order: readonly PaneEntry[]): void {
		const started = performance.now();
		let applied = 0;
		for (const entry of order) {
			while (entry.deltaQueueHead < entry.deltaQueue.length) {
				// Always make one unit of FIFO progress; thereafter yield first to
				// input and then to paint. One malformed frame drops the pending
				// delta queue before the bridge switches the pane back to raw mode.
				if (applied > 0 && (this._inputPending() || performance.now() - started >= DELTA_FRAME_BUDGET_MS)) return;
				const frame = this._takeQueuedDeltaFrame(entry);
				if (!frame) break;
				try {
				if (this._applyDeltaFrame(entry, frame.bytes)) this._noteTuiCursorSettle(entry, performance.now());
				} catch (error) {
					this._clearQueuedDeltaFrames(entry);
					frame.onError?.(error);
					break;
				}
				applied += 1;
			}
		}
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
		const started = performance.now();
		for (const entry of order) {
			let drained = 0;
			while (hasDeferredFeed(entry) && drained < MAX_DEFERRED_CHUNKS_PER_FRAME) {
				// Preserve a small frame budget for paint/input and rotate to the
				// next pane once it is spent; this catches up ordinary bursts without
				// letting one noisy PTY monopolise the Remote main thread.
				if (performance.now() - started >= FEED_FRAME_BUDGET_MS) return;
				const buf = takeDeferredFeed(entry);
				if (!buf) break;
				// Remove one chunk BEFORE _feedNow — the call re-enqueues only
				// the unconsumed remainder, preserving order behind older chunks.
				this._feedNow(entry, buf);
				drained += 1;
			}
		}
	}

	/** §A.4 — flush any pending coalesced bytes to the kernel and clear the
	 *  timer. Safe to call when the buffer is empty (no-op). */
	private _flushFeedBuffer(entry: PaneEntry): void {
		if (entry.feedFlushTimer !== null) {
			clearTimeout(entry.feedFlushTimer);
			entry.feedFlushTimer = null;
		}
		let first = entry.feedBuffer;
		const chunks = entry.feedBufferChunks;
		// Keep the FIFO lossless even if a legacy/diagnostic caller populated
		// chunks without the head fragment.
		if (first === null && chunks.length > 0) first = chunks.shift()!;
		const total = entry.feedBufferBytes ||
			(first?.byteLength ?? 0) + chunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
		if (first === null || total === 0) {
			entry.feedBuffer = null;
			chunks.length = 0;
			entry.feedBufferBytes = 0;
			return;
		}
		entry.feedBuffer = null;
		entry.feedBufferBytes = 0;
		if (chunks.length === 0) {
			this._feedNow(entry, first);
			return;
		}
		const buf = new Uint8Array(total);
		let offset = 0;
		buf.set(first, offset);
		offset += first.byteLength;
		for (const chunk of chunks) {
			buf.set(chunk, offset);
			offset += chunk.byteLength;
		}
		chunks.length = 0;
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
	prependScrollback(paneId: string, data: string | Uint8Array): boolean {
		const entry = this.panes.get(paneId);
		if (!entry) return false;
		const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
		if (bytes.length === 0) return false;
		entry.kernel.prependScrollback(bytes);
		entry.renderPending = true;
		// No selection / search clear here: prepend grows the scrollback
		// at its older end and leaves all existing rows in place, so any
		// currently-active selection or search anchor is still valid.
		// Likewise no pending_response / pending_events to drain — the
		// kernel discards both for prepend-mode bytes by design.
		this.wake();
		return true;
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
		if (!entry?.dataHandler) return;
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
	 *
	 * @param isTui When true the TUI owns the keyboard; host Copy on
	 * Ctrl+C (selection present) is skipped so the TUI always receives
	 * `\x03`. The caller should pass the result of `isTuiSticky()`.
	 */
	handleKeyDown(paneId: string, ev: KeyboardEvent, isTui: boolean = false): boolean {
		const entry = this.panes.get(paneId);
		if (!entry?.dataHandler) return false;

		// macOS: treat Cmd as Ctrl for terminal apps.
		const isMac = isMacPlatform();
		const ctrl = ev.ctrlKey || (isMac && ev.metaKey);

		// Handle OS native Copy on Ctrl+C / Cmd+C when text is selected.
		// When a TUI owns the keyboard the Ctrl+C byte always belongs to
		// the TUI (SIGINT / TUI keybinding). Host copy is still reachable
		// via Ctrl+Shift+C (handled in RidgePane).
		const isCtrlC = ctrl && ev.key.toLowerCase() === 'c';
		if (isCtrlC && !isTui) {
			if (copySelectionIfPresent(entry)) {
				// Don't encode \x03, instead copy and clear selection
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
		traceKeydown(entry, ev, bytes);
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
		if (!entry?.dataHandler) return false;
		if (entry.kernel.mouseReportingModes() === 0) return false;

		const cell = this.cellFromEvent(paneId, ev);
		if (!cell) return false;
		const { row, col } = cell;

		const delta = ev.deltaY;
		if (delta === 0) return false;

		const isMac = isMacPlatform();
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
	 * Alternate-scroll fallback: when a full-screen (alt-screen) application
	 * is active but has NOT enabled DEC mouse reporting, translate the wheel
	 * into cursor-key presses so pagers / menus that only read arrow keys
	 * (less, man, git log, fzf, claude /theme menu, …) scroll on wheel. This
	 * mirrors the `alternateScroll` resource enabled by default in xterm,
	 * Windows Terminal, iTerm2 and kitty.
	 *
	 * Returns true (caller should `preventDefault`) only when bytes were
	 * actually emitted. Conditions, all required:
	 *   - alt-screen active (a TUI owns the primary→alt swap),
	 *   - mouse reporting OFF (else `handleWheel` already owns the event),
	 *   - there is no in-kernel scrollback to scroll instead (alt screen has
	 *     none, but guard anyway so primary-screen scrollback keeps winning).
	 *
	 * Arrow encoding is delegated to `kernel.encodeKey('ArrowUp'/'ArrowDown')`
	 * so DECCKM (app-cursor-keys mode `\x1bOA` vs `\x1b[A`) is honoured by the
	 * same code path as a real keypress — no second encoding to drift.
	 *
	 * One arrow press per ~`WHEEL_LINES_DIVISOR` px of deltaY, clamped to a
	 * small max so a fast flick can't fire dozens of presses in one event.
	 */
	wheelAltScroll(paneId: string, ev: WheelEvent): boolean {
		const entry = this.panes.get(paneId);
		if (!entry?.dataHandler) return false;
		if (!entry.kernel.isAltScreen()) return false;
		if (entry.kernel.mouseReportingModes() !== 0) return false;
		const delta = ev.deltaY;
		if (delta === 0) return false;
		// deltaMode 1 = lines, 2 = pages; treat their units as ~1 press each,
		// deltaMode 0 = pixels → 1 press per WHEEL_LINES_DIVISOR px.
		const WHEEL_LINES_DIVISOR = 30;
		const MAX_PRESSES_PER_EVENT = 5;
		const magnitude = ev.deltaMode === 0 ? Math.abs(delta) / WHEEL_LINES_DIVISOR : Math.abs(delta);
		const presses = Math.max(1, Math.min(MAX_PRESSES_PER_EVENT, Math.round(magnitude)));
		const key = delta < 0 ? 'ArrowUp' : 'ArrowDown';
		const oneArrow = entry.kernel.encodeKey(key, false, false, false, false);
		if (oneArrow.length === 0) return false;
		const seq = new Uint8Array(oneArrow.length * presses);
		for (let i = 0; i < presses; i++) seq.set(oneArrow, i * oneArrow.length);
		entry.dataHandler(seq);
		return true;
	}

	/**
	 * Soft-reset the pane's terminal *input* modes. Clears the DEC private
	 * modes a crashed TUI can leave stuck — mouse tracking (?1000/1002/1003 +
	 * ?1005/1006/1015 encodings), focus reporting (?1004), bracketed paste
	 * (?2004) — restores the text cursor (?25h) and leaves the alt-screen
	 * (?1049/1047/47). The DECRST bytes are fed into the kernel only (local
	 * emulator state); nothing is written to the PTY, so the running shell is
	 * untouched. Scrollback and screen contents are preserved (soft reset, not
	 * RIS).
	 *
	 * Use case: a TUI (e.g. an Ink/Bun app) segfaults without emitting its
	 * mode-restore sequences, leaving the terminal forwarding wheel scrolls as
	 * SGR mouse reports into the shell ("[<65;…M" garbage). Bound to a host
	 * shortcut (Ctrl+Shift+R) so it stays reachable even while the TUI gate
	 * would suppress the pane's own right-click / shortcuts.
	 */
	resetInputModes(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const seq =
			'\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l' + // mouse tracking
			'\x1b[?1005l\x1b[?1006l\x1b[?1015l' + // mouse report encodings (UTF8/SGR/urxvt)
			'\x1b[?1004l' + // focus reporting
			'\x1b[?2004l' + // bracketed paste
			'\x1b[?25h' + // show cursor
			'\x1b[?1049l\x1b[?1047l\x1b[?47l'; // leave alt-screen (all variants)
		try {
			const bytes = new TextEncoder().encode(seq);
			entry.kernel.feed(bytes);
		} catch {
			return; // kernel may be freed mid-teardown
		}
		entry.linkSpans.markDirty();
		this.wake();
		const reply = entry.kernel.takePendingResponse();
		if (reply.length > 0 && entry.dataHandler) entry.dataHandler(reply);
	}

	/**
	 * Paste text into the pane. Wraps in bracketed-paste markers if mode 2004
	 * is active. Pushes through onData.
	 */
	paste(paneId: string, text: string): void {
		const entry = this.panes.get(paneId);
		if (!entry?.dataHandler) return;
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
		if (ent.geometry && this._sharedRemoteMode) {
			const rows = ent.kernel.rows();
			const cols = ent.kernel.cols();
			if (rows === 0 || cols === 0) return null;
			return cellFromVisualClientPoint(
				ent.geometry,
				e.clientX,
				e.clientY,
				ent.visualOffsetY ?? 0,
				ent.geometryVisualOffsetY ?? 0,
				rows,
				cols,
			);
		}
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

	/** Keep shared-grid pointer mapping aligned with a CSS-only stage transform. */
	setVisualOffsetY(paneId: string, offsetY: number): void {
		const ent = this.panes.get(paneId);
		if (ent) ent.visualOffsetY = Number.isFinite(offsetY) ? offsetY : 0;
	}

	/** Write raw bytes to the pane's PTY via dataHandler. */
	sendData(paneId: string, data: Uint8Array): void {
		const ent = this.panes.get(paneId);
		if (!ent?.dataHandler) return;
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
        if (!ent?.selectionStartAbs) return;
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
		const entry = this.panes.get(paneId);
		if (!entry) return;
		// User clear is a stream cut: bytes already queued before the click
		// must not reappear after the visible screen/history is wiped.
		this._releaseScrollback(entry, true);
		this.wake();
	}

	/** Explicit UI clear: remove old rows and move shell prompt to row zero. */
	clearTerminalPreservingPrompt(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		this._releaseScrollback(entry, true, !entry.kernel.isAltScreen());
		this.wake();
	}

	private _releaseScrollback(
		entry: PaneEntry,
		dropPendingFeed = false,
		preservePrompt = false,
	): void {
		if (dropPendingFeed) {
			dropPendingFeedBuffers(entry);
			this._clearQueuedDeltaFrames(entry);
			this._syncPendingFrameWork(entry);
			this._releaseTuiCursorSuppression(entry);
		} else {
			// Automatic memory reclaim must not turn a scrollback sweep into a
			// synchronous catch-up. Flush the short coalescer, leave the bounded
			// deferred FIFO for the next frame, and clear only retained history.
			this._flushFeedBuffer(entry);
			this.wake();
		}
		if (preservePrompt) entry.kernel.clearTerminalPreservingPrompt();
		else entry.kernel.clearScrollback();
		this._clearLinkUnderline(entry);
		// Clear the JS-side hyperlink index too. It owns copied visible strings
		// and otherwise retains the pre-clear output until a later Ctrl/hover
		// hit-test happens to rebuild it.
		entry.linkSpans.clear();
		// Worker mode owns a second semantic kernel. Mirror the same explicit
		// clear there; ordinary memory reclaim only needs ED 3.
		entry.lastScrollOffset = -1;
		entry.lastScrollTotal = -1;
	}

	/** Tell the wasm renderer whether this pane is the focused one. Only one
	 *  pane per workspace may own a cursor; claiming focus clears the prior
	 *  owner's renderer before the new pane is allowed to blink. */
	setFocused(paneId: string, focused: boolean): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		const workspaceId = entry.workspaceId;
		if (focused) {
			const previousId = this._focusedPaneByWorkspace.get(workspaceId);
			if (previousId && previousId !== paneId) {
				this.panes.get(previousId)?.handle?.setFocused(false);
			}
			this._focusedPaneByWorkspace.set(workspaceId, paneId);
			entry.handle?.setFocused(true);
			entry.lastForegroundAt = Date.now();
		} else {
			if (this._focusedPaneByWorkspace.get(workspaceId) === paneId) {
				this._focusedPaneByWorkspace.delete(workspaceId);
			}
			entry.handle?.setFocused(false);
		}
		// Cursor visibility changed → cursor row dirties → wake.
		this.wake();
	}

	/** P2.2 (2026-05-20): build the per-frame pane visit order for the
	 *  RAF tick — focused pane first, then non-focused entries rotated
	 *  by `_rafRotationIndex` so over many frames every non-focused
	 *  pane gets first-of-the-rest treatment in turn. Parked entries and
	 *  inactive-workspace entries are excluded from this paint order; the
	 *  separate `_feedOrder` keeps their kernels current without paying
	 *  renderer/dirty-probe work. */
	private _renderOrder(): PaneEntry[] {
		const live: PaneEntry[] = [];
		if (this._activeWorkspaceId !== null) {
			const paneIds = this.paneIdsByWorkspace.get(this._activeWorkspaceId);
			if (paneIds) {
				for (const paneId of paneIds) {
					const entry = this.panes.get(paneId);
					if (entry && !entry.parked) live.push(entry);
				}
				return this._orderPanes(live);
			}
			// Keep the bootstrap/test path compatible with entries injected before
			// the workspace index was introduced.
			for (const entry of this.panes.values()) {
				if (!entry.parked && entry.workspaceId === this._activeWorkspaceId) live.push(entry);
			}
			return this._orderPanes(live);
		}
		for (const entry of this.panes.values()) {
			if (!entry.parked) live.push(entry);
		}
		return this._orderPanes(live);
	}

	/** Mark whether a pane currently needs parser work on a compositor turn. */
	private _syncPendingFrameWork(entry: PaneEntry): void {
		if (entry.deltaQueueHead < entry.deltaQueue.length || hasDeferredFeed(entry)) {
			this.pendingFrameWorkPanes.add(entry.paneId);
		} else {
			this.pendingFrameWorkPanes.delete(entry.paneId);
		}
	}

	/**
	 * Background parser work must continue for hidden panes so their kernels
	 * remain current when a workspace is shown. Only panes with queued parser
	 * work enter this order; parked panes stay indexed but wait until unparked.
	 */
	private _feedOrder(): PaneEntry[] {
		const live: PaneEntry[] = [];
		for (const paneId of this.pendingFrameWorkPanes) {
			const entry = this.panes.get(paneId);
			if (!entry) {
				this.pendingFrameWorkPanes.delete(paneId);
				continue;
			}
			if (entry.deltaQueueHead >= entry.deltaQueue.length && !hasDeferredFeed(entry)) {
				this.pendingFrameWorkPanes.delete(paneId);
				continue;
			}
			if (!entry.parked) live.push(entry);
		}
		return this._orderPanes(live);
	}

	private _orderPanes(live: PaneEntry[]): PaneEntry[] {
		if (live.length <= 1) return live;
		const focusedId = this._activeWorkspaceId === null
			? live.find((entry) => this._focusedPaneByWorkspace.get(entry.workspaceId) === entry.paneId)?.paneId ?? null
			: this._focusedPaneByWorkspace.get(this._activeWorkspaceId) ?? null;
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
		this.wake();
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
			if (cur?.scrollStateHandler === handler) cur.scrollStateHandler = null;
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

	/** RIDGE_DIAG-only geometry snapshot for real-browser remote E2E. */
	debugGeometry(): unknown[] {
		return Array.from(this.panes.values()).map((entry) => {
			const container = entry.container.getBoundingClientRect();
			const canvas = entry.canvas.getBoundingClientRect();
			const clientX = canvas.right - entry.cellW / 2;
			const clientY = canvas.bottom - entry.cellH / 2;
			return {
				paneId: entry.paneId,
				workspaceId: entry.workspaceId,
				container: { x: container.x, y: container.y, width: container.width, height: container.height },
				canvas: { x: canvas.x, y: canvas.y, width: canvas.width, height: canvas.height },
				backing: { width: entry.canvas.width, height: entry.canvas.height },
				cell: { width: entry.cellW, height: entry.cellH },
				kernel: { rows: entry.kernel.rows(), cols: entry.kernel.cols() },
				scrollback: { offset: entry.kernel.scrollOffset(), total: entry.kernel.scrollbackLen() },
				cursor: { row: entry.kernel.cursorRow(), col: entry.kernel.cursorCol() },
				visibleText: ((entry.kernel.dumpVisibleText?.() as string[] | undefined) ?? []).map(String),
				inputAnchor: this.inputAnchorResolved(entry.paneId),
				reported: { rows: entry.lastReportedRows, cols: entry.lastReportedCols },
				bottomRightHit: this.cellFromEvent(entry.paneId, { clientX, clientY }),
			};
		});
	}

	/** iter-60 G3：标记 raw 字节模式 pane；外部 `pty-resized` 事件负责回灌
	 *  canonical grid，仅 `localGridAuthority` pane 在 fit 时主动 claim。
	 *  幂等；park/unpark 间存续。 */
	setLocalGridAuthority(paneId: string, on: boolean): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.localGridAuthority = on;
		if (on) this._scheduleInitialFit(entry);
	}

	/** iter-60 G4: actual render backend of this pane's handle (WebGPU),
	 *  or null when the pane isn't attached. The mobile footer
	 *  binds this —前 P4 重构后曾恒显默认值。 */
	backendName(paneId: string): string | null {
		const h = this.panes.get(paneId)?.handle as unknown as
			| { backendName?: () => string }
			| undefined;
		try {
			return h && typeof h.backendName === 'function' ? h.backendName() : null;
		} catch {
			return null;
		}
	}

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

	/** E2E probe — last `setPreedit` call for the given pane, or `null`
	 *  if `clearPreedit` was the most recent call (or no preedit yet).
	 *  Specs use this to assert overlay-cell == textarea-cell == anchor. */
	lastPreeditCall(paneId: string): { row: number; col: number; text: string } | null {
		return this._lastPreeditCall.get(paneId) ?? null;
	}

	/** §1.34 — shell-history overlay, rendered directly on the wasm
	 *  canvas (sibling of `setPreedit`). RidgePane computes the filtered
	 *  history items + the cursor anchor, then pushes them here; the
	 *  renderer paints the popup so it inherits pane focus, theme, cell
	 *  metrics and DPR for free. Like `setPreedit`, this targets the
	 *  main-thread `entry.handle` only — the worker renderer mirror does
	 *  not carry overlays, matching the preedit path. See
	 *  `packages/ridge-term/src/render/renderer.rs::HistoryOverlay`. */
	setHistoryOverlay(paneId: string, overlay: {
		items: readonly string[];
		selectedIndex: number;
		anchorRow: number;
		anchorCol: number;
		placeAbove: boolean;
		totalItems: number;
		firstVisible: number;
	}): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		type HistoryOverlayArgs = [
			string[], number, number, number, boolean, number, number, number, number,
		];
		const h = entry.handle as unknown as {
			setHistoryOverlay?: (...args: HistoryOverlayArgs) => void;
		};
		// `items` is the JS-windowed VISIBLE slice; `selectedIndex` is
		// slice-relative; `totalItems`/`firstVisible` drive the scrollbar.
		h.setHistoryOverlay?.(
			[...overlay.items],
			overlay.selectedIndex,
			overlay.anchorRow,
			overlay.anchorCol,
			overlay.placeAbove,
			overlay.totalItems,
			overlay.firstVisible,
			entry.kernel.cols(),
			entry.kernel.rows(),
		);
		this.wake();
	}

	/** Remove the shell-history overlay. Called on commit / close / focus
	 *  loss. Mirrors `clearPreedit`. */
	clearHistoryOverlay(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		const h = entry.handle as unknown as { clearHistoryOverlay?: () => void };
		h.clearHistoryOverlay?.();
		this.wake();
	}

	/** §1.34 — gate the shell-history popup on the wasm kernel's TUI
	 *  heuristic. Any live TUI signal — DECCKM / alt screen / mouse
	 *  reporting / inline-TUI heuristic / hidden cursor / 2 s sticky
	 *  after any of those — returns false, so an ArrowUp inside claude
	 *  code / vim / less falls through to the kernel encoder instead of
	 *  popping the overlay. Returns false when the pane is unknown. */
	shouldAllowShellHistory(paneId: string): boolean {
		const entry = this.panes.get(paneId);
		if (!entry) return false;
		const k = entry.kernel as unknown as { shouldAllowShellHistory?: () => boolean };
		return k.shouldAllowShellHistory?.() ?? false;
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

	/**
	 * Open the validated link under a viewport cell without touching focus or
	 * keyboard state. Remote's capture-phase pointer guard calls this directly;
	 * desktop's shared pointer listener keeps the same plan/arbitration path.
	 */
	openLinkAt(paneId: string, row: number, col: number): boolean {
		const entry = this.panes.get(paneId);
		if (!entry) return false;
		const oscLink = entry.kernel.hyperlinkAt(row, col) as { uri?: string } | null;
		const textSpan = oscLink
			? null
			: entry.linkSpans.hitTest(entry.kernel, row, col);
		if (!oscLink?.uri && !textSpan) return false;
		const cwd = TerminalManager._currentPaneCwd(entry);
		const workspaceRoot = TerminalManager._workspaceRoot(entry) ?? null;
		const plan = oscLink?.uri
			? planHostOpen(oscLink.uri, 'osc8', {
				paneCwd: cwd,
				workspaceRoot,
				preferEditor: true,
			})
			: buildOpenPlanFromHit({
				text: textSpan!.text,
				kind: textSpan!.kind,
				paneCwd: cwd,
				workspaceRoot,
			});
		return TerminalManager._executeOpenPlan(plan, entry, oscLink?.uri ?? textSpan!.text);
	}

	/** Whether a cell contains a validated OSC-8 or plain-text link.  This is
	 * deliberately side-effect free so touch handling can reserve link taps
	 * without suppressing press-drag-release delivery to mouse-reporting TUIs. */
	hasLinkAt(paneId: string, row: number, col: number): boolean {
		const entry = this.panes.get(paneId);
		if (!entry) return false;
		const oscLink = entry.kernel.hyperlinkAt(row, col) as { uri?: string } | null;
		return Boolean(oscLink?.uri || entry.linkSpans.hitTest(entry.kernel, row, col));
	}

	/** Forward a pointerdown event to the TUI application when DEC mouse
	 *  reporting is active. Encodes the click as an SGR mouse sequence
	 *  and sends it through the data handler. This is the public entry
	 *  point called from RidgePane's onContainerPointerDown, separate
	 *  from the internal pointerDownListener closure which handles the
	 *  container's addEventListener path. */
	handlePointerDown(paneId: string, e: PointerEvent): boolean {
		const ent = this.panes.get(paneId);
		if (!ent?.dataHandler) return false;
		const modes = ent.kernel.mouseReportingModes();
		if (modes === 0) return false;
		if (ent.cellW <= 0 || ent.cellH <= 0) return false;
		// Check scrollbar target — same as internal isInScrollbar.
		const tgt = e.target as Element | null;
		if (tgt?.closest?.('.rg-scrollbar-track, .rg-scrollbar-thumb')) return false;
		const cell = this.cellFromEvent(paneId, e);
		if (!cell) return false;
		const { row: cellRow, col } = cell;
		// Cancel any queued mouse move to prevent stale motion events.
		if (ent.mouseMoveRaf !== null) {
			cancelAnimationFrame(ent.mouseMoveRaf);
			ent.mouseMoveRaf = null;
		}
		ent.pendingMouseMove = null;
		const isMac = isMacPlatform();
		const mod = e.ctrlKey || (isMac && e.metaKey);
		const btn = e.button;
		const bytes = ent.kernel.encodeMouse(cellRow, col, btn, 0, e.shiftKey, mod, e.altKey);
		if (bytes.length > 0) {
			ent.dataHandler(bytes);
			ent.lastMouseSent = { row: cellRow, col, buttons: e.buttons, action: 0 };
			try { (e.target as Element | null)?.setPointerCapture?.(e.pointerId); } catch {}
			return true;
		}
		return false;
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
	 *  at mount; this path is disabled until a worker can own a WebGPU host.
	 *  drive) a main-thread `RenderHandle`. The decision is process-wide
	 *  for now — the flag and the singleton are both global — so callers
	 *  can query without a `paneId`. Mid-session flag toggles take effect
	 *  on the next pane attach; already-attached panes keep their initial
	 *  decision until detach. */

	/** §1.33 (2026-05-22): kernel-side gate for the shell-history popup.
	 *  Returns true ONLY when the wasm kernel is confident a normal shell
	 *  prompt owns the input line on this pane — every known TUI signal
	 *  short-circuits to false, AND a 2-second sticky window holds the
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
	 *  whenever the pane state is reset. The pre-submit IME anchor is
	 *  invalid at the same boundary: keeping it would pin the next
	 *  mobile keyboard placement to the old prompt while the shell is
	 *  printing output. TUI panes still resolve through their recent
	 *  absolute-positioning CSI before the live cursor. */
	clearInputStart(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry) return;
		entry.inputStartRow = null;
		entry.inputStartCol = null;
		entry.imeAnchor = null;
		entry.imeCompositionActive = false;
		this._emitImeAnchor(entry);
	}

	/** Capture the live cursor as the user's IME anchor. A TUI may redraw old
	 * rows after input, but that must not move the composition window. */
	captureImeAnchor(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		if (entry.imeAnchorRaf !== null) {
			cancelAnimationFrame(entry.imeAnchorRaf);
			entry.imeAnchorRaf = null;
		}
		entry.imeAnchor = {
			row: entry.kernel.cursorRow(),
			col: entry.kernel.cursorCol(),
		};
		this._emitImeAnchor(entry);
	}

	/** Start an IME composition at the cursor visible at compositionstart. */
	beginImeComposition(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		entry.imeCompositionActive = true;
		this.captureImeAnchor(paneId);
	}

	/** End an IME composition. Keep the captured post-composition cell until
	 * the committed PTY echo schedules the next anchor; cancellation therefore
	 * cannot fall back to a TUI spinner row. */
	endImeComposition(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		this.captureImeAnchor(paneId);
		entry.imeCompositionActive = false;
	}

	/** Notify the manager about input sent outside its key/write helpers. The
	 * delayed capture waits for the PTY echo before moving the anchor. */
	noteUserInput(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (entry) this.scheduleImeAnchorCapture(entry);
	}

	/** Subscribe to the same anchor used by `inputAnchorResolved`. The
	 * callback bridges delayed PTY echo back to the DOM IME sink, so its
	 * browser caret cannot remain at the previous terminal cell. */
	onImeAnchor(
		paneId: string,
		handler: (anchor: { row: number; col: number } | null) => void,
	): () => void {
		const entry = this.panes.get(paneId);
		if (!entry) return () => {};
		entry.imeAnchorHandler = handler;
		try { handler(entry.imeAnchor); } catch { /* component may be tearing down */ }
		return () => {
			const current = this.panes.get(paneId);
			if (current?.imeAnchorHandler === handler) current.imeAnchorHandler = null;
		};
	}

	/** §1.32 Wave F — reconstruct the real shell-input line by READING
	 *  the kernel grid cells between the `markInputStart` anchor and the
	 *  live cursor, instead of trusting the keystroke mirror. Robust to
	 *  Tab-completion echoes, alias/`$VAR` expansion and vi-mode moves
	 *  the mirror can't see. Returns `null` (caller falls back to the
	 *  keystroke mirror) when:
	 *    - the pane is unknown / has no input-start marker,
	 *    - the input wrapped to another row (cursorRow ≠ startRow),
	 *    - the prompt was redrawn under us (cursorCol < startCol).
	 *  Shape is `InputBufferState` so it drops straight into
	 *  `computeReplaySequence`. */
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
		// Multi-row input (long command that wrapped, or cursor moved to
		// another row via PageUp etc.) — not handled. Fall back to mirror.
		if (cursorRow !== startRow) return null;
		// Cursor jumped behind input start — prompt was redrawn under us
		// (Ctrl+L, screen clear). Snapshot is invalid.
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
			this._emitImeAnchor(entry);
		});
	}

	private _emitImeAnchor(entry: PaneEntry): void {
		try { entry.imeAnchorHandler?.(entry.imeAnchor); }
		catch (error) { console.error('[ridge-term] imeAnchorHandler error', entry.paneId, error); }
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
		const gridRow = e.kernel.cursorRow();
		const col = e.kernel.cursorCol();
		// The kernel cursor row is a grid position. When the user has scrolled
		// into history (scrollOffset > 0), the grid is pushed down in the
		// viewport; add scrollOffset to get the viewport row for pixel math.
		const scrollOff = e.kernel.scrollOffset();
		const vpRow = gridRow + scrollOff;
		// Container has CSS `padding: Npx` (set by setPadding); absolute-
		// positioned IME helper measures `left/top` from the padding-box
		// while the canvas lays out inside the content-box. Add `pad` so
		// (col=0, row=0) returns the canvas top-left, not N px above-left.
		const pad = e.lastFitPaddingPx ?? e.lastAppliedPaddingPx ?? 0;
		return {
			x: Math.round(col * e.cellW) + pad,
			y: Math.round(vpRow * e.cellH) + pad,
			cellW: e.cellW,
			cellH: e.cellH,
			fontSizePx: this.opts.fontSizePx,
		};
	}

	/** §IME-scissor (2026-06-18, 缺陷 B): scissor-同源的 IME textarea 像素
	 *  换算。桌面共享 host canvas 模式下，分区内容画在 host canvas 的 scissor
	 *  偏移处（`_recomputeViewport` 的 `xDev/yDev`），而 textarea 是分区容器
	 *  内的 absolute 元素。旧公式 `round(col*cellW)+pad` 与 scissor 的
	 *  `floor(cssX*dpr)` 取整基准/原点都不同，非整数缩放（125%/150%）下偏。
	 *  这里把 textarea 的 CSS-px left/top 从 scissor 原点 + 渲染器逐格设备
	 *  取整 `round(col*cellW*dpr)` 反推回容器坐标系（见 `imeAnchor.ts`），
	 *  使 textarea 精确压在渲染器绘制该格的设备像素上。
	 *
	 *  仅在 host 模式且本分区已算出 viewport（scissor）时返回非 null；
	 *  尚未有 viewport 时返回 null，让调用方走通用锚点路径。
	 *
	 *  `vpRow`/`col` 为已 clamp 过的视口内行列（含 scrollOffset）。 */
	private _imeScissorCssPosition(
		entry: PaneEntry,
		vpRow: number,
		col: number,
	): { x: number; y: number } | null {
		const gh = this.globalHost;
		if (!gh || !this._isHostMode(entry)) return null;
		const vp = entry.viewport;
		if (!vp) return null;
		if (entry.cellW <= 0 || entry.cellH <= 0) return null;
		const cr = entry.container.getBoundingClientRect();
		const hr = gh.canvas.getBoundingClientRect();
		if (cr.width <= 0 || cr.height <= 0) return null;
		const cs = window.getComputedStyle(entry.container);
		const padL = Number.parseFloat(cs.paddingLeft) || 0;
		const padT = Number.parseFloat(cs.paddingTop) || 0;
		const dpr = window.devicePixelRatio || 1;
		// `padL/padT` 与 scissor 同源：`_recomputeViewport` 用同样的
		// `cssX = cr.left - hr.left + padL` → `floor(cssX*dpr)`，因此
		// `imeHelperCssPosition` 内部重算的 scissor 原点会精确等于
		// `entry.viewport.x/y`。这里复用同一份输入即保证两坐标系同源。
		const input: ImeAnchorInput = {
			containerLeft: cr.left,
			containerTop: cr.top,
			hostLeft: hr.left,
			hostTop: hr.top,
			padL,
			padT,
			cellW: entry.cellW,
			cellH: entry.cellH,
			col,
			row: vpRow,
			dpr,
		};
		const pos = imeHelperCssPosition(input);
		return { x: pos.x, y: pos.y };
	}

	/** Pixel position of the IME helper anchor (§1.27 fix) — uses the
	 *  stable user-input snapshot (`PaneEntry.imeAnchor`) for TUI composition
	 *  instead of the live kernel cursor, so background PTY redraws
	 *  (Ink/log-update spinner walks) don't drag the helper. Plain Shell
	 *  composition deliberately follows the live cursor.
	 *
	 *  §1.27-tail fallback chain when `imeAnchor` is null (for example before
	 *  the first user input): recent TUI CSI, then the live cursor. Outside a
	 *  live Shell composition, a user-input anchor wins over stale CSI. */
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
			const scrollOff = e.kernel.scrollOffset();
			const vpRow = row + scrollOff;
			const r = Math.min(vpRow, Math.max(0, rows - 1));
			const c = Math.min(col, Math.max(0, cols - 1));
			// §IME-scissor: host 模式下优先用 scissor-同源换算，消除非整数
			// 缩放/多分屏的偏移；非 host 模式 (null) 退回旧的 round+pad 公式。
			const scissor = this._imeScissorCssPosition(e, r, c);
			if (scissor) {
				return {
					x: scissor.x,
					y: scissor.y,
					cellW: e.cellW,
					cellH: e.cellH,
					fontSizePx: this.opts.fontSizePx,
				};
			}
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

		// Shell composition follows the live cursor so a real prompt move is
		// reflected in the same compositionupdate. TUI composition stays on the
		// start snapshot because its live cursor walks through redraw rows.
		if (e.imeCompositionActive && !isAlt && !isInlineTui) {
			return pickAt(e.kernel.cursorRow(), e.kernel.cursorCol());
		}
		// Outside shell composition, or inside a TUI, a user-input snapshot is
		// authoritative. CSI is only a pre-input fallback.
		const anchor = e.imeAnchor;
		if (anchor) return pickAt(anchor.row, anchor.col);
		if ((isAlt || isInlineTui) && typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi && Date.now() - csi.atMs < ABS_CSI_DECAY_MS) {
				return pickAt(csi.row, csi.col);
			}
		}

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

	/** Row/col version of `inputAnchorPixelPosition`. Shell composition follows
	 *  the live cursor; TUI composition uses the same locked anchor as the
	 *  pixel path, so the preedit overlay and browser sink never diverge. */
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
		if (e.imeCompositionActive && !isAlt && !isInlineTui) {
			return { row: k.cursorRow(), col: k.cursorCol() };
		}
		// A user-input snapshot is authoritative outside live shell
		// composition. Before one exists, an absolute CSI is best effort.
		if (e.imeAnchor) return { row: e.imeAnchor.row, col: e.imeAnchor.col };
		if ((isAlt || isInlineTui) && typeof k.lastAbsCsiPosition === 'function') {
			const csi = k.lastAbsCsiPosition();
			if (csi) return { row: csi.row, col: csi.col };
		}
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
		const scrollOff = e.kernel.scrollOffset();
		const vpRow = r + scrollOff;
		const vpR = Math.min(vpRow, Math.max(0, rows - 1));
		// §IME-scissor: host 模式走 scissor-同源换算，非 host 退回旧公式。
		const scissor = this._imeScissorCssPosition(e, vpR, c);
		const px = scissor ?? { x: Math.round(c * e.cellW) + pad, y: Math.round(vpR * e.cellH) + pad };
		return {
			row: r,
			col: c,
			x: px.x,
			y: px.y,
			cellW: e.cellW,
			cellH: e.cellH,
			fontSizePx: this.opts.fontSizePx,
		};
	}

	/** Convert a grid (row, col) to pixel position relative to the pane
	 *  container's padding-box, accounting for scroll offset. Used by
	 *  RidgePane during active IME composition to recompute the textarea
	 *  position from the locked composingAnchor without moving the anchor
	 *  itself (which would break the wasm preedit overlay). */
	pixelPositionFromCell(
		paneId: string,
		row: number,
		col: number,
	): { x: number; y: number; cellW: number; cellH: number } | null {
		const e = this.panes.get(paneId);
		if (!e || e.cellW <= 0 || e.cellH <= 0) return null;
		const rows = e.kernel.rows();
		const cols = e.kernel.cols();
		const scrollOff = e.kernel.scrollOffset();
		const vpRow = row + scrollOff;
		const r = Math.min(vpRow, Math.max(0, rows - 1));
		const c = Math.min(col, Math.max(0, cols - 1));
		const pad = e.lastFitPaddingPx ?? e.lastAppliedPaddingPx ?? 0;
		// §IME-scissor: host 模式走 scissor-同源换算，非 host 退回旧公式。
		const scissor = this._imeScissorCssPosition(e, r, c);
		if (scissor) {
			return { x: scissor.x, y: scissor.y, cellW: e.cellW, cellH: e.cellH };
		}
		return {
			x: Math.round(c * e.cellW) + pad,
			y: Math.round(r * e.cellH) + pad,
			cellW: e.cellW,
			cellH: e.cellH,
		};
	}

	/** Force a full-frame redraw on the next rAF tick (§1.27 fix). Used
	 *  by `RidgePane::onCompositionEnd` to repaint cells underneath the
	 *  IME helper textarea after the opaque `.is-composing` overlay.
	 *  WebGPU redraws every visible row per tick, so this is a no-op there
	 *  beyond a single extra wake. */
	/** Refresh specific panes by id — invalidates render cache and wakes
	 *  the rAF loop. Used after split resize to redraw affected panes. */
	forceFullRedrawFor(ids: string[]): void {
		for (const id of ids) {
			const entry = this.panes.get(id);
			if (!entry || entry.parked) continue;
			this._invalidateEntry(entry);
		}
		if (ids.length) this.wake();
	}

	forceFullRedraw(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		this._invalidateEntry(entry);
		this.wake();
	}

	/** Same as `forceFullRedraw` but applied across every attached pane.
	 *  Used when a global font event lands — e.g. Twemoji finishes loading
	 *  AFTER panes have already been streaming output. Each pane's
	 *  `invalidateAll` clears the WebGPU `GlyphAtlas` LRU so the next frame
	 *  re-rasterizes against the new font stack. Parked panes are skipped (their
	 *  handles have been freed); they pick up the new font on unpark. */
	/** Invalidate all panes in a specific workspace. Called after split
	 *  resize drag completes to refresh all affected panes. */
	invalidateWorkspace(workspaceId: string): void {
		for (const entry of this.panes.values()) {
			if (entry.parked) continue;
			if (entry.workspaceId !== workspaceId) continue;
			this._invalidateEntry(entry);
		}
		this.wake();
	}

	invalidateAllPanes(): void {
		for (const entry of this.panes.values()) {
			if (entry.parked) continue;
			this._invalidateEntry(entry);
		}
		this.wake();
	}

	/**
	 * Apply font family/size globally. Re-measures cells and triggers fit
	 * for every attached pane. (round 2.5 will store this once per surface
	 * rather than per-pane.)
	 */
	setFont(family: string, sizePx: number): Promise<void> {
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
		if (!this.wasmReady) return Promise.resolve();
		return this._ensureFontStack(family).then(() => {
			if (this.opts.fontFamily !== family || this.opts.fontSizePx !== sizePx) return;
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
				if (!entry.handle) continue;
				const [w, h] = entry.handle.configure(family, sizePx, dpr) as
					| [number, number]
					| Float32Array;
				entry.cellW = Number(w);
				entry.cellH = Number(h);
				entry.lastConfiguredDpr = dpr;
				this._invalidateEntry(entry);
				void this.fitPane(entry, this._sharedRemoteMode);
			}
			this.wake();
		});
	}

	/** Apply theme overrides to all panes. */
	setTheme(theme: Record<string, string>): void {
		this.opts.theme = theme;
		const generation = ++this._themeGeneration;
		if (this._themeDeferredTimer !== null) {
			clearTimeout(this._themeDeferredTimer);
			this._themeDeferredTimer = null;
		}
		let applied = 0;
		let parked = 0;
		const deferred: PaneEntry[] = [];
		const applyEntry = (entry: PaneEntry): void => {
			if (entry.parked) return;
			entry.handle?.applyDefaultTheme();
			entry.handle?.applyTheme(theme);
			this._invalidateEntry(entry);
			applied++;
		};
		for (const entry of this.panes.values()) {
			// Parked panes pick up the theme on the next unpark via this.opts.
			if (entry.parked) { parked++; continue; }
			if (this._activeWorkspaceId !== null && entry.workspaceId !== this._activeWorkspaceId) {
				deferred.push(entry);
				continue;
			}
			entry.handle?.applyDefaultTheme();
			entry.handle?.applyTheme(theme);
			// Theme change doesn't bump kernel dirty. Force a full renderer
			// refresh so the next frame re-resolves every cell colour.
			this._invalidateEntry(entry);
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
		if (deferred.length > 0) {
			let index = 0;
			const flushDeferred = () => {
				this._themeDeferredTimer = null;
				if (generation !== this._themeGeneration) return;
				const end = Math.min(index + 4, deferred.length);
				for (; index < end; index++) applyEntry(deferred[index]!);
				if (index < deferred.length) {
					this._themeDeferredTimer = setTimeout(flushDeferred, 0);
				} else {
					this.wake();
				}
			};
			this._themeDeferredTimer = setTimeout(flushDeferred, 0);
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
	 * will fire its own initial fit. Returns the underlying async fit so
	 * attach can wait for kernel + PTY sync before activating a shell.
	 */
	fitPaneNow(paneId: string, force = false): Promise<void> {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return Promise.resolve();
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
			entry.pendingFitTimer = null;
		}
		this._cancelInitialFit(entry);
		return this.fitPane(entry, this._sharedRemoteMode, force).finally(() => {
			if (this._initialFitNeedsRetry(entry)) this._scheduleInitialFit(entry);
		});
	}

	/** Toggle shared-grid + centered-letterbox mode. Enabled on the browser
	 *  controller; live layout frames remain visual-only and the bounded
	 *  trailing fit claims the final grid.
	 *  Re-letterboxes every attached pane on the transition so the change is
	 *  visible without waiting for the next ResizeObserver fire. */
	setSharedRemoteMode(on: boolean): void {
		if (this._sharedRemoteMode === on) return;
		this._sharedRemoteMode = on;
		for (const entry of this.panes.values()) {
			if (entry.parked) continue;
			if (on) {
				// Re-clip to the kernel grid centered in the container.
				this._recomputeViewport(entry);
			} else {
				// Back to normal: re-fit (claim) so the pane fills its container.
				void this.fitPane(entry, true);
			}
		}
		this._invalidateHost();
		this.wake();
	}

	/** Explicitly lock the shared PTY to this viewer — the refresh button.
	 *  It resizes the real PTY (`resize_pane` over the tunnel) to the
	 *  container size; the broadcast Resize delta then grows every viewer's
	 *  kernel grid, and the centered-letterbox tracking re-clips to it. In
	 *  normal (non-shared) mode it is identical to `fitPaneNow`. */
	claimPaneSize(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
			entry.pendingFitTimer = null;
		}
		void this.fitPane(entry, true, true);
		// iter-60 G2 self-heal: a claim is only "done" when the broadcast
		// Resize delta round-trips into THIS kernel. If after 1s the kernel
		// grid still disagrees with the last claimed target (delta dropped /
		// resize_pane failed / another viewer re-claimed), retry ONCE and log
		// loudly — this was previously a silent no-op ("resize 按钮没反应").
		setTimeout(() => {
			const e = this.panes.get(paneId);
			if (!e || e.parked) return;
			const rowsOk = e.lastReportedRows < 0 || e.kernel.rows() === e.lastReportedRows;
			const colsOk = e.lastReportedCols < 0 || e.kernel.cols() === e.lastReportedCols;
			if (rowsOk && colsOk) return;
			console.warn(
				'[ridge-term] claimPaneSize verify failed — kernel',
				`${e.kernel.cols()}×${e.kernel.rows()}`,
				'≠ claimed',
				`${e.lastReportedCols}×${e.lastReportedRows}`,
				'; retrying once',
			);
			void this.fitPane(e, true, true);
		}, 1000);
	}

	/** Apply the canonical grid announced by another refresh owner. */
	applyPaneResize(paneId: string, rows: number, cols: number): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;
		const nextRows = Math.floor(rows);
		const nextCols = Math.floor(cols);
		if (!Number.isFinite(nextRows) || !Number.isFinite(nextCols) || nextRows <= 0 || nextCols <= 0) return;
		if (entry.pendingFitTimer !== null) {
			clearTimeout(entry.pendingFitTimer);
			entry.pendingFitTimer = null;
		}
		this._cancelInitialFit(entry);
		entry.lastReportedRows = nextRows;
		entry.lastReportedCols = nextCols;
		entry.kernel.resize(nextRows, nextCols);
		if (this._sharedRemoteMode) this._recomputeViewport(entry);
		this._invalidateEntry(entry);
		entry.linkSpans.markDirty();
		try {
			entry.handle?.render(entry.kernel);
			entry.renderPending = false;
		} catch (error) { console.error('[ridge-term] external resize render error', paneId, error); }
		this._scheduleFitRedraw(paneId);
		this.wake();
	}

	/**
	 * Container-size changed.
	 *
	 * **Scissor (visual clip region) — immediate.**
	 * The GPU scissor rectangle is a trivial arithmetic update (DOM
	 * rect → device-pixel clamp → `setViewportOffset` + `resize_surface`,
	 * the latter short-circuits when dims haven't changed). Deferring it
	 * creates visible right/bottom clipping during drag that only snaps
	 * back on release — the symptom first reported as "随 pane resize
	 * 下/右遮挡、松手恢复".
	 *
	 * **Kernel grid resize + PTY SIGWINCH — debounced.**
	 * Resizing the kernel grid mid-drag is actively dangerous: in-flight
	 * PTY bytes carry absolute cursor positions valid only under one
	 * given grid. Collapsing the whole drag into a single trailing-edge
	 * `fitPane` at settle is the correct strategy — it eliminates the
	 * "TUI drawing 错位 / 不完整" symptom that the prior 120 ms debounce
	 * produced when a partial re-fit landed during continuous motion and
	 * drift accumulated as the user kept dragging.
	 *
	 * Settle triggers (either):
	 *   a. `RESIZE_SETTLE_MS` (500 ms) elapses with no further
	 *      `viewportChanged` events — user paused mid-drag.
	 *   b. A global `pointerup` lands — user released the splitter /
	 *      window-edge handle (see `_ensureResizeReleaseListener`).
	 *
	 * Initial fit at `attach()` bypasses the debounce — synchronous
	 * resize, no concurrent in-flight bytes.
	 */
	viewportChanged(paneId: string): void {
		const entry = this.panes.get(paneId);
		if (!entry || entry.parked) return;

		// Immediate: recompute GPU scissor + viewport offset so the
		// terminal visual region tracks the DOM container in real time
		// during splitter-sidebar drag. Expensive kernel resize is
		// deferred to the debounced `fitPane` below.
		this._recomputeViewport(entry);
		this._invalidateHost();
		this.wake();

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
			void this.fitPane(e, this._sharedRemoteMode);
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
			if (tgt?.closest?.('[data-rg-pane-id]')) {
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
			void this.fitPane(entry, this._sharedRemoteMode);
		}
	}

	private _cancelInitialFit(entry: PaneEntry): void {
		if (entry.initialFitTimer !== null) {
			clearTimeout(entry.initialFitTimer);
			entry.initialFitTimer = null;
		}
		entry.initialFitAttempt = 0;
	}

	/** Return true while a cold pane still needs a real layout fit. The shared
	 * viewer intentionally follows the host grid, so only local-grid panes and
	 * normal desktop panes use the container-vs-kernel comparison. */
	private _initialFitNeedsRetry(entry: PaneEntry): boolean {
		if (entry.parked) return false;
		try {
			const rect = entry.container.getBoundingClientRect();
			const cs = window.getComputedStyle(entry.container);
			const measurement: InitialFitMeasurement = {
				containerWidth: rect.width,
				containerHeight: rect.height,
				paddingLeft: Number.parseFloat(cs.paddingLeft) || 0,
				paddingRight: Number.parseFloat(cs.paddingRight) || 0,
				paddingTop: Number.parseFloat(cs.paddingTop) || 0,
				paddingBottom: Number.parseFloat(cs.paddingBottom) || 0,
				cellWidth: entry.cellW,
				cellHeight: entry.cellH,
				kernelRows: entry.kernel.rows(),
				kernelCols: entry.kernel.cols(),
				sharedRemoteMode: this._sharedRemoteMode,
				localGridAuthority: entry.localGridAuthority === true,
			};
			return needsInitialPaneFit(measurement);
		} catch {
			return true;
		}
	}

	/** Fit after cold mount/visibility transitions with a small, bounded retry
	 * window. ResizeObserver is not guaranteed to fire for display:none→flex or
	 * late font metrics, so relying on it alone leaves a pane at 80×24 forever. */
	private _scheduleInitialFit(entry: PaneEntry): void {
		if (entry.parked || entry.initialFitTimer !== null) return;
		const attempt = entry.initialFitAttempt;
		const delay = INITIAL_FIT_RETRY_DELAYS_MS[Math.min(attempt, INITIAL_FIT_RETRY_DELAYS_MS.length - 1)]!;
		const run = () => {
			entry.initialFitTimer = null;
			const current = this.panes.get(entry.paneId);
			if (!current || current !== entry || current.parked) return;
			void this.fitPane(current, this._sharedRemoteMode).finally(() => {
				const live = this.panes.get(entry.paneId);
				if (!live || live !== entry || live.parked) return;
				if (!this._initialFitNeedsRetry(live) || attempt >= INITIAL_FIT_RETRY_DELAYS_MS.length - 1) {
					live.initialFitAttempt = 0;
					return;
				}
				live.initialFitAttempt = attempt + 1;
				this._scheduleInitialFit(live);
			});
		};
		// A zero-delay task runs after the current Svelte/flex commit and, unlike
		// a bare rAF callback, can be cancelled during park/detach.
		entry.initialFitTimer = setTimeout(run, delay);
	}

	private _measureFit(entry: PaneEntry): { wCss: number; hCss: number } | null {
		if (this._isHostMode(entry)) {
			const rect = entry.container.getBoundingClientRect();
			const styles = window.getComputedStyle(entry.container);
			const horizontal = (Number.parseFloat(styles.paddingLeft) || 0) + (Number.parseFloat(styles.paddingRight) || 0);
			const vertical = (Number.parseFloat(styles.paddingTop) || 0) + (Number.parseFloat(styles.paddingBottom) || 0);
			return { wCss: Math.max(0, rect.width - horizontal), hCss: Math.max(0, rect.height - vertical) };
		}
		const rect = entry.canvas.getBoundingClientRect();
		return { wCss: Math.floor(rect.width), hCss: Math.floor(rect.height) };
	}

	private _syncFitDpr(entry: PaneEntry, dpr: number): void {
		if (entry.lastConfiguredDpr === dpr || !entry.handle) return;
		const [width, height] = entry.handle.configure(this.opts.fontFamily, this.opts.fontSizePx, dpr) as [number, number] | Float32Array;
		entry.cellW = Number(width);
		entry.cellH = Number(height);
		entry.lastConfiguredDpr = dpr;
	}

	private _computeFitGrid(entry: PaneEntry, claim: boolean, dpr: number, measured: { wCss: number; hCss: number }): FitGeometry | null {
		let { wCss, hCss } = measured;
		if (this._sharedRemoteMode && !claim && !entry.localGridAuthority) {
			this._recomputeViewport(entry);
			return null;
		}
		let cols = Math.max(1, Math.floor(wCss / entry.cellW));
		let rows = Math.max(1, Math.floor(hCss / entry.cellH));
		if (this._isHostMode(entry)) {
			rows = Math.max(1, Math.floor(hCss / entry.cellH));
			entry.lastFitPaddingPx = entry.lastAppliedPaddingPx ?? 0;
			this._recomputeViewport(entry);
		} else if (this._sharedRemoteMode) {
			const rect = entry.container.getBoundingClientRect();
			const styles = window.getComputedStyle(entry.container);
			wCss = Math.floor(rect.width - (Number.parseFloat(styles.paddingLeft) || 0) - (Number.parseFloat(styles.paddingRight) || 0));
			hCss = Math.floor(rect.height - (Number.parseFloat(styles.paddingTop) || 0) - (Number.parseFloat(styles.paddingBottom) || 0));
			cols = Math.max(1, Math.floor(wCss / entry.cellW));
			rows = Math.max(1, Math.floor(hCss / entry.cellH));
		} else {
			entry.canvas.style.position = 'relative';
			entry.canvas.style.left = '';
			entry.canvas.style.top = '';
			entry.canvas.style.width = '100%';
			entry.canvas.style.height = '100%';
			entry.handle?.resize(wCss, hCss, dpr);
		}
		if (this._sharedRemoteMode) this._recomputeViewport(entry);
		return { wCss, hCss, rows, cols };
	}

	private _fitGridChanged(entry: PaneEntry, grid: FitGeometry): boolean {
		const sizeChanged = grid.rows !== entry.lastReportedRows || grid.cols !== entry.lastReportedCols || grid.rows !== entry.kernel.rows() || grid.cols !== entry.kernel.cols();
		if (!sizeChanged) return false;
		if (import.meta.env?.DEV) console.debug('[ridge-term] fit', entry.paneId, `${grid.cols}x${grid.rows}`, `(was ${entry.lastReportedCols < 0 ? '?' : entry.lastReportedCols}x${entry.lastReportedRows < 0 ? '?' : entry.lastReportedRows})`);
		entry.lastReportedRows = grid.rows;
		entry.lastReportedCols = grid.cols;
		return true;
	}

	private _logResizeDecision(entry: PaneEntry, rows: number, cols: number, isAlt: boolean, isInlineTui: boolean): void {
		if (!import.meta.env?.DEV || typeof console.debug !== 'function') return;
		const lastAbsCsiPos = entry.kernel.lastAbsCsiPosition();
		console.debug('[ridge-term] resize decision', {
			paneId: entry.paneId,
			old: { rows: entry.lastReportedRows, cols: entry.lastReportedCols },
			new: { rows, cols },
			isAlt,
			isInlineTui,
			wipeBeforePty: isAlt || isInlineTui,
			cursorVisible: entry.kernel.isCursorVisible(),
			heuristic: lastAbsCsiPos ? { absCsiRow: lastAbsCsiPos.row, absCsiCol: lastAbsCsiPos.col, absCsiAt: lastAbsCsiPos.atMs } : null,
		});
	}

	private _scheduleFitRedraw(paneId: string): void {
		setTimeout(() => {
			const entry = this.panes.get(paneId);
			if (entry && !entry.parked) this.forceFullRedraw(paneId);
		}, 150);
	}

	private async _applyFitResize(entry: PaneEntry, grid: FitGeometry): Promise<void> {
		const isAlt = entry.kernel.isAltScreen();
		const isInlineTui = !isAlt && entry.kernel.isInlineTuiMode();
		this._logResizeDecision(entry, grid.rows, grid.cols, isAlt, isInlineTui);
		if (entry.localGridAuthority || this._sharedRemoteMode) entry.kernel.resize(grid.rows, grid.cols);
		await entry.resizeHandler?.(grid.rows, grid.cols, isAlt, isInlineTui);
		if (this._sharedRemoteMode && entry.localGridAuthority) this._recomputeViewport(entry);
		this._invalidateEntry(entry);
		entry.linkSpans.markDirty();
		try {
			entry.handle?.render(entry.kernel);
			entry.renderPending = false;
		} catch (error) { console.error('[ridge-term] post-resize render error', entry.paneId, error); }
		this._scheduleFitRedraw(entry.paneId);
		this.wake();
	}

	private async fitPane(entry: PaneEntry, claim = false, force = false): Promise<void> {
		const measured = this._measureFit(entry);
		if (!measured || measured.wCss <= 0 || measured.hCss <= 0 || entry.cellW <= 0 || entry.cellH <= 0) return;
		const dpr = window.devicePixelRatio || 1;
		this._syncFitDpr(entry, dpr);
		const grid = this._computeFitGrid(entry, claim, dpr, measured);
		if (!grid) return;
		if (!this._fitGridChanged(entry, grid) && !force) return;
		await this._applyFitResize(entry, grid);
	}

	// ---- frame loop -------------------------------------------------

	private _installVisibilityListener(): void {
		if (this.visibilityListener !== null || typeof document === 'undefined') return;
		this.visibilityListener = () => {
			if (document.hidden) {
				this.reclaimTerminalMemory({ documentHidden: true });
				return;
			}
			this._restoreMemoryParked(this._activeWorkspaceId);
			this._invalidateHost();
			this.wake();
		};
		document.addEventListener('visibilitychange', this.visibilityListener);
	}

	private _hostPaneDirty(entry: PaneEntry, dateNow: number): boolean {
		if (entry.renderPending) return true;
		if (entry.wasHiddenLastTick || entry.handle === null) return true;
		const handle = entry.handle as unknown as { isDirty?: (kernel: TerminalKernel, now: number) => boolean };
		if (typeof handle.isDirty !== 'function') return true;
		try { return handle.isDirty(entry.kernel, dateNow); }
		catch { return true; }
	}

	private _collectHostDirty(frameOrder: PaneEntry[], dateNow: number): {
		dirtyByPane: Map<string, boolean>;
		activeWsId: string | null;
	} {
		const dirtyByPane = new Map<string, boolean>();
		let activeWsId: string | null = null;
		for (const entry of frameOrder) {
			if (entry.parked || !this._isHostMode(entry) || this._isContainerHidden(entry)) continue;
			activeWsId ??= entry.workspaceId;
			if (entry.workspaceId !== activeWsId) continue;
			if (this._sharedRemoteMode) {
				const rows = entry.kernel.rows();
				const cols = entry.kernel.cols();
				if (rows !== entry.lastViewportKernelRows || cols !== entry.lastViewportKernelCols) this._recomputeViewport(entry);
			}
			const dirty = this._hostPaneDirty(entry, dateNow);
			dirtyByPane.set(entry.paneId, dirty);
		}
		return { dirtyByPane, activeWsId };
	}

	private _newRafFrame(perfNow: number, dateNow: number): RafFrameState {
		if (perfNow - this._lastMemorySweepAt >= TERMINAL_MEMORY_SWEEP_MS) {
			this._lastMemorySweepAt = perfNow;
			this.reclaimTerminalMemory();
		}
		const host = this._globalHostHandle() as unknown as { needsFullSeed?: () => boolean } | null;
		let hostNeedsSeed = false;
		try { hostNeedsSeed = host?.needsFullSeed?.() === true; } catch { /* old wasm bundle */ }
		const surfaceJustWiped = this._hostInvalidatePending || hostNeedsSeed;
		this._hostInvalidatePending = false;
		const frameOrder = this._renderOrder();
		const feedOrder = this._feedOrder();
		this._drainQueuedDeltaFrames(feedOrder);
		this._drainDeferredFeeds(feedOrder);
		// Delta draining has a strict CPU budget but can still consume a few
		// milliseconds. Anchor hold/sync deadlines after it, not at RAF entry.
		const framePerfNow = performance.now();
		const dirty = this._collectHostDirty(frameOrder, dateNow);
		return {
			frameOrder, feedOrder, dateNow, perfNow: framePerfNow, surfaceJustWiped,
			...dirty,
			activeHost: dirty.activeWsId === null ? null : this._globalHostHandle(),
			hostFrameOpen: false,
			frameFailed: false,
			anyRendered: false,
			renderDeferred: false,
			renderDeadlineMs: framePerfNow + RENDER_FRAME_BUDGET_MS,
			minDeadlineMs: Infinity,
		};
	}

	private _ensureHostFrame(state: RafFrameState): boolean {
		if (state.hostFrameOpen) return true;
		if (state.activeHost === null) return false;
		state.hostFrameOpen = state.activeHost.beginFrame(this._currentThemeBgRgba());
		if (!state.hostFrameOpen) {
			state.frameFailed = true;
			this.frameFailureCount = Math.min(this.frameFailureCount + 1, 6);
		}
		return state.hostFrameOpen;
	}

	private _renderEntryAfterSync(entry: PaneEntry, state: RafFrameState): boolean {
		const sync = entry.kernel.isSyncOutput();
		if (!sync) {
			if (entry.syncStart !== null) {
				entry.syncStart = null;
				entry.syncTimeoutRendered = false;
			}
			return true;
		}
		entry.syncStart ??= state.perfNow;
		const elapsed = state.perfNow - entry.syncStart;
		if (elapsed < SYNC_OUTPUT_TIMEOUT_MS) {
			state.minDeadlineMs = Math.min(state.minDeadlineMs, SYNC_OUTPUT_TIMEOUT_MS - elapsed);
			return false;
		}
		if (entry.syncTimeoutRendered) return false;
		entry.syncTimeoutRendered = true;
		return true;
	}

	private _markRenderPending(entry: PaneEntry): void {
		entry.renderPending = true;
	}

	private _invalidateEntry(entry: PaneEntry): void {
		this._markRenderPending(entry);
		entry.handle?.invalidateAll();
	}

	private _entryDirty(entry: PaneEntry, state: RafFrameState): boolean {
		if (entry.renderPending) return true;
		if (this._isHostMode(entry)) return state.dirtyByPane.get(entry.paneId) ?? true;
		const handle = entry.handle as unknown as { isDirty?: (kernel: TerminalKernel, now: number) => boolean } | null;
		if (handle === null || typeof handle.isDirty !== 'function') return true;
		try { return handle.isDirty(entry.kernel, state.dateNow); }
		catch { return true; }
	}

	private _paintFrameEntry(entry: PaneEntry, state: RafFrameState, becameVisible: boolean): void {
		const isHost = this._isHostMode(entry);
		try {
			if (isHost && (state.surfaceJustWiped || becameVisible)) {
				const handle = entry.handle as unknown as {
					repaintAll?: () => void;
					invalidateAll?: () => void;
				} | null;
				if (typeof handle?.repaintAll === 'function') handle.repaintAll();
				else handle?.invalidateAll?.();
			}
			perfMark('rg.terminal.render', () =>
				perfMark(() => `rg.terminal.render.pane.${entry.paneId}`, () => entry.handle?.render(entry.kernel)));
			entry.renderPending = false;
			state.anyRendered = true;
		} catch (error) {
			state.frameFailed = true;
			this.frameFailureCount = Math.min(this.frameFailureCount + 1, 6);
			console.error('[ridge-term] render error', entry.paneId, error);
		}
	}

	private _updateBlinkDeadline(entry: PaneEntry, state: RafFrameState): void {
		const handle = entry.handle as unknown as { nextBlinkDeadlineMs?: (kernel: TerminalKernel, now: number) => number } | null;
		if (typeof handle?.nextBlinkDeadlineMs !== 'function') return;
		try {
			const deadline = handle.nextBlinkDeadlineMs(entry.kernel, state.dateNow);
			if (Number.isFinite(deadline)) state.minDeadlineMs = Math.min(state.minDeadlineMs, deadline);
		} catch { /* watchdog covers old wasm bundles */ }
	}

	private _renderFrameEntry(entry: PaneEntry, state: RafFrameState): void {
		const hidden = !entry.parked && this._isContainerHidden(entry);
		if (entry.parked || hidden) {
			if (hidden) entry.wasHiddenLastTick = true;
			return;
		}
		const becameVisible = !!entry.wasHiddenLastTick;
		if (entry.wasHiddenLastTick) {
			entry.wasHiddenLastTick = false;
			this._recomputeViewport(entry);
			void this.fitPane(entry, this._sharedRemoteMode);
		}
		const syncWasActive = entry.syncStart !== null;
		if (!this._renderEntryAfterSync(entry, state)) return;
		if (syncWasActive && entry.syncStart === null) {
			// An explicit ?2026l boundary is stronger than the heuristic quiet
			// window: present the final cursor with the final grid frame.
			this._releaseTuiCursorSuppression(entry);
		} else if (entry.tuiCursorSuppressUntil > state.perfNow) {
			state.minDeadlineMs = Math.min(state.minDeadlineMs, entry.tuiCursorSuppressUntil - state.perfNow);
		} else if (entry.tuiCursorSuppressed) {
			this._releaseTuiCursorSuppression(entry);
		}
		const dirty = this._entryDirty(entry, state);
		const shouldRender = this._isHostMode(entry)
			? state.activeHost !== null && (dirty || state.surfaceJustWiped || becameVisible)
			: dirty;
		if (shouldRender) {
			const deadline = Number.isFinite(state.renderDeadlineMs) ? state.renderDeadlineMs : Infinity;
			const budgetExpired = performance.now() >= deadline;
			if (state.anyRendered && budgetExpired) {
				// Keep the renderer dirty for the next RAF. A wiped/just-visible
				// host also needs explicit invalidation because its old frame store
				// pixels may have been cleared before this pane got a turn.
				state.renderDeferred = true;
				if (state.surfaceJustWiped || becameVisible) {
					const handle = entry.handle as unknown as { invalidateAll?: () => void } | null;
					this._markRenderPending(entry);
					handle?.invalidateAll?.();
				}
			} else if (!this._isHostMode(entry) || this._ensureHostFrame(state)) {
				this._paintFrameEntry(entry, state, becameVisible);
			}
		}
		this._updateBlinkDeadline(entry, state);
	}

	private _finishHostFrame(state: RafFrameState): void {
		if (!state.hostFrameOpen || state.activeHost === null) return;
		try { state.activeHost.endFrame(); }
		catch (error) { console.error('[ridge-term] surfaceHost.endFrame error', error); }
	}

	private _scheduleIdleFrame(state: RafFrameState, tick: () => void): void {
		if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
		const sleepMs = Math.min(Math.max(state.minDeadlineMs, 1), 1000);
		this.idleTimer = setTimeout(() => {
			this.idleTimer = null;
			this.startRafLoop();
		}, sleepMs);
	}

	private _scheduleFrameRetry(): void {
		if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
		if (this.idleTimer !== null) return;
		const exponent = Math.max(0, this.frameFailureCount - 1);
		const retryMs = Math.min(1000, 50 * (2 ** exponent));
		this.idleTimer = setTimeout(() => {
			this.idleTimer = null;
			this.startRafLoop();
		}, retryMs);
	}

	private _hasQueuedFrameWork(order: readonly PaneEntry[]): boolean {
		if (order.some((entry) =>
			entry.deltaQueueHead < entry.deltaQueue.length || hasDeferredFeed(entry),
		)) return true;
		// A transport callback may enqueue work after `_newRafFrame` captured
		// `feedOrder`; consult the sparse live index so that wakeups cannot fall
		// asleep for the idle watchdog interval.
		for (const paneId of this.pendingFrameWorkPanes) {
			const entry = this.panes.get(paneId);
			if (!entry || (entry.deltaQueueHead >= entry.deltaQueue.length && !hasDeferredFeed(entry))) {
				this.pendingFrameWorkPanes.delete(paneId);
				continue;
			}
			if (!entry.parked) return true;
		}
		return false;
	}

	private _scheduleNextFrame(state: RafFrameState, tick: () => void): void {
		if (this.panes.size === 0) return;
		if (state.frameFailed) {
			this._scheduleFrameRetry();
			return;
		}
		// A synchronized-output boundary may intentionally suppress paint while
		// parser work remains queued. Keep draining on compositor turns; sleeping
		// until the 150 ms safety timeout makes a pre-buffered TUI burst freeze and
		// then jump to its tail even though each individual apply is cheap.
		const feedOrder = state.feedOrder ?? state.frameOrder;
		if (state.renderDeferred || state.anyRendered || this._hasQueuedFrameWork(feedOrder)) {
			this.rafHandle = requestAnimationFrame(tick);
			return;
		}
		this._scheduleIdleFrame(state, tick);
	}

	private _runRafTick(tick: () => void): void {
		perfMark('rg.frame.tick', () => {
			this.rafHandle = null;
			const perfNow = performance.now();
			const state = this._newRafFrame(perfNow, Date.now());
			for (const entry of state.frameOrder) this._renderFrameEntry(entry, state);
			this._finishHostFrame(state);
			if (!state.frameFailed) this.frameFailureCount = 0;
			this._emitScrollStateChanges();
			this._rafRotationIndex = (this._rafRotationIndex + 1) >>> 0;
			this._scheduleNextFrame(state, tick);
		});
	}

	private startRafLoop(): void {
		if (this.rafHandle !== null) return;
		this._installVisibilityListener();
		const tick = () => this._runRafTick(tick);
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
		this.frameFailureCount = 0;
		this._lastMemorySweepAt = 0;
		if (this.panes.size === 0) this._memoryRestorePending.clear();
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
