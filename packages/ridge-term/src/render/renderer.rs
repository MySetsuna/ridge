//! Renderer state: dirty row tracking + frame composition.
//!
//! The renderer owns:
//!   - Last-drawn snapshot of the grid (per-row hash or shallow copy)
//!   - Per-row dirty bits computed by diffing current grid vs snapshot
//!   - The backend instance
//!
//! Each `tick()` call:
//!   1. Diff current grid vs snapshot → dirty rows
//!   2. If anything changed, ask backend to draw
//!   3. Update snapshot
//!
//! ## Why per-row diff and not per-cell
//!
//! A 80×24 grid has 1,920 cells. Per-cell dirty bits = 240 bytes/grid
//! plus ~2k branch decisions per frame. Per-row hash = 24 u64 = 192
//! bytes plus 24 hash compares. The redraw cost difference between
//! "redraw 1 cell" and "redraw 1 row" on Canvas2D is < 0.1ms — not
//! worth tracking finer.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::render::backend::{
    draw_frame, CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme,
};
use crate::selection::Range as SelRange;
use crate::term::Terminal;

pub struct Renderer<B: RenderBackend> {
    backend: B,
    /// Per-row hash of last-drawn state. Length grows on demand to match
    /// the active grid size; rows beyond `len()` are treated as dirty.
    snapshot: Vec<u64>,
    /// Last-drawn cursor descriptor. When the cursor moves (or its row
    /// changes), the row it WAS on must redraw to erase the old cursor.
    last_cursor: Option<CursorDraw>,
    /// Last-seen viewport scroll_offset. When the user scrolls into
    /// history, every row's content mapping changes — we force a full
    /// redraw on offset change rather than trying to be clever.
    last_offset: usize,
    /// Last-seen selection range. When this changes we force a full
    /// redraw because translucent overlays don't erase themselves — old
    /// tints would persist on rows that left the selection.
    last_selection: Option<SelRange>,
    /// Last computed cursor-blink phase. `false` = off-half (cursor hidden),
    /// `true` = on-half (cursor visible). Toggling this on phase change
    /// dirties the cursor row so it redraws.
    last_blink_phase: bool,
    /// Whether this renderer's surface is currently focused. Multi-pane
    /// hosts drive this via `set_focused(bool)` so unfocused panes hide
    /// their cursor entirely — only the truly active terminal blinks.
    /// Default `true` preserves single-pane behavior at construction.
    focused: bool,
    metrics: FrameMetrics,
    theme: Theme,
    /// `true` until the first successful frame; forces a clear+redraw all.
    first_frame: bool,
    /// Whether a full redraw is needed next frame (theme change, font
    /// change, resize). Cleared after the next tick.
    full_redraw_pending: bool,
}

