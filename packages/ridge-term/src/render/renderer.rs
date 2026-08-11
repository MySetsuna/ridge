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
    draw_frame, CursorDraw, CursorStyle, FrameDraw, FrameMetrics, RenderBackend, RowDraw, Theme,
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
    /// pre-cap to a sensible value). Acts as a hard floor on popup
    /// height regardless of how much history the shell has.
    pub max_visible_rows: usize,
    /// §history-scroll — total number of entries in the FULL filtered
    /// list (NOT just the visible window). When `total_items >
    /// visible_count` the renderer paints a scrollbar so the user can
    /// see there's more and where they are. JS pre-windows `items` to
    /// the visible slice and reports the full count here.
    pub total_items: usize,
    /// §history-scroll — index of `items[0]` within the full filtered
    /// list. Drives the scrollbar thumb's vertical position.
    pub first_visible: usize,
    /// Visible terminal grid dimensions. Geometry is clipped to this
    /// pane-local cell rect, never the workspace host canvas.
    pub viewport_cols: usize,
    pub viewport_rows: usize,
}

#[cfg(any(target_arch = "wasm32", test))]
pub(crate) const HISTORY_OVERLAY_COL_CAP: usize = 80;

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryOverlayGeometry {
    pub panel_x: f32,
    pub panel_y: f32,
    pub panel_w: f32,
    pub panel_h: f32,
    pub pad_w: f32,
    pub pad_h: f32,
    pub content_cols: usize,
    pub visible_count: usize,
    pub scrollbar_w: f32,
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn history_text(text: &str) -> String {
    text.replace(['\r', '\n'], " ↵ ")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn history_text_width(text: &str) -> usize {
    text.chars()
        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
        .sum::<usize>()
        .min(HISTORY_OVERLAY_COL_CAP)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn truncate_history_text(text: &str, max_cells: usize) -> String {
    let mut cells = 0usize;
    text.chars()
        .take_while(|ch| {
            let next = cells + if ch.is_ascii() { 1 } else { 2 };
            if next > max_cells {
                false
            } else {
                cells = next;
                true
            }
        })
        .collect()
}

/// Resolve the popup inside the pane-local cell rectangle. Prefer the
/// requested anchor side, flip when the other side fits, then clamp.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn history_overlay_geometry(
    overlay: &HistoryOverlay,
    widest_cells: usize,
    requested_visible: usize,
    cell_w: f32,
    cell_h: f32,
) -> Option<HistoryOverlayGeometry> {
    let (viewport_w, viewport_h) = overlay_viewport(overlay, requested_visible, cell_w, cell_h)?;

    let (pad_w, pad_h) = overlay_padding(viewport_w, viewport_h, cell_w, cell_h);

    let visible_count = requested_visible
        .min(overlay.max_visible_rows)
        .min(((viewport_h - 2.0 * pad_h) / cell_h).floor().max(1.0) as usize);
    let (needs_scrollbar, scrollbar_w, scrollbar_gap) = overlay_scrollbar(
        viewport_w,
        cell_w,
        pad_w,
        visible_count,
        overlay.total_items,
    );

    let desired_cols = widest_cells.clamp(8, HISTORY_OVERLAY_COL_CAP);
    let desired_w = desired_cols as f32 * cell_w + 2.0 * pad_w + scrollbar_w + scrollbar_gap;
    let panel_w = desired_w.min(viewport_w);
    let content_cols = (((panel_w - 2.0 * pad_w - scrollbar_w - scrollbar_gap) / cell_w)
        .floor()
        .max(1.0) as usize)
        .min(desired_cols);

    let anchor_row = overlay
        .anchor_row
        .min(overlay.viewport_rows.saturating_sub(1));
    let above_h = anchor_row as f32 * cell_h;
    let below_h = viewport_h - (anchor_row as f32 + 1.0) * cell_h;
    let rows_that_fit = |height: f32| ((height - 2.0 * pad_h) / cell_h).floor().max(0.0) as usize;
    let above_rows = rows_that_fit(above_h);
    let below_rows = rows_that_fit(below_h);
    let (place_above, visible_count) =
        overlay_placement(overlay.place_above, visible_count, above_rows, below_rows);

    let panel_h = (visible_count as f32 * cell_h + 2.0 * pad_h).min(viewport_h);
    let panel_x = if desired_w > viewport_w {
        ((viewport_w - panel_w) / 2.0).round()
    } else {
        (overlay
            .anchor_col
            .min(overlay.viewport_cols.saturating_sub(1)) as f32
            * cell_w)
            .clamp(0.0, viewport_w - panel_w)
    };
    let wanted_y = if place_above {
        anchor_row as f32 * cell_h - panel_h
    } else {
        (anchor_row as f32 + 1.0) * cell_h
    };
    let panel_y = wanted_y.clamp(0.0, viewport_h - panel_h);

    Some(HistoryOverlayGeometry {
        panel_x,
        panel_y,
        panel_w,
        panel_h,
        pad_w,
        pad_h,
        content_cols,
        visible_count,
        scrollbar_w: if needs_scrollbar { scrollbar_w } else { 0.0 },
    })
}

#[cfg(any(target_arch = "wasm32", test))]
fn overlay_viewport(
    overlay: &HistoryOverlay,
    requested_visible: usize,
    cell_w: f32,
    cell_h: f32,
) -> Option<(f32, f32)> {
    if requested_visible == 0
        || overlay.viewport_cols == 0
        || overlay.viewport_rows == 0
        || cell_w <= 0.0
        || cell_h <= 0.0
    {
        return None;
    }
    let width = overlay.viewport_cols as f32 * cell_w;
    let height = overlay.viewport_rows as f32 * cell_h;
    (width >= cell_w && height >= cell_h).then_some((width, height))
}

#[cfg(any(target_arch = "wasm32", test))]
fn overlay_padding(viewport_w: f32, viewport_h: f32, cell_w: f32, cell_h: f32) -> (f32, f32) {
    let pad_w = (0.6 * cell_w).min(((viewport_w - cell_w) / 2.0).max(0.0));
    let pad_h = (0.35 * cell_h).min(((viewport_h - cell_h) / 2.0).max(0.0));
    (pad_w, pad_h)
}

#[cfg(any(target_arch = "wasm32", test))]
fn overlay_scrollbar(
    viewport_w: f32,
    cell_w: f32,
    pad_w: f32,
    visible_count: usize,
    total_items: usize,
) -> (bool, f32, f32) {
    if total_items <= visible_count {
        return (false, 0.0, 0.0);
    }
    let width = (cell_w * 0.30).clamp(4.0, 10.0);
    let gap = (cell_w * 0.18).max(2.0);
    if viewport_w < cell_w + 2.0 * pad_w + width + gap {
        (false, 0.0, 0.0)
    } else {
        (true, width, gap)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn overlay_placement(
    requested_above: bool,
    visible_count: usize,
    above_rows: usize,
    below_rows: usize,
) -> (bool, usize) {
    let preferred = if requested_above {
        above_rows
    } else {
        below_rows
    };
    let fallback = if requested_above {
        below_rows
    } else {
        above_rows
    };
    if preferred >= visible_count {
        return (requested_above, visible_count);
    }
    if fallback >= visible_count {
        return (!requested_above, visible_count);
    }
    if above_rows.max(below_rows) == 0 {
        return (requested_above, visible_count);
    }
    let place_above = above_rows >= below_rows;
    let visible = visible_count.min(if place_above { above_rows } else { below_rows });
    (place_above, visible)
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
        self.update_screen_state(terminal);

        // Selection changed → force redraw so old translucent overlay
        // doesn't linger on rows that left the selection.
        self.update_selection_state(selection);

        // Cursor blink phase: 500ms on / 500ms off, derived from now_ms
        // so all panes blink in unison and we don't need a wakeup timer.
        // Phase change → mark previous cursor row dirty (so the cursor
        // gets erased on the off-half).
        let blink_phase = self.update_blink_phase(terminal, now_ms);
        /*
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
        */

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
        self.update_snapshot_size(rows_n);
        /*
        if self.snapshot.len() != rows_n {
            self.snapshot.resize(rows_n, 0);
            self.full_redraw_pending = true;
            // Backing pixels for new / wrap-around rows are undefined
            // — backend must seed bg so `LoadOp::Load` doesn't expose
            // them.
            self.backend.on_full_invalidate();
        }
        */

        // Backends that can't preserve content across frames (WebGPU
        // clears the swap-chain on every present) need every visible row
        // dirty every tick — otherwise non-dirty rows render only their
        // cleared bg and lose all glyphs. Canvas2D returns false here so
        // dirty-row diffing keeps its perf benefit.
        self.update_backend_frame_policy();

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
        self.update_inline_tui_policy(terminal, wall_ms);

        // Viewport scroll offset change → full redraw. The row→content
        // mapping shifts when the user pages history, so per-row hashes
        // computed against last frame's mapping aren't valid.
        let offset = terminal.scroll_offset();
        self.update_scroll_state(offset);
        /*
        if offset != self.last_offset {
            self.full_redraw_pending = true;
            self.last_offset = offset;
            // Row→content remap means every row's pixels now correspond
            // to a different scrollback position; backend must seed bg
            // so `LoadOp::Load` doesn't carry over the prior mapping.
            self.backend.on_full_invalidate();
        }
        */

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
        let mut dirty_rows = self.collect_dirty_rows(terminal, rows_n);

        // Cursor handling: show the cursor when (a) the surface is
        // focused and (b) we're on the visible half of the blink phase.
        // When the viewport is scrolled into history the cursor is drawn
        // at its shifted on-screen row (`cur.row + offset`) as long as it
        // is still inside the viewport; `compute_cursor_draw` returns None
        // once the cursor scrolls off the bottom. Unfocused panes = no
        // cursor (matches xterm behavior + multi-pane convention).
        let new_cursor = if self.focused && blink_phase {
            self.compute_cursor_draw(terminal, offset)
        } else {
            None
        };

        self.add_cursor_dirty_rows(&mut dirty_rows, &new_cursor);
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
        self.add_selection_dirty_rows(&mut dirty_rows, selection, terminal, rows_n);

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
        let tui_mode = terminal.grid().is_alt_screen()
            || terminal.grid().is_inline_tui_active_at(
                crate::term::clock::now_ms(),
                terminal.modes().cursor_visible,
            );
        let tui_metrics = FrameMetrics {
            tui_mode,
            ..self.metrics
        };
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
            FrameDraw {
                metrics: tui_metrics,
                theme: &self.theme,
                rows: &rows,
                cursor: self.last_cursor.as_ref(),
                attrs_table: &terminal.grid().attrs,
                full_redraw: do_full,
                selection_rects: &sel_rects,
                hyperlink_rects: &hl_rects,
                preedit: self.preedit.as_ref(),
                history_overlay: self.history_overlay.as_ref(),
            },
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
    fn update_screen_state(&mut self, terminal: &Terminal) {
        let is_alt = terminal.is_alt_screen();
        if is_alt != self.last_is_alt {
            self.last_is_alt = is_alt;
            self.invalidate_all();
        }
    }

    fn update_selection_state(&mut self, selection: Option<SelRange>) {
        if selection_eq(selection, self.last_selection) {
            return;
        }
        self.full_redraw_pending = true;
        self.last_selection = selection;
        self.backend.on_full_invalidate();
    }

    fn update_blink_phase(&mut self, terminal: &Terminal, now_ms: f64) -> bool {
        let active = terminal.modes().cursor_visible && terminal.modes().cursor_blink;
        let phase = !active || ((now_ms / 500.0) as i64).rem_euclid(2) == 1;
        if phase != self.last_blink_phase {
            if let Some(previous) = self
                .last_cursor
                .as_ref()
                .filter(|cursor| cursor.row < self.snapshot.len())
            {
                self.snapshot[previous.row] = self.snapshot[previous.row].wrapping_add(1);
            }
            self.last_blink_phase = phase;
        }
        phase
    }

    fn update_snapshot_size(&mut self, rows: usize) {
        if self.snapshot.len() == rows {
            return;
        }
        self.snapshot.resize(rows, 0);
        self.full_redraw_pending = true;
        self.backend.on_full_invalidate();
    }

    fn update_backend_frame_policy(&mut self) {
        if self.backend.requires_full_frame() {
            self.full_redraw_pending = true;
        }
    }

    fn update_inline_tui_policy(&mut self, terminal: &Terminal, now_ms: i64) {
        if terminal
            .grid()
            .is_inline_tui_active_at(now_ms, terminal.modes().cursor_visible)
        {
            self.full_redraw_pending = true;
        }
    }

    fn update_scroll_state(&mut self, offset: usize) {
        if offset == self.last_offset {
            return;
        }
        self.full_redraw_pending = true;
        self.last_offset = offset;
        self.backend.on_full_invalidate();
    }

    fn collect_dirty_rows(&mut self, terminal: &Terminal, rows: usize) -> Vec<usize> {
        let mut dirty = Vec::with_capacity(rows);
        let mut flags = vec![false; rows];
        for (row_index, flag) in flags.iter_mut().enumerate() {
            let Some(row) = terminal.viewport_row(row_index) else {
                continue;
            };
            let hash = compute_row_hash(row);
            if self.full_redraw_pending
                || row_index >= self.snapshot.len()
                || hash != self.snapshot[row_index]
            {
                if row_index < self.snapshot.len() {
                    self.snapshot[row_index] = hash;
                } else {
                    self.snapshot.push(hash);
                }
                dirty.push(row_index);
                *flag = true;
            }
        }
        for row_index in (1..rows).rev() {
            if flags[row_index] && !flags[row_index - 1] {
                dirty.push(row_index - 1);
                flags[row_index - 1] = true;
            }
        }
        dirty
    }

    fn add_cursor_dirty_rows(&mut self, dirty: &mut Vec<usize>, current: &Option<CursorDraw>) {
        if cursor_eq(&self.last_cursor, current) {
            return;
        }
        for row in [
            self.last_cursor.as_ref().map(|cursor| cursor.row),
            current.as_ref().map(|cursor| cursor.row),
        ]
        .into_iter()
        .flatten()
        {
            if !dirty.contains(&row) {
                dirty.push(row);
            }
        }
    }

    fn add_selection_dirty_rows(
        &self,
        dirty: &mut Vec<usize>,
        selection: Option<SelRange>,
        terminal: &Terminal,
        rows: usize,
    ) {
        if self.full_redraw_pending || dirty.is_empty() || selection.is_none() {
            return;
        }
        for (row, _, _) in selection_to_rects(selection, terminal.cols(), terminal.rows()) {
            if row < rows && !dirty.contains(&row) {
                dirty.push(row);
            }
        }
    }

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
        let new_cursor = if self.focused && blink_phase {
            self.compute_cursor_draw(terminal, offset)
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
    fn compute_cursor_draw(&self, terminal: &Terminal, offset: usize) -> Option<CursorDraw> {
        if !terminal.modes().cursor_visible {
            return None;
        }
        let grid = terminal.grid();
        let cur = grid.cursor();
        let row = grid.row(cur.row)?;
        let cell = row.cells.get(cur.col).copied().unwrap_or_default();
        let cluster_text = row.cluster_at(cur.col).map(|c| c.text.as_ref().to_string());
        // The cursor sits at live-grid row `cur.row`; the viewport shifts
        // the live grid down by `offset` (see `Terminal::viewport_row`), so
        // its on-screen row is `cur.row + offset`. Once that lands at or
        // past the bottom of the viewport the cursor has scrolled out of
        // sight — draw nothing. `terminal.rows()` is the same viewport row
        // count `tick` uses (`rows_n`).
        let vp_row = cur.row + offset;
        if vp_row >= terminal.rows() {
            return None;
        }
        Some(CursorDraw {
            row: vp_row,
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

    /// §atlas-pin: passthrough so JS can pin a cached pane's glyph layers
    /// before this frame's full renders run (see `WebGpuPaneBackend::
    /// pin_cached_layers`).
    pub fn pin_cached_layers(&mut self) {
        self.backend.pin_cached_layers()
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
    use super::{compute_row_hash, history_overlay_geometry, HistoryOverlay};
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

    fn overlay(row: usize, col: usize, above: bool, rows: usize, cols: usize) -> HistoryOverlay {
        HistoryOverlay {
            items: vec!["01234567890123456789".into(); 12],
            selected_index: 0,
            anchor_row: row,
            anchor_col: col,
            place_above: above,
            max_visible_rows: 12,
            total_items: 12,
            first_visible: 0,
            viewport_cols: cols,
            viewport_rows: rows,
        }
    }

    #[test]
    fn history_overlay_flips_and_stays_inside_each_dpr_fixture() {
        for dpr in [1.0_f32, 1.25, 1.5, 2.0] {
            let o = overlay(23, 79, false, 24, 80);
            let g = history_overlay_geometry(&o, 20, 8, 8.0 * dpr, 16.0 * dpr).expect("geometry");
            let viewport_w = 80.0 * 8.0 * dpr;
            let viewport_h = 24.0 * 16.0 * dpr;
            assert!(g.panel_x >= 0.0 && g.panel_y >= 0.0);
            assert!(g.panel_x + g.panel_w <= viewport_w + 0.01);
            assert!(g.panel_y + g.panel_h <= viewport_h + 0.01);
            assert!(g.panel_y < 23.0 * 16.0 * dpr, "must flip above");
        }
    }

    #[test]
    fn history_overlay_narrow_width_is_centered() {
        let o = overlay(10, 18, false, 20, 6);
        let g = history_overlay_geometry(&o, 80, 5, 8.0, 16.0).expect("geometry");
        let viewport_w = 6.0 * 8.0;
        let center_error = ((g.panel_x + g.panel_w / 2.0) - viewport_w / 2.0).abs();
        assert!(center_error <= 1.0);
        assert!(g.content_cols >= 1);
    }

    #[test]
    fn history_overlay_reduces_rows_when_neither_side_fits() {
        let o = overlay(2, 2, true, 5, 20);
        let g = history_overlay_geometry(&o, 10, 12, 8.0, 16.0).expect("geometry");
        assert!(g.visible_count < 12);
        assert!(g.panel_y + g.panel_h <= 5.0 * 16.0);
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
