//! Per-pane parser engine (P3.3, 2026-05-20).
//!
//! Owns a `ridge_term::Terminal` (parser + grid + modes) per pane on the
//! Rust side, and turns every `feed(bytes)` call into a `DeltaFrame` of
//! incremental grid changes for the frontend. This is the centerpiece of
//! P3 — once the per-pane Tokio task feeding PTY bytes through here is
//! wired (P3.4 + lib.rs glue), the wasm bundle no longer has to run the
//! full VTE state machine on the JS main thread; it only has to apply
//! the diff its bigger sibling already computed.
//!
//! Diff strategy (first cut)
//! -------------------------
//! Keep one snapshot per pane: `Vec<Vec<DeltaCell>>` mirroring the live
//! visible grid (NOT scrollback). After every `feed()`:
//!   1. Compare each new row to the snapshot row.
//!   2. If different, emit `GridDelta::Cells { row, col: 0, cells: full_row }`
//!      and update the snapshot row to match.
//!   3. Compare cursor (row, col, visible, blink, shape); emit `Cursor` if
//!      anything changed.
//!   4. Compare `is_alt_screen` before/after; emit `ScreenSwitch` on flip.
//!   5. Drain `take_pending_events()` from the kernel and forward
//!      Title / Cwd / Bell as their `GridDelta` cousins.
//!
//! Future optimization (deferred):
//!   * Column-range diff inside changed rows (currently emits the whole
//!     row, which is correct but wastes IPC bytes for the common case of
//!     "one new char appeared at the cursor").
//!   * Scrollback append tracking (`ScrollbackAppend` variant). Today,
//!     once scrollback grows the frontend can pull it lazily via the
//!     existing `get_pane_scrollback_before` bridge; only live-grid
//!     deltas are emitted here.
//!   * Mode-flip emission (`ModeChange` variant) — `Modes` doesn't
//!     expose a per-mode diff API yet, so we skip mode deltas in v1.
//!     The cursor delta already carries visibility/blink/shape which
//!     is the user-visible subset that matters for rendering.
//!
//! Notes on this commit
//! --------------------
//! The engine compiles and is unit-tested but **not wired into
//! `pty.rs`** yet — that's P3.4's job (gated by a frontend
//! `Settings.parserBackend = 'rust'` flag so we can roll out without
//! removing the wasm parser path). Adding the engine now keeps that PR
//! small and lets the diff logic ship + accrete tests independently of
//! the IPC plumbing.

// P3.8 (2026-05-20): `PaneParser` is now wired into the main event loop
// — `lib.rs` calls `feed_and_diff` for every `GlobalEvent::PtyOutput`
// when the per-pane `delta_mode` AtomicBool is set. Methods that
// remain dead-code (e.g. resize until P3.9.r wires it) keep targeted
// #[allow] annotations at their definition site.

