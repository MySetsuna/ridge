/* @ts-self-types="./ridge_term.d.ts" */

export class RenderHandle {
    static __wrap(ptr) {
        const obj = Object.create(RenderHandle.prototype);
        obj.__wbg_ptr = ptr;
        RenderHandleFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        RenderHandleFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_renderhandle_free(ptr, 0);
    }
    /**
     * Reset to the default dark theme.
     */
    applyDefaultTheme() {
        wasm.renderhandle_applyDefaultTheme(this.__wbg_ptr);
    }
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
     * @param {any} theme_obj
     */
    applyTheme(theme_obj) {
        const ret = wasm.renderhandle_applyTheme(this.__wbg_ptr, theme_obj);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Configure font + measure cell dimensions. Returns [cell_w, cell_h]
     * in CSS pixels so JS can calculate cols/rows for a target
     * container size.
     * @param {string} font_family
     * @param {number} font_size_px
     * @param {number} dpr
     * @returns {Float32Array}
     */
    configure(font_family, font_size_px, dpr) {
        const ptr0 = passStringToWasm0(font_family, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.renderhandle_configure(this.__wbg_ptr, ptr0, len0, font_size_px, dpr);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
    /**
     * Force a full redraw on the next render() — useful after
     * invalidating external state without using the dedicated setters.
     */
    invalidateAll() {
        wasm.renderhandle_invalidateAll(this.__wbg_ptr);
    }
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
     * @param {TerminalKernel} kernel
     * @param {number} now_ms
     * @returns {boolean}
     */
    isDirty(kernel, now_ms) {
        _assertClass(kernel, TerminalKernel);
        const ret = wasm.renderhandle_isDirty(this.__wbg_ptr, kernel.__wbg_ptr, now_ms);
        return ret !== 0;
    }
    /**
     * Sync constructor — Canvas2D-only. JS calls
     * `new RenderHandle(canvas)`. For runtime-WebGPU adoption with
     * graceful Canvas2D fallback, JS calls
     * `await RenderHandle.newWithWebgpuFirst(canvas)` instead.
     * @param {HTMLCanvasElement} canvas
     */
    constructor(canvas) {
        const ret = wasm.renderhandle_new(canvas);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        RenderHandleFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
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
     * @param {HTMLCanvasElement} canvas
     * @returns {Promise<RenderHandle>}
     */
    static newWithWebgpuFirst(canvas) {
        const ret = wasm.renderhandle_newWithWebgpuFirst(canvas);
        return ret;
    }
    /**
     * Milliseconds until the next cursor-blink phase boundary. JS
     * callers use this to schedule a `setTimeout` wake-up while
     * the RAF loop is paused. Returns a very large number
     * (effectively infinity) when the cursor isn't blinking — the
     * caller should treat any value > some reasonable cap (e.g.
     * 1000 ms) as "no blink, sleep at most a second on a watchdog".
     * @param {TerminalKernel} kernel
     * @param {number} now_ms
     * @returns {number}
     */
    nextBlinkDeadlineMs(kernel, now_ms) {
        _assertClass(kernel, TerminalKernel);
        const ret = wasm.renderhandle_nextBlinkDeadlineMs(this.__wbg_ptr, kernel.__wbg_ptr, now_ms);
        return ret;
    }
    /**
     * Drive one frame from the kernel's current grid. Returns true
     * if anything was drawn (caller can use this to decide whether
     * to schedule another frame). Selection range comes from the
     * kernel's `selection` field — when set, the renderer paints a
     * translucent overlay over those cells. Wall-clock comes from
     * `Date.now()` for cursor-blink phase.
     * @param {TerminalKernel} kernel
     * @returns {boolean}
     */
    render(kernel) {
        _assertClass(kernel, TerminalKernel);
        const ret = wasm.renderhandle_render(this.__wbg_ptr, kernel.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} width_css
     * @param {number} height_css
     * @param {number} dpr
     */
    resize(width_css, height_css, dpr) {
        const ret = wasm.renderhandle_resize(this.__wbg_ptr, width_css, height_css, dpr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Multi-pane hosts call this when the active pane changes. When
     * `focused` is false, the renderer skips cursor draw entirely so
     * only the truly active terminal blinks. Idempotent.
     * @param {boolean} focused
     */
    setFocused(focused) {
        wasm.renderhandle_setFocused(this.__wbg_ptr, focused);
    }
}
if (Symbol.dispose) RenderHandle.prototype[Symbol.dispose] = RenderHandle.prototype.free;

export class TerminalKernel {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        TerminalKernelFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_terminalkernel_free(ptr, 0);
    }
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
     * @param {number} row
     * @param {number} col
     * @param {number} len
     * @returns {any[]}
     */
    cellsAt(row, col, len) {
        const ret = wasm.terminalkernel_cellsAt(this.__wbg_ptr, row, col, len);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    clearSelection() {
        wasm.terminalkernel_clearSelection(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    cols() {
        const ret = wasm.terminalkernel_cols(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Cursor column in viewport coordinates (0-based).
     * @returns {number}
     */
    cursorCol() {
        const ret = wasm.terminalkernel_cursorCol(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Cursor row in viewport coordinates (0-based). Used by the IME
     * helper-textarea positioning to anchor the candidate window near
     * the actual input position.
     * @returns {number}
     */
    cursorRow() {
        const ret = wasm.terminalkernel_cursorRow(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {any[]}
     */
    dumpVisibleText() {
        const ret = wasm.terminalkernel_dumpVisibleText(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Encode a key event to the byte sequence the PTY expects. Returns
     * an empty array if the event is unknown (caller may then let the
     * browser handle it natively).
     *
     * The JS-side normalizes `meta` (Cmd) into `ctrl` on macOS before
     * calling — see `input.rs` for rationale.
     * @param {string} key
     * @param {boolean} ctrl
     * @param {boolean} alt
     * @param {boolean} shift
     * @param {boolean} meta
     * @returns {Uint8Array}
     */
    encodeKey(key, ctrl, alt, shift, meta) {
        const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.terminalkernel_encodeKey(this.__wbg_ptr, ptr0, len0, ctrl, alt, shift, meta);
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * Wrap a paste string for the PTY, applying bracketed-paste
     * markers when DEC mode 2004 is active.
     * @param {string} text
     * @returns {Uint8Array}
     */
    encodePaste(text) {
        const ptr0 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.terminalkernel_encodePaste(this.__wbg_ptr, ptr0, len0);
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * @param {Uint8Array} bytes
     */
    feed(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.terminalkernel_feed(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @returns {string}
     */
    getSelectionText() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.terminalkernel_getSelectionText(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {boolean}
     */
    hasSelection() {
        const ret = wasm.terminalkernel_hasSelection(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Look up the OSC 8 hyperlink span containing the cell at `(row, col)`
     * in viewport coordinates. Returns `{ uri, id }` or `null`. Used by
     * the manager's Ctrl+click handler to decide whether to open a link.
     * @param {number} row
     * @param {number} col
     * @returns {any}
     */
    hyperlinkAt(row, col) {
        const ret = wasm.terminalkernel_hyperlinkAt(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * @returns {boolean}
     */
    isAltScreen() {
        const ret = wasm.terminalkernel_isAltScreen(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {boolean}
     */
    isAppCursorKeys() {
        const ret = wasm.terminalkernel_isAppCursorKeys(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {boolean}
     */
    isBracketedPaste() {
        const ret = wasm.terminalkernel_isBracketedPaste(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {boolean}
     */
    isCursorVisible() {
        const ret = wasm.terminalkernel_isCursorVisible(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Focus reporting mode `?1004`. While `true`, the manager should emit
     * `\x1b[I` on focus-in and `\x1b[O` on focus-out via the same
     * dataHandler channel as keyboard input. claude code, vim, fzf use
     * these to refresh state when the user switches to / from the pane.
     * @returns {boolean}
     */
    isFocusReporting() {
        const ret = wasm.terminalkernel_isFocusReporting(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * §A.3 inline-TUI heuristic — true when an Ink-style app is rendering
     * inline on primary (cursor hidden + recent absolute-positioning CSI
     * within the decay window) and the kernel is NOT on alt screen.
     * Read by `manager.ts::fitPane` to decide whether to wipe primary
     * before resizing the PTY (mirrors the existing alt-screen branch).
     * @returns {boolean}
     */
    isInlineTuiMode() {
        const ret = wasm.terminalkernel_isInlineTuiMode(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Synchronous output mode `?2026`. While `true`, the manager should
     * hold off rendering frames so the user doesn't see torn intermediate
     * states during multi-step redraws (Ink/lazygit/bottom). Manager
     * owns the timeout fallback (default 150ms) so this stays a clock-free
     * boolean check.
     * @returns {boolean}
     */
    isSyncOutput() {
        const ret = wasm.terminalkernel_isSyncOutput(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Whether the user has paged into history and PTY output is
     * currently being held back from auto-snapping the viewport.
     * JS surfaces this as a "follow tail" indicator. Cleared by
     * `scrollToBottom`.
     * @returns {boolean}
     */
    isUserScrollLocked() {
        const ret = wasm.terminalkernel_isUserScrollLocked(this.__wbg_ptr);
        return ret !== 0;
    }
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
     * @returns {any[]}
     */
    lastResizeDiags() {
        const ret = wasm.terminalkernel_lastResizeDiags(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @param {number} rows
     * @param {number} cols
     * @param {number} scrollback
     */
    constructor(rows, cols, scrollback) {
        const ret = wasm.terminalkernel_new(rows, cols, scrollback);
        this.__wbg_ptr = ret;
        TerminalKernelFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
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
     * @param {Uint8Array} bytes
     */
    prependScrollback(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.terminalkernel_prependScrollback(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @param {number} rows
     * @param {number} cols
     */
    resize(rows, cols) {
        wasm.terminalkernel_resize(this.__wbg_ptr, rows, cols);
    }
    /**
     * @returns {number}
     */
    rows() {
        const ret = wasm.terminalkernel_rows(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} n
     */
    scrollDown(n) {
        wasm.terminalkernel_scrollDown(this.__wbg_ptr, n);
    }
    /**
     * @returns {number}
     */
    scrollOffset() {
        const ret = wasm.terminalkernel_scrollOffset(this.__wbg_ptr);
        return ret >>> 0;
    }
    scrollToBottom() {
        wasm.terminalkernel_scrollToBottom(this.__wbg_ptr);
    }
    /**
     * @param {number} n
     */
    scrollUp(n) {
        wasm.terminalkernel_scrollUp(this.__wbg_ptr, n);
    }
    /**
     * @returns {number}
     */
    scrollbackLen() {
        const ret = wasm.terminalkernel_scrollbackLen(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the active match index, or `usize::MAX` when no active match.
     * @returns {number}
     */
    searchActiveIndex() {
        const ret = wasm.terminalkernel_searchActiveIndex(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Clear search state and the highlight selection.
     */
    searchClear() {
        wasm.terminalkernel_searchClear(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    searchMatchCount() {
        const ret = wasm.terminalkernel_searchMatchCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Step to the next match (wraps). Returns the new active index, or
     * `usize::MAX` if there are no matches.
     * @returns {number}
     */
    searchNext() {
        const ret = wasm.terminalkernel_searchNext(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Step to the previous match (wraps).
     * @returns {number}
     */
    searchPrev() {
        const ret = wasm.terminalkernel_searchPrev(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Run a search across scrollback + viewport. Returns the number of
     * matches. Scrolls the viewport so the first match is visible and
     * sets the selection to it (renderer's existing overlay highlights).
     * Empty query clears search state and selection.
     * @param {string} query
     * @param {boolean} case_sensitive
     * @returns {number}
     */
    searchSetQuery(query, case_sensitive) {
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.terminalkernel_searchSetQuery(this.__wbg_ptr, ptr0, len0, case_sensitive);
        return ret >>> 0;
    }
    selectAll() {
        wasm.terminalkernel_selectAll(this.__wbg_ptr);
    }
    /**
     * Triple-click line selection.
     * @param {number} row
     */
    selectLineAt(row) {
        wasm.terminalkernel_selectLineAt(this.__wbg_ptr, row);
    }
    /**
     * Double-click word selection. Selects the word at the given cell
     * coordinate; clears selection when the cell is whitespace/empty.
     * @param {number} row
     * @param {number} col
     */
    selectWordAt(row, col) {
        wasm.terminalkernel_selectWordAt(this.__wbg_ptr, row, col);
    }
    /**
     * Programmatically set a selection range. Coordinates are
     * viewport-relative (same as the renderer).
     * @param {number} start_row
     * @param {number} start_col
     * @param {number} end_row
     * @param {number} end_col
     */
    setSelection(start_row, start_col, end_row, end_col) {
        wasm.terminalkernel_setSelection(this.__wbg_ptr, start_row, start_col, end_row, end_col);
    }
    /**
     * Drain semantic events (title, cwd, hyperlinks, bell) produced by
     * the parser during the most recent `feed` calls. Returns a JS
     * array of tagged objects: `{ type: "TitleChanged", value: "..." }`
     * etc. Caller routes each event to the relevant Svelte store
     * (paneTitleStore, paneCwdStore, ...).
     * @returns {any[]}
     */
    takePendingEvents() {
        const ret = wasm.terminalkernel_takePendingEvents(this.__wbg_ptr);
        var v1 = getArrayJsValueFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Drain query-response bytes (DSR `\x1b[r;cR`, DA `\x1b[?...c`) the
     * parser produced during the most recent `feed` calls. Caller MUST
     * forward these bytes to the PTY as if they were keystrokes; without
     * this round-trip, PowerShell + ConPTY render the prompt at a stale
     * cursor row after a child process exits (e.g. Ctrl+C out of a TUI),
     * overwriting whatever was on screen.
     * @returns {Uint8Array}
     */
    takePendingResponse() {
        const ret = wasm.terminalkernel_takePendingResponse(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
}
if (Symbol.dispose) TerminalKernel.prototype[Symbol.dispose] = TerminalKernel.prototype.free;

export function _init() {
    wasm._init();
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Error_3639a60ed15f87e7: function(arg0, arg1) {
            const ret = Error(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_Window_b0c275b50676d397: function(arg0) {
            const ret = arg0.Window;
            return ret;
        },
        __wbg_WorkerGlobalScope_7a1f78d9f7542cfa: function(arg0) {
            const ret = arg0.WorkerGlobalScope;
            return ret;
        },
        __wbg___wbindgen_debug_string_07cb72cfcc952e2b: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_function_2f0fd7ceb86e64c5: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_null_066086be3abe9bb3: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_object_5b22ff2418063a9c: function(arg0) {
            const val = arg0;
            const ret = typeof(val) === 'object' && val !== null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_244a92c34d3b6ec0: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_string_get_965592073e5d848c: function(arg0, arg1) {
            const obj = arg1;
            const ret = typeof(obj) === 'string' ? obj : undefined;
            var ptr1 = isLikeNone(ret) ? 0 : passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            var len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_throw_9c75d47bf9e7731e: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_158e43e869788cdc: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_beginComputePass_0fb772608bf84f44: function(arg0, arg1) {
            const ret = arg0.beginComputePass(arg1);
            return ret;
        },
        __wbg_beginRenderPass_c662486e5caabb09: function(arg0, arg1) {
            const ret = arg0.beginRenderPass(arg1);
            return ret;
        },
        __wbg_buffer_9ee17426fe5a5d65: function(arg0) {
            const ret = arg0.buffer;
            return ret;
        },
        __wbg_call_a41d6421b30a32c5: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_clearBuffer_e063e34f4a181e05: function(arg0, arg1, arg2, arg3) {
            arg0.clearBuffer(arg1, arg2, arg3);
        },
        __wbg_clearBuffer_f330030ddc7767fc: function(arg0, arg1, arg2) {
            arg0.clearBuffer(arg1, arg2);
        },
        __wbg_clearRect_ff21a25636146bdd: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.clearRect(arg1, arg2, arg3, arg4);
        },
        __wbg_configure_c71c9f57ca3edf98: function(arg0, arg1) {
            arg0.configure(arg1);
        },
        __wbg_copyBufferToBuffer_910ae8c201bdff01: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.copyBufferToBuffer(arg1, arg2, arg3, arg4, arg5);
        },
        __wbg_copyBufferToTexture_8c287708aff282a4: function(arg0, arg1, arg2, arg3) {
            arg0.copyBufferToTexture(arg1, arg2, arg3);
        },
        __wbg_copyExternalImageToTexture_540fcadea7d8323f: function(arg0, arg1, arg2, arg3) {
            arg0.copyExternalImageToTexture(arg1, arg2, arg3);
        },
        __wbg_copyTextureToBuffer_76965133f36672a4: function(arg0, arg1, arg2, arg3) {
            arg0.copyTextureToBuffer(arg1, arg2, arg3);
        },
        __wbg_copyTextureToTexture_04331d5254bea8fc: function(arg0, arg1, arg2, arg3) {
            arg0.copyTextureToTexture(arg1, arg2, arg3);
        },
        __wbg_createBindGroupLayout_fe258aa231f602a1: function(arg0, arg1) {
            const ret = arg0.createBindGroupLayout(arg1);
            return ret;
        },
        __wbg_createBindGroup_783178b92eca4f94: function(arg0, arg1) {
            const ret = arg0.createBindGroup(arg1);
            return ret;
        },
        __wbg_createBuffer_05c143bc69af7de1: function(arg0, arg1) {
            const ret = arg0.createBuffer(arg1);
            return ret;
        },
        __wbg_createCommandEncoder_eeac00d01e7c7215: function(arg0, arg1) {
            const ret = arg0.createCommandEncoder(arg1);
            return ret;
        },
        __wbg_createComputePipeline_70cb69a35311bb5a: function(arg0, arg1) {
            const ret = arg0.createComputePipeline(arg1);
            return ret;
        },
        __wbg_createPipelineLayout_3195019c488e9d1f: function(arg0, arg1) {
            const ret = arg0.createPipelineLayout(arg1);
            return ret;
        },
        __wbg_createQuerySet_a8afd88335f1ae22: function(arg0, arg1) {
            const ret = arg0.createQuerySet(arg1);
            return ret;
        },
        __wbg_createRenderBundleEncoder_0ae4be9a26b4f4aa: function(arg0, arg1) {
            const ret = arg0.createRenderBundleEncoder(arg1);
            return ret;
        },
        __wbg_createRenderPipeline_430c946fe289280f: function(arg0, arg1) {
            const ret = arg0.createRenderPipeline(arg1);
            return ret;
        },
        __wbg_createSampler_59ee59f9ce9c89e6: function(arg0, arg1) {
            const ret = arg0.createSampler(arg1);
            return ret;
        },
        __wbg_createShaderModule_cb92dd515bc68e5a: function(arg0, arg1) {
            const ret = arg0.createShaderModule(arg1);
            return ret;
        },
        __wbg_createTexture_ae83ede28133180f: function(arg0, arg1) {
            const ret = arg0.createTexture(arg1);
            return ret;
        },
        __wbg_createView_c0fb516125a12571: function(arg0, arg1) {
            const ret = arg0.createView(arg1);
            return ret;
        },
        __wbg_data_a8804167f4745f97: function(arg0, arg1) {
            const ret = arg1.data;
            const ptr1 = passArray8ToWasm0(ret, wasm.__wbindgen_malloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_destroy_d1537bee2b5a7849: function(arg0) {
            arg0.destroy();
        },
        __wbg_destroy_d28e196e9dbc3b27: function(arg0) {
            arg0.destroy();
        },
        __wbg_destroy_ddd5bee0b4b02f49: function(arg0) {
            arg0.destroy();
        },
        __wbg_dispatchWorkgroupsIndirect_e915df9199133ac5: function(arg0, arg1, arg2) {
            arg0.dispatchWorkgroupsIndirect(arg1, arg2);
        },
        __wbg_dispatchWorkgroups_0d71a3ed9fcaee9f: function(arg0, arg1, arg2, arg3) {
            arg0.dispatchWorkgroups(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0);
        },
        __wbg_document_69bb6a2f7927d532: function(arg0) {
            const ret = arg0.document;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_drawIndexedIndirect_0954a720a9b13248: function(arg0, arg1, arg2) {
            arg0.drawIndexedIndirect(arg1, arg2);
        },
        __wbg_drawIndexedIndirect_7882fca885de47ce: function(arg0, arg1, arg2) {
            arg0.drawIndexedIndirect(arg1, arg2);
        },
        __wbg_drawIndexed_280977bb1d3baf3d: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.drawIndexed(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4, arg5 >>> 0);
        },
        __wbg_drawIndexed_9a150a51a8427045: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.drawIndexed(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4, arg5 >>> 0);
        },
        __wbg_drawIndirect_b393626eb70ae7fb: function(arg0, arg1, arg2) {
            arg0.drawIndirect(arg1, arg2);
        },
        __wbg_drawIndirect_c6c299eb2ddf8fd7: function(arg0, arg1, arg2) {
            arg0.drawIndirect(arg1, arg2);
        },
        __wbg_draw_26370233bc7d2e7e: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.draw(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4 >>> 0);
        },
        __wbg_draw_83285c3877561ec1: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.draw(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4 >>> 0);
        },
        __wbg_end_420d93a37f764933: function(arg0) {
            arg0.end();
        },
        __wbg_end_97a4259681c42d8d: function(arg0) {
            arg0.end();
        },
        __wbg_error_a6fa202b58aa1cd3: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_error_d9a855c84f9b4e4c: function(arg0) {
            const ret = arg0.error;
            return ret;
        },
        __wbg_executeBundles_452872ac4afbbf92: function(arg0, arg1) {
            arg0.executeBundles(arg1);
        },
        __wbg_features_15adc13e5b141301: function(arg0) {
            const ret = arg0.features;
            return ret;
        },
        __wbg_features_f6c1f470639a88e2: function(arg0) {
            const ret = arg0.features;
            return ret;
        },
        __wbg_fillRect_9219f775d7e8e73e: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        },
        __wbg_fillText_6d1a4715d8d662d0: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_fillText_9fbea3af94326c74: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_finish_23cbd862d4229ec3: function(arg0, arg1) {
            const ret = arg0.finish(arg1);
            return ret;
        },
        __wbg_finish_52172eac54898d16: function(arg0) {
            const ret = arg0.finish();
            return ret;
        },
        __wbg_finish_94bc184b535e2a90: function(arg0, arg1) {
            const ret = arg0.finish(arg1);
            return ret;
        },
        __wbg_finish_dad34d81d4500e85: function(arg0) {
            const ret = arg0.finish();
            return ret;
        },
        __wbg_fontBoundingBoxAscent_affa96c213c0488c: function(arg0) {
            const ret = arg0.fontBoundingBoxAscent;
            return ret;
        },
        __wbg_fontBoundingBoxDescent_a9a41cad7bb276a8: function(arg0) {
            const ret = arg0.fontBoundingBoxDescent;
            return ret;
        },
        __wbg_getBindGroupLayout_6d503a1fba524ee6: function(arg0, arg1) {
            const ret = arg0.getBindGroupLayout(arg1 >>> 0);
            return ret;
        },
        __wbg_getBindGroupLayout_bc897888c0670dbe: function(arg0, arg1) {
            const ret = arg0.getBindGroupLayout(arg1 >>> 0);
            return ret;
        },
        __wbg_getCompilationInfo_469a33f449854be7: function(arg0) {
            const ret = arg0.getCompilationInfo();
            return ret;
        },
        __wbg_getContext_5d4707454276e47f: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getContext_f17252002286474d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getCurrentTexture_e27b103ea7a3ce3c: function(arg0) {
            const ret = arg0.getCurrentTexture();
            return ret;
        },
        __wbg_getImageData_d83fb05650ce22a1: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            const ret = arg0.getImageData(arg1, arg2, arg3, arg4);
            return ret;
        }, arguments); },
        __wbg_getMappedRange_4f36f39e059a63c6: function(arg0, arg1, arg2) {
            const ret = arg0.getMappedRange(arg1, arg2);
            return ret;
        },
        __wbg_getPreferredCanvasFormat_13332df72e63723a: function(arg0) {
            const ret = arg0.getPreferredCanvasFormat();
            return (__wbindgen_enum_GpuTextureFormat.indexOf(ret) + 1 || 96) - 1;
        },
        __wbg_get_41476db20fef99a8: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_652f640b3b0b6e3e: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return ret;
        },
        __wbg_get_a6a7ef761f5bd232: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_gpu_c773d7932dc745d7: function(arg0) {
            const ret = arg0.gpu;
            return ret;
        },
        __wbg_has_b54bd7b6e9da11c7: function(arg0, arg1, arg2) {
            const ret = arg0.has(getStringFromWasm0(arg1, arg2));
            return ret;
        },
        __wbg_instanceof_CanvasRenderingContext2d_b433938013de3a1e: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuAdapter_0731153d2b08720b: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUAdapter;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuCanvasContext_d14121c7bd72fcef: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUCanvasContext;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuDeviceLostInfo_a3677ebb8241d800: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUDeviceLostInfo;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuOutOfMemoryError_391d9a08edbfa04b: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUOutOfMemoryError;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuValidationError_f4d803c383da3c92: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUValidationError;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Object_af9351f8f1c6f0c4: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Object;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_OffscreenCanvasRenderingContext2d_23f7ce578afab75f: function(arg0) {
            let result;
            try {
                result = arg0 instanceof OffscreenCanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_4153c1818a1c0c0b: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_label_614ef5e608843844: function(arg0, arg1) {
            const ret = arg1.label;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_length_0a6ce016dc1460b0: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_ba3c032602efe310: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_length_d34bf7d191aa0640: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_limits_2bfe39eb5f0b5a01: function(arg0) {
            const ret = arg0.limits;
            return ret;
        },
        __wbg_limits_77193ad8b62f8502: function(arg0) {
            const ret = arg0.limits;
            return ret;
        },
        __wbg_lineNum_95b780ade9fb4ba3: function(arg0) {
            const ret = arg0.lineNum;
            return ret;
        },
        __wbg_lost_21e9db8a9502a0ca: function(arg0) {
            const ret = arg0.lost;
            return ret;
        },
        __wbg_mapAsync_f4fc38ac51855b15: function(arg0, arg1, arg2, arg3) {
            const ret = arg0.mapAsync(arg1 >>> 0, arg2, arg3);
            return ret;
        },
        __wbg_maxBindGroups_cc0c1b6031ac310e: function(arg0) {
            const ret = arg0.maxBindGroups;
            return ret;
        },
        __wbg_maxBindingsPerBindGroup_d950de0c90e382e0: function(arg0) {
            const ret = arg0.maxBindingsPerBindGroup;
            return ret;
        },
        __wbg_maxBufferSize_01e5e024c304478a: function(arg0) {
            const ret = arg0.maxBufferSize;
            return ret;
        },
        __wbg_maxColorAttachmentBytesPerSample_91fc5eb9155186fd: function(arg0) {
            const ret = arg0.maxColorAttachmentBytesPerSample;
            return ret;
        },
        __wbg_maxColorAttachments_69f3bac8513cd2ce: function(arg0) {
            const ret = arg0.maxColorAttachments;
            return ret;
        },
        __wbg_maxComputeInvocationsPerWorkgroup_5d8e1f9e65b5443c: function(arg0) {
            const ret = arg0.maxComputeInvocationsPerWorkgroup;
            return ret;
        },
        __wbg_maxComputeWorkgroupSizeX_e8c75fa90e0b00b7: function(arg0) {
            const ret = arg0.maxComputeWorkgroupSizeX;
            return ret;
        },
        __wbg_maxComputeWorkgroupSizeY_72bce71ec7fa9330: function(arg0) {
            const ret = arg0.maxComputeWorkgroupSizeY;
            return ret;
        },
        __wbg_maxComputeWorkgroupSizeZ_8c7050ac47c80e42: function(arg0) {
            const ret = arg0.maxComputeWorkgroupSizeZ;
            return ret;
        },
        __wbg_maxComputeWorkgroupStorageSize_b789a39c5a0fd04a: function(arg0) {
            const ret = arg0.maxComputeWorkgroupStorageSize;
            return ret;
        },
        __wbg_maxComputeWorkgroupsPerDimension_a02a7f66f7c68b9c: function(arg0) {
            const ret = arg0.maxComputeWorkgroupsPerDimension;
            return ret;
        },
        __wbg_maxDynamicStorageBuffersPerPipelineLayout_90d4eb33665de8d1: function(arg0) {
            const ret = arg0.maxDynamicStorageBuffersPerPipelineLayout;
            return ret;
        },
        __wbg_maxDynamicUniformBuffersPerPipelineLayout_835864d8a793cc95: function(arg0) {
            const ret = arg0.maxDynamicUniformBuffersPerPipelineLayout;
            return ret;
        },
        __wbg_maxSampledTexturesPerShaderStage_f1fdaca8bd10047f: function(arg0) {
            const ret = arg0.maxSampledTexturesPerShaderStage;
            return ret;
        },
        __wbg_maxSamplersPerShaderStage_a0126ce660fc903a: function(arg0) {
            const ret = arg0.maxSamplersPerShaderStage;
            return ret;
        },
        __wbg_maxStorageBufferBindingSize_9ed12d54b564312c: function(arg0) {
            const ret = arg0.maxStorageBufferBindingSize;
            return ret;
        },
        __wbg_maxStorageBuffersPerShaderStage_7db5a7548c1199e6: function(arg0) {
            const ret = arg0.maxStorageBuffersPerShaderStage;
            return ret;
        },
        __wbg_maxStorageTexturesPerShaderStage_3df697d427690d26: function(arg0) {
            const ret = arg0.maxStorageTexturesPerShaderStage;
            return ret;
        },
        __wbg_maxTextureArrayLayers_759d0ac67e0a7d26: function(arg0) {
            const ret = arg0.maxTextureArrayLayers;
            return ret;
        },
        __wbg_maxTextureDimension1D_4bfdff8638ada7c1: function(arg0) {
            const ret = arg0.maxTextureDimension1D;
            return ret;
        },
        __wbg_maxTextureDimension2D_ea0c9c4d0b239666: function(arg0) {
            const ret = arg0.maxTextureDimension2D;
            return ret;
        },
        __wbg_maxTextureDimension3D_e76f3604806f47be: function(arg0) {
            const ret = arg0.maxTextureDimension3D;
            return ret;
        },
        __wbg_maxUniformBufferBindingSize_591ad000ffe10aad: function(arg0) {
            const ret = arg0.maxUniformBufferBindingSize;
            return ret;
        },
        __wbg_maxUniformBuffersPerShaderStage_6e5696dba506ca6c: function(arg0) {
            const ret = arg0.maxUniformBuffersPerShaderStage;
            return ret;
        },
        __wbg_maxVertexAttributes_fef434a4cf2ba188: function(arg0) {
            const ret = arg0.maxVertexAttributes;
            return ret;
        },
        __wbg_maxVertexBufferArrayStride_de60c38ec574b423: function(arg0) {
            const ret = arg0.maxVertexBufferArrayStride;
            return ret;
        },
        __wbg_maxVertexBuffers_d1a4a2fba06ae7d6: function(arg0) {
            const ret = arg0.maxVertexBuffers;
            return ret;
        },
        __wbg_measureText_22ac8156da00630f: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.measureText(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_measureText_29ad84bd45ab9fce: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.measureText(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_message_8fd23df93c50075a: function(arg0, arg1) {
            const ret = arg1.message;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_message_b00edacf4a520b03: function(arg0, arg1) {
            const ret = arg1.message;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_message_d2eedafa0bd554a6: function(arg0, arg1) {
            const ret = arg1.message;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_messages_1df11461d071c92c: function(arg0) {
            const ret = arg0.messages;
            return ret;
        },
        __wbg_minStorageBufferOffsetAlignment_49f6b6baa1d34111: function(arg0) {
            const ret = arg0.minStorageBufferOffsetAlignment;
            return ret;
        },
        __wbg_minUniformBufferOffsetAlignment_39ec7837ddc9ee2c: function(arg0) {
            const ret = arg0.minUniformBufferOffsetAlignment;
            return ret;
        },
        __wbg_navigator_83daf29f5beb4064: function(arg0) {
            const ret = arg0.navigator;
            return ret;
        },
        __wbg_navigator_f3468c6dc9006b7c: function(arg0) {
            const ret = arg0.navigator;
            return ret;
        },
        __wbg_new_227d7c05414eb861: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_2fad8ca02fd00684: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_3baa8d9866155c79: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_new_e6faaf6e832d3086: function() { return handleError(function (arg0, arg1) {
            const ret = new OffscreenCanvas(arg0 >>> 0, arg1 >>> 0);
            return ret;
        }, arguments); },
        __wbg_new_from_slice_5a173c243af2e823: function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_typed_1137602701dc87d4: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return wasm_bindgen__convert__closures_____invoke__h386c8d8a4d76669f(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return ret;
            } finally {
                state0.a = 0;
            }
        },
        __wbg_new_with_byte_offset_and_length_643e5e9e2fb6b1ad: function(arg0, arg1, arg2) {
            const ret = new Uint8Array(arg0, arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_now_4f457f10f864aec5: function() {
            const ret = Date.now();
            return ret;
        },
        __wbg_offset_78dcfcd1f3ebc4ea: function(arg0) {
            const ret = arg0.offset;
            return ret;
        },
        __wbg_popErrorScope_efb23ea2dcc3b587: function(arg0) {
            const ret = arg0.popErrorScope();
            return ret;
        },
        __wbg_prototypesetcall_fd4050e806e1d519: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        },
        __wbg_pushErrorScope_9a7570b7a9f67657: function(arg0, arg1) {
            arg0.pushErrorScope(__wbindgen_enum_GpuErrorFilter[arg1]);
        },
        __wbg_push_60a5366c0bb22a7d: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_querySelectorAll_a9cd19a1a678838e: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.querySelectorAll(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_queueMicrotask_40ac6ffc2848ba77: function(arg0) {
            queueMicrotask(arg0);
        },
        __wbg_queueMicrotask_74d092439f6494c1: function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        },
        __wbg_queue_9595c5175ef399b9: function(arg0) {
            const ret = arg0.queue;
            return ret;
        },
        __wbg_reason_f9df4a653cfa764b: function(arg0) {
            const ret = arg0.reason;
            return (__wbindgen_enum_GpuDeviceLostReason.indexOf(ret) + 1 || 3) - 1;
        },
        __wbg_renderhandle_new: function(arg0) {
            const ret = RenderHandle.__wrap(arg0);
            return ret;
        },
        __wbg_requestAdapter_592f04f645dfaf68: function(arg0, arg1) {
            const ret = arg0.requestAdapter(arg1);
            return ret;
        },
        __wbg_requestDevice_52bb2980e6280ebc: function(arg0, arg1) {
            const ret = arg0.requestDevice(arg1);
            return ret;
        },
        __wbg_resolveQuerySet_b316102e1d152b52: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.resolveQuerySet(arg1, arg2 >>> 0, arg3 >>> 0, arg4, arg5 >>> 0);
        },
        __wbg_resolve_9feb5d906ca62419: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_restore_5bff5e1cc672e792: function(arg0) {
            arg0.restore();
        },
        __wbg_save_512a4b0787b6682e: function(arg0) {
            arg0.save();
        },
        __wbg_setBindGroup_0fb411b7d1ec4966: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setBindGroup(arg1 >>> 0, arg2, getArrayU32FromWasm0(arg3, arg4), arg5, arg6 >>> 0);
        },
        __wbg_setBindGroup_1c6bfc705c95f81f: function(arg0, arg1, arg2) {
            arg0.setBindGroup(arg1 >>> 0, arg2);
        },
        __wbg_setBindGroup_2ec8db65419ec50c: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setBindGroup(arg1 >>> 0, arg2, getArrayU32FromWasm0(arg3, arg4), arg5, arg6 >>> 0);
        },
        __wbg_setBindGroup_3afbefd496741277: function(arg0, arg1, arg2) {
            arg0.setBindGroup(arg1 >>> 0, arg2);
        },
        __wbg_setBindGroup_4ac51c0e16178380: function(arg0, arg1, arg2) {
            arg0.setBindGroup(arg1 >>> 0, arg2);
        },
        __wbg_setBindGroup_c2fbfec522cc7572: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setBindGroup(arg1 >>> 0, arg2, getArrayU32FromWasm0(arg3, arg4), arg5, arg6 >>> 0);
        },
        __wbg_setBlendConstant_00bed453ac51c91b: function(arg0, arg1) {
            arg0.setBlendConstant(arg1);
        },
        __wbg_setIndexBuffer_42017bb879ab062b: function(arg0, arg1, arg2, arg3) {
            arg0.setIndexBuffer(arg1, __wbindgen_enum_GpuIndexFormat[arg2], arg3);
        },
        __wbg_setIndexBuffer_4876c05f77106bb6: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.setIndexBuffer(arg1, __wbindgen_enum_GpuIndexFormat[arg2], arg3, arg4);
        },
        __wbg_setIndexBuffer_8c79ee0b0b6460fa: function(arg0, arg1, arg2, arg3) {
            arg0.setIndexBuffer(arg1, __wbindgen_enum_GpuIndexFormat[arg2], arg3);
        },
        __wbg_setIndexBuffer_e10a7cf5d063fdab: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.setIndexBuffer(arg1, __wbindgen_enum_GpuIndexFormat[arg2], arg3, arg4);
        },
        __wbg_setPipeline_5c5a949bf12f8a5f: function(arg0, arg1) {
            arg0.setPipeline(arg1);
        },
        __wbg_setPipeline_c4793bebd98b8e56: function(arg0, arg1) {
            arg0.setPipeline(arg1);
        },
        __wbg_setPipeline_ce7a683c2c94919d: function(arg0, arg1) {
            arg0.setPipeline(arg1);
        },
        __wbg_setProperty_d6673329a267577b: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.setProperty(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_setScissorRect_cf24179de05b8393: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.setScissorRect(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4 >>> 0);
        },
        __wbg_setStencilReference_7a98f054e2f31f54: function(arg0, arg1) {
            arg0.setStencilReference(arg1 >>> 0);
        },
        __wbg_setTransform_f25014a0bb3cb050: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setTransform(arg1, arg2, arg3, arg4, arg5, arg6);
        }, arguments); },
        __wbg_setVertexBuffer_06dd033f8e75af24: function(arg0, arg1, arg2, arg3) {
            arg0.setVertexBuffer(arg1 >>> 0, arg2, arg3);
        },
        __wbg_setVertexBuffer_c973cd35605098e4: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.setVertexBuffer(arg1 >>> 0, arg2, arg3, arg4);
        },
        __wbg_setVertexBuffer_e80315ecd1774568: function(arg0, arg1, arg2, arg3) {
            arg0.setVertexBuffer(arg1 >>> 0, arg2, arg3);
        },
        __wbg_setVertexBuffer_ef41a6013dba1352: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.setVertexBuffer(arg1 >>> 0, arg2, arg3, arg4);
        },
        __wbg_setViewport_75637b1c9a301986: function(arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setViewport(arg1, arg2, arg3, arg4, arg5, arg6);
        },
        __wbg_set_0574e274b35c5501: function(arg0, arg1, arg2) {
            arg0.set(arg1, arg2 >>> 0);
        },
        __wbg_set_5337f8ac82364a3f: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_set_6be42768c690e380: function(arg0, arg1, arg2) {
            arg0[arg1] = arg2;
        },
        __wbg_set_fillStyle_a3656c7c5d4ad803: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_fillStyle_f2dd6e6182484100: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_38efcddbe831b07e: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_5b1b8c76449f5864: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_height_77937c921db92223: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_height_89a4ecd0f9cc3dfa: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_onuncapturederror_5c20c4125b115c22: function(arg0, arg1) {
            arg0.onuncapturederror = arg1;
        },
        __wbg_set_textBaseline_68cf9979f06f859b: function(arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_textBaseline_bb8350220310ce4c: function(arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_width_d2ec5d6689655fa9: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_set_width_da52058a27694474: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_size_b5c1b72884cb3fa5: function(arg0) {
            const ret = arg0.size;
            return ret;
        },
        __wbg_stack_3b0d974bbf31e44f: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_THIS_1c7f1bd6c6941fdb: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_e039bc914f83e74e: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_8bf8c48c28420ad5: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_6aeee9b51652ee0f: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_style_ad734f3851a343fb: function(arg0) {
            const ret = arg0.style;
            return ret;
        },
        __wbg_submit_6ffa2ed48b3eaecf: function(arg0, arg1) {
            arg0.submit(arg1);
        },
        __wbg_then_20a157d939b514f5: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbg_then_4d0dc09d0334f8a0: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbg_then_5ef9b762bc91555c: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_then_7ebd9021bf33072f: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_type_ba6bfed8f5073b9e: function(arg0) {
            const ret = arg0.type;
            return (__wbindgen_enum_GpuCompilationMessageType.indexOf(ret) + 1 || 4) - 1;
        },
        __wbg_unmap_d610a495d70ebb5e: function(arg0) {
            arg0.unmap();
        },
        __wbg_usage_92ae9f7605bb82c1: function(arg0) {
            const ret = arg0.usage;
            return ret;
        },
        __wbg_valueOf_67fbc181e7e6159f: function(arg0) {
            const ret = arg0.valueOf();
            return ret;
        },
        __wbg_width_7c985ca9f3cc024f: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbg_writeBuffer_28f398e6955ad305: function(arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.writeBuffer(arg1, arg2, arg3, arg4, arg5);
        },
        __wbg_writeTexture_4eafae0e29b3eac0: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.writeTexture(arg1, arg2, arg3, arg4);
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 270, ret: Result(Unit), inner_ret: Some(Result(Unit)) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h5b8f9f9118d17a3b);
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [Externref], shim_idx: 51, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42);
            return ret;
        },
        __wbindgen_cast_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("GPUUncapturedErrorEvent")], shim_idx: 51, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42_2);
            return ret;
        },
        __wbindgen_cast_0000000000000004: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_cast_0000000000000005: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U8)) -> NamedExternref("Uint8Array")`.
            const ret = getArrayU8FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000006: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000007: function(arg0) {
            // Cast intrinsic for `U64 -> Externref`.
            const ret = BigInt.asUintN(64, arg0);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./ridge_term_bg.js": import0,
    };
}

function wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42(arg0, arg1, arg2) {
    wasm.wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42(arg0, arg1, arg2);
}

function wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42_2(arg0, arg1, arg2) {
    wasm.wasm_bindgen__convert__closures_____invoke__h15d8de1645cc0e42_2(arg0, arg1, arg2);
}

function wasm_bindgen__convert__closures_____invoke__h5b8f9f9118d17a3b(arg0, arg1, arg2) {
    const ret = wasm.wasm_bindgen__convert__closures_____invoke__h5b8f9f9118d17a3b(arg0, arg1, arg2);
    if (ret[1]) {
        throw takeFromExternrefTable0(ret[0]);
    }
}

function wasm_bindgen__convert__closures_____invoke__h386c8d8a4d76669f(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen__convert__closures_____invoke__h386c8d8a4d76669f(arg0, arg1, arg2, arg3);
}


const __wbindgen_enum_GpuCompilationMessageType = ["error", "warning", "info"];


const __wbindgen_enum_GpuDeviceLostReason = ["unknown", "destroyed"];


const __wbindgen_enum_GpuErrorFilter = ["validation", "out-of-memory", "internal"];


const __wbindgen_enum_GpuIndexFormat = ["uint16", "uint32"];


const __wbindgen_enum_GpuTextureFormat = ["r8unorm", "r8snorm", "r8uint", "r8sint", "r16uint", "r16sint", "r16float", "rg8unorm", "rg8snorm", "rg8uint", "rg8sint", "r32uint", "r32sint", "r32float", "rg16uint", "rg16sint", "rg16float", "rgba8unorm", "rgba8unorm-srgb", "rgba8snorm", "rgba8uint", "rgba8sint", "bgra8unorm", "bgra8unorm-srgb", "rgb9e5ufloat", "rgb10a2uint", "rgb10a2unorm", "rg11b10ufloat", "rg32uint", "rg32sint", "rg32float", "rgba16uint", "rgba16sint", "rgba16float", "rgba32uint", "rgba32sint", "rgba32float", "stencil8", "depth16unorm", "depth24plus", "depth24plus-stencil8", "depth32float", "depth32float-stencil8", "bc1-rgba-unorm", "bc1-rgba-unorm-srgb", "bc2-rgba-unorm", "bc2-rgba-unorm-srgb", "bc3-rgba-unorm", "bc3-rgba-unorm-srgb", "bc4-r-unorm", "bc4-r-snorm", "bc5-rg-unorm", "bc5-rg-snorm", "bc6h-rgb-ufloat", "bc6h-rgb-float", "bc7-rgba-unorm", "bc7-rgba-unorm-srgb", "etc2-rgb8unorm", "etc2-rgb8unorm-srgb", "etc2-rgb8a1unorm", "etc2-rgb8a1unorm-srgb", "etc2-rgba8unorm", "etc2-rgba8unorm-srgb", "eac-r11unorm", "eac-r11snorm", "eac-rg11unorm", "eac-rg11snorm", "astc-4x4-unorm", "astc-4x4-unorm-srgb", "astc-5x4-unorm", "astc-5x4-unorm-srgb", "astc-5x5-unorm", "astc-5x5-unorm-srgb", "astc-6x5-unorm", "astc-6x5-unorm-srgb", "astc-6x6-unorm", "astc-6x6-unorm-srgb", "astc-8x5-unorm", "astc-8x5-unorm-srgb", "astc-8x6-unorm", "astc-8x6-unorm-srgb", "astc-8x8-unorm", "astc-8x8-unorm-srgb", "astc-10x5-unorm", "astc-10x5-unorm-srgb", "astc-10x6-unorm", "astc-10x6-unorm-srgb", "astc-10x8-unorm", "astc-10x8-unorm-srgb", "astc-10x10-unorm", "astc-10x10-unorm-srgb", "astc-12x10-unorm", "astc-12x10-unorm-srgb", "astc-12x12-unorm", "astc-12x12-unorm-srgb"];
const TerminalKernelFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_terminalkernel_free(ptr, 1));
const RenderHandleFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_renderhandle_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => wasm.__wbindgen_destroy_closure(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayJsValueFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    const mem = getDataViewMemory0();
    const result = [];
    for (let i = ptr; i < ptr + 4 * len; i += 4) {
        result.push(wasm.__wbindgen_externrefs.get(mem.getUint32(i, true)));
    }
    wasm.__externref_drop_slice(ptr, len);
    return result;
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('ridge_term_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
