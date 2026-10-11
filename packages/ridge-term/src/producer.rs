//! Shared terminal producer: one VT parse, dirty-row deltas and atomic packet metadata.
//! Native adapters provide lifetime identity and transport; they do not own terminal semantics.

use crate::term::cell::Row;
use crate::term::delta::{
    CursorShape as DeltaCursorShape, DeltaCell, DeltaFrame, DeltaLine, GridDelta,
};
use crate::term::grid::ScrollOp;
use crate::term::modes::{CursorShape as KernelCursorShape, Modes};
use crate::term::terminal::{KernelEvent, Terminal};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorSnap {
    row: u16,
    col: u16,
    visible: bool,
    blink: bool,
    shape: DeltaCursorShape,
}

/// Collapse consecutive equivalent scrolls into one viewport move. Parser
/// frames are atomic: subsequent cell deltas describe the final grid, so a
/// run of bottom-margin LFs never needs one IPC record per physical line.
fn coalesce_scroll_ops(ops: Vec<ScrollOp>) -> Vec<ScrollOp> {
    let mut compact: Vec<ScrollOp> = Vec::with_capacity(ops.len().min(4));
    for op in ops {
        if op.top > op.bottom || op.count == 0 {
            continue;
        }
        let mut remaining = op.count;
        while remaining > 0 {
            if let Some(previous) = compact.last_mut().filter(|previous| {
                previous.top == op.top
                    && previous.bottom == op.bottom
                    && previous.up == op.up
                    && previous.count < u16::MAX as usize
            }) {
                let added = remaining.min(u16::MAX as usize - previous.count);
                previous.count += added;
                remaining -= added;
            } else {
                let count = remaining.min(u16::MAX as usize);
                compact.push(ScrollOp { count, ..op });
                remaining -= count;
            }
        }
    }
    compact
}

/// Per-pane parser. Single-threaded; the caller (P3.4 plumbing) parks
/// one of these inside the Tokio task that owns the PTY reader for that
/// pane and calls `feed_and_diff` on every byte chunk.
pub struct TerminalProducer {
    terminal: Terminal,
    terminal_epoch: String,
    semantic_metadata: Option<[crate::semantic::ScreenMetadata; 2]>,
    semantic_history: Option<crate::terminal_protocol::HistoryRange>,
    /// Last-emitted view of the visible grid, row-major. `rows × cols`,
    /// resized in lockstep with `Terminal::resize`. Reads from
    /// `terminal.grid().row(r)`; writes happen here.
    snapshot: Vec<Vec<DeltaCell>>,
    /// Per-live-row soft-wrap metadata paired with `snapshot`.
    snapshot_wrapped: Vec<bool>,
    /// Grid row revisions paired with `snapshot`. `u64::MAX` means the row
    /// has never been emitted and must be materialized once.
    snapshot_row_revisions: Vec<u64>,
    /// Exact terminal rows paired with `snapshot_row_revisions`. A TUI may
    /// clear and rewrite a visually identical row in one PTY batch; its
    /// revision changes, but materializing `DeltaCell`s again would be pure
    /// parser work with no frame for the browser to consume.
    snapshot_visual_rows: Vec<Option<Row>>,
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
    /// Monotonic physical-clear snapshot. Length alone cannot distinguish a
    /// real clear from ordinary resize/reflow shrink, nor report clearing an
    /// already-empty frontend ring while backend raw history still exists.
    last_scrollback_clears: u64,
    /// P3.12 — `Terminal::modes()` snapshot from the previous diff.
    /// `None` only before the first frame; once set, `Modes::diff` runs
    /// against it on every frame and emits one `GridDelta::ModeChange`
    /// per flipped field. Excludes cursor visibility / blink / shape
    /// because those flow through `GridDelta::Cursor` already.
    last_modes: Option<Modes>,
    last_tui_keyboard_lease: Option<bool>,
    /// Monotonic per-pane sequence; bumped on every emitted frame.
    /// Frontend logs a warning on gaps. Resets to 0 on `new`.
    pane_seq: u64,
    /// §6 — latest OSC 0/2 window title seen on this pane's stream. Mirrors
    /// what the desktop surfaces into `paneOscTitleStore`; the remote
    /// `list-panes` reads it so remote terminal names match the desktop pane
    /// header's variable title. `None` until the program sets a title.
    last_title: Option<String>,
    /// Latest OSC 7 working directory, included in atomic remote snapshots.
    last_cwd: Option<String>,
}

impl TerminalProducer {
    /// `scrollback_lines` is the same dimension passed to the wasm
    /// `TerminalKernel::new` — 5_000 in the default JS path.
    #[cfg(test)]
    pub fn new(rows: u16, cols: u16, scrollback_lines: usize) -> Self {
        Self::with_epoch(rows, cols, scrollback_lines, "test-lifetime".into())
    }

    pub fn with_epoch(
        rows: u16,
        cols: u16,
        scrollback_lines: usize,
        terminal_epoch: String,
    ) -> Self {
        assert!(!terminal_epoch.is_empty());
        let rows = rows.max(1);
        let cols = cols.max(1);
        let terminal = Terminal::new(rows as usize, cols as usize, scrollback_lines);
        let snapshot = vec![vec![DeltaCell::blank(); cols as usize]; rows as usize];
        let snapshot_wrapped = vec![false; rows as usize];
        let snapshot_row_revisions = vec![u64::MAX; rows as usize];
        let snapshot_visual_rows = vec![None; rows as usize];
        Self {
            terminal,
            terminal_epoch,
            semantic_metadata: None,
            semantic_history: None,
            snapshot,
            snapshot_wrapped,
            snapshot_row_revisions,
            snapshot_visual_rows,
            cursor: None,
            is_alt: None,
            last_scrollback_len: 0,
            last_scrollback_evictions: 0,
            last_scrollback_clears: 0,
            last_modes: None,
            last_tui_keyboard_lease: None,
            pane_seq: 0,
            last_title: None,
            last_cwd: None,
        }
    }

