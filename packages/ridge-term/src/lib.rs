//! WASM entry point — round 2.3 surface.
//!
//! What's exposed to JS:
//!
//!   class TerminalKernel
//!     - new TerminalKernel(rows, cols, scrollback)
//!     - feed(bytes)
//!     - resize(rows, cols)
//!     - rows() / cols()
//!     - encodeKey(key, ctrl, alt, shift, meta) → Uint8Array
//!     - encodePaste(text) → Uint8Array  (bracketed-paste aware)
//!     - scrollUp(n) / scrollDown(n) / scrollToBottom()
//!     - scrollOffset() / scrollbackLen() / isUserScrollLocked()
//!     - selectAll()
//!     - clearSelection()
//!     - setSelection(startRow, startCol, endRow, endCol)
//!     - getSelectionText()
//!     - hasSelection()
//!     - isAltScreen() / isCursorVisible() / isBracketedPaste() / isAppCursorKeys()
//!     - dumpVisibleText()  (debug)
//!
//!   class RenderHandle (wasm32-only)
//!     - new RenderHandle(canvas)
//!     - configure(fontFamily, sizePx, dpr) → [cellW, cellH]
//!     - resize(widthCss, heightCss, dpr)
//!     - applyTheme(themeJsObject)        — partial overrides allowed
//!     - applyDefaultTheme()
//!     - render(kernel) → bool
//!     - invalidateAll()
//!
//! `RenderHandle` and `TerminalKernel` are still 1:1 in this round —
//! round 2.4 adds the `TerminalManager` that owns multiple kernels and
//! renders them onto a shared surface.

use wasm_bindgen::prelude::*;

pub mod input;
pub mod render;
pub mod search;
pub mod selection;
pub mod term;

use crate::input::KeyEvent;
use crate::search::SearchState;
use crate::selection::{Pos, Range, Selection};
use crate::term::Terminal;

#[wasm_bindgen(start)]
pub fn _init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen(js_name = TerminalKernel)]
pub struct JsTerminal {
    inner: Terminal,
    selection: Selection,
    search: SearchState,
}

#[wasm_bindgen(js_class = TerminalKernel)]
impl JsTerminal {
    #[wasm_bindgen(constructor)]
    pub fn new(rows: usize, cols: usize, scrollback: usize) -> JsTerminal {
        JsTerminal {
            inner: Terminal::new(rows.max(1), cols.max(1), scrollback),
            selection: Selection::new(),
            search: SearchState::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        // §B.2 (2026-05-08) — selection now uses abs-row anchors that
        // are stable across TUI redraws, viewport scroll, and even
        // ordinary push-to-scrollback. The ONLY case where stored
        // abs_row values become stale is when a row gets EVICTED from
        // the oldest end of the scrollback ring (capacity rollover);
        // after eviction, abs_row 0 silently points to a different
        // content row. Use the monotonic eviction counter to detect
        // that case and clear only then.
        //
        // Pre-fix this clear fired on EVERY non-empty feed — including
        // every TUI frame redraw — which made user selections
        // disappear instantly under htop / vim / claude / less, the
        // user-reported "TUI 一直刷新无法选中复制" symptom. The
        // abs-row infrastructure has been in place for several rounds;
        // the over-eager invalidation was the actual blocker.
        let evictions_before = self.inner.scrollback_eviction_count();
        self.inner.feed(bytes);
        let evictions_after = self.inner.scrollback_eviction_count();
        if evictions_after != evictions_before {
            // Eviction crossed an abs-row boundary — anchor records
            // would now point to wrong content. Drop them; user
            // re-issues the query.
            self.selection.clear();
            self.search.clear();
        }
    }

    /// Prepend older history at the OLDEST end of the scrollback ring.
    ///
    /// Used by `manager.fetchOlderScrollback`: when the user pages up past
    /// the in-kernel scrollback boundary, the JS layer fetches an older
    /// chunk from the Tauri `get_pane_scrollback_before` command and feeds
    /// it here. The bytes are parsed in an isolated sandbox so the live
    /// grid, cursor, attrs, modes, and pending queues are untouched —
    /// only the scrollback ring grows at its older end. Selection and
    /// search anchors stay valid because existing rows don't move.
    ///
    /// See `Terminal::prepend_scrollback` for sandbox / AttrId-remap
    /// semantics.
    #[wasm_bindgen(js_name = prependScrollback)]
    pub fn prepend_scrollback(&mut self, bytes: &[u8]) {
        self.inner.prepend_scrollback(bytes);
    }

    /// Drain query-response bytes (DSR `\x1b[r;cR`, DA `\x1b[?...c`) the
    /// parser produced during the most recent `feed` calls. Caller MUST
    /// forward these bytes to the PTY as if they were keystrokes; without
    /// this round-trip, PowerShell + ConPTY render the prompt at a stale
    /// cursor row after a child process exits (e.g. Ctrl+C out of a TUI),
    /// overwriting whatever was on screen.
    #[wasm_bindgen(js_name = takePendingResponse)]
    pub fn take_pending_response(&mut self) -> Vec<u8> {
        self.inner.take_pending_response()
    }

    /// Drain semantic events (title, cwd, hyperlinks, bell) produced by
    /// the parser during the most recent `feed` calls. Returns a JS
    /// array of tagged objects: `{ type: "TitleChanged", value: "..." }`
    /// etc. Caller routes each event to the relevant Svelte store
    /// (paneTitleStore, paneCwdStore, ...).
    #[wasm_bindgen(js_name = takePendingEvents)]
    pub fn take_pending_events(&mut self) -> Vec<JsValue> {
        self.inner
            .take_pending_events()
            .into_iter()
            .filter_map(|ev| serde_wasm_bindgen::to_value(&ev).ok())
            .collect()
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.inner.resize(rows.max(1), cols.max(1));
        self.selection.clear();
    }

    /// Diagnostic accessor for the alt-screen-resize bug investigation
    /// (§1.22 / §1.23 / §1.24). Returns the kernel's last 32 resize calls
    /// as a JS array of `{ old_rows, old_cols, new_rows, new_cols, is_alt,
    /// dim_changed, branch, wipe_fired }` objects, newest last.
    ///
    /// Frontend usage (when `localStorage.RIDGE_DIAG === '1'`):
    ///   `__RIDGE_KERNEL.lastResizeDiags()` after a live resize confirms
    ///   whether `is_alt` was true at the kernel level and whether the
    ///   §1.22 wipe path fired. See `docs/term-rebuild/REPRO_alt_resize.md`.
    /// §1.27-tail (2026-05-07) — JS-accessible snapshot of the cursor's
    /// position at the moment of the most recent absolute-positioning
    /// CSI. Returns `null` (JS) when no abs CSI has been observed.
    /// Otherwise returns `{ row, col, atMs }` where `atMs` is the
    /// wall-clock unix-epoch ms timestamp.
    ///
    /// Frontend usage: `manager.ts::inputAnchorPixelPosition` falls back
    /// to this snapshot (when within the inline-TUI decay window) before
    /// falling back to the live cursor — so the IME helper anchor stays
    /// at the inline-TUI's input row even when the live cursor is
    /// mid-walk. See §1.27 in CLAUDE.md.
    #[wasm_bindgen(js_name = lastAbsCsiPosition)]
    pub fn last_abs_csi_position(&self) -> JsValue {
        let Some((row, col, at_ms)) = self.inner.grid().last_abs_csi_position() else {
            return JsValue::NULL;
        };
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"row".into(), &(row as u32).into());
        let _ = js_sys::Reflect::set(&obj, &"col".into(), &(col as u32).into());
        // f64 covers the full unix-epoch ms range comfortably.
        let _ = js_sys::Reflect::set(&obj, &"atMs".into(), &(at_ms as f64).into());
        obj.into()
    }

