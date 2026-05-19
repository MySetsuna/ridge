//! Selection model — abs-row coords for scroll-stable highlighting (§1.20).
//!
//! ## Coordinate spaces
//!
//! Two row coord systems are in play:
//!
//! - **Viewport-relative** (`Pos { row, col }` / `Range`): row in `0..rows`,
//!   what the renderer + JS `setSelection` API speak. Drifts under scroll —
//!   the same vp_row points at different content as `scroll_offset` changes.
//! - **Absolute** (`RangeAbs { start_abs_row, ..., end_abs_row, ... }`):
//!   `0..sb_len` → scrollback row (oldest-first); `sb_len..sb_len+rows`
//!   → live grid row. Stable: each piece of content keeps the same
//!   abs_row regardless of where the user scrolls (until scrollback
//!   eviction at capacity, which currently also clears the selection
//!   via the `feed()` path in `JsTerminal`).
//!
//! Selection stores **abs-row internally** so the highlight tracks its
//! original cells through scroll. Public API still accepts viewport
//! coords (the natural input from a mouse drag — manager.ts pointerdown
//! always speaks viewport space) and translates at the boundary.
//!
//! Conversion formula (mirrors `search.rs::match_to_viewport_range`):
//!   abs_row = sb_len + vp_row - scroll_offset       (vp → abs)
//!   vp_row  = abs_row + scroll_offset - sb_len      (abs → vp; needs checked_sub)
//! Both work uniformly across the scrollback / live-grid boundary.

use crate::term::Terminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Range {
    pub start: Pos,
    pub end: Pos,
}