impl<B: RenderBackend> Renderer<B> {
    pub fn new(backend: B, metrics: FrameMetrics, theme: Theme) -> Self {
        Self {
            backend,
            snapshot: Vec::new(),
            last_cursor: None,
            last_offset: 0,
            last_selection: None,
            last_blink_phase: true,
            focused: true,
            metrics,
            theme,
            first_frame: true,
            full_redraw_pending: true,
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.invalidate_all();
    }

    pub fn set_metrics(&mut self, metrics: FrameMetrics) {
        // Cell size change → all rows must redraw at new positions.
        if (metrics.cell_w - self.metrics.cell_w).abs() > 0.1
            || (metrics.cell_h - self.metrics.cell_h).abs() > 0.1
        {
            self.invalidate_all();
        }
        self.metrics = metrics;
    }

    /// DPR-only update. Used after `resize_surface` when cell dimensions
    /// haven't changed (drag-resize the canvas, monitor DPR change). Cell
    /// dimensions stay the same; only the transform scale needs to follow.
    pub fn set_dpr(&mut self, dpr: f32) {
        if (self.metrics.dpr - dpr).abs() > 0.001 {
            self.metrics.dpr = dpr;
            self.invalidate_all();
        }
    }

    /// Force a full redraw next frame. Call after font / theme / size changes.
    pub fn invalidate_all(&mut self) {
        self.snapshot.clear();
        self.full_redraw_pending = true;
    }

    /// Multi-pane hosts call this when the active pane changes. When the
    /// focus flag flips, we dirty the row that last held the cursor so the
    /// next frame redraws it without the cursor (focus lost) or with it
    /// back (focus gained). Idempotent — no-op when the value is unchanged.
    pub fn set_focused(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if let Some(prev) = self.last_cursor {
            if prev.row < self.snapshot.len() {
                self.snapshot[prev.row] = self.snapshot[prev.row].wrapping_add(1);
            }
        }
    }

    pub fn backend_mut(&mut self) -> &mut B { &mut self.backend }

    /// Read-only access to the current theme — used by the JS layer when
    /// it wants to layer partial overrides on top of the existing theme.
    pub fn theme(&self) -> &Theme { &self.theme }

    /// Drive one frame. Returns `true` if anything was drawn (caller may
    /// use this to decide whether to skip swapchain present in WebGPU
    /// cases — Canvas2D ignores the return).
    ///
    /// `selection` is the kernel's current selection range (drawn as a
    /// translucent overlay over selected cells). Pass `None` for no
    /// selection. The renderer detects changes vs `last_selection` and
    /// forces a full redraw so the previous overlay tint gets erased.
    ///
    /// `now_ms` is the wall-clock time (e.g. `performance.now()` from JS,
    /// or any monotonic millisecond source). Used for cursor-blink phase
    /// computation. Pass 0.0 if you want a stable non-blinking cursor —
    /// blink is also gated on `Modes::cursor_blink`.
    pub fn tick(
        &mut self,
        terminal: &Terminal,
        selection: Option<SelRange>,
        now_ms: f64,
    ) -> bool {
        let rows_n = terminal.rows();

        // Selection changed → force redraw so old translucent overlay
        // doesn't linger on rows that left the selection.
        let sel_changed = !selection_eq(selection, self.last_selection);
        if sel_changed {
            self.full_redraw_pending = true;
            self.last_selection = selection;
        }

        // Cursor blink phase: 500ms on / 500ms off, derived from now_ms
        // so all panes blink in unison and we don't need a wakeup timer.
        // Phase change → mark previous cursor row dirty (so the cursor
        // gets erased on the off-half).
        let blink_active = terminal.modes().cursor_visible
            && terminal.modes().cursor_blink;
        let blink_phase = if blink_active {
            ((now_ms / 500.0) as i64).rem_euclid(2) == 1
        } else {
            true // non-blinking → always on
        };
        if blink_phase != self.last_blink_phase {
            if let Some(prev) = self.last_cursor {
                if prev.row < self.snapshot.len() {
                    self.snapshot[prev.row] = self.snapshot[prev.row].wrapping_add(1);
                }
            }
            self.last_blink_phase = blink_phase;
        }

        // Grow snapshot if the grid grew.
        if self.snapshot.len() < rows_n {
            self.snapshot.resize(rows_n, 0);
            self.full_redraw_pending = true;
        }

        // Viewport scroll offset change → full redraw. The row→content
        // mapping shifts when the user pages history, so per-row hashes
        // computed against last frame's mapping aren't valid.
        let offset = terminal.scroll_offset();
        if offset != self.last_offset {
            self.full_redraw_pending = true;
            self.last_offset = offset;
        }

        // Compute dirty rows by hashing each visible row's cells. Hash
        // is keyed off (ch, attr_id, width). We read via `viewport_row`
        // so the same code path covers live grid AND scrollback views.
        let mut dirty_rows: Vec<usize> = Vec::with_capacity(rows_n);
        for r in 0..rows_n {
            let Some(row) = terminal.viewport_row(r) else { continue };
            let mut hasher = DefaultHasher::new();
            for cell in &row.cells {
                cell.ch.hash(&mut hasher);
                cell.attr.0.hash(&mut hasher);
                cell.width.hash(&mut hasher);
            }
            let h = hasher.finish();
            if self.full_redraw_pending || h != self.snapshot[r] {
                self.snapshot[r] = h;
                dirty_rows.push(r);
            }
        }

        // Cursor handling: only show the cursor when (a) the surface is
        // focused, (b) the viewport is at the live grid (offset == 0),
        // and (c) we're on the visible half of the blink phase. Scrolled-
        // into-history view and unfocused panes both = no cursor (matches
        // xterm behavior + multi-pane convention).
        let new_cursor = if self.focused && offset == 0 && blink_phase {
            self.compute_cursor_draw(terminal)
        } else {
            None
        };

        if !cursor_eq(&self.last_cursor, &new_cursor) {
            if let Some(prev) = self.last_cursor {
                if !dirty_rows.contains(&prev.row) {
                    dirty_rows.push(prev.row);
                }
            }
            if let Some(ref cur) = new_cursor {
                if !dirty_rows.contains(&cur.row) {
                    dirty_rows.push(cur.row);
                }
            }
        }
        self.last_cursor = new_cursor;

        if dirty_rows.is_empty() && !self.full_redraw_pending {
            return false;
        }

        // Build RowDraw views for the backend, reading via viewport_row.
        // Note: `RowDraw` borrows the row; we collect into a Vec held
        // for the duration of `draw_frame`, then drop. The backend never
        // sees the live grid or scrollback storage directly.
        let rows: Vec<RowDraw<'_>> = dirty_rows
            .iter()
            .filter_map(|&idx| terminal.viewport_row(idx).map(|r| RowDraw {
                row_index: idx,
                cells: &r.cells,
            }))
            .collect();

        let do_full = self.first_frame || self.full_redraw_pending;
        let sel_rects = selection_to_rects(selection, terminal.cols(), terminal.rows());
        // Collect hyperlink rects from every visible row. Most rows have
        // empty `hyperlinks` so this is cheap. We always re-emit on full
        // redraw; partial draws still emit them so the underlines aren't
        // erased by other row repaints.
        let mut hl_rects: Vec<(usize, usize, usize)> = Vec::new();
        for r in 0..rows_n {
            let Some(row) = terminal.viewport_row(r) else { continue };
            for span in &row.hyperlinks {
                hl_rects.push((r, span.col_start, span.col_end));
            }
        }
        draw_frame(
            &mut self.backend,
            self.metrics,
            &self.theme,
            &rows,
            self.last_cursor.as_ref(),
            &terminal.grid().attrs,
            do_full,
            &sel_rects,
            &hl_rects,
        );
        self.first_frame = false;
        self.full_redraw_pending = false;
        true
    }

    /// Compute the cursor descriptor for this frame. Returns None when
    /// DECTCEM is off (cursor hidden) or terminal is on alt screen with
    /// inactive cursor mode (future).
    fn compute_cursor_draw(&self, terminal: &Terminal) -> Option<CursorDraw> {
        if !terminal.modes().cursor_visible {
            return None;
        }
        let grid = terminal.grid();
        let cur = grid.cursor();
        let row = grid.row(cur.row)?;
        let cell = row.cells.get(cur.col).copied().unwrap_or_default();
        Some(CursorDraw {
            row: cur.row,
            col: cur.col,
            // Honors DECSCUSR `CSI <n> SP q`. `Modes::cursor_shape` is the
            // single source of truth — set by the parser when an app emits
            // DECSCUSR. Blink (`Modes::cursor_blink`) is a future render-side
            // concern; today we render solid in all shapes regardless.
            style: match terminal.modes().cursor_shape {
                crate::term::modes::CursorShape::Block => CursorStyle::Block,
                crate::term::modes::CursorShape::Underline => CursorStyle::Underline,
                crate::term::modes::CursorShape::Bar => CursorStyle::Bar,
            },
            ch: cell.ch,
            ch_attr: cell.attr,
            width: cell.width.max(1),
        })
    }
}

fn cursor_eq(a: &Option<CursorDraw>, b: &Option<CursorDraw>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.row == y.row && x.col == y.col && x.style == y.style,
        _ => false,
    }
}

