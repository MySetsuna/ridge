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
    /// Last-seen `Grid::is_alt_screen()` value. On any flip
    /// (primary→alt, alt→primary) the snapshot tracks rows from the
    /// *other* screen and would produce stale dirty-row decisions —
    /// most visibly, exiting a TUI (e.g. `vim`, `htop`) appeared to
    /// blank the primary scrollback because per-row hashes happened to
    /// match between alt and primary content. We compare here and force
    /// `invalidate_all` on transitions so the next frame redraws
    /// against the currently-active screen from scratch.
    last_is_alt: bool,
    /// IME preedit overlay (CJK composition in progress). When `Some`,
    /// the renderer paints the preedit text on top of the cell grid at
    /// `row, col` as a final pass — the cells themselves are NOT
    /// modified, so a TUI redrawing into the same row mid-composition
    /// can't corrupt the preedit AND the preedit can't corrupt the TUI's
    /// rendered cells. Cleared via `clear_preedit` from JS on
    /// `compositionend`.
    preedit: Option<Preedit>,
    /// §1.34 (2026-05-22) — shell-history popup overlay. When `Some`,
    /// the renderer paints a panel of history rows on top of the cell
    /// grid as the final pass each frame, anchored at
    /// `(anchor_row, anchor_col)` and growing either upward
    /// (`place_above=true`) or downward (`place_above=false`).
    /// The Svelte/DOM `<TerminalHistoryPopup>` component was replaced
    /// by this overlay so the popup lives on the SAME canvas as the
    /// terminal cells — no separate DOM element, no z-index battles
    /// with split-container CSS, no font-metric drift between DOM
    /// renderer and wasm renderer. Mirror of `preedit` in lifecycle:
    /// JS installs via `setHistoryOverlay`, every frame paints, JS
    /// clears via `clearHistoryOverlay`.
    history_overlay: Option<HistoryOverlay>,
}

#[derive(Debug, Clone)]
pub struct Preedit {
    pub text: String,
    pub row: usize,
    pub col: usize,
}

