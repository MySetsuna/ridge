//! Single grid cell + a row container.
//!
//! `Cell` is 8 bytes: `char` (4B, valid Unicode scalar) + `AttrId` (2B) +
//! `width` (1B: 0/1/2) + 1B padding. We keep `char` rather than indexing
//! a glyph atlas because (a) the renderer hasn't been written yet, and (b)
//! emoji + CJK don't fit cleanly into a 16-bit glyph index anyway.

use super::attr_table::AttrId;

/// One grid cell. `width` carries the result of `wcwidth` so the renderer
/// (and selection logic) can skip the second half of a wide cell without
/// re-running the unicode-width table.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub attr: AttrId,
    /// 0 = continuation half of a wide cell (renderer must skip),
    /// 1 = normal cell, 2 = first half of a wide cell.
    pub width: u8,
    _pad: u8,
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        ch: ' ',
        attr: AttrId::DEFAULT,
        width: 1,
        _pad: 0,
    };

    pub fn new(ch: char, attr: AttrId, width: u8) -> Self {
        Self { ch, attr, width, _pad: 0 }
    }

    pub fn is_blank(self) -> bool {
        self.ch == ' ' && self.attr == AttrId::DEFAULT
    }

    /// Continuation half written to position N+1 when a wide cell is placed
    /// at N. Carries the same attr so the bg color spans both halves.
    pub fn wide_spacer(attr: AttrId) -> Self {
        Self { ch: '\0', attr, width: 0, _pad: 0 }
    }
}

impl Default for Cell {
    fn default() -> Self { Self::EMPTY }
}

/// One OSC 8 hyperlink range on a row: cells `[col_start, col_end)` are
/// part of the same link. Coalesced with adjacent same-(uri,id) writes
/// at insertion time so a 12-char filename only produces one span, not 12.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperlinkSpan {
    pub col_start: usize,
    pub col_end: usize,
    pub uri: String,
    pub id: Option<String>,
}

