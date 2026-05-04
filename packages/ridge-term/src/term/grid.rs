//! The Grid: visible rows + cursor + scrollback, with alt screen and
//! scroll region (DECSTBM) support.
//!
//! ## Coordinate system
//! Row 0 = top, col 0 = left. Same as xterm/VT.
//!
//! ## Two screens
//! `Grid` owns a *primary* and an *alternate* screen. `is_alt` selects
//! which one is active for rendering and parser ops. Switching screens
//! does NOT touch the inactive screen's contents — that's how vim/less
//! preserve the underlying shell view.
//!
//! ### Important property: scrollback only follows the primary screen
//! When the alt screen scrolls, lines are dropped on the floor — they do
//! NOT enter the scrollback ring. Otherwise `vim` would pollute history
//! with thousands of viewport-replays. Tested across xterm, VTE, kitty,
//! alacritty — universal behavior.
//!
//! ## Scroll region (DECSTBM)
//! `scroll_top` / `scroll_bottom` are 0-based inclusive row indices that
//! constrain the scroll *region* used by LF at the bottom, IND, RI, SU,
//! SD, IL, DL. Default = full screen.
//!
//! ### Scroll region + scrollback interaction
//! Lines scrolled out of a *partial* scroll region (e.g. less shows a
//! status line at the bottom and scrolls only rows 0..rows-2) do NOT
//! enter scrollback either. Only full-screen scrolls (`top=0,
//! bottom=rows-1`) push to scrollback. This matches xterm — and is why
//! `less +F` doesn't fill your scrollback while tailing a log.

use super::attr_table::{AttrId, AttrTable};
use super::attrs::Attrs;
use super::cell::{Cell, Row};
use super::cursor::{Cursor, SavedCursor};
use super::scrollback::Scrollback;
use super::wcwidth::wcwidth;

/// Erase-in-display modes (CSI J).
#[derive(Debug, Clone, Copy)]
pub enum EraseMode {
    /// 0: from cursor to end.
    Below,
    /// 1: from start to cursor.
    Above,
    /// 2: entire screen.
    All,
}

/// One screen buffer. Primary and alt are both `Screen`; `Grid` switches
/// the active one. Each screen carries its own cursor + saved cursor +
/// scroll region — switching to alt resets none of those, mirroring xterm.
pub struct Screen {
    rows: Vec<Row>,
    pub cursor: Cursor,
    pub saved_cursor: Option<SavedCursor>,
    /// Top of the scroll region, 0-based inclusive. Default 0.
    pub scroll_top: usize,
    /// Bottom of the scroll region, 0-based inclusive. Default rows-1.
    pub scroll_bottom: usize,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: (0..rows).map(|_| Row::new(cols)).collect(),
            cursor: Cursor::default(),
            saved_cursor: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
        }
    }

    /// Whether the scroll region currently covers the entire screen.
    /// Used to decide if scrolled-off rows should enter scrollback.
    fn is_full_region(&self) -> bool {
        self.scroll_top == 0 && self.scroll_bottom + 1 == self.rows.len()
    }
}

pub struct Grid {
    rows: usize,
    cols: usize,
    primary: Screen,
    alt: Screen,
    /// `false` = primary is active, `true` = alt is active.
    is_alt: bool,
    pub attrs: AttrTable,
    pub scrollback: Scrollback,
}

impl Grid {
    pub fn new(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        Self {
            rows,
            cols,
            primary: Screen::new(rows, cols),
            alt: Screen::new(rows, cols),
            is_alt: false,
            attrs: AttrTable::default(),
            scrollback: Scrollback::new(scrollback_lines),
        }
    }

    pub fn rows(&self) -> usize { self.rows }
    pub fn cols(&self) -> usize { self.cols }
    pub fn is_alt_screen(&self) -> bool { self.is_alt }
    /// Top of the scroll region on the active screen, 0-based inclusive.
    /// Used by the parser to apply DECOM (?6 origin mode) offsets to CUP
    /// and VPA: when origin mode is on, `H`/`f`/`d` are interpreted
    /// relative to this row instead of the screen top.
    pub fn scroll_top(&self) -> usize { self.screen().scroll_top }
    /// Bottom of the scroll region on the active screen, 0-based
    /// inclusive. Used together with `scroll_top()` to clamp DECOM-mode
    /// cursor positioning.
    pub fn scroll_bottom(&self) -> usize { self.screen().scroll_bottom }

    fn screen(&self) -> &Screen {
        if self.is_alt { &self.alt } else { &self.primary }
    }
    fn screen_mut(&mut self) -> &mut Screen {
        if self.is_alt { &mut self.alt } else { &mut self.primary }
    }

    pub fn cursor(&self) -> &Cursor { &self.screen().cursor }
    pub fn cursor_mut(&mut self) -> &mut Cursor { &mut self.screen_mut().cursor }
    pub fn saved_cursor_mut(&mut self) -> &mut Option<SavedCursor> {
        &mut self.screen_mut().saved_cursor
    }

    pub fn row(&self, idx: usize) -> Option<&Row> {
        self.screen().rows.get(idx)
    }

    /// Switch to alt screen (DECSET 1049 / 47 / 1047). Idempotent.
    /// `clear_on_enter` corresponds to the `1049` variant: clear the alt
    /// screen on entry so we get a fresh blank canvas for fullscreen apps.
    pub fn enter_alt_screen(&mut self, clear_on_enter: bool) {
        if self.is_alt { return; }
        self.is_alt = true;
        if clear_on_enter {
            for r in &mut self.alt.rows { r.clear(); }
            self.alt.cursor = Cursor::default();
            self.alt.scroll_top = 0;
            self.alt.scroll_bottom = self.rows.saturating_sub(1);
        }
    }

    /// Leave alt screen (DECRST 1049 / 47 / 1047). Idempotent.
    pub fn leave_alt_screen(&mut self) {
        if !self.is_alt { return; }
        self.is_alt = false;
    }

    /// CSI ? r  — set scroll region. 1-based-on-the-wire bounds clamped
    /// internally to 0-based inclusive. Empty/default args = full screen.
    /// xterm also moves cursor to (0,0) on STBM, so we do too.
    pub fn set_scroll_region(&mut self, top_1based: Option<usize>, bottom_1based: Option<usize>) {
        let last = self.rows.saturating_sub(1);
        let top = top_1based.map(|v| v.saturating_sub(1)).unwrap_or(0).min(last);
        let bottom = bottom_1based.map(|v| v.saturating_sub(1)).unwrap_or(last).min(last);
        if top >= bottom {
            // Invalid region — silently fall back to full screen, like xterm.
            let scr = self.screen_mut();
            scr.scroll_top = 0;
            scr.scroll_bottom = last;
        } else {
            let scr = self.screen_mut();
            scr.scroll_top = top;
            scr.scroll_bottom = bottom;
        }
        self.cursor_to(0, 0);
    }