    #[wasm_bindgen(js_name = lastResizeDiags)]
    pub fn last_resize_diags(&self) -> Vec<JsValue> {
        self.inner
            .last_resize_diags()
            .iter()
            .filter_map(|d| serde_wasm_bindgen::to_value(d).ok())
            .collect()
    }

    /// §1.27 (2026-05-07) — diagnostic cell inspector for the dim/IME
    /// residue investigation. Returns up to `len` cells starting at
    /// (row, col) on the active screen as a JS array of plain objects
    /// `{ col, ch, codepoint, width, attrId, dim, bold, italic,
    /// underline, inverse, hidden, fg, bg }` so devtools can correlate
    /// "what does the user see at this position" with "what attrs are
    /// stored".
    ///
    /// Out-of-range row, col, or len silently returns a shorter array
    /// (or empty) rather than panicking — devtools should treat the
    /// shorter result as "row missing or too narrow".
    ///
    /// Frontend usage (when `localStorage.RIDGE_DIAG === '1'`):
    ///   `__RIDGE_KERNEL.cellsAt(cursorRow, 0, 80)` right after a
    ///   compositionEnd to verify whether DIM cells leaked into the
    ///   prompt area, or after observing residue to confirm whether
    ///   the underlying cell carries a DIM attribute (kernel bug) vs
    ///   correct attrs but stale pixels (renderer bug). See
    ///   `docs/term-rebuild/REPRO_dim_residue.md`.
    #[wasm_bindgen(js_name = cellsAt)]
    pub fn cells_at(&self, row: usize, col: usize, len: usize) -> Vec<JsValue> {
        use crate::term::attrs::{ColorKind, Flags};
        let Some(r) = self.inner.grid().row(row) else {
            return Vec::new();
        };
        let attr_table = &self.inner.grid().attrs;
        let end = col.saturating_add(len).min(r.cells.len());
        let mut out = Vec::with_capacity(end.saturating_sub(col));
        let fmt_color = |kind: ColorKind| match kind {
            ColorKind::Default => "default".to_string(),
            ColorKind::Indexed(i) => format!("idx({i})"),
            ColorKind::Rgb(rr, gg, bb) => format!("rgb({rr},{gg},{bb})"),
        };
        for c in col..end {
            let cell = r.cells[c];
            let attrs = attr_table.get(cell.attr);
            let fg = fmt_color(attrs.fg.kind());
            let bg = fmt_color(attrs.bg.kind());
            // Build the result object directly via js_sys to keep the
            // field shape stable without a serde adapter just for this
            // diagnostic. ignore_result on `Reflect::set` because the
            // calls only fail on objects that aren't extensible — this
            // brand-new Object always is.
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&obj, &"col".into(), &(c as u32).into());
            let _ = js_sys::Reflect::set(&obj, &"ch".into(), &cell.ch.to_string().into());
            let _ = js_sys::Reflect::set(&obj, &"codepoint".into(), &(cell.ch as u32).into());
            let _ = js_sys::Reflect::set(&obj, &"width".into(), &(cell.width as u32).into());
            let _ = js_sys::Reflect::set(&obj, &"attrId".into(), &(cell.attr.0 as u32).into());
            let _ = js_sys::Reflect::set(
                &obj,
                &"dim".into(),
                &attrs.flags.contains(Flags::DIM).into(),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &"bold".into(),
                &attrs.flags.contains(Flags::BOLD).into(),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &"italic".into(),
                &attrs.flags.contains(Flags::ITALIC).into(),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &"underline".into(),
                &attrs.flags.contains(Flags::UNDERLINE).into(),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &"inverse".into(),
                &attrs.flags.contains(Flags::INVERSE).into(),
            );
            let _ = js_sys::Reflect::set(
                &obj,
                &"hidden".into(),
                &attrs.flags.contains(Flags::HIDDEN).into(),
            );
            let _ = js_sys::Reflect::set(&obj, &"fg".into(), &fg.into());
            let _ = js_sys::Reflect::set(&obj, &"bg".into(), &bg.into());
            out.push(obj.into());
        }
        out
    }

    pub fn rows(&self) -> usize {
        self.inner.rows()
    }
    pub fn cols(&self) -> usize {
        self.inner.cols()
    }

    /// Cursor row in viewport coordinates (0-based). Used by the IME
    /// helper-textarea positioning to anchor the candidate window near
    /// the actual input position.
    #[wasm_bindgen(js_name = cursorRow)]
    pub fn cursor_row(&self) -> usize {
        self.inner.grid().cursor().row
    }

    /// Cursor column in viewport coordinates (0-based).
    #[wasm_bindgen(js_name = cursorCol)]
    pub fn cursor_col(&self) -> usize {
        self.inner.grid().cursor().col
    }

    // ---- input encoding ---------------------------------------------

    /// Encode a key event to the byte sequence the PTY expects. Returns
    /// an empty array if the event is unknown (caller may then let the
    /// browser handle it natively).
    ///
    /// The JS-side normalizes `meta` (Cmd) into `ctrl` on macOS before
    /// calling — see `input.rs` for rationale.
    #[wasm_bindgen(js_name = encodeKey)]
    pub fn encode_key(
        &self,
        key: String,
        ctrl: bool,
        alt: bool,
        shift: bool,
        meta: bool,
    ) -> Vec<u8> {
        let ev = KeyEvent {
            key,
            ctrl,
            alt,
            shift,
            meta,
        };
        let res = crate::input::encode(&ev, self.inner.modes());
        if res.consumed {
            res.bytes
        } else {
            Vec::new()
        }
    }

    /// Wrap a paste string for the PTY, applying bracketed-paste
    /// markers when DEC mode 2004 is active.
    #[wasm_bindgen(js_name = encodePaste)]
    pub fn encode_paste(&self, text: String) -> Vec<u8> {
        crate::input::wrap_paste(&text, self.inner.modes().bracketed_paste)
    }

    // ---- viewport scroll --------------------------------------------

    #[wasm_bindgen(js_name = scrollUp)]
    pub fn scroll_up(&mut self, n: usize) {
        self.inner.scroll_up_view(n);
    }

    #[wasm_bindgen(js_name = scrollDown)]
    pub fn scroll_down(&mut self, n: usize) {
        self.inner.scroll_down_view(n);
    }

    #[wasm_bindgen(js_name = scrollToBottom)]
    pub fn scroll_to_bottom(&mut self) {
        self.inner.scroll_to_bottom();
    }

    #[wasm_bindgen(js_name = scrollOffset)]
    pub fn scroll_offset(&self) -> usize {
        self.inner.scroll_offset()
    }

    #[wasm_bindgen(js_name = scrollbackLen)]
    pub fn scrollback_len(&self) -> usize {
        self.inner.scrollback_len()
    }

    /// §B.2 (2026-05-08) — drop the in-kernel scrollback ring buffer
    /// (physical clear) and snap viewport to live grid. Mirrors
    /// `\x1b[3J` at the JS API level so the right-click "清空" path
    /// can wipe both screen + saved lines without a PTY round trip
    /// (and without stepping on shells that don't translate Ctrl+L
    /// into ED 3). Selection is cleared so it doesn't survive into
    /// nonexistent rows. Search results similarly drop.
    #[wasm_bindgen(js_name = clearScrollback)]
    pub fn clear_scrollback(&mut self) {
        self.inner.clear_scrollback();
        self.selection.clear();
        self.search.clear();
    }

    /// Whether the user has paged into history and PTY output is
    /// currently being held back from auto-snapping the viewport.
    /// JS surfaces this as a "follow tail" indicator. Cleared by
    /// `scrollToBottom`.
    #[wasm_bindgen(js_name = isUserScrollLocked)]
    pub fn is_user_scroll_locked(&self) -> bool {
        self.inner.is_user_scroll_locked()
    }

    // ---- selection --------------------------------------------------

    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&mut self) {
        self.selection.select_all(&self.inner);
    }

    #[wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    /// Double-click word selection. Selects the word at the given cell
    /// coordinate; clears selection when the cell is whitespace/empty.
    #[wasm_bindgen(js_name = selectWordAt)]
    pub fn select_word_at(&mut self, row: usize, col: usize) {
        self.selection.select_word(&self.inner, row, col);
    }

    /// Triple-click line selection.
    #[wasm_bindgen(js_name = selectLineAt)]
    pub fn select_line_at(&mut self, row: usize) {
        self.selection.select_line(&self.inner, row);
    }

    /// Programmatically set a selection range. Coordinates are
    /// viewport-relative (same as the renderer).
    #[wasm_bindgen(js_name = setSelection)]
    pub fn set_selection(
        &mut self,
        start_row: usize,
        start_col: usize,
        end_row: usize,
        end_col: usize,
    ) {
        // Pass &self.inner so Selection captures the current scroll state
        // and stores the range in abs-row form (§1.20). After this, scroll
        // changes don't dislodge the highlight from its original cells.
        self.selection.set(
            &self.inner,
            Range {
                start: Pos {
                    row: start_row,
                    col: start_col,
                },
                end: Pos {
                    row: end_row,
                    col: end_col,
                },
            },
        );
    }

    #[wasm_bindgen(js_name = getSelectionText)]
    pub fn get_selection_text(&self) -> String {
        self.selection.text(&self.inner)
    }

    #[wasm_bindgen(js_name = hasSelection)]
    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    // ---- search -----------------------------------------------------

    /// Run a search across scrollback + viewport. Returns the number of
    /// matches. Scrolls the viewport so the first match is visible and
    /// sets the selection to it (renderer's existing overlay highlights).
    /// Empty query clears search state and selection.
    #[wasm_bindgen(js_name = searchSetQuery)]
    pub fn search_set_query(&mut self, query: String, case_sensitive: bool) -> usize {
        let n = self.search.set_query(&self.inner, &query, case_sensitive);
        self.apply_active_match();
        n
    }

    /// Step to the next match (wraps). Returns the new active index, or
    /// `usize::MAX` if there are no matches.
    #[wasm_bindgen(js_name = searchNext)]
    pub fn search_next(&mut self) -> usize {
        if self.search.next().is_some() {
            self.apply_active_match();
            self.search.active_index().unwrap_or(usize::MAX)
        } else {
            usize::MAX
        }
    }

    /// Step to the previous match (wraps).
    #[wasm_bindgen(js_name = searchPrev)]
    pub fn search_prev(&mut self) -> usize {
        if self.search.prev().is_some() {
            self.apply_active_match();
            self.search.active_index().unwrap_or(usize::MAX)
        } else {
            usize::MAX
        }
    }

    /// Clear search state and the highlight selection.
    #[wasm_bindgen(js_name = searchClear)]
    pub fn search_clear(&mut self) {
        self.search.clear();
        self.selection.clear();
    }

    #[wasm_bindgen(js_name = searchMatchCount)]
    pub fn search_match_count(&self) -> usize {
        self.search.match_count()
    }

    /// Returns the active match index, or `usize::MAX` when no active match.
    #[wasm_bindgen(js_name = searchActiveIndex)]
    pub fn search_active_index(&self) -> usize {
        self.search.active_index().unwrap_or(usize::MAX)
    }

    /// Internal: bring the active match into view + highlight via selection.
    /// Scroll policy: place the matched row at viewport top (vp_row 0) for
    /// scrollback matches; reset to live grid for viewport matches.
    fn apply_active_match(&mut self) {
        let Some(m) = self.search.active_match() else {
            self.selection.clear();
            return;
        };
        let sb_len = self.inner.scrollback_len();
        let rows_n = self.inner.rows();
        let offset = SearchState::desired_scroll_offset_for(m, sb_len);
        // Apply the offset via the public scroll API (scroll_offset is
        // private to Terminal).
        self.inner.scroll_to_bottom();
        if offset > 0 {
            self.inner.scroll_up_view(offset);
        }
        if let Some(r) = SearchState::match_to_viewport_range(m, offset, sb_len, rows_n) {
            // After scroll_up_view above, &self.inner reports the new
            // scroll_offset; selection.set captures it and converts r
            // to abs-row form (§1.20).
            self.selection.set(&self.inner, r);
        } else {
            self.selection.clear();
        }
    }

    // ---- mode queries -----------------------------------------------

    #[wasm_bindgen(js_name = isAltScreen)]
    pub fn is_alt_screen(&self) -> bool {
        self.inner.is_alt_screen()
    }

    #[wasm_bindgen(js_name = isCursorVisible)]
    pub fn is_cursor_visible(&self) -> bool {
        self.inner.modes().cursor_visible
    }

    /// §A.3 inline-TUI heuristic — true when an Ink-style app is rendering
    /// inline on primary (cursor hidden + recent absolute-positioning CSI
    /// within the decay window) and the kernel is NOT on alt screen.
    /// Read by `manager.ts::fitPane` to decide whether to wipe primary
    /// before resizing the PTY (mirrors the existing alt-screen branch).
    #[wasm_bindgen(js_name = isInlineTuiMode)]
    pub fn is_inline_tui_mode(&self) -> bool {
        self.inner.is_inline_tui_mode_at(js_sys::Date::now() as i64)
    }

    #[wasm_bindgen(js_name = isBracketedPaste)]
    pub fn is_bracketed_paste(&self) -> bool {
        self.inner.modes().bracketed_paste
    }

    #[wasm_bindgen(js_name = isAppCursorKeys)]
    pub fn is_app_cursor_keys(&self) -> bool {
        self.inner.modes().app_cursor_keys
    }

    /// Synchronous output mode `?2026`. While `true`, the manager should
    /// hold off rendering frames so the user doesn't see torn intermediate
    /// states during multi-step redraws (Ink/lazygit/bottom). Manager
    /// owns the timeout fallback (default 150ms) so this stays a clock-free
    /// boolean check.
    #[wasm_bindgen(js_name = isSyncOutput)]
    pub fn is_sync_output(&self) -> bool {
        self.inner.modes().sync_output
    }

    /// Focus reporting mode `?1004`. While `true`, the manager should emit
    /// `\x1b[I` on focus-in and `\x1b[O` on focus-out via the same
    /// dataHandler channel as keyboard input. claude code, vim, fzf use
    /// these to refresh state when the user switches to / from the pane.
    #[wasm_bindgen(js_name = isFocusReporting)]
    pub fn is_focus_reporting(&self) -> bool {
        self.inner.modes().mouse_focus
    }

    #[wasm_bindgen(js_name = dumpVisibleText)]
    pub fn dump_visible_text(&self) -> Vec<JsValue> {
        self.inner
            .dump_visible_text()
            .into_iter()
            .map(JsValue::from)
            .collect()
    }

    /// Look up the OSC 8 hyperlink span containing the cell at `(row, col)`
    /// in viewport coordinates. Returns `{ uri, id }` or `null`. Used by
    /// the manager's Ctrl+click handler to decide whether to open a link.
    #[wasm_bindgen(js_name = hyperlinkAt)]
    pub fn hyperlink_at(&self, row: usize, col: usize) -> JsValue {
        let Some(r) = self.inner.viewport_row(row) else {
            return JsValue::NULL;
        };
        let Some(span) = r.link_at(col) else {
            return JsValue::NULL;
        };
        // Build a small JS object via Reflect — avoids serde dep cost
        // for this single-shot lookup.
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"uri".into(), &span.uri.as_str().into());
        match &span.id {
            Some(id) => {
                let _ = js_sys::Reflect::set(&obj, &"id".into(), &id.as_str().into());
            }
            None => {
                let _ = js_sys::Reflect::set(&obj, &"id".into(), &JsValue::NULL);
            }
        }
        obj.into()
    }
}

