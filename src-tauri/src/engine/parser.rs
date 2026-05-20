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

// P3.3 (2026-05-20): `PaneParser` is intentionally unused at build time
// — the production path still routes PTY bytes through wasm. The
// `dead_code` allow stays until P3.4 wires this into `engine::pty` and
// the global event loop; revisit then.
#![allow(dead_code)]

use ridge_term::term::delta::{CursorShape as DeltaCursorShape, DeltaCell, DeltaFrame, GridDelta};
use ridge_term::term::modes::CursorShape as KernelCursorShape;
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
    /// Monotonic per-pane sequence; bumped on every emitted frame.
    /// Frontend logs a warning on gaps. Resets to 0 on `new`.
    pane_seq: u64,
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
            pane_seq: 0,
        }
    }

    pub fn rows(&self) -> u16 {
        self.terminal.rows() as u16
    }

    pub fn cols(&self) -> u16 {
        self.terminal.cols() as u16
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

    /// Resize the underlying grid and re-allocate the snapshot. Returns
    /// a delta frame that surfaces the `Resize` event AND any cell
    /// changes the resize itself caused (reflow can fill new rows or
    /// drop the right margin).
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

        let mut frame = self.diff_into_frame();
        // Prepend the explicit Resize so the frontend resizes its mirror
        // BEFORE applying any `Cells` deltas that reference the new
        // dimensions.
        frame.deltas.insert(0, GridDelta::Resize { rows, cols });
        frame
    }

    fn diff_into_frame(&mut self) -> DeltaFrame {
        let mut deltas: Vec<GridDelta> = Vec::new();

        // 1. Screen-switch (alt ↔ primary) — emit FIRST because the
        //    cells deltas that follow describe the now-active screen.
        let alt_now = self.terminal.grid().is_alt_screen();
        if self.is_alt != Some(alt_now) {
            deltas.push(GridDelta::ScreenSwitch { is_alt: alt_now });
            self.is_alt = Some(alt_now);
        }

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
                });
            }
            // Snapshot row may be shorter than cols if we resized below
            // (impossible after `resize()` above but defensive). Pad.
            if let Some(snap_row) = self.snapshot.get_mut(r) {
                if snap_row.len() != cols {
                    snap_row.resize(cols, DeltaCell::blank());
                }
                if *snap_row != now_row {
                    deltas.push(GridDelta::Cells {
                        row: r as u16,
                        col: 0,
                        cells: now_row.clone(),
                    });
                    *snap_row = now_row;
                }
            }
        }

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
                KernelEvent::TitleChanged(t) => deltas.push(GridDelta::Title(t)),
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
        let saw_hidden = hidden.deltas.iter().any(|d| {
            matches!(
                d,
                GridDelta::Cursor {
                    visible: false,
                    ..
                }
            )
        });
        assert!(
            saw_hidden,
            "expected Cursor(visible=false) on DECRST 25; got {:?}",
            hidden.deltas
        );
    }

    #[test]
    fn osc_title_emits_title_delta() {
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"");
        let frame = p.feed_and_diff(b"\x1b]0;hello\x07");
        let saw = frame
            .deltas
            .iter()
            .any(|d| matches!(d, GridDelta::Title(t) if t == "hello"));
        assert!(saw, "expected Title('hello'); got {:?}", frame.deltas);
    }

    #[test]
    fn resize_emits_resize_first_then_reframes_content() {
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"X");
        let frame = p.resize(3, 6);
        let first = frame.deltas.first().expect("resize frame must be non-empty");
        assert!(
            matches!(
                first,
                GridDelta::Resize {
                    rows: 3,
                    cols: 6,
                }
            ),
            "expected first delta to be Resize{{rows:3,cols:6}}; got {:?}",
            first
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
}