/// §1.34 (2026-05-22) — descriptor for the shell-history popup overlay
/// rendered directly on the wasm canvas (replacing the prior Svelte
/// `<TerminalHistoryPopup>` DOM element). The JS layer owns the
/// filter / dedup logic and pushes a snapshot every time the user
/// changes selection or the filter narrows; the renderer just paints.
#[derive(Debug, Clone)]
pub struct HistoryOverlay {
    /// Filtered history entries, newest first. The renderer paints
    /// items[0..min(items.len(), max_visible_rows)] in order.
    /// Empty `items` is allowed (renderer no-ops) but the caller
    /// should prefer `clear_history_overlay` in that case.
    pub items: Vec<String>,
    /// Currently selected row index, or `-1` for "no selection".
    /// `-1` is rendered without the inverse-color highlight so the
    /// popup-open state is visually distinct from a row-picked state.
    pub selected_index: i32,
    /// Cell row of the input anchor on the active screen (viewport
    /// coords). The overlay is positioned to abut this row — above
    /// when `place_above=true`, below otherwise.
    pub anchor_row: usize,
    /// Cell column of the input anchor.
    pub anchor_col: usize,
    /// Place the popup ABOVE the anchor (overflowing upward) when
    /// `true`. Used when the prompt sits in the bottom half of the
    /// viewport so the popup doesn't get clipped by the bottom edge.
    pub place_above: bool,
    /// Maximum number of history rows to paint. Items beyond this
    /// cap are dropped at render time (the JS caller is expected to
    /// pre-cap to a sensible value like 10). Acts as a hard floor on
    /// popup height regardless of how much history the shell has.
    pub max_visible_rows: usize,
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
            last_is_alt: false,
            preedit: None,
            history_overlay: None,
        }
    }

    /// §1.34 (2026-05-22) — install the shell-history popup overlay.
    /// Replaces any prior overlay state in place; the next frame
    /// repaints with the new items / selected_index / anchor.
    /// `full_redraw_pending = true` so the overlay (and any cells it
    /// just covered) are painted on the very next tick instead of
    /// waiting for an unrelated dirty signal.
    pub fn set_history_overlay(&mut self, overlay: HistoryOverlay) {
        self.history_overlay = Some(overlay);
        self.full_redraw_pending = true;
    }

    /// §1.34 — remove the history overlay (Enter / ArrowRight / Esc).
    /// No-op when no overlay is installed. Forces a full redraw so
    /// the cells underneath the prior overlay region repaint from
    /// kernel state.
    pub fn clear_history_overlay(&mut self) {
        if self.history_overlay.is_some() {
            self.history_overlay = None;
            self.full_redraw_pending = true;
        }
    }

    /// Install an IME preedit overlay at the given cell. The renderer
    /// paints the text on top of the cell grid as a final pass each
    /// frame — non-destructive (cells unchanged). Replaces any prior
    /// preedit. Empty `text` is treated the same as `clear_preedit`.
    pub fn set_preedit(&mut self, text: String, row: usize, col: usize) {
        if text.is_empty() {
            self.preedit = None;
        } else {
            self.preedit = Some(Preedit { text, row, col });
        }
        // Force the next frame to repaint so the overlay (or its
        // removal) is visible immediately. Without this an idle
        // renderer might skip the next tick entirely.
        self.full_redraw_pending = true;
    }

    /// Remove the preedit overlay (called on `compositionend` after the
    /// committed string has been shipped to the PTY).
    pub fn clear_preedit(&mut self) {
        if self.preedit.is_some() {
            self.preedit = None;
            self.full_redraw_pending = true;
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

    /// Force a full redraw next frame. Call after font / theme / size
    /// changes — covers theme swap, font config change, DPR change,
    /// surface resize, and pane reattach.
    ///
    /// Resets every per-frame cache the renderer carries:
    ///   * `snapshot` — per-row hashes (next tick re-hashes everything).
    ///   * `last_cursor` — old cursor coords may now be off-grid after
    ///     a reflow / resize; clearing forces an unconditional draw of
    ///     the new cursor without trying to "erase" a stale row that
    ///     no longer exists.
    ///   * `last_offset` — the row→content mapping has changed; the
    ///     stored offset is meaningless against the new grid.
    ///   * `last_selection` — overlay rects refer to absolute rows
    ///     that may have shifted under reflow.
    ///   * `last_blink_phase` — pin to "visible" so the post-resize
    ///     frame actually shows the cursor instead of catching it on
    ///     the off-half by accident.
    ///
    /// Also forwards to the backend's `invalidate_atlas` so any GPU
    /// glyph cache (WebGPU `GlyphAtlas`) drops stale entries sized for
    /// the previous metrics. Canvas2D's default no-op is a free
    /// fall-through.
    pub fn invalidate_all(&mut self) {
        self.snapshot.clear();
        self.last_cursor = None;
        self.last_offset = 0;
        self.last_selection = None;
        self.last_blink_phase = true;
        self.full_redraw_pending = true;
        self.backend.invalidate_atlas();
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
        if let Some(ref prev) = self.last_cursor {
            if prev.row < self.snapshot.len() {
                self.snapshot[prev.row] = self.snapshot[prev.row].wrapping_add(1);
            }
        }
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Read-only access to the current theme — used by the JS layer when
    /// it wants to layer partial overrides on top of the existing theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

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
    pub fn tick(&mut self, terminal: &Terminal, selection: Option<SelRange>, now_ms: f64) -> bool {
        let rows_n = terminal.rows();

        // Screen-switch invalidation: when the active screen flips
        // (DECSET/DECRST ?1049 / ?47 / ?1047), the snapshot was built
        // against the *previous* screen's rows. Without clearing it,
        // exiting a fullscreen TUI like `vim` or `htop` could leave the
        // primary scrollback blank — alt-screen rows and the now-active
        // primary rows would hash-collide on common blank patterns and
        // the renderer would skip those rows entirely. Force a full
        // reset on every transition so the next frame redraws the
        // currently-active screen against an empty snapshot. The check
        // happens before sel/blink/resize so the post-invalidate state
        // captured below already reflects the post-switch screen.
        let cur_is_alt = terminal.is_alt_screen();
        if cur_is_alt != self.last_is_alt {
            self.last_is_alt = cur_is_alt;
            self.invalidate_all();
        }

        // Selection changed → force redraw so old translucent overlay
        // doesn't linger on rows that left the selection.
        let sel_changed = !selection_eq(selection, self.last_selection);
        if sel_changed {
            self.full_redraw_pending = true;
            self.last_selection = selection;
            // Notify backend so any preserved-content path (WebGPU
            // `LoadOp::Load`) seeds bg this frame instead of compositing
            // a new selection state over stale pixels.
            self.backend.on_full_invalidate();
        }

        // Cursor blink phase: 500ms on / 500ms off, derived from now_ms
        // so all panes blink in unison and we don't need a wakeup timer.
        // Phase change → mark previous cursor row dirty (so the cursor
        // gets erased on the off-half).
        let blink_active = terminal.modes().cursor_visible && terminal.modes().cursor_blink;
        let blink_phase = if blink_active {
            ((now_ms / 500.0) as i64).rem_euclid(2) == 1
        } else {
            true // non-blinking → always on
        };
        if blink_phase != self.last_blink_phase {
            if let Some(ref prev) = self.last_cursor {
                if prev.row < self.snapshot.len() {
                    self.snapshot[prev.row] = self.snapshot[prev.row].wrapping_add(1);
                }
            }
            self.last_blink_phase = blink_phase;
        }

        // Grow OR shrink the snapshot if the grid changed size. §A.3
        // (2026-05-07): previously this branch only fired on growth, so
        // a *narrowing* primary-screen resize left the dirty-row cache
        // sized to the old grid and Canvas2D never marked the trailing
        // rows for redraw — old pixels past the new bottom or right of
        // each row stayed visible (the §1.26 ghost-prompt symptom under
        // Canvas2D specifically). Forcing both ends to track `rows_n`
        // here pairs with `Grid::resize` clearing the cell state: the
        // next frame re-hashes everything against the cleared cells and
        // paints blanks over the stale pixels. WebGPU was already safe
        // because `requires_full_frame()` clears the swap-chain every
        // tick, but going through this path keeps both backends honest.
        if self.snapshot.len() != rows_n {
            self.snapshot.resize(rows_n, 0);
            self.full_redraw_pending = true;
            // Backing pixels for new / wrap-around rows are undefined
            // — backend must seed bg so `LoadOp::Load` doesn't expose
            // them.
            self.backend.on_full_invalidate();
        }

        // Backends that can't preserve content across frames (WebGPU
        // clears the swap-chain on every present) need every visible row
        // dirty every tick — otherwise non-dirty rows render only their
        // cleared bg and lose all glyphs. Canvas2D returns false here so
        // dirty-row diffing keeps its perf benefit.
        if self.backend.requires_full_frame() {
            self.full_redraw_pending = true;
        }

        // §1.27 (2026-05-07): Ink/log-update walks the cursor up through
        // its previous frame via repeated CUU+EL2, then writes the new
        // frame and emits CHA `\x1b[G` at the end (which trips the
        // §A.3 absolute-positioning timestamp). The per-row hash diff
        // can leave Canvas2D pixels stale when a row's *cells* match
        // across two ticks but the row was painted over by an opaque
        // overlay (the IME helper textarea) earlier in the session.
        // Force full-frame whenever the inline-TUI heuristic says we're
        // inside an Ink-style redraw window — bounded by the 2 s
        // INLINE_TUI_DECAY_MS so quiescent shells stay on the dirty-row
        // diff fast path. WebGPU already redraws everything, so this
        // branch is a no-op for the WebGPU path; Canvas2D gains
        // correctness for the Ink-active window only. Uses wall-clock
        // (`clock::now_ms()`, unix-epoch `i64`) to match the timestamp
        // domain `note_absolute_positioning` records — the renderer's
        // own `now_ms: f64` parameter is `performance.now()` (page-load
        // relative) and would always read as far in the past.
        let wall_ms = crate::term::clock::now_ms();
        if terminal
            .grid()
            .is_inline_tui_active_at(wall_ms, terminal.modes().cursor_visible)
        {
            self.full_redraw_pending = true;
        }

        // Viewport scroll offset change → full redraw. The row→content
        // mapping shifts when the user pages history, so per-row hashes
        // computed against last frame's mapping aren't valid.
        let offset = terminal.scroll_offset();
        if offset != self.last_offset {
            self.full_redraw_pending = true;
            self.last_offset = offset;
            // Row→content remap means every row's pixels now correspond
            // to a different scrollback position; backend must seed bg
            // so `LoadOp::Load` doesn't carry over the prior mapping.
            self.backend.on_full_invalidate();
        }

        // Compute dirty rows by hashing each visible row's cells +
        // hyperlink span shape. Cell hash is keyed off (ch, attr_id,
        // width); span shape adds (count, col_start, col_end) per
        // span. We read via `viewport_row` so the same code path
        // covers live grid AND scrollback views.
        //
        // Why include hyperlink spans: the hyperlink-underline pass
        // paints from `row.hyperlinks` every frame. A row whose span
        // set changes without the cell content changing would
        // otherwise stay "clean" → underline pixels persist or
        // vanish a frame late. All current cell-mutating Grid
        // methods (clear / erase_in_line / erase_chars / insert_chars
        // / delete_chars / Row::resize) already keep spans in sync,
        // but defending the dirty calc against future span-only
        // mutations is cheap (most rows have 0 spans). URI/id are NOT
        // hashed — the underline overlay only varies spatially, so
        // identical (col_start, col_end) → identical pixels. (TASKS §1.18.c.)
        let mut dirty_rows = Vec::with_capacity(rows_n);
        let mut dirty_flags = vec![false; rows_n];

        for r in 0..rows_n {
            let Some(row) = terminal.viewport_row(r) else {
                continue;
            };
            let h = compute_row_hash(row);
            if self.full_redraw_pending || r >= self.snapshot.len() || h != self.snapshot[r] {
                if r < self.snapshot.len() {
                    self.snapshot[r] = h;
                } else {
                    self.snapshot.push(h);
                }
                dirty_rows.push(r);
                dirty_flags[r] = true;
            }
        }

        // Expand dirty rows upwards to fix descender cutoff (Row N's background covers Row N-1's descenders).
        for r in (1..rows_n).rev() {
            if dirty_flags[r] && !dirty_flags[r - 1] {
                dirty_rows.push(r - 1);
                dirty_flags[r - 1] = true;
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
            if let Some(ref prev) = self.last_cursor {
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

        // Selection overlay anti-stacking: if a partial redraw is about to
        // happen (some rows dirty, but not all selection rows) AND the
        // selection is non-empty, force the selection-covered rows into
        // `dirty_rows` so their backgrounds get repainted opaquely
        // before `draw_selection_overlay` lays a fresh alpha on top.
        // Without this, every cursor-blink tick would paint another
        // 0x60-alpha overlay on selection rows that aren't otherwise
        // dirty — alpha accumulates frame-over-frame, the selection
        // tint darkens visibly within seconds.
        //
        // We skip this when full_redraw_pending is already set (every
        // row will be cleared + repainted anyway, so adding to dirty_rows
        // is redundant) and when no rows are otherwise dirty (return
        // false below — keeping the previous frame's pixels intact is
        // exactly what we want for an idle selected viewport).
        if !self.full_redraw_pending && !dirty_rows.is_empty() && selection.is_some() {
            let sel_rects = selection_to_rects(selection, terminal.cols(), terminal.rows());
            for &(row, _, _) in &sel_rects {
                if row < rows_n && !dirty_rows.contains(&row) {
                    dirty_rows.push(row);
                }
            }
        }

        if dirty_rows.is_empty() && !self.full_redraw_pending {
            return false;
        }

        // Build RowDraw views for the backend, reading via viewport_row.
        // Note: `RowDraw` borrows the row; we collect into a Vec held
        // for the duration of `draw_frame`, then drop. The backend never
        // sees the live grid or scrollback storage directly.
        let rows: Vec<RowDraw<'_>> = dirty_rows
            .iter()
            .filter_map(|&idx| {
                terminal.viewport_row(idx).map(|r| RowDraw {
                    row_index: idx,
                    cells: &r.cells,
                    clusters: &r.clusters,
                })
            })
            .collect();

        let do_full = self.first_frame || self.full_redraw_pending;
        let sel_rects = selection_to_rects(selection, terminal.cols(), terminal.rows());
        // Potentially set tui_mode on the metrics so backends can avoid
        // forcing the theme background onto cells whose background hasn't
        // been explicitly set by the foreground program.
        let tui_mode = terminal.grid().is_alt_screen() || terminal
            .grid()
            .is_inline_tui_active_at(crate::term::clock::now_ms(), terminal.modes().cursor_visible);
        let tui_metrics = FrameMetrics { tui_mode, ..self.metrics };
        // Collect hyperlink rects from every visible row. Most rows have
        // empty `hyperlinks` so this is cheap. We always re-emit on full
        // redraw; partial draws still emit them so the underlines aren't
        // erased by other row repaints.
        let mut hl_rects: Vec<(usize, usize, usize)> = Vec::new();
        for r in 0..rows_n {
            let Some(row) = terminal.viewport_row(r) else {
                continue;
            };
            for span in &row.hyperlinks {
                hl_rects.push((r, span.col_start, span.col_end));
            }
        }
        draw_frame(
            &mut self.backend,
            tui_metrics,
            &self.theme,
            &rows,
            self.last_cursor.as_ref(),
            &terminal.grid().attrs,
            do_full,
            &sel_rects,
            &hl_rects,
            self.preedit.as_ref(),
            self.history_overlay.as_ref(),
        );
        self.first_frame = false;
        self.full_redraw_pending = false;
        true
    }

    /// Non-mutating mirror of the early-exit conditions in `tick`.
    /// Returns true when the next `tick` call would do any drawing
    /// work — false when the renderer has nothing to redraw and the
    /// caller can safely sleep its RAF loop. Used by `manager.ts` to
    /// pause the per-pane animation frame loop on idle.
    ///
    /// Cost: ~24 row hashes for an 80×24 grid (≈4 µs). The hashes are
    /// re-computed in `tick`; calling both back-to-back doubles that
    /// cost — still cheaper than one `draw_row` call by two orders of
    /// magnitude, and avoids tearing the snapshot.
    pub fn is_dirty(&self, terminal: &Terminal, selection: Option<SelRange>, now_ms: f64) -> bool {
        // Pending unconditional redraw — first frame or set by an
        // earlier mutation we haven't tick-consumed yet.
        if self.first_frame || self.full_redraw_pending {
            return true;
        }

        // Selection toggled / range changed.
        if !selection_eq(selection, self.last_selection) {
            return true;
        }

        // Viewport scrolled.
        if terminal.scroll_offset() != self.last_offset {
            return true;
        }

        // Cursor blink phase boundary crossed since last draw — but
        // only when the cursor is visible at all (DECTCEM on +
        // focused + viewport at live grid). Off-half phases when the
        // cursor was previously visible also count, since the prior
        // frame painted it and this frame must erase it.
        let blink_active = terminal.modes().cursor_visible && terminal.modes().cursor_blink;
        let blink_phase = if blink_active {
            ((now_ms / 500.0) as i64).rem_euclid(2) == 1
        } else {
            true
        };
        if blink_phase != self.last_blink_phase {
            return true;
        }

        // Snapshot length mismatch → grid grew.
        let rows_n = terminal.rows();
        if self.snapshot.len() < rows_n {
            return true;
        }

        // Per-row content + hyperlink-span hash diff.
        for r in 0..rows_n {
            let Some(row) = terminal.viewport_row(r) else {
                continue;
            };
            if compute_row_hash(row) != self.snapshot[r] {
                return true;
            }
        }

        // Cursor moved (position / style / glyph beneath).
        let offset = terminal.scroll_offset();
        let new_cursor = if self.focused && offset == 0 && blink_phase {
            self.compute_cursor_draw(terminal)
        } else {
            None
        };
        !cursor_eq(&self.last_cursor, &new_cursor)
    }

    /// Milliseconds until the next cursor-blink phase boundary, given
    /// the current wall-clock `now_ms`. Returns `f64::INFINITY` when
    /// the cursor isn't blinking (DECTCEM off or `cursor_blink` mode
    /// off) so the caller can skip scheduling a wakeup.
    ///
    /// Phase boundary is every 500 ms aligned to the same time origin
    /// `tick` uses. Caller is responsible for the lower bound (e.g.
    /// `Math.max(deadline, 1)` to avoid 0-ms timers).
    pub fn next_blink_deadline_ms(&self, terminal: &Terminal, now_ms: f64) -> f64 {
        // `self.focused` gates cursor rendering at compute_cursor_draw
        // (line 355): when the pane isn't focused, `new_cursor` is
        // always None, `last_cursor` quickly settles to None, and no
        // further blink-driven dirty events fire. Returning a finite
        // deadline here would still wake the RAF loop every 500 ms to
        // run a no-op tick — burning the whole point of letting the
        // loop sleep through unfocused idle. Cap to Infinity so the
        // loop falls through to its 1 s watchdog (caller clamps).
        if !self.focused {
            return f64::INFINITY;
        }
        let blink_active = terminal.modes().cursor_visible && terminal.modes().cursor_blink;
        if !blink_active {
            return f64::INFINITY;
        }
        let half = 500.0;
        // ms past the most recent phase boundary
        let past = now_ms.rem_euclid(half);
        // ms remaining until the next one
        half - past
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
        let cluster_text = row.cluster_at(cur.col).map(|c| c.text.as_ref().to_string());
        // §B.9 — cumulative extra cells from wide-cluster glyph expansion
        // before the cursor column. Used by backends to compute the visual
        // cursor position when preceding emoji have expanded beyond their
        // grid span. Heuristic: only multi-codepoint clusters (emoji ZWJ,
        // flags) expand; plain CJK (width=2, no cluster) does not.
        let extra_cells = {
            let mut extra = 0.0f64;
            let mut i = 0;
            while i < cur.col {
                let c = row.cells.get(i).copied().unwrap_or_default();
                if c.width >= 2 {
                    if row.cluster_at(i).is_some() {
                        extra += 1.0;
                    }
                    i += c.width as usize;
                } else if c.width == 1 {
                    i += 1;
                } else {
                    i += 1;
                }
            }
            extra
        };
        Some(CursorDraw {
            row: cur.row,
            col: cur.col,
            // Honors DECSCUSR `CSI <n> SP q`. `Modes::cursor_shape` is the
            // single source of truth — set by the parser when an app emits
            // DECSCUSR. Blink (`Modes::cursor_shape`) is a future render-side
            // concern; today we render solid in all shapes regardless.
            style: match terminal.modes().cursor_shape {
                crate::term::modes::CursorShape::Block => CursorStyle::Block,
                crate::term::modes::CursorShape::Underline => CursorStyle::Underline,
                crate::term::modes::CursorShape::Bar => CursorStyle::Bar,
            },
            ch: cell.ch,
            ch_attr: cell.attr,
            width: cell.width.max(1),
            cluster_text,
            extra_cells,
        })
    }
}

/// §4b per-pane increment cache (2026-05-08): a thin `AnyBackend`-only
/// passthrough for the WebGPU cached-record path. Lives in its own
/// impl block (not the generic `impl<B: RenderBackend>`) because the
/// underlying method exists on `AnyBackend::record_cached_only` rather
/// than the `RenderBackend` trait — Canvas2D returns `false` and the
/// caller falls back to a normal `tick`/`render` cycle.
#[cfg(target_arch = "wasm32")]
impl Renderer<crate::render::AnyBackend> {
    pub fn record_cached_only(&mut self) -> bool {
        self.backend.record_cached_only()
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
    let Some(range) = range else {
        return Vec::new();
    };
    let r = range.normalized();
    if r.start.row >= rows {
        return Vec::new();
    }
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

/// Compute the per-row dirty hash. Extracted from `Renderer::tick` so
/// the §1.18.c invariant — that hyperlink span shape changes dirty the
/// row, while URI/id-only changes do not — has direct host-side test
/// coverage. The cells contribute `(ch, attr_id, width)`; the
/// hyperlinks contribute `(count, col_start, col_end)` per span.
fn compute_row_hash(row: &crate::term::cell::Row) -> u64 {
    let mut hasher = DefaultHasher::new();
    for cell in &row.cells {
        cell.ch.hash(&mut hasher);
        cell.attr.0.hash(&mut hasher);
        cell.width.hash(&mut hasher);
    }
    row.hyperlinks.len().hash(&mut hasher);
    for span in &row.hyperlinks {
        span.col_start.hash(&mut hasher);
        span.col_end.hash(&mut hasher);
    }
    // §4.7 (2026-05-07): include grapheme cluster sidecar in the row
    // hash so a cluster-only change (e.g. a ZWJ cluster overwritten
    // with a different ZWJ cluster at the same col) re-renders the
    // row even when `cell.ch` (= first codepoint) happens to match.
    row.clusters.len().hash(&mut hasher);
    for span in &row.clusters {
        span.col.hash(&mut hasher);
        span.text.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::compute_row_hash;
    use crate::term::cell::{Cell, HyperlinkSpan, Row};

    fn row_with_text(text: &str, cols: usize) -> Row {
        let mut r = Row::new(cols);
        for (i, ch) in text.chars().enumerate() {
            if i >= cols {
                break;
            }
            r.cells[i] = Cell::new(ch, crate::term::attr_table::AttrId::DEFAULT, 1);
        }
        r
    }

    #[test]
    fn identical_rows_hash_equal() {
        let a = row_with_text("hello", 10);
        let b = row_with_text("hello", 10);
        assert_eq!(compute_row_hash(&a), compute_row_hash(&b));
    }

    #[test]
    fn cell_change_dirties_hash() {
        let a = row_with_text("hello", 10);
        let b = row_with_text("hellz", 10);
        assert_ne!(compute_row_hash(&a), compute_row_hash(&b));
    }

    #[test]
    fn span_added_dirties_hash() {
        // §1.18.c regression test: adding a hyperlink span to an
        // otherwise-identical row must change the dirty hash so the
        // renderer redraws the row and the underline pass paints
        // (or — on removal — bg+glyph repaint clears the previous
        // underline pixels).
        let a = row_with_text("hello", 10);
        let mut b = row_with_text("hello", 10);
        b.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 5,
            uri: "https://example.com".into(),
            id: None,
        });
        assert_ne!(compute_row_hash(&a), compute_row_hash(&b));
    }

    #[test]
    fn span_position_change_dirties_hash() {
        let mut a = row_with_text("hello", 10);
        a.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 5,
            uri: "https://example.com".into(),
            id: None,
        });
        let mut b = row_with_text("hello", 10);
        b.hyperlinks.push(HyperlinkSpan {
            col_start: 1,
            col_end: 5,
            uri: "https://example.com".into(),
            id: None,
        });
        assert_ne!(compute_row_hash(&a), compute_row_hash(&b));
    }

    #[test]
    fn span_uri_only_change_does_not_dirty_hash() {
        // URI/id are intentionally NOT in the hash. The underline
        // overlay is purely spatial — same (col_start, col_end) →
        // same pixels. Avoids redraws on URI-only rebuilds (e.g.,
        // some shells re-emit OSC 8 with a slightly different
        // tracking id every frame).
        let mut a = row_with_text("hello", 10);
        a.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 5,
            uri: "https://example.com".into(),
            id: None,
        });
        let mut b = row_with_text("hello", 10);
        b.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 5,
            uri: "https://different.example.com".into(),
            id: Some("anchor-42".into()),
        });
        assert_eq!(compute_row_hash(&a), compute_row_hash(&b));
    }

    #[test]
    fn span_count_difference_dirties_hash() {
        let mut a = row_with_text("ab cd", 10);
        a.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 2,
            uri: "u".into(),
            id: None,
        });
        let mut b = row_with_text("ab cd", 10);
        b.hyperlinks.push(HyperlinkSpan {
            col_start: 0,
            col_end: 2,
            uri: "u".into(),
            id: None,
        });
        b.hyperlinks.push(HyperlinkSpan {
            col_start: 3,
            col_end: 5,
            uri: "u2".into(),
            id: None,
        });
        assert_ne!(compute_row_hash(&a), compute_row_hash(&b));
    }

    // ─── selection_to_rects ───────────────────────────────────────────
    use super::selection_to_rects;
    use crate::selection::{Pos, Range};

    fn range(sr: usize, sc: usize, er: usize, ec: usize) -> Range {
        Range {
            start: Pos { row: sr, col: sc },
            end: Pos { row: er, col: ec },
        }
    }

    #[test]
    fn selection_none_returns_empty() {
        let rects = selection_to_rects(None, 80, 24);
        assert!(rects.is_empty());
    }

    #[test]
    fn selection_single_row_one_rect_clipped_to_range() {
        // (5, 3) → (5, 10) in an 80×24 viewport.
        let rects = selection_to_rects(Some(range(5, 3, 5, 10)), 80, 24);
        assert_eq!(rects, vec![(5, 3, 10)]);
    }

    #[test]
    fn selection_multi_row_first_and_last_clipped_middle_full_width() {
        // (2, 5) → (4, 7) over 80 cols × 24 rows. Row 2 starts at col 5,
        // row 3 spans full width, row 4 ends at col 7.
        let rects = selection_to_rects(Some(range(2, 5, 4, 7)), 80, 24);
        assert_eq!(rects, vec![(2, 5, 80), (3, 0, 80), (4, 0, 7),]);
    }

    #[test]
    fn selection_normalizes_reversed_range() {
        // Range with start > end (user dragged backwards) — must
        // normalize before slicing.
        let rects = selection_to_rects(Some(range(5, 10, 5, 3)), 80, 24);
        assert_eq!(rects, vec![(5, 3, 10)]);
    }

    #[test]
    fn selection_clamps_end_row_past_viewport() {
        // End row 50 in a 24-row viewport → clamp to row 23.
        let rects = selection_to_rects(Some(range(20, 0, 50, 5)), 80, 24);
        // Rows 20, 21, 22, 23 — last clamped.
        assert_eq!(rects.len(), 4);
        assert_eq!(rects[0], (20, 0, 80));
        assert_eq!(rects[3].0, 23);
    }

    #[test]
    fn selection_returns_empty_when_start_row_past_viewport() {
        let rects = selection_to_rects(Some(range(50, 0, 60, 0)), 80, 24);
        assert!(rects.is_empty());
    }

    #[test]
    fn selection_skips_empty_ranges_within_row() {
        // Single row with start col == end col → empty rect, skipped.
        let rects = selection_to_rects(Some(range(3, 5, 3, 5)), 80, 24);
        assert!(rects.is_empty());
    }

    // ─── cursor_eq ────────────────────────────────────────────────────
    use super::cursor_eq;
    use crate::render::backend::{CursorDraw, CursorStyle};

    fn cursor(row: usize, col: usize, style: CursorStyle) -> CursorDraw {
        CursorDraw {
            row,
            col,
            style,
            // The ch / ch_attr / width fields are intentionally NOT
            // compared by cursor_eq — they're carried inline so the
            // backend can paint the glyph on top of the cursor block,
            // but a cell content change is already caught by the
            // per-row dirty hash. Filling them with arbitrary values
            // here proves cursor_eq ignores them.
            ch: ' ',
            ch_attr: crate::term::attr_table::AttrId::DEFAULT,
            width: 1,
            cluster_text: None,
            extra_cells: 0.0,
        }
    }

    #[test]
    fn cursor_eq_both_none() {
        assert!(cursor_eq(&None, &None));
    }

    #[test]
    fn cursor_eq_none_vs_some_false() {
        assert!(!cursor_eq(&None, &Some(cursor(0, 0, CursorStyle::Block))));
        assert!(!cursor_eq(&Some(cursor(0, 0, CursorStyle::Block)), &None));
    }

    #[test]
    fn cursor_eq_same_position_and_style_true() {
        let a = cursor(5, 12, CursorStyle::Block);
        let b = cursor(5, 12, CursorStyle::Block);
        assert!(cursor_eq(&Some(a), &Some(b)));
    }

    #[test]
    fn cursor_eq_ignores_ch_difference() {
        // ch and ch_attr differ but row/col/style match — equal.
        // Production-correct: cell content changes already dirty the
        // row via the hash, so the cursor doesn't need to also re-mark.
        let mut a = cursor(3, 7, CursorStyle::Block);
        let b = cursor(3, 7, CursorStyle::Block);
        a.ch = 'A';
        assert!(cursor_eq(&Some(a), &Some(b)));
    }

    #[test]
    fn cursor_eq_different_row_false() {
        let a = cursor(2, 5, CursorStyle::Block);
        let b = cursor(3, 5, CursorStyle::Block);
        assert!(!cursor_eq(&Some(a), &Some(b)));
    }

    #[test]
    fn cursor_eq_different_col_false() {
        let a = cursor(2, 5, CursorStyle::Block);
        let b = cursor(2, 6, CursorStyle::Block);
        assert!(!cursor_eq(&Some(a), &Some(b)));
    }

    #[test]
    fn cursor_eq_different_style_false() {
        let a = cursor(2, 5, CursorStyle::Block);
        let b = cursor(2, 5, CursorStyle::Bar);
        assert!(!cursor_eq(&Some(a), &Some(b)));
    }

    // ─── selection_eq ─────────────────────────────────────────────────
    use super::selection_eq;

    #[test]
    fn selection_eq_both_none() {
        assert!(selection_eq(None, None));
    }

    #[test]
    fn selection_eq_none_vs_some_false() {
        let r = range(1, 2, 3, 4);
        assert!(!selection_eq(None, Some(r)));
        assert!(!selection_eq(Some(r), None));
    }

    #[test]
    fn selection_eq_identical_true() {
        let a = range(1, 2, 3, 4);
        let b = range(1, 2, 3, 4);
        assert!(selection_eq(Some(a), Some(b)));
    }

    #[test]
    fn selection_eq_reversed_ranges_normalize_to_equal() {
        // Drag-forward and drag-backward over the same span produce
        // ranges with swapped start/end. Renderer must treat them as
        // equal so it doesn't redraw on a no-op direction flip.
        let a = range(1, 2, 3, 4);
        let b = range(3, 4, 1, 2);
        assert!(selection_eq(Some(a), Some(b)));
    }

    #[test]
    fn selection_eq_different_start_false() {
        let a = range(1, 2, 3, 4);
        let b = range(1, 3, 3, 4);
        assert!(!selection_eq(Some(a), Some(b)));
    }

    #[test]
    fn selection_eq_different_end_false() {
        let a = range(1, 2, 3, 4);
        let b = range(1, 2, 3, 5);
        assert!(!selection_eq(Some(a), Some(b)));
    }
}
