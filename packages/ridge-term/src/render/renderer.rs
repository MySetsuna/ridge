//! Renderer state: dirty row tracking + frame composition.
//!
//! The renderer owns:
//!   - Last-drawn Grid row revisions
//!   - Per-row dirty rows computed from source-side damage revisions
//!   - The backend instance
//!
//! Each `tick()` call:
//!   1. Compare current row revisions vs snapshot → dirty rows
//!   2. If anything changed, ask backend to draw
//!   3. Update snapshot
//!
//! ## Why per-row revisions and not per-cell scanning
//!
//! Grid mutators already know which row changed. A 24-row viewport therefore
//! needs 24 integer compares per frame, rather than hashing 1,920 cells twice
//! (`is_dirty` and `tick`). Backends still redraw a complete dirty row because
//! that is cheaper and safer than partial-row compositing.

use crate::render::backend::{
    draw_frame, CursorDraw, CursorStyle, FrameDraw, FrameMetrics, RenderBackend, RowDraw, Theme,
};
use crate::selection::Range as SelRange;
use crate::term::{cell::Row, grid::ScrollOp, Terminal};

pub struct Renderer<B: RenderBackend> {
    backend: B,
    /// Per-row Grid revision last drawn. Grid mutators advance the revision
    /// at the source, so the render loop never hashes every cell just to
    /// discover that an idle/TUI row is unchanged.
    snapshot: Vec<u64>,
    /// Exact visual content paired with `snapshot`. Revisions are an O(1)
    /// damage hint, but a TUI can clear-and-rewrite an identical row within
    /// one transaction. Only revision-mismatched rows compare this snapshot;
    /// unchanged terminal output stays on the cheap integer path.
    visual_snapshot: Vec<Option<Row>>,
    /// Last-presented cursor descriptor. When the cursor moves (or its row
    /// changes), the row it WAS on must redraw to erase the old cursor.
    /// During a presentation freeze this remains the last actually painted
    /// state instead of following the kernel's cursor walk.
    last_cursor: Option<CursorDraw>,
    /// Cursor descriptor captured at the start of a presentation transaction.
    /// Grid rows continue painting while this is active; only the cursor's
    /// position/shape stays at the last stable presented state.
    frozen_cursor: Option<CursorDraw>,
    /// Last cell that was actually presented, kept across blink-off frames
    /// where `last_cursor` is None so a freeze can still capture that cell.
    last_presented_cursor: Option<CursorDraw>,
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
    /// Presentation-only cursor gate used while a native parser reports an
    /// inline-TUI repaint walk. The grid continues to paint each frame; the
    /// cursor stays frozen at its last presented position until quiet.
    presentation_cursor_suppressed: bool,
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
    /// blank the primary scrollback because per-row revisions happened to
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

/// Shell history is an elevated surface, not another transparent terminal
/// cell. Derive it from the active theme while forcing full opacity so a
/// wallpaper or translucent terminal background can never bleed through.
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) fn history_overlay_surface(bg: [u8; 4], fg: [u8; 4]) -> [u8; 4] {
    let mut color = [0u8; 4];
    for index in 0..3 {
        color[index] = (bg[index] as f32 * 0.88 + fg[index] as f32 * 0.12).round() as u8;
    }
    color[3] = 255;
    color
}

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
            visual_snapshot: Vec::new(),
            last_cursor: None,
            frozen_cursor: None,
            last_presented_cursor: None,
            last_offset: 0,
            last_selection: None,
            last_blink_phase: true,
            focused: true,
            presentation_cursor_suppressed: false,
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
    ///   * `snapshot` — per-row revisions (next tick redraws everything).
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
    /// Also forwards to the WebGPU backend's `invalidate_atlas` so the shared
    /// `GlyphAtlas` drops stale entries sized for the previous metrics.
    pub fn invalidate_all(&mut self) {
        self.snapshot.clear();
        self.visual_snapshot.clear();
        self.last_cursor = None;
        self.frozen_cursor = None;
        self.last_presented_cursor = None;
        self.last_offset = 0;
        self.last_selection = None;
        self.last_blink_phase = true;
        self.full_redraw_pending = true;
        self.backend.invalidate_atlas();
    }

    /// Repaint the complete viewport without invalidating the shared glyph
    /// atlas. Structural compositor seeds erase pixels, not font resources.
    pub fn request_full_redraw(&mut self) {
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
    }

    /// Freeze or restore only the painted terminal cursor. On the rising edge,
    /// capture the last presented descriptor once; grid paints remain on the
    /// normal compositor cadence while cursor-rewind intermediates cannot
    /// move through the output area. The old API name stays wire-compatible.
    pub fn set_presentation_cursor_suppressed(&mut self, suppressed: bool) {
        if self.presentation_cursor_suppressed == suppressed {
            return;
        }
        self.frozen_cursor = if suppressed {
            self.last_cursor
                .clone()
                .or_else(|| self.last_presented_cursor.clone())
        } else {
            None
        };
        self.presentation_cursor_suppressed = suppressed;
    }

    /// Last cell actually presented, including a frozen cursor. Read-only
    /// probe for DEV/e2e; the compositor hot path does not call this.
    pub fn presented_cursor_cell(&self) -> Option<(usize, usize)> {
        self.last_cursor
            .as_ref()
            .or(self.frozen_cursor.as_ref())
            .or(self.last_presented_cursor.as_ref())
            .map(|cursor| (cursor.row, cursor.col))
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

    /// Drive one frame. Returns `true` if anything was drawn; the caller uses
    /// this to avoid scheduling or presenting an idle WebGPU frame.
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
        self.tick_with_scroll(terminal, selection, now_ms, &[])
    }

    /// Drive one frame, optionally reusing pixels for one compacted physical
    /// scroll observed by the VT kernel. Primary shells, inline TUIs,
    /// alt-screen applications and DECSTBM regions share this path; overlays,
    /// invalid mappings and structural invalidation fall back to repainting.
    pub fn tick_with_scroll(
        &mut self,
        terminal: &Terminal,
        selection: Option<SelRange>,
        now_ms: f64,
        scroll_ops: &[ScrollOp],
    ) -> bool {
        let rows_n = terminal.rows();

        // Screen-switch invalidation: when the active screen flips
        // (DECSET/DECRST ?1049 / ?47 / ?1047), the snapshot was built
        // against the *previous* screen's rows. Without clearing it,
        // exiting a fullscreen TUI like `vim` or `htop` could leave the
        // primary scrollback blank — alt-screen rows and the now-active
        // primary rows would have unrelated revision baselines and
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
        // Historical root cause (2026-05-07): the removed Canvas2D path only
        // handled growth, so a narrowing resize left the dirty-row cache sized
        // to the old grid and never marked trailing pixels for redraw. Those
        // stale pixels resurfaced when the size was restored (§1.26).
        // Current shared GPU rendering keeps the invariant: snapshot
        // cardinality must track `rows_n`, so the next frame re-snapshots each
        // source revision and paints blanks over every former cell.
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

        // Honor backend one-frame seed requests. WebGPU's persistent frame
        // store normally preserves clean rows; after construction or a
        // structural invalidation `requires_full_frame()` forces one complete
        // encode, then row-revision diffing resumes.
        self.update_backend_frame_policy();

        // Paint truth is the Grid row-revision snapshot, not "the app used
        // absolute CSI / looks like a TUI". Ratatui/crossterm (Codex CLI)
        // and Ink double-buffer then emit VT for (often large) regions;
        // between two *settled* frames most cells are identical. Forcing
        // full_redraw_pending here used to wipe the whole viewport on
        // every tick while inline-TUI was active → visible flash even
        // when revisions matched. Content-diff via collect_dirty_rows is
        // enough; true mapping breaks (resize/scroll/screen/first frame)
        // still set full_redraw_pending elsewhere. IME preedit install
        // has its own invalidate path.

        // Viewport scroll offset change → full redraw. The row→content
        // mapping shifts when the user pages history, so per-row revisions
        // stored against the last mapping aren't valid.
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

        // For ordinary shell/TUI output, a scroll moves rows without changing
        // their cells. Record the physical copy before advancing the snapshot;
        // the backend must return every exposed or non-copyable destination so
        // failed/fractional-DPR copies are repaired in this same frame.
        let scroll_copy = self.scroll_copy_candidate(terminal, selection, offset, scroll_ops);
        let scroll_repaint_rows = if let Some(scroll) = scroll_copy {
            let result = self.backend.scroll_rows(scroll, self.metrics);
            self.shift_snapshot_for_scroll(scroll);
            result.repaint_rows
        } else {
            Vec::new()
        };

        // Grid mutators advance the affected row revision, including
        // hyperlink and grapheme sidecars. We read via `viewport_row` so the
        // same O(rows) revision compare covers live grid and scrollback views
        // without allocating a flags array or hashing every visible cell.
        let mut dirty_rows = self.collect_dirty_rows(terminal, rows_n);
        for row in scroll_repaint_rows {
            if row < rows_n && !dirty_rows.contains(&row) {
                dirty_rows.push(row);
            }
        }

        let tui_mode = Self::is_tui_mode(terminal);

        // Cursor handling: obey focus + DEC ?25. TUI frames keep an enabled
        // cursor continuously visible; this avoids blink churn without
        // guessing that every fullscreen/inline application paints a caret.
        // During a presentation transaction, keep that gate but use the
        // descriptor captured before the kernel's cursor walk. This keeps
        // grid paints immediate without exposing intermediate cursor moves.
        let cursor_visible =
            self.focused && terminal.modes().cursor_visible && (tui_mode || blink_phase);
        let computed_cursor = if cursor_visible && !self.presentation_cursor_suppressed {
            self.compute_cursor_draw(terminal, offset)
        } else {
            None
        };
        if self.presentation_cursor_suppressed {
            self.refresh_frozen_cursor_cell(terminal);
        }
        let new_cursor = if self.presentation_cursor_suppressed {
            if cursor_visible {
                self.frozen_cursor
                    .as_ref()
                    .filter(|cursor| cursor.row < rows_n && cursor.col < terminal.cols())
            } else {
                None
            }
        } else {
            computed_cursor.as_ref()
        };

        if let Some(scroll) = scroll_copy {
            self.add_scrolled_cursor_dirty_row(&mut dirty_rows, scroll);
        }
        self.add_cursor_dirty_rows(&mut dirty_rows, new_cursor);
        if self.presentation_cursor_suppressed {
            if new_cursor.is_some() {
                if !cursor_refs_eq(self.last_cursor.as_ref(), new_cursor) {
                    // Only allocate when a blink/focus/visibility transition
                    // changes the actually presented cursor. Steady frozen
                    // frames borrow `frozen_cursor` directly.
                    self.last_cursor = new_cursor.cloned();
                }
            } else {
                self.last_cursor = None;
            }
        }

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
        let tui_metrics = FrameMetrics {
            tui_mode,
            ..self.metrics
        };
        // Hyperlink underlines share the persistent frame store with row
        // pixels. Re-emit spans only for rows painted in this frame; a full
        // redraw still supplies every visible row through `dirty_rows`.
        let mut hl_rects: Vec<(usize, usize, usize)> = Vec::new();
        for row_draw in &rows {
            let Some(row) = terminal.viewport_row(row_draw.row_index) else {
                continue;
            };
            for span in &row.hyperlinks {
                hl_rects.push((row_draw.row_index, span.col_start, span.col_end));
            }
        }
        draw_frame(
            &mut self.backend,
            FrameDraw {
                metrics: tui_metrics,
                theme: &self.theme,
                rows: &rows,
                cursor: new_cursor,
                attrs_table: &terminal.grid().attrs,
                full_redraw: do_full,
                selection_rects: &sel_rects,
                hyperlink_rects: &hl_rects,
                preedit: self.preedit.as_ref(),
                history_overlay: self.history_overlay.as_ref(),
            },
        );
        if !self.presentation_cursor_suppressed {
            self.last_cursor = computed_cursor;
        }
        self.remember_presented_cursor();
        self.first_frame = false;
        self.full_redraw_pending = false;
        true
    }

    /// Return the one scroll which is safe to represent as a GPU copy rather
    /// than a full viewport repaint. Keeping this conservative is important:
    /// a wrong copy gives visible stale pixels, while a rejected copy merely
    /// falls back to the existing exact revision renderer.
    fn scroll_copy_candidate(
        &self,
        terminal: &Terminal,
        selection: Option<SelRange>,
        offset: usize,
        scroll_ops: &[ScrollOp],
    ) -> Option<ScrollOp> {
        let [scroll] = scroll_ops else {
            return None;
        };
        let rows = terminal.rows();
        if !self.backend.supports_scroll_copy()
            || self.first_frame
            || self.full_redraw_pending
            || selection.is_some()
            || self.last_selection.is_some()
            || self.preedit.is_some()
            || self.history_overlay.is_some()
            || offset != 0
            || scroll.count == 0
            || scroll.bottom >= rows
            || scroll.top > scroll.bottom
        {
            return None;
        }
        let height = scroll.bottom.saturating_sub(scroll.top).saturating_add(1);
        if scroll.count >= height {
            return None;
        }
        Some(*scroll)
    }

    /// Keep the renderer's row-revision snapshot aligned with a successful
    /// pixel copy. Moved rows retain their source revisions; newly exposed
    /// rows receive an impossible baseline and therefore repaint once.
    fn shift_snapshot_for_scroll(&mut self, scroll: ScrollOp) {
        let rows = self.snapshot.len();
        if rows == 0 {
            return;
        }
        if self.visual_snapshot.len() != rows {
            self.visual_snapshot.resize(rows, None);
        }
        let top = scroll.top.min(rows - 1);
        let bottom = scroll.bottom.min(rows - 1);
        if top > bottom {
            return;
        }
        let count = scroll.count.min(bottom - top + 1);
        if count == 0 {
            return;
        }
        if scroll.up {
            self.snapshot[top..=bottom].rotate_left(count);
            self.visual_snapshot[top..=bottom].rotate_left(count);
            for revision in &mut self.snapshot[bottom + 1 - count..=bottom] {
                *revision = u64::MAX;
            }
            for row in &mut self.visual_snapshot[bottom + 1 - count..=bottom] {
                *row = None;
            }
        } else {
            self.snapshot[top..=bottom].rotate_right(count);
            self.visual_snapshot[top..=bottom].rotate_right(count);
            for revision in &mut self.snapshot[top..top + count] {
                *revision = u64::MAX;
            }
            for row in &mut self.visual_snapshot[top..top + count] {
                *row = None;
            }
        }
    }

    /// A painted cursor is part of the old pixels, not the grid revision.
    /// When pixels move we must repaint its mapped destination even if the
    /// logical cursor remains at the same terminal coordinate.
    fn add_scrolled_cursor_dirty_row(&self, dirty: &mut Vec<usize>, scroll: ScrollOp) {
        let Some(previous) = self.last_cursor.as_ref() else {
            return;
        };
        let row = if scroll.up
            && previous.row >= scroll.top.saturating_add(scroll.count)
            && previous.row <= scroll.bottom
        {
            Some(previous.row - scroll.count)
        } else if !scroll.up
            && previous.row >= scroll.top
            && previous.row <= scroll.bottom.saturating_sub(scroll.count)
        {
            Some(previous.row + scroll.count)
        } else {
            None
        };
        if let Some(row) = row.filter(|row| !dirty.contains(row)) {
            dirty.push(row);
        }
    }

    /// Non-mutating mirror of the early-exit conditions in `tick`.
    /// Returns true when the next `tick` call would do any drawing
    /// work — false when the renderer has nothing to redraw and the
    /// caller can safely sleep its RAF loop. Used by `manager.ts` to
    /// pause the per-pane animation frame loop on idle.
    ///
    /// Cost: one integer revision compare per visible row. `tick` repeats
    /// the compare before committing its snapshot, preserving the
    /// non-mutating semantics needed by the JS scheduler.
    fn update_screen_state(&mut self, terminal: &Terminal) {
        let is_alt = terminal.is_alt_screen();
        if is_alt != self.last_is_alt {
            self.last_is_alt = is_alt;
            self.invalidate_all();
        }
    }

    /// Detect fullscreen and inline TUI modes. Cursor blink scheduling uses
    /// this to hold an enabled terminal cursor steady, while DEC ?25 remains
    /// authoritative for visibility.
    fn is_tui_mode(terminal: &Terminal) -> bool {
        terminal.is_alt_screen() || terminal.is_inline_tui_resize_at(crate::term::clock::now_ms())
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
            self.last_blink_phase = phase;
        }
        phase
    }

    fn update_snapshot_size(&mut self, rows: usize) {
        if self.snapshot.len() == rows {
            return;
        }
        self.snapshot.resize(rows, 0);
        self.visual_snapshot.resize(rows, None);
        self.full_redraw_pending = true;
        self.backend.on_full_invalidate();
    }

    fn update_backend_frame_policy(&mut self) {
        if self.backend.requires_full_frame() {
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
        let mut previous_dirty = false;
        for row_index in 0..rows {
            let Some(row) = terminal.viewport_row(row_index) else {
                previous_dirty = false;
                continue;
            };
            let revision = row.revision();
            let row_dirty = self.full_redraw_pending
                || row_index >= self.snapshot.len()
                || revision != self.snapshot[row_index];
            if !row_dirty {
                previous_dirty = false;
                continue;
            }
            if row_index < self.snapshot.len() {
                self.snapshot[row_index] = revision;
            } else {
                self.snapshot.push(revision);
            }
            if row_index >= self.visual_snapshot.len() {
                self.visual_snapshot.resize(row_index + 1, None);
            }
            let content_changed = self.full_redraw_pending
                || self.visual_snapshot[row_index]
                    .as_ref()
                    .is_none_or(|previous| !row.visual_eq(previous));
            if content_changed {
                self.visual_snapshot[row_index] = Some(row.clone());
                if row_index > 0 && !previous_dirty {
                    dirty.push(row_index - 1);
                }
                dirty.push(row_index);
            }
            previous_dirty = content_changed;
        }
        dirty
    }

    fn add_cursor_dirty_rows(&self, dirty: &mut Vec<usize>, current: Option<&CursorDraw>) {
        if cursor_refs_eq(self.last_cursor.as_ref(), current) {
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

    fn remember_presented_cursor(&mut self) {
        if self.last_cursor.is_none()
            || cursor_refs_eq(
                self.last_presented_cursor.as_ref(),
                self.last_cursor.as_ref(),
            )
        {
            return;
        }
        self.last_presented_cursor = self.last_cursor.clone();
    }

    /// Refresh only the cell payload carried by a frozen cursor descriptor.
    /// Its row/column/style stay fixed; glyph data follows the live grid so a
    /// rewrite under the stable cursor does not paint stale text. The common
    /// non-cluster path performs no allocation.
    fn refresh_frozen_cursor_cell(&mut self, terminal: &Terminal) {
        let Some(cursor) = self.frozen_cursor.as_mut() else {
            return;
        };
        let Some(row) = terminal.viewport_row(cursor.row) else {
            return;
        };
        let Some(cell) = row.cells.get(cursor.col).copied() else {
            return;
        };
        cursor.ch = cell.ch;
        cursor.ch_attr = cell.attr;
        cursor.width = cell.width.max(1);
        match row.cluster_at(cursor.col) {
            Some(cluster) if cursor.cluster_text.as_deref() != Some(cluster.text.as_ref()) => {
                cursor.cluster_text = Some(cluster.text.to_string());
            }
            Some(_) => {}
            None => cursor.cluster_text = None,
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
        if self.first_frame || self.full_redraw_pending || self.backend.requires_full_frame() {
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
        let tui_mode = Self::is_tui_mode(terminal);
        if self.focused && !tui_mode && blink_phase != self.last_blink_phase {
            return true;
        }

        // Snapshot length mismatch → grid dimensions changed.
        let rows_n = terminal.rows();
        if self.snapshot.len() != rows_n {
            return true;
        }

        // Per-row source revision diff. A changed revision may still carry
        // identical final pixels after a TUI's clear-and-rewrite transaction;
        // compare only those rows against the exact painted snapshot.
        for r in 0..rows_n {
            let Some(row) = terminal.viewport_row(r) else {
                continue;
            };
            if row.revision() != self.snapshot[r] {
                let unchanged = self
                    .visual_snapshot
                    .get(r)
                    .and_then(Option::as_ref)
                    .is_some_and(|previous| row.visual_eq(previous));
                if !unchanged {
                    return true;
                }
            }
        }

        // Cursor moved (position / style / glyph beneath). During a
        // presentation transaction compare against the frozen descriptor so
        // kernel cursor walks do not wake or move the painted cursor.
        let offset = terminal.scroll_offset();
        let cursor_visible =
            self.focused && terminal.modes().cursor_visible && (tui_mode || blink_phase);
        if self.presentation_cursor_suppressed {
            let new_cursor = if cursor_visible {
                self.frozen_cursor
                    .as_ref()
                    .filter(|cursor| cursor.row < terminal.rows() && cursor.col < terminal.cols())
            } else {
                None
            };
            return !cursor_refs_eq(self.last_cursor.as_ref(), new_cursor);
        }
        let new_cursor = if cursor_visible {
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
        if Self::is_tui_mode(terminal) {
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

fn cursor_eq(a: &Option<CursorDraw>, b: &Option<CursorDraw>) -> bool {
    cursor_refs_eq(a.as_ref(), b.as_ref())
}

fn cursor_refs_eq(a: Option<&CursorDraw>, b: Option<&CursorDraw>) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{history_overlay_geometry, history_overlay_surface, HistoryOverlay};

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
    fn history_overlay_surface_is_theme_derived_and_always_opaque() {
        assert_eq!(
            history_overlay_surface([10, 20, 30, 0], [110, 120, 130, 64]),
            [22, 32, 42, 255]
        );
        assert_eq!(
            history_overlay_surface([245, 245, 245, 80], [5, 5, 5, 0])[3],
            255
        );
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
            // per-row source revision. Filling them with arbitrary values
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
        // row via its revision, so the cursor doesn't need to also re-mark.
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

    // ─── settled-frame dirty paint (Codex/ratatui-style) ───────────────
    //
    // Codex CLI interactive UI = Rust `codex-tui` (openai/codex codex-rs/tui):
    // **ratatui + crossterm** double-buffer → emit VT for buffer diffs;
    // typically alt-screen and optional synchronized output `CSI ? 2026 h/l`.
    // Evidence: upstream `codex-rs/tui/Cargo.toml` deps + local `codex.exe`
    // strings (`ratatui`, `crossterm`, `codex_tui`). Paint policy must treat
    // large VT rewrites as grid updates and dirty only revision-changed rows —
    // never whole-viewport clear solely because absolute CSI / TUI is active.

    use super::Renderer;
    use crate::render::backend::{FrameMetrics, RenderBackend, RowDraw, ScrollCopyResult, Theme};
    use crate::term::attr_table::AttrTable;
    use crate::term::{grid::ScrollOp, Terminal};

    #[derive(Default)]
    struct RecordingBackend {
        clears: u32,
        frames: u32,
        cursors: u32,
        cursor_positions: Vec<(usize, usize)>,
        atlas_invalidations: u32,
        scroll_copy: bool,
        scroll_copy_fails: bool,
        scroll_repaint_rows: Option<Vec<usize>>,
        requires_full_frame: bool,
        scrolls: Vec<ScrollOp>,
        drawn_rows: Vec<usize>,
        last_drawn: Vec<usize>,
        hyperlink_rects: Vec<(usize, usize, usize)>,
    }

    impl RenderBackend for RecordingBackend {
        fn measure_font(&self, _: &str, _: f32) -> Result<(f32, f32), String> {
            Ok((8.0, 16.0))
        }
        fn resize_surface(&mut self, _: u32, _: u32, _: f32) -> Result<(), String> {
            Ok(())
        }
        fn begin_frame(&mut self, _: FrameMetrics, _: &Theme) {
            self.last_drawn.clear();
            self.hyperlink_rects.clear();
        }
        fn clear(&mut self) {
            self.clears += 1;
        }
        fn invalidate_atlas(&mut self) {
            self.atlas_invalidations += 1;
        }
        fn supports_scroll_copy(&self) -> bool {
            self.scroll_copy
        }
        fn requires_full_frame(&self) -> bool {
            self.requires_full_frame
        }
        fn scroll_rows(&mut self, scroll: ScrollOp, _: FrameMetrics) -> ScrollCopyResult {
            self.scrolls.push(scroll);
            if self.scroll_copy_fails {
                ScrollCopyResult::repaint_all(scroll)
            } else if let Some(rows) = self.scroll_repaint_rows.clone() {
                ScrollCopyResult::new(rows)
            } else {
                ScrollCopyResult::copied(scroll)
            }
        }
        fn draw_row_backgrounds(&mut self, row: &RowDraw<'_>, _: &AttrTable) {
            self.last_drawn.push(row.row_index);
            self.drawn_rows.push(row.row_index);
        }
        fn draw_row_texts(&mut self, _: &RowDraw<'_>, _: &AttrTable) {}
        fn draw_cursor(&mut self, cursor: &CursorDraw, _: &AttrTable) {
            self.cursors += 1;
            self.cursor_positions.push((cursor.row, cursor.col));
        }
        fn draw_selection_overlay(&mut self, _: &[(usize, usize, usize)]) {}
        fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
            self.hyperlink_rects.extend_from_slice(rects);
        }
        fn end_frame(&mut self) {
            self.frames += 1;
            self.requires_full_frame = false;
        }
    }

    fn metrics() -> FrameMetrics {
        FrameMetrics {
            cell_w: 8.0,
            cell_h: 16.0,
            dpr: 1.0,
            tui_mode: false,
        }
    }

    #[test]
    fn fullscreen_tui_keeps_terminal_cursor_steady_without_blink_wakeup() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"\x1b[?1049h");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );

        assert!(renderer.tick(&term, None, 500.0));
        assert_eq!(renderer.backend().cursors, 1);
        assert!(!renderer.is_dirty(&term, None, 1000.0));
        assert!(renderer.next_blink_deadline_ms(&term, 1000.0).is_infinite());

        term.feed(b"\x1b[?25l");
        assert!(renderer.tick(&term, None, 1000.0));
        assert_eq!(
            renderer.backend().cursors,
            1,
            "DECTCEM off must hide the cursor"
        );
    }

    #[test]
    fn inline_tui_keeps_visible_terminal_cursor_steady() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"\x1b[?1h\x1b[H");
        assert!(term.is_inline_tui_resize_at(crate::term::clock::now_ms()));

        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        assert_eq!(renderer.backend().cursors, 1);
        assert!(!renderer.is_dirty(&term, None, 1000.0));
        assert!(renderer.next_blink_deadline_ms(&term, 1000.0).is_infinite());
    }

    #[test]
    fn compositor_seed_repaints_all_rows_without_dropping_atlas() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"stable");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 0.0));
        renderer.backend_mut().last_drawn.clear();
        let clears = renderer.backend().clears;

        renderer.request_full_redraw();
        assert!(renderer.tick(&term, None, 0.0));
        assert_eq!(renderer.backend().last_drawn, vec![0, 1, 2, 3]);
        assert_eq!(renderer.backend().clears, clears + 1);
        assert_eq!(renderer.backend().atlas_invalidations, 0);
    }

    /// Ratatui-like settled frame: sync begin, home, paint all rows, sync end.
    fn feed_ratatui_frame(t: &mut Terminal, marker: &str) {
        let cols = t.cols();
        let rows = t.rows();
        let mut out = Vec::new();
        out.extend_from_slice(b"\x1b[?2026h");
        out.extend_from_slice(b"\x1b[H");
        for r in 0..rows {
            let line = format!(
                "{m}{r:02}{pad}",
                m = marker,
                r = r,
                pad = " ".repeat(cols.saturating_sub(marker.len() + 2))
            );
            let line: String = line.chars().take(cols).collect();
            out.extend_from_slice(line.as_bytes());
            if r + 1 < rows {
                out.extend_from_slice(b"\r\n");
            }
        }
        out.extend_from_slice(b"\x1b[?2026l");
        t.feed(&out);
    }

    #[test]
    fn settled_identical_ratatui_frames_skip_draw_and_clear() {
        let rows = 8usize;
        let cols = 24usize;
        let mut term = Terminal::new(rows, cols, 0);
        // Hide cursor so blink/cursor rows cannot dirtiness-noise the assert.
        term.feed(b"\x1b[?25l");
        feed_ratatui_frame(&mut term, "A");

        let mut r = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(r.tick(&term, None, 0.0), "first frame must paint");
        let clears_after_first = r.backend().clears;
        assert!(clears_after_first >= 1, "seed frame clears once");
        let frames_after_first = r.backend().frames;

        // Second settled frame: full-region rewrite VT, identical cells.
        feed_ratatui_frame(&mut term, "A");
        let drew = r.tick(&term, None, 0.0);
        assert!(
            !drew,
            "identical settled content must return nothing-to-draw"
        );
        assert_eq!(
            r.backend().clears,
            clears_after_first,
            "must not whole-viewport clear when row revisions are stable"
        );
        assert_eq!(r.backend().frames, frames_after_first);
        assert!(
            !r.is_dirty(&term, None, 0.0),
            "is_dirty must agree with tick early-exit"
        );
    }

    #[test]
    fn small_content_change_dirties_only_affected_rows() {
        let rows = 10usize;
        let cols = 20usize;
        let mut term = Terminal::new(rows, cols, 0);
        term.feed(b"\x1b[?25l");
        feed_ratatui_frame(&mut term, "X");

        let mut r = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(r.tick(&term, None, 0.0));
        let clears_seed = r.backend().clears;
        r.backend_mut().last_drawn.clear();
        r.backend_mut().drawn_rows.clear();

        // Change a single near-top row via CUP + EL + rewrite (crossterm style).
        // collect_dirty_rows also marks the prior row for glyph bleed; with
        // change at row 1 the dirty set is {0,1} — cardinality << rows.
        let change_row = 1usize;
        let line = format!("Y{:02}{}", change_row, " ".repeat(cols));
        let line: String = line.chars().take(cols).collect();
        let seq = format!("\x1b[{};1H\x1b[2K{}", change_row + 1, line);
        term.feed(seq.as_bytes());

        assert!(r.tick(&term, None, 0.0), "changed row must paint");
        assert_eq!(
            r.backend().clears,
            clears_seed,
            "partial change must not full-clear"
        );
        let mut unique: Vec<usize> = r.backend().last_drawn.clone();
        unique.sort_unstable();
        unique.dedup();
        assert!(
            !unique.is_empty() && unique.len() * 2 < rows,
            "dirty cardinality {unique:?} must be << viewport rows={rows}"
        );
        assert!(
            unique.contains(&change_row),
            "changed row {change_row} must be dirty; got {unique:?}"
        );
        assert!(
            !unique.contains(&(rows - 1)),
            "unchanged bottom row must stay clean; got {unique:?}"
        );
    }

    #[test]
    fn primary_shell_scroll_copies_pixels_and_repaints_only_exposed_rows() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"A\r\nB\r\nC\r\nD");
        let mut renderer = Renderer::new(
            RecordingBackend {
                scroll_copy: true,
                ..RecordingBackend::default()
            },
            metrics(),
            Theme::default_dark(),
        );
        renderer.set_focused(false);
        assert!(renderer.tick(&term, None, 0.0));
        renderer.backend_mut().last_drawn.clear();
        renderer.backend_mut().scrolls.clear();

        term.feed(b"\r\nE");
        let scrolls = term.take_scroll_ops();
        assert_eq!(scrolls.len(), 1, "fixture must scroll once");
        assert!(renderer.tick_with_scroll(&term, None, 0.0, &scrolls));

        assert_eq!(
            renderer.backend().scrolls,
            vec![ScrollOp {
                top: 0,
                bottom: 3,
                count: 1,
                up: true,
            }],
            "the backend receives the physical move instead of a full repaint"
        );
        let mut drawn = renderer.backend().last_drawn.clone();
        drawn.sort_unstable();
        drawn.dedup();
        assert_eq!(drawn, vec![2, 3]);
    }

    #[test]
    fn backend_only_full_frame_request_wakes_idle_renderer_once() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"stable");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 0.0));
        assert!(!renderer.is_dirty(&term, None, 0.0));

        renderer.backend_mut().requires_full_frame = true;
        assert!(renderer.is_dirty(&term, None, 0.0));
        assert!(renderer.tick(&term, None, 0.0));
        assert_eq!(renderer.backend().last_drawn, vec![0, 1, 2, 3]);
        assert!(!renderer.is_dirty(&term, None, 0.0));
    }

    #[test]
    fn alt_screen_scroll_copies_pixels_and_repaints_exposed_band() {
        let mut term = Terminal::new(6, 12, 0);
        term.feed(
            b"\x1b[?1049h\x1b[?25l\x1b[1;1Hline1\r\nline2\r\nline3\r\nline4\r\nline5\r\nline6",
        );
        let mut renderer = Renderer::new(
            RecordingBackend {
                scroll_copy: true,
                ..RecordingBackend::default()
            },
            metrics(),
            Theme::default_dark(),
        );
        renderer.set_focused(false);
        assert!(renderer.tick(&term, None, 0.0));
        renderer.backend_mut().last_drawn.clear();
        renderer.backend_mut().scrolls.clear();

        term.feed(b"\nnext");
        let scrolls = term.take_scroll_ops();
        assert!(!scrolls.is_empty(), "alt-screen newline must emit a scroll");
        assert!(renderer.tick_with_scroll(&term, None, 0.0, &scrolls));
        assert!(
            !renderer.backend().scrolls.is_empty(),
            "TUI/alt-screen scrolls must use the pixel copy path"
        );
        assert_eq!(
            renderer.backend().clears,
            1,
            "TUI scroll must not clear the whole viewport"
        );
        let mut drawn = renderer.backend().last_drawn.clone();
        drawn.sort_unstable();
        drawn.dedup();
        assert_eq!(drawn, vec![4, 5]);
    }

    #[test]
    fn inline_tui_scroll_copies_pixels_and_repaints_exposed_band() {
        let mut term = Terminal::new(6, 12, 0);
        term.feed(b"\x1b[?25l");
        feed_ratatui_frame(&mut term, "I");
        assert!(term
            .grid()
            .is_inline_tui_active_at(crate::term::clock::now_ms(), false));

        let mut renderer = Renderer::new(
            RecordingBackend {
                scroll_copy: true,
                ..RecordingBackend::default()
            },
            metrics(),
            Theme::default_dark(),
        );
        renderer.set_focused(false);
        assert!(renderer.tick(&term, None, 0.0));
        renderer.backend_mut().last_drawn.clear();
        renderer.backend_mut().scrolls.clear();

        term.feed(b"\x1b[6;1H\nnext");
        let scrolls = term.take_scroll_ops();
        assert_eq!(scrolls.len(), 1, "inline TUI fixture must scroll once");
        assert!(renderer.tick_with_scroll(&term, None, 0.0, &scrolls));
        assert_eq!(renderer.backend().scrolls, scrolls);
        assert_eq!(renderer.backend().clears, 1);
        let mut drawn = renderer.backend().last_drawn.clone();
        drawn.sort_unstable();
        drawn.dedup();
        assert_eq!(drawn, vec![4, 5]);
    }

    #[test]
    fn failed_scroll_copy_repaints_region_and_keeps_snapshot_exact() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"A\r\nB\r\nC\r\nD");
        let mut renderer = Renderer::new(
            RecordingBackend {
                scroll_copy: true,
                scroll_copy_fails: true,
                ..RecordingBackend::default()
            },
            metrics(),
            Theme::default_dark(),
        );
        renderer.set_focused(false);
        assert!(renderer.tick(&term, None, 0.0));
        renderer.backend_mut().last_drawn.clear();

        term.feed(b"\r\nE");
        let scrolls = term.take_scroll_ops();
        assert!(renderer.tick_with_scroll(&term, None, 0.0, &scrolls));
        let mut drawn = renderer.backend().last_drawn.clone();
        drawn.sort_unstable();
        drawn.dedup();
        assert_eq!(drawn, vec![0, 1, 2, 3]);
        assert!(
            !renderer.tick(&term, None, 0.0),
            "failed copy fallback must still commit the exact row snapshot"
        );
    }

    #[test]
    fn decstbm_partial_region_scroll_copies_only_the_region() {
        let mut term = Terminal::new(8, 12, 0);
        // Set a four-row DECSTBM region (terminal rows 2..=5), then fill it
        // without scrolling so the next line produces exactly one partial
        // region scroll operation.
        term.feed(b"\x1b[2;5r\x1b[2;1HA\r\nB\r\nC\r\nD");
        assert!(term.take_scroll_ops().is_empty());

        let mut renderer = Renderer::new(
            RecordingBackend {
                scroll_copy: true,
                ..RecordingBackend::default()
            },
            metrics(),
            Theme::default_dark(),
        );
        renderer.set_focused(false);
        assert!(renderer.tick(&term, None, 0.0));
        renderer.backend_mut().last_drawn.clear();
        renderer.backend_mut().scrolls.clear();

        term.feed(b"\r\nE");
        let scrolls = term.take_scroll_ops();
        assert_eq!(
            scrolls,
            vec![ScrollOp {
                top: 1,
                bottom: 4,
                count: 1,
                up: true,
            }],
            "DECSTBM must emit a bounded region scroll"
        );
        assert!(renderer.tick_with_scroll(&term, None, 0.0, &scrolls));
        assert_eq!(renderer.backend().scrolls, scrolls);
        let mut drawn = renderer.backend().last_drawn.clone();
        drawn.sort_unstable();
        drawn.dedup();
        assert_eq!(drawn, vec![3, 4]);
    }

    #[test]
    fn osc_8_rewrite_advances_row_revision_for_renderer() {
        let mut term = Terminal::new(2, 12, 0);
        term.feed(b"X");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 0.0));

        // Reprint under OSC 8 at the same coordinate. This must wake the
        // revision-driven renderer, including the hyperlink underline pass.
        term.feed(b"\x1b[HX\x1b]8;;https://example.com\x07\x1b[HX\x1b]8;;\x07");
        assert!(
            renderer.is_dirty(&term, None, 0.0),
            "OSC 8 rewrite must wake the renderer"
        );
        assert!(renderer.tick(&term, None, 0.0));
        assert!(renderer.backend().last_drawn.contains(&0));
    }

    #[test]
    fn partial_repaint_emits_hyperlinks_only_for_painted_rows() {
        let mut term = Terminal::new(4, 16, 0);
        term.feed(b"\x1b]8;;https://example.com\x07link\x1b]8;;\x07");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        renderer.set_focused(false);

        assert!(renderer.tick(&term, None, 0.0));
        assert_eq!(
            renderer.backend().hyperlink_rects,
            vec![(0, 0, 4)],
            "full redraw must emit the visible link"
        );

        // Change a distant row. The persistent frame store retains row 0,
        // so its underline must not be rebuilt for this partial paint.
        term.feed(b"\x1b[4;1HZ");
        assert!(renderer.tick(&term, None, 0.0));
        assert!(
            renderer.backend().hyperlink_rects.is_empty(),
            "partial redraw must not scan or re-emit clean-row links"
        );
    }

    #[test]
    fn presentation_cursor_freeze_keeps_grid_paints_immediate() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"before");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        let clears_after_seed = renderer.backend().clears;
        let cursors_after_seed = renderer.backend().cursors;

        renderer.set_presentation_cursor_suppressed(true);
        term.feed(b"\x1b[Hafter");
        assert!(renderer.tick(&term, None, 500.0));
        assert!(
            !renderer.backend().last_drawn.is_empty(),
            "grid changes must paint while the cursor is frozen"
        );
        assert_eq!(
            renderer.backend().cursors,
            cursors_after_seed + 1,
            "freeze must keep the last presented cursor visible"
        );
        assert_eq!(
            renderer.backend().cursor_positions.last().copied(),
            Some((0, 6)),
            "cursor must stay at its stable pre-transaction cell"
        );
        assert_eq!(
            renderer.backend().clears,
            clears_after_seed,
            "cursor freeze must not turn a partial repaint into a clear"
        );

        renderer.set_presentation_cursor_suppressed(false);
        assert!(renderer.is_dirty(&term, None, 500.0));
        assert!(renderer.tick(&term, None, 500.0));
        assert_eq!(renderer.backend().cursors, cursors_after_seed + 2);
        assert_eq!(
            renderer.backend().cursor_positions.last().copied(),
            Some((0, 5)),
            "quiet boundary must present the kernel's final cursor"
        );
    }

    #[test]
    fn presentation_cursor_freezes_across_rewind_frames() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        let stable = renderer.backend().cursor_positions.last().copied();
        assert_eq!(stable, Some((0, 4)));

        renderer.set_presentation_cursor_suppressed(true);
        for (row, col, ch) in [(1usize, 2usize, 'A'), (2, 5, 'B'), (1, 8, 'C')] {
            let frame = format!("\x1b[{};{}H{}", row + 1, col + 1, ch);
            term.feed(frame.as_bytes());
            assert!(renderer.tick(&term, None, 500.0));
            assert_eq!(
                renderer.backend().cursor_positions.last().copied(),
                stable,
                "kernel cursor walk must not move the presented cursor"
            );
            assert_eq!(
                renderer.presented_cursor_cell(),
                Some((0, 4)),
                "presented-cursor probe must report the frozen cell"
            );
        }
    }

    #[test]
    fn presentation_cursor_release_moves_once_and_repaints_old_cell() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        renderer.set_presentation_cursor_suppressed(true);
        term.feed(b"\x1b[3;3H");
        renderer.backend_mut().last_drawn.clear();

        renderer.set_presentation_cursor_suppressed(false);
        assert!(renderer.tick(&term, None, 500.0));
        assert_eq!(
            renderer.backend().cursor_positions,
            vec![(0, 4), (2, 2)],
            "release must paint the final cursor exactly once"
        );
        let mut redrawn = renderer.backend().last_drawn.clone();
        redrawn.sort_unstable();
        redrawn.dedup();
        assert_eq!(
            redrawn,
            vec![0, 2],
            "release must repaint both old and final cursor rows"
        );
    }

    #[test]
    fn presentation_cursor_freeze_does_not_revive_hidden_or_unfocused_cursor() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));

        renderer.set_presentation_cursor_suppressed(true);
        term.feed(b"\x1b[?25l");
        assert!(renderer.tick(&term, None, 500.0));
        assert_eq!(renderer.backend().cursor_positions, vec![(0, 4)]);

        renderer.set_presentation_cursor_suppressed(false);
        assert!(!renderer.tick(&term, None, 500.0));
        assert_eq!(renderer.backend().cursor_positions, vec![(0, 4)]);

        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        renderer.set_presentation_cursor_suppressed(true);
        renderer.set_focused(false);
        term.feed(b"\x1b[2;2HX");
        assert!(renderer.tick(&term, None, 500.0));
        assert_eq!(renderer.backend().cursor_positions, vec![(0, 4)]);
    }

    #[test]
    fn presentation_cursor_freeze_preserves_blink_at_stable_position() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        renderer.set_presentation_cursor_suppressed(true);
        term.feed(b"\x1b[2;2HX");
        assert!(renderer.tick(&term, None, 500.0));
        let drawn_while_on = renderer.backend().cursors;

        assert_eq!(
            renderer.next_blink_deadline_ms(&term, 1000.0),
            500.0,
            "freeze must not disable the real blink deadline"
        );
        assert!(renderer.is_dirty(&term, None, 1000.0));
        assert!(renderer.tick(&term, None, 1000.0));
        assert_eq!(renderer.backend().cursors, drawn_while_on);

        assert!(renderer.is_dirty(&term, None, 1500.0));
        assert!(renderer.tick(&term, None, 1500.0));
        assert_eq!(renderer.backend().cursors, drawn_while_on + 1);
        assert_eq!(
            renderer.backend().cursor_positions.last().copied(),
            Some((0, 4))
        );
    }

    #[test]
    fn presentation_cursor_freeze_captures_cell_when_armed_on_blink_off() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        assert!(renderer.tick(&term, None, 1000.0));
        let cursors_after_off = renderer.backend().cursors;
        assert_eq!(
            renderer.backend().cursor_positions.last().copied(),
            Some((0, 4))
        );

        renderer.set_presentation_cursor_suppressed(true);
        term.feed(b"\x1b[2;2HX");
        assert!(renderer.tick(&term, None, 1500.0));
        assert!(
            !renderer.backend().last_drawn.is_empty(),
            "grid changes must paint while the cursor is frozen"
        );
        assert_eq!(
            renderer.backend().cursors,
            cursors_after_off + 1,
            "blink-on during freeze must restore the last presented cell, not hide"
        );
        assert_eq!(
            renderer.backend().cursor_positions.last().copied(),
            Some((0, 4)),
            "arming freeze on a blink-off frame must not follow the kernel walk"
        );
    }

    #[test]
    fn presentation_cursor_freeze_survives_viewport_grow() {
        let mut term = Terminal::new(4, 12, 0);
        term.feed(b"seed");
        let mut renderer = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(renderer.tick(&term, None, 500.0));
        renderer.set_presentation_cursor_suppressed(true);
        term.resize(8, 12);
        term.feed(b"\x1b[3;3HY");
        assert!(renderer.tick(&term, None, 500.0));
        assert!(
            renderer.backend().last_drawn.len() >= 8,
            "grown grid must paint immediately under a frozen cursor"
        );
        assert_eq!(
            renderer.backend().cursor_positions.last().copied(),
            Some((0, 4)),
            "viewport grow must not hide or move the frozen cursor"
        );
    }

    #[test]
    fn fullscreen_rewrite_with_no_cell_delta_skips_clear() {
        let rows = 6usize;
        let cols = 16usize;
        let mut term = Terminal::new(rows, cols, 0);
        term.feed(b"\x1b[?25l");
        // Alt-screen enter (Codex/ratatui common) + frame.
        term.feed(b"\x1b[?1049h");
        feed_ratatui_frame(&mut term, "Z");

        let mut r = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(r.tick(&term, None, 0.0));
        let clears_seed = r.backend().clears;

        // Full-screen-style rewrite: home + ED + same content (net zero cell delta).
        let mut out = Vec::new();
        out.extend_from_slice(b"\x1b[?2026h\x1b[H\x1b[2J\x1b[H");
        for row in 0..rows {
            let line = format!("Z{row:02}{}", " ".repeat(cols));
            let line: String = line.chars().take(cols).collect();
            out.extend_from_slice(line.as_bytes());
            if row + 1 < rows {
                out.extend_from_slice(b"\r\n");
            }
        }
        out.extend_from_slice(b"\x1b[?2026l");
        term.feed(&out);

        let drew = r.tick(&term, None, 0.0);
        assert!(
            !drew,
            "net-zero cell delta after fullscreen-style rewrite must not paint"
        );
        assert_eq!(
            r.backend().clears,
            clears_seed,
            "fullscreen-style rewrite with equal settled cells must not force whole-viewport clear"
        );
    }

    #[test]
    fn absolute_csi_inline_tui_does_not_force_full_redraw() {
        // CHA/CUP trip note_absolute_positioning → is_inline_tui_active_at,
        // which historically forced full_redraw_pending every tick.
        let rows = 5usize;
        let cols = 12usize;
        let mut term = Terminal::new(rows, cols, 0);
        term.feed(b"\x1b[?25l");
        feed_ratatui_frame(&mut term, "Q");
        assert!(
            term.grid()
                .is_inline_tui_active_at(crate::term::clock::now_ms(), false)
                || term.grid().is_inline_tui_active_at(
                    crate::term::clock::now_ms(),
                    term.modes().cursor_visible
                ),
            "fixture must engage absolute-positioning heuristic"
        );

        let mut r = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(r.tick(&term, None, 0.0));
        let clears_seed = r.backend().clears;
        // Re-emit absolute CSI without changing cells.
        term.feed(b"\x1b[H\x1b[G");
        assert!(!r.tick(&term, None, 0.0));
        assert_eq!(r.backend().clears, clears_seed);
    }

    /// Split remounts a sibling; that must not treat the already-painted
    /// pane as a whole-viewport clear when its row revisions are unchanged.
    #[test]
    fn sibling_split_does_not_full_clear_stable_pane() {
        let rows = 6usize;
        let cols = 16usize;
        let mut term_a = Terminal::new(rows, cols, 0);
        term_a.feed(b"\x1b[?25l");
        feed_ratatui_frame(&mut term_a, "A");

        let mut pane_a = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(pane_a.tick(&term_a, None, 0.0), "pane A seed frame");
        let clears_a = pane_a.backend().clears;
        let frames_a = pane_a.backend().frames;
        assert!(clears_a >= 1);

        let mut term_b = Terminal::new(rows, cols, 0);
        term_b.feed(b"\x1b[?25l");
        feed_ratatui_frame(&mut term_b, "B");
        let mut pane_b = Renderer::new(
            RecordingBackend::default(),
            metrics(),
            Theme::default_dark(),
        );
        assert!(pane_b.tick(&term_b, None, 0.0), "new split pane paints");
        assert!(pane_b.backend().clears >= 1);

        feed_ratatui_frame(&mut term_a, "A");
        let drew_a = pane_a.tick(&term_a, None, 0.0);
        assert!(!drew_a, "stable sibling must not repaint after split");
        assert_eq!(
            pane_a.backend().clears,
            clears_a,
            "split must not whole-viewport clear the previous pane when revisions are stable"
        );
        assert_eq!(pane_a.backend().frames, frames_a);
        assert!(!pane_a.is_dirty(&term_a, None, 0.0));
    }
}