use ridge_term::term::delta::{CursorShape as DeltaCursorShape, DeltaCell, DeltaFrame, GridDelta};
use ridge_term::term::modes::{CursorShape as KernelCursorShape, Modes};
use ridge_term::term::terminal::{KernelEvent, Terminal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorSnap {
    row: u16,
    col: u16,
    visible: bool,
    blink: bool,
    shape: DeltaCursorShape,
}

/// Per-pane parser. Single-threaded; the caller (P3.4 plumbing) parks
/// one of these inside the Tokio task that owns the PTY reader for that
/// pane and calls `feed_and_diff` on every byte chunk.
pub struct PaneParser {
    terminal: Terminal,
    /// Last-emitted view of the visible grid, row-major. `rows × cols`,
    /// resized in lockstep with `Terminal::resize`. Reads from
    /// `terminal.grid().row(r)`; writes happen here.
    snapshot: Vec<Vec<DeltaCell>>,
    /// Cursor as we last reported it. `None` only on the very first
    /// feed — forces a `Cursor` delta in the initial frame.
    cursor: Option<CursorSnap>,
    /// alt-screen flag as we last reported it. Matches
    /// `Terminal::grid().is_alt_screen()`. None on first feed.
    is_alt: Option<bool>,
    /// P3.11 — `Terminal::scrollback_len()` at the time of the last
    /// diff. Used together with `last_scrollback_evictions` to compute
    /// how many fresh rows entered scrollback since the last frame.
    last_scrollback_len: usize,
    /// P3.11 — `Terminal::scrollback_eviction_count()` at the time of
    /// the last diff. Combined with `last_scrollback_len` we can tell
    /// "scrollback grew by N" from "scrollback grew but K oldest rows
    /// got evicted" (capacity rollover).
    last_scrollback_evictions: u64,
    /// P3.12 — `Terminal::modes()` snapshot from the previous diff.
    /// `None` only before the first frame; once set, `Modes::diff` runs
    /// against it on every frame and emits one `GridDelta::ModeChange`
    /// per flipped field. Excludes cursor visibility / blink / shape
    /// because those flow through `GridDelta::Cursor` already.
    last_modes: Option<Modes>,
    /// Monotonic per-pane sequence; bumped on every emitted frame.
    /// Frontend logs a warning on gaps. Resets to 0 on `new`.
    pane_seq: u64,
    /// §6 — latest OSC 0/2 window title seen on this pane's stream. Mirrors
    /// what the desktop surfaces into `paneOscTitleStore`; the remote
    /// `list-panes` reads it so remote terminal names match the desktop pane
    /// header's variable title. `None` until the program sets a title.
    last_title: Option<String>,
}

impl PaneParser {
    /// `scrollback_lines` is the same dimension passed to the wasm
    /// `TerminalKernel::new` — 5_000 in the default JS path.
    pub fn new(rows: u16, cols: u16, scrollback_lines: usize) -> Self {
        let terminal = Terminal::new(rows as usize, cols as usize, scrollback_lines);
        let snapshot = vec![vec![DeltaCell::blank(); cols as usize]; rows as usize];
        Self {
            terminal,
            snapshot,
            cursor: None,
            is_alt: None,
            last_scrollback_len: 0,
            last_scrollback_evictions: 0,
            last_modes: None,
            pane_seq: 0,
            last_title: None,
        }
    }

    /// §6 — latest OSC window title seen on this pane (OSC 0/2), or `None` if
    /// the program never set one. Used by the remote `list-panes` so remote
    /// terminal names match the desktop pane header's variable title.
    pub fn title(&self) -> Option<String> {
        self.last_title.clone()
    }

    /// P3.9.r will use this to report current dimensions back to the
    /// `resize_pane_parser` Tauri command for the idempotent guard
    /// (skip work if the requested size matches). Test-only consumer
    /// keeps the symbol live for now.
    #[allow(dead_code)]
    pub fn rows(&self) -> u16 {
        self.terminal.rows() as u16
    }

    /// See `rows()` — P3.9.r idempotent guard counterpart.
    #[allow(dead_code)]
    pub fn cols(&self) -> u16 {
        self.terminal.cols() as u16
    }

    /// §resize-flag-authority (2026-06-16) — the backend parser is the ONLY
    /// component that sees raw VT bytes, so it is the authoritative source for
    /// the alt-screen / inline-TUI state that `resize_pane` uses to pick the
    /// wipe-before-SIGWINCH ordering and the ConPTY silence-window skip. The
    /// frontend mirror (`JsTerminal`) is delta-only — it never records the
    /// absolute-positioning CSIs the inline-TUI heuristic keys off — so its
    /// `isInlineTuiMode()` is structurally always-false in the (now sole)
    /// delta mode. Relying on the frontend flag left the §A.3 / §resize-order
    /// ordering DISENGAGED for real inline TUIs (Claude Code without
    /// fullscreen / NO_FLICKER): the PTY resize fired before the parser wipe
    /// and the silence window swallowed the redraw. Query these directly so
    /// the ordering matches the wipe the parser actually performs.
    pub fn is_alt_screen(&self) -> bool {
        self.terminal.is_alt_screen()
    }

    /// Authoritative inline-TUI heuristic snapshot for the RESIZE decision —
    /// see `is_alt_screen`. Uses the sticky-aware variant so an idle inline TUI
    /// (default Claude at its prompt, all live signals decayed) is still
    /// classified for the wipe-before-SIGWINCH ordering. `now_ms` is the
    /// caller's wall-clock so the decay window matches the resize clock.
    pub fn is_inline_tui_resize_at(&self, now_ms: i64) -> bool {
        self.terminal.is_inline_tui_resize_at(now_ms)
    }

    /// Feed PTY bytes and return the resulting delta frame.
    ///
    /// The frame's `pane_seq` is the new value (post-increment from the
    /// previous one). If the feed produced no visible change (which can
    /// happen for query bytes like DSR `\x1b[5n` that only mutate
    /// `pending_response`) the returned frame's `deltas` is empty —
    /// callers may skip emitting it over IPC to save bandwidth.
    pub fn feed_and_diff(&mut self, bytes: &[u8]) -> DeltaFrame {
        self.terminal.feed(bytes);
        self.diff_into_frame()
    }

    /// Drain accumulated parser responses (DSR / DA replies) that must
    /// be written BACK to the PTY. The wiring layer should pump these
    /// into the pty writer immediately after each `feed_and_diff`.
    pub fn take_pending_response(&mut self) -> Vec<u8> {
        self.terminal.take_pending_response()
    }

    /// P3.9 (2026-05-20) — clear the diff baseline so the next
    /// `feed_and_diff` call emits a complete reframe (ScreenSwitch +
    /// Cursor + every dirty row as Cells). Used by
    /// `set_pane_delta_mode` when flipping false → true: the front-end
    /// mirror just enabled rust-parser mode and may have arbitrary
    /// stale state from an earlier wasm-parser session; sending a full
    /// reframe immediately bootstraps it to a known-good state without
    /// requiring the user to scrollback or re-input.
    ///
    /// Does NOT touch the underlying `Terminal` state — just the diff
    /// snapshot. The next visible frame is identical content-wise to
    /// what was on screen before; only the wire payload is larger.
    ///
    /// **§5 — mobile PaneParser bootstrap.** Use this for per-client
    /// parsers that need both visible grid AND scrollback in one frame.
    /// While `force_full_reframe` snaps the scrollback baseline to the
    /// current value (so old rows don't re-emit), this method resets it
    /// to zero so every scrollback row is included in the emitted frame.
    pub fn full_reframe_with_scrollback(&mut self) -> DeltaFrame {
        let cols = self.terminal.cols();
        let rows = self.terminal.rows();
        self.snapshot = vec![vec![DeltaCell::blank(); cols]; rows];
        self.cursor = None;
        self.is_alt = None;
        self.last_modes = None;
        // Force every scrollback row into the emitted frame by resetting
        // the diff baseline to zero.
        self.last_scrollback_len = 0;
        self.last_scrollback_evictions = 0;
        let mut frame = self.diff_into_frame();
        // Prepend an explicit Resize so a fresh subscriber whose kernel is
        // still at its construction size (24×80) resizes its grid to the
        // canonical dimensions BEFORE applying the bootstrap cells. Without
        // this, a remote client rendering the SHARED canonical grid (see
        // remote/server.rs subscribe-pane) could OOB on the first frame.
        frame.deltas.insert(
            0,
            GridDelta::Resize {
                rows: rows as u16,
                cols: cols as u16,
            },
        );
        frame
    }

    pub fn force_full_reframe(&mut self) {
        let rows = self.terminal.rows();
        let cols = self.terminal.cols();
        self.snapshot = vec![vec![DeltaCell::blank(); cols]; rows];
        self.cursor = None;
        self.is_alt = None;
        // P3.11 — scrollback growth baseline. The mirror's wasm
        // Terminal already has its current scrollback (the wasm-mode
        // feed loop maintained it); we MUST NOT re-emit those rows on
        // backend switch because the mirror would then double-push
        // them. Snap baseline to the current values; only growth from
        // now on emits ScrollbackAppend.
        self.last_scrollback_len = self.terminal.scrollback_len();
        self.last_scrollback_evictions = self.terminal.scrollback_eviction_count();
        // P3.12 — drop the mode snapshot so the next frame re-emits
        // every non-default mode. mirror's wasm Terminal already
        // tracks them from the wasm-mode session, so the resulting
        // ModeChange deltas are idempotent (rewriting the same value);
        // for a brand-new mirror they bootstrap from default → current.
        self.last_modes = None;
    }

    /// Resize the underlying grid and re-allocate the snapshot. Returns
    /// a delta frame that surfaces the `Resize` event AND any cell
    /// changes the resize itself caused (reflow can fill new rows or
    /// drop the right margin).
    ///
    /// Called by the `resize_pane_parser` Tauri command (P3.9.r) which
    /// routes fitPane through Rust in 'rust' mode to preserve the
    /// "parser resizes first, mirror follows via apply_delta(Resize)"
    /// invariant. No other call site today; tests keep the symbol live.
    #[allow(dead_code)]
    pub fn resize(&mut self, rows: u16, cols: u16) -> DeltaFrame {
        self.terminal.resize(rows as usize, cols as usize);
        // Snapshot must match the new grid dimensions BEFORE we diff —
        // otherwise the diff loop OOBs on `self.snapshot[r]` or compares
        // mismatched widths. Re-initialize blank; the next diff will
        // emit the actual new content as Cells deltas.
        self.snapshot = vec![vec![DeltaCell::blank(); cols as usize]; rows as usize];
        // Cursor / alt-state must also be re-emitted because the
        // frontend's mirror just resized too.
        self.cursor = None;
        self.is_alt = None;
        // P3.11 — `Grid::resize` runs reflow on primary which may
        // reshape scrollback (rows can shrink-with-wrap on column
        // change). The mirror's `Terminal::resize` runs the same
        // reflow against its own scrollback, so both sides end up at
        // an equivalent shape post-resize. We snap the baseline to
        // post-resize values so the next diff doesn't re-emit existing
        // scrollback rows as if they were freshly appended.
        self.last_scrollback_len = self.terminal.scrollback_len();
        self.last_scrollback_evictions = self.terminal.scrollback_eviction_count();

        let mut frame = self.diff_into_frame();
        // Prepend the explicit Resize so the frontend resizes its mirror
        // BEFORE applying any `Cells` deltas that reference the new
        // dimensions.
        frame.deltas.insert(0, GridDelta::Resize { rows, cols });
        frame
    }

    fn diff_into_frame(&mut self) -> DeltaFrame {
        let mut deltas: Vec<GridDelta> = Vec::new();

        // P3.10 — RIS observed since the last frame. Reset the mirror
        // first, then drop our diff baseline so the rest of this method
        // emits a full reframe (ScreenSwitch + Cursor + every dirty
        // row). The mirror's `apply_delta(Reset)` is symmetric — it
        // applies the same reset the kernel just applied inline. We
        // preserve scrollback on both sides (matches the kernel's RIS
        // semantics: Alacritty-style "keep history through stray RIS").
        if self.terminal.take_pending_reset() {
            deltas.push(GridDelta::Reset);
            let cols = self.terminal.cols();
            let rows = self.terminal.rows();
            self.snapshot = vec![vec![DeltaCell::blank(); cols]; rows];
            self.cursor = None;
            self.is_alt = None;
            // P3.12 — the parser's RIS handler set modes back to default;
            // the mirror's `apply_delta(Reset)` does the same. Drop the
            // mode snapshot so the next diff doesn't think a real flip
            // happened (kernel.modes went from "current" → "default" in
            // ONE step; last_modes carried the pre-RIS value).
            self.last_modes = Some(Modes::default());
        }

        // 1. Screen-switch (alt ↔ primary) — emit FIRST because the
        //    cells deltas that follow describe the now-active screen.
        let alt_now = self.terminal.grid().is_alt_screen();
        if self.is_alt != Some(alt_now) {
            deltas.push(GridDelta::ScreenSwitch { is_alt: alt_now });
            self.is_alt = Some(alt_now);
        }

        // 1b. Scrollback growth. Compare today's `scrollback_len()` +
        //     `scrollback_eviction_count()` to the snapshot from the
        //     last frame to compute how many fresh rows entered
        //     scrollback. The eviction counter advances when capacity
        //     rolls over (the oldest row gets dropped to make room),
        //     so a stable len plus a positive eviction delta still
        //     means "new rows came in" — they just displaced equal
        //     numbers of oldest ones. Mirror naturally evicts the
        //     equivalent rows on push because both sides have matched
        //     scrollback capacities (set by `Terminal::new`).
        let now_len = self.terminal.scrollback_len();
        let now_evictions = self.terminal.scrollback_eviction_count();
        let evicted_since = now_evictions.saturating_sub(self.last_scrollback_evictions);
        // After K evictions, the previously-counted rows shifted down
        // by K. last_logical_len is what scrollback_len() would have
        // been now if nothing new were added.
        let last_logical_len = self
            .last_scrollback_len
            .saturating_sub(evicted_since as usize);
        if now_len > last_logical_len {
            let new_n = now_len - last_logical_len;
            let mut lines: Vec<Vec<DeltaCell>> = Vec::with_capacity(new_n);
            let start = now_len - new_n;
            for i in start..now_len {
                if let Some(row) = self.terminal.grid().scrollback.get(i) {
                    let mut row_cells: Vec<DeltaCell> = Vec::with_capacity(row.cells.len());
                    for (ci, cell) in row.cells.iter().enumerate() {
                        let attrs = self.terminal.grid().attrs.get(cell.attr);
                        row_cells.push(DeltaCell {
                            ch: cell.ch,
                            fg: attrs.fg,
                            bg: attrs.bg,
                            flags: attrs.flags,
                            width: cell.width,
                            // §emoji-cluster — carry the multi-codepoint
                            // cluster so scrolled-back emoji keep ZWJ/skin/
                            // flag composition on the wasm mirror.
                            cluster: row.cluster_at(ci).map(|cs| cs.text.clone()),
                        });
                    }
                    lines.push(row_cells);
                }
            }
            if !lines.is_empty() {
                deltas.push(GridDelta::ScrollbackAppend { lines });
            }
        }
        self.last_scrollback_len = now_len;
        self.last_scrollback_evictions = now_evictions;

        // 2. Per-row diff. Resolve each cell via the live AttrTable so
        //    the comparison is stable across feed batches (an interned
        //    AttrId means nothing across grids).
        let rows = self.terminal.rows();
        let cols = self.terminal.cols();
        for r in 0..rows {
            let live_row = match self.terminal.grid().row(r) {
                Some(row) => row,
                None => continue,
            };
            // Build the "now" row as DeltaCells. Width matches the grid
            // (NOT live_row.cells.len() — those can differ briefly
            // during a resize, though resize() above resets the snapshot
            // to grid dims and clears cursor/alt to force a full reframe).
            let mut now_row: Vec<DeltaCell> = Vec::with_capacity(cols);
            for c in 0..cols {
                let cell = live_row.cells.get(c).copied().unwrap_or_default();
                let attrs = self.terminal.grid().attrs.get(cell.attr);
                now_row.push(DeltaCell {
                    ch: cell.ch,
                    fg: attrs.fg,
                    bg: attrs.bg,
                    flags: attrs.flags,
                    width: cell.width,
                    // §emoji-cluster — anchor cell carries the full grapheme
                    // cluster (👨‍👩‍👧 / 👍🏽 / 🇯🇵 / ❤️) so the wasm renderer
                    // paints the composed glyph instead of just `ch` (the
                    // base codepoint). The per-cell diff treats a changed
                    // cluster as a cell change (DeltaCell: PartialEq).
                    cluster: live_row.cluster_at(c).map(|cs| cs.text.clone()),
                });
            }
            // Snapshot row may be shorter than cols if we resized below
            // (impossible after `resize()` above but defensive). Pad.
            if let Some(snap_row) = self.snapshot.get_mut(r) {
                if snap_row.len() != cols {
                    snap_row.resize(cols, DeltaCell::blank());
                }
                // P3.13 — col-range diff. Find the smallest contiguous
                // span that contains every changed cell, and emit only
                // that slice. For the common "one new char at the
                // cursor" case the wire payload drops from `cols`
                // DeltaCells (~80 × 17 B = 1.3 KB raw → ~200 B postcard)
                // to a single DeltaCell (~17 B raw → ~10 B postcard).
                // Identical rows skip the emit entirely (first_diff is
                // None). Bookended changes still emit a single span
                // covering both — that's a feature, not a bug: a
                // tighter "two-span" diff would double the per-row
                // encoding overhead with diminishing returns.
                let mut first_diff: Option<usize> = None;
                let mut last_diff: usize = 0;
                for c in 0..cols {
                    if snap_row[c] != now_row[c] {
                        if first_diff.is_none() {
                            first_diff = Some(c);
                        }
                        last_diff = c;
                    }
                }
                if let Some(first) = first_diff {
                    let slice = now_row[first..=last_diff].to_vec();
                    deltas.push(GridDelta::Cells {
                        row: r as u16,
                        col: first as u16,
                        cells: slice,
                    });
                    // Write the new state into the snapshot for the
                    // changed range only — unchanged cells already
                    // match.
                    for c in first..=last_diff {
                        snap_row[c] = now_row[c].clone();
                    }
                }
            }
        }

        // 2b. Mode diff. Compare current `Modes` against the snapshot
        //     from the last frame and emit one ModeChange per flipped
        //     field. Cursor visibility / blink / shape are excluded
        //     here — the `Cursor` delta below carries those.
        let modes_now = *self.terminal.modes();
        let modes_prev = self.last_modes.unwrap_or_else(Modes::default);
        for (code, on) in modes_now.diff(&modes_prev) {
            deltas.push(GridDelta::ModeChange { mode: code, on });
        }
        self.last_modes = Some(modes_now);

        // 3. Cursor diff. Visibility / blink / shape live on `Modes`;
        //    position lives on `Grid::cursor()`. Composite into one
        //    snapshot so a blink change emits the same delta shape as
        //    a position change.
        let modes = *self.terminal.modes();
        let grid_cursor = *self.terminal.grid().cursor();
        let cursor_now = CursorSnap {
            row: grid_cursor.row as u16,
            col: grid_cursor.col as u16,
            visible: modes.cursor_visible,
            blink: modes.cursor_blink,
            shape: kernel_shape_to_delta(modes.cursor_shape),
        };
        if self.cursor != Some(cursor_now) {
            deltas.push(GridDelta::Cursor {
                row: cursor_now.row,
                col: cursor_now.col,
                visible: cursor_now.visible,
                blink: cursor_now.blink,
                shape: cursor_now.shape,
            });
            self.cursor = Some(cursor_now);
        }

        // 4. Kernel events (title / cwd / bell). IconNameChanged has no
        //    counterpart in GridDelta — drop it; the existing
        //    `PaneTitleChanged` path already covers OSC 0/2 which is
        //    what UIs surface.
        for ev in self.terminal.take_pending_events() {
            match ev {
                KernelEvent::TitleChanged(t) => {
                    self.last_title = Some(t.clone());
                    deltas.push(GridDelta::Title(t));
                }
                KernelEvent::CwdChanged(p) => deltas.push(GridDelta::Cwd(p)),
                KernelEvent::Bell => deltas.push(GridDelta::Bell),
                KernelEvent::IconNameChanged(_) => {}
            }
        }

        let frame = DeltaFrame::new(self.pane_seq, deltas);
        self.pane_seq = self.pane_seq.saturating_add(1);
        frame
    }
}

fn kernel_shape_to_delta(s: KernelCursorShape) -> DeltaCursorShape {
    match s {
        KernelCursorShape::Block => DeltaCursorShape::Block,
        KernelCursorShape::Underline => DeltaCursorShape::Underline,
        KernelCursorShape::Bar => DeltaCursorShape::Bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parser(rows: u16, cols: u16) -> PaneParser {
        PaneParser::new(rows, cols, 1_000)
    }

    #[test]
    fn new_parser_dimensions_match_constructor_args() {
        let p = make_parser(24, 80);
        assert_eq!(p.rows(), 24);
        assert_eq!(p.cols(), 80);
    }

    #[test]
    fn first_feed_emits_cursor_and_screen_switch() {
        // Even a no-op feed (empty bytes) must surface the initial
        // cursor + alt-screen state so a fresh frontend mirror has the
        // info it needs to draw anything.
        let mut p = make_parser(4, 10);
        let frame = p.feed_and_diff(b"");
        assert_eq!(frame.version, DeltaFrame::PROTOCOL_VERSION);
        assert_eq!(frame.pane_seq, 0);
        let has_screen = frame
            .deltas
            .iter()
            .any(|d| matches!(d, GridDelta::ScreenSwitch { is_alt: false }));
        let has_cursor = frame.deltas.iter().any(|d| {
            matches!(
                d,
                GridDelta::Cursor {
                    row: 0,
                    col: 0,
                    visible: true,
                    ..
                }
            )
        });
        assert!(has_screen, "missing ScreenSwitch in first frame");
        assert!(has_cursor, "missing Cursor in first frame");
    }

    #[test]
    fn printing_one_char_emits_one_changed_row() {
        let mut p = make_parser(3, 5);
        let _ = p.feed_and_diff(b"");
        let frame = p.feed_and_diff(b"A");
        let cell_deltas: Vec<_> = frame
            .deltas
            .iter()
            .filter_map(|d| match d {
                GridDelta::Cells { row, cells, .. } => Some((*row, cells.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            cell_deltas.len(),
            1,
            "exactly one row should be reported dirty"
        );
        let (row, cells) = &cell_deltas[0];
        assert_eq!(*row, 0);
        assert_eq!(cells[0].ch, 'A');
        let advanced = frame.deltas.iter().any(|d| {
            matches!(
                d,
                GridDelta::Cursor {
                    row: 0,
                    col: 1,
                    visible: true,
                    ..
                }
            )
        });
        assert!(advanced, "cursor should have advanced to col 1");
    }

    #[test]
    fn single_char_change_emits_one_cell_range() {
        // P3.13 — col-range diff. Print one char, then overwrite one
        // mid-row cell with a different char. The resulting Cells
        // delta must cover ONLY that cell (col + 1-element cells),
        // not the whole row.
        let mut p = make_parser(2, 10);
        let _ = p.feed_and_diff(b"hello");
        // Cursor is now at col 5. Move it back to col 2 and overwrite
        // the 'l' with 'X'. The diff should target col=2, len=1.
        let frame = p.feed_and_diff(b"\x1b[1;3HX");
        let cell_deltas: Vec<&GridDelta> = frame
            .deltas
            .iter()
            .filter(|d| matches!(d, GridDelta::Cells { .. }))
            .collect();
        assert_eq!(
            cell_deltas.len(),
            1,
            "exactly one Cells delta expected; got {:?}",
            frame.deltas
        );
        match cell_deltas[0] {
            GridDelta::Cells { row, col, cells } => {
                assert_eq!(*row, 0);
                assert_eq!(*col, 2, "diff must start at the column of the changed cell");
                assert_eq!(
                    cells.len(),
                    1,
                    "diff must contain ONE cell, not the whole row"
                );
                assert_eq!(cells[0].ch, 'X');
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn mid_line_change_emits_narrow_range_not_whole_row() {
        // Bookended change: write 'A' at col 2 and 'B' at col 5 on
        // an otherwise unchanged row. The single-range diff must
        // span col 2..=5 (4 cells), not the whole 10-cell row.
        let mut p = make_parser(2, 10);
        let _ = p.feed_and_diff(b"          "); // 10 spaces, prime snapshot
                                                // Move to (1,3) write 'A', move to (1,6) write 'B'.
        let frame = p.feed_and_diff(b"\x1b[1;3HA\x1b[1;6HB");
        let cell_deltas: Vec<&GridDelta> = frame
            .deltas
            .iter()
            .filter(|d| matches!(d, GridDelta::Cells { .. }))
            .collect();
        assert_eq!(
            cell_deltas.len(),
            1,
            "single contiguous span expected; got {:?}",
            frame.deltas
        );
        match cell_deltas[0] {
            GridDelta::Cells { row: _, col, cells } => {
                assert_eq!(*col, 2, "span starts at first changed col");
                assert_eq!(
                    cells.len(),
                    4,
                    "span ends at last changed col (3+1+1+1 = 4)"
                );
                assert_eq!(cells[0].ch, 'A');
                assert_eq!(cells[3].ch, 'B');
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn second_identical_state_emits_empty_delta() {
        let mut p = make_parser(3, 5);
        let _ = p.feed_and_diff(b"hi");
        let frame = p.feed_and_diff(b"");
        assert!(
            frame.deltas.is_empty(),
            "expected empty delta on no-change feed, got {:?}",
            frame.deltas
        );
    }

    #[test]
    fn pane_seq_increments_monotonically() {
        let mut p = make_parser(2, 3);
        let a = p.feed_and_diff(b"a");
        let b = p.feed_and_diff(b"b");
        let c = p.feed_and_diff(b"c");
        assert_eq!(a.pane_seq, 0);
        assert_eq!(b.pane_seq, 1);
        assert_eq!(c.pane_seq, 2);
    }

    #[test]
    fn alt_screen_toggle_emits_screen_switch() {
        let mut p = make_parser(4, 10);
        let _ = p.feed_and_diff(b"");
        let enter = p.feed_and_diff(b"\x1b[?1049h");
        assert!(
            enter
                .deltas
                .iter()
                .any(|d| matches!(d, GridDelta::ScreenSwitch { is_alt: true })),
            "expected ScreenSwitch(is_alt=true) on DECSET 1049; got {:?}",
            enter.deltas
        );
        let leave = p.feed_and_diff(b"\x1b[?1049l");
        assert!(
            leave
                .deltas
                .iter()
                .any(|d| matches!(d, GridDelta::ScreenSwitch { is_alt: false })),
            "expected ScreenSwitch(is_alt=false) on DECRST 1049; got {:?}",
            leave.deltas
        );
    }

    #[test]
    fn cursor_hide_show_emits_cursor_delta() {
        let mut p = make_parser(3, 5);
        let _ = p.feed_and_diff(b"");
        let hidden = p.feed_and_diff(b"\x1b[?25l");
        let saw_hidden = hidden
            .deltas
            .iter()
            .any(|d| matches!(d, GridDelta::Cursor { visible: false, .. }));
        assert!(
            saw_hidden,
            "expected Cursor(visible=false) on DECRST 25; got {:?}",
            hidden.deltas
        );
    }

    #[test]
    fn bracketed_paste_toggle_emits_modechange() {
        // `CSI ? 2004 h` enables bracketed paste; the producer must
        // emit a ModeChange so the mirror's Modes flags follow.
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"");
        // After first feed bracketed_paste was OFF (default).
        let frame = p.feed_and_diff(b"\x1b[?2004h");
        let saw_paste_on = frame.deltas.iter().any(|d| {
            matches!(
                d,
                GridDelta::ModeChange {
                    mode: 2004,
                    on: true
                }
            )
        });
        assert!(
            saw_paste_on,
            "expected ModeChange(2004, on=true) on ?2004h; got {:?}",
            frame.deltas,
        );
        let off_frame = p.feed_and_diff(b"\x1b[?2004l");
        assert!(
            off_frame.deltas.iter().any(|d| matches!(
                d,
                GridDelta::ModeChange {
                    mode: 2004,
                    on: false
                }
            )),
            "expected ModeChange(2004, on=false) on ?2004l; got {:?}",
            off_frame.deltas,
        );
    }

    #[test]
    fn mouse_mode_toggle_round_trip_through_apply_delta() {
        // Producer emits ModeChange for mouse modes; running the same
        // delta through a mirror Terminal must drive its Modes flags
        // to the same value.
        use ridge_term::term::terminal::Terminal;
        let mut producer = make_parser(2, 5);
        let mut mirror = Terminal::new(2, 5, 100);
        let _ = producer.feed_and_diff(b"");
        let frame = producer.feed_and_diff(b"\x1b[?1006h\x1b[?1000h"); // SGR + normal mouse
        for d in &frame.deltas {
            mirror.apply_delta(d);
        }
        assert!(mirror.modes().mouse_sgr, "mirror must have mouse_sgr=true");
        assert!(
            mirror.modes().mouse_normal,
            "mirror must have mouse_normal=true"
        );
    }

    #[test]
    fn osc_title_emits_title_delta() {
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"");
        // §6: no title until the program sets one.
        assert_eq!(p.title(), None);
        let frame = p.feed_and_diff(b"\x1b]0;hello\x07");
        let saw = frame
            .deltas
            .iter()
            .any(|d| matches!(d, GridDelta::Title(t) if t == "hello"));
        assert!(saw, "expected Title('hello'); got {:?}", frame.deltas);
        // §6: the latest title is retained for the remote list-panes lookup.
        assert_eq!(p.title().as_deref(), Some("hello"));
        // OSC 2 (window title only) updates it too.
        let _ = p.feed_and_diff(b"\x1b]2;world\x07");
        assert_eq!(p.title().as_deref(), Some("world"));
    }

    #[test]
    fn resize_emits_resize_first_then_reframes_content() {
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"X");
        let frame = p.resize(3, 6);
        let first = frame
            .deltas
            .first()
            .expect("resize frame must be non-empty");
        assert!(
            matches!(first, GridDelta::Resize { rows: 3, cols: 6 }),
            "expected first delta to be Resize{{rows:3,cols:6}}; got {:?}",
            first
        );
    }

    #[test]
    fn scrollback_growth_emits_scrollback_append() {
        // Push 3 lines into a 2-row viewport so one row spills into
        // scrollback. The producer must emit `ScrollbackAppend` with
        // exactly that one new line.
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"");
        let frame = p.feed_and_diff(b"AB\r\nCD\r\nEF");
        let appends: Vec<&Vec<Vec<DeltaCell>>> = frame
            .deltas
            .iter()
            .filter_map(|d| match d {
                GridDelta::ScrollbackAppend { lines } => Some(lines),
                _ => None,
            })
            .collect();
        assert_eq!(
            appends.len(),
            1,
            "expected exactly one ScrollbackAppend, got {:?}",
            frame.deltas,
        );
        let lines = appends[0];
        // 'AB' is the first row pushed into scrollback.
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0][0].ch, 'A');
        assert_eq!(lines[0][1].ch, 'B');
    }

    #[test]
    fn no_growth_emits_no_scrollback_append() {
        // Feed that produces no scrollback push (in-viewport edits)
        // must NOT include ScrollbackAppend.
        let mut p = make_parser(3, 5);
        let _ = p.feed_and_diff(b"hi");
        let frame = p.feed_and_diff(b"!");
        assert!(
            !frame
                .deltas
                .iter()
                .any(|d| matches!(d, GridDelta::ScrollbackAppend { .. })),
            "in-viewport edit must not emit ScrollbackAppend; got {:?}",
            frame.deltas,
        );
    }

    #[test]
    fn ris_emits_reset_then_reframes() {
        // RIS (`ESC c`) at the kernel level wipes everything; the
        // producer must flag it so the mirror gets a `GridDelta::Reset`
        // ahead of the post-reset Cells deltas. After the reset the
        // producer's snapshot is blank, so the next visible content
        // emits a full reframe rather than a no-op diff.
        let mut p = make_parser(3, 5);
        // Prime the snapshot with some content so the next reset has
        // something to "diff away from".
        let _ = p.feed_and_diff(b"AB");
        let frame = p.feed_and_diff(b"\x1bc");
        // Reset must be the first delta — the mirror needs to apply
        // it before any subsequent Cells that describe the now-blank
        // post-reset grid.
        assert!(
            matches!(frame.deltas.first(), Some(GridDelta::Reset)),
            "expected Reset to lead post-RIS frame; got {:?}",
            frame.deltas,
        );
        // The reset is the BEGINNING of a full reframe — ScreenSwitch
        // and Cursor should follow because the snapshot got cleared.
        assert!(
            frame
                .deltas
                .iter()
                .any(|d| matches!(d, GridDelta::ScreenSwitch { is_alt: false })),
            "post-Reset frame must include ScreenSwitch reframe; got {:?}",
            frame.deltas,
        );
        // A second feed with no input produces no further Reset.
        let next = p.feed_and_diff(b"");
        assert!(
            !next.deltas.iter().any(|d| matches!(d, GridDelta::Reset)),
            "Reset must not repeat after take_pending_reset drained it; got {:?}",
            next.deltas,
        );
    }

    #[test]
    fn bell_byte_emits_bell_delta() {
        let mut p = make_parser(2, 3);
        let _ = p.feed_and_diff(b"");
        let frame = p.feed_and_diff(b"\x07");
        assert!(
            frame.deltas.iter().any(|d| matches!(d, GridDelta::Bell)),
            "expected Bell delta; got {:?}",
            frame.deltas
        );
    }

    /// P3.4 round-trip: prove producer (`PaneParser`) + consumer
    /// (`Terminal::apply_delta`) are symmetric. Feed identical byte
    /// streams through (a) a baseline Terminal that runs vte locally
    /// — the existing wasm path — and (b) a PaneParser whose frames
    /// are applied to a fresh "mirror" Terminal. After every chunk,
    /// the mirror's visible grid must match the baseline's.
    #[test]
    fn round_trip_matches_direct_feed() {
        use ridge_term::term::terminal::Terminal;

        let chunks: &[&[u8]] = &[
            b"hello world\r\n",
            b"line two\r\n",
            b"\x1b[1;31mRED\x1b[0m bold reset\r\n",
            // Switch to alt screen, draw something, switch back.
            b"\x1b[?1049h\x1b[2J\x1b[Halt screen content\r\n",
            b"\x1b[?1049l",
            // Move cursor + overwrite a cell.
            b"\x1b[1;1H@",
        ];

        let rows: u16 = 6;
        let cols: u16 = 30;
        let scrollback = 1_000;

        let mut baseline = Terminal::new(rows as usize, cols as usize, scrollback);
        let mut mirror = Terminal::new(rows as usize, cols as usize, scrollback);
        let mut producer = PaneParser::new(rows, cols, scrollback);

        for chunk in chunks {
            baseline.feed(chunk);
            let frame = producer.feed_and_diff(chunk);
            mirror
                .apply_frame(&frame)
                .expect("mirror must accept producer-emitted frame");
            // Drain query responses so they don't bleed into the next
            // chunk's diff comparison (baseline keeps them too;
            // identical churn cancels).
            let _ = baseline.take_pending_response();
            let _ = mirror.take_pending_response();

            // Visible-grid equality: walk row-by-row and compare
            // resolved DeltaCells (char + concrete attrs + width).
            // Comparing raw `Cell` would require AttrId stability
            // across two grids, which isn't guaranteed.
            for r in 0..rows as usize {
                let base_row = baseline.grid().row(r).expect("baseline row");
                let mir_row = mirror.grid().row(r).expect("mirror row");
                for c in 0..cols as usize {
                    let b_cell = base_row.cells.get(c).copied().unwrap_or_default();
                    let m_cell = mir_row.cells.get(c).copied().unwrap_or_default();
                    let b_attrs = baseline.grid().attrs.get(b_cell.attr);
                    let m_attrs = mirror.grid().attrs.get(m_cell.attr);
                    assert_eq!(
                        (
                            b_cell.ch,
                            b_attrs.fg,
                            b_attrs.bg,
                            b_attrs.flags,
                            b_cell.width
                        ),
                        (
                            m_cell.ch,
                            m_attrs.fg,
                            m_attrs.bg,
                            m_attrs.flags,
                            m_cell.width
                        ),
                        "cell mismatch at row={} col={} after chunk {:?}",
                        r,
                        c,
                        std::str::from_utf8(chunk).unwrap_or("<non-utf8>"),
                    );
                }
            }
            // Cursor + alt-screen also must match.
            assert_eq!(
                baseline.grid().cursor().row,
                mirror.grid().cursor().row,
                "cursor row mismatch after chunk {:?}",
                std::str::from_utf8(chunk).unwrap_or("<non-utf8>")
            );
            assert_eq!(
                baseline.grid().cursor().col,
                mirror.grid().cursor().col,
                "cursor col mismatch after chunk {:?}",
                std::str::from_utf8(chunk).unwrap_or("<non-utf8>")
            );
            assert_eq!(
                baseline.grid().is_alt_screen(),
                mirror.grid().is_alt_screen(),
                "alt-screen mismatch after chunk {:?}",
                std::str::from_utf8(chunk).unwrap_or("<non-utf8>")
            );
        }
    }
}
