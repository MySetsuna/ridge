//! In-pane text search.
//!
//! Round 4: case-insensitive (toggleable) substring search across **both**
//! scrollback rows AND the viewport. Each call to `set_query` rebuilds
//! the match list — it's an O((scrollback + rows) × cols) scan, but the
//! data sets here (≤2000 scrollback + 24..200 viewport, typical cols 80)
//! make this cheap enough to skip incremental tracking.
//!
//! Match positions are stored in an "absolute" row coordinate space:
//!   * `0 .. scrollback.len()` → scrollback rows, oldest first
//!   * `scrollback.len() .. scrollback.len() + rows` → viewport rows top→bottom
//!
//! The caller (`JsTerminal`) owns the policy of "scroll the viewport so the
//! active match becomes visible" — this module only finds the matches and
//! provides `match_to_viewport_range` to convert an active match into a
//! viewport-relative `Range` given the chosen `scroll_offset`.

use crate::selection::{Pos, Range};
use crate::term::Terminal;

/// One match, stored in absolute-row coords (see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchAbs {
    pub abs_row: usize,
    pub col_start: usize,
    pub col_end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// Last query that produced `matches`. Used to short-circuit re-search
    /// when the caller hasn't actually changed anything.
    last_query: String,
    last_case_sensitive: bool,
    /// All matches in scan order (top→bottom, left→right). Absolute-row coords.
    matches: Vec<MatchAbs>,
    /// Index into `matches` of the currently-active match. None when
    /// `matches` is empty or the user explicitly cleared.
    active: Option<usize>,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recompute matches for `query` against the terminal's current
    /// viewport. Returns the number of matches found. Sets `active` to
    /// the first match (Some(0)) if any, else None.
    pub fn set_query(&mut self, terminal: &Terminal, query: &str, case_sensitive: bool) -> usize {
        // Fast path: same query AND case → keep existing matches/active.
        // Caller can force a refresh by passing an empty query first.
        if !self.matches.is_empty()
            && query == self.last_query
            && case_sensitive == self.last_case_sensitive
        {
            return self.matches.len();
        }
        self.last_query = query.to_string();
        self.last_case_sensitive = case_sensitive;
        self.matches.clear();
        self.active = None;
        if query.is_empty() {
            return 0;
        }
        let needle: Vec<char> = if case_sensitive {
            query.chars().collect()
        } else {
            query.to_lowercase().chars().collect()
        };
        if needle.is_empty() {
            return 0;
        }

        let sb_len = terminal.scrollback_len();
        // Pass 1: scrollback (oldest → newest). Direct access via grid
        // bypasses viewport_row's scroll-offset logic — search must scan
        // the full state regardless of where the user has scrolled to.
        for sb_idx in 0..sb_len {
            let Some(row) = terminal.grid().scrollback.get(sb_idx) else {
                continue;
            };
            scan_row_into(
                &row.cells,
                &needle,
                case_sensitive,
                sb_idx,
                &mut self.matches,
            );
        }
        // Pass 2: viewport rows (top → bottom). Same direct grid access.
        let rows_n = terminal.rows();
        for r in 0..rows_n {
            let Some(row) = terminal.grid().row(r) else {
                continue;
            };
            scan_row_into(
                &row.cells,
                &needle,
                case_sensitive,
                sb_len + r,
                &mut self.matches,
            );
        }
        if !self.matches.is_empty() {
            self.active = Some(0);
        }
        self.matches.len()
    }

    /// Convert an absolute-row match into a viewport-relative `Range`
    /// given the supplied `scroll_offset` and terminal dimensions. The
    /// caller is expected to set scroll_offset first via the policy
    /// returned by `desired_scroll_offset_for`.
    pub fn match_to_viewport_range(
        m: MatchAbs,
        scroll_offset: usize,
        scrollback_len: usize,
        rows_n: usize,
    ) -> Option<Range> {
        // Where does this absolute row land in the current viewport?
        // viewport_row(vp) at offset N maps to:
        //   vp < N  → scrollback[sb_len - N + vp]
        //   vp >= N → grid.row(vp - N)
        // Inverse:
        //   abs_row in scrollback (< sb_len): vp = abs_row - (sb_len - N) = abs_row - sb_len + N
        //   abs_row in viewport (>= sb_len):  vp = abs_row - sb_len + N
        // So unified: vp = (abs_row + N).checked_sub(sb_len)? — but only if vp < rows_n.
        let vp = (m.abs_row + scroll_offset).checked_sub(scrollback_len)?;
        if vp >= rows_n {
            return None;
        }
        Some(Range {
            start: Pos {
                row: vp,
                col: m.col_start,
            },
            end: Pos {
                row: vp,
                col: m.col_end,
            },
        })
    }

    /// Choose a scroll_offset that brings the match into view at vp_row 0
    /// (top of viewport). For viewport matches returns 0 (live grid).
    /// For scrollback matches returns `sb_len - abs_row`, capped at sb_len.
    pub fn desired_scroll_offset_for(m: MatchAbs, scrollback_len: usize) -> usize {
        if m.abs_row >= scrollback_len {
            0 // already in viewport — show at live grid
        } else {
            (scrollback_len - m.abs_row).min(scrollback_len)
        }
    }

    /// Advance to the next match (wraps around). Returns the active match
    /// after the advance, or None if no matches exist.
    pub fn next(&mut self) -> Option<MatchAbs> {
        if self.matches.is_empty() {
            return None;
        }
        let new_idx = match self.active {
            None => 0,
            Some(i) => (i + 1) % self.matches.len(),
        };
        self.active = Some(new_idx);
        Some(self.matches[new_idx])
    }

    /// Step backwards (wraps around).
    pub fn prev(&mut self) -> Option<MatchAbs> {
        if self.matches.is_empty() {
            return None;
        }
        let new_idx = match self.active {
            None => self.matches.len() - 1,
            Some(0) => self.matches.len() - 1,
            Some(i) => i - 1,
        };
        self.active = Some(new_idx);
        Some(self.matches[new_idx])
    }

    pub fn clear(&mut self) {
        self.matches.clear();
        self.active = None;
        self.last_query.clear();
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }

    pub fn active_index(&self) -> Option<usize> {
        self.active
    }

    pub fn active_match(&self) -> Option<MatchAbs> {
        self.active.and_then(|i| self.matches.get(i).copied())
    }
}