    /// Resize. Primary screen reflows on column change (Phase 1 — see
    /// `reflow_primary` below; design notes in OVERVIEW.md §7 / TASKS §2.3).
    /// Alt screen and rows-only changes keep the naive truncate/pad path
    /// because alt-screen TUIs redraw on SIGWINCH anyway, and re-wrapping at
    /// the same column count is a no-op. Phase 2 (scrollback reflow +
    /// selection / hyperlink anchor migration) is still deferred — long lines
    /// already in scrollback show with the old column width when scrolled
    /// into view.
    ///
    /// Scroll-region preservation rule: if the region was the default
    /// full screen before resize (top=0, bottom=rows-1), extend it to
    /// match the new size. Otherwise it's a custom DECSTBM range — clamp
    /// to the new bounds and revert to full if the clamp would invalidate.
    /// Without this, a kernel created at 24 rows then resized to 26 keeps
    /// scroll_bottom=23, leaving rows 24..25 as a frozen footer; LF at the
    /// real bottom never scrolls and scrollback never grows.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        let cols_changed = cols != self.cols;

        // Primary screen: reflow when columns change (preserves wrapped lines,
        // see OVERVIEW.md §7 for design). Rows-only change keeps the naive
        // truncate/pad path because re-wrapping at the same column count is a
        // no-op.
        if cols_changed {
            self.reflow_primary(rows, cols);
        } else {
            Self::naive_resize_screen(&mut self.primary, rows, cols);
        }

        // Alt screen: always naive truncate/pad. TUIs (vim/less/htop) own
        // their alt-screen redraw via SIGWINCH; reflowing under them would
        // smear the half-drawn frame they're about to overwrite anyway.
        Self::naive_resize_screen(&mut self.alt, rows, cols);