    /// §6 — latest OSC window title seen on this pane (OSC 0/2), or `None` if
    /// the program never set one. Used by the remote `list-panes` so remote
    /// terminal names match the desktop pane header's variable title.
    pub fn title(&self) -> Option<String> {
        self.last_title.clone()
    }

    pub fn revision(&self) -> u64 {
        self.pane_seq
    }

    pub fn terminal_epoch(&self) -> &str {
        &self.terminal_epoch
    }
    pub fn terminal(&self) -> &Terminal {
        &self.terminal
    }
    pub fn set_initial_cwd(&mut self, cwd: String) {
        assert_eq!(self.pane_seq, 0);
        self.last_cwd = Some(cwd);
    }

    pub fn feed_semantic(&mut self, bytes: &[u8]) -> crate::semantic::SemanticUpdate {
        *self.feed_and_diff(bytes).semantic_update.expect("producer captures each update")
    }

    pub fn resize_semantic(&mut self, rows: u16, cols: u16) -> crate::semantic::SemanticUpdate {
        *self.resize(rows, cols).semantic_update.expect("producer captures resize state")
    }

    pub fn semantic_snapshot(&self) -> crate::semantic::SemanticUpdate {
        crate::semantic::SemanticUpdate {
            terminal_epoch: self.terminal_epoch.clone(),
            body: crate::terminal_protocol::TerminalBody::Snapshot(crate::semantic::snapshot(
                &self.terminal,
                self.pane_seq,
                self.last_title.clone().unwrap_or_default(),
                self.last_cwd.clone().unwrap_or_default(),
                self.terminal
                    .rows()
                    .saturating_mul(2)
                    .min(crate::terminal_protocol::MAX_HISTORY_PAGE_ROWS as usize),
            )),
        }
    }

    pub fn semantic_history(
        &self,
        request: &crate::terminal_protocol::HistoryRequest,
    ) -> Result<crate::semantic::SemanticUpdate, &'static str> {
        crate::semantic::history_page(&self.terminal, request, &self.terminal_epoch)
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

    /// Snapshot of the pane's current terminal modes. Used to build a reattach
    /// preamble (`Modes::to_reattach_preamble`) so a remote controller learns
    /// mouse-reporting / bracketed-paste / app-cursor state that the one-time
    /// enable sequences (long since off the scrollback tail) would have set.
    pub fn modes(&self) -> Modes {
        *self.terminal.modes()
    }

    /// Authoritative inline-TUI heuristic snapshot for the RESIZE decision —
    /// see `is_alt_screen`. Uses the sticky-aware variant so an idle inline TUI
    /// (default Claude at its prompt, all live signals decayed) is still
    /// classified for the wipe-before-SIGWINCH ordering. `now_ms` is the
    /// caller's wall-clock so the decay window matches the resize clock.
    pub fn is_inline_tui_resize_at(&self, now_ms: i64) -> bool {
        self.terminal.is_inline_tui_resize_at(now_ms)
    }

    /// §wsl-resize-silence — 本 pane 是否发过 shell-integration prompt OSC（有=能早
    /// 释放 silence）。false ⇒ 无集成（WSL/cmd/explicit-launch），resize 应跳过 silence
    /// 窗口，否则那 80ms 窗口会吞掉 SIGWINCH 重绘（实测 ~25ms 落在窗口内）。
    pub fn has_shell_integration(&self) -> bool {
        self.terminal.has_shell_integration()
    }

    /// Feed PTY bytes and return the resulting delta frame.
    ///
    /// The frame's `pane_seq` is the new value (post-increment from the
    /// previous one). If the feed produced no visible change (which can
    /// happen for query bytes like DSR `\x1b[5n` that only mutate
    /// `pending_response`) the returned frame's `deltas` is empty —
    /// callers may skip emitting it over IPC to save bandwidth.
    pub fn feed_and_diff(&mut self, bytes: &[u8]) -> DeltaFrame {
        let render_activity_before = self.terminal.grid().render_activity_seq();
        self.terminal.feed(bytes);
        let mut frame = self.diff_into_frame();
        // The native parser sees the raw CSI walk (`EL`/`CUU`/rewind CUP)
        // while the delta mirror never can. Carry one per-frame hint instead
        // of making the frontend guess from cursor visibility or app modes.
        frame.requires_render_settle =
            self.terminal.grid().render_activity_seq() != render_activity_before;
        frame
    }

    /// Explicit user clear. Mutates the authoritative parser directly (never
    /// writes ANSI bytes into the PTY input stream), then emits the same delta
    /// shape consumed by the wasm mirror.
    pub fn clear_terminal(&mut self) -> DeltaFrame {
        self.terminal.clear_terminal();
        self.diff_into_frame()
    }

    /// Explicit user clear for a normal shell: drop scrollback and old rows,
    /// then move the current prompt row to the top.
    pub fn clear_terminal_preserving_prompt(&mut self) -> DeltaFrame {
        self.terminal.clear_terminal_preserving_prompt();
        self.diff_into_frame()
    }

    /// Visible row 0 as a string (for tests / diagnostics). Trims trailing blanks.
    pub fn viewport_line0_text(&self) -> String {
        let Some(row) = self.terminal.row_at_abs(0) else {
            return String::new();
        };
        let s: String = row.cells.iter().map(|c| c.ch).collect();
        s.trim_end().to_string()
    }

    /// Drain accumulated parser responses (DSR / DA replies) that must
    /// be written BACK to the PTY. The wiring layer should pump these
    /// into the pty writer immediately after each `feed_and_diff`.
    pub fn take_pending_response(&mut self) -> Vec<u8> {
        self.terminal.take_pending_response()
    }

