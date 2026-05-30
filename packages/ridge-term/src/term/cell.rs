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
        Self {
            ch,
            attr,
            width,
            _pad: 0,
        }
    }

    pub fn is_blank(self) -> bool {
        self.ch == ' ' && self.attr == AttrId::DEFAULT
    }

    /// Continuation half written to position N+1 when a wide cell is placed
    /// at N. Carries the same attr so the bg color spans both halves.
    pub fn wide_spacer(attr: AttrId) -> Self {
        Self {
            ch: '\0',
            attr,
            width: 0,
            _pad: 0,
        }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::EMPTY
    }
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

/// Render path classification for a line of cells. Determines whether
/// the line can take the fast equal-width path or needs the slow
/// cluster-aware path with text shaping and consume tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderPath {
    /// All cells are pure ASCII or standard single-width chars. No clusters,
    /// no wide characters, no color emoji. Skip complex shaping entirely.
    Fast,
    /// Row contains wide chars, clusters, or potential color emoji. Needs
    /// cluster-aware rendering with consume tracking and measured widths.
    Slow,
}

/// Efficient single-line scan that classifies a row's render path.
///
/// **Fast-Path**: If every cell is a pure ASCII character (codepoint ≤ 0x7F)
/// with width 1 and no cluster spans, the line can skip complex text shaping
/// and consume logic entirely. This is the common case for code and log output.
///
/// **Slow-Path**: Triggered when ANY cell has width ≥ 2, a non-ASCII
/// codepoint, or cluster spans are present. These lines need cluster-aware
/// rendering with measured glyph widths and right-side cell consume tracking.
///
/// The function is deliberately lightweight — no allocations, no lookups,
/// just a linear scan with early exit on the first non-trivial character.
pub fn scan_line_path(cells: &[Cell], clusters: &[ClusterSpan]) -> RenderPath {
    if !clusters.is_empty() {
        return RenderPath::Slow;
    }
    for cell in cells {
        if cell.width >= 2 {
            return RenderPath::Slow;
        }
        let cp = cell.ch as u32;
        if cp > 0x7F && cell.ch != '\0' {
            return RenderPath::Slow;
        }
    }
    RenderPath::Fast
}

/// §4.7 (2026-05-07) — multi-codepoint grapheme cluster anchored at a
/// specific column on a row. Used so emoji ZWJ sequences (👨‍👩‍👧),
/// flag-style RIS pairs (🇺🇸), and emoji-with-VS16 (🏳️‍🌈) survive as
/// a single visual glyph instead of fanning out across N cells per
/// codepoint. The cell at `col` carries the FIRST codepoint of the
/// cluster (so per-cell hashing / reflow / selection still see *some*
/// glyph there); renderers that find a matching `ClusterSpan` use
/// `text` instead of `cell.ch`. Ordered by `col`; a row typically has
/// 0–2 clusters in non-emoji-heavy output, so linear scan is fine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClusterSpan {
    pub col: u16,
    pub text: Box<str>,
}