// =====================================================================
// Renderer (wasm-only)
// =====================================================================

#[cfg(target_arch = "wasm32")]
mod renderer_js {
    use super::*;
    use crate::render::backend::RenderBackend;
    use crate::render::{AnyBackend, Canvas2dBackend, FrameMetrics, Renderer, Theme};
    use web_sys::HtmlCanvasElement;

    #[wasm_bindgen]
    pub struct RenderHandle {
        renderer: Renderer<AnyBackend>,
    }

    #[wasm_bindgen]
    impl RenderHandle {
        /// Sync constructor — Canvas2D-only. JS calls
        /// `new RenderHandle(canvas)`. For runtime-WebGPU adoption with
        /// graceful Canvas2D fallback, JS calls
        /// `await RenderHandle.newWithWebgpuFirst(canvas)` instead.
        #[wasm_bindgen(constructor)]
        pub fn new(canvas: HtmlCanvasElement) -> Result<RenderHandle, JsValue> {
            let backend = Canvas2dBackend::new(canvas).map_err(JsValue::from)?;
            let metrics = FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
                tui_mode: false,
            };
            let renderer = Renderer::new(
                AnyBackend::Canvas2d(backend),
                metrics,
                Theme::default_dark(),
            );
            Ok(RenderHandle { renderer })
        }