    pub fn force_full_reframe(&mut self) {
        let rows = self.terminal.rows();
        let cols = self.terminal.cols();
        self.snapshot = vec![vec![DeltaCell::blank(); cols]; rows];
        self.snapshot_wrapped = vec![false; rows];
        self.snapshot_row_revisions = vec![u64::MAX; rows];
        self.snapshot_visual_rows = vec![None; rows];
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
        self.last_scrollback_clears = self.terminal.scrollback_clear_count();
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
        self.snapshot_wrapped = vec![false; rows as usize];
        self.snapshot_row_revisions = vec![u64::MAX; rows as usize];
        self.snapshot_visual_rows = vec![None; rows as usize];
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

    fn apply_pending_reset(&mut self, deltas: &mut Vec<GridDelta>) {
        if !self.terminal.take_pending_reset() {
            return;
        }
        deltas.push(GridDelta::Reset);
        let cols = self.terminal.cols();
        let rows = self.terminal.rows();
        self.snapshot = vec![vec![DeltaCell::blank(); cols]; rows];
        self.snapshot_wrapped = vec![false; rows];
        self.snapshot_row_revisions = vec![u64::MAX; rows];
        self.snapshot_visual_rows = vec![None; rows];
        self.cursor = None;
        self.is_alt = None;
        self.last_modes = Some(Modes::default());
    }

    fn append_screen_switch(&mut self, deltas: &mut Vec<GridDelta>) {
        let is_alt = self.terminal.grid().is_alt_screen();
        if self.is_alt == Some(is_alt) {
            return;
        }
        deltas.push(GridDelta::ScreenSwitch { is_alt });
        self.is_alt = Some(is_alt);
        let cols = self.terminal.cols();
        let rows = self.terminal.rows();
        self.snapshot = vec![vec![DeltaCell::blank(); cols]; rows];
        self.snapshot_wrapped = vec![false; rows];
        self.snapshot_row_revisions = vec![u64::MAX; rows];
        self.snapshot_visual_rows = vec![None; rows];
    }

    fn append_scrollback(&mut self, deltas: &mut Vec<GridDelta>) {
        let now_len = self.terminal.scrollback_len();
        let now_evictions = self.terminal.scrollback_eviction_count();
        let now_clears = self.terminal.scrollback_clear_count();
        if now_clears != self.last_scrollback_clears {
            deltas.push(GridDelta::ScrollbackClear);
            self.last_scrollback_len = 0;
            self.last_scrollback_evictions = now_evictions;
        }
        let evicted_since = now_evictions.saturating_sub(self.last_scrollback_evictions);
        let previous_len = self
            .last_scrollback_len
            .saturating_sub(evicted_since as usize);
        if now_len > previous_len {
            let lines = self.scrollback_lines(previous_len, now_len);
            if !lines.is_empty() {
                deltas.push(GridDelta::ScrollbackAppend { lines });
            }
        }
        self.last_scrollback_len = now_len;
        self.last_scrollback_evictions = now_evictions;
        self.last_scrollback_clears = now_clears;
    }

    fn scrollback_lines(&self, previous_len: usize, now_len: usize) -> Vec<DeltaLine> {
        let mut lines = Vec::with_capacity(now_len.saturating_sub(previous_len));
        for index in previous_len..now_len {
            let Some(row) = self.terminal.grid().scrollback.get(index) else {
                continue;
            };
            // A fresh mirror row already contains default blanks. Shipping the
            // full terminal width for a short log line was the dominant cost
            // in Ctrl+C bursts: 90+ empty cells per line were decoded, copied,
            // and interned on both sides. Preserve all meaningful trailing
            // state (coloured BCE blanks, wide-cell halves, clusters), omit
            // only the default blank suffix.
            let end = row
                .cells
                .iter()
                .enumerate()
                .rposition(|(col, cell)| {
                    !cell.is_blank()
                        || cell.width != 1
                        || row.cluster_at(col).is_some()
                        || row.link_at(col).is_some()
                })
                .map_or(0, |col| col + 1);
            let cells = row
                .cells
                .iter()
                .take(end)
                .enumerate()
                .map(|(index, cell)| {
                    let attrs = self.terminal.grid().attrs.get(cell.attr);
                    DeltaCell {
                        ch: cell.ch,
                        fg: attrs.fg,
                        bg: attrs.bg,
                        flags: attrs.flags,
                        width: cell.width,
                        cluster: row.cluster_at(index).map(|cluster| cluster.text.clone()),
                    }
                })
                .collect();
            lines.push(DeltaLine {
                cells,
                wrapped: row.wrapped,
                hyperlinks: row.hyperlinks.clone(),
            });
        }
        lines
    }

    fn append_scrolls(&mut self, deltas: &mut Vec<GridDelta>) {
        for op in coalesce_scroll_ops(self.terminal.take_scroll_ops()) {
            self.shift_snapshot_for_scroll(op);
            deltas.push(GridDelta::Scroll {
                top: op.top.min(u16::MAX as usize) as u16,
                bottom: op.bottom.min(u16::MAX as usize) as u16,
                count: op.count.min(u16::MAX as usize) as u16,
                up: op.up,
            });
        }
    }

    /// Advance our fixed-row snapshot by the same move just sent to the
    /// mirror. Grid revisions travel with moved rows, so `append_rows` still
    /// detects writes that happened before a row crossed the bottom margin.
    fn shift_snapshot_for_scroll(&mut self, op: ScrollOp) {
        let rows = self.snapshot.len();
        if rows == 0 {
            return;
        }
        let top = op.top.min(rows - 1);
        let bottom = op.bottom.min(rows - 1);
        if top > bottom {
            return;
        }
        let count = op.count.min(bottom - top + 1);
        if count == 0 {
            return;
        }
        let cols = self.terminal.cols();
        if op.up {
            self.snapshot[top..=bottom].rotate_left(count);
            self.snapshot_wrapped[top..=bottom].rotate_left(count);
            self.snapshot_row_revisions[top..=bottom].rotate_left(count);
            self.snapshot_visual_rows[top..=bottom].rotate_left(count);
            for row in bottom + 1 - count..=bottom {
                self.snapshot[row] = vec![DeltaCell::blank(); cols];
                self.snapshot_wrapped[row] = false;
                self.snapshot_row_revisions[row] = u64::MAX;
                self.snapshot_visual_rows[row] = None;
            }
        } else {
            self.snapshot[top..=bottom].rotate_right(count);
            self.snapshot_wrapped[top..=bottom].rotate_right(count);
            self.snapshot_row_revisions[top..=bottom].rotate_right(count);
            self.snapshot_visual_rows[top..=bottom].rotate_right(count);
            for row in top..top + count {
                self.snapshot[row] = vec![DeltaCell::blank(); cols];
                self.snapshot_wrapped[row] = false;
                self.snapshot_row_revisions[row] = u64::MAX;
                self.snapshot_visual_rows[row] = None;
            }
        }
    }

    fn append_rows(&mut self, deltas: &mut Vec<GridDelta>) {
        let rows = self.terminal.rows();
        let cols = self.terminal.cols();
        for row in 0..rows {
            let revision = self.terminal.grid().row_revision(row).unwrap_or(u64::MAX);
            if self.snapshot_row_revisions.get(row).copied() == Some(revision) {
                continue;
            }
            self.append_row_delta(row, cols, revision, deltas);
        }
    }

    fn append_row_delta(
        &mut self,
        row_index: usize,
        cols: usize,
        revision: u64,
        deltas: &mut Vec<GridDelta>,
    ) {
        let Some(live_row) = self.terminal.grid().row(row_index) else {
            return;
        };
        if self
            .snapshot_visual_rows
            .get(row_index)
            .and_then(Option::as_ref)
            .is_some_and(|previous| live_row.visual_eq(previous))
        {
            if let Some(previous) = self.snapshot_row_revisions.get_mut(row_index) {
                *previous = revision;
            }
            return;
        }
        let visual_row = live_row.clone();
        let links_changed = self
            .snapshot_visual_rows
            .get(row_index)
            .and_then(Option::as_ref)
            .map_or(!live_row.hyperlinks.is_empty(), |previous| {
                previous.hyperlinks != live_row.hyperlinks
            });
        let now_row = (0..cols)
            .map(|col| {
                let cell = live_row.cells.get(col).copied().unwrap_or_default();
                let attrs = self.terminal.grid().attrs.get(cell.attr);
                DeltaCell {
                    ch: cell.ch,
                    fg: attrs.fg,
                    bg: attrs.bg,
                    flags: attrs.flags,
                    width: cell.width,
                    cluster: live_row.cluster_at(col).map(|cluster| cluster.text.clone()),
                }
            })
            .collect::<Vec<_>>();
        let Some(snapshot) = self.snapshot.get_mut(row_index) else {
            return;
        };
        snapshot.resize(cols, DeltaCell::blank());
        let mut first_changed = None;
        let mut last_changed = 0;
        for col in 0..cols {
            if snapshot[col] != now_row[col] {
                first_changed.get_or_insert(col);
                last_changed = col;
            }
        }
        if links_changed {
            first_changed = Some(0);
            last_changed = cols.saturating_sub(1);
        }
        let wrapped_changed = self
            .snapshot_wrapped
            .get(row_index)
            .copied()
            .map_or(true, |previous| previous != live_row.wrapped);
        if let Some(first) = first_changed {
            deltas.push(GridDelta::Cells {
                row: row_index as u16,
                col: first as u16,
                wrapped: live_row.wrapped,
                cells: now_row[first..=last_changed].to_vec(),
            });
            for col in first..=last_changed {
                snapshot[col] = now_row[col].clone();
            }
        } else if wrapped_changed {
            deltas.push(GridDelta::Cells {
                row: row_index as u16,
                col: 0,
                wrapped: live_row.wrapped,
                cells: Vec::new(),
            });
        }
        if let Some(previous) = self.snapshot_wrapped.get_mut(row_index) {
            *previous = live_row.wrapped;
        }
        if let Some(previous) = self.snapshot_row_revisions.get_mut(row_index) {
            *previous = revision;
        }
        if let Some(previous) = self.snapshot_visual_rows.get_mut(row_index) {
            *previous = Some(visual_row);
        }
    }

    fn append_modes(&mut self, deltas: &mut Vec<GridDelta>) {
        let lease = self.terminal.grid().is_inline_tui_sticky();
        if self.last_modes.is_none() || self.last_tui_keyboard_lease != Some(lease) {
            deltas.push(GridDelta::ModeChange {
                mode: crate::term::delta::TUI_KEYBOARD_LEASE_MODE,
                on: lease,
            });
            self.last_tui_keyboard_lease = Some(lease);
        }
        let current = *self.terminal.modes();
        let previous = self.last_modes.unwrap_or_default();
        deltas.extend(
            current
                .diff(&previous)
                .into_iter()
                .map(|(mode, on)| GridDelta::ModeChange { mode, on }),
        );
        self.last_modes = Some(current);
    }

    fn append_cursor(&mut self, deltas: &mut Vec<GridDelta>) {
        let modes = *self.terminal.modes();
        let cursor = *self.terminal.grid().cursor();
        let current = CursorSnap {
            row: cursor.row as u16,
            col: cursor.col as u16,
            visible: modes.cursor_visible,
            blink: modes.cursor_blink,
            shape: kernel_shape_to_delta(modes.cursor_shape),
        };
        if self.cursor == Some(current) {
            return;
        }
        deltas.push(GridDelta::Cursor {
            row: current.row,
            col: current.col,
            visible: current.visible,
            blink: current.blink,
            shape: current.shape,
        });
        self.cursor = Some(current);
    }

    fn append_events(&mut self, deltas: &mut Vec<GridDelta>) {
        for event in self.terminal.take_pending_events() {
            match event {
                KernelEvent::TitleChanged(title) => {
                    self.last_title = Some(title.clone());
                    deltas.push(GridDelta::Title(title));
                }
                KernelEvent::CwdChanged(path) => {
                    self.last_cwd = Some(path.clone());
                    deltas.push(GridDelta::Cwd(path));
                }
                KernelEvent::Bell => deltas.push(GridDelta::Bell),
                KernelEvent::IconNameChanged(_) => {}
            }
        }
    }

    fn diff_into_frame(&mut self) -> DeltaFrame {
        let mut deltas: Vec<GridDelta> = Vec::new();

        self.apply_pending_reset(&mut deltas);
        self.append_screen_switch(&mut deltas);
        self.append_scrollback(&mut deltas);
        self.append_scrolls(&mut deltas);
        self.append_rows(&mut deltas);
        self.append_modes(&mut deltas);
        self.append_cursor(&mut deltas);
        self.append_events(&mut deltas);
        self.pane_seq = self.pane_seq.saturating_add(1);
        let mut frame = DeltaFrame::new(self.pane_seq, deltas);
        frame.semantic_update = Some(Box::new(crate::semantic::capture_update(
            &self.terminal,
            &frame,
            &self.terminal_epoch,
            self.last_title.clone().unwrap_or_default(),
            self.last_cwd.clone().unwrap_or_default(),
        )));
        let history = crate::semantic::history_range(&self.terminal);
        if let Some(previous) = self.semantic_history {
            let update = frame.semantic_update.as_mut().unwrap();
            if let crate::terminal_protocol::TerminalBody::Delta(delta) = &update.body {
                let appended = delta.deltas.iter().filter_map(|item| match item {
                    crate::terminal_protocol::TerminalDelta::ScrollbackAppend { lines, .. } => Some(lines.len() as u64),
                    _ => None,
                }).sum::<u64>();
                if previous.epoch != history.epoch || previous.end.saturating_add(appended) != history.end {
                    update.body = crate::terminal_protocol::TerminalBody::Snapshot(crate::semantic::snapshot(
                        &self.terminal, self.pane_seq, self.last_title.clone().unwrap_or_default(),
                        self.last_cwd.clone().unwrap_or_default(),
                        self.terminal.rows().saturating_mul(2).min(crate::terminal_protocol::MAX_HISTORY_PAGE_ROWS as usize),
                    ));
                }
            }
        }
        self.semantic_history = Some(history);
        let metadata = crate::semantic::screen_metadata(&self.terminal);
        if let Some(previous) = &self.semantic_metadata {
            if let crate::terminal_protocol::TerminalBody::Delta(delta) =
                &mut frame.semantic_update.as_mut().unwrap().body
            {
                use crate::terminal_protocol::{ScreenKind, TerminalDelta};
                for (index, screen) in [ScreenKind::Primary, ScreenKind::Alternate]
                    .into_iter()
                    .enumerate()
                {
                    if previous[index].cursor != metadata[index].cursor && !delta.deltas.iter().any(|item| matches!(item, TerminalDelta::Cursor { screen: target, .. } if *target == screen)) {
                        delta.deltas.push(TerminalDelta::Cursor { screen, cursor: metadata[index].cursor });
                    }
                    if previous[index].saved != metadata[index].saved {
                        delta.deltas.push(TerminalDelta::SavedCursor {
                            screen,
                            cursor: metadata[index].saved,
                        });
                    }
                    if (previous[index].top, previous[index].bottom)
                        != (metadata[index].top, metadata[index].bottom)
                    {
                        delta.deltas.push(TerminalDelta::ScrollRegion {
                            screen,
                            top: metadata[index].top,
                            bottom: metadata[index].bottom,
                        });
                    }
                }
            }
        }
        self.semantic_metadata = Some(metadata);
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

    #[test]
    fn native_delta_keeps_tui_lease_after_a_visible_cursor_repaint() {
        let mut producer = TerminalProducer::new(24, 80, 200);
        let mut mirror = Terminal::new(24, 80, 200);
        mirror
            .apply_frame(&producer.feed_and_diff(b"\x1b[?25l\x1b[?25h"))
            .unwrap();
        assert!(mirror.modes().cursor_visible);
        assert!(mirror.grid().is_inline_tui_sticky());
        mirror
            .apply_frame(&producer.feed_and_diff(b"\x1b]633;A\x07"))
            .unwrap();
        assert!(!mirror.grid().is_inline_tui_sticky());
    }

    #[test]
    fn wide_wrap_gap_keeps_empty_text_override_through_native_delta() {
        let mut producer = TerminalProducer::new(2, 17, 200);
        let frame = producer.feed_and_diff("https://x/a/中文档".as_bytes());
        let bytes = crate::term::delta::encode_frame(&frame).unwrap();
        let mut mirror = Terminal::new(2, 17, 200);
        mirror
            .apply_frame(&crate::term::delta::decode_frame(&bytes).unwrap())
            .unwrap();
        assert_eq!(
            mirror
                .wrapped_link_rows()
                .iter()
                .map(|row| row.1.as_str())
                .collect::<String>(),
            "https://x/a/中文档"
        );
    }

    #[test]
    fn scroll_compaction_preserves_movement_and_splits_at_the_wire_limit() {
        let op = ScrollOp {
            top: 0,
            bottom: 2,
            count: 3,
            up: true,
        };
        assert_eq!(coalesce_scroll_ops(vec![op; 10])[0].count, 30);
        let large = ScrollOp {
            count: u16::MAX as usize,
            ..op
        };
        let compact = coalesce_scroll_ops(vec![large, op]);
        assert_eq!(
            compact.iter().map(|op| op.count).sum::<usize>(),
            u16::MAX as usize + 3
        );
        assert!(compact.iter().all(|op| op.count <= u16::MAX as usize));
    }

    fn make_parser(rows: u16, cols: u16) -> TerminalProducer {
        TerminalProducer::new(rows, cols, 1_000)
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
        assert_eq!(frame.pane_seq, 1);
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
    fn repaint_walk_sets_delta_presentation_hint() {
        let mut p = make_parser(6, 20);
        let initial = p.feed_and_diff(b"\x1b[5;8H");
        assert!(!initial.requires_render_settle);

        // The second landing rewinds to an earlier row; an EL then erases
        // that row. Both are native-only repaint semantics which must cross
        // the delta boundary for the browser compositor.
        let repaint = p.feed_and_diff(b"\x1b[2;1H\x1b[2Kframe");
        assert!(repaint.requires_render_settle);

        let ordinary = p.feed_and_diff(b"\r\nplain output");
        assert!(!ordinary.requires_render_settle);
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
    fn soft_wrap_metadata_survives_delta_mirror_for_copy() {
        use crate::selection::Selection;
        use crate::term::terminal::Terminal;

        let url = "https://example.com/long/path";
        let mut producer = make_parser(3, 12);
        let mut mirror = Terminal::new(3, 12, 1_000);
        mirror
            .apply_frame(&producer.feed_and_diff(b""))
            .expect("initial frame");
        let frame = producer.feed_and_diff(url.as_bytes());
        let row_zero = frame.deltas.iter().find_map(|delta| match delta {
            GridDelta::Cells {
                row,
                wrapped,
                cells,
                ..
            } if *row == 0 && !cells.is_empty() => Some(*wrapped),
            _ => None,
        });
        assert_eq!(
            row_zero,
            Some(true),
            "producer must mark the URL head wrapped"
        );
        mirror.apply_frame(&frame).expect("URL frame");
        assert!(mirror.grid().row(0).unwrap().wrapped);

        let mut selection = Selection::default();
        selection.select_all(&mirror);
        assert_eq!(selection.text(&mirror), url);
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
            GridDelta::Cells {
                row, col, cells, ..
            } => {
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
            GridDelta::Cells {
                row: _, col, cells, ..
            } => {
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
        assert_eq!(a.pane_seq, 1);
        assert_eq!(b.pane_seq, 2);
        assert_eq!(c.pane_seq, 3);
    }

    #[test]
    fn semantic_packets_capture_links_at_production_and_survive_later_mutation() {
        use crate::terminal_protocol::{
            decode_frame, encode_frame, PaneRef, TerminalBody, TerminalFrame,
        };
        let mut producer = TerminalProducer::new(3, 20, 100);
        producer.feed_and_diff(b"seed");
        let captured =
            producer.feed_and_diff(b"\r\x1b]8;id=first;https://first.example\x07old\x1b]8;;\x07");
        let update = captured.semantic_update.unwrap();
        let TerminalBody::Delta(delta) = &update.body else {
            panic!("ordinary OSC8 writes must remain incremental")
        };
        assert!(delta
            .hyperlinks
            .iter()
            .any(|link| link.uri == "https://first.example"));
        producer.feed_and_diff(b"\r\x1b[2K\x1b]8;;https://second.example\x07new\x1b]8;;\x07");
        assert!(delta
            .hyperlinks
            .iter()
            .all(|link| link.uri != "https://second.example"));
        let frame = TerminalFrame::new(
            PaneRef::new("work", "pane"),
            update.terminal_epoch,
            1,
            update.body,
        );
        assert_eq!(decode_frame(&encode_frame(&frame).unwrap()).unwrap(), frame);
    }

    #[test]
    fn semantic_metadata_carries_link_only_saved_cursor_and_scroll_region_changes() {
        use crate::terminal_protocol::*;
        let mut source = TerminalProducer::new(4, 20, 20);
        source.feed_and_diff(b"\x1b]8;;https://old.example\x07same\x1b]8;;\x07");
        let changed = source.feed_and_diff(b"\r\x1b]8;;https://new.example\x07same\x1b]8;;\x07").semantic_update.unwrap();
        let TerminalBody::Delta(changed) = changed.body else { unreachable!() };
        assert!(changed.hyperlinks.iter().any(|link| link.uri == "https://new.example"));
        assert!(changed.deltas.iter().any(|delta| matches!(delta, TerminalDelta::Cells { .. })));
        let metadata = source.feed_and_diff(b"\x1b[31m\x1b7\x1b[2;4r").semantic_update.unwrap();
        let TerminalBody::Delta(metadata) = metadata.body else { unreachable!() };
        assert!(metadata.deltas.iter().any(|delta| matches!(delta, TerminalDelta::SavedCursor { cursor: Some(saved), .. } if saved.fg == WireColor::Indexed(1))));
        assert!(metadata.deltas.iter().any(|delta| matches!(delta, TerminalDelta::ScrollRegion { top: 1, bottom: 3, .. })));
    }

    #[test]
    fn semantic_seed_and_older_page_restore_exact_history_while_live_output_continues() {
        use crate::terminal_protocol::*;
        let mut producer = TerminalProducer::new(3, 20, 100);
        for row in 0..60 {
            producer.feed_and_diff(format!("row-{row:02}\r\n").as_bytes());
        }
        let initial = producer.semantic_snapshot();
        let TerminalBody::Snapshot(seed) = &initial.body else {
            unreachable!()
        };
        assert_eq!(seed.scrollback.len(), 6);
        assert!(seed.scrollback_start > seed.history.start);
        let request = HistoryRequest {
            pane: PaneRef::new("work", "pane"),
            terminal_epoch: producer.terminal_epoch().into(),
            activation_id: 1,
            history_epoch: seed.history.epoch,
            before: seed.scrollback_start,
            max_rows: 10,
        };
        let captured_page = producer.semantic_history(&request).unwrap();
        let live = producer.feed_and_diff(b"later\r\n");
        let TerminalBody::HistoryPage(page) = &captured_page.body else {
            unreachable!()
        };
        assert_eq!(page.start + page.lines.len() as u64, request.before);
        assert_eq!(page.history.epoch, producer.terminal.history_generation());
        let mut mirror = Terminal::new(3, 20, 100);
        mirror.apply_semantic_snapshot(seed);
        mirror.scroll_up_view(2);
        let before = mirror.dump_visible_text();
        let TerminalBody::Delta(delta) = &live.semantic_update.unwrap().body else {
            unreachable!()
        };
        mirror.apply_semantic_delta(delta);
        assert_eq!(mirror.dump_visible_text(), before);
        mirror.prepend_semantic_history(page);
        assert_eq!(mirror.dump_visible_text(), before);
        assert_eq!(
            mirror.scrollback_len(),
            seed.scrollback.len() + page.lines.len() + 1
        );
        let frame = captured_page.into_frame(PaneRef::new("work", "pane"), 1);
        assert_eq!(decode_frame(&encode_frame(&frame).unwrap()).unwrap(), frame);
    }

    #[test]
    fn history_generation_fences_clear_resize_eviction_and_foreign_lifetimes() {
        use crate::terminal_protocol::*;
        let mut producer = TerminalProducer::new(2, 20, 10);
        for row in 0..15 {
            producer.feed_and_diff(format!("{row}\r\n").as_bytes());
        }
        let TerminalBody::Snapshot(seed) = producer.semantic_snapshot().body else {
            unreachable!()
        };
        let mut request = HistoryRequest {
            pane: PaneRef::new("work", "pane"),
            terminal_epoch: producer.terminal_epoch().into(),
            activation_id: 1,
            history_epoch: seed.history.epoch,
            before: seed.history.start - 1,
            max_rows: 2,
        };
        assert_eq!(producer.semantic_history(&request), Err("HISTORY_EVICTED"));
        request.before = seed.history.end;
        request.terminal_epoch = "foreign".into();
        assert_eq!(
            producer.semantic_history(&request),
            Err("TERMINAL_LIFETIME_CHANGED")
        );
        request.terminal_epoch = producer.terminal_epoch().into();
        producer.resize(3, 10);
        assert_eq!(
            producer.semantic_history(&request),
            Err("HISTORY_GENERATION_CHANGED")
        );
        let TerminalBody::Snapshot(resized) = producer.semantic_snapshot().body else {
            unreachable!()
        };
        request.history_epoch = resized.history.epoch;
        producer.feed_and_diff(b"\x1b[3J");
        assert_eq!(
            producer.semantic_history(&request),
            Err("HISTORY_GENERATION_CHANGED")
        );
    }

    #[test]
    fn snapshot_revision_accepts_the_very_next_output_frame() {
        let mut parser = make_parser(3, 12);
        let first = parser.feed_and_diff(b"first");
        let snapshot = parser.semantic_snapshot();
        assert_eq!(snapshot.revision(), first.pane_seq);
        let next = parser.feed_and_diff(b"!");
        assert_eq!(next.pane_seq, snapshot.revision() + 1);
        let crate::terminal_protocol::TerminalBody::Snapshot(snapshot) = snapshot.body else { unreachable!() };
        let crate::terminal_protocol::TerminalBody::Delta(delta) = next.semantic_update.unwrap().body else { unreachable!() };
        let mut mirror = Terminal::new(3, 12, 1000);
        mirror.apply_semantic_snapshot(&snapshot);
        mirror.apply_semantic_delta(&delta);
        assert_eq!(mirror.dump_visible_text()[0].trim(), "first!");
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
    fn screen_switch_resets_diff_baseline_so_content_reemits() {
        // 「TUI 退出再进残留」回归测试(后端侧)。切屏会整屏改变
        // 内容映射,cell-diff 基线 `self.snapshot` 必须随之重置为全
        // blank,否则当 alt 屏的内容恰好与上一屏 snapshot 相同(同位
        // 同字)时,diff 会判定"无变化"而不发 Cells,镜像便保留上次
        // 残留。这里在 primary row0 写 "ABC",再进入 alt 屏同样在
        // row0 写 "ABC":若基线未重置,alt 的 "ABC" 与 snapshot 的
        // "ABC" 相同 → 无 Cells;基线重置后 → 重发 "ABC"。
        let mut p = make_parser(4, 10);
        let _ = p.feed_and_diff(b"");
        // primary row0 = "ABC",将其写入基线 snapshot。
        let _ = p.feed_and_diff(b"ABC");
        // 进入 alt 屏(DECSET 1049),光标 home,在 alt row0 写 "ABC"。
        let frame = p.feed_and_diff(b"\x1b[?1049h\x1b[HABC");
        // 必须有覆盖 row0 且含 'A' 的 Cells delta。
        let reemits_row0 = frame.deltas.iter().any(|d| match d {
            GridDelta::Cells { row, cells, .. } => *row == 0 && cells.iter().any(|c| c.ch == 'A'),
            _ => false,
        });
        assert!(
            reemits_row0,
            "切屏后基线未重置,alt 屏内容未被重发;got {:?}",
            frame.deltas
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
        use crate::term::terminal::Terminal;
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
        let appends: Vec<&Vec<DeltaLine>> = frame
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
        assert_eq!(
            lines[0].cells.len(),
            2,
            "default blank suffix must stay implicit on the wire"
        );
        assert_eq!(lines[0].cells[0].ch, 'A');
        assert_eq!(lines[0].cells[1].ch, 'B');
        assert!(
            !lines[0].wrapped,
            "hard newline must not be marked soft-wrapped"
        );
    }

    #[test]
    fn bottom_margin_scroll_moves_rows_without_full_cell_retransmit() {
        use crate::term::terminal::Terminal;

        let mut producer = make_parser(3, 8);
        let mut mirror = Terminal::new(3, 8, 1_000);
        mirror
            .apply_frame(&producer.feed_and_diff(b"A\r\nB\r\nC"))
            .expect("initial frame applies");

        let frame = producer.feed_and_diff(b"\r\nD");
        assert!(
            frame.deltas.iter().any(|delta| matches!(
                delta,
                GridDelta::Scroll {
                    top: 0,
                    bottom: 2,
                    count: 1,
                    up: true
                }
            )),
            "bottom-margin LF must travel as a viewport Scroll: {:?}",
            frame.deltas,
        );
        assert!(
            !frame.deltas.iter().any(|delta| matches!(
                delta,
                GridDelta::Cells { row, .. } if *row < 2
            )),
            "moved rows must not be serialized again: {:?}",
            frame.deltas,
        );

        mirror.apply_frame(&frame).expect("scroll frame applies");
        assert_eq!(
            mirror.dump_visible_text(),
            producer.terminal.dump_visible_text(),
            "scroll delta plus bottom-row cells must exactly mirror producer"
        );
        assert_eq!(
            mirror.scrollback_len(),
            producer.terminal.scrollback_len(),
            "Scroll must not duplicate history already carried by ScrollbackAppend"
        );
    }

    #[test]
    fn writes_that_scroll_again_within_one_frame_survive_snapshot_shift() {
        use crate::term::terminal::Terminal;

        let mut producer = make_parser(4, 8);
        let mut mirror = Terminal::new(4, 8, 1_000);
        mirror
            .apply_frame(&producer.feed_and_diff(b"A\r\nB\r\nC\r\nD"))
            .expect("initial frame applies");

        // E is written at the bottom, then moves up during the second LF in
        // this SAME raw PTY batch. The snapshot must retain its old revision
        // baseline until append_rows sees that E is genuinely new.
        let frame = producer.feed_and_diff(b"\r\nE\r\nF");
        assert!(
            frame.deltas.iter().any(|delta| matches!(
                delta,
                GridDelta::Scroll {
                    top: 0,
                    bottom: 3,
                    count: 2,
                    up: true
                }
            )),
            "consecutive bottom scrolls should coalesce: {:?}",
            frame.deltas,
        );
        assert!(
            frame.deltas.iter().any(|delta| matches!(
                delta,
                GridDelta::Cells { row: 2, .. } | GridDelta::Cells { row: 3, .. }
            )),
            "post-scroll rows must carry E/F writes: {:?}",
            frame.deltas,
        );
        mirror
            .apply_frame(&frame)
            .expect("coalesced scroll frame applies");
        assert_eq!(
            mirror.dump_visible_text(),
            producer.terminal.dump_visible_text(),
            "mirror must retain writes that moved before frame emission"
        );
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
    fn repeated_in_place_tui_frames_never_enter_scrollback() {
        use crate::term::terminal::Terminal;

        let mut producer = make_parser(6, 24);
        let mut mirror = Terminal::new(6, 24, 1_000);
        mirror
            .apply_frame(&producer.feed_and_diff(b""))
            .expect("initial frame applies");

        // Ink/log-update style redraw: home, erase two rows, repaint them,
        // then repeat. No frame reaches the bottom margin, so it must stay
        // entirely in the visible grid even after a sustained animation.
        for tick in 0..200 {
            let bytes = format!("\x1b[H\x1b[2Kspinner {tick:03}\r\n\x1b[2Kworking");
            let frame = producer.feed_and_diff(bytes.as_bytes());
            assert!(
                !frame
                    .deltas
                    .iter()
                    .any(|delta| matches!(delta, GridDelta::ScrollbackAppend { .. })),
                "in-place TUI frame {tick} unexpectedly entered scrollback: {:?}",
                frame.deltas
            );
            mirror.apply_frame(&frame).expect("TUI frame applies");
        }

        assert_eq!(producer.terminal.scrollback_len(), 0);
        assert_eq!(mirror.scrollback_len(), 0);
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

    /// P3.4 round-trip: prove producer (`TerminalProducer`) + consumer
    /// (`Terminal::apply_delta`) are symmetric. Feed identical byte
    /// streams through (a) a baseline Terminal that runs vte locally
    /// — the existing wasm path — and (b) a TerminalProducer whose frames
    /// are applied to a fresh "mirror" Terminal. After every chunk,
    /// the mirror's visible grid must match the baseline's.
    #[test]
    fn round_trip_matches_direct_feed() {
        use crate::term::terminal::Terminal;

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
        let mut producer = TerminalProducer::new(rows, cols, scrollback);

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

    #[test]
    fn shell_clear_emits_scrollback_clear_delta() {
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"a\r\nb\r\nc\r\nd");
        let frame = p.feed_and_diff(b"\x1b[H\x1b[2J");
        assert!(
            frame
                .deltas
                .iter()
                .any(|d| matches!(d, GridDelta::ScrollbackClear)),
            "accepted primary-screen ED 2 must tell the mirror/backend to release history: {:?}",
            frame.deltas
        );
    }

    #[test]
    fn explicit_clear_emits_clear_and_blank_cells() {
        let mut p = make_parser(2, 5);
        let _ = p.feed_and_diff(b"secret\r\nhistory\r\nvisible");
        let frame = p.clear_terminal();
        assert!(frame
            .deltas
            .iter()
            .any(|d| matches!(d, GridDelta::ScrollbackClear)));
        assert_eq!(p.viewport_line0_text(), "");
    }

    #[test]
    fn multiline_console_echo_preserves_source_line_order_through_delta_mirror() {
        use crate::term::terminal::Terminal;

        let lines = [
            "$P='http://127.0.0.1:51081'",
            "$C='chrome.exe'",
            "Start-Process $C",
            "notebooklm.google.com",
        ];
        let mut producer = make_parser(8, 80);
        let mut mirror = Terminal::new(8, 80, 1_000);
        mirror
            .apply_frame(&producer.feed_and_diff(b""))
            .expect("initial mirror frame");
        mirror
            .apply_frame(&producer.feed_and_diff(lines.join("\r\n").as_bytes()))
            .expect("ordered multiline delta frame");

        for (row, expected) in lines.iter().enumerate() {
            let actual: String = mirror
                .grid()
                .row(row)
                .expect("visible row")
                .cells
                .iter()
                .map(|cell| cell.ch)
                .collect();
            assert_eq!(
                actual.trim_end(),
                *expected,
                "line {row} reversed or displaced"
            );
        }
    }
}