/// One row. Wraps `Vec<Cell>` plus a small amount of per-row metadata
/// (wrap flag for reflow, hyperlink spans for OSC 8 click-through,
/// grapheme cluster spans for ZWJ-emoji rendering).
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
    /// §4.7 grapheme cluster overrides. Empty for the common ASCII /
    /// CJK case — only populated when the parser saw a multi-codepoint
    /// extended grapheme cluster (emoji ZWJ sequences etc.). Renderers
    /// must check `cluster_at(col)` before falling back to `cell.ch`.
    pub clusters: Vec<ClusterSpan>,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::EMPTY; cols],
            wrapped: false,
            hyperlinks: Vec::new(),
            clusters: Vec::new(),
        }
    }

    /// Resize in-place. Growth pads with EMPTY; shrink truncates.
    /// Hyperlink spans past the new width are dropped; spans straddling
    /// the boundary get clipped (col_end clamped to new cols). Cluster
    /// spans past the new width are dropped (a cluster lives at exactly
    /// one column — no straddle case).
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
        if !self.clusters.is_empty() {
            let cols_u16 = cols.min(u16::MAX as usize) as u16;
            self.clusters.retain(|c| c.col < cols_u16);
        }
    }

    /// Reset all cells to default + clear wrap flag + drop hyperlinks
    /// + drop cluster overrides. Used by ED (erase display) and when
    /// scrollback ejects a row back into the grid.
    pub fn clear(&mut self) {
        self.fill_blank(Cell::EMPTY);
    }

    /// Like `clear()` but fills every cell with the provided blank cell
    /// instead of `Cell::EMPTY`. Used by BCE (Background Color Erase):
    /// erase / scroll / IL / DL paths build a `Cell { ch: ' ', attr:
    /// <SGR bg attrs> }` and fill rows with it so xterm-compatible
    /// "colour-fill on erase" works without each call site duplicating
    /// the wrap/hyperlink/cluster reset.
    pub fn fill_blank(&mut self, blank: Cell) {
        for c in &mut self.cells {
            *c = blank;
        }
        self.wrapped = false;
        self.hyperlinks.clear();
        self.clusters.clear();
    }

    /// §4.7 — return the cluster span anchored at `col` if any. O(N)
    /// linear scan; expected N is 0 for the common case and small
    /// (<10) even for emoji-heavy rows.
    pub fn cluster_at(&self, col: usize) -> Option<&ClusterSpan> {
        if self.clusters.is_empty() {
            return None;
        }
        let target = col.min(u16::MAX as usize) as u16;
        self.clusters.iter().find(|c| c.col == target)
    }

    /// §4.7 — register a multi-codepoint grapheme cluster anchored at
    /// `col`. Idempotent: if a cluster already lives at `col` it's
    /// replaced. Single-codepoint "clusters" should NOT come through
    /// here — caller should put the codepoint in `Cell::ch` directly
    /// and skip the sidecar overhead.
    pub fn set_cluster(&mut self, col: usize, text: Box<str>) {
        let col_u16 = col.min(u16::MAX as usize) as u16;
        if let Some(existing) = self.clusters.iter_mut().find(|c| c.col == col_u16) {
            existing.text = text;
        } else {
            self.clusters.push(ClusterSpan { col: col_u16, text });
        }
    }

    /// §4.7 — drop any cluster anchored at `col`. Called on cell
    /// overwrite paths so a non-cluster write (regular ASCII / CJK)
    /// at a previously-clustered col doesn't leave a stale sidecar.
    pub fn clear_cluster_at(&mut self, col: usize) {
        if self.clusters.is_empty() {
            return;
        }
        let target = col.min(u16::MAX as usize) as u16;
        self.clusters.retain(|c| c.col != target);
    }

    /// §B.2 (2026-05-08) — drop every cluster sidecar whose anchor col
    /// falls inside `[start, end)`. Called by erase / shift paths
    /// (EL / ECH / DCH / ICH) so wiping or moving cells also drops
    /// the multi-codepoint cluster strings that were anchored on those
    /// cells. Without this the renderer's per-cell `cluster_at(col)`
    /// lookup keeps finding the original emoji string at a now-blank
    /// or now-shifted position, painting "ghost" emoji on cleared
    /// cells (the user-visible "退格出现乱码" symptom in cluster-
    /// rich rows).
    pub fn clear_clusters_in_range(&mut self, start: usize, end: usize) {
        if self.clusters.is_empty() || start >= end {
            return;
        }
        let lo = start.min(u16::MAX as usize) as u16;
        let hi = end.min(u16::MAX as usize) as u16;
        self.clusters.retain(|c| c.col < lo || c.col >= hi);
    }

    /// §B.2 — shift every cluster sidecar at col ≥ `at_or_after` LEFT
    /// by `by` cells. Used by DCH (delete-chars). Sidecars whose
    /// post-shift col is < `at_or_after - by` (i.e. would have been
    /// emitted from inside the deletion range — caller is expected to
    /// drop those first via `clear_clusters_in_range`) survive only
    /// when their original col ≥ at_or_after; this method assumes the
    /// deletion range was already cleared. Sidecars to the LEFT of
    /// `at_or_after` are untouched.
    pub fn shift_clusters_left(&mut self, at_or_after: usize, by: usize) {
        if self.clusters.is_empty() || by == 0 {
            return;
        }
        let pivot = at_or_after.min(u16::MAX as usize) as u16;
        let by_u16 = by.min(u16::MAX as usize) as u16;
        for c in &mut self.clusters {
            if c.col >= pivot {
                c.col = c.col.saturating_sub(by_u16);
            }
        }
    }

    /// §B.2 — shift every cluster sidecar at col ≥ `at_or_after` RIGHT
    /// by `by` cells, dropping any whose post-shift col would be ≥
    /// `max_cols`. Used by ICH (insert-chars).
    pub fn shift_clusters_right(&mut self, at_or_after: usize, by: usize, max_cols: usize) {
        if self.clusters.is_empty() || by == 0 {
            return;
        }
        let pivot = at_or_after.min(u16::MAX as usize) as u16;
        let by_u16 = by.min(u16::MAX as usize) as u16;
        let limit = max_cols.min(u16::MAX as usize) as u16;
        // Two-pass: shift in place, then drop overflow. Single-pass
        // retain_mut would be cleaner but stable Rust's `retain_mut`
        // only reads the closure's mutated value at the *next* call —
        // safer to keep semantics explicit.
        for c in &mut self.clusters {
            if c.col >= pivot {
                let shifted = c.col.saturating_add(by_u16);
                c.col = shifted;
            }
        }
        self.clusters.retain(|c| c.col < limit);
    }

    /// Return the hyperlink span containing `col`, if any. O(N) over
    /// spans on this row — expected to be 0..3 for typical rows.
    pub fn link_at(&self, col: usize) -> Option<&HyperlinkSpan> {
        self.hyperlinks
            .iter()
            .find(|s| col >= s.col_start && col < s.col_end)
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
            col_start: 0,
            col_end: 2,
            uri: "u".into(),
            id: None,
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
            r.cells[i] = Cell::new(
                char::from_u32(b'a' as u32 + i as u32).unwrap(),
                AttrId::DEFAULT,
                1,
            );
        }
        r.resize(3);
        assert_eq!(r.cells.len(), 3);
        assert_eq!(r.cells[0].ch, 'a');
        assert_eq!(r.cells[2].ch, 'c');
    }

    #[test]
    fn row_resize_drops_hyperlinks_past_new_width() {
        let mut r = Row::new(20);
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 3,
            uri: "a".into(),
            id: None,
        });
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 8,
            col_end: 12,
            uri: "b".into(),
            id: None,
        });
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 15,
            col_end: 18,
            uri: "c".into(),
            id: None,
        });
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
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 5,
            uri: "u".into(),
            id: None,
        });
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
            col_start: 2,
            col_end: 7,
            uri: "u".into(),
            id: Some("anchor".into()),
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
            col_start: 0,
            col_end: 5,
            uri: "first".into(),
            id: None,
        });
        r.hyperlinks.push(HyperlinkSpan {
            col_start: 10,
            col_end: 15,
            uri: "second".into(),
            id: None,
        });
        assert_eq!(r.link_at(2).unwrap().uri, "first");
        assert_eq!(r.link_at(12).unwrap().uri, "second");
        // Gap between spans.
        assert!(r.link_at(7).is_none());
    }
}