        self.rows = rows;
        self.cols = cols;
    }

    /// Existing truncate/pad behavior, factored out so `resize()` can pick
    /// per-screen behavior. Used by alt screen unconditionally and by primary
    /// when only the row count changed.
    fn naive_resize_screen(screen: &mut Screen, rows: usize, cols: usize) {
        let old_last = screen.rows.len().saturating_sub(1);
        let region_was_full = screen.scroll_top == 0 && screen.scroll_bottom == old_last;

        for r in &mut screen.rows {
            r.resize(cols);
        }
        if rows < screen.rows.len() {
            screen.rows.truncate(rows);
        } else {
            while screen.rows.len() < rows {
                screen.rows.push(Row::new(cols));
            }
        }
        let last = rows.saturating_sub(1);
        screen.cursor.row = screen.cursor.row.min(last);
        screen.cursor.col = screen.cursor.col.min(cols.saturating_sub(1));
        screen.cursor.pending_wrap = false;

        if region_was_full {
            screen.scroll_top = 0;
            screen.scroll_bottom = last;
        } else {
            screen.scroll_top = screen.scroll_top.min(last);
            screen.scroll_bottom = screen.scroll_bottom.min(last);
            if screen.scroll_top >= screen.scroll_bottom {
                screen.scroll_top = 0;
                screen.scroll_bottom = last;
            }
        }
    }

    /// Re-wrap the primary screen for a new column count.
    ///
    /// Algorithm (see OVERVIEW.md §7.5):
    ///   1. Find `last_content_row` = max(highest row with non-blank cells,
    ///      cursor row). Rows below this are unused "future" buffer.
    ///   2. Stitch wrapped chains into logical lines. While stitching,
    ///      record the cursor's logical offset (which line + offset within).
    ///      Trim trailing blank padding from each logical line.
    ///   3. Re-wrap each logical line to `new_cols`, setting `wrapped=true`
    ///      on intermediate breaks. Empty logical lines become one blank row.
    ///   4. Place cursor at its tracked logical offset.
    ///   5. If overflow (>new_rows), push oldest rows to scrollback and
    ///      shift the cursor up by the same amount.
    ///   6. If underflow (<new_rows), pad the bottom with blank rows.
    ///
    /// Phase 1 limitations (TASKS §2.3): per-row hyperlink spans are dropped
    /// — they regenerate naturally on next print. Selection clears via the
    /// caller (kernel-level `JsTerminal::resize` already calls
    /// `selection.clear()`). Scrollback stays at original width — paging
    /// up after reflow shows historical content at its old wrapping.
    fn reflow_primary(&mut self, new_rows: usize, new_cols: usize) {
        if new_cols == 0 || new_rows == 0 {
            // Pathological dimensions — fall back to naive resize.
            Self::naive_resize_screen(&mut self.primary, new_rows, new_cols);
            return;
        }

        let cursor_src_row = self.primary.cursor.row;
        let cursor_src_col = self.primary.cursor.col;
        // pending_wrap conceptually parks the cursor "one past" col cols-1
        // — print() resolves it by linefeed-then-write. Reflow needs to
        // see that virtual past-end position too, otherwise a cursor at
        // (last_col, pending_wrap=true) gets mapped to a mid-row position
        // in the new layout instead of end-of-line, and the right-edge
        // wrap semantic is lost. (TASKS §1.10.)
        let cursor_src_pending_wrap = self.primary.cursor.pending_wrap;

        // Step 1: find last row that has content (or contains the cursor).
        let last_with_content = self
            .primary
            .rows
            .iter()
            .enumerate()
            .rev()
            .find(|(_, r)| r.cells.iter().any(|c| !c.is_blank()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let last_content_row = last_with_content
            .max(cursor_src_row)
            .min(self.primary.rows.len().saturating_sub(1));

        // Step 2: stitch into logical lines, tracking cursor.
        let mut logical_lines: Vec<Vec<Cell>> = Vec::new();
        let mut cursor_logical_idx: usize = 0;
        let mut cursor_logical_offset: usize = 0;
        let mut cursor_placed = false;
        let mut current: Vec<Cell> = Vec::new();

        for r_idx in 0..=last_content_row {
            let row = &self.primary.rows[r_idx];
            if r_idx == cursor_src_row {
                cursor_logical_idx = logical_lines.len();
                cursor_logical_offset = current.len() + cursor_src_col;
                if cursor_src_pending_wrap {
                    // Bump to the virtual "one past last col" position so
                    // the post-while end-of-line branch picks up the
                    // right-edge case below. May overshoot a now-trimmed
                    // line.len() if the anchor cell was a blank that
                    // got stripped — clamped at push time below.
                    cursor_logical_offset += 1;
                }
                cursor_placed = true;
            }
            current.extend_from_slice(&row.cells);
            // Continue stitching only while wrapped flag is set AND we have
            // more rows in our content range. Hard-stop at last_content_row.
            let line_continues = row.wrapped && r_idx < last_content_row;
            if !line_continues {
                // Trim trailing blank padding so a row that ended at col 5
                // doesn't carry 75 trailing spaces into the new wrap.
                while current.last().map(|c| c.is_blank()).unwrap_or(false) {
                    current.pop();
                }
                let pushed_idx = logical_lines.len();
                let pushed_len = current.len();
                logical_lines.push(std::mem::take(&mut current));
                // Clamp pending_wrap-bumped offset if the anchor cell
                // was trimmed (rare: cursor at last col with pending_wrap
                // and the just-printed char at last col was a blank).
                if pushed_idx == cursor_logical_idx
                    && cursor_logical_offset > pushed_len
                {
                    cursor_logical_offset = pushed_len;
                }
            }
        }
        // Edge case: cursor was below last_content_row entirely (e.g.,
        // freshly-cleared screen with cursor on row 10, all rows blank).
        if !cursor_placed {
            while logical_lines.len() <= cursor_src_row {
                logical_lines.push(Vec::new());
            }
            cursor_logical_idx = cursor_src_row;
            cursor_logical_offset = cursor_src_col;
        }

        // Step 3: re-wrap each logical line to new_cols.
        let mut out: Vec<Row> = Vec::new();
        let mut cursor_target_row: usize = 0;
        let mut cursor_target_col: usize = 0;
        // pending_wrap state for the relocated cursor. Defaults to false;
        // the end-of-line branch flips it to true when the cursor lands
        // on the last column of an exactly-filled row (used == 0 case),
        // matching what print() will see on the next character (it should
        // wrap to a new row, not overwrite the last cell).
        let mut cursor_pending_wrap = false;

        for (ll_idx, line) in logical_lines.iter().enumerate() {
            let on_this_line = ll_idx == cursor_logical_idx;
            if line.is_empty() {
                if on_this_line {
                    cursor_target_row = out.len();
                    cursor_target_col = cursor_logical_offset.min(new_cols - 1);
                }
                out.push(Row::new(new_cols));
                continue;
            }
            let mut start = 0;
            while start < line.len() {
                let mut end = (start + new_cols).min(line.len());
                // Wide-char split protection: if the would-be slice puts a
                // wide glyph's lead (width=2) at the LAST cell of this row
                // and its continuation half (width=0) at the START of the
                // next row, pull the slice back by 1 cell so the wide char
                // moves to the next row intact. The freed cell at the row's
                // end stays blank — same convention xterm uses when a wide
                // char doesn't fit at the right margin.
                //
                // Guards:
                //   `end < line.len()`     — only when actually wrapping.
                //   `end - start >= 2`     — degenerate `new_cols == 1` case
                //                            can't preserve wide chars; orphan
                //                            and accept the rendering glitch.
                if end < line.len()
                    && end - start >= 2
                    && line[end - 1].width == 2
                {
                    end -= 1;
                }
                let mut new_row = Row::new(new_cols);
                for (i, cell) in line[start..end].iter().enumerate() {
                    new_row.cells[i] = *cell;
                }
                new_row.wrapped = end < line.len();
                // Cursor placement: use `end` (post-pullback), not
                // `start + new_cols`. After a wide-char pullback, cursor
                // offsets in the freed cell belong to the NEXT row.
                if on_this_line
                    && cursor_logical_offset >= start
                    && cursor_logical_offset < end
                {
                    cursor_target_row = out.len();
                    cursor_target_col = (cursor_logical_offset - start).min(new_cols - 1);
                }
                out.push(new_row);
                start = end;
            }
            // Cursor at the very end of the line (offset == line.len()) lands
            // on the next column of the last emitted row, or wraps if at edge.
            //
            // Exact-boundary case (used == 0 && line.len() > 0): the line is
            // exactly k * new_cols cells long, so the last emitted row is
            // already FULL at width new_cols. Conceptually the cursor is at
            // "col 0 of an unborn (k+1)-th row". Place it at the last col of
            // the last row and set pending_wrap=true — matches print()'s
            // semantics: next character wraps to a new row instead of
            // overwriting cell (last_row, new_cols-1). Without pending_wrap,
            // the next print clobbers the just-emitted last cell.
            if on_this_line && cursor_logical_offset == line.len() {
                let last_idx = out.len().saturating_sub(1);
                cursor_target_row = last_idx;
                let used = line.len() % new_cols;
                if used == 0 && !line.is_empty() {
                    cursor_target_col = new_cols - 1;
                    cursor_pending_wrap = true;
                } else {
                    cursor_target_col = used.min(new_cols - 1);
                    cursor_pending_wrap = false;
                }
            }
        }

        // Step 5: push overflow to scrollback (oldest first); cursor follows.
        while out.len() > new_rows {
            let oldest = out.remove(0);
            self.scrollback.push(oldest);
            cursor_target_row = cursor_target_row.saturating_sub(1);
        }

        // Step 6: pad bottom with blank rows if we shrank.
        while out.len() < new_rows {
            out.push(Row::new(new_cols));
        }

        // Final commit + reset scroll region.
        let last = new_rows - 1;
        self.primary.rows = out;
        self.primary.cursor.row = cursor_target_row.min(last);
        self.primary.cursor.col = cursor_target_col.min(new_cols - 1);
        // pending_wrap was tracked through Step 3 — preserves the right-edge
        // semantics across reflow (TASKS §1.10). Inner-line cursor positions
        // leave it at the default `false`.
        self.primary.cursor.pending_wrap = cursor_pending_wrap;
        // Scroll region: reset to full screen. Reflow invalidates any custom
        // DECSTBM region (rows have moved); the next program-emitted DECSTBM
        // will re-establish it. Matches xterm.js behavior.
        self.primary.scroll_top = 0;
        self.primary.scroll_bottom = last;
    }

    // ------------------------------------------------------------------
    // Printing
    // ------------------------------------------------------------------

    /// Place one printable char at the cursor, advancing it.
    /// See cursor.rs for the DECAWM `pending_wrap` rationale.
    pub fn print(&mut self, ch: char, attrs: Attrs) {
        let w = wcwidth(ch as u32);
        if w == 0 {
            // Combining: best-effort attach to previous cell. Real grapheme
            // cluster support is a larger refactor (cell holds a SmallStr).
            // Leaving the simple fallback so combining marks don't advance
            // the cursor.
            return;
        }

        let attr_id = self.attrs.intern(attrs);
        let cols = self.cols;
        let scroll_top = self.screen().scroll_top;
        let scroll_bottom = self.screen().scroll_bottom;

        // Resolve pending wrap from the previous print.
        if self.screen().cursor.pending_wrap {
            self.screen_mut().cursor.pending_wrap = false;
            // Mark wrapped so reflow/copy can stitch the lines back.
            let row = self.screen().cursor.row;
            self.screen_mut().rows[row].wrapped = true;
            self.screen_mut().cursor.col = 0;
            self.linefeed();
        }

        // Wide char that won't fit: write a blank in the last column,
        // wrap, then print on the next line.
        if w == 2 && self.screen().cursor.col + 1 >= cols {
            let cur = self.screen().cursor;
            if cur.col < cols {
                self.screen_mut().rows[cur.row].cells[cur.col] =
                    Cell::new(' ', attr_id, 1);
            }
            self.screen_mut().rows[cur.row].wrapped = true;
            self.screen_mut().cursor.col = 0;
            self.linefeed();
        }

        // If we're about to overwrite the second half of a wide cell, also
        // clear the first half so we don't leave a stray glyph.
        let cur_col = self.screen().cursor.col;
        let cur_row = self.screen().cursor.row;
        if cur_col < cols {
            let here = self.screen().rows[cur_row].cells[cur_col];
            if here.width == 0 && cur_col > 0 {
                self.screen_mut().rows[cur_row].cells[cur_col - 1] =
                    Cell::new(' ', AttrId::DEFAULT, 1);
            }
        }

        // Place the cell(s).
        let row_idx = self.screen().cursor.row;
        if w == 2 {
            let col = self.screen().cursor.col;
            self.screen_mut().rows[row_idx].cells[col] = Cell::new(ch, attr_id, 2);
            self.screen_mut().rows[row_idx].cells[col + 1] = Cell::wide_spacer(attr_id);
            self.screen_mut().cursor.col += 2;
        } else {
            let col = self.screen().cursor.col;
            self.screen_mut().rows[row_idx].cells[col] = Cell::new(ch, attr_id, 1);
            self.screen_mut().cursor.col += 1;
        }

        // Don't advance past the rightmost column — set pending_wrap and
        // sit on cols-1. The next printable char will resolve it.
        if self.screen().cursor.col >= cols {
            self.screen_mut().cursor.col = cols - 1;
            self.screen_mut().cursor.pending_wrap = true;
        }

        // Silence unused warnings — these will be consumed when we
        // implement region-aware operations next round.
        let _ = (scroll_top, scroll_bottom);
    }

    // ------------------------------------------------------------------
    // Cursor motion
    // ------------------------------------------------------------------

    pub fn carriage_return(&mut self) {
        let cur = self.cursor_mut();
        cur.col = 0;
        cur.pending_wrap = false;
    }

    /// LF / IND. Move down one row; if at the bottom of the *scroll region*,
    /// scroll the region (which may push to scrollback when region is full).
    pub fn linefeed(&mut self) {
        let scr = self.screen();
        if scr.cursor.row == scr.scroll_bottom {
            self.scroll_region_up(1);
        } else if scr.cursor.row + 1 < self.rows {
            self.cursor_mut().row += 1;
        }
        // else cursor is below scroll region — clamp to last row, no scroll.
        self.cursor_mut().pending_wrap = false;
    }

    pub fn backspace(&mut self) {
        let cur = self.cursor_mut();
        if cur.col > 0 { cur.col -= 1; }
        cur.pending_wrap = false;
    }

    pub fn tab(&mut self) {
        let cols = self.cols;
        let cur = self.cursor_mut();
        let next = ((cur.col / 8) + 1) * 8;
        cur.col = next.min(cols.saturating_sub(1));
        cur.pending_wrap = false;
    }

    /// CBT — cursor backward by `n` tab stops. Tab stops are the default
    /// every-8-columns set (HTS/TBC for custom stops not yet modelled).
    /// At each step: if already on a tab stop (col % 8 == 0), back up
    /// to the previous one (col - 8); otherwise round down to the
    /// containing tab stop. Clamps at column 0 — never wraps to a
    /// negative column.
    pub fn cursor_back_tab(&mut self, n: usize) {
        let cur = self.cursor_mut();
        let mut col = cur.col;
        for _ in 0..n {
            if col == 0 { break; }
            col = ((col - 1) / 8) * 8;
        }
        cur.col = col;
        cur.pending_wrap = false;
    }

    pub fn cursor_to(&mut self, row: usize, col: usize) {
        let last_row = self.rows.saturating_sub(1);
        let last_col = self.cols.saturating_sub(1);
        let cur = self.cursor_mut();
        cur.row = row.min(last_row);
        cur.col = col.min(last_col);
        cur.pending_wrap = false;
    }

    pub fn cursor_up(&mut self, n: usize) {
        // Cursor up obeys the scroll region: it doesn't go above scroll_top
        // when the cursor was already inside the region.
        let scr = self.screen();
        let limit = if scr.cursor.row >= scr.scroll_top { scr.scroll_top } else { 0 };
        let new_row = scr.cursor.row.saturating_sub(n).max(limit);
        let cur = self.cursor_mut();
        cur.row = new_row;
        cur.pending_wrap = false;
    }

    pub fn cursor_down(&mut self, n: usize) {
        let scr = self.screen();
        let last = self.rows.saturating_sub(1);
        let limit = if scr.cursor.row <= scr.scroll_bottom { scr.scroll_bottom } else { last };
        let new_row = (scr.cursor.row + n).min(limit);
        let cur = self.cursor_mut();
        cur.row = new_row;
        cur.pending_wrap = false;
    }

    pub fn cursor_left(&mut self, n: usize) {
        let cur = self.cursor_mut();
        cur.col = cur.col.saturating_sub(n);
        cur.pending_wrap = false;
    }

    pub fn cursor_right(&mut self, n: usize) {
        let last_col = self.cols.saturating_sub(1);
        let cur = self.cursor_mut();
        cur.col = (cur.col + n).min(last_col);
        cur.pending_wrap = false;
    }

    // ------------------------------------------------------------------
    // Erase
    // ------------------------------------------------------------------

    pub fn erase_in_display(&mut self, mode: EraseMode) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        let total_rows = self.rows;
        match mode {
            EraseMode::Below => {
                self.erase_row_range(cur_row, cur_col, cols);
                for r in (cur_row + 1)..total_rows {
                    self.screen_mut().rows[r].clear();
                }
            }
            EraseMode::Above => {
                for r in 0..cur_row {
                    self.screen_mut().rows[r].clear();
                }
                self.erase_row_range(cur_row, 0, cur_col + 1);
            }
            EraseMode::All => {
                for r in &mut self.screen_mut().rows { r.clear(); }
            }
        }
    }

    pub fn erase_in_line(&mut self, mode: EraseMode) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        match mode {
            EraseMode::Below => self.erase_row_range(cur_row, cur_col, cols),
            EraseMode::Above => self.erase_row_range(cur_row, 0, cur_col + 1),
            EraseMode::All   => self.erase_row_range(cur_row, 0, cols),
        }
    }

    fn erase_row_range(&mut self, row: usize, start: usize, end: usize) {
        if let Some(r) = self.screen_mut().rows.get_mut(row) {
            let clamped_end = end.min(r.cells.len());
            for c in start..clamped_end {
                r.cells[c] = Cell::EMPTY;
            }
            // OSC 8 hyperlink spans must be kept in sync with the cells
            // they describe. Without this, CSI K / CSI J erase paths
            // wipe the cells but leave the span — and the renderer's
            // hyperlink-underline pass then paints an underline under
            // empty cells. Claude Code emits these heavily for status
            // redraws (TASKS §1.18.b residue symptom).
            clip_hyperlinks_around(&mut r.hyperlinks, start, clamped_end);
        }
    }

    // ------------------------------------------------------------------
    // In-line cell editing (ECH / ICH / DCH)
    //
    // These three are how line editors (PSReadLine, readline) and TUI
    // libraries (Ink, ratatui, blessed) do *partial* row updates without
    // redrawing the whole screen. Without them, Ink's frame N+1 ECH that
    // was supposed to wipe frame N's old characters is silently dropped
    // and the old text shows through behind the new text — the visible
    // "character residue" symptom.
    // ------------------------------------------------------------------

    /// ECH `CSI <n> X` — erase N cells starting at the cursor, replace
    /// with blanks. Cursor does NOT move. Cells past the right margin
    /// are clamped (no row spill).
    pub fn erase_chars(&mut self, n: usize) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        let end = (cur_col + n).min(cols);
        if let Some(r) = self.screen_mut().rows.get_mut(cur_row) {
            let clamped_end = end.min(r.cells.len());
            for c in cur_col..clamped_end {
                r.cells[c] = Cell::EMPTY;
            }
            // Same hyperlink-clipping invariant as `erase_row_range`:
            // ECH wipes cells, so any span overlapping the cleared
            // range must be clipped or dropped. (TASKS §1.18.b.)
            clip_hyperlinks_around(&mut r.hyperlinks, cur_col, clamped_end);
        }
        // ECH explicitly clears pending_wrap per xterm spec.
        self.cursor_mut().pending_wrap = false;
    }

    /// ICH `CSI <n> @` — insert N blank cells at the cursor, shifting
    /// the rest of the row right. Cells pushed past the right margin
    /// are dropped. Cursor does NOT move.
    pub fn insert_chars(&mut self, n: usize) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        if let Some(r) = self.screen_mut().rows.get_mut(cur_row) {
            let n = n.min(cols.saturating_sub(cur_col));
            if n == 0 { return; }
            // Shift right-of-cursor cells right by n; cells falling off are dropped.
            // Walk from the right edge inward to avoid overwriting source cells.
            for dst in (cur_col + n..cols).rev() {
                let src = dst - n;
                if src < r.cells.len() && dst < r.cells.len() {
                    r.cells[dst] = r.cells[src];
                }
            }
            for c in cur_col..(cur_col + n).min(r.cells.len()) {
                r.cells[c] = Cell::EMPTY;
            }
            // Hyperlink spans straddling or after the cursor get
            // invalidated. Line-edit operations (PSReadLine / readline /
            // Claude Code prompt edits) shift cell content but the
            // visible label of any hyperlink no longer corresponds to
            // its original click target — drop spans that overlap or
            // extend past the edit point. Matches xterm's "edit
            // invalidates the link" UX. (TASKS §1.18.b extension.)
            r.hyperlinks.retain(|span| span.col_end <= cur_col);
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// Mark a printed cell as part of an OSC 8 hyperlink span. Coalesces
    /// with the trailing span on the same row when uri+id match and the
    /// new cell starts exactly where the previous span ended — so writing
    /// "hello" inside one OSC 8 produces ONE span, not five.
    pub fn annotate_cell_with_link(
        &mut self,
        row: usize,
        col: usize,
        width: usize,
        uri: &str,
        id: Option<&str>,
    ) {
        let Some(r) = self.screen_mut().rows.get_mut(row) else { return };
        let end = col + width.max(1);
        if let Some(last) = r.hyperlinks.last_mut() {
            let id_match = match (&last.id, id) {
                (None, None) => true,
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if last.col_end == col && last.uri == uri && id_match {
                last.col_end = end;
                return;
            }
        }
        r.hyperlinks.push(super::cell::HyperlinkSpan {
            col_start: col,
            col_end: end,
            uri: uri.to_string(),
            id: id.map(|s| s.to_string()),
        });
    }

    /// DCH `CSI <n> P` — delete N cells at the cursor, shifting the
    /// rest of the row left. Blanks fill from the right margin. Cursor
    /// does NOT move.
    pub fn delete_chars(&mut self, n: usize) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        if let Some(r) = self.screen_mut().rows.get_mut(cur_row) {
            let n = n.min(cols.saturating_sub(cur_col));
            if n == 0 { return; }
            // Shift left.
            for dst in cur_col..(cols - n) {
                let src = dst + n;
                if src < r.cells.len() && dst < r.cells.len() {
                    r.cells[dst] = r.cells[src];
                }
            }
            // Fill the right side with blanks.
            for c in (cols - n)..cols.min(r.cells.len()) {
                r.cells[c] = Cell::EMPTY;
            }
            // Drop any hyperlink span overlapping or after the cursor
            // — see ICH for rationale. (TASKS §1.18.b extension.)
            r.hyperlinks.retain(|span| span.col_end <= cur_col);
        }
        self.cursor_mut().pending_wrap = false;
    }

    // ------------------------------------------------------------------
    // Scroll (region-aware)
    // ------------------------------------------------------------------

    /// Internal: scroll the active screen's scroll region up by `n` rows.
    /// New blank rows appear at scroll_bottom; rows leaving scroll_top
    /// enter scrollback ONLY if (a) we're on the primary screen AND
    /// (b) the region covers the entire screen.
    fn scroll_region_up(&mut self, n: usize) {
        let scr = self.screen();
        let top = scr.scroll_top;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - top + 1;
        let n = n.min(region_h);
        let push_to_scrollback = !self.is_alt && scr.is_full_region();
        let cols = self.cols;

        for _ in 0..n {
            // Pull the top row out — its allocation goes either to scrollback
            // (and recycles back as the new bottom) or to the new bottom
            // directly (alt screen / partial region: no scrollback push).
            let evicted_top = self.screen_mut().rows.remove(top);

            // The new bottom row: prefer recycling an evicted scrollback row.
            let new_bottom = if push_to_scrollback {
                match self.scrollback.push(evicted_top) {
                    Some(mut recycled) => {
                        recycled.clear();
                        recycled.resize(cols);
                        recycled
                    }
                    None => Row::new(cols),
                }
            } else {
                // Reuse the dropped row's allocation directly: clear and place
                // it at the bottom. This keeps alloc count flat per scroll.
                let mut row = evicted_top;
                row.clear();
                row.resize(cols);
                row
            };

            // Insert the new blank at `bottom`. Because we just removed at
            // `top`, the indices [top..bottom-1] shifted down by one — so
            // inserting at `bottom` puts it right after the last region row.
            self.screen_mut().rows.insert(bottom, new_bottom);
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// Internal: scroll the active region down by `n` rows. New blank rows
    /// at scroll_top, rows leaving scroll_bottom dropped (no scrollback).
    fn scroll_region_down(&mut self, n: usize) {
        let scr = self.screen();
        let top = scr.scroll_top;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - top + 1;
        let n = n.min(region_h);
        let cols = self.cols;

        for _ in 0..n {
            // Drop the bottom row, recycle its allocation as the new top.
            let mut recycled = self.screen_mut().rows.remove(bottom);
            recycled.clear();
            recycled.resize(cols);
            self.screen_mut().rows.insert(top, recycled);
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// CSI S — scroll up. Operates on the scroll region.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_region_up(n);
    }

    /// CSI T / RI — scroll down (reverse linefeed). Operates on the scroll region.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_region_down(n);
    }

    /// RI (ESC M): reverse linefeed. If at scroll_top, scrolls the region
    /// down; otherwise just moves cursor up.
    pub fn reverse_linefeed(&mut self) {
        let scr = self.screen();
        if scr.cursor.row == scr.scroll_top {
            self.scroll_region_down(1);
        } else if scr.cursor.row > 0 {
            self.cursor_mut().row -= 1;
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// CSI L — insert blank lines at cursor row, pushing rows down within
    /// the scroll region. No-op if cursor is outside the region.
    pub fn insert_lines(&mut self, n: usize) {
        let scr = self.screen();
        if scr.cursor.row < scr.scroll_top || scr.cursor.row > scr.scroll_bottom {
            return;
        }
        let cur = scr.cursor.row;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - cur + 1;
        let n = n.min(region_h);
        let cols = self.cols;
        for _ in 0..n {
            // Drop the row at `bottom`, recycle its allocation as the new
            // blank inserted at `cur`. Net: rows[cur..bottom] shift down by 1.
            let mut recycled = self.screen_mut().rows.remove(bottom);
            recycled.clear();
            recycled.resize(cols);
            self.screen_mut().rows.insert(cur, recycled);
        }
        let cur_mut = self.cursor_mut();
        cur_mut.col = 0;
        cur_mut.pending_wrap = false;
    }

    /// CSI M — delete lines at cursor row, pulling rows up within the
    /// scroll region. No-op if cursor is outside the region.
    pub fn delete_lines(&mut self, n: usize) {
        let scr = self.screen();
        if scr.cursor.row < scr.scroll_top || scr.cursor.row > scr.scroll_bottom {
            return;
        }
        let cur = scr.cursor.row;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - cur + 1;
        let n = n.min(region_h);
        let cols = self.cols;
        for _ in 0..n {
            // Remove the row at `cur`, recycle as new blank at `bottom`.
            let mut recycled = self.screen_mut().rows.remove(cur);
            recycled.clear();
            recycled.resize(cols);
            self.screen_mut().rows.insert(bottom, recycled);
        }
        let cur_mut = self.cursor_mut();
        cur_mut.col = 0;
        cur_mut.pending_wrap = false;
    }
}

/// Clip OSC 8 hyperlink spans on a row so they no longer cover cells in
/// the just-erased `[start, end)` column range.
///
/// Per TASKS §1.18.b, the partial-erase paths (`CSI K` line erase,
/// `CSI J` cursor-relative display erase, `CSI X` ECH) used to leave
/// hyperlink spans untouched while wiping the underlying cells. The
/// renderer's hyperlink-underline pass then drew an underline under
/// blank cells, producing the "leftover residue" the user reported in
/// Claude Code output (which uses these escapes heavily for status-line
/// redraws).
///
/// Cases:
///   - span entirely outside `[start, end)` → keep
///   - span entirely inside `[start, end)` → drop
///   - erase wipes span tail (span.col_start < start && span.col_end <= end) → clip end to start
///   - erase wipes span head (span.col_start >= start && span.col_end > end) → clip start to end
///   - erase punches a hole in the middle of a span (span.col_start < start && span.col_end > end)
///     → drop the entire span. We can't split into two without growing the Vec
///     mid-`retain`; the surviving prefix and suffix become unlinked, which
///     matches xterm's "erase invalidates the link" UX (the user can re-emit
///     OSC 8 to restore it). This is rare in practice — partial-erase usually
///     covers a whole word or label.
fn clip_hyperlinks_around(spans: &mut Vec<super::cell::HyperlinkSpan>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    spans.retain_mut(|span| {
        if span.col_end <= start || span.col_start >= end {
            true // entirely outside the erase window
        } else if span.col_start >= start && span.col_end <= end {
            false // entirely inside — drop
        } else if span.col_start < start && span.col_end > end {
            false // hole punched in the middle — drop (see doc-comment)
        } else if span.col_end > end {
            // erase covers the head; clip start forward to `end`.
            span.col_start = end;
            true
        } else {
            // erase covers the tail; clip end backward to `start`.
            span.col_end = start;
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_screen_isolates_content() {
        let mut g = Grid::new(3, 5, 10);
        g.print('a', Attrs::DEFAULT);
        g.print('b', Attrs::DEFAULT);
        g.enter_alt_screen(true);
        // alt is blank
        assert_eq!(g.row(0).unwrap().cells[0].ch, ' ');
        g.print('X', Attrs::DEFAULT);
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'X');
        g.leave_alt_screen();
        // primary survived intact
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(0).unwrap().cells[1].ch, 'b');
    }

    #[test]
    fn alt_screen_does_not_pollute_scrollback() {
        let mut g = Grid::new(2, 5, 10);
        g.enter_alt_screen(true);
        // Fill alt and force scroll
        for _ in 0..5 {
            g.print('Z', Attrs::DEFAULT);
            g.linefeed();
            g.carriage_return();
        }
        // Scrollback must remain empty.
        assert_eq!(g.scrollback.len(), 0);
    }

    #[test]
    fn scroll_region_constrains_linefeed() {
        let mut g = Grid::new(5, 5, 10);
        g.set_scroll_region(Some(2), Some(4)); // rows 1..3 (0-based)
        // Fill some rows
        for ch in ['a', 'b', 'c', 'd', 'e'] {
            g.print(ch, Attrs::DEFAULT);
            g.linefeed();
            g.carriage_return();
        }
        // The first row (row 0) should still be untouched because LF at
        // the bottom of the region only scrolls rows 1..3.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        // No scrollback either: partial scroll region.
        assert_eq!(g.scrollback.len(), 0);
    }

    #[test]
    fn full_region_scroll_pushes_to_scrollback() {
        let mut g = Grid::new(2, 5, 10);
        // Default region = full screen.
        g.print('1', Attrs::DEFAULT); g.linefeed(); g.carriage_return();
        g.print('2', Attrs::DEFAULT); g.linefeed(); g.carriage_return();
        g.print('3', Attrs::DEFAULT);
        // Should have scrolled '1' into scrollback.
        assert_eq!(g.scrollback.len(), 1);
        assert_eq!(g.scrollback.get(0).unwrap().cells[0].ch, '1');
    }

    #[test]
    fn resize_grow_extends_default_scroll_region() {
        // Repro of the "stuck on bottom row" bug: kernel created at 24 rows
        // then resized up to 26 used to keep scroll_bottom=23, leaving rows
        // 24..25 frozen and breaking scrollback push.
        let mut g = Grid::new(24, 80, 100);
        g.resize(26, 49);
        assert_eq!(g.primary.scroll_top, 0);
        assert_eq!(g.primary.scroll_bottom, 25);
        assert!(g.primary.is_full_region());

        // Drive 30 lines through the grid; each LF at the new bottom must
        // scroll and push the evicted row into scrollback.
        for i in 0..30u32 {
            for ch in i.to_string().chars() {
                g.print(ch, Attrs::DEFAULT);
            }
            g.linefeed();
            g.carriage_return();
        }
        // 30 lines into a 26-row screen → at least 4 evictions to scrollback.
        assert!(g.scrollback.len() >= 4, "scrollback empty after grow-resize");
    }

    #[test]
    fn resize_grow_preserves_custom_scroll_region() {
        // DECSTBM-set custom region must NOT be silently extended on resize;
        // it just gets clamped to new bounds (or reset if invalidated).
        let mut g = Grid::new(10, 10, 0);
        g.set_scroll_region(Some(2), Some(6)); // rows 1..5 (0-based)
        assert_eq!(g.primary.scroll_top, 1);
        assert_eq!(g.primary.scroll_bottom, 5);
        g.resize(12, 10);
        assert_eq!(g.primary.scroll_top, 1);
        assert_eq!(g.primary.scroll_bottom, 5); // preserved, not extended
        assert!(!g.primary.is_full_region());
    }

    #[test]
    fn resize_shrink_clamps_default_scroll_region() {
        let mut g = Grid::new(10, 10, 0);
        g.resize(5, 10);
        assert_eq!(g.primary.scroll_top, 0);
        assert_eq!(g.primary.scroll_bottom, 4);
        assert!(g.primary.is_full_region());
    }

    #[test]
    fn ri_at_scroll_top_scrolls_down() {
        let mut g = Grid::new(3, 3, 0);
        g.print('a', Attrs::DEFAULT); g.linefeed(); g.carriage_return();
        g.print('b', Attrs::DEFAULT);
        g.cursor_to(0, 0);
        g.reverse_linefeed();
        // After RI at top, row 0 is blank, 'a' moved to row 1, 'b' to row 2.
        assert_eq!(g.row(0).unwrap().cells[0].ch, ' ');
        assert_eq!(g.row(1).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(2).unwrap().cells[0].ch, 'b');
    }

    #[test]
    fn insert_delete_lines_within_region() {
        let mut g = Grid::new(4, 3, 0);
        // Place 'a','b','c','d' on rows 0..3 without triggering the
        // bottom-of-region scroll. Print + LF + CR for the first three;
        // for the last, only print (no trailing LF) so 'a' isn't ejected.
        for ch in ['a', 'b', 'c'] {
            g.print(ch, Attrs::DEFAULT); g.linefeed(); g.carriage_return();
        }
        g.print('d', Attrs::DEFAULT);
        // Sanity: setup placed all four rows correctly.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(3).unwrap().cells[0].ch, 'd');

        // IL at row 1: insert one blank, push 'b','c' down, 'd' lost.
        g.cursor_to(1, 0);
        g.insert_lines(1);
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(1).unwrap().cells[0].ch, ' ');
        assert_eq!(g.row(2).unwrap().cells[0].ch, 'b');
        assert_eq!(g.row(3).unwrap().cells[0].ch, 'c');
    }

    // ---- Reflow (Phase 1) ---------------------------------------------
    // See OVERVIEW.md §7 + TASKS §2.3 for the design these tests cover.

    /// Helper: read the printable text of a row (stripping trailing blanks).
    fn row_text(g: &Grid, r: usize) -> String {
        let row = g.row(r).expect("row in range");
        let mut s: String = row.cells.iter().map(|c| c.ch).collect();
        while s.ends_with(' ') {
            s.pop();
        }
        s
    }

    #[test]
    fn reflow_shrink_wraps_long_line() {
        // 80-col grid, print 70 'a's. Resize to 40 cols. The single logical
        // line of 70 'a's should re-wrap into 40 + 30 (with first row wrapped).
        let mut g = Grid::new(5, 80, 100);
        for _ in 0..70 {
            g.print('a', Attrs::DEFAULT);
        }
        g.resize(5, 40);
        assert_eq!(g.cols(), 40);
        assert_eq!(row_text(&g, 0), "a".repeat(40));
        assert_eq!(g.row(0).unwrap().wrapped, true);
        assert_eq!(row_text(&g, 1), "a".repeat(30));
        assert_eq!(g.row(1).unwrap().wrapped, false);
    }

    #[test]
    fn reflow_grow_unwraps_continued_line() {
        // 40-col grid, print 70 'a's → row 0 has 40 'a's (wrapped=true),
        // row 1 has 30 'a's. Resize to 80 cols → single row with 70 'a's.
        let mut g = Grid::new(5, 40, 100);
        for _ in 0..70 {
            g.print('a', Attrs::DEFAULT);
        }
        // Sanity check the setup.
        assert_eq!(g.row(0).unwrap().wrapped, true);
        assert_eq!(row_text(&g, 1), "a".repeat(30));

        g.resize(5, 80);
        assert_eq!(g.cols(), 80);
        assert_eq!(row_text(&g, 0), "a".repeat(70));
        assert_eq!(g.row(0).unwrap().wrapped, false);
        // Row 1 should be blank now (the long line collapsed into row 0).
        assert_eq!(row_text(&g, 1), "");
    }

    #[test]
    fn reflow_preserves_cursor_logical_position() {
        // Print "hello world" then move cursor onto 'w' at (0, 6).
        // After reflow to 5 cols, the logical line "hello world" wraps into
        // ["hello", " worl", "d"]; cursor at logical offset 6 lands at
        // row 1, col 1 (the space before 'w', since offset 6 = 1*5 + 1).
        let mut g = Grid::new(5, 20, 100);
        for ch in "hello world".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        g.cursor_to(0, 6);
        g.resize(5, 5);
        assert_eq!(g.cols(), 5);
        assert_eq!(row_text(&g, 0), "hello");
        assert_eq!(g.row(0).unwrap().wrapped, true);
        // Cursor was at logical offset 6 → (6 / 5, 6 % 5) = (1, 1).
        assert_eq!(g.cursor().row, 1);
        assert_eq!(g.cursor().col, 1);
    }

    #[test]
    fn reflow_skips_alt_screen() {
        // Alt screen content should be truncate/pad-resized regardless of
        // column change, because TUIs handle SIGWINCH themselves.
        let mut g = Grid::new(3, 10, 100);
        g.enter_alt_screen(true);
        for ch in "abcdefghij".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        // Alt row 0 holds 10 chars at cols=10. Resize to cols=5 should
        // truncate to "abcde", NOT reflow into 2 rows.
        g.resize(3, 5);
        assert_eq!(g.cols(), 5);
        assert_eq!(row_text(&g, 0), "abcde");
        // Row 1 stays blank — proof we didn't reflow the truncated half.
        assert_eq!(row_text(&g, 1), "");
        // Wrapped flag stays false since we did not reflow.
        assert_eq!(g.row(0).unwrap().wrapped, false);
    }

    #[test]
    fn reflow_chain_of_three_rows_round_trip() {
        // Wrap a 25-char line across 3 rows at cols=10:
        // ["0123456789", "abcdefghij", "ABCDE"]. Resize to 15 → two rows
        // (15 + 10). Resize back to 10 → original three rows.
        let mut g = Grid::new(5, 10, 100);
        for ch in "0123456789abcdefghijABCDE".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        assert_eq!(row_text(&g, 0), "0123456789");
        assert_eq!(row_text(&g, 1), "abcdefghij");
        assert_eq!(row_text(&g, 2), "ABCDE");
        assert_eq!(g.row(0).unwrap().wrapped, true);
        assert_eq!(g.row(1).unwrap().wrapped, true);
        assert_eq!(g.row(2).unwrap().wrapped, false);

        g.resize(5, 15);
        assert_eq!(row_text(&g, 0), "0123456789abcde");
        assert_eq!(row_text(&g, 1), "fghijABCDE");
        assert_eq!(g.row(0).unwrap().wrapped, true);
        assert_eq!(g.row(1).unwrap().wrapped, false);

        g.resize(5, 10);
        assert_eq!(row_text(&g, 0), "0123456789");
        assert_eq!(row_text(&g, 1), "abcdefghij");
        assert_eq!(row_text(&g, 2), "ABCDE");
    }

    #[test]
    fn reflow_no_op_when_cols_unchanged() {
        // Rows-only change must NOT re-wrap (which would also strip trailing
        // blank padding on intermediate rows). The old naive truncate/pad
        // path should be taken instead.
        let mut g = Grid::new(5, 20, 100);
        for ch in "hello".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        g.resize(8, 20); // grow rows, same cols
        assert_eq!(g.rows(), 8);
        assert_eq!(g.cols(), 20);
        assert_eq!(row_text(&g, 0), "hello");
        // Rows 1..7 should be blank (padding).
        for r in 1..8 {
            assert_eq!(row_text(&g, r), "");
        }
    }

    #[test]
    fn reflow_preserves_pending_wrap_at_exact_boundary() {
        // 10 chars in a 10-col grid → cursor at col 9 with pending_wrap=true
        // (print() set it; next char would wrap to new row).
        let mut g = Grid::new(5, 10, 100);
        for ch in "0123456789".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        assert_eq!(g.cursor().col, 9, "cursor at last col after 10 prints in 10-col grid");
        assert!(g.cursor().pending_wrap, "print() set pending_wrap at right edge");

        // Resize to 5 cols. Reflow stitches 10 chars → 1 logical line of
        // length 10. Re-wrap: row 0 = "01234" (wrap=true), row 1 = "56789"
        // (wrap=false). Cursor was at logical offset 10 (== line.len()).
        // new_cols=5, used = 10 % 5 = 0 → exact-boundary case. Cursor
        // should land at (1, 4) WITH pending_wrap=true preserved.
        g.resize(5, 5);

        assert_eq!(g.cursor().row, 1, "cursor on last row of wrap chain");
        assert_eq!(g.cursor().col, 4, "cursor at last col of that row");
        assert!(
            g.cursor().pending_wrap,
            "pending_wrap preserved across reflow at exact boundary"
        );

        // Sanity: next print should wrap (not overwrite cell (1,4)).
        g.print('x', Attrs::DEFAULT);
        assert_eq!(g.row(1).unwrap().cells[4].ch, '9', "row 1 col 4 unchanged");
        assert_eq!(g.row(2).unwrap().cells[0].ch, 'x', "'x' wrapped to row 2 col 0");
    }

    #[test]
    fn reflow_no_pending_wrap_when_line_doesnt_fill_last_row() {
        // Inverse of the above: line.len() = 7 chars, new_cols = 5.
        // 7 % 5 = 2. Cursor at offset 7 lands on (1, 2) — middle of row 1,
        // pending_wrap should be false (next print writes to (1, 2), no wrap).
        let mut g = Grid::new(5, 10, 100);
        for ch in "abcdefg".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        // Cursor after 7 prints: col 7 (no pending_wrap because we haven't
        // hit the right edge of the 10-col line).
        assert_eq!(g.cursor().col, 7);
        assert!(!g.cursor().pending_wrap);

        g.resize(5, 5);
        assert_eq!(g.cursor().row, 1);
        assert_eq!(g.cursor().col, 2);
        assert!(!g.cursor().pending_wrap, "no pending_wrap mid-row");
    }

    #[test]
    fn reflow_keeps_wide_char_intact_at_boundary() {
        // 80-col grid: 39 ASCII 'a's, then a wide CJK '中' at cols 39-40
        // (lead at 39, continuation half at 40), then 'b' at col 41.
        // Resizing to 40 cols would naïvely slice [0..40] → lead at idx 39
        // (last col of new row), half at idx 40 (next row first col):
        // wide char split. Protection should pull slice back to end=39
        // so the lead+half move together to row 1.
        let mut g = Grid::new(5, 80, 100);
        for _ in 0..39 {
            g.print('a', Attrs::DEFAULT);
        }
        g.print('中', Attrs::DEFAULT); // wcwidth → 2, occupies cols 39-40
        g.print('b', Attrs::DEFAULT); // col 41

        g.resize(5, 40);

        // Row 0 should have 39 'a's then a blank at col 39 (the freed cell).
        let row0 = g.row(0).unwrap();
        let mut count_a = 0;
        for c in &row0.cells {
            if c.ch == 'a' {
                count_a += 1;
            }
        }
        assert_eq!(count_a, 39, "row 0 keeps 39 'a's after pullback");
        assert!(row0.cells[39].is_blank(), "freed cell at col 39 is blank");
        assert!(row0.wrapped, "row 0 wraps to row 1");

        // Row 1 starts with the intact wide char (lead width=2, half width=0)
        // followed by 'b'.
        let row1 = g.row(1).unwrap();
        assert_eq!(row1.cells[0].ch, '中', "wide lead moved to row 1 col 0");
        assert_eq!(row1.cells[0].width, 2);
        assert_eq!(row1.cells[1].width, 0, "continuation half at row 1 col 1");
        assert_eq!(row1.cells[2].ch, 'b');
    }

    #[test]
    fn reflow_shrink_overflow_pushes_to_scrollback() {
        // 5-row grid, fill all 5 rows with distinct content (no soft-wrap).
        // Resize from 10 cols to 5 cols. Each line wraps into 2 rows → 10
        // total rows. With new_rows=5, the oldest 5 wrapped rows must enter
        // scrollback so cursor + most-recent content stays visible.
        let mut g = Grid::new(5, 10, 100);
        for line in 0..5 {
            for _ in 0..10 {
                g.print(char::from(b'A' + line as u8), Attrs::DEFAULT);
            }
            if line < 4 {
                g.linefeed();
                g.carriage_return();
            }
        }
        assert_eq!(g.scrollback.len(), 0);
        g.resize(5, 5);
        // Every line was 10 chars wide, now wraps to 2 rows of 5 → 10 rows
        // of content. Visible window holds 5 rows; oldest 5 went to scrollback.
        assert_eq!(g.scrollback.len(), 5);
        // The last visible row should contain the tail of "EEEEEEEEEE".
        assert!(row_text(&g, 4).contains('E'));
    }
}
