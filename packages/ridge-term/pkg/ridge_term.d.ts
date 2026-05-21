/* tslint:disable */
/* eslint-disable */

export class RenderHandle {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Reset to the default dark theme.
     */
    applyDefaultTheme(): void;
    /**
     * Apply a theme. Accepts a JS object with any subset of:
     *   { background, foreground, cursor, cursorAccent,
     *     black, red, green, yellow, blue, magenta, cyan, white,
     *     brightBlack, brightRed, brightGreen, brightYellow,
     *     brightBlue, brightMagenta, brightCyan, brightWhite }
     * Each value is a CSS hex color "#rgb" / "#rrggbb" / "#rrggbbaa".
     * Keys not provided keep their existing palette entry.
     *
     * Note: this is **partial overrides on top of the current theme**,
     * not a full replace. To reset, call `applyDefaultTheme()` first.
     */
    applyTheme(theme_obj: any): void;
    /**
     * Configure font + measure cell dimensions. Returns [cell_w, cell_h]
     * in CSS pixels so JS can calculate cols/rows for a target
     * container size.
     */
    configure(font_family: string, font_size_px: number, dpr: number): Float32Array;
    /**
     * Diagnostic: return the kernel-side renderer.theme.{bg, fg,
     * cursor_color, tui_bg} as a Uint8Array of 16 bytes (4×RGBA).
     * Lets JS confirm whether `applyTheme` actually propagated into
     * the renderer state — the JS-side `opts.theme` snapshot only
     * proves the manager *received* the theme, not that the
     * wasm renderer accepted it. Cheap (one Theme clone, 16 bytes
     * copied) so callers may poll without harm.
     */
    currentThemeProbe(): Uint8Array;
    /**
     * Force a full redraw on the next render() — useful after
     * invalidating external state without using the dedicated setters.
     */
    invalidateAll(): void;
    /**
     * Non-mutating mirror of `render`'s early-exit conditions:
     * returns `true` when the next `render` call would do any
     * drawing work, `false` when the renderer has nothing to
     * redraw and the JS caller can sleep its RAF loop. `now_ms`
     * must use the same epoch as the value passed to `render`
     * (`Date.now()` in JS).
     *
     * Cost: ~24 row hashes for an 80×24 grid (≈4 µs) plus the
     * selection / scroll / blink checks. Cheaper than one
     * `draw_row` call by two orders of magnitude.
     */
    isDirty(kernel: TerminalKernel, now_ms: number): boolean;
    /**
     * Sync constructor — Canvas2D-only. JS calls
     * `new RenderHandle(canvas)`. For runtime-WebGPU adoption with
     * graceful Canvas2D fallback, JS calls
     * `await RenderHandle.newWithWebgpuFirst(canvas)` instead.
     */
    constructor(canvas: HTMLCanvasElement);
    /**
     * Async constructor — try WebGPU first, fall back to Canvas2D
     * on adapter miss / device-creation failure. Always succeeds
     * when `Canvas2dBackend::new` succeeds; returns Err only if
     * even the Canvas2D fallback can't initialize (rare; usually
     * indicates a malformed canvas element).
     *
     * Only compiled when the `webgpu` cargo feature is on (the
     * `wasm-bindgen-futures` dep needed for `#[wasm_bindgen]
     * async fn` is gated behind that feature). In default builds,
     * JS callers should use the sync `new RenderHandle(canvas)`
     * constructor; they can detect the async constructor's
     * presence via `typeof RenderHandle.newWithWebgpuFirst ===
     * 'function'`.
     */
    static newWithWebgpuFirst(canvas: HTMLCanvasElement, surface_host?: SurfaceHostHandle | null): Promise<RenderHandle>;
    /**
     * Milliseconds until the next cursor-blink phase boundary. JS
     * callers use this to schedule a `setTimeout` wake-up while
     * the RAF loop is paused. Returns a very large number
     * (effectively infinity) when the cursor isn't blinking — the
     * caller should treat any value > some reasonable cap (e.g.
     * 1000 ms) as "no blink, sleep at most a second on a watchdog".
     */
    nextBlinkDeadlineMs(kernel: TerminalKernel, now_ms: number): number;
    /**
     * §4b per-pane increment cache (2026-05-08): re-record this
     * pane's previously-uploaded GPU instance buffer into the
     * host's current frame without retraversing the kernel grid.
     * Returns `true` on success, `false` when the cache was
     * invalidated (caller must fall back to full `render`).
     *
     * Used by `manager.ts::startRafLoop` for visible host-mode
     * panes that pre-pass marked NOT dirty: the swap-chain
     * `LoadOp::Clear` would otherwise wipe their region (forcing
     * a re-encode for unchanged content). With this path, the
     * per-tick CPU cost of N idle visible panes drops from
     * O(rows × cols × N) to one GPU draw call per pane —
     * eliminating the typing-while-other-panes-have-output lag
     * (forceHostRenderAll's multiplier).
     */
    recordCachedOnly(): boolean;
    /**
     * Drive one frame from the kernel's current grid. Returns true
     * if anything was drawn (caller can use this to decide whether
     * to schedule another frame). Selection range comes from the
     * kernel's `selection` field — when set, the renderer paints a
     * translucent overlay over those cells. Wall-clock comes from
     * `Date.now()` for cursor-blink phase.
     */
    render(kernel: TerminalKernel): boolean;
    resize(width_css: number, height_css: number, dpr: number): void;
    /**
     * Multi-pane hosts call this when the active pane changes. When
     * `focused` is false, the renderer skips cursor draw entirely so
     * only the truly active terminal blinks. Idempotent.
     */
    setFocused(focused: boolean): void;
    /**
     * Phase B: record the pane's `(x, y)` position on the host
     * canvas in **device pixels**. JS calls this from
     * `manager.ts::_recomputeViewport` whenever the splitter drag
     * moves the pane's container without changing its size.
     *
     * No-op for Canvas2D-backed handles. WebGPU handles forward
     * to `WebGpuPaneBackend::set_viewport_offset`. Does **not**
     * trigger a redraw on its own — the pane content is unchanged
     * on a positional shift; JS calls `surfaceHost.invalidate()`
     * after layout settle to clear the old area.
     */
    setViewportOffset(x: number, y: number): void;
}

