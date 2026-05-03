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
