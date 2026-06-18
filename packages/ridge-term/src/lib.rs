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
//!     - isMouseReporting() / isMouseButtonEvent() / isMouseAnyEvent() / isMouseSgr()
//!     - encodeMouse(row, col, button, action, shift, ctrl, alt) → Uint8Array
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
use crate::selection::{Pos, Range, RangeAbs, Selection};
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
    /// §1.33 (2026-05-22) — wall-clock ms of the most recent observation
    /// that ANY TUI signal was true on this kernel. Bumped from inside
    /// `should_allow_shell_history` every time the gate runs while a
    /// live signal still holds, and read by the sticky-window branch to
    /// keep the shell-history popup gated for `SHELL_HISTORY_STICKY_MS`
    /// after the last TUI frame — irrespective of cursor visibility,
    /// which is where the previous JS-side `tuiGate` leaked (Claude
    /// Code menu transitions briefly show the cursor between frames).
    last_tui_signal_at_ms: i64,
}

/// §1.33 (2026-05-22) — sticky window for the shell-history popup gate.
/// After any TUI signal (DECCKM, alt screen, mouse reporting, inline-TUI
/// heuristic, cursor hidden) is observed true, the gate stays closed for
/// this many milliseconds even after the signal clears. Matches the
/// kernel's existing `INLINE_TUI_DECAY_MS` so a TUI's intra-frame
/// "all-signals-false" gap can't race the popup open between repaints.
const SHELL_HISTORY_STICKY_MS: i64 = 0;