impl Range {
    /// Normalize so `start` is always the textual-earlier point.
    pub fn normalized(self) -> Range {
        let (a, b) = (self.start, self.end);
        if (a.row, a.col) <= (b.row, b.col) {
            Range { start: a, end: b }
        } else {
            Range { start: b, end: a }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Selection range in absolute-row coords (see module docstring).
/// Stable across viewport scroll. The renderer translates through
/// `Selection::range_in_viewport` per frame, naturally clipping
/// rows that are above or below the current viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeAbs {
    pub start_abs_row: usize,
    pub start_col: usize,
    pub end_abs_row: usize,
    pub end_col: usize,
}

impl RangeAbs {
    pub fn normalized(self) -> RangeAbs {
        if (self.start_abs_row, self.start_col) <= (self.end_abs_row, self.end_col) {
            self
        } else {
            RangeAbs {
                start_abs_row: self.end_abs_row,
                start_col: self.end_col,
                end_abs_row: self.start_abs_row,
                end_col: self.start_col,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start_abs_row == self.end_abs_row && self.start_col == self.end_col
    }
}

/// Convert a viewport row to an absolute row given the terminal's
/// current scroll state. Saturating: scroll_offset is clamped at
/// sb_len in `scroll_up_view`, so `sb_len + vp_row >= scroll_offset`
/// always holds and the subtraction never underflows.
fn vp_to_abs(vp_row: usize, scroll_offset: usize, sb_len: usize) -> usize {
    sb_len.saturating_add(vp_row).saturating_sub(scroll_offset)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Selection {
    range_abs: Option<RangeAbs>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Programmatic set from viewport coords (e.g. mouse drag from
    /// manager.ts). Takes the terminal so it can capture the current
    /// scroll state and store the range in abs-row form. After this,
    /// the selection follows its original cells through scroll.
    pub fn set(&mut self, terminal: &Terminal, range: Range) {
        if range.is_empty() {
            self.range_abs = None;
            return;
        }
        let n = range.normalized();
        let off = terminal.scroll_offset();
        let sb = terminal.scrollback_len();
        let abs = RangeAbs {
            start_abs_row: vp_to_abs(n.start.row, off, sb),
            start_col: n.start.col,
            end_abs_row: vp_to_abs(n.end.row, off, sb),
            end_col: n.end.col,
        };
        if abs.is_empty() {
            self.range_abs = None;
        } else {
            self.range_abs = Some(abs.normalized());
        }
    }

    /// Set directly in abs coords (for callers that already speak
    /// abs-row, e.g. the search → selection sync path in lib.rs).
    pub fn set_abs(&mut self, range: RangeAbs) {
        if range.is_empty() {
            self.range_abs = None;
        } else {
            self.range_abs = Some(range.normalized());
        }
    }

    pub fn clear(&mut self) {
        self.range_abs = None;
    }

    /// Raw abs-row range. Used by the renderer's selection-changed
    /// detector and by callers that need scroll-stable identity.
    pub fn range_abs(&self) -> Option<RangeAbs> {
        self.range_abs
    }

    /// Translate the stored abs-row range to a viewport-relative
    /// `Range`, clipped to the visible viewport. Returns `None` when
    /// the selection is entirely above/below the viewport (or empty).
    ///
    /// For multi-row selections that partially overlap the viewport:
    ///  * If the start row is clipped off the top, `start.col = 0`
    ///    so the first visible row shows full-width highlight.
    ///  * If the end row is clipped off the bottom, `end.col = cols`
    ///    so the last visible row shows full-width highlight.
    /// This is the same shape `selection_to_rects` produces from a
    /// fully-in-viewport `Range`, so the renderer doesn't need to
    /// special-case the clipped form.
    pub fn range_in_viewport(&self, terminal: &Terminal) -> Option<Range> {
        let abs = self.range_abs?;
        let off = terminal.scroll_offset();
        let sb = terminal.scrollback_len();
        let rows = terminal.rows();
        let cols = terminal.cols();
        if rows == 0 {
            return None;
        }

        // Viewport's abs-row span: [sb - off, sb - off + rows).
        let vp_first_abs = sb.saturating_sub(off);
        let vp_last_abs = vp_first_abs.saturating_add(rows).saturating_sub(1);

        // Clip selection abs range to viewport span.
        if abs.end_abs_row < vp_first_abs || abs.start_abs_row > vp_last_abs {
            return None;
        }
        let visible_start_abs = abs.start_abs_row.max(vp_first_abs);
        let visible_end_abs = abs.end_abs_row.min(vp_last_abs);

        // Translate visible abs back to vp_row.
        let vp_start_row = visible_start_abs + off - sb;
        let vp_end_row = visible_end_abs + off - sb;

        // Decide which columns to use: original start_col only when
        // the visible range still includes the original start; else 0.
        // Mirror logic for end.
        let start_col = if visible_start_abs == abs.start_abs_row {
            abs.start_col
        } else {
            0
        };
        let end_col = if visible_end_abs == abs.end_abs_row {
            abs.end_col
        } else {
            cols
        };

        Some(Range {
            start: Pos {
                row: vp_start_row,
                col: start_col,
            },
            end: Pos {
                row: vp_end_row,
                col: end_col,
            },
        })
    }

    pub fn is_empty(&self) -> bool {
        self.range_abs.is_none()
    }

    /// Programmatic "select all visible". Selects the entire current
    /// viewport in abs-row terms — i.e. abs `[sb_len, sb_len + rows)`.
    /// If the user later scrolls into history, the selection still
    /// covers the original viewport content (not the freshly-visible
    /// scrollback rows).
    pub fn select_all(&mut self, terminal: &Terminal) {
        let rows = terminal.rows();
        let cols = terminal.cols();
        let sb = terminal.scrollback_len();
        if rows == 0 || cols == 0 {
            self.range_abs = None;
            return;
        }
        self.range_abs = Some(RangeAbs {
            start_abs_row: sb,
            start_col: 0,
            end_abs_row: sb + rows - 1,
            end_col: cols,
        });
    }

    /// Double-click word selection. Selects the contiguous run of
    /// non-whitespace, non-NUL chars containing the clicked cell. If
    /// the clicked cell IS whitespace or empty, clears the selection
    /// (matches xterm).
    ///
    /// `row` is viewport-relative (manager.ts speaks vp coords); we
    /// resolve the cell content via `viewport_row` and store the
    /// resulting span in abs coords.
    ///
    /// Note on scrollback: `viewport_row` already transparently routes
    /// `vp_row < scroll_offset` to the matching scrollback entry (see
    /// `Terminal::viewport_row`). Double-clicking a line that's currently
    /// shown but pulled from history therefore works without any extra
    /// dispatch — `vp_to_abs` rewrites the stored row into the same
    /// absolute index the resolved row came from, so a later
    /// `range_in_viewport` translation lands on the right cells across
    /// further scrolls. Selecting cells that are NOT in the current
    /// viewport (e.g. a future "context-menu select word at abs row N"
    /// feature) is a separate code path; the pointer-driven case here
    /// can't reach those cells by construction.
    pub fn select_word(&mut self, terminal: &Terminal, row: usize, col: usize) {
        let Some(r) = terminal.viewport_row(row) else {
            self.range_abs = None;
            return;
        };
        let n = r.cells.len();
        if col >= n {
            self.range_abs = None;
            return;
        }
        let is_word = |ch: char| !ch.is_whitespace() && ch != '\0';
        if !is_word(r.cells[col].ch) {
            self.range_abs = None;
            return;
        }
        let mut lo = col;
        while lo > 0 && is_word(r.cells[lo - 1].ch) {
            lo -= 1;
        }
        let mut hi = col + 1;
        while hi < n && is_word(r.cells[hi].ch) {
            hi += 1;
        }
        let abs_row = vp_to_abs(row, terminal.scroll_offset(), terminal.scrollback_len());
        self.range_abs = Some(RangeAbs {
            start_abs_row: abs_row,
            start_col: lo,
            end_abs_row: abs_row,
            end_col: hi,
        });
    }

    /// Triple-click line selection. Selects the entire row.
    ///
    /// Same scrollback note as `select_word`: `vp_to_abs` maps the
    /// viewport row to its underlying abs index, which works uniformly
    /// for live-grid rows and scrollback rows currently displayed in
    /// the viewport. No `_at_abs` variant is needed for the
    /// pointer-driven path.
    pub fn select_line(&mut self, terminal: &Terminal, row: usize) {
        let cols = terminal.cols();
        if row >= terminal.rows() || cols == 0 {
            self.range_abs = None;
            return;
        }
        let abs_row = vp_to_abs(row, terminal.scroll_offset(), terminal.scrollback_len());
        self.range_abs = Some(RangeAbs {
            start_abs_row: abs_row,
            start_col: 0,
            end_abs_row: abs_row,
            end_col: cols,
        });
    }

    /// Extract the selected text. Reads via `terminal.row_at_abs(abs)`
    /// so cross-scrollback selections work. Soft-wrapped rows
    /// (`row.wrapped == true`) join without inserting a newline; hard
    /// breaks get `\n`. Trailing whitespace per row is trimmed (same as
    /// xterm's "copy a line" convention).
    pub fn text(&self, terminal: &Terminal) -> String {
        let Some(range) = self.range_abs else {
            return String::new();
        };
        let cols = terminal.cols();
        let mut out = String::new();

        for abs in range.start_abs_row..=range.end_abs_row {
            let Some(row) = terminal.row_at_abs(abs) else {
                break;
            };

            let lo = if abs == range.start_abs_row {
                range.start_col
            } else {
                0
            };
            let hi = if abs == range.end_abs_row {
                range.end_col
            } else {
                cols
            };
            let hi = hi.min(row.cells.len());
            if lo >= hi {
                let is_last = abs == range.end_abs_row;
                if !is_last && !row.wrapped {
                    out.push('\n');
                }
                continue;
            }

            let mut line = String::new();
            for col in lo..hi {
                let cell = &row.cells[col];
                if cell.width == 0 {
                    continue;
                }
                line.push(cell.ch);
            }
            let trimmed = line.trim_end_matches(' ');
            out.push_str(trimmed);

            let is_last = abs == range.end_abs_row;
            if !is_last && !row.wrapped {
                out.push('\n');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::Terminal;

    fn vp_range(sr: usize, sc: usize, er: usize, ec: usize) -> Range {
        Range {
            start: Pos { row: sr, col: sc },
            end: Pos { row: er, col: ec },
        }
    }

    #[test]
    fn empty_selection_yields_empty_string() {
        let t = Terminal::new(3, 5, 0);
        let s = Selection::default();
        assert_eq!(s.text(&t), "");
    }

    #[test]
    fn select_all_copies_visible_grid() {
        let mut t = Terminal::new(2, 5, 0);
        t.feed(b"hi\r\nho");
        let mut s = Selection::default();
        s.select_all(&t);
        assert_eq!(s.text(&t), "hi\nho");
    }

    #[test]
    fn range_normalization_swaps_reverse_drag() {
        let r = Range {
            start: Pos { row: 2, col: 5 },
            end: Pos { row: 0, col: 0 },
        }
        .normalized();
        assert_eq!(r.start, Pos { row: 0, col: 0 });
        assert_eq!(r.end, Pos { row: 2, col: 5 });
    }

    #[test]
    fn select_word_finds_contiguous_non_whitespace() {
        let mut t = Terminal::new(1, 30, 0);
        t.feed(b"hello   world.txt   foo");
        let mut s = Selection::default();
        s.select_word(&t, 0, 11);
        assert_eq!(s.text(&t), "world.txt");
    }

    #[test]
    fn select_word_on_whitespace_clears() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"hello   world");
        let mut s = Selection::default();
        s.set(&t, vp_range(0, 0, 0, 5));
        s.select_word(&t, 0, 6);
        assert!(s.is_empty(), "click on whitespace must clear selection");
    }

    #[test]
    fn select_line_covers_full_row() {
        let mut t = Terminal::new(2, 10, 0);
        t.feed(b"abcdef");
        let mut s = Selection::default();
        s.select_line(&t, 0);
        assert_eq!(s.text(&t), "abcdef");
    }

    #[test]
    fn soft_wrapped_rows_join_without_newline() {
        let mut t = Terminal::new(3, 4, 0);
        t.feed(b"abcdef");
        let mut s = Selection::default();
        s.select_all(&t);
        let text = s.text(&t);
        assert!(text.starts_with("abcdef"), "got {:?}", text);
    }

    #[test]
    fn range_is_empty_when_start_equals_end() {
        let r = Range {
            start: Pos { row: 1, col: 5 },
            end: Pos { row: 1, col: 5 },
        };
        assert!(r.is_empty());
        let r2 = Range {
            start: Pos { row: 1, col: 5 },
            end: Pos { row: 1, col: 6 },
        };
        assert!(!r2.is_empty());
    }

    #[test]
    fn set_empty_range_clears_selection() {
        let t = Terminal::new(3, 10, 0);
        let mut s = Selection::default();
        s.set(&t, vp_range(0, 0, 0, 5));
        assert!(!s.is_empty());
        s.set(&t, vp_range(0, 0, 0, 0));
        assert!(s.is_empty(), "empty range must collapse to None on set()");
    }

    #[test]
    fn clear_resets_state() {
        let t = Terminal::new(3, 10, 0);
        let mut s = Selection::default();
        s.set(&t, vp_range(0, 0, 1, 3));
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
        assert!(s.range_abs().is_none());
    }

    #[test]
    fn set_normalizes_reversed_range() {
        let t = Terminal::new(3, 10, 0);
        let mut s = Selection::default();
        s.set(&t, vp_range(2, 5, 0, 0));
        let r = s.range_in_viewport(&t).unwrap();
        assert_eq!(r.start, Pos { row: 0, col: 0 });
        assert_eq!(r.end, Pos { row: 2, col: 5 });
    }

    #[test]
    fn hard_wrapped_rows_join_with_newline() {
        let mut t = Terminal::new(2, 10, 0);
        t.feed(b"abc\r\ndef");
        let mut s = Selection::default();
        s.select_all(&t);
        let text = s.text(&t);
        assert!(text.contains("abc"));
        assert!(text.contains("def"));
        assert!(
            text.contains('\n'),
            "hard-wrapped rows must be separated by \\n: got {:?}",
            text
        );
    }

    #[test]
    fn select_word_at_col_zero_selects_first_word() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"first second third");
        let mut s = Selection::default();
        s.select_word(&t, 0, 0);
        assert_eq!(s.text(&t), "first");
    }

    // ─── §1.20 abs-row scroll-stable behavior ─────────────────────

    #[test]
    fn selection_survives_scroll_into_scrollback() {
        // 3-row viewport, write 8 lines so 5 spill into scrollback.
        let mut t = Terminal::new(3, 10, 100);
        for i in 0..8 {
            t.feed(format!("line{}\n", i).as_bytes());
        }
        // Live grid currently shows lines 5,6,7 at vp_row 0,1,2.
        // Select the middle live row "line6".
        let mut s = Selection::default();
        s.select_line(&t, 1);
        let abs_before = s.range_abs().unwrap();
        // Scroll up 3 rows — abs-row range MUST NOT change.
        t.scroll_up_view(3);
        let abs_after = s.range_abs().unwrap();
        assert_eq!(
            abs_before, abs_after,
            "abs-row range must be invariant under scroll"
        );
    }

    #[test]
    fn range_in_viewport_translates_with_scroll() {
        // 5-row viewport, scrollback up to 100, write 10 lines.
        let mut t = Terminal::new(5, 10, 100);
        for i in 0..10 {
            t.feed(format!("ln{}\n", i).as_bytes());
        }
        // After 10 lines feed, 5 in scrollback + 5 in grid.
        // Select line at vp_row 4 (last live grid row).
        let mut s = Selection::default();
        s.select_line(&t, 4);
        let r0 = s.range_in_viewport(&t).unwrap();
        assert_eq!(r0.start.row, 4);
        assert_eq!(r0.end.row, 4);
        // Scroll up 1 — original vp_row 4 (newest live grid row) gets
        // pushed below the viewport's bottom edge → should clip to None.
        t.scroll_up_view(1);
        let r1 = s.range_in_viewport(&t);
        assert!(
            r1.is_none(),
            "originally-bottom row must clip after scroll up; got {:?}",
            r1
        );
    }

    #[test]
    fn empty_terminal_select_all_is_safe() {
        let t = Terminal::new(0, 0, 0);
        let mut s = Selection::default();
        s.select_all(&t);
        assert!(s.is_empty());
    }
}