/// Compare two optional selection ranges by their normalized endpoints.
/// `Range` doesn't impl PartialEq directly because it isn't normalized,
/// so we explicitly normalize both sides before comparing.
fn selection_eq(a: Option<SelRange>, b: Option<SelRange>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            let nx = x.normalized();
            let ny = y.normalized();
            nx.start == ny.start && nx.end == ny.end
        }
        _ => false,
    }
}

/// Decompose a selection range into per-row `(row, col_start, col_end)`
/// tuples. Single-row selections produce one rect; multi-row selections
/// produce one rect per visible row, with the first/last clipped to the
/// selection's start/end column and the middle rows spanning full width.
fn selection_to_rects(
    range: Option<SelRange>,
    cols: usize,
    rows: usize,
) -> Vec<(usize, usize, usize)> {
    let Some(range) = range else { return Vec::new() };
    let r = range.normalized();
    if r.start.row >= rows { return Vec::new() }
    let mut out = Vec::with_capacity(r.end.row.saturating_sub(r.start.row) + 1);
    let last_row = r.end.row.min(rows.saturating_sub(1));
    for row in r.start.row..=last_row {
        let lo = if row == r.start.row { r.start.col } else { 0 };
        let hi = if row == r.end.row { r.end.col } else { cols };
        let hi = hi.min(cols);
        if hi > lo {
            out.push((row, lo, hi));
        }
    }
    out
}