export class SurfaceHostHandle {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Begin one host frame: acquire swap-chain texture + create
     * encoder. Returns `true` on success, `false` on surface-lost
     * — JS skips the rest of the frame and lets the next RAF
     * retry. `theme_bg` is a 4-byte RGBA buffer; values outside
     * `[0..255]` get clamped at the byte boundary by
     * `Uint8Array.set`.
     *
     * Idempotent guard: a second call without an intervening
     * `endFrame` drops the stale frame and starts fresh (defense
     * against JS bugs that skip the end half).
     */
    beginFrame(theme_bg: Uint8Array): boolean;
    /**
     * JS-callable clone: produces a new `SurfaceHostHandle` JS
     * wrapper that bumps the inner `Rc` refcount. Required because
     * `RenderHandle::newWithWebgpuFirst(canvas, host)` consumes
     * its `host` parameter (wasm-bindgen `Option<T>` semantics —
     * the JS-side wrapper is freed after the call). When N panes
     * in the same workspace each call attach, JS must
     * `host.clone()` per call so the manager's stored handle
     * stays alive.
     */
    clone(): SurfaceHostHandle;
    /**
     * Finish the host's command encoder + queue.submit + present.
     * One call per frame after all dirty panes have rendered. Safe
     * to call without a matching `beginFrame` — internal guard
     * returns early.
     */
    endFrame(): void;
    /**
     * Async constructor: create one swap chain bound to `canvas`.
     * One SurfaceHostHandle per workspace tab — JS holds a Map
     * keyed by workspace id and passes the matching handle to
     * each pane's `RenderHandle.newWithWebgpuFirst(canvas, host)`.
     *
     * Returns `Err` (rejected promise on the JS side) when the
     * WebGPU adapter / device acquisition fails or
     * `instance.create_surface` rejects the canvas. JS catches
     * and either retries or falls back to per-pane Canvas2D for
     * panes in this workspace.
     */
    static init(canvas: HTMLCanvasElement): Promise<SurfaceHostHandle>;
    /**
     * Mark the next frame for a fresh `LoadOp::Clear`. JS calls
     * this when a pane detaches / parks / unparks (so departed
     * pixels don't linger), when the theme changes, and after
     * splitter settle moves pane boundaries.
     */
    invalidate(): void;
    /**
     * Resize the host swap chain. JS drives this from a
     * ResizeObserver on the host canvas's parent so the surface
     * always matches the visible workspace area.
     */
    resize(width_css: number, height_css: number, dpr: number): void;
}

