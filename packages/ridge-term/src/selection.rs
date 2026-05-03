//! Selection model — minimum skeleton for round 2.3.
//!
//! This round provides:
//!   - `set_range(start, end)` — programmatic selection (e.g. selectAll)
//!   - `clear()`
//!   - `text(terminal)` — extract the selected text, joining soft-wrapped
//!     rows correctly (no `stripSoftWraps` workaround needed)
//!
//! Deferred to round 4:
//!   - Mouse drag to select (manager-level pointer routing)
//!   - Word/line selection (double/triple click)
//!   - Selection across scrollback boundaries (read scrollback rows)
//!   - Highlight rendering (the renderer paints selection bg over cells)
//!
//! ## Coordinate system
//! Positions are `(row, col)` in viewport-relative coordinates — same
//! space the renderer uses. A selection range covers all cells from
//! `start` (inclusive) to `end` (exclusive), traversing left-to-right
//! within a row and top-to-bottom across rows.

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

#[derive(Debug, Clone, Copy, Default)]
pub struct Selection {
    range: Option<Range>,
}

impl Selection {
    pub fn new() -> Self { Self::default() }

    pub fn set(&mut self, range: Range) {
        if range.is_empty() {
            self.range = None;
        } else {
            self.range = Some(range.normalized());
        }
    }

    pub fn clear(&mut self) {
        self.range = None;
    }

    pub fn range(&self) -> Option<Range> { self.range }

    pub fn is_empty(&self) -> bool { self.range.is_none() }

    /// Programmatic "select all visible". Selects the entire viewport.
    pub fn select_all(&mut self, terminal: &Terminal) {
        let rows = terminal.rows();
        let cols = terminal.cols();
        if rows == 0 || cols == 0 {
            self.range = None;
            return;
        }
        self.range = Some(Range {
            start: Pos { row: 0, col: 0 },
            end: Pos { row: rows - 1, col: cols },
        });
    }

    /// Double-click word selection. Selects the contiguous run of
    /// non-whitespace, non-NUL chars containing the clicked cell. If
    /// the clicked cell IS whitespace or empty, clears the selection
    /// (matches xterm).
    pub fn select_word(&mut self, terminal: &Terminal, row: usize, col: usize) {
        let Some(r) = terminal.viewport_row(row) else {
            self.range = None;
            return;
        };
        let n = r.cells.len();
        if col >= n {
            self.range = None;
            return;
        }
        let is_word = |ch: char| !ch.is_whitespace() && ch != '\0';
        if !is_word(r.cells[col].ch) {
            self.range = None;
            return;
        }
        let mut lo = col;
        while lo > 0 && is_word(r.cells[lo - 1].ch) { lo -= 1; }
        let mut hi = col + 1;
        while hi < n && is_word(r.cells[hi].ch) { hi += 1; }
        self.range = Some(Range {
            start: Pos { row, col: lo },
            end: Pos { row, col: hi },
        });
    }

    /// Triple-click line selection. Selects the entire row.
    pub fn select_line(&mut self, terminal: &Terminal, row: usize) {
        let cols = terminal.cols();
        if row >= terminal.rows() || cols == 0 {
            self.range = None;
            return;
        }
        self.range = Some(Range {
            start: Pos { row, col: 0 },
            end: Pos { row, col: cols },
        });
    }

    /// Extract the selected text. Soft-wrapped lines (rows with `wrapped =
    /// true`) are joined without inserting a newline — the original cells
    /// were one logical line. Hard line breaks (`wrapped = false` rows
    /// that aren't the last selected row) get a `\n`.
    ///
    /// Trailing whitespace per logical line is trimmed (matches the user
    /// expectation for "copy a line of text" — `xterm` does the same).
    pub fn text(&self, terminal: &Terminal) -> String {
        let Some(range) = self.range else { return String::new() };
        let rows = terminal.rows();
        let cols = terminal.cols();
        let mut out = String::new();

        for r in range.start.row..=range.end.row.min(rows.saturating_sub(1)) {
            let Some(row) = terminal.viewport_row(r) else { break };

            // Column bounds within this row.
            let lo = if r == range.start.row { range.start.col } else { 0 };
            let hi = if r == range.end.row { range.end.col } else { cols };
            let hi = hi.min(row.cells.len());

            // Collect cells, skipping width-0 (continuation halves).
            let mut line = String::new();
            for col in lo..hi {
                let cell = &row.cells[col];
                if cell.width == 0 { continue; }
                line.push(cell.ch);
            }

            // Trim trailing spaces from this row's contribution (visual
            // tidiness for typical terminal output).
            let trimmed = line.trim_end_matches(' ');
            out.push_str(trimmed);

            // Decide whether to insert a line separator. If this row was
            // soft-wrapped to the next, no newline. If this is the last
            // selected row, no newline either. Otherwise '\n'.
            let is_last = r == range.end.row;
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
            end:   Pos { row: 0, col: 0 },
        }.normalized();
        assert_eq!(r.start, Pos { row: 0, col: 0 });
        assert_eq!(r.end,   Pos { row: 2, col: 5 });
    }

    #[test]
    fn select_word_finds_contiguous_non_whitespace() {
        let mut t = Terminal::new(1, 30, 0);
        t.feed(b"hello   world.txt   foo");
        let mut s = Selection::default();
        // Click on the 'o' of "world" → should select "world.txt".
        s.select_word(&t, 0, 11);
        assert_eq!(s.text(&t), "world.txt");
    }

    #[test]
    fn select_word_on_whitespace_clears() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"hello   world");
        let mut s = Selection::default();
        s.set(Range {
            start: Pos { row: 0, col: 0 },
            end: Pos { row: 0, col: 5 },
        });
        s.select_word(&t, 0, 6); // a space
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
        // 4-col terminal, write 6 chars → row 0 wraps to row 1.
        let mut t = Terminal::new(3, 4, 0);
        t.feed(b"abcdef");
        let mut s = Selection::default();
        s.select_all(&t);
        let text = s.text(&t);
        // Row 0 was soft-wrapped, so no '\n' between "abcd" and "ef".
        assert!(text.starts_with("abcdef"), "got {:?}", text);
    }
}
