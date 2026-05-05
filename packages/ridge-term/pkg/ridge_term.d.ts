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
     * Force a full redraw on the next render() — useful after
     * invalidating external state without using the dedicated setters.
     */
    invalidateAll(): void;
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
    static newWithWebgpuFirst(canvas: HTMLCanvasElement): Promise<RenderHandle>;
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
}

export class TerminalKernel {
    free(): void;
    [Symbol.dispose](): void;
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
     * Synchronous output mode `?2026`. While `true`, the manager should
     * hold off rendering frames so the user doesn't see torn intermediate
     * states during multi-step redraws (Ink/lazygit/bottom). Manager
     * owns the timeout fallback (default 150ms) so this stays a clock-free
     * boolean check.
     */
    isSyncOutput(): boolean;
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
    readonly __wbg_renderhandle_free: (a: number, b: number) => void;
    readonly __wbg_terminalkernel_free: (a: number, b: number) => void;
    readonly _init: () => void;
    readonly renderhandle_applyDefaultTheme: (a: number) => void;
    readonly renderhandle_applyTheme: (a: number, b: any) => [number, number];
    readonly renderhandle_configure: (a: number, b: number, c: number, d: number, e: number) => [number, number, number, number];
    readonly renderhandle_invalidateAll: (a: number) => void;
    readonly renderhandle_new: (a: any) => [number, number, number];
    readonly renderhandle_newWithWebgpuFirst: (a: any) => any;
    readonly renderhandle_render: (a: number, b: number) => number;
    readonly renderhandle_resize: (a: number, b: number, c: number, d: number) => [number, number];
    readonly renderhandle_setFocused: (a: number, b: number) => void;
    readonly terminalkernel_clearSelection: (a: number) => void;
    readonly terminalkernel_cols: (a: number) => number;
    readonly terminalkernel_cursorCol: (a: number) => number;
    readonly terminalkernel_cursorRow: (a: number) => number;
    readonly terminalkernel_dumpVisibleText: (a: number) => [number, number];
    readonly terminalkernel_encodeKey: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number];
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
    readonly terminalkernel_isSyncOutput: (a: number) => number;
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
    readonly terminalkernel_takePendingEvents: (a: number) => [number, number];
    readonly terminalkernel_takePendingResponse: (a: number) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h5b8f9f9118d17a3b: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h386c8d8a4d76669f: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42_2: (a: number, b: number, c: any) => void;
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