export class TerminalKernel {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * P3.6 (2026-05-20) — apply one postcard-encoded `DeltaFrame` (produced
     * by the Rust-side `engine::parser::PaneParser`) to the mirror grid.
     *
     * Counterpart to `feed()` for the `Settings.parserBackend = 'rust'`
     * path: PTY bytes are parsed once by the native PaneParser, the
     * resulting frame is postcard-encoded and emitted as a Tauri event,
     * and the wasm consumer applies the diff here instead of running its
     * own vte parse on the JS main thread.
     *
     * Returns `Err(JsValue)` with a human-readable string on decode
     * failure OR protocol-version mismatch — caller is expected to log
     * and trigger a `force_full_reframe` self-heal (manager.ts P3.9
     * wiring). Selection / search anchors are cleared on every applied
     * frame because the mirror's grid mutates without going through
     * `feed()` (which has its own eviction-counter-based clear).
     */
    applyDeltaFrame(bytes: Uint8Array): void;
    /**
     * §1.27 (2026-05-07) — diagnostic cell inspector for the dim/IME
     * residue investigation. Returns up to `len` cells starting at
     * (row, col) on the active screen as a JS array of plain objects
     * `{ col, ch, codepoint, width, attrId, dim, bold, italic,
     * underline, inverse, hidden, fg, bg }` so devtools can correlate
     * "what does the user see at this position" with "what attrs are
     * stored".
     *
     * Out-of-range row, col, or len silently returns a shorter array
     * (or empty) rather than panicking — devtools should treat the
     * shorter result as "row missing or too narrow".
     *
     * Frontend usage (when `localStorage.RIDGE_DIAG === '1'`):
     *   `__RIDGE_KERNEL.cellsAt(cursorRow, 0, 80)` right after a
     *   compositionEnd to verify whether DIM cells leaked into the
     *   prompt area, or after observing residue to confirm whether
     *   the underlying cell carries a DIM attribute (kernel bug) vs
     *   correct attrs but stale pixels (renderer bug). See
     *   `docs/term-rebuild/REPRO_dim_residue.md`.
     */
    cellsAt(row: number, col: number, len: number): any[];
    /**
     * §B.2 (2026-05-08) — drop the in-kernel scrollback ring buffer
     * (physical clear) and snap viewport to live grid. Mirrors
     * `\x1b[3J` at the JS API level so the right-click "清空" path
     * can wipe both screen + saved lines without a PTY round trip
     * (and without stepping on shells that don't translate Ctrl+L
     * into ED 3). Selection is cleared so it doesn't survive into
     * nonexistent rows. Search results similarly drop.
     */
    clearScrollback(): void;
    clearSelection(): void;
    cols(): number;
    /**
     * Cursor column in viewport coordinates (0-based).
     */
    cursorCol(): number;
    /**
     * Cursor row in viewport coordinates (0-based). Used by the IME
     * helper-textarea positioning to anchor the candidate window near
     * the actual input position.
     */
    cursorRow(): number;
    dumpVisibleText(): any[];
    /**
     * Encode a key event to the byte sequence the PTY expects. Returns
     * an empty array if the event is unknown (caller may then let the
     * browser handle it natively).
     *
     * The JS-side normalizes `meta` (Cmd) into `ctrl` on macOS before
     * calling — see `input.rs` for rationale.
     */
    encodeKey(key: string, ctrl: boolean, alt: boolean, shift: boolean, meta: boolean): Uint8Array;
    /**
     * Encode a mouse event as an SGR terminal sequence. Delegates to
     * `input::encode_mouse` which generates `ESC [ < btn ; col ; row [Mm]`
     * per xterm SGR spec (column first, then row).
     * Always uses SGR format regardless of ?1006 state — the terminal
     * decodes both; SGR is simpler and doesn't overflow at high row/col.
     */
    encodeMouse(row: number, col: number, button: number, action: number, shift: boolean, ctrl: boolean, alt: boolean): Uint8Array;
    /**
     * Wrap a paste string for the PTY, applying bracketed-paste
     * markers when DEC mode 2004 is active.
     */
    encodePaste(text: string): Uint8Array;
    feed(bytes: Uint8Array): void;
    getSelectionText(): string;
    hasSelection(): boolean;
    /**
     * Look up the OSC 8 hyperlink span containing the cell at `(row, col)`
     * in viewport coordinates. Returns `{ uri, id }` or `null`. Used by
     * the manager's Ctrl+click handler to decide whether to open a link.
     */
    hyperlinkAt(row: number, col: number): any;
    isAltScreen(): boolean;
    isAppCursorKeys(): boolean;
    isBracketedPaste(): boolean;
    isCursorVisible(): boolean;
    /**
     * Focus reporting mode `?1004`. While `true`, the manager should emit
     * `\x1b[I` on focus-in and `\x1b[O` on focus-out via the same
     * dataHandler channel as keyboard input. claude code, vim, fzf use
     * these to refresh state when the user switches to / from the pane.
     */
    isFocusReporting(): boolean;
    /**
     * §A.3 inline-TUI heuristic — true when an Ink-style app is rendering
     * inline on primary (cursor hidden + recent absolute-positioning CSI
     * within the decay window) and the kernel is NOT on alt screen.
     * Read by `manager.ts::fitPane` to decide whether to wipe primary
     * before resizing the PTY (mirrors the existing alt-screen branch).
     * Also read by `manager.ts::isInlineTuiActive` for keyboard/mouse
     * priority routing — see also `isMouseReporting`.
     */
    isInlineTuiMode(): boolean;
    /**
     * Returns true when ?1003 (any-event / motion tracking) is active.
     */
    isMouseAnyEvent(): boolean;
    /**
     * Returns true when ?1002 (button-event / drag tracking) is active.
     */
    isMouseButtonEvent(): boolean;
    /**
     * Returns true when any DEC mouse reporting mode is active
     * (?1000 normal, ?1002 button-event, or ?1003 any-event).
     */
    isMouseReporting(): boolean;
    /**
     * Returns true when ?1006 (SGR mouse encoding) is active.
     */
    isMouseSgr(): boolean;
    /**
     * Synchronous output mode `?2026`. While `true`, the manager should
     * hold off rendering frames so the user doesn't see torn intermediate
     * states during multi-step redraws (Ink/lazygit/bottom). Manager
     * owns the timeout fallback (default 150ms) so this stays a clock-free
     * boolean check.
     */
    isSyncOutput(): boolean;
    /**
     * Whether the user has paged into history and PTY output is
     * currently being held back from auto-snapping the viewport.
     * JS surfaces this as a "follow tail" indicator. Cleared by
     * `scrollToBottom`.
     */
    isUserScrollLocked(): boolean;
    /**
     * Diagnostic accessor for the alt-screen-resize bug investigation
     * (§1.22 / §1.23 / §1.24). Returns the kernel's last 32 resize calls
     * as a JS array of `{ old_rows, old_cols, new_rows, new_cols, is_alt,
     * dim_changed, branch, wipe_fired }` objects, newest last.
     *
     * Frontend usage (when `localStorage.RIDGE_DIAG === '1'`):
     *   `__RIDGE_KERNEL.lastResizeDiags()` after a live resize confirms
     *   whether `is_alt` was true at the kernel level and whether the
     *   §1.22 wipe path fired. See `docs/term-rebuild/REPRO_alt_resize.md`.
     * §1.27-tail (2026-05-07) — JS-accessible snapshot of the cursor's
     * position at the moment of the most recent absolute-positioning
     * CSI. Returns `null` (JS) when no abs CSI has been observed.
     * Otherwise returns `{ row, col, atMs }` where `atMs` is the
     * wall-clock unix-epoch ms timestamp.
     *
     * Frontend usage: `manager.ts::inputAnchorPixelPosition` falls back
     * to this snapshot (when within the inline-TUI decay window) before
     * falling back to the live cursor — so the IME helper anchor stays
     * at the inline-TUI's input row even when the live cursor is
     * mid-walk. See §1.27 in CLAUDE.md.
     */
    lastAbsCsiPosition(): any;
    lastResizeDiags(): any[];
    /**
     * Single-call bitmask of every DEC mouse mode the caller cares
     * about. Eliminates the 3-4 separate wasm boundary crossings the
     * JS pointer handlers used to make per pointermove event:
     *
     *   bit 0 (0x1) = ?1000 (mouse_normal)
     *   bit 1 (0x2) = ?1002 (button_event / drag tracking)
     *   bit 2 (0x4) = ?1003 (any_event / all motion)
     *   bit 3 (0x8) = ?1006 (SGR encoding)
     *
     * `bits != 0` <=> `isMouseReporting() == true`. The individual
     * boolean getters above are kept for non-hot-path callers.
     */
    mouseReportingModes(): number;
    constructor(rows: number, cols: number, scrollback: number);
    /**
     * Prepend older history at the OLDEST end of the scrollback ring.
     *
     * Used by `manager.fetchOlderScrollback`: when the user pages up past
     * the in-kernel scrollback boundary, the JS layer fetches an older
     * chunk from the Tauri `get_pane_scrollback_before` command and feeds
     * it here. The bytes are parsed in an isolated sandbox so the live
     * grid, cursor, attrs, modes, and pending queues are untouched —
     * only the scrollback ring grows at its older end. Selection and
     * search anchors stay valid because existing rows don't move.
     *
     * See `Terminal::prepend_scrollback` for sandbox / AttrId-remap
     * semantics.
     */
    prependScrollback(bytes: Uint8Array): void;
    resize(rows: number, cols: number): void;
    rows(): number;
    scrollDown(n: number): void;
    scrollOffset(): number;
    scrollToBottom(): void;
    scrollUp(n: number): void;
    scrollbackLen(): number;
    /**
     * Returns the active match index, or `usize::MAX` when no active match.
     */
    searchActiveIndex(): number;
    /**
     * Clear search state and the highlight selection.
     */
    searchClear(): void;
    searchMatchCount(): number;
    /**
     * Step to the next match (wraps). Returns the new active index, or
     * `usize::MAX` if there are no matches.
     */
    searchNext(): number;
    /**
     * Step to the previous match (wraps).
     */
    searchPrev(): number;
    /**
     * Run a search across scrollback + viewport. Returns the number of
     * matches. Scrolls the viewport so the first match is visible and
     * sets the selection to it (renderer's existing overlay highlights).
     * Empty query clears search state and selection.
     */
    searchSetQuery(query: string, case_sensitive: boolean): number;
    selectAll(): void;
    /**
     * Triple-click line selection.
     */
    selectLineAt(row: number): void;
    /**
     * Double-click word selection. Selects the word at the given cell
     * coordinate; clears selection when the cell is whitespace/empty.
     */
    selectWordAt(row: number, col: number): void;
    /**
     * Programmatically set a selection range. Coordinates are
     * viewport-relative (same as the renderer).
     */
    setSelection(start_row: number, start_col: number, end_row: number, end_col: number): void;
    /**
     * Programmatically set a selection range in **absolute-row coords**
     * (see `selection.rs` module docstring). The JS-side drag state
     * machine in `manager.ts` stores its anchor / focus as `abs_row =
     * vp_row + scroll_offset` so the selection survives scroll without
     * the caller having to re-translate every sync — this entry point
     * lets it forward those abs values directly. Skips the vp→abs
     * conversion that `set_selection` does internally, so it's safe to
     * call repeatedly during a drag that scrolls the viewport.
     */
    setSelectionAbs(start_abs_row: number, start_col: number, end_abs_row: number, end_col: number): void;
    /**
     * Drain semantic events (title, cwd, hyperlinks, bell) produced by
     * the parser during the most recent `feed` calls. Returns a JS
     * array of tagged objects: `{ type: "TitleChanged", value: "..." }`
     * etc. Caller routes each event to the relevant Svelte store
     * (paneTitleStore, paneCwdStore, ...).
     */
    takePendingEvents(): any[];
    /**
     * Drain query-response bytes (DSR `\x1b[r;cR`, DA `\x1b[?...c`) the
     * parser produced during the most recent `feed` calls. Caller MUST
     * forward these bytes to the PTY as if they were keystrokes; without
     * this round-trip, PowerShell + ConPTY render the prompt at a stale
     * cursor row after a child process exits (e.g. Ctrl+C out of a TUI),
     * overwriting whatever was on screen.
     */
    takePendingResponse(): Uint8Array;
}