/// One row. Wraps `Vec<Cell>` plus a small amount of per-row metadata
/// (wrap flag for reflow, hyperlink spans for OSC 8 click-through).
#[derive(Debug, Clone)]
pub struct Row {
    pub cells: Vec<Cell>,
    /// Set when this row's last cell wrapped to the next row. xterm needs
    /// this to glue soft-wrapped lines back together on copy/reflow.
    /// Not consumed yet — placeholder for resize reflow work.
    pub wrapped: bool,
    /// OSC 8 hyperlink spans active on this row. Empty for rows without
    /// links (the common case). Spans are stored in scan order; a
    /// quick linear lookup via `link_at` finds the span containing a col.
    pub hyperlinks: Vec<HyperlinkSpan>,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::EMPTY; cols],
            wrapped: false,
            hyperlinks: Vec::new(),
        }
    }

    /// Resize in-place. Growth pads with EMPTY; shrink truncates.
    /// Hyperlink spans past the new width are dropped; spans straddling
    /// the boundary get clipped (col_end clamped to new cols).
    pub fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::EMPTY);
        if !self.hyperlinks.is_empty() {
            self.hyperlinks.retain(|s| s.col_start < cols);
            for s in &mut self.hyperlinks {
                if s.col_end > cols {
                    s.col_end = cols;
                }
            }
        }
    }

    /// Reset all cells to default + clear wrap flag + drop hyperlinks.
    /// Used by ED (erase display) and when scrollback ejects a row back
    /// into the grid.
    pub fn clear(&mut self) {
        for c in &mut self.cells {
            *c = Cell::EMPTY;
        }
        self.wrapped = false;
        self.hyperlinks.clear();
    }

    /// Return the hyperlink span containing `col`, if any. O(N) over
    /// spans on this row — expected to be 0..3 for typical rows.
    pub fn link_at(&self, col: usize) -> Option<&HyperlinkSpan> {
        self.hyperlinks.iter().find(|s| col >= s.col_start && col < s.col_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Cell ────────────────────────────────────────────────────────

    #[test]
    fn cell_empty_is_blank() {
        assert!(Cell::EMPTY.is_blank());
    }

    #[test]
    fn cell_default_equals_empty() {
        let d: Cell = Default::default();
        assert_eq!(d.ch, Cell::EMPTY.ch);
        assert_eq!(d.attr, Cell::EMPTY.attr);
        assert_eq!(d.width, Cell::EMPTY.width);
    }

    #[test]
    fn cell_is_blank_requires_space_and_default_attr() {
        // Space + default attr → blank.
        let blank = Cell::new(' ', AttrId::DEFAULT, 1);
        assert!(blank.is_blank());

        // Non-space char → NOT blank, even at default attr.
        let letter = Cell::new('a', AttrId::DEFAULT, 1);
        assert!(!letter.is_blank());

        // Space at non-default attr → NOT blank (paints colored bg
        // even though glyph is invisible — counts as content).
        let colored_space = Cell::new(' ', AttrId(7), 1);
        assert!(!colored_space.is_blank());
    }

    #[test]
    fn cell_new_records_all_fields() {
        let c = Cell::new('X', AttrId(42), 2);
        assert_eq!(c.ch, 'X');
        assert_eq!(c.attr, AttrId(42));
        assert_eq!(c.width, 2);
    }

    #[test]
    fn cell_wide_spacer_is_continuation_half() {
        // Width-0 continuation cell sitting in slot N+1 of a wide
        // glyph rendered at slot N. Carries the parent cell's attr
        // so bg color spans both halves of the wide cell.
        let spacer = Cell::wide_spacer(AttrId(99));
        assert_eq!(spacer.ch, '\0');
        assert_eq!(spacer.width, 0);
        assert_eq!(spacer.attr, AttrId(99));
    }

    // ─── Row ─────────────────────────────────────────────────────────

    #[test]
    fn row_new_initializes_empty_cells_and_no_metadata() {
        let r = Row::new(5);
        assert_eq!(r.cells.len(), 5);
        for c in &r.cells {
            assert!(c.is_blank());
        }
        assert!(!r.wrapped);
        assert!(r.hyperlinks.is_empty());
    }

    #[test]
    fn row_clear_resets_cells_wrap_and_hyperlinks() {
        let mut r = Row::new(5);
        r.cells[0] = Cell::new('a', AttrId(1), 1);
        r.cells[1] = Cell::new('b', AttrId(2), 1);
        r.wrapped = true;
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 0, col_end: 2, uri: "u".into(), id: None,
        });
        r.clear();
        assert!(r.cells.iter().all(|c| c.is_blank()));
        assert!(!r.wrapped);
        assert!(r.hyperlinks.is_empty());
    }

    #[test]
    fn row_resize_grow_pads_with_empty() {
        let mut r = Row::new(3);
        r.cells[0] = Cell::new('x', AttrId::DEFAULT, 1);
        r.resize(6);
        assert_eq!(r.cells.len(), 6);
        assert_eq!(r.cells[0].ch, 'x');
        // Newly-allocated tail cells are blank.
        for i in 3..6 {
            assert!(r.cells[i].is_blank());
        }
    }

    #[test]
    fn row_resize_shrink_truncates_cells() {
        let mut r = Row::new(8);
        for i in 0..8 {
            r.cells[i] = Cell::new(char::from_u32(b'a' as u32 + i as u32).unwrap(), AttrId::DEFAULT, 1);
        }
        r.resize(3);
        assert_eq!(r.cells.len(), 3);
        assert_eq!(r.cells[0].ch, 'a');
        assert_eq!(r.cells[2].ch, 'c');
    }

    #[test]
    fn row_resize_drops_hyperlinks_past_new_width() {
        let mut r = Row::new(20);
        r.hyperlinks.push(HyperlinkSpan { col_start: 0, col_end: 3, uri: "a".into(), id: None });
        r.hyperlinks.push(HyperlinkSpan { col_start: 8, col_end: 12, uri: "b".into(), id: None });
        r.hyperlinks.push(HyperlinkSpan { col_start: 15, col_end: 18, uri: "c".into(), id: None });
        // Shrink to 10 cols. Span 'a' (0..3) survives; 'b' (8..12)
        // straddles, col_end clamped; 'c' (15..18) starts past width,
        // dropped.
        r.resize(10);
        assert_eq!(r.hyperlinks.len(), 2);
        assert_eq!(r.hyperlinks[0].uri, "a");
        assert_eq!(r.hyperlinks[1].uri, "b");
        assert_eq!(r.hyperlinks[1].col_end, 10);
    }

    #[test]
    fn row_resize_keeps_all_hyperlinks_when_growing() {
        let mut r = Row::new(5);
        r.hyperlinks.push(HyperlinkSpan { col_start: 0, col_end: 5, uri: "u".into(), id: None });
        r.resize(10);
        assert_eq!(r.hyperlinks.len(), 1);
        assert_eq!(r.hyperlinks[0].col_end, 5);
    }

    // ─── link_at ─────────────────────────────────────────────────────

    #[test]
    fn link_at_returns_none_on_empty_row() {
        let r = Row::new(10);
        assert!(r.link_at(0).is_none());
        assert!(r.link_at(5).is_none());
    }

    #[test]
    fn link_at_finds_span_containing_col() {
        let mut r = Row::new(10);
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 2, col_end: 7, uri: "u".into(), id: Some("anchor".into()),
        });
        // Inside range.
        assert_eq!(r.link_at(2).unwrap().uri, "u");
        assert_eq!(r.link_at(5).unwrap().uri, "u");
        assert_eq!(r.link_at(6).unwrap().uri, "u");
        // Boundary semantics: col_start inclusive, col_end exclusive.
        assert!(r.link_at(7).is_none());
        // Below start.
        assert!(r.link_at(1).is_none());
        // Far past end.
        assert!(r.link_at(9).is_none());
    }

    #[test]
    fn link_at_picks_span_when_multiple_present() {
        let mut r = Row::new(20);
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 0, col_end: 5, uri: "first".into(), id: None,
        });
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 10, col_end: 15, uri: "second".into(), id: None,
        });
        assert_eq!(r.link_at(2).unwrap().uri, "first");
        assert_eq!(r.link_at(12).unwrap().uri, "second");
        // Gap between spans.
        assert!(r.link_at(7).is_none());
    }
}
