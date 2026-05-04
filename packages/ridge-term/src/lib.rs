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
//!     - scrollOffset() / scrollbackLen()
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
        self.inner.feed(bytes);
        // Any new output invalidates the selection (matches xterm — the
        // user's selection is anchored to cells that may have moved).
        // Clear it; round 4's mouse-driven selection can do better than
        // this if it tracks logical character indices.
        if !bytes.is_empty() {
            self.selection.clear();
            // Same reasoning for search results — old match positions
            // point to cells that may have shifted. Caller can re-issue
            // the query if they want fresh results.
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

    pub fn rows(&self) -> usize { self.inner.rows() }
    pub fn cols(&self) -> usize { self.inner.cols() }

    /// Cursor row in viewport coordinates (0-based). Used by the IME
    /// helper-textarea positioning to anchor the candidate window near
    /// the actual input position.
    #[wasm_bindgen(js_name = cursorRow)]
    pub fn cursor_row(&self) -> usize { self.inner.grid().cursor().row }

    /// Cursor column in viewport coordinates (0-based).
    #[wasm_bindgen(js_name = cursorCol)]
    pub fn cursor_col(&self) -> usize { self.inner.grid().cursor().col }

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
        let ev = KeyEvent { key, ctrl, alt, shift, meta };
        let res = crate::input::encode(&ev, self.inner.modes());
        if res.consumed { res.bytes } else { Vec::new() }
    }

    /// Wrap a paste string for the PTY, applying bracketed-paste
    /// markers when DEC mode 2004 is active.
    #[wasm_bindgen(js_name = encodePaste)]
    pub fn encode_paste(&self, text: String) -> Vec<u8> {
        crate::input::wrap_paste(&text, self.inner.modes().bracketed_paste)
    }

    // ---- viewport scroll --------------------------------------------

    #[wasm_bindgen(js_name = scrollUp)]
    pub fn scroll_up(&mut self, n: usize) { self.inner.scroll_up_view(n); }

    #[wasm_bindgen(js_name = scrollDown)]
    pub fn scroll_down(&mut self, n: usize) { self.inner.scroll_down_view(n); }

    #[wasm_bindgen(js_name = scrollToBottom)]
    pub fn scroll_to_bottom(&mut self) { self.inner.scroll_to_bottom(); }

    #[wasm_bindgen(js_name = scrollOffset)]
    pub fn scroll_offset(&self) -> usize { self.inner.scroll_offset() }

    #[wasm_bindgen(js_name = scrollbackLen)]
    pub fn scrollback_len(&self) -> usize { self.inner.scrollback_len() }

    // ---- selection --------------------------------------------------

    #[wasm_bindgen(js_name = selectAll)]
    pub fn select_all(&mut self) {
        self.selection.select_all(&self.inner);
    }

    #[wasm_bindgen(js_name = clearSelection)]
    pub fn clear_selection(&mut self) { self.selection.clear(); }

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
        start_row: usize, start_col: usize,
        end_row: usize, end_col: usize,
    ) {
        self.selection.set(Range {
            start: Pos { row: start_row, col: start_col },
            end:   Pos { row: end_row,   col: end_col },
        });
    }

    #[wasm_bindgen(js_name = getSelectionText)]
    pub fn get_selection_text(&self) -> String {
        self.selection.text(&self.inner)
    }

    #[wasm_bindgen(js_name = hasSelection)]
    pub fn has_selection(&self) -> bool { !self.selection.is_empty() }

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
    pub fn search_match_count(&self) -> usize { self.search.match_count() }

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
            self.selection.set(r);
        } else {
            self.selection.clear();
        }
    }

    // ---- mode queries -----------------------------------------------

    #[wasm_bindgen(js_name = isAltScreen)]
    pub fn is_alt_screen(&self) -> bool { self.inner.is_alt_screen() }

    #[wasm_bindgen(js_name = isCursorVisible)]
    pub fn is_cursor_visible(&self) -> bool { self.inner.modes().cursor_visible }

    #[wasm_bindgen(js_name = isBracketedPaste)]
    pub fn is_bracketed_paste(&self) -> bool { self.inner.modes().bracketed_paste }

    #[wasm_bindgen(js_name = isAppCursorKeys)]
    pub fn is_app_cursor_keys(&self) -> bool { self.inner.modes().app_cursor_keys }

    /// Synchronous output mode `?2026`. While `true`, the manager should
    /// hold off rendering frames so the user doesn't see torn intermediate
    /// states during multi-step redraws (Ink/lazygit/bottom). Manager
    /// owns the timeout fallback (default 150ms) so this stays a clock-free
    /// boolean check.
    #[wasm_bindgen(js_name = isSyncOutput)]
    pub fn is_sync_output(&self) -> bool { self.inner.modes().sync_output }

    /// Focus reporting mode `?1004`. While `true`, the manager should emit
    /// `\x1b[I` on focus-in and `\x1b[O` on focus-out via the same
    /// dataHandler channel as keyboard input. claude code, vim, fzf use
    /// these to refresh state when the user switches to / from the pane.
    #[wasm_bindgen(js_name = isFocusReporting)]
    pub fn is_focus_reporting(&self) -> bool { self.inner.modes().mouse_focus }

    #[wasm_bindgen(js_name = dumpVisibleText)]
    pub fn dump_visible_text(&self) -> Vec<JsValue> {
        self.inner.dump_visible_text()
            .into_iter()
            .map(JsValue::from)
            .collect()
    }

    /// Look up the OSC 8 hyperlink span containing the cell at `(row, col)`
    /// in viewport coordinates. Returns `{ uri, id }` or `null`. Used by
    /// the manager's Ctrl+click handler to decide whether to open a link.
    #[wasm_bindgen(js_name = hyperlinkAt)]
    pub fn hyperlink_at(&self, row: usize, col: usize) -> JsValue {
        let Some(r) = self.inner.viewport_row(row) else { return JsValue::NULL };
        let Some(span) = r.link_at(col) else { return JsValue::NULL };
        // Build a small JS object via Reflect — avoids serde dep cost
        // for this single-shot lookup.
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"uri".into(), &span.uri.as_str().into());
        match &span.id {
            Some(id) => { let _ = js_sys::Reflect::set(&obj, &"id".into(), &id.as_str().into()); }
            None => { let _ = js_sys::Reflect::set(&obj, &"id".into(), &JsValue::NULL); }
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
    use crate::render::{AnyBackend, Canvas2dBackend, FrameMetrics, Renderer, Theme};
    use crate::render::backend::RenderBackend;
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
            let metrics = FrameMetrics { cell_w: 8.0, cell_h: 16.0, dpr: 1.0 };
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
        ) -> Result<RenderHandle, JsValue> {
            if let Ok(b) = crate::render::webgpu::WebGpuBackend::new(canvas.clone()).await {
                let metrics = FrameMetrics { cell_w: 8.0, cell_h: 16.0, dpr: 1.0 };
                let renderer = Renderer::new(
                    AnyBackend::Webgpu(b),
                    metrics,
                    Theme::default_dark(),
                );
                return Ok(RenderHandle { renderer });
            }
            // WebGPU adapter missed — fall through to Canvas2D.
            let backend = Canvas2dBackend::new(canvas).map_err(JsValue::from)?;
            let metrics = FrameMetrics { cell_w: 8.0, cell_h: 16.0, dpr: 1.0 };
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
            let (w, h) = self.renderer
                .backend_mut()
                .measure_font(&font_family, font_size_px)
                .map_err(JsValue::from)?;
            self.renderer.set_metrics(FrameMetrics {
                cell_w: w, cell_h: h, dpr,
            });
            Ok(vec![w, h])
        }

        pub fn resize(
            &mut self,
            width_css: u32,
            height_css: u32,
            dpr: f32,
        ) -> Result<(), JsValue> {
            self.renderer
                .backend_mut()
                .resize_surface(width_css, height_css, dpr)
                .map_err(JsValue::from)?;
            self.renderer.set_dpr(dpr);
            self.renderer.invalidate_all();
            Ok(())
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
                kernel.selection.range(),
                js_sys::Date::now(),
            )
        }

        /// Force a full redraw on the next render() — useful after
        /// invalidating external state without using the dedicated setters.
        #[wasm_bindgen(js_name = invalidateAll)]
        pub fn invalidate_all(&mut self) {
            self.renderer.invalidate_all();
        }

        /// Multi-pane hosts call this when the active pane changes. When
        /// `focused` is false, the renderer skips cursor draw entirely so
        /// only the truly active terminal blinks. Idempotent.
        #[wasm_bindgen(js_name = setFocused)]
        pub fn set_focused(&mut self, focused: bool) {
            self.renderer.set_focused(focused);
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
}

#[cfg(target_arch = "wasm32")]
pub use renderer_js::RenderHandle;