export function _init(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_terminalkernel_free: (a: number, b: number) => void;
    readonly _init: () => void;
    readonly terminalkernel_applyDeltaFrame: (a: number, b: number, c: number) => [number, number];
    readonly terminalkernel_cellsAt: (a: number, b: number, c: number, d: number) => [number, number];
    readonly terminalkernel_clearScrollback: (a: number) => void;
    readonly terminalkernel_clearSelection: (a: number) => void;
    readonly terminalkernel_cols: (a: number) => number;
    readonly terminalkernel_cursorCol: (a: number) => number;
    readonly terminalkernel_cursorRow: (a: number) => number;
    readonly terminalkernel_dumpVisibleText: (a: number) => [number, number];
    readonly terminalkernel_encodeKey: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
    readonly terminalkernel_encodeMouse: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly terminalkernel_encodePaste: (a: number, b: number, c: number) => [number, number];
    readonly terminalkernel_feed: (a: number, b: number, c: number) => void;
    readonly terminalkernel_getSelectionText: (a: number) => [number, number];
    readonly terminalkernel_hasSelection: (a: number) => number;
    readonly terminalkernel_hyperlinkAt: (a: number, b: number, c: number) => any;
    readonly terminalkernel_isAltScreen: (a: number) => number;
    readonly terminalkernel_isAppCursorKeys: (a: number) => number;
    readonly terminalkernel_isBracketedPaste: (a: number) => number;
    readonly terminalkernel_isCursorVisible: (a: number) => number;
    readonly terminalkernel_isFocusReporting: (a: number) => number;
    readonly terminalkernel_isInlineTuiMode: (a: number) => number;
    readonly terminalkernel_isMouseAnyEvent: (a: number) => number;
    readonly terminalkernel_isMouseButtonEvent: (a: number) => number;
    readonly terminalkernel_isMouseReporting: (a: number) => number;
    readonly terminalkernel_isMouseSgr: (a: number) => number;
    readonly terminalkernel_isSyncOutput: (a: number) => number;
    readonly terminalkernel_isUserScrollLocked: (a: number) => number;
    readonly terminalkernel_lastAbsCsiPosition: (a: number) => any;
    readonly terminalkernel_lastResizeDiags: (a: number) => [number, number];
    readonly terminalkernel_mouseReportingModes: (a: number) => number;
    readonly terminalkernel_new: (a: number, b: number, c: number) => number;
    readonly terminalkernel_prependScrollback: (a: number, b: number, c: number) => void;
    readonly terminalkernel_resize: (a: number, b: number, c: number) => void;
    readonly terminalkernel_rows: (a: number) => number;
    readonly terminalkernel_scrollDown: (a: number, b: number) => void;
    readonly terminalkernel_scrollOffset: (a: number) => number;
    readonly terminalkernel_scrollToBottom: (a: number) => void;
    readonly terminalkernel_scrollUp: (a: number, b: number) => void;
    readonly terminalkernel_scrollbackLen: (a: number) => number;
    readonly terminalkernel_searchActiveIndex: (a: number) => number;
    readonly terminalkernel_searchClear: (a: number) => void;
    readonly terminalkernel_searchMatchCount: (a: number) => number;
    readonly terminalkernel_searchNext: (a: number) => number;
    readonly terminalkernel_searchPrev: (a: number) => number;
    readonly terminalkernel_searchSetQuery: (a: number, b: number, c: number, d: number) => number;
    readonly terminalkernel_selectAll: (a: number) => void;
    readonly terminalkernel_selectLineAt: (a: number, b: number) => void;
    readonly terminalkernel_selectWordAt: (a: number, b: number, c: number) => void;
    readonly terminalkernel_setSelection: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly terminalkernel_setSelectionAbs: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly terminalkernel_takePendingEvents: (a: number) => [number, number];
    readonly terminalkernel_takePendingResponse: (a: number) => [number, number];
    readonly __wbg_renderhandle_free: (a: number, b: number) => void;
    readonly __wbg_surfacehosthandle_free: (a: number, b: number) => void;
    readonly renderhandle_applyDefaultTheme: (a: number) => void;
    readonly renderhandle_applyTheme: (a: number, b: any) => [number, number];
    readonly renderhandle_configure: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly renderhandle_currentThemeProbe: (a: number) => [number, number];
    readonly renderhandle_invalidateAll: (a: number) => void;
    readonly renderhandle_isDirty: (a: number, b: number, c: number) => number;
    readonly renderhandle_new: (a: any) => [number, number, number];
    readonly renderhandle_newWithWebgpuFirst: (a: any, b: number) => any;
    readonly renderhandle_nextBlinkDeadlineMs: (a: number, b: number, c: number) => number;
    readonly renderhandle_recordCachedOnly: (a: number) => number;
    readonly renderhandle_render: (a: number, b: number) => number;
    readonly renderhandle_resize: (a: number, b: number, c: number, d: number) => [number, number];
    readonly renderhandle_setFocused: (a: number, b: number) => void;
    readonly renderhandle_setViewportOffset: (a: number, b: number, c: number) => void;
    readonly surfacehosthandle_beginFrame: (a: number, b: number, c: number) => number;
    readonly surfacehosthandle_clone: (a: number) => number;
    readonly surfacehosthandle_endFrame: (a: number) => void;
    readonly surfacehosthandle_init: (a: any) => any;
    readonly surfacehosthandle_invalidate: (a: number) => void;
    readonly surfacehosthandle_resize: (a: number, b: number, c: number, d: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__hb9b2628a6f28b20e: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h8146b976d3444cd3: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h6d9ba260bb3306dc: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h5d643d96cae4f886: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__hde54ef055b7489c1: (a: number, b: number) => number;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __externref_drop_slice: (a: number, b: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