        /// Async constructor — try WebGPU first, fall back to Canvas2D
        /// on adapter miss / device-creation failure. Always succeeds
        /// when `Canvas2dBackend::new` succeeds; returns Err only if
        /// even the Canvas2D fallback can't initialize (rare; usually
        /// indicates a malformed canvas element).
        ///
        /// Only compiled when the `webgpu` cargo feature is on (the
        /// `wasm-bindgen-futures` dep needed for `#[wasm_bindgen]
        /// async fn` is gated behind that feature). In default builds,
        /// JS callers should use the sync `new RenderHandle(canvas)`
        /// constructor; they can detect the async constructor's
        /// presence via `typeof RenderHandle.newWithWebgpuFirst ===
        /// 'function'`.
        #[cfg(feature = "webgpu")]
        #[wasm_bindgen(js_name = newWithWebgpuFirst)]
        pub async fn new_with_webgpu_first(
            canvas: HtmlCanvasElement,
            surface_host: Option<SurfaceHostHandle>,
        ) -> Result<RenderHandle, JsValue> {
            // Per-workspace SurfaceHost (2026-05-08): JS passes the
            // pane's workspace's SurfaceHostHandle. If `None` (Canvas2D-
            // only build, manager.attachHost failed for this workspace,
            // adapter miss), fall through to Canvas2D against this
            // pane's own canvas.
            //
            // The `canvas` parameter is the per-pane fallback DOM
            // element used by Canvas2D. WebGPU draws never touch it
            // — they go through the per-workspace `<canvas data-rg-ws-host>`
            // bound to the workspace's SurfaceHost.
            if let Some(handle) = surface_host {
                if let Ok(b) =
                    crate::render::webgpu::WebGpuPaneBackend::new(handle.host_rc()).await
                {
                    let metrics = FrameMetrics {
                        cell_w: 8.0,
                        cell_h: 16.0,
                        dpr: 1.0,
                        tui_mode: false,
                    };
                    let renderer =
                        Renderer::new(AnyBackend::Webgpu(b), metrics, Theme::default_dark());
                    return Ok(RenderHandle { renderer });
                }
            }
            // No host available or WebGPU adapter missed — Canvas2D.
            let backend = Canvas2dBackend::new(canvas).map_err(JsValue::from)?;
            let metrics = FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
                tui_mode: false,
            };
            let renderer = Renderer::new(
                AnyBackend::Canvas2d(backend),
                metrics,
                Theme::default_dark(),
            );
            Ok(RenderHandle { renderer })
        }

        /// Configure font + measure cell dimensions. Returns [cell_w, cell_h]
        /// in CSS pixels so JS can calculate cols/rows for a target
        /// container size.
        pub fn configure(
            &mut self,
            font_family: String,
            font_size_px: f32,
            dpr: f32,
        ) -> Result<Vec<f32>, JsValue> {
            // Unified font config — AnyBackend dispatches to
            // Canvas2dBackend::set_font (which expects a single CSS
            // string built from family+size) or to
            // WebGpuBackend::set_font_config (which takes them
            // separately for the rasterizer).
            self.renderer
                .backend_mut()
                .set_font_config(font_family.clone(), font_size_px);
            let (w, h) = self
                .renderer
                .backend_mut()
                .measure_font(&font_family, font_size_px)
                .map_err(JsValue::from)?;
            self.renderer.set_metrics(FrameMetrics {
                cell_w: w,
                cell_h: h,
                dpr,
                tui_mode: false,
            });
            Ok(vec![w, h])
        }

        pub fn resize(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), JsValue> {
            self.renderer
                .backend_mut()
                .resize_surface(width_css, height_css, dpr)
                .map_err(JsValue::from)?;
            self.renderer.set_dpr(dpr);
            self.renderer.invalidate_all();
            Ok(())
        }

        /// Phase B: record the pane's `(x, y)` position on the host
        /// canvas in **device pixels**. JS calls this from
        /// `manager.ts::_recomputeViewport` whenever the splitter drag
        /// moves the pane's container without changing its size.
        ///
        /// No-op for Canvas2D-backed handles. WebGPU handles forward
        /// to `WebGpuPaneBackend::set_viewport_offset`. Does **not**
        /// trigger a redraw on its own — the pane content is unchanged
        /// on a positional shift; JS calls `surfaceHost.invalidate()`
        /// after layout settle to clear the old area.
        #[wasm_bindgen(js_name = setViewportOffset)]
        pub fn set_viewport_offset(&mut self, x: u32, y: u32) {
            self.renderer.backend_mut().set_viewport_offset(x, y);
        }

        /// Apply a theme. Accepts a JS object with any subset of:
        ///   { background, foreground, cursor, cursorAccent,
        ///     black, red, green, yellow, blue, magenta, cyan, white,
        ///     brightBlack, brightRed, brightGreen, brightYellow,
        ///     brightBlue, brightMagenta, brightCyan, brightWhite }
        /// Each value is a CSS hex color "#rgb" / "#rrggbb" / "#rrggbbaa".
        /// Keys not provided keep their existing palette entry.
        ///
        /// Note: this is **partial overrides on top of the current theme**,
        /// not a full replace. To reset, call `applyDefaultTheme()` first.
        #[wasm_bindgen(js_name = applyTheme)]
        pub fn apply_theme(&mut self, theme_obj: JsValue) -> Result<(), JsValue> {
            // We use Reflect rather than serde-wasm-bindgen to avoid the
            // dep cost — the API is small enough that field-by-field
            // lookup is fine.
            let mut t = self.current_theme();
            t.apply_partial(|key| {
                let val = js_sys::Reflect::get(&theme_obj, &JsValue::from_str(key)).ok()?;
                val.as_string()
            });
            self.renderer.set_theme(t);
            Ok(())
        }

        /// Reset to the default dark theme.
        #[wasm_bindgen(js_name = applyDefaultTheme)]
        pub fn apply_default_theme(&mut self) {
            self.renderer.set_theme(Theme::default_dark());
        }

        /// Drive one frame from the kernel's current grid. Returns true
        /// if anything was drawn (caller can use this to decide whether
        /// to schedule another frame). Selection range comes from the
        /// kernel's `selection` field — when set, the renderer paints a
        /// translucent overlay over those cells. Wall-clock comes from
        /// `Date.now()` for cursor-blink phase.
        pub fn render(&mut self, kernel: &JsTerminal) -> bool {
            self.renderer.tick(
                &kernel.inner,
                // range_in_viewport translates the stored abs-row
                // selection through the current scroll state per
                // frame, naturally clipping rows outside the viewport
                // (§1.20). Renderer's last_selection comparator sees
                // updated vp coords on scroll → triggers redraw with
                // the highlight at its new position.
                kernel.selection.range_in_viewport(&kernel.inner),
                js_sys::Date::now(),
            )
        }

        /// Force a full redraw on the next render() — useful after
        /// invalidating external state without using the dedicated setters.
        #[wasm_bindgen(js_name = invalidateAll)]
        pub fn invalidate_all(&mut self) {
            self.renderer.invalidate_all();
        }

        /// §4b per-pane increment cache (2026-05-08): re-record this
        /// pane's previously-uploaded GPU instance buffer into the
        /// host's current frame without retraversing the kernel grid.
        /// Returns `true` on success, `false` when the cache was
        /// invalidated (caller must fall back to full `render`).
        ///
        /// Used by `manager.ts::startRafLoop` for visible host-mode
        /// panes that pre-pass marked NOT dirty: the swap-chain
        /// `LoadOp::Clear` would otherwise wipe their region (forcing
        /// a re-encode for unchanged content). With this path, the
        /// per-tick CPU cost of N idle visible panes drops from
        /// O(rows × cols × N) to one GPU draw call per pane —
        /// eliminating the typing-while-other-panes-have-output lag
        /// (forceHostRenderAll's multiplier).
        #[wasm_bindgen(js_name = recordCachedOnly)]
        pub fn record_cached_only(&mut self) -> bool {
            self.renderer.record_cached_only()
        }

        /// Multi-pane hosts call this when the active pane changes. When
        /// `focused` is false, the renderer skips cursor draw entirely so
        /// only the truly active terminal blinks. Idempotent.
        #[wasm_bindgen(js_name = setFocused)]
        pub fn set_focused(&mut self, focused: bool) {
            self.renderer.set_focused(focused);
        }

        /// Non-mutating mirror of `render`'s early-exit conditions:
        /// returns `true` when the next `render` call would do any
        /// drawing work, `false` when the renderer has nothing to
        /// redraw and the JS caller can sleep its RAF loop. `now_ms`
        /// must use the same epoch as the value passed to `render`
        /// (`Date.now()` in JS).
        ///
        /// Cost: ~24 row hashes for an 80×24 grid (≈4 µs) plus the
        /// selection / scroll / blink checks. Cheaper than one
        /// `draw_row` call by two orders of magnitude.
        #[wasm_bindgen(js_name = isDirty)]
        pub fn is_dirty(&self, kernel: &JsTerminal, now_ms: f64) -> bool {
            self.renderer.is_dirty(
                &kernel.inner,
                kernel.selection.range_in_viewport(&kernel.inner),
                now_ms,
            )
        }

        /// Milliseconds until the next cursor-blink phase boundary. JS
        /// callers use this to schedule a `setTimeout` wake-up while
        /// the RAF loop is paused. Returns a very large number
        /// (effectively infinity) when the cursor isn't blinking — the
        /// caller should treat any value > some reasonable cap (e.g.
        /// 1000 ms) as "no blink, sleep at most a second on a watchdog".
        #[wasm_bindgen(js_name = nextBlinkDeadlineMs)]
        pub fn next_blink_deadline_ms(&self, kernel: &JsTerminal, now_ms: f64) -> f64 {
            self.renderer.next_blink_deadline_ms(&kernel.inner, now_ms)
        }

        /// Internal: snapshot the current theme so apply_partial can layer
        /// overrides on top of it. Renderer doesn't expose `theme()`
        /// directly — we reach in via a fresh default + reapply isn't
        /// right either (would lose previous overrides). Solution:
        /// renderer needs a getter. Add one.
        fn current_theme(&self) -> Theme {
            self.renderer.theme().clone()
        }
    }

    // ──────────────────────────────────────────────────────────────
    // SurfaceHostHandle (§A.8 per-workspace)
    //
    // JS-facing wrapper around a `SurfaceHost`. One instance per
    // workspace tab — `manager.ts` holds a Map<workspaceId, handle>
    // and passes the matching handle to each pane's
    // `RenderHandle.newWithWebgpuFirst(canvas, host)` so the pane's
    // WebGPU draws land on its workspace's canvas.
    //
    // Per-frame protocol from JS RAF (active workspace only):
    //   1. host.beginFrame(themeBg)         — acquire swap-chain texture
    //   2. for each dirty pane in this workspace: handle.render(kernel)
    //   3. host.endFrame()                   — submit + present
    // ──────────────────────────────────────────────────────────────

    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    #[derive(Clone)]
    pub struct SurfaceHostHandle {
        host: std::rc::Rc<std::cell::RefCell<crate::render::surface_host::SurfaceHost>>,
    }

    #[cfg(feature = "webgpu")]
    impl SurfaceHostHandle {
        /// Internal accessor: the per-workspace SurfaceHost Rc. Used by
        /// `RenderHandle::new_with_webgpu_first` so newly-constructed
        /// `WebGpuPaneBackend`s share the same SurfaceHost (and thus
        /// the same `<canvas data-rg-ws-host>`) as their workspace tab.
        pub(crate) fn host_rc(
            &self,
        ) -> std::rc::Rc<std::cell::RefCell<crate::render::surface_host::SurfaceHost>>
        {
            self.host.clone()
        }
    }

    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    impl SurfaceHostHandle {
        /// JS-callable clone: produces a new `SurfaceHostHandle` JS
        /// wrapper that bumps the inner `Rc` refcount. Required because
        /// `RenderHandle::newWithWebgpuFirst(canvas, host)` consumes
        /// its `host` parameter (wasm-bindgen `Option<T>` semantics —
        /// the JS-side wrapper is freed after the call). When N panes
        /// in the same workspace each call attach, JS must
        /// `host.clone()` per call so the manager's stored handle
        /// stays alive.
        #[wasm_bindgen(js_name = clone)]
        pub fn js_clone(&self) -> SurfaceHostHandle {
            Clone::clone(self)
        }
    }

    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    impl SurfaceHostHandle {
        /// Async constructor: create one swap chain bound to `canvas`.
        /// One SurfaceHostHandle per workspace tab — JS holds a Map
        /// keyed by workspace id and passes the matching handle to
        /// each pane's `RenderHandle.newWithWebgpuFirst(canvas, host)`.
        ///
        /// Returns `Err` (rejected promise on the JS side) when the
        /// WebGPU adapter / device acquisition fails or
        /// `instance.create_surface` rejects the canvas. JS catches
        /// and either retries or falls back to per-pane Canvas2D for
        /// panes in this workspace.
        #[wasm_bindgen(js_name = init)]
        pub async fn init(canvas: HtmlCanvasElement) -> Result<SurfaceHostHandle, JsValue> {
            let host = crate::render::surface_host::SurfaceHost::init(canvas)
                .await
                .map_err(JsValue::from)?;
            Ok(SurfaceHostHandle { host })
        }

        /// Resize the host swap chain. JS drives this from a
        /// ResizeObserver on the host canvas's parent so the surface
        /// always matches the visible workspace area.
        pub fn resize(&self, width_css: u32, height_css: u32, dpr: f32) {
            self.host.borrow_mut().resize(width_css, height_css, dpr);
        }

        /// Mark the next frame for a fresh `LoadOp::Clear`. JS calls
        /// this when a pane detaches / parks / unparks (so departed
        /// pixels don't linger), when the theme changes, and after
        /// splitter settle moves pane boundaries.
        pub fn invalidate(&self) {
            self.host.borrow_mut().invalidate();
        }

        /// Begin one host frame: acquire swap-chain texture + create
        /// encoder. Returns `true` on success, `false` on surface-lost
        /// — JS skips the rest of the frame and lets the next RAF
        /// retry. `theme_bg` is a 4-byte RGBA buffer; values outside
        /// `[0..255]` get clamped at the byte boundary by
        /// `Uint8Array.set`.
        ///
        /// Idempotent guard: a second call without an intervening
        /// `endFrame` drops the stale frame and starts fresh (defense
        /// against JS bugs that skip the end half).
        #[wasm_bindgen(js_name = beginFrame)]
        pub fn begin_frame(&self, theme_bg: &[u8]) -> bool {
            let mut rgba = [0u8; 4];
            // Clamp short input to opaque-on-black so we never panic on
            // a malformed slice; JS callers are expected to send
            // exactly 4 bytes.
            for (i, &b) in theme_bg.iter().take(4).enumerate() {
                rgba[i] = b;
            }
            if theme_bg.len() < 4 {
                rgba[3] = 255;
            }
            self.host.borrow_mut().begin_frame(rgba)
        }

        /// Finish the host's command encoder + queue.submit + present.
        /// One call per frame after all dirty panes have rendered. Safe
        /// to call without a matching `beginFrame` — internal guard
        /// returns early.
        #[wasm_bindgen(js_name = endFrame)]
        pub fn end_frame(&self) {
            self.host.borrow_mut().end_frame();
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use renderer_js::RenderHandle;

#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub use renderer_js::SurfaceHostHandle;