/// Helper: scan one row's cells for `needle` and push every match into
/// `out` with the supplied absolute row index. Skips width-0 (continuation
/// halves) so column arithmetic stays in cell space.
fn scan_row_into(
    cells: &[crate::term::cell::Cell],
    needle: &[char],
    case_sensitive: bool,
    abs_row: usize,
    out: &mut Vec<MatchAbs>,
) {
    let mut chars: Vec<char> = Vec::with_capacity(cells.len());
    let mut char_to_cell: Vec<usize> = Vec::with_capacity(cells.len());
    for (col, cell) in cells.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        let ch = if case_sensitive {
            cell.ch
        } else {
            cell.ch.to_ascii_lowercase()
        };
        chars.push(ch);
        char_to_cell.push(col);
    }
    if chars.len() < needle.len() {
        return;
    }
    let max_start = chars.len() - needle.len() + 1;
    for start in 0..max_start {
        if chars[start..start + needle.len()] == needle[..] {
            let col_start = char_to_cell[start];
            let last = start + needle.len() - 1;
            let last_cell = char_to_cell[last];
            let end_cell = if last_cell + 1 < cells.len() && cells[last_cell].width == 2 {
                last_cell + 2
            } else {
                last_cell + 1
            };
            out.push(MatchAbs {
                abs_row,
                col_start,
                col_end: end_cell,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_substring_in_single_row() {
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"hello world hello!");
        let mut s = SearchState::new();
        let n = s.set_query(&t, "hello", true);
        assert_eq!(n, 2);
    }

    #[test]
    fn case_insensitive_finds_mixed_case() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"Hello HELLO hello");
        let mut s = SearchState::new();
        assert_eq!(s.set_query(&t, "hello", false), 3);
    }

    #[test]
    fn case_sensitive_distinguishes() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"Hello HELLO hello");
        let mut s = SearchState::new();
        assert_eq!(s.set_query(&t, "Hello", true), 1);
    }

    #[test]
    fn next_and_prev_wrap_around() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"abc abc abc");
        let mut s = SearchState::new();
        s.set_query(&t, "abc", true);
        assert_eq!(s.active_index(), Some(0));
        s.next();
        assert_eq!(s.active_index(), Some(1));
        s.next();
        assert_eq!(s.active_index(), Some(2));
        s.next();
        assert_eq!(s.active_index(), Some(0));
        s.prev();
        assert_eq!(s.active_index(), Some(2));
    }

    #[test]
    fn empty_query_clears_state() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"hello");
        let mut s = SearchState::new();
        s.set_query(&t, "hello", true);
        assert_eq!(s.match_count(), 1);
        s.set_query(&t, "", true);
        assert_eq!(s.match_count(), 0);
        assert!(s.active_index().is_none());
    }

    #[test]
    fn no_matches_when_query_absent() {
        let mut t = Terminal::new(1, 10, 0);
        t.feed(b"abcdef");
        let mut s = SearchState::new();
        assert_eq!(s.set_query(&t, "xyz", true), 0);
        assert!(s.next().is_none());
    }

    #[test]
    fn search_finds_match_in_scrollback() {
        // 2-row terminal with 10 rows of scrollback. Push 5 lines so 3 of
        // them go into scrollback and 2 stay in viewport.
        let mut t = Terminal::new(2, 10, 10);
        for line in ["needle1", "filler1", "filler2", "filler3", "needle2"] {
            t.feed(line.as_bytes());
            t.feed(b"\r\n");
        }
        // After 5 LFs at the bottom: 3 lines should be in scrollback.
        assert!(t.scrollback_len() >= 1, "scrollback should have entries");
        let mut s = SearchState::new();
        let n = s.set_query(&t, "needle", true);
        assert_eq!(n, 2, "should find both 'needle1' and 'needle2'");
        let first = s.active_match().unwrap();
        // First match (needle1) should be in scrollback territory.
        assert!(
            first.abs_row < t.scrollback_len(),
            "first match abs_row {} should land in scrollback (sb_len {})",
            first.abs_row,
            t.scrollback_len(),
        );
    }

    #[test]
    fn match_to_viewport_range_translates_correctly() {
        // sb_len = 5, rows = 3. abs_row 0..5 = scrollback, 5..8 = viewport.
        // To bring scrollback[2] (oldest scrollback) to top of viewport,
        // we'd choose offset = sb_len - 2 = 3. Then vp_row 0 should map.
        let m = MatchAbs {
            abs_row: 2,
            col_start: 4,
            col_end: 7,
        };
        let r = SearchState::match_to_viewport_range(m, 3, 5, 3).unwrap();
        assert_eq!(r.start.row, 0);
        assert_eq!(r.start.col, 4);
        assert_eq!(r.end.col, 7);

        // Viewport match at abs_row 6 (= viewport row 1), offset 0:
        // vp = (6 + 0) - 5 = 1. ✓
        let m = MatchAbs {
            abs_row: 6,
            col_start: 0,
            col_end: 3,
        };
        let r = SearchState::match_to_viewport_range(m, 0, 5, 3).unwrap();
        assert_eq!(r.start.row, 1);
    }

    #[test]
    fn match_to_viewport_range_returns_none_when_above_visible_window() {
        // sb_len=5, rows=3, offset=0 → visible rows are abs 5..8.
        // A match at abs_row=2 is in scrollback that's NOT scrolled
        // into view → returns None.
        let m = MatchAbs {
            abs_row: 2,
            col_start: 0,
            col_end: 3,
        };
        assert!(SearchState::match_to_viewport_range(m, 0, 5, 3).is_none());
    }

    #[test]
    fn match_to_viewport_range_returns_none_when_below_visible_window() {
        // sb_len=5, rows=3, offset=3 → visible rows are abs 2..5
        // (top-of-viewport = sb_len - offset = 2). A live-grid match
        // at abs_row=7 is below the visible window.
        let m = MatchAbs {
            abs_row: 7,
            col_start: 0,
            col_end: 3,
        };
        assert!(SearchState::match_to_viewport_range(m, 3, 5, 3).is_none());
    }

    #[test]
    fn desired_scroll_offset_recent_grid_match_returns_zero() {
        // Match in live grid (abs_row >= sb_len) needs no scrollback —
        // already visible at offset=0.
        let m = MatchAbs {
            abs_row: 6,
            col_start: 0,
            col_end: 3,
        };
        let off = SearchState::desired_scroll_offset_for(m, /*sb_len=*/ 5);
        assert_eq!(off, 0);
    }

    #[test]
    fn desired_scroll_offset_for_oldest_scrollback_returns_full_offset() {
        // To bring abs_row=0 (oldest scrollback) to viewport top,
        // user must scroll back the full scrollback length.
        let m = MatchAbs {
            abs_row: 0,
            col_start: 0,
            col_end: 3,
        };
        let off = SearchState::desired_scroll_offset_for(m, /*sb_len=*/ 10);
        assert_eq!(off, 10);
    }

    #[test]
    fn clear_resets_matches_and_active_index() {
        let mut t = Terminal::new(1, 20, 0);
        t.feed(b"hello world");
        let mut s = SearchState::new();
        s.set_query(&t, "hello", true);
        assert_eq!(s.match_count(), 1);
        assert!(s.active_index().is_some());
        s.clear();
        assert_eq!(s.match_count(), 0);
        assert!(s.active_index().is_none());
        assert!(s.active_match().is_none());
    }

    #[test]
    fn match_count_zero_for_fresh_state() {
        let s = SearchState::new();
        assert_eq!(s.match_count(), 0);
        assert!(s.active_index().is_none());
        assert!(s.active_match().is_none());
    }
}
