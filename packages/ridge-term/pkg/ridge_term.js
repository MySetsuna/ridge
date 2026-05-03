/* @ts-self-types="./ridge_term.d.ts" */

//#region exports

export class RenderHandle {
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        wasm.renderhandle_invalidateAll(this.__wbg_ptr);
    }
    /**
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertClass(kernel, TerminalKernel);
        if (kernel.__wbg_ptr === 0) {
            throw new Error('Attempt to use a moved value');
        }
        const ret = wasm.renderhandle_render(this.__wbg_ptr, kernel.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} width_css
     * @param {number} height_css
     * @param {number} dpr
     */
    resize(width_css, height_css, dpr) {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(width_css);
        _assertNum(height_css);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertBoolean(focused);
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
    clearSelection() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        wasm.terminalkernel_clearSelection(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    cols() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_cols(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Cursor column in viewport coordinates (0-based).
     * @returns {number}
     */
    cursorCol() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_cursorRow(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {any[]}
     */
    dumpVisibleText() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ptr0 = passStringToWasm0(key, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        _assertBoolean(ctrl);
        _assertBoolean(alt);
        _assertBoolean(shift);
        _assertBoolean(meta);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
            if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
            _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(row);
        _assertNum(col);
        const ret = wasm.terminalkernel_hyperlinkAt(this.__wbg_ptr, row, col);
        return ret;
    }
    /**
     * @returns {boolean}
     */
    isAltScreen() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_isAltScreen(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {boolean}
     */
    isAppCursorKeys() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_isAppCursorKeys(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {boolean}
     */
    isBracketedPaste() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_isBracketedPaste(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {boolean}
     */
    isCursorVisible() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_isFocusReporting(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_isSyncOutput(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} rows
     * @param {number} cols
     * @param {number} scrollback
     */
    constructor(rows, cols, scrollback) {
        _assertNum(rows);
        _assertNum(cols);
        _assertNum(scrollback);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.terminalkernel_prependScrollback(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @param {number} rows
     * @param {number} cols
     */
    resize(rows, cols) {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(rows);
        _assertNum(cols);
        wasm.terminalkernel_resize(this.__wbg_ptr, rows, cols);
    }
    /**
     * @returns {number}
     */
    rows() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_rows(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} n
     */
    scrollDown(n) {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(n);
        wasm.terminalkernel_scrollDown(this.__wbg_ptr, n);
    }
    /**
     * @returns {number}
     */
    scrollOffset() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_scrollOffset(this.__wbg_ptr);
        return ret >>> 0;
    }
    scrollToBottom() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        wasm.terminalkernel_scrollToBottom(this.__wbg_ptr);
    }
    /**
     * @param {number} n
     */
    scrollUp(n) {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(n);
        wasm.terminalkernel_scrollUp(this.__wbg_ptr, n);
    }
    /**
     * @returns {number}
     */
    scrollbackLen() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_scrollbackLen(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns the active match index, or `usize::MAX` when no active match.
     * @returns {number}
     */
    searchActiveIndex() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_searchActiveIndex(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Clear search state and the highlight selection.
     */
    searchClear() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        wasm.terminalkernel_searchClear(this.__wbg_ptr);
    }
    /**
     * @returns {number}
     */
    searchMatchCount() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_searchMatchCount(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Step to the next match (wraps). Returns the new active index, or
     * `usize::MAX` if there are no matches.
     * @returns {number}
     */
    searchNext() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ret = wasm.terminalkernel_searchNext(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Step to the previous match (wraps).
     * @returns {number}
     */
    searchPrev() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        const ptr0 = passStringToWasm0(query, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        _assertBoolean(case_sensitive);
        const ret = wasm.terminalkernel_searchSetQuery(this.__wbg_ptr, ptr0, len0, case_sensitive);
        return ret >>> 0;
    }
    selectAll() {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        wasm.terminalkernel_selectAll(this.__wbg_ptr);
    }
    /**
     * Triple-click line selection.
     * @param {number} row
     */
    selectLineAt(row) {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(row);
        wasm.terminalkernel_selectLineAt(this.__wbg_ptr, row);
    }
    /**
     * Double-click word selection. Selects the word at the given cell
     * coordinate; clears selection when the cell is whitespace/empty.
     * @param {number} row
     * @param {number} col
     */
    selectWordAt(row, col) {
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(row);
        _assertNum(col);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
        _assertNum(start_row);
        _assertNum(start_col);
        _assertNum(end_row);
        _assertNum(end_col);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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
        if (this.__wbg_ptr == 0) throw new Error('Attempt to use a moved value');
        _assertNum(this.__wbg_ptr);
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

//#endregion

//#region wasm imports
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_debug_string_07cb72cfcc952e2b: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
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
        __wbg_error_a6fa202b58aa1cd3: function() { return logError(function (arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        }, arguments); },
        __wbg_fillRect_9219f775d7e8e73e: function() { return logError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        }, arguments); },
        __wbg_fillText_9fbea3af94326c74: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_fontBoundingBoxAscent_affa96c213c0488c: function() { return logError(function (arg0) {
            const ret = arg0.fontBoundingBoxAscent;
            return ret;
        }, arguments); },
        __wbg_fontBoundingBoxDescent_a9a41cad7bb276a8: function() { return logError(function (arg0) {
            const ret = arg0.fontBoundingBoxDescent;
            return ret;
        }, arguments); },
        __wbg_getContext_f17252002286474d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_get_41476db20fef99a8: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_instanceof_CanvasRenderingContext2d_b433938013de3a1e: function() { return logError(function (arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            _assertBoolean(ret);
            return ret;
        }, arguments); },
        __wbg_measureText_22ac8156da00630f: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.measureText(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_new_227d7c05414eb861: function() { return logError(function () {
            const ret = new Error();
            return ret;
        }, arguments); },
        __wbg_new_2fad8ca02fd00684: function() { return logError(function () {
            const ret = new Object();
            return ret;
        }, arguments); },
        __wbg_now_4f457f10f864aec5: function() { return logError(function () {
            const ret = Date.now();
            return ret;
        }, arguments); },
        __wbg_restore_5bff5e1cc672e792: function() { return logError(function (arg0) {
            arg0.restore();
        }, arguments); },
        __wbg_save_512a4b0787b6682e: function() { return logError(function (arg0) {
            arg0.save();
        }, arguments); },
        __wbg_setProperty_d6673329a267577b: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.setProperty(getStringFromWasm0(arg1, arg2), getStringFromWasm0(arg3, arg4));
        }, arguments); },
        __wbg_setTransform_f25014a0bb3cb050: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setTransform(arg1, arg2, arg3, arg4, arg5, arg6);
        }, arguments); },
        __wbg_set_5337f8ac82364a3f: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            _assertBoolean(ret);
            return ret;
        }, arguments); },
        __wbg_set_6be42768c690e380: function() { return logError(function (arg0, arg1, arg2) {
            arg0[arg1] = arg2;
        }, arguments); },
        __wbg_set_fillStyle_a3656c7c5d4ad803: function() { return logError(function (arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        }, arguments); },
        __wbg_set_font_5b1b8c76449f5864: function() { return logError(function (arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        }, arguments); },
        __wbg_set_height_89a4ecd0f9cc3dfa: function() { return logError(function (arg0, arg1) {
            arg0.height = arg1 >>> 0;
        }, arguments); },
        __wbg_set_textBaseline_68cf9979f06f859b: function() { return logError(function (arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        }, arguments); },
        __wbg_set_width_d2ec5d6689655fa9: function() { return logError(function (arg0, arg1) {
            arg0.width = arg1 >>> 0;
        }, arguments); },
        __wbg_stack_3b0d974bbf31e44f: function() { return logError(function (arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        }, arguments); },
        __wbg_style_ad734f3851a343fb: function() { return logError(function (arg0) {
            const ret = arg0.style;
            return ret;
        }, arguments); },
        __wbg_width_7c985ca9f3cc024f: function() { return logError(function (arg0) {
            const ret = arg0.width;
            return ret;
        }, arguments); },
        __wbindgen_cast_0000000000000001: function() { return logError(function (arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        }, arguments); },
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


//#endregion
const TerminalKernelFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_terminalkernel_free(ptr, 1));
const RenderHandleFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_renderhandle_free(ptr, 1));


//#region intrinsics
function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

function _assertBoolean(n) {
    if (typeof(n) !== 'boolean') {
        throw new Error(`expected a boolean argument, found ${typeof(n)}`);
    }
}

function _assertClass(instance, klass) {
    if (!(instance instanceof klass)) {
        throw new Error(`expected instance of ${klass.name}`);
    }
}

function _assertNum(n) {
    if (typeof(n) !== 'number') throw new Error(`expected a number argument, found ${typeof(n)}`);
}

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

function logError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        let error = (function () {
            try {
                return e instanceof Error ? `${e.message}\n\nStack:\n${e.stack}` : e.toString();
            } catch(_) {
                return "<failed to stringify thrown value>";
            }
        }());
        console.error("wasm-bindgen: imported JS function that was not marked as `catch` threw an error:", error);
        throw e;
    }
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (typeof(arg) !== 'string') throw new Error(`expected a string argument, found ${typeof(arg)}`);
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
        if (ret.read !== arg.length) throw new Error('failed to pass whole string');
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


//#endregion

//#region wasm loading
let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
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
//#endregion
export { wasm as __wasm }