#[wasm_bindgen(js_class = TerminalKernel)]
impl JsTerminal {
    #[wasm_bindgen(constructor)]
    pub fn new(rows: usize, cols: usize, scrollback: usize) -> JsTerminal {
        JsTerminal {
            inner: Terminal::new(rows.max(1), cols.max(1), scrollback),
            selection: Selection::new(),
            search: SearchState::new(),
            last_tui_signal_at_ms: 0,
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

    /// P3.6 (2026-05-20) — apply one postcard-encoded `DeltaFrame` (produced
    /// by the Rust-side `engine::parser::PaneParser`) to the mirror grid.
    ///
    /// Counterpart to `feed()` for the `Settings.parserBackend = 'rust'`
    /// path: PTY bytes are parsed once by the native PaneParser, the
    /// resulting frame is postcard-encoded and emitted as a Tauri event,
    /// and the wasm consumer applies the diff here instead of running its
    /// own vte parse on the JS main thread.
    ///
    /// Returns `Err(JsValue)` with a human-readable string on decode
    /// failure OR protocol-version mismatch — caller is expected to log
    /// and trigger a `force_full_reframe` self-heal (manager.ts P3.9
    /// wiring).
    ///
    /// Selection / search invalidation: only on the same two conditions
    /// `feed()` uses — scrollback eviction (capacity rollover) or a hard
    /// `Reset` delta. Every other delta variant (`Cells`, `Cursor`,
    /// `ScreenSwitch`, `Resize`, semantic events, `ModeChange`,
    /// `ScrollbackAppend` below capacity) leaves abs-row anchors valid,
    /// so the user's drag-selection survives the high-frequency TUI
    /// redraws Claude Code / htop / vim / less emit (the same
    /// "TUI 一直刷新无法选中复制" symptom the feed() path fixed in §B.2,
    /// rebroken by the unconditional clear that originally lived here
    /// when the rust-parser backend landed in P3.6).
    #[wasm_bindgen(js_name = applyDeltaFrame)]
    pub fn apply_delta_frame(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let frame = crate::term::delta::decode_frame(bytes)
            .map_err(|e| JsValue::from_str(&format!("delta decode: {e}")))?;
        let evictions_before = self.inner.scrollback_eviction_count();
        let has_reset = frame
            .deltas
            .iter()
            .any(|d| matches!(d, crate::term::delta::GridDelta::Reset));
        self.inner
            .apply_frame(&frame)
            .map_err(|v| JsValue::from_str(&format!("protocol version {v} not supported")))?;
        let evictions_after = self.inner.scrollback_eviction_count();
        if has_reset || evictions_after != evictions_before {
            self.selection.clear();
            self.search.clear();
        }
        Ok(())
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

    /// Programmatically set a selection range in **absolute-row coords**
    /// (see `selection.rs` module docstring). The JS-side drag state
    /// machine in `manager.ts` stores its anchor / focus as `abs_row =
    /// vp_row + scroll_offset` so the selection survives scroll without
    /// the caller having to re-translate every sync — this entry point
    /// lets it forward those abs values directly. Skips the vp→abs
    /// conversion that `set_selection` does internally, so it's safe to
    /// call repeatedly during a drag that scrolls the viewport.
    #[wasm_bindgen(js_name = setSelectionAbs)]
    pub fn set_selection_abs(
        &mut self,
        start_abs_row: usize,
        start_col: usize,
        end_abs_row: usize,
        end_col: usize,
    ) {
        self.selection.set_abs(RangeAbs {
            start_abs_row,
            start_col,
            end_abs_row,
            end_col,
        });
    }

    #[wasm_bindgen(js_name = getSelectionText)]
    pub fn get_selection_text(&self) -> String {
        self.selection.text(&self.inner)
    }

    #[wasm_bindgen(js_name = hasSelection)]
    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    /// E2E-only — build a postcard-encoded `DeltaFrame` carrying a single
    /// `Cursor` delta with the supplied coordinates and a default block
    /// shape. Lets `tests/e2e-shell/` exercise the `applyDeltaFrame` path
    /// without spinning up a real shell or hand-rolling the postcard
    /// schema. No state mutation; returns bytes ready to feed back into
    /// `applyDeltaFrame`.
    ///
    /// `pane_seq` is opaque to the mirror (used only for diagnostics) —
    /// callers can pass any monotonically increasing u32. We accept a
    /// narrower `u32` than the field's underlying `u64` because JS
    /// numbers lose precision past 2^53 and tests never need values
    /// anywhere near that range.
    #[wasm_bindgen(js_name = e2eEncodeCursorDeltaFrame)]
    pub fn e2e_encode_cursor_delta_frame(&self, pane_seq: u32, row: u16, col: u16) -> Vec<u8> {
        use crate::term::delta::{
            CursorShape as DeltaCursorShape, DeltaFrame, GridDelta, encode_frame,
        };
        let frame = DeltaFrame::new(
            pane_seq as u64,
            vec![GridDelta::Cursor {
                row,
                col,
                visible: true,
                blink: true,
                shape: DeltaCursorShape::Block,
            }],
        );
        encode_frame(&frame).unwrap_or_default()
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

    /// §1.35 — force-leave alt screen on the kernel side when the PTY
    /// process exits while a TUI is still in alt screen mode. Without
    /// this the new shell spawned by `pane-pty-closed` would write into
    /// the alt buffer, hiding the primary screen content from the user.
    #[wasm_bindgen(js_name = leaveAltScreen)]
    pub fn leave_alt_screen(&mut self) {
        self.inner.leave_alt_screen();
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
    /// Also read by `manager.ts::isInlineTuiActive` for keyboard/mouse
    /// priority routing — see also `isMouseReporting`.
    #[wasm_bindgen(js_name = isInlineTuiMode)]
    pub fn is_inline_tui_mode(&self) -> bool {
        self.inner.is_inline_tui_mode_at(js_sys::Date::now() as i64)
    }

    /// Called from `manager.ts::handleKeyDown` immediately after the
    /// user sends Ctrl+C (ETX `\x03`) through the data handler. Arms
    /// the inline-TUI heuristic's grace window so subsequent PSReadLine
    /// CHA `\x1b[G` emits don't keep the pane stuck in "inline TUI
    /// mode" forever after the user killed the foreground TUI. See
    /// `Grid::is_inline_tui_active_at` for the full rationale.
    #[wasm_bindgen(js_name = noteCtrlCSent)]
    pub fn note_ctrl_c_sent(&mut self) {
        self.inner.note_ctrl_c_sent(js_sys::Date::now() as i64);
    }

    #[wasm_bindgen(js_name = isBracketedPaste)]
    pub fn is_bracketed_paste(&self) -> bool {
        self.inner.modes().bracketed_paste
    }

    #[wasm_bindgen(js_name = isAppCursorKeys)]
    pub fn is_app_cursor_keys(&self) -> bool {
        self.inner.modes().app_cursor_keys
    }

    /// §1.33 (2026-05-22) — hard gate for the shell-history popup
    /// feature. Returns `true` ONLY when the kernel is confident a
    /// normal shell prompt owns the input line on this pane; every
    /// known TUI signal short-circuits to `false`.
    ///
    /// Why this lives in WASM instead of the Svelte layer it used to
    /// be in (`src/lib/terminal/tuiGate.ts`):
    ///   - The JS `tuiGate` honoured the sticky window only while the
    ///     cursor was hidden. Claude Code's input prompt flips the
    ///     cursor visible BEFORE the inline-TUI heuristic decays, so
    ///     the user saw the popup hijack ArrowUp inside Claude.
    ///   - With the gate inside the kernel, we own a sticky timestamp
    ///     that bumps on every signal observation, independent of
    ///     cursor visibility — the JS layer can no longer race the
    ///     popup open between TUI frames.
    ///
    /// Decision order (any `false` wins immediately):
    ///   1. DECCKM `?1` (app_cursor_keys) — protocol-level "the app
    ///      owns the arrow keys" declaration. zsh+zle, bash+readline-
    ///      vi-mode, PSReadLine, Ink TUIs all set this.
    ///   2. Alt screen `?1049` / `?47` — full-screen TUI (vim, less,
    ///      htop) actively rendering.
    ///   3. Mouse reporting `?1000` / `?1002` / `?1003` — TUI tracks
    ///      mouse input, keyboard ownership goes with it.
    ///   4. Inline-TUI heuristic — Ink / log-update style apps that
    ///      hide the cursor + emit absolute-positioning CSIs within
    ///      the kernel's `INLINE_TUI_DECAY_MS` window.
    ///   5. Cursor hidden `?25l` — shell prompts always run with the
    ///      cursor visible; a hidden cursor is strong evidence a TUI
    ///      is mid-render or holding the screen between frames.
    ///   6. Sticky window — if any signal above was observed true
    ///      within `SHELL_HISTORY_STICKY_MS`, stay closed. Catches
    ///      the brief intra-frame "all signals false" windows that
    ///      let the JS-side gate leak before this method existed.
    ///
    /// Side effect: takes `&mut self` because the sticky timestamp is
    /// refreshed on every live-signal observation. Callers must hold
    /// a mutable handle to the kernel (they always do — wasm-bindgen
    /// generates `&mut self` on the JS side too).
    #[wasm_bindgen(js_name = shouldAllowShellHistory)]
    pub fn should_allow_shell_history(&mut self) -> bool {
        self.should_allow_shell_history_at(js_sys::Date::now() as i64)
    }

    /// Test-driveable variant of `should_allow_shell_history` — same
    /// logic but takes `now_ms` explicitly so native cargo tests can
    /// step the clock without `js_sys::Date::now()`. The wasm-exposed
    /// `should_allow_shell_history` is the only caller in production.
    pub(crate) fn should_allow_shell_history_at(&mut self, now_ms: i64) -> bool {
        let m = self.inner.modes();

        let live_tui = m.app_cursor_keys
            || self.inner.is_alt_screen()
            || m.mouse_normal
            || m.mouse_button_event
            || m.mouse_any_event
            || self.inner.is_inline_tui_mode_at(now_ms)
            || !m.cursor_visible;

        if live_tui {
            self.last_tui_signal_at_ms = now_ms;
            return false;
        }

        // §1.33 — sticky reference is the MAX of:
        //   - this gate's own bump (set above whenever JS queried during
        //     a live signal), and
        //   - the parser-side bump (set inside `grid.last_tui_signal_at_ms`
        //     whenever a TUI mode flipped active during a feed).
        // The parser-side bump is what catches signals JS never had a
        // chance to query — e.g. Claude Code flipping `?1049h` mid-frame
        // without the user pressing an arrow key at that moment. Take
        // the larger of the two so a fresher gate-time bump is still
        // honoured if the parser's was older.
        let sticky_ref = self
            .last_tui_signal_at_ms
            .max(self.inner.grid().last_tui_signal_at_ms());

        if sticky_ref > 0 && now_ms.saturating_sub(sticky_ref) < SHELL_HISTORY_STICKY_MS {
            return false;
        }

        true
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

    // ---- mouse mode queries -----------------------------------------

    /// Returns true when any DEC mouse reporting mode is active
    /// (?1000 normal, ?1002 button-event, or ?1003 any-event).
    #[wasm_bindgen(js_name = isMouseReporting)]
    pub fn is_mouse_reporting(&self) -> bool {
        let m = self.inner.modes();
        m.mouse_normal || m.mouse_button_event || m.mouse_any_event
    }

    /// Returns true when ?1002 (button-event / drag tracking) is active.
    #[wasm_bindgen(js_name = isMouseButtonEvent)]
    pub fn is_mouse_button_event(&self) -> bool {
        self.inner.modes().mouse_button_event
    }

    /// Returns true when ?1003 (any-event / motion tracking) is active.
    #[wasm_bindgen(js_name = isMouseAnyEvent)]
    pub fn is_mouse_any_event(&self) -> bool {
        self.inner.modes().mouse_any_event
    }

    /// Returns true when ?1006 (SGR mouse encoding) is active.
    #[wasm_bindgen(js_name = isMouseSgr)]
    pub fn is_mouse_sgr(&self) -> bool {
        self.inner.modes().mouse_sgr
    }

    /// Single-call bitmask of every DEC mouse mode the caller cares
    /// about. Eliminates the 3-4 separate wasm boundary crossings the
    /// JS pointer handlers used to make per pointermove event:
    ///
    ///   bit 0 (0x1) = ?1000 (mouse_normal)
    ///   bit 1 (0x2) = ?1002 (button_event / drag tracking)
    ///   bit 2 (0x4) = ?1003 (any_event / all motion)
    ///   bit 3 (0x8) = ?1006 (SGR encoding)
    ///
    /// `bits != 0` <=> `isMouseReporting() == true`. The individual
    /// boolean getters above are kept for non-hot-path callers.
    #[wasm_bindgen(js_name = mouseReportingModes)]
    pub fn mouse_reporting_modes(&self) -> u32 {
        let m = self.inner.modes();
        let mut bits = 0u32;
        if m.mouse_normal {
            bits |= 1;
        }
        if m.mouse_button_event {
            bits |= 2;
        }
        if m.mouse_any_event {
            bits |= 4;
        }
        if m.mouse_sgr {
            bits |= 8;
        }
        bits
    }

    /// Encode a mouse event as an SGR terminal sequence. Delegates to
    /// `input::encode_mouse` which generates `ESC [ < btn ; col ; row [Mm]`
    /// per xterm SGR spec (column first, then row).
    /// Always uses SGR format regardless of ?1006 state — the terminal
    /// decodes both; SGR is simpler and doesn't overflow at high row/col.
    #[wasm_bindgen(js_name = encodeMouse)]
    pub fn encode_mouse(
        &self,
        row: usize,
        col: usize,
        button: u8,
        action: u8,
        shift: bool,
        ctrl: bool,
        alt: bool,
    ) -> Vec<u8> {
        crate::input::encode_mouse(button, row, col, action, shift, ctrl, alt, self.inner.modes())
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

#[cfg(test)]
mod delta_selection_tests {
    //! Regression coverage for `apply_delta_frame`'s selection guard.
    //!
    //! Locks down the §B.2 invariant for the rust-parser delta path:
    //! ordinary repaint frames (Cells / Cursor / ModeChange / non-evicting
    //! ScrollbackAppend) MUST NOT clear the active selection. Only
    //! scrollback eviction or a hard `Reset` may invalidate anchors.
    //!
    //! The bug this test catches: in P3.6 the wasm `applyDeltaFrame`
    //! cleared selection unconditionally on every applied frame. With the
    //! rust parser backend as the default (P3.7), Claude Code / htop / vim
    //! / less emit ~30+ frames/s in active panes, so every drag-select was
    //! erased one frame after pointerdown — the visible "selection
    //! flickers, can't copy text from a refreshing TUI" symptom the user
    //! re-reported on 2026-05-21.
    use super::*;
    use crate::term::delta::{
        CursorShape as DeltaCursorShape, DeltaCell, DeltaFrame, GridDelta, encode_frame,
    };

    /// Drive a typical TUI repaint into a fresh kernel, set a host
    /// selection over a known string, slam many no-op delta frames in
    /// (mimicking Claude's per-frame cursor blink), assert the
    /// selection text is unchanged.
    #[test]
    fn apply_delta_frame_preserves_selection_across_repaints() {
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"hello world\r\n");
        // Select "hello" — abs row = scrollback_len() + 0 (live grid).
        // Selection range is end-exclusive, so cols [0, 5) covers
        // the five chars 'h','e','l','l','o'.
        let sb = t.inner.scrollback_len();
        t.set_selection_abs(sb, 0, sb, 5);
        assert!(t.has_selection(), "precondition: selection set");
        assert_eq!(t.get_selection_text(), "hello");

        let cursor_frame = encode_frame(&DeltaFrame::new(
            0,
            vec![GridDelta::Cursor {
                row: 0,
                col: 12,
                visible: true,
                blink: true,
                shape: DeltaCursorShape::Block,
            }],
        ))
        .expect("encode cursor frame");
        let cells_frame = encode_frame(&DeltaFrame::new(
            1,
            vec![GridDelta::Cells {
                row: 1,
                col: 0,
                cells: (b"redraw-line".iter())
                    .map(|&b| {
                        let mut c = DeltaCell::blank();
                        c.ch = b as char;
                        c
                    })
                    .collect(),
            }],
        ))
        .expect("encode cells frame");

        // 30 repaint frames — matches the per-second blink burst the
        // user reported as the trigger. Pre-fix this would clear
        // selection on iteration 1.
        for _ in 0..30 {
            t.apply_delta_frame(&cursor_frame).unwrap();
            t.apply_delta_frame(&cells_frame).unwrap();
        }

        assert!(
            t.has_selection(),
            "selection must survive 30 redraw frames (had cleared on iter 1 before §B.2 follow-up)"
        );
        assert_eq!(
            t.get_selection_text(),
            "hello",
            "selection content must be byte-identical after redraw storm"
        );
    }

    #[test]
    fn apply_delta_frame_clears_selection_on_reset_delta() {
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"hello world\r\n");
        let sb = t.inner.scrollback_len();
        t.set_selection_abs(sb, 0, sb, 5);
        assert!(t.has_selection());

        let reset_frame =
            encode_frame(&DeltaFrame::new(0, vec![GridDelta::Reset])).expect("encode reset frame");
        t.apply_delta_frame(&reset_frame).unwrap();
        assert!(
            !t.has_selection(),
            "Reset delta MUST drop selection — abs anchors point to wiped cells"
        );
    }

    #[test]
    fn apply_delta_frame_clears_selection_on_scrollback_eviction() {
        // Tiny scrollback so we can trigger eviction with a single
        // ScrollbackAppend that overflows capacity.
        let mut t = JsTerminal::new(4, 8, 2);
        // Push two rows into scrollback, then select something in the
        // live grid (abs anchor sits above sb_len, stable target).
        t.feed(b"row0\r\nrow1\r\n");
        let sb_before = t.inner.scrollback_len();
        t.set_selection_abs(sb_before, 0, sb_before, 3);
        assert!(t.has_selection());
        let evictions_before = t.inner.scrollback_eviction_count();

        // Append more lines than capacity → at least one eviction.
        let lines: Vec<Vec<DeltaCell>> = (0..4)
            .map(|_| {
                let mut row = Vec::new();
                for _ in 0..4 {
                    row.push(DeltaCell::blank());
                }
                row
            })
            .collect();
        let frame = encode_frame(&DeltaFrame::new(0, vec![GridDelta::ScrollbackAppend { lines }]))
            .expect("encode append");
        t.apply_delta_frame(&frame).unwrap();

        assert!(
            t.inner.scrollback_eviction_count() > evictions_before,
            "precondition: append must have evicted at least one row"
        );
        assert!(
            !t.has_selection(),
            "eviction MUST drop selection — abs anchors point to evicted content"
        );
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
    use web_sys::{HtmlCanvasElement, OffscreenCanvas};

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

        /// §p4.9 (2026-05-22) — worker-thread constructor.
        ///
        /// JS bridge: `RenderHandle.newFromOffscreen(offscreenCanvas)`.
        /// Called from `renderWorker.ts::loadKernelAdapter` after the
        /// host transferred a canvas via
        /// `canvas.transferControlToOffscreen()` + postMessage. The
        /// `OffscreenCanvas` here is the same object the worker side
        /// receives in the `bindCanvas` request.
        ///
        /// Canvas2D-only — the WebGPU-first branch is reserved for the
        /// main-thread `newWithWebgpuFirst` because the WebGPU surface
        /// host needs DOM access (window-level GPU adapter / device).
        /// On the worker path we paint via Canvas2D, which is fully
        /// available inside a DedicatedWorker since 2018.
        #[wasm_bindgen(js_name = newFromOffscreen)]
        pub fn new_from_offscreen(canvas: OffscreenCanvas) -> Result<RenderHandle, JsValue> {
            let backend = Canvas2dBackend::new_from_offscreen(canvas).map_err(JsValue::from)?;
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
                match crate::render::webgpu::WebGpuPaneBackend::new(handle.host_rc()).await {
                    Ok(b) => {
                        web_sys::console::log_1(&"[ridge] WebGPU backend OK".into());
                        let metrics = FrameMetrics {
                            cell_w: 8.0,
                            cell_h: 16.0,
                            dpr: 1.0,
                            tui_mode: false,
                        };
                        let renderer = Renderer::new(
                            AnyBackend::Webgpu(b),
                            metrics,
                            Theme::default_dark(),
                        );
                        return Ok(RenderHandle { renderer });
                    }
                    Err(e) => {
                        web_sys::console::log_1(
                            &format!("[ridge] WebGPU backend failed: {e:?}").into(),
                        );
                    }
                }
            } else {
                web_sys::console::log_1(
                    &"[ridge] surface_host is None — attachHost never completed".into(),
                );
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

        /// §atlas-pin: before this frame's panes full-render, pin every
        /// visible cached pane's glyph layers so another pane's glyph
        /// admission can't evict + overwrite a layer this pane's
        /// `recordCachedOnly` replay still samples. Caller: `manager.ts`
        /// host loop, right after the host frame opens.
        #[wasm_bindgen(js_name = pinCachedLayers)]
        pub fn pin_cached_layers(&mut self) {
            self.renderer.pin_cached_layers();
        }

        /// Multi-pane hosts call this when the active pane changes. When
        /// `focused` is false, the renderer skips cursor draw entirely so
        /// only the truly active terminal blinks. Idempotent.
        #[wasm_bindgen(js_name = setFocused)]
        pub fn set_focused(&mut self, focused: bool) {
            self.renderer.set_focused(focused);
        }

        /// Install an IME preedit overlay at the given cell. The renderer
        /// will paint `text` on top of the cell grid each frame until
        /// `clearPreedit` is called. Cells themselves are NOT modified,
        /// so a TUI re-rendering into the overlay's row mid-composition
        /// can't corrupt the preedit, and the preedit can't corrupt the
        /// TUI's rendered cells. JS calls this on `compositionupdate`.
        #[wasm_bindgen(js_name = setPreedit)]
        pub fn set_preedit(&mut self, text: &str, row: usize, col: usize) {
            self.renderer.set_preedit(text.to_string(), row, col);
        }

        /// Remove the preedit overlay (JS calls on `compositionend` after
        /// shipping the committed string to the PTY).
        #[wasm_bindgen(js_name = clearPreedit)]
        pub fn clear_preedit(&mut self) {
            self.renderer.clear_preedit();
        }

        /// §1.34 (2026-05-22) — install the shell-history popup overlay.
        /// `items` is the JS-pre-windowed VISIBLE slice (filtered, newest
        /// first). `selected_index` is `-1` for "no row picked" or a
        /// slice-relative `0..items.len()-1`. `(anchor_row, anchor_col)`
        /// is the input anchor in viewport cell coords; `place_above`
        /// chooses growth direction.
        ///
        /// §history-scroll — `total_items` is the FULL filtered count and
        /// `first_visible` is the index of `items[0]` within it; the
        /// renderer draws a scrollbar thumb from these when the list is
        /// longer than the window (Warp-style: many entries reachable by
        /// scrolling, with a position indicator). JS owns the windowing so
        /// the renderer just paints the slice + the thumb.
        #[wasm_bindgen(js_name = setHistoryOverlay)]
        pub fn set_history_overlay(
            &mut self,
            items: js_sys::Array,
            selected_index: i32,
            anchor_row: u32,
            anchor_col: u32,
            place_above: bool,
            total_items: u32,
            first_visible: u32,
        ) {
            let items: Vec<String> = items.iter().filter_map(|v| v.as_string()).collect();
            // JS pre-windows to the visible slice; render all of it (capped
            // to a sane ceiling as a floor-to-ceiling guard).
            let max_visible_rows = items.len().min(40);
            self.renderer
                .set_history_overlay(crate::render::renderer::HistoryOverlay {
                    items,
                    selected_index,
                    anchor_row: anchor_row as usize,
                    anchor_col: anchor_col as usize,
                    place_above,
                    max_visible_rows,
                    total_items: total_items as usize,
                    first_visible: first_visible as usize,
                });
        }

        /// §1.34 — remove the history overlay (Enter / ArrowRight / Esc).
        #[wasm_bindgen(js_name = clearHistoryOverlay)]
        pub fn clear_history_overlay(&mut self) {
            self.renderer.clear_history_overlay();
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

        /// Diagnostic: return the kernel-side renderer.theme.{bg, fg,
        /// cursor_color, tui_bg} as a Uint8Array of 16 bytes (4×RGBA).
        /// Lets JS confirm whether `applyTheme` actually propagated into
        /// the renderer state — the JS-side `opts.theme` snapshot only
        /// proves the manager *received* the theme, not that the
        /// wasm renderer accepted it. Cheap (one Theme clone, 16 bytes
        /// copied) so callers may poll without harm.
        #[wasm_bindgen(js_name = currentThemeProbe)]
        pub fn current_theme_probe(&self) -> Box<[u8]> {
            let t = self.current_theme();
            let mut out = Vec::with_capacity(16);
            out.extend_from_slice(&t.bg);
            out.extend_from_slice(&t.fg);
            out.extend_from_slice(&t.cursor_color);
            out.extend_from_slice(&t.tui_bg);
            out.into_boxed_slice()
        }

        /// Return the active rendering backend name: `"WebGPU"` or `"Canvas2D"`.
        /// Used by the remote page to show a small indicator badge.
        #[wasm_bindgen(js_name = backendName)]
        pub fn backend_name(&self) -> String {
            match self.renderer.backend() {
                AnyBackend::Canvas2d(_) => "Canvas2D".to_string(),
                #[cfg(feature = "webgpu")]
                AnyBackend::Webgpu(_) => "WebGPU".to_string(),
            }
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

#[cfg(test)]
mod shell_history_gate_tests {
    //! §1.33 (2026-05-22) — regression coverage for the shell-history
    //! popup gate. The previous JS-side gate (`tuiGate.ts`) was leaking
    //! ArrowUp/ArrowDown into Claude Code because the sticky branch was
    //! conditioned on `!cursorVisible` — once a TUI flashed the cursor
    //! visible between frames the gate opened, and the popup hijacked
    //! the next ArrowUp before the next TUI signal landed. The kernel-
    //! side gate added here closes that race by:
    //!   1. blocking on EVERY TUI signal (including cursor-hidden
    //!      treated as TUI-active, not just as a sticky-precondition),
    //!   2. holding sticky for `SHELL_HISTORY_STICKY_MS` regardless of
    //!      current cursor state.
    //!
    //! Tests use the native `should_allow_shell_history_at(now_ms)` so
    //! they don't need `js_sys::Date::now()` or a browser harness; the
    //! production wasm-exposed method is a thin wrapper.

    use super::*;

    /// The parser bumps `grid.last_tui_signal_at_ms` using the real
    /// wall clock (`clock::now_ms()`), so unit tests must derive their
    /// synthetic `now` from the same clock to keep sticky comparisons
    /// honest. `clock_baseline()` snapshots the clock right after the
    /// test's last TUI-activating feed; subsequent assertions use
    /// `baseline + offset_ms` so the sticky window straddles a
    /// deterministic offset regardless of how slow CI is.
    fn clock_baseline() -> i64 {
        crate::term::clock::now_ms()
    }

    #[test]
    fn allows_history_on_fresh_shell_prompt() {
        // Default modes: cursor visible, no DECCKM, no alt screen, no
        // mouse reporting, no inline-TUI activity → popup permitted.
        let mut t = JsTerminal::new(24, 80, 200);
        assert!(t.should_allow_shell_history_at(clock_baseline()));
    }

    #[test]
    fn blocks_when_app_cursor_keys_set() {
        // DECCKM (`?1h`) → app owns arrows; no sticky needed since the
        // mode is persistent until the app clears it.
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[?1h");
        assert!(!t.should_allow_shell_history_at(clock_baseline()));
    }

    #[test]
    fn blocks_when_alt_screen_active() {
        // `?1049h` swaps to alt screen — vim/less/htop convention.
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[?1049h");
        assert!(!t.should_allow_shell_history_at(clock_baseline()));
    }

    #[test]
    fn blocks_when_mouse_reporting_active() {
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[?1000h");
        assert!(!t.should_allow_shell_history_at(clock_baseline()));
    }

    #[test]
    fn blocks_when_cursor_hidden() {
        // `?25l` — every full-screen and inline TUI hides the cursor
        // while rendering. Treat it as a live TUI signal so the popup
        // doesn't open between repaints.
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[?25l");
        assert!(!t.should_allow_shell_history_at(clock_baseline()));
    }

    #[test]
    fn opens_immediately_after_tui_signal_clears() {
        // §1.35 — SHELL_HISTORY_STICKY_MS = 0, so the gate opens
        // immediately as soon as every live signal is false. A TUI
        // that flickered a signal on/off (e.g. `?25l?25h`) must NOT
        // gate the popup once the cursor is visible again.
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[?25l\x1b[?25h"); // hide and re-show in one chunk
        let baseline = clock_baseline();
        // With sticky=0 the gate must open immediately — no extra
        // buffer after the last signal clears.
        assert!(
            t.should_allow_shell_history_at(baseline + 100),
            "gate must open immediately after TUI signal clears (sticky=0)",
        );
    }

    #[test]
    fn gate_stays_closed_while_live_signal_holds() {
        // Even with sticky=0, the gate must stay closed while a live
        // signal is still asserted. This was previously covered by
        // the sticky-refresh test — now we verify the live-signal
        // branch directly with repeated queries.
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[?25l");
        let baseline = clock_baseline();
        assert!(!t.should_allow_shell_history_at(baseline));
        // Still hidden 5 s later — still blocked (live signal).
        assert!(
            !t.should_allow_shell_history_at(baseline + 5_000),
            "live cursor-hidden must block regardless of sticky window",
        );
        // Show cursor → gate opens immediately.
        t.feed(b"\x1b[?25h");
        assert!(
            t.should_allow_shell_history_at(baseline + 5_100),
            "gate must open immediately once cursor is visible",
        );
    }

    #[test]
    fn allows_history_even_when_inline_tui_csi_is_stale() {
        // Once a real shell prompt has been up for >> sticky window
        // with no further TUI activity AND cursor is visible, the gate
        // must permit the popup. Without this assertion a regression
        // that locked sticky permanently after the first TUI use would
        // pass the earlier negative tests but break daily use.
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"\x1b[H");                    // CUP — abs-positioning CSI
        t.feed(b"\x1b[?25l");                 // hide cursor (TUI active)
        let baseline = clock_baseline();
        assert!(!t.should_allow_shell_history_at(baseline));
        t.feed(b"\x1b[?25h");                 // back to shell
        // Far past both sticky and inline-TUI decay.
        assert!(t.should_allow_shell_history_at(baseline + 10_000));
    }
}
