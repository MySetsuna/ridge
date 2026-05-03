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

    /// Resize. Naive: truncate/pad rows + cols on both screens. Soft-wrap
    /// reflow is left for a later round (it requires walking back through
    /// `wrapped` flags to glue continuation lines).
    ///
    /// Scroll-region preservation rule: if the region was the default
    /// full screen before resize (top=0, bottom=rows-1), extend it to
    /// match the new size. Otherwise it's a custom DECSTBM range — clamp
    /// to the new bounds and revert to full if the clamp would invalidate.
    /// Without this, a kernel created at 24 rows then resized to 26 keeps
    /// scroll_bottom=23, leaving rows 24..25 as a frozen footer; LF at the
    /// real bottom never scrolls and scrollback never grows.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        for screen in [&mut self.primary, &mut self.alt] {
            // Capture region-was-default before mutating rows.
            let old_last = screen.rows.len().saturating_sub(1);
            let region_was_full = screen.scroll_top == 0 && screen.scroll_bottom == old_last;

            if cols != self.cols {
                for r in &mut screen.rows { r.resize(cols); }
            }
            if rows < screen.rows.len() {
                screen.rows.truncate(rows);
            } else {
                while screen.rows.len() < rows {
                    screen.rows.push(Row::new(cols));
                }
            }
            // Clamp cursor to new bounds.
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
        self.rows = rows;
        self.cols = cols;
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
            for c in start..end.min(r.cells.len()) {
                r.cells[c] = Cell::EMPTY;
            }
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
            for c in cur_col..end.min(r.cells.len()) {
                r.cells[c] = Cell::EMPTY;
            }
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
}
