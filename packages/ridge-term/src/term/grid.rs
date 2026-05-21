//! The Grid: visible rows + cursor + scrollback, with alt screen and
//! scroll region (DECSTBM) support.
//!
//! ## Coordinate system
//! Row 0 = top, col 0 = left. Same as xterm/VT.
//!
//! ## Two screens
//! `Grid` owns a *primary* and an *alternate* screen. `is_alt` selects
//! which one is active for rendering and parser ops. Switching screens
//! does NOT touch the inactive screen's contents — that's how vim/less
//! preserve the underlying shell view.
//!
//! ### Important property: scrollback only follows the primary screen
//! When the alt screen scrolls, lines are dropped on the floor — they do
//! NOT enter the scrollback ring. Otherwise `vim` would pollute history
//! with thousands of viewport-replays. Tested across xterm, VTE, kitty,
//! alacritty — universal behavior.
//!
//! ## Scroll region (DECSTBM)
//! `scroll_top` / `scroll_bottom` are 0-based inclusive row indices that
//! constrain the scroll *region* used by LF at the bottom, IND, RI, SU,
//! SD, IL, DL. Default = full screen.
//!
//! ### Scroll region + scrollback interaction
//! Lines scrolled out of a *partial* scroll region (e.g. less shows a
//! status line at the bottom and scrolls only rows 0..rows-2) do NOT
//! enter scrollback either. Only full-screen scrolls (`top=0,
//! bottom=rows-1`) push to scrollback. This matches xterm — and is why
//! `less +F` doesn't fill your scrollback while tailing a log.

use super::attr_table::{AttrId, AttrTable};
use super::attrs::{Attrs, Color, Flags};
use super::cell::{Cell, Row};
use super::cursor::{Cursor, SavedCursor};
use super::scrollback::Scrollback;
use super::wcwidth::{wcwidth, wcwidth_grapheme};

/// Erase-in-display modes (CSI J).
#[derive(Debug, Clone, Copy)]
pub enum EraseMode {
    /// 0: from cursor to end.
    Below,
    /// 1: from start to cursor.
    Above,
    /// 2: entire screen.
    All,
    /// 3: xterm extension — erase saved (scrollback) lines. Does NOT
    /// touch the visible grid. Modern shells (PowerShell `Clear-Host`,
    /// bash `clear -x`, `printf '\\e[3J'`) use this when the user
    /// explicitly asks to wipe both screen and scrollback. Without
    /// this variant the kernel was silently demoting `\x1b[3J` to
    /// `\x1b[2J`, leaving the in-memory ring buffer untouched —
    /// matches the user-reported "clear 不能完全清理" symptom.
    SavedLines,
}

/// One screen buffer. Primary and alt are both `Screen`; `Grid` switches
/// the active one. Each screen carries its own cursor + saved cursor +
/// scroll region — switching to alt resets none of those, mirroring xterm.
pub struct Screen {
    rows: Vec<Row>,
    pub cursor: Cursor,
    pub saved_cursor: Option<SavedCursor>,
    /// Top of the scroll region, 0-based inclusive. Default 0.
    pub scroll_top: usize,
    /// Bottom of the scroll region, 0-based inclusive. Default rows-1.
    pub scroll_bottom: usize,
}

impl Screen {
    fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: (0..rows).map(|_| Row::new(cols)).collect(),
            cursor: Cursor::default(),
            saved_cursor: None,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
        }
    }

    /// Whether the scroll region currently covers the entire screen.
    /// Used to decide if scrolled-off rows should enter scrollback.
    fn is_full_region(&self) -> bool {
        self.scroll_top == 0 && self.scroll_bottom + 1 == self.rows.len()
    }
}

/// Which branch `Grid::resize` actually took. Retained so frontend devtools
/// (`__RIDGE_KERNEL.lastResizeDiags()`) can confirm the §1.22 wipe path
/// fired in a live scenario. Since §1.25 (2026-05-06) the kernel never
/// reflows on resize — both primary and alt always go through naive
/// truncate/pad — so only one branch exists. The enum stays for
/// forward-compat: a future deferred-reflow mode (run only when no TUI
/// is active and only on idle) would add a `DeferredReflow` variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ResizeBranch {
    /// Naive truncate/pad on both screens. The only branch §1.25 takes;
    /// `ResizeDiag::is_alt` disambiguates whether alt was active and
    /// therefore whether the §1.22 wipe ran.
    Naive,
}

/// One entry in the resize trace ring. Captured per `Grid::resize` call.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ResizeDiag {
    pub old_rows: usize,
    pub old_cols: usize,
    pub new_rows: usize,
    pub new_cols: usize,
    pub is_alt: bool,
    pub dim_changed: bool,
    pub branch: ResizeBranch,
    /// Whether the §1.22 alt-buffer wipe path fired (entire visible alt
    /// region cleared + cursor homed).
    pub wipe_fired: bool,
    /// Whether the §1.26 partial primary cleanup fired (cursor row past
    /// cur_col + every row below cursor cleared, rows above preserved).
    /// Used for plain shell resizes — keeps prior command output visible.
    pub cleared_below_cursor: bool,
    /// Whether the §A.3 inline-TUI **full** primary wipe fired (entire
    /// visible primary region cleared + cursor homed, scrollback
    /// preserved). Used when an Ink-style app (Claude Code's input box)
    /// is foreground on primary so its diff redraw lands on a blank
    /// canvas. Mutually exclusive with `wipe_fired` (alt) and with
    /// `cleared_below_cursor` (which would be redundant under a full wipe).
    pub inline_tui_wipe: bool,
    /// Snapshot of the inline-TUI heuristic at resize time, for live
    /// debugging via `__RIDGE_KERNEL.lastResizeDiags()`. True when the
    /// caller said "this primary pane is hosting an inline TUI right now"
    /// (cursor hidden + recent absolute-positioning CSI within decay window).
    pub inline_tui_active: bool,
}

const RESIZE_DIAG_RING_CAP: usize = 32;

/// How long after an absolute-positioning CSI we still consider the pane
/// "in inline-TUI mode". 2 seconds is generous enough to cover Ink's
/// idle frames between user keystrokes, but short enough that a one-shot
/// `clear; printf '\x1b[H'` doesn't leave the heuristic stuck on for the
/// rest of the session.
const INLINE_TUI_DECAY_MS: i64 = 2_000;

/// Grace window after the user sends Ctrl+C during which the inline-TUI
/// heuristic is force-disabled. 3 s covers: (a) the SIGINT delivery
/// roundtrip through ConPTY, (b) the shell's prompt repaint
/// (`PS C:\...> `), and (c) a couple of PSReadLine keystroke-driven
/// CHA emits that would otherwise immediately re-arm the heuristic.
/// Past 3 s, real surviving TUIs (those that trapped SIGINT) get
/// re-classified normally on their next abs-positioning CSI.
const CTRL_C_GRACE_MS: i64 = 3_000;

pub struct Grid {
    rows: usize,
    cols: usize,
    primary: Screen,
    alt: Screen,
    /// `false` = primary is active, `true` = alt is active.
    is_alt: bool,
    pub attrs: AttrTable,
    pub scrollback: Scrollback,
    /// Bounded ring of the most recent `resize` calls. Used by JS devtools
    /// (`__RIDGE_KERNEL.lastResizeDiags()`) to confirm which branch fired
    /// during a live repro of the alt-screen resize bug.
    last_resizes: Vec<ResizeDiag>,
    /// Wall-clock ms (unix epoch) of the most recent absolute-positioning
    /// CSI processed by the parser (CUP `H`, HVP `f`, CHA `G` / HPA `` ` ``,
    /// VPA `d`). Combined with the `cursor_visible` mode flag this is the
    /// "is an inline TUI live in this primary pane" heuristic — see §1.26
    /// in CLAUDE.md. 0 sentinel = no absolute-positioning has ever been
    /// observed (so the heuristic returns false until a TUI like Claude
    /// Code's Ink layer drives the cursor).
    last_abs_csi_at_ms: i64,
    /// §1.27-tail (2026-05-07) — cursor (row, col) snapshot at the moment
    /// of the most recent absolute-positioning CSI. Ink-style apps emit
    /// many CUPs per frame (one per row during the walk-and-redraw); the
    /// LAST one of any frame parks the cursor at the input row (bottom
    /// of the rendered frame). Sampling AFTER a feed batch completes
    /// therefore yields the inline-TUI's stable input row even when the
    /// live cursor was mid-walk during the frame. JS uses this as the
    /// IME helper anchor when no user-keystroke anchor has been captured
    /// yet (the "user clicked into a Claude Code pane and immediately
    /// typed pinyin" case where `inputAnchorPixelPosition` would
    /// otherwise teleport the helper to whatever spinner row the live
    /// cursor happens to be passing through).
    last_abs_csi_row: u16,
    last_abs_csi_col: u16,
    /// §A.4 (2026-05-08) — most-recent timestamp of a CSI that participates
    /// in an inline-TUI redraw walk but does NOT specify an absolute target
    /// position: EL `K`, ED `J`, CUU `A`, CUD `B` / VPR `e`. These open
    /// Ink/log-update's `(\x1b[2K\x1b[1A)*N` walk-and-erase prelude before
    /// the trailing `\x1b[G` parks the cursor. Tracked separately from
    /// `last_abs_csi_at_ms` so `last_abs_csi_position()` (read by the IME
    /// helper anchor) keeps its "last absolute LANDING" semantics — adding
    /// redraw CSIs there would corrupt the anchor.
    last_redraw_csi_at_ms: i64,
    /// Timestamp of the most recent Ctrl+C the user sent to this pane.
    /// Within `CTRL_C_GRACE_MS` of this timestamp,
    /// `is_inline_tui_active_at` returns false unconditionally — even
    /// if cursor-hidden + recent abs-CSI would normally classify the
    /// pane as inline-TUI mode.
    ///
    /// Why: when a user kills an Ink-style TUI with Ctrl+C, the TUI
    /// has no chance to emit `?25h` (show cursor), so `cursor_visible`
    /// stays `false`. PSReadLine then emits `CHA \x1b[G` on every
    /// keystroke of the user's next command — an absolute-positioning
    /// CSI that keeps `last_abs_csi_at_ms` perpetually fresh. The
    /// three heuristic conditions stay satisfied forever and the
    /// shell-history IME helper / popup never re-enables.
    ///
    /// Grace window short-circuits this: for a few seconds post-Ctrl+C
    /// we assume any TUI we were running has been killed. If it
    /// trapped SIGINT and is still running, it'll re-emit CSIs and
    /// the heuristic re-engages cleanly once the grace expires.
    /// Sentinel 0 = no Ctrl+C ever observed.
    last_ctrl_c_at_ms: i64,
    /// SGR "pen" mirrored from the parser's `current_attrs` for BCE
    /// (Background Color Erase). Erase / scroll / IL / DL paths fill
    /// blanked cells with `Cell { ch: ' ', attr: <pen.bg> }` so a TUI
    /// that paints a coloured status line and then ED-clears the rest
    /// of the row preserves the bg colour to the right margin — xterm
    /// / iTerm2 / VTE standard behaviour. Parser keeps this in sync
    /// via `set_pen` after every SGR / DECSTR / RIS.
    pen: Attrs,
}

impl Grid {
    pub fn new(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        Self {
            rows,
            cols,
            primary: Screen::new(rows, cols),
            alt: Screen::new(rows, cols),
            is_alt: false,
            attrs: AttrTable::default(),
            scrollback: Scrollback::new(scrollback_lines),
            last_resizes: Vec::with_capacity(RESIZE_DIAG_RING_CAP),
            last_abs_csi_at_ms: 0,
            last_abs_csi_row: 0,
            last_abs_csi_col: 0,
            last_redraw_csi_at_ms: 0,
            last_ctrl_c_at_ms: 0,
            pen: Attrs::DEFAULT,
        }
    }

    /// Sync the BCE pen from the parser's `current_attrs`. Called after
    /// every SGR / DECSTR / RIS so subsequent erase / scroll / IL / DL
    /// paths fill blank cells with the active background colour.
    pub fn set_pen(&mut self, attrs: Attrs) {
        self.pen = attrs;
    }

    /// Build the cell that erase / scroll / IL / DL paths use to fill
    /// blanked positions. When the pen carries the default background
    /// this collapses to `Cell::EMPTY` — no attr table churn, identical
    /// to the pre-BCE behaviour. When the pen carries a non-default
    /// background, only the `bg` field is preserved (fg drops to default
    /// and flags clear) — matches xterm's BCE rule which intentionally
    /// strips fg / underline / bold from the blank so a future print
    /// inside the cleared region starts from a sensible base.
    fn bce_cell(&mut self) -> Cell {
        if matches!(self.pen.bg.kind(), super::attrs::ColorKind::Default) {
            return Cell::EMPTY;
        }
        let bce_attrs = Attrs {
            fg: Color::DEFAULT,
            bg: self.pen.bg,
            flags: Flags::empty(),
        };
        let attr_id = self.attrs.intern(bce_attrs);
        Cell::new(' ', attr_id, 1)
    }

    /// Most recent `resize` calls (newest last), bounded to 32 entries.
    pub fn last_resize_diags(&self) -> &[ResizeDiag] {
        &self.last_resizes
    }

    /// Record that the parser just dispatched an absolute-positioning CSI
    /// (CUP `H`, HVP `f`, CHA `G` / HPA `` ` ``, VPA `d`). The timestamp
    /// is consumed by `is_inline_tui_active_at` to decide whether the next
    /// resize on a primary pane should wipe the visible region.
    ///
    /// §1.27-tail also snapshots the cursor's NEW (post-CUP) position so
    /// `last_abs_csi_position` can serve as a stable IME helper anchor for
    /// inline TUIs that walk the cursor through every frame row.
    ///
    /// Caller passes wall-clock ms (`crate::term::clock::now_ms()` at
    /// runtime). Tests pass a controlled value to drive the decay window
    /// deterministically.
    pub fn note_absolute_positioning(&mut self, now_ms: i64) {
        self.last_abs_csi_at_ms = now_ms;
        let cur = self.screen().cursor;
        self.last_abs_csi_row = cur.row.min(u16::MAX as usize) as u16;
        self.last_abs_csi_col = cur.col.min(u16::MAX as usize) as u16;
    }

    /// §A.4 (2026-05-08) — record an EL/ED/CUU/CUD dispatch. Only the
    /// timestamp is stored (no cursor snapshot): this is purely a "redraw
    /// activity is happening" hint that participates in
    /// `is_inline_tui_active_at` but must NOT affect the IME anchor read
    /// from `last_abs_csi_position()`.
    pub fn note_redraw_csi(&mut self, now_ms: i64) {
        self.last_redraw_csi_at_ms = now_ms;
    }

    /// Record that the user just sent Ctrl+C (ETX `\x03`) to this pane.
    /// Within `CTRL_C_GRACE_MS` of this timestamp, the inline-TUI
    /// heuristic is force-disabled — see `last_ctrl_c_at_ms` doc for
    /// motivation. Caller passes wall-clock ms.
    pub fn note_ctrl_c_sent(&mut self, now_ms: i64) {
        self.last_ctrl_c_at_ms = now_ms;
    }

    /// Most recent absolute-positioning timestamp. 0 = never observed.
    /// Exposed for tests and for `Terminal::is_inline_tui_mode_at`.
    pub fn last_abs_csi_at_ms(&self) -> i64 {
        self.last_abs_csi_at_ms
    }

    /// §1.27-tail — cursor (row, col, at_ms) at the moment of the most
    /// recent absolute-positioning CSI. Returns `None` when no abs CSI
    /// has been observed (sentinel `at_ms == 0`). Used by JS as the IME
    /// helper anchor when no user-keystroke anchor exists; sampling
    /// AFTER a feed batch completes yields the inline-TUI's resting
    /// (input-row) position even when intermediate state was mid-walk.
    pub fn last_abs_csi_position(&self) -> Option<(usize, usize, i64)> {
        if self.last_abs_csi_at_ms == 0 {
            return None;
        }
        Some((
            self.last_abs_csi_row as usize,
            self.last_abs_csi_col as usize,
            self.last_abs_csi_at_ms,
        ))
    }

    /// Inline-TUI heuristic: returns true iff
    /// 1. NOT on alt screen (`?1049h` apps use the alt-wipe path instead).
    /// 2. The caller's `cursor_visible` snapshot is false (`?25l` was last set).
    /// 3. An absolute-positioning CSI was processed within the last
    ///    `INLINE_TUI_DECAY_MS` (currently 2 s).
    ///
    /// The cursor-hide criterion alone would false-positive on PSReadLine
    /// (which briefly hides cursor during prompt redraw); the absolute-
    /// positioning criterion alone would false-positive on `clear`-style
    /// commands that emit a one-shot `CSI H`. Together they pin the
    /// "Ink / lazygit-style continuously-redrawing TUI on primary" case.
    pub fn is_inline_tui_active_at(&self, now_ms: i64, cursor_visible: bool) -> bool {
        if self.is_alt {
            return false;
        }
        if cursor_visible {
            return false;
        }
        // Ctrl+C grace window: caller sent SIGINT recently. Assume any
        // inline-TUI we were tracking is now dead (or about to be). If
        // a surviving TUI keeps re-emitting CSIs, the next check after
        // the grace expires will re-engage the heuristic naturally.
        // See `last_ctrl_c_at_ms` doc for the PSReadLine-keeps-it-stuck
        // bug this fixes.
        if self.last_ctrl_c_at_ms > 0
            && now_ms.saturating_sub(self.last_ctrl_c_at_ms) < CTRL_C_GRACE_MS
        {
            return false;
        }
        // §A.4 — accept either an absolute-positioning CSI (CUP/HVP/CHA/VPA)
        // OR a redraw-walk CSI (EL/ED/CUU/CUD) within the decay window. The
        // latter covers Ink/log-update's `(\x1b[2K\x1b[1A)*N` prelude where
        // §1.27 alone would not activate until the trailing `\x1b[G`.
        let last = self.last_abs_csi_at_ms.max(self.last_redraw_csi_at_ms);
        if last == 0 {
            return false;
        }
        now_ms.saturating_sub(last) < INLINE_TUI_DECAY_MS
    }

    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn is_alt_screen(&self) -> bool {
        self.is_alt
    }
    /// Top of the scroll region on the active screen, 0-based inclusive.
    /// Used by the parser to apply DECOM (?6 origin mode) offsets to CUP
    /// and VPA: when origin mode is on, `H`/`f`/`d` are interpreted
    /// relative to this row instead of the screen top.
    pub fn scroll_top(&self) -> usize {
        self.screen().scroll_top
    }
    /// Bottom of the scroll region on the active screen, 0-based
    /// inclusive. Used together with `scroll_top()` to clamp DECOM-mode
    /// cursor positioning.
    pub fn scroll_bottom(&self) -> usize {
        self.screen().scroll_bottom
    }

    fn screen(&self) -> &Screen {
        if self.is_alt {
            &self.alt
        } else {
            &self.primary
        }
    }
    fn screen_mut(&mut self) -> &mut Screen {
        if self.is_alt {
            &mut self.alt
        } else {
            &mut self.primary
        }
    }

    pub fn cursor(&self) -> &Cursor {
        &self.screen().cursor
    }
    pub fn cursor_mut(&mut self) -> &mut Cursor {
        &mut self.screen_mut().cursor
    }
    pub fn saved_cursor_mut(&mut self) -> &mut Option<SavedCursor> {
        &mut self.screen_mut().saved_cursor
    }

    pub fn row(&self, idx: usize) -> Option<&Row> {
        self.screen().rows.get(idx)
    }

    /// Mutable row access on the active screen. Added for the P3.4
    /// delta-apply path so `Terminal::apply_delta` can overwrite cell
    /// contents from a `GridDelta::Cells` payload without having to
    /// re-feed the change through the vte parser (which would defeat
    /// the entire point of having the parser run on the Rust side).
    /// Returns `None` past the last live row; callers should ignore
    /// such writes rather than treat them as errors — the producer
    /// (`PaneParser`) only emits in-bounds rows.
    pub fn row_mut(&mut self, idx: usize) -> Option<&mut Row> {
        self.screen_mut().rows.get_mut(idx)
    }

    /// Write a span of `(ch, attrs, width)` cells starting at
    /// `(row, col)`. Used by the P3.4 delta-apply path; the AttrTable
    /// re-interns each cell's attrs to a local AttrId before writing
    /// so the resulting cell is comparable with the rest of this
    /// grid's cells (interned ids are per-AttrTable, not portable).
    ///
    /// Out-of-bounds writes are silently ignored — see `row_mut`.
    pub fn write_delta_cells(
        &mut self,
        row: usize,
        col: usize,
        cells: &[(char, Attrs, u8)],
    ) {
        // Intern attrs in a first pass so we don't hold &mut self.attrs
        // and &mut self.screen at the same time (the borrow checker
        // would reject it even though the fields are disjoint).
        let attr_ids: Vec<crate::term::attr_table::AttrId> = cells
            .iter()
            .map(|(_, attrs, _)| self.attrs.intern(*attrs))
            .collect();
        let target = match self.screen_mut().rows.get_mut(row) {
            Some(r) => r,
            None => return,
        };
        for (i, (ch, _attrs, width)) in cells.iter().enumerate() {
            let target_col = col + i;
            let grid_cell = match target.cells.get_mut(target_col) {
                Some(c) => c,
                None => break,
            };
            grid_cell.ch = *ch;
            grid_cell.attr = attr_ids[i];
            grid_cell.width = *width;
        }
    }

    /// Switch to alt screen (DECSET 1049 / 47 / 1047). Idempotent.
    /// `clear_on_enter` corresponds to the `1049` variant: clear the alt
    /// screen on entry so we get a fresh blank canvas for fullscreen apps.
    pub fn enter_alt_screen(&mut self, clear_on_enter: bool) {
        if self.is_alt {
            return;
        }
        self.is_alt = true;
        if clear_on_enter {
            let bce = self.bce_cell();
            for r in &mut self.alt.rows {
                r.fill_blank(bce);
            }
            self.alt.cursor = Cursor::default();
            self.alt.scroll_top = 0;
            self.alt.scroll_bottom = self.rows.saturating_sub(1);
        }
    }

    /// Leave alt screen (DECRST 1049 / 47 / 1047). Idempotent.
    pub fn leave_alt_screen(&mut self) {
        if !self.is_alt {
            return;
        }
        self.is_alt = false;
    }

    /// CSI ? r  — set scroll region. 1-based-on-the-wire bounds clamped
    /// internally to 0-based inclusive. Empty/default args = full screen.
    /// xterm also moves cursor to (0,0) on STBM, so we do too.
    pub fn set_scroll_region(&mut self, top_1based: Option<usize>, bottom_1based: Option<usize>) {
        let last = self.rows.saturating_sub(1);
        let top = top_1based
            .map(|v| v.saturating_sub(1))
            .unwrap_or(0)
            .min(last);
        let bottom = bottom_1based
            .map(|v| v.saturating_sub(1))
            .unwrap_or(last)
            .min(last);
        if top >= bottom {
            // Invalid region — silently fall back to full screen, like xterm.
            let scr = self.screen_mut();
            scr.scroll_top = 0;
            scr.scroll_bottom = last;
        } else {
            let scr = self.screen_mut();
            scr.scroll_top = top;
            scr.scroll_bottom = bottom;
        }
        self.cursor_to(0, 0);
    }

    /// Resize. Both primary and alt screens always go through naive
    /// truncate/pad — the kernel does not reflow.
    ///
    /// §1.25 (2026-05-06): the previous reflow path (`reflow_primary`) was
    /// removed. Reasoning: any application that cares about the new size
    /// receives SIGWINCH from the PTY and emits its own redraw at the new
    /// width — shells (PSReadLine, fish, zsh-zle), full-screen TUIs (vim,
    /// less, htop), and Ink-based CLIs (claude code, lazygit) all do this.
    /// A simultaneous kernel-side reflow races with that redraw: while
    /// reflow is moving cells around to a new wrap, the application's own
    /// repaint bytes arrive and overwrite cells based on a layout the
    /// kernel has already mutated, producing "字符打架" (visible
    /// overdraw) and post-exit cursor drift. Naive truncate/pad
    /// eliminates the race entirely — the only state the kernel mutates
    /// is the row count and a per-row column truncation/extension; no
    /// cells move between rows.
    ///
    /// This matches xterm, kitty, alacritty, iTerm2, and Windows Terminal
    /// — none of which reflow on resize by default. Reflow of *scrollback*
    /// (when the user pages up after a width change) remains a separate,
    /// deferrable concern that can be addressed without touching the
    /// active viewport.
    ///
    /// Scroll-region preservation rule (unchanged): if the region was the
    /// default full screen before resize (top=0, bottom=rows-1), extend it
    /// to match the new size. Otherwise it's a custom DECSTBM range —
    /// clamp to the new bounds and revert to full if the clamp would
    /// invalidate. Without this, a kernel created at 24 rows then resized
    /// to 26 keeps scroll_bottom=23, leaving rows 24..25 as a frozen
    /// footer; LF at the real bottom never scrolls and scrollback never
    /// grows.
    ///
    /// `primary.saved_cursor` (DECSC'd by `?1049h`) is clamped to the new
    /// bounds inside `naive_resize_screen` so `?1049l` exit lands on a
    /// valid cell. Because no rewrap moves cells, the saved (row, col)
    /// stays semantically anchored to the same prompt line.
    /// Convenience wrapper used by tests and the rare caller that doesn't
    /// know whether an inline TUI is active. Equivalent to
    /// `resize_with_inline_tui(rows, cols, false)`. The production wasm
    /// path goes through `Terminal::resize`, which always supplies the
    /// inline-TUI flag derived from `Grid::is_inline_tui_active_at`.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.resize_with_inline_tui(rows, cols, false);
    }

    /// Resize with explicit inline-TUI awareness. When `inline_tui_active`
    /// is true and we're currently on primary AND dimensions changed, the
    /// visible primary region is fully wiped (§A.3) so an Ink-style app's
    /// SIGWINCH redraw paints onto a clean canvas — the same treatment
    /// alt-screen TUIs already get via §1.22.
    pub fn resize_with_inline_tui(&mut self, rows: usize, cols: usize, inline_tui_active: bool) {
        let old_rows = self.rows;
        let old_cols = self.cols;
        let cols_changed = cols != self.cols;
        let rows_changed = rows != self.rows;
        let dim_changed = cols_changed || rows_changed;

        // §1.25: no reflow ever. Both screens take the naive path.
        Self::naive_resize_screen(&mut self.primary, rows, cols);
        Self::naive_resize_screen(&mut self.alt, rows, cols);
        let branch = ResizeBranch::Naive;

        // §1.22 (2026-05-05): when CURRENTLY viewing alt screen at resize,
        // clear the alt buffer so the application's SIGWINCH-driven redraw
        // lands on a blank canvas. Without this, the OLD layout (cells from
        // before resize, now naively repositioned by truncate/pad) overlaps
        // with the NEW redraw — Claude Code / lazygit / Ink-based CLIs use
        // partial-diff redraws and DON'T necessarily repaint every cell,
        // so the result is "错位行和字符" (offset rows and chars). Native
        // terminal emulators (Windows Terminal, iTerm2) wipe the visible
        // alt-screen on resize for the same reason; this is mainstream.
        //
        // Only fires when (a) the user is currently on alt screen AND
        // (b) dimensions actually changed. No-op resizes (same dims) leave
        // existing content alone. Primary uses naive resize while alt is
        // active (see §1.23 above); reflow deferred to next non-alt resize.
        let wipe_fired = dim_changed && self.is_alt;
        if wipe_fired {
            let bce = self.bce_cell();
            for r in &mut self.alt.rows {
                r.fill_blank(bce);
            }
            self.alt.cursor = Cursor::default();
            self.alt.scroll_top = 0;
            self.alt.scroll_bottom = rows.saturating_sub(1);
        }

        // §A.3 (2026-05-07): inline-TUI primary full wipe. When the
        // foreground app is rendering inline on primary (Ink-based CLIs
        // like Claude Code's input box: cursor hidden + recent absolute-
        // positioning CSI; never enters `?1049h` alt screen), the
        // §1.22-style alt wipe doesn't fire and the §1.26 cursor-row+
        // below cleanup is too narrow — the input box's TOP border
        // typically sits ABOVE the cursor row, so cursor-below cleanup
        // leaves wrapped border garbage on the rows where the user
        // actually sees the broken box.
        //
        // Fix: when CURRENTLY on primary AND dims changed AND the
        // inline-TUI heuristic was true at the moment fitPane sampled
        // it, clear the WHOLE visible primary region (every row), home
        // the cursor, reset the scroll region to full-screen.
        // Scrollback is never touched — the conversation history above
        // the inline TUI lives there and stays intact. Ink's diff
        // redraw on SIGWINCH then paints every cell it cares about
        // against blanks, so any "cell unchanged in Ink's model"
        // optimization can't leave wrapped garbage behind.
        //
        // Mutually exclusive with `cleared_below_cursor` below — when
        // the full wipe fires, the partial cleanup is redundant and
        // skipped. Mutually exclusive with `wipe_fired` (alt path) by
        // the `!self.is_alt` guard.
        let inline_tui_wipe = dim_changed && !self.is_alt && inline_tui_active;
        if inline_tui_wipe {
            // §3 (2026-05-08): narrow the wipe to "from the inline-TUI's
            // top row downward". The original §A.3 implementation cleared
            // the ENTIRE visible primary region — for `claude` (Ink input
            // box at the bottom + multi-line conversation history above),
            // this also blanked the conversation rows. Ink's diff redraw
            // on SIGWINCH only re-emits the input box's own rows, so the
            // conversation history stayed blank until the next scroll —
            // the user-perceptible "已输出内容表现为被截断" symptom.
            //
            // `last_abs_csi_row` is the row index where the most recent
            // absolute-positioning CSI (CUP / HVP / VPA / CHA / HPA) put
            // the cursor. For Ink-based CLIs that's the start of their
            // own frame (log-update writes a final `\x1b[G` after the
            // walk). Clearing only `[abs_row..rows]` preserves rows above
            // (the conversation, prior shell output, etc.) and gives Ink
            // a clean canvas for the rows IT cares about. Cursor goes to
            // (abs_row, 0) so post-resize movement starts where Ink
            // expects it.
            //
            // Fallback: if we have NO recorded absolute-positioning
            // event (cold pane, or the heuristic was driven purely by
            // EL/CUU CSIs without an absolute landing — see §A.4
            // `last_redraw_csi_at_ms`), keep the original full-wipe
            // behaviour. That's correct for the original §A.3 case
            // (lazygit's bottom-of-screen sticky bar, etc.) which
            // doesn't have a stable inline frame top row.
            let last_row_idx = rows.saturating_sub(1);
            let wipe_from_row = if self.last_abs_csi_at_ms != 0 {
                (self.last_abs_csi_row as usize).min(last_row_idx)
            } else {
                0
            };
            let bce = self.bce_cell();
            for r in self.primary.rows.iter_mut().skip(wipe_from_row) {
                r.fill_blank(bce);
            }
            // Preserve current SGR attrs by mutating instead of
            // rebuilding the Cursor struct — avoids the `attr` field
            // resetting to default and breaking colored-prompt apps
            // that mid-frame got a SIGWINCH.
            self.primary.cursor.row = wipe_from_row;
            self.primary.cursor.col = 0;
            self.primary.cursor.pending_wrap = false;
            self.primary.scroll_top = 0;
            self.primary.scroll_bottom = last_row_idx;
        }

        // §1.26 (2026-05-07): primary cursor-row+below cleanup.
        // Symptom: after resizing a primary-screen pane (typical
        // PowerShell + oh-my-posh prompt `<path> > `), the path-to-`>`
        // gap collapses and ghost characters sit past the new prompt's
        // end. Combined with §1.24's silence window, PSReadLine's own
        // SIGWINCH-driven redraw bytes were dropped, so the kernel was
        // left displaying old prompt cells past the new prompt's end.
        //
        // Fix: when CURRENTLY on primary AND dims changed AND no inline
        // TUI was detected (the §A.3 full wipe handles that case more
        // aggressively), blank out:
        //   (a) cursor row, columns `cur_col + 1 .. row_len`;
        //   (b) every row strictly below the cursor.
        // Cells AT cursor.col and to its left are preserved — shells
        // without SIGWINCH-driven full redraws (raw echo loops, Windows
        // cmd.exe) keep the user's typed-but-not-yet-submitted text.
        // PSReadLine / fish-zle / zsh-zle re-emit the full prompt on
        // SIGWINCH; their bytes overwrite the cleared range cleanly
        // once §1.24's (now §A.2-shrunk to 80ms) silence window
        // releases. Rows above the cursor are scrollback / prior
        // command output — never touched here.
        //
        // Alt screen has its own §1.22 wipe; this branch is gated on
        // `!self.is_alt`. `naive_resize_screen` has already clamped
        // `primary.cursor` and resized each row, so `cur_col + 1` and
        // `row.cells.len()` are valid bounds.
        let cleared_below_cursor = dim_changed && !self.is_alt && !inline_tui_wipe;
        if cleared_below_cursor {
            let last_row_idx = rows.saturating_sub(1);
            let last_col_idx = cols.saturating_sub(1);
            let cur_row = self.primary.cursor.row.min(last_row_idx);
            let cur_col = self.primary.cursor.col.min(last_col_idx);
            let bce = self.bce_cell();
            if let Some(r) = self.primary.rows.get_mut(cur_row) {
                let row_len = r.cells.len();
                let start = (cur_col + 1).min(row_len);
                for c in start..row_len {
                    r.cells[c] = bce;
                }
                // Mirror `erase_row_range`'s hyperlink-clipping
                // invariant so OSC 8 underlines don't outlive their
                // cells. (TASKS §1.18.b residue symptom.)
                if !r.hyperlinks.is_empty() {
                    r.hyperlinks.retain(|s| s.col_start < start);
                    for s in &mut r.hyperlinks {
                        if s.col_end > start {
                            s.col_end = start;
                        }
                    }
                }
            }
            for r in self.primary.rows.iter_mut().skip(cur_row + 1) {
                r.fill_blank(bce);
            }
        }

        self.rows = rows;
        self.cols = cols;

        // Diagnostic ring (§1.24, Phase 1.1) — confirms in live repro which
        // branch fired and whether the §1.22 wipe ran. Bounded to
        // RESIZE_DIAG_RING_CAP so a long session can't grow this unbounded.
        if self.last_resizes.len() == RESIZE_DIAG_RING_CAP {
            self.last_resizes.remove(0);
        }
        self.last_resizes.push(ResizeDiag {
            old_rows,
            old_cols,
            new_rows: rows,
            new_cols: cols,
            is_alt: self.is_alt,
            dim_changed,
            branch,
            wipe_fired,
            cleared_below_cursor,
            inline_tui_wipe,
            inline_tui_active,
        });
    }

    /// Existing truncate/pad behavior, factored out so `resize()` can pick
    /// per-screen behavior. Used by alt screen unconditionally and by primary
    /// when only the row count changed.
    fn naive_resize_screen(screen: &mut Screen, rows: usize, cols: usize) {
        let old_last = screen.rows.len().saturating_sub(1);
        let region_was_full = screen.scroll_top == 0 && screen.scroll_bottom == old_last;

        for r in &mut screen.rows {
            r.resize(cols);
        }
        if rows < screen.rows.len() {
            screen.rows.truncate(rows);
        } else {
            while screen.rows.len() < rows {
                screen.rows.push(Row::new(cols));
            }
        }
        let last = rows.saturating_sub(1);
        let last_col = cols.saturating_sub(1);
        screen.cursor.row = screen.cursor.row.min(last);
        screen.cursor.col = screen.cursor.col.min(last_col);
        screen.cursor.pending_wrap = false;

        // Clamp saved_cursor too — without this, a `?1049h` saved row may
        // sit past the new bottom (or saved col past the new right) and
        // the eventual `?1049l` DECRC would land out-of-bounds. Active
        // screen here: alt screen always naive-resizes, primary naive-
        // resizes while alt is active (§1.23 in `resize`).
        if let Some(s) = screen.saved_cursor.as_mut() {
            s.row = s.row.min(last);
            s.col = s.col.min(last_col);
            if s.col < last_col {
                // pending_wrap is only meaningful when parked at the
                // rightmost column; clamping inward invalidates it.
                s.pending_wrap = false;
            }
        }

        if region_was_full {
            screen.scroll_top = 0;
            screen.scroll_bottom = last;
        } else {
            screen.scroll_top = screen.scroll_top.min(last);
            screen.scroll_bottom = screen.scroll_bottom.min(last);
            if screen.scroll_top >= screen.scroll_bottom {
                screen.scroll_top = 0;
                screen.scroll_bottom = last;
            }
        }
    }

    // ------------------------------------------------------------------
    // Printing
    // ------------------------------------------------------------------

    /// Place one printable char at the cursor, advancing it.
    /// See cursor.rs for the DECAWM `pending_wrap` rationale.
    pub fn print(&mut self, ch: char, attrs: Attrs) {
        let w = wcwidth(ch as u32);
        if w == 0 {
            // Combining: best-effort attach to previous cell. Real grapheme
            // cluster support is a larger refactor (cell holds a SmallStr).
            // Leaving the simple fallback so combining marks don't advance
            // the cursor.
            return;
        }

        let attr_id = self.attrs.intern(attrs);
        let cols = self.cols;
        let scroll_top = self.screen().scroll_top;
        let scroll_bottom = self.screen().scroll_bottom;

        // Resolve pending wrap from the previous print.
        if self.screen().cursor.pending_wrap {
            self.screen_mut().cursor.pending_wrap = false;
            // Mark wrapped so reflow/copy can stitch the lines back.
            let row = self.screen().cursor.row;
            self.screen_mut().rows[row].wrapped = true;
            self.screen_mut().cursor.col = 0;
            self.linefeed();
        }

        // Wide char that won't fit: write a blank in the last column,
        // wrap, then print on the next line.
        if w == 2 && self.screen().cursor.col + 1 >= cols {
            let cur = self.screen().cursor;
            if cur.col < cols {
                self.screen_mut().rows[cur.row].cells[cur.col] = Cell::new(' ', attr_id, 1);
            }
            self.screen_mut().rows[cur.row].wrapped = true;
            self.screen_mut().cursor.col = 0;
            self.linefeed();
        }

        // §1.28 (2026-05-07): keep wide-cell pair integrity on overwrite.
        //
        // A wide char occupies two cells: a main at col (width=2) and a
        // continuation at col+1 (width=0). Either side surviving without
        // its partner is an orphan, and the renderer / overwrite logic
        // both mishandle orphans:
        //
        //   - Renderer skips width==0 cells, so an orphan continuation
        //     just looks like a blank, but the *next* narrow write to
        //     that column triggers the "I see a width==0 here, clear
        //     the main at col-1" branch below — which then wipes a
        //     freshly-written narrow char a column to the left. That's
        //     the chain Ink's frame-redraw triggers: 中 → narrow over
        //     col=2 → orphan continuation at col=3 → next narrow at
        //     col=3 deletes the col=2 narrow we just wrote. Same root
        //     cause behind "中文字符只渲染一半", "字符消失只剩占位",
        //     and "改色文本多余字符" symptoms during `claude` runs.
        //
        // Two symmetric pre-write guards tear both halves down in lock
        // step so we never leave an orphan:
        let cur_col = self.screen().cursor.col;
        let cur_row = self.screen().cursor.row;
        if cur_col < cols {
            let here = self.screen().rows[cur_row].cells[cur_col];
            // (a) writing onto a continuation → clear the prior main.
            //     §B.2 (2026-05-08): also drop any cluster sidecar
            //     anchored at the orphaned main col. Without this a
            //     multi-codepoint cluster (👨‍👩‍👧, 🏳️‍🌈) survives the
            //     overwrite as a stale sidecar pointing at a now-
            //     replaced (' ', w=1) cell, and the renderer paints the
            //     cluster's full emoji glyph over what should now be a
            //     blank space — the user-visible "退格一次出现乱码字符"
            //     symptom: shell echoes BS+SP+BS to erase a wide cluster,
            //     SP lands on the continuation, branch (a) clears the
            //     main, but the cluster sidecar persists and the
            //     renderer keeps painting the original emoji on top of
            //     the now-' '-cell.
            if here.width == 0 && cur_col > 0 {
                self.screen_mut().rows[cur_row].clear_cluster_at(cur_col - 1);
                self.screen_mut().rows[cur_row].cells[cur_col - 1] =
                    Cell::new(' ', AttrId::DEFAULT, 1);
            }
            // (b) writing onto a main → clear the trailing continuation.
            //     §B.2: same cluster-sidecar invariant as (a). The cell
            //     at cur_col itself will be overwritten by the actual
            //     `print` below (which already calls `clear_cluster_at`),
            //     so we only need to wipe the sidecar at cur_col+1 if
            //     the existing main carried a cluster — but cluster
            //     sidecars are anchored at the MAIN col only, never the
            //     continuation. So no extra clear_cluster_at(cur_col+1)
            //     needed; the trailing-continuation cell never owns a
            //     sidecar by construction.
            if here.width == 2 && cur_col + 1 < cols {
                self.screen_mut().rows[cur_row].cells[cur_col + 1] =
                    Cell::new(' ', AttrId::DEFAULT, 1);
            }
        }
        // (c) wide writes only: the spacer we'll lay at cur_col+1 might
        //     itself land on a different pair's main — orphan its
        //     continuation at cur_col+2.
        //     §B.2: drop the orphaned main's cluster sidecar at
        //     cur_col+1 so it doesn't outlive the wide-cell write that
        //     overwrites it.
        if w == 2 {
            let nxt = cur_col + 1;
            if nxt < cols {
                let next_cell = self.screen().rows[cur_row].cells[nxt];
                if next_cell.width == 2 && nxt + 1 < cols {
                    self.screen_mut().rows[cur_row].clear_cluster_at(nxt);
                    self.screen_mut().rows[cur_row].cells[nxt + 1] =
                        Cell::new(' ', AttrId::DEFAULT, 1);
                }
            }
        }

        // Place the cell(s). §4.7: also drop any stale ClusterSpan
        // anchored at the col we're about to overwrite — single-char
        // writes must not leave a previous multi-codepoint cluster's
        // sidecar pointing at a now-mismatched cell.
        let row_idx = self.screen().cursor.row;
        if w == 2 {
            let col = self.screen().cursor.col;
            self.screen_mut().rows[row_idx].clear_cluster_at(col);
            self.screen_mut().rows[row_idx].cells[col] = Cell::new(ch, attr_id, 2);
            self.screen_mut().rows[row_idx].cells[col + 1] = Cell::wide_spacer(attr_id);
            self.screen_mut().cursor.col += 2;
        } else {
            let col = self.screen().cursor.col;
            self.screen_mut().rows[row_idx].clear_cluster_at(col);
            self.screen_mut().rows[row_idx].cells[col] = Cell::new(ch, attr_id, 1);
            self.screen_mut().cursor.col += 1;
        }

        // Don't advance past the rightmost column — set pending_wrap and
        // sit on cols-1. The next printable char will resolve it.
        if self.screen().cursor.col >= cols {
            self.screen_mut().cursor.col = cols - 1;
            self.screen_mut().cursor.pending_wrap = true;
        }

        // Silence unused warnings — these will be consumed when we
        // implement region-aware operations next round.
        let _ = (scroll_top, scroll_bottom);
    }

    /// §4.7 (2026-05-07) — print one extended grapheme cluster as a
    /// single visual unit. Called by the parser AFTER it segments the
    /// incoming byte stream into clusters via `unicode-segmentation`.
    ///
    /// Single-codepoint clusters fast-path through `print(ch, attrs)` —
    /// no sidecar entry, no Box allocation — so ASCII / CJK output
    /// keeps its existing zero-overhead path.
    ///
    /// Multi-codepoint clusters (👨‍👩‍👧, 🏳️‍🌈, 🇺🇸, 👨‍💻):
    ///   1. Compute visual width from the whole cluster
    ///      (`wcwidth_grapheme` accounts for ZWJ → 0, RIS pairs → 2).
    ///   2. Place the FIRST codepoint via `print(first, attrs)` so all
    ///      the wrap / pending_wrap / wide-spacer bookkeeping stays in
    ///      one place. The cell at that col carries the first codepoint
    ///      as `cell.ch` (so per-cell hashing / search / selection
    ///      still see *some* glyph).
    ///   3. If the cluster's visual width disagrees with the first
    ///      codepoint's wcwidth (e.g. RIS pair: each is wcwidth=1 but
    ///      together they're width=2), patch the cell's width and the
    ///      cursor so subsequent prints land at the right col.
    ///   4. Register the full cluster string on the row's `clusters`
    ///      sidecar at the placement col so renderers paint the
    ///      cluster glyph instead of just the first codepoint.
    ///
    /// Whole-cluster zero-width strings (rare — combining-only input
    /// like a stray ZWJ) fall back to `print(first, attrs)` which itself
    /// short-circuits on width-0.
    pub fn print_grapheme(&mut self, s: &str, attrs: Attrs) {
        let mut chars = s.chars();
        let Some(first) = chars.next() else {
            return;
        };
        let multi = chars.next().is_some();

        if !multi {
            self.print(first, attrs);
            return;
        }

        let cluster_w = wcwidth_grapheme(s);
        if cluster_w == 0 {
            self.print(first, attrs);
            return;
        }

        // Place first codepoint via the existing path. After the call,
        // the cursor has advanced and `pending_wrap` may be set.
        self.print(first, attrs);

        // Compute the col where the cell was actually written. After
        // print(), cursor sits at `written_col + first_w` (or stays
        // at cols-1 with pending_wrap when first_w==1 hit the right
        // edge).
        let cur = *self.cursor();
        let row_idx = cur.row;
        let first_w = wcwidth(first as u32);
        let written_col = if cur.pending_wrap {
            cur.col
        } else {
            cur.col.saturating_sub(first_w as usize)
        };

        // Patch cell width if the cluster's visual width differs from
        // the first codepoint's wcwidth — RIS pair is the canonical
        // case (first RIS is wcwidth=1, pair renders at width 2). We
        // only widen (1 → 2), never narrow (renderer can paint a
        // cluster that's "smaller than declared" cleanly; the reverse
        // would clip).
        if cluster_w == 2 && first_w == 1 {
            let cols = self.cols;
            let row_len = self.screen().rows[row_idx].cells.len();
            if written_col + 1 < row_len {
                let attr_id = self.attrs.intern(attrs);
                self.screen_mut().rows[row_idx].cells[written_col] = Cell::new(first, attr_id, 2);
                self.screen_mut().rows[row_idx].cells[written_col + 1] = Cell::wide_spacer(attr_id);
                // Advance cursor by the extra column claimed by the
                // upgraded width-2 placement, mirroring the wide-char
                // path in `print`.
                if !cur.pending_wrap {
                    self.screen_mut().cursor.col = (cur.col + 1).min(cols.saturating_sub(1));
                    if self.screen().cursor.col + 1 >= cols {
                        self.screen_mut().cursor.col = cols.saturating_sub(1);
                        self.screen_mut().cursor.pending_wrap = true;
                    }
                }
            }
        }

        // Register the cluster on the row sidecar.
        let row_len = self.screen().rows[row_idx].cells.len();
        if written_col < row_len {
            self.screen_mut().rows[row_idx].set_cluster(written_col, Box::from(s));
        }
    }

    // ------------------------------------------------------------------
    // Cursor motion
    // ------------------------------------------------------------------

    pub fn carriage_return(&mut self) {
        let cur = self.cursor_mut();
        cur.col = 0;
        cur.pending_wrap = false;
    }

    /// LF / IND. Move down one row; if at the bottom of the *scroll region*,
    /// scroll the region (which may push to scrollback when region is full).
    pub fn linefeed(&mut self) {
        let scr = self.screen();
        if scr.cursor.row == scr.scroll_bottom {
            self.scroll_region_up(1);
        } else if scr.cursor.row + 1 < self.rows {
            self.cursor_mut().row += 1;
        }
        // else cursor is below scroll region — clamp to last row, no scroll.
        self.cursor_mut().pending_wrap = false;
    }

    pub fn backspace(&mut self) {
        let cur = self.cursor_mut();
        if cur.col > 0 {
            cur.col -= 1;
        }
        cur.pending_wrap = false;
        // §B.4 (2026-05-08) — placeholder normalization. A wide cell
        // occupies two grid slots: the MAIN at col N (width=2) and a
        // CONTINUATION at col N+1 (width=0). After BS over a wide
        // pair, the strict VT contract leaves the cursor on the
        // continuation — but no shell / line editor expects to write
        // INTO the middle of a wide character, so the placeholder is
        // a meaningless cursor position visually (it appears to sit
        // ON TOP of the right half of the wide glyph). Modern
        // terminals (Windows Terminal, iTerm2, Konsole) normalize
        // this by skipping past the continuation to the main.
        //
        // Without this, PSReadLine / readline / Ink-style editors
        // that send only `BS` (without a follow-up `SP BS` overwrite
        // pair) for delete-char on a wide grapheme leave the user
        // staring at an unaltered emoji with the cursor blinking on
        // its right half — the user-reported "退格一次出现乱码字符,
        // 退格两次才彻底清除" symptom on 🎂.
        //
        // Normalize in a separate read-after-write so we don't
        // accidentally cross a screen boundary; bounded to one extra
        // step (placeholder is exactly 1 cell wide by construction,
        // so we never need to skip more than once).
        let cur_col = self.screen().cursor.col;
        let cur_row = self.screen().cursor.row;
        if cur_col > 0 {
            let lands_on_placeholder = self
                .screen()
                .rows
                .get(cur_row)
                .and_then(|r| r.cells.get(cur_col))
                .map(|c| c.width == 0)
                .unwrap_or(false);
            if lands_on_placeholder {
                self.cursor_mut().col -= 1;
            }
        }
    }

    pub fn tab(&mut self) {
        let cols = self.cols;
        let cur = self.cursor_mut();
        let next = ((cur.col / 8) + 1) * 8;
        cur.col = next.min(cols.saturating_sub(1));
        cur.pending_wrap = false;
    }

    /// CBT — cursor backward by `n` tab stops. Tab stops are the default
    /// every-8-columns set (HTS/TBC for custom stops not yet modelled).
    /// At each step: if already on a tab stop (col % 8 == 0), back up
    /// to the previous one (col - 8); otherwise round down to the
    /// containing tab stop. Clamps at column 0 — never wraps to a
    /// negative column.
    pub fn cursor_back_tab(&mut self, n: usize) {
        let cur = self.cursor_mut();
        let mut col = cur.col;
        for _ in 0..n {
            if col == 0 {
                break;
            }
            col = ((col - 1) / 8) * 8;
        }
        cur.col = col;
        cur.pending_wrap = false;
    }

    pub fn cursor_to(&mut self, row: usize, col: usize) {
        let last_row = self.rows.saturating_sub(1);
        let last_col = self.cols.saturating_sub(1);
        let cur = self.cursor_mut();
        cur.row = row.min(last_row);
        cur.col = col.min(last_col);
        cur.pending_wrap = false;
        // §B.11 (2026-05-08) — placeholder normalization on absolute
        // positioning paths. CSI CHA / CUP / HVP / VPA all funnel
        // through `cursor_to`, so a shell that emits `CSI <col>H`
        // pointing at the continuation half (width=0) of a wide cell
        // would land the cursor on a meaningless position. The next
        // print would trigger §1.28 branch (a) which clears the
        // orphan main, replacing the wide cell's main glyph with
        // ' ' before the new char overwrites the placeholder slot —
        // the user-reported "退格出现乱码" pattern when shell uses
        // CSI positioning rather than BS+SP+BS.
        //
        // Step BACK one cell (to the main col) when we land on a
        // placeholder. Same convention as `backspace` and
        // `cursor_left` (§B.4 / §B.5). Forward-step would also work
        // but introduces unbounded skip in pathological rows;
        // backward is bounded to ≤1 cell.
        let cur_col = self.screen().cursor.col;
        let cur_row = self.screen().cursor.row;
        if cur_col > 0 {
            let on_placeholder = self
                .screen()
                .rows
                .get(cur_row)
                .and_then(|r| r.cells.get(cur_col))
                .map(|c| c.width == 0)
                .unwrap_or(false);
            if on_placeholder {
                self.cursor_mut().col -= 1;
            }
        }
    }

    pub fn cursor_up(&mut self, n: usize) {
        // Cursor up obeys the scroll region: it doesn't go above scroll_top
        // when the cursor was already inside the region.
        let scr = self.screen();
        let limit = if scr.cursor.row >= scr.scroll_top {
            scr.scroll_top
        } else {
            0
        };
        let new_row = scr.cursor.row.saturating_sub(n).max(limit);
        let cur = self.cursor_mut();
        cur.row = new_row;
        cur.pending_wrap = false;
    }

    pub fn cursor_down(&mut self, n: usize) {
        let scr = self.screen();
        let last = self.rows.saturating_sub(1);
        let limit = if scr.cursor.row <= scr.scroll_bottom {
            scr.scroll_bottom
        } else {
            last
        };
        let new_row = (scr.cursor.row + n).min(limit);
        let cur = self.cursor_mut();
        cur.row = new_row;
        cur.pending_wrap = false;
    }

    pub fn cursor_left(&mut self, n: usize) {
        let cur = self.cursor_mut();
        cur.col = cur.col.saturating_sub(n);
        cur.pending_wrap = false;
        // §B.5 (2026-05-08) — placeholder normalization, same as
        // `backspace`. CSI nD (CUB) is the relative-left counterpart
        // of BS at the parser level; both must agree on what "land
        // on placeholder" means visually so PSReadLine / readline
        // editors see consistent behaviour whether they pick the C0
        // BS byte or the CSI form.
        let cur_col = self.screen().cursor.col;
        let cur_row = self.screen().cursor.row;
        if cur_col > 0 {
            let on_placeholder = self
                .screen()
                .rows
                .get(cur_row)
                .and_then(|r| r.cells.get(cur_col))
                .map(|c| c.width == 0)
                .unwrap_or(false);
            if on_placeholder {
                self.cursor_mut().col -= 1;
            }
        }
    }

    pub fn cursor_right(&mut self, n: usize) {
        let last_col = self.cols.saturating_sub(1);
        let cur = self.cursor_mut();
        cur.col = (cur.col + n).min(last_col);
        cur.pending_wrap = false;
    }

    // ------------------------------------------------------------------
    // Erase
    // ------------------------------------------------------------------

    pub fn erase_in_display(&mut self, mode: EraseMode) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        let total_rows = self.rows;
        let bce = self.bce_cell();
        match mode {
            EraseMode::Below => {
                self.erase_row_range(cur_row, cur_col, cols);
                for r in (cur_row + 1)..total_rows {
                    self.screen_mut().rows[r].fill_blank(bce);
                }
            }
            EraseMode::Above => {
                for r in 0..cur_row {
                    self.screen_mut().rows[r].fill_blank(bce);
                }
                self.erase_row_range(cur_row, 0, cur_col + 1);
            }
            EraseMode::All => {
                for r in &mut self.screen_mut().rows {
                    r.fill_blank(bce);
                }
            }
            EraseMode::SavedLines => {
                // §B.2 (2026-05-08) — xterm `CSI 3 J` extension. Drops
                // the entire scrollback ring buffer (physical clear:
                // every `Vec<Option<Row>>` slot back to None, head/len
                // reset to 0). Visible grid stays untouched, cursor
                // stays put — this is the operation that makes a "real"
                // clear actually clear: after this call both `clear`
                // (`\x1b[2J\x1b[H`) AND scrollback are gone, so the
                // user's pgup history doesn't resurrect what they just
                // wiped.
                //
                // No-op on the alt screen — alt screen has no
                // scrollback to begin with, and TUI apps that swap
                // back to primary expect their preserved scrollback
                // intact (kakoune / vim / less depend on this).
                if !self.is_alt {
                    self.scrollback.clear();
                }
            }
        }
    }

    pub fn erase_in_line(&mut self, mode: EraseMode) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        match mode {
            EraseMode::Below => self.erase_row_range(cur_row, cur_col, cols),
            EraseMode::Above => self.erase_row_range(cur_row, 0, cur_col + 1),
            EraseMode::All => self.erase_row_range(cur_row, 0, cols),
            // EL has no semantic for "saved lines" — `CSI 3 K` is
            // unspecified by xterm. Treat as no-op to match xterm's
            // silent ignore (and avoid surprising side effects on
            // shells that emit it by accident). EL is ROW-scoped; it
            // never touched scrollback in any spec.
            EraseMode::SavedLines => {}
        }
    }

    fn erase_row_range(&mut self, row: usize, start: usize, end: usize) {
        let bce = self.bce_cell();
        if let Some(r) = self.screen_mut().rows.get_mut(row) {
            let clamped_end = end.min(r.cells.len());
            // §1.28: orphan-clear any wide-pair half whose partner falls
            // outside the erase range. Done BEFORE the wipe loop so the
            // outside-of-range partner is normalized while the in-range
            // half still carries its width marker for the boundary check.
            clip_wide_pair_at_range_boundaries(r, start, clamped_end);
            for c in start..clamped_end {
                r.cells[c] = bce;
            }
            // §B.2 (2026-05-08): drop every cluster sidecar whose anchor
            // col is in the erased range. Without this, ED/EL leaves
            // the multi-codepoint cluster strings dangling on now-EMPTY
            // cells, and a future re-print at the same col without
            // setting a sidecar would let the renderer find the stale
            // cluster and paint the original emoji over the new char.
            r.clear_clusters_in_range(start, clamped_end);
            // OSC 8 hyperlink spans must be kept in sync with the cells
            // they describe. Without this, CSI K / CSI J erase paths
            // wipe the cells but leave the span — and the renderer's
            // hyperlink-underline pass then paints an underline under
            // empty cells. Claude Code emits these heavily for status
            // redraws (TASKS §1.18.b residue symptom).
            clip_hyperlinks_around(&mut r.hyperlinks, start, clamped_end);
        }
    }

    // ------------------------------------------------------------------
    // In-line cell editing (ECH / ICH / DCH)
    //
    // These three are how line editors (PSReadLine, readline) and TUI
    // libraries (Ink, ratatui, blessed) do *partial* row updates without
    // redrawing the whole screen. Without them, Ink's frame N+1 ECH that
    // was supposed to wipe frame N's old characters is silently dropped
    // and the old text shows through behind the new text — the visible
    // "character residue" symptom.
    // ------------------------------------------------------------------

    /// ECH `CSI <n> X` — erase N cells starting at the cursor, replace
    /// with blanks. Cursor does NOT move. Cells past the right margin
    /// are clamped (no row spill).
    pub fn erase_chars(&mut self, n: usize) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        let end = (cur_col + n).min(cols);
        let bce = self.bce_cell();
        if let Some(r) = self.screen_mut().rows.get_mut(cur_row) {
            let clamped_end = end.min(r.cells.len());
            // §1.28: same wide-pair boundary guard as erase_row_range.
            clip_wide_pair_at_range_boundaries(r, cur_col, clamped_end);
            for c in cur_col..clamped_end {
                r.cells[c] = bce;
            }
            // §B.2 — drop cluster sidecars in the erased range.
            r.clear_clusters_in_range(cur_col, clamped_end);
            // Same hyperlink-clipping invariant as `erase_row_range`:
            // ECH wipes cells, so any span overlapping the cleared
            // range must be clipped or dropped. (TASKS §1.18.b.)
            clip_hyperlinks_around(&mut r.hyperlinks, cur_col, clamped_end);
        }
        // ECH explicitly clears pending_wrap per xterm spec.
        self.cursor_mut().pending_wrap = false;
    }

    /// ICH `CSI <n> @` — insert N blank cells at the cursor, shifting
    /// the rest of the row right. Cells pushed past the right margin
    /// are dropped. Cursor does NOT move.
    pub fn insert_chars(&mut self, n: usize) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        let bce = self.bce_cell();
        if let Some(r) = self.screen_mut().rows.get_mut(cur_row) {
            let n = n.min(cols.saturating_sub(cur_col));
            if n == 0 {
                return;
            }
            // §1.28: if the cut point splits a wide pair (cells[cur_col]
            // is a continuation whose main lives at cur_col-1), the
            // shift would leave the main orphaned with `n` blanks
            // between it and a now-displaced continuation. Clear both
            // halves before shifting.
            if cur_col > 0
                && cur_col < r.cells.len()
                && r.cells[cur_col].width == 0
                && r.cells[cur_col - 1].width == 2
            {
                r.cells[cur_col - 1] = Cell::EMPTY;
                r.cells[cur_col] = Cell::EMPTY;
                // §B.2 — drop cluster sidecar at the orphaned main.
                r.clear_cluster_at(cur_col - 1);
            }
            // §1.28: pairs near the right margin that the shift would
            // push partly off the row also need their inside half
            // cleared so an orphan main doesn't land at cells[cols-1].
            if cols > n
                && cols >= n + 1
                && cols - n - 1 < r.cells.len()
                && cols - n < r.cells.len()
                && r.cells[cols - n - 1].width == 2
                && r.cells[cols - n].width == 0
            {
                r.cells[cols - n - 1] = Cell::EMPTY;
                r.cells[cols - n] = Cell::EMPTY;
                // §B.2 — orphan main at cols-n-1 dropped its cluster
                // sidecar too. (Continuation at cols-n never carried
                // a sidecar by construction.)
                r.clear_cluster_at(cols - n - 1);
            }
            // §B.2 — shift cluster sidecars at col ≥ cur_col RIGHT by n,
            // dropping any that would land at col ≥ cols. Performed
            // BEFORE the cell shift so the sidecar's pre-shift cols
            // are still meaningful when matched against the cells they
            // describe. The cells_at_split orphan-clear above already
            // dropped sidecars that would otherwise be moved to cols-1
            // (an orphan-main slot).
            r.shift_clusters_right(cur_col, n, cols);
            // Shift right-of-cursor cells right by n; cells falling off are dropped.
            // Walk from the right edge inward to avoid overwriting source cells.
            for dst in (cur_col + n..cols).rev() {
                let src = dst - n;
                if src < r.cells.len() && dst < r.cells.len() {
                    r.cells[dst] = r.cells[src];
                }
            }
            for c in cur_col..(cur_col + n).min(r.cells.len()) {
                r.cells[c] = bce;
            }
            // Hyperlink spans straddling or after the cursor get
            // invalidated. Line-edit operations (PSReadLine / readline /
            // Claude Code prompt edits) shift cell content but the
            // visible label of any hyperlink no longer corresponds to
            // its original click target — drop spans that overlap or
            // extend past the edit point. Matches xterm's "edit
            // invalidates the link" UX. (TASKS §1.18.b extension.)
            r.hyperlinks.retain(|span| span.col_end <= cur_col);
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// Mark a printed cell as part of an OSC 8 hyperlink span. Coalesces
    /// with the trailing span on the same row when uri+id match and the
    /// new cell starts exactly where the previous span ended — so writing
    /// "hello" inside one OSC 8 produces ONE span, not five.
    pub fn annotate_cell_with_link(
        &mut self,
        row: usize,
        col: usize,
        width: usize,
        uri: &str,
        id: Option<&str>,
    ) {
        let Some(r) = self.screen_mut().rows.get_mut(row) else {
            return;
        };
        let end = col + width.max(1);
        if let Some(last) = r.hyperlinks.last_mut() {
            let id_match = match (&last.id, id) {
                (None, None) => true,
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if last.col_end == col && last.uri == uri && id_match {
                last.col_end = end;
                return;
            }
        }
        r.hyperlinks.push(super::cell::HyperlinkSpan {
            col_start: col,
            col_end: end,
            uri: uri.to_string(),
            id: id.map(|s| s.to_string()),
        });
    }

    /// DCH `CSI <n> P` — delete N cells at the cursor, shifting the
    /// rest of the row left. Blanks fill from the right margin. Cursor
    /// does NOT move.
    pub fn delete_chars(&mut self, n: usize) {
        let cur_row = self.screen().cursor.row;
        let cur_col = self.screen().cursor.col;
        let cols = self.cols;
        let bce = self.bce_cell();
        if let Some(r) = self.screen_mut().rows.get_mut(cur_row) {
            let n = n.min(cols.saturating_sub(cur_col));
            if n == 0 {
                return;
            }
            // §1.28: clip wide pairs at the deletion range boundaries
            // so we don't leave orphans after the shift. Range is
            // [cur_col, cur_col + n).
            clip_wide_pair_at_range_boundaries(r, cur_col, cur_col + n);
            // §B.2 — drop cluster sidecars in the to-be-deleted range
            // BEFORE the shift, then shift remaining sidecars left.
            // Order matters: clearing first means the shift never has
            // to consider sidecars that were inside the range.
            r.clear_clusters_in_range(cur_col, cur_col + n);
            r.shift_clusters_left(cur_col + n, n);
            // Shift left.
            for dst in cur_col..(cols - n) {
                let src = dst + n;
                if src < r.cells.len() && dst < r.cells.len() {
                    r.cells[dst] = r.cells[src];
                }
            }
            // Fill the right side with blanks.
            for c in (cols - n)..cols.min(r.cells.len()) {
                r.cells[c] = bce;
            }
            // Drop any hyperlink span overlapping or after the cursor
            // — see ICH for rationale. (TASKS §1.18.b extension.)
            r.hyperlinks.retain(|span| span.col_end <= cur_col);
        }
        self.cursor_mut().pending_wrap = false;
    }

    // ------------------------------------------------------------------
    // Scroll (region-aware)
    // ------------------------------------------------------------------

    /// Internal: scroll the active screen's scroll region up by `n` rows.
    /// New blank rows appear at scroll_bottom; rows leaving scroll_top
    /// enter scrollback ONLY if (a) we're on the primary screen AND
    /// (b) the region covers the entire screen.
    fn scroll_region_up(&mut self, n: usize) {
        let bce = self.bce_cell();
        let scr = self.screen();
        let top = scr.scroll_top;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - top + 1;
        let n = n.min(region_h);
        let push_to_scrollback = !self.is_alt && scr.is_full_region();
        let cols = self.cols;

        for _ in 0..n {
            // Pull the top row out — its allocation goes either to scrollback
            // (and recycles back as the new bottom) or to the new bottom
            // directly (alt screen / partial region: no scrollback push).
            let evicted_top = self.screen_mut().rows.remove(top);

            // The new bottom row: prefer recycling an evicted scrollback row.
            let new_bottom = if push_to_scrollback {
                match self.scrollback.push(evicted_top) {
                    Some(mut recycled) => {
                        recycled.fill_blank(bce);
                        recycled.resize(cols);
                        recycled
                    }
                    None => {
                        let mut row = Row::new(cols);
                        row.fill_blank(bce);
                        row
                    }
                }
            } else {
                // Reuse the dropped row's allocation directly: clear and place
                // it at the bottom. This keeps alloc count flat per scroll.
                let mut row = evicted_top;
                row.fill_blank(bce);
                row.resize(cols);
                row
            };

            // Insert the new blank at `bottom`. Because we just removed at
            // `top`, the indices [top..bottom-1] shifted down by one — so
            // inserting at `bottom` puts it right after the last region row.
            self.screen_mut().rows.insert(bottom, new_bottom);
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// Internal: scroll the active region down by `n` rows. New blank rows
    /// at scroll_top, rows leaving scroll_bottom dropped (no scrollback).
    fn scroll_region_down(&mut self, n: usize) {
        let bce = self.bce_cell();
        let scr = self.screen();
        let top = scr.scroll_top;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - top + 1;
        let n = n.min(region_h);
        let cols = self.cols;

        for _ in 0..n {
            // Drop the bottom row, recycle its allocation as the new top.
            let mut recycled = self.screen_mut().rows.remove(bottom);
            recycled.fill_blank(bce);
            recycled.resize(cols);
            self.screen_mut().rows.insert(top, recycled);
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// CSI S — scroll up. Operates on the scroll region.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_region_up(n);
    }

    /// CSI T / RI — scroll down (reverse linefeed). Operates on the scroll region.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_region_down(n);
    }

    /// RI (ESC M): reverse linefeed. If at scroll_top, scrolls the region
    /// down; otherwise just moves cursor up.
    pub fn reverse_linefeed(&mut self) {
        let scr = self.screen();
        if scr.cursor.row == scr.scroll_top {
            self.scroll_region_down(1);
        } else if scr.cursor.row > 0 {
            self.cursor_mut().row -= 1;
        }
        self.cursor_mut().pending_wrap = false;
    }

    /// CSI L — insert blank lines at cursor row, pushing rows down within
    /// the scroll region. No-op if cursor is outside the region.
    pub fn insert_lines(&mut self, n: usize) {
        let scr = self.screen();
        if scr.cursor.row < scr.scroll_top || scr.cursor.row > scr.scroll_bottom {
            return;
        }
        let cur = scr.cursor.row;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - cur + 1;
        let n = n.min(region_h);
        let cols = self.cols;
        let bce = self.bce_cell();
        for _ in 0..n {
            // Drop the row at `bottom`, recycle its allocation as the new
            // blank inserted at `cur`. Net: rows[cur..bottom] shift down by 1.
            let mut recycled = self.screen_mut().rows.remove(bottom);
            recycled.fill_blank(bce);
            recycled.resize(cols);
            self.screen_mut().rows.insert(cur, recycled);
        }
        let cur_mut = self.cursor_mut();
        cur_mut.col = 0;
        cur_mut.pending_wrap = false;
    }

    /// CSI M — delete lines at cursor row, pulling rows up within the
    /// scroll region. No-op if cursor is outside the region.
    pub fn delete_lines(&mut self, n: usize) {
        let scr = self.screen();
        if scr.cursor.row < scr.scroll_top || scr.cursor.row > scr.scroll_bottom {
            return;
        }
        let cur = scr.cursor.row;
        let bottom = scr.scroll_bottom;
        let region_h = bottom - cur + 1;
        let n = n.min(region_h);
        let cols = self.cols;
        let bce = self.bce_cell();
        for _ in 0..n {
            // Remove the row at `cur`, recycle as new blank at `bottom`.
            let mut recycled = self.screen_mut().rows.remove(cur);
            recycled.fill_blank(bce);
            recycled.resize(cols);
            self.screen_mut().rows.insert(bottom, recycled);
        }
        let cur_mut = self.cursor_mut();
        cur_mut.col = 0;
        cur_mut.pending_wrap = false;
    }
}

/// Clip OSC 8 hyperlink spans on a row so they no longer cover cells in
/// the just-erased `[start, end)` column range.
///
/// Per TASKS §1.18.b, the partial-erase paths (`CSI K` line erase,
/// `CSI J` cursor-relative display erase, `CSI X` ECH) used to leave
/// hyperlink spans untouched while wiping the underlying cells. The
/// renderer's hyperlink-underline pass then drew an underline under
/// blank cells, producing the "leftover residue" the user reported in
/// Claude Code output (which uses these escapes heavily for status-line
/// redraws).
///
/// Cases:
///   - span entirely outside `[start, end)` → keep
///   - span entirely inside `[start, end)` → drop
///   - erase wipes span tail (span.col_start < start && span.col_end <= end) → clip end to start
///   - erase wipes span head (span.col_start >= start && span.col_end > end) → clip start to end
///   - erase punches a hole in the middle of a span (span.col_start < start && span.col_end > end)
///     → drop the entire span. We can't split into two without growing the Vec
///     mid-`retain`; the surviving prefix and suffix become unlinked, which
///     matches xterm's "erase invalidates the link" UX (the user can re-emit
///     OSC 8 to restore it). This is rare in practice — partial-erase usually
///     covers a whole word or label.
/// §1.28 (2026-05-07): when an erase / shift range `[start, end)` cuts
/// through the middle of a wide-cell pair (main at width=2, continuation
/// at width=0), the half that lives OUTSIDE the range becomes an orphan.
/// This helper clears the outside half so the pair invariant survives.
///
/// §B.2 (2026-05-08) — upgraded from `&mut [Cell]` to `&mut Row` so the
/// orphan-clearing path can ALSO drop the cluster sidecar at the
/// orphan main col. Without this, a wide cluster (👨‍👩‍👧, 🇺🇸)
/// straddled by an erase range survived as a stale sidecar that the
/// renderer kept painting on top of the now-blank cell.
///
/// Called by EL / ECH / ICH / DCH — every cell-edit op whose range can
/// straddle a wide pair. Cheap (two boundary peeks + at most one
/// `clear_cluster_at`); safe to call on empty rows, zero-length ranges,
/// or out-of-bounds indices.
fn clip_wide_pair_at_range_boundaries(row: &mut super::cell::Row, start: usize, end: usize) {
    if start >= end || row.cells.is_empty() {
        return;
    }
    let cells = &mut row.cells[..];
    let mut left_orphan_main: Option<usize> = None;
    let mut right_orphan_continuation: Option<usize> = None;
    // Left boundary: cells[start] is a continuation, so its main at
    // start-1 sits outside the range — orphan it away.
    if start > 0 && start < cells.len() && cells[start].width == 0 && cells[start - 1].width == 2 {
        cells[start - 1] = super::cell::Cell::EMPTY;
        left_orphan_main = Some(start - 1);
    }
    // Right boundary: cells[end-1] is a wide main inside the range, so
    // its continuation at `end` sits outside — orphan it away.
    if end <= cells.len()
        && end > 0
        && end < cells.len()
        && cells[end - 1].width == 2
        && cells[end].width == 0
    {
        cells[end] = super::cell::Cell::EMPTY;
        right_orphan_continuation = Some(end);
    }
    // Cluster sidecars are anchored at the MAIN col of a wide pair —
    // never at the continuation. So we only need to clear the sidecar
    // for `left_orphan_main` (always a main col) and never for
    // `right_orphan_continuation` (always a continuation col).
    let _ = right_orphan_continuation; // documented for clarity; no-op
    if let Some(col) = left_orphan_main {
        row.clear_cluster_at(col);
    }
}

fn clip_hyperlinks_around(spans: &mut Vec<super::cell::HyperlinkSpan>, start: usize, end: usize) {
    if start >= end {
        return;
    }
    spans.retain_mut(|span| {
        if span.col_end <= start || span.col_start >= end {
            true // entirely outside the erase window
        } else if span.col_start >= start && span.col_end <= end {
            false // entirely inside — drop
        } else if span.col_start < start && span.col_end > end {
            false // hole punched in the middle — drop (see doc-comment)
        } else if span.col_end > end {
            // erase covers the head; clip start forward to `end`.
            span.col_start = end;
            true
        } else {
            // erase covers the tail; clip end backward to `start`.
            span.col_end = start;
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BCE: when the pen carries a non-default background, EL all should
    /// fill blanked cells with `bg = pen.bg` (default fg, no flags). The
    /// classic xterm/iTerm behaviour without which "TUI 设的色被 ED 清掉"
    /// after a colour-set + clear sequence.
    #[test]
    fn bce_erase_in_line_all_preserves_pen_bg() {
        let mut g = Grid::new(2, 5, 0);
        let blue = Attrs {
            fg: Color::DEFAULT,
            bg: Color::indexed(4),
            flags: Flags::empty(),
        };
        g.set_pen(blue);
        g.erase_in_line(EraseMode::All);
        let row = g.row(0).unwrap();
        for c in &row.cells {
            assert_eq!(c.ch, ' ');
            assert_eq!(g.attrs.get(c.attr).bg, Color::indexed(4));
            assert_eq!(g.attrs.get(c.attr).fg, Color::DEFAULT);
        }
    }

    /// BCE: when the pen is default the helper short-circuits to
    /// `Cell::EMPTY` — `AttrId::DEFAULT` index 0, no attr-table churn.
    /// This keeps the common path (no SGR change before clear) identical
    /// to the pre-BCE behaviour.
    #[test]
    fn bce_erase_with_default_pen_yields_attrid_default() {
        let mut g = Grid::new(1, 4, 0);
        g.erase_in_line(EraseMode::All);
        let row = g.row(0).unwrap();
        for c in &row.cells {
            assert_eq!(c.attr, AttrId::DEFAULT);
        }
    }

    /// BCE: ECH must respect the pen too — TUIs commonly do "set bg
    /// → ECH N → write text" to repaint a coloured run in place
    /// (PSReadLine prompt redraws, fzf preview pane).
    #[test]
    fn bce_erase_chars_preserves_pen_bg() {
        let mut g = Grid::new(1, 6, 0);
        let red = Attrs {
            fg: Color::DEFAULT,
            bg: Color::indexed(1),
            flags: Flags::empty(),
        };
        g.set_pen(red);
        g.erase_chars(3);
        let row = g.row(0).unwrap();
        for c in row.cells.iter().take(3) {
            assert_eq!(g.attrs.get(c.attr).bg, Color::indexed(1));
        }
        // Cells past the erase range should remain untouched (still default).
        for c in row.cells.iter().skip(3) {
            assert_eq!(c.attr, AttrId::DEFAULT);
        }
    }

    /// BCE: DCH shifts the row left and fills the right margin with
    /// blanks — those right-margin fills should carry the pen bg.
    #[test]
    fn bce_delete_chars_right_fill_uses_pen_bg() {
        let mut g = Grid::new(1, 6, 0);
        for ch in "ABCDEF".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        let green = Attrs {
            fg: Color::DEFAULT,
            bg: Color::indexed(2),
            flags: Flags::empty(),
        };
        g.set_pen(green);
        g.cursor_to(0, 0);
        g.delete_chars(2);
        let row = g.row(0).unwrap();
        // After DCH(2): "CDEF" + 2 blanks → blanks at cols 4..6 carry bg=2.
        assert_eq!(row.cells[0].ch, 'C');
        assert_eq!(row.cells[3].ch, 'F');
        for c in row.cells.iter().skip(4) {
            assert_eq!(c.ch, ' ');
            assert_eq!(g.attrs.get(c.attr).bg, Color::indexed(2));
        }
    }

    /// BCE: scroll_up at the bottom of the scroll region inserts a new
    /// blank row at `bottom` — that row should carry the pen bg.
    #[test]
    fn bce_scroll_up_new_row_uses_pen_bg() {
        let mut g = Grid::new(2, 4, 0);
        g.print('X', Attrs::DEFAULT);
        let cyan = Attrs {
            fg: Color::DEFAULT,
            bg: Color::indexed(6),
            flags: Flags::empty(),
        };
        g.set_pen(cyan);
        g.scroll_up(1);
        // Row 1 (the new bottom) should be filled with cyan blanks.
        let row1 = g.row(1).unwrap();
        for c in &row1.cells {
            assert_eq!(c.ch, ' ');
            assert_eq!(g.attrs.get(c.attr).bg, Color::indexed(6));
        }
    }

    /// BCE: BG is preserved on erase but fg / bold / underline are NOT —
    /// matches xterm's "Background Color Erase" definition (only bg
    /// follows the pen; fg + flags reset). This guards against future
    /// well-meaning patches that copy the full `Attrs` into the blank.
    #[test]
    fn bce_strips_fg_and_flags_keeps_only_bg() {
        let mut g = Grid::new(1, 3, 0);
        let bold_red_on_blue = Attrs {
            fg: Color::indexed(1),
            bg: Color::indexed(4),
            flags: Flags::BOLD,
        };
        g.set_pen(bold_red_on_blue);
        g.erase_in_line(EraseMode::All);
        let row = g.row(0).unwrap();
        for c in &row.cells {
            let a = g.attrs.get(c.attr);
            assert_eq!(a.bg, Color::indexed(4));
            assert_eq!(a.fg, Color::DEFAULT);
            assert_eq!(a.flags, Flags::empty());
        }
    }

    #[test]
    fn alt_screen_isolates_content() {
        let mut g = Grid::new(3, 5, 10);
        g.print('a', Attrs::DEFAULT);
        g.print('b', Attrs::DEFAULT);
        g.enter_alt_screen(true);
        // alt is blank
        assert_eq!(g.row(0).unwrap().cells[0].ch, ' ');
        g.print('X', Attrs::DEFAULT);
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'X');
        g.leave_alt_screen();
        // primary survived intact
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(0).unwrap().cells[1].ch, 'b');
    }

    #[test]
    fn alt_screen_does_not_pollute_scrollback() {
        let mut g = Grid::new(2, 5, 10);
        g.enter_alt_screen(true);
        // Fill alt and force scroll
        for _ in 0..5 {
            g.print('Z', Attrs::DEFAULT);
            g.linefeed();
            g.carriage_return();
        }
        // Scrollback must remain empty.
        assert_eq!(g.scrollback.len(), 0);
    }

    #[test]
    fn scroll_region_constrains_linefeed() {
        let mut g = Grid::new(5, 5, 10);
        g.set_scroll_region(Some(2), Some(4)); // rows 1..3 (0-based)
                                               // Fill some rows
        for ch in ['a', 'b', 'c', 'd', 'e'] {
            g.print(ch, Attrs::DEFAULT);
            g.linefeed();
            g.carriage_return();
        }
        // The first row (row 0) should still be untouched because LF at
        // the bottom of the region only scrolls rows 1..3.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        // No scrollback either: partial scroll region.
        assert_eq!(g.scrollback.len(), 0);
    }

    #[test]
    fn full_region_scroll_pushes_to_scrollback() {
        let mut g = Grid::new(2, 5, 10);
        // Default region = full screen.
        g.print('1', Attrs::DEFAULT);
        g.linefeed();
        g.carriage_return();
        g.print('2', Attrs::DEFAULT);
        g.linefeed();
        g.carriage_return();
        g.print('3', Attrs::DEFAULT);
        // Should have scrolled '1' into scrollback.
        assert_eq!(g.scrollback.len(), 1);
        assert_eq!(g.scrollback.get(0).unwrap().cells[0].ch, '1');
    }

    #[test]
    fn resize_grow_extends_default_scroll_region() {
        // Repro of the "stuck on bottom row" bug: kernel created at 24 rows
        // then resized up to 26 used to keep scroll_bottom=23, leaving rows
        // 24..25 frozen and breaking scrollback push.
        let mut g = Grid::new(24, 80, 100);
        g.resize(26, 49);
        assert_eq!(g.primary.scroll_top, 0);
        assert_eq!(g.primary.scroll_bottom, 25);
        assert!(g.primary.is_full_region());

        // Drive 30 lines through the grid; each LF at the new bottom must
        // scroll and push the evicted row into scrollback.
        for i in 0..30u32 {
            for ch in i.to_string().chars() {
                g.print(ch, Attrs::DEFAULT);
            }
            g.linefeed();
            g.carriage_return();
        }
        // 30 lines into a 26-row screen → at least 4 evictions to scrollback.
        assert!(
            g.scrollback.len() >= 4,
            "scrollback empty after grow-resize"
        );
    }

    #[test]
    fn resize_grow_preserves_custom_scroll_region() {
        // DECSTBM-set custom region must NOT be silently extended on resize;
        // it just gets clamped to new bounds (or reset if invalidated).
        let mut g = Grid::new(10, 10, 0);
        g.set_scroll_region(Some(2), Some(6)); // rows 1..5 (0-based)
        assert_eq!(g.primary.scroll_top, 1);
        assert_eq!(g.primary.scroll_bottom, 5);
        g.resize(12, 10);
        assert_eq!(g.primary.scroll_top, 1);
        assert_eq!(g.primary.scroll_bottom, 5); // preserved, not extended
        assert!(!g.primary.is_full_region());
    }

    #[test]
    fn resize_shrink_clamps_default_scroll_region() {
        let mut g = Grid::new(10, 10, 0);
        g.resize(5, 10);
        assert_eq!(g.primary.scroll_top, 0);
        assert_eq!(g.primary.scroll_bottom, 4);
        assert!(g.primary.is_full_region());
    }

    // §1.22 (2026-05-05): when on alt screen and dimensions change, the alt
    // buffer should be wiped so the application's SIGWINCH redraw paints on
    // a clean canvas (Claude Code / lazygit / Ink-based CLIs use partial-
    // diff redraws that DON'T necessarily repaint every cell).
    #[test]
    fn resize_on_alt_screen_clears_alt_buffer() {
        let mut g = Grid::new(5, 10, 0);
        g.enter_alt_screen(true);
        // Paint some content on the alt screen.
        for ch in ['x', 'y', 'z'] {
            g.print(ch, Attrs::DEFAULT);
        }
        // Sanity: alt now has those cells.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'x');
        assert_eq!(g.row(0).unwrap().cells[1].ch, 'y');
        assert_eq!(g.row(0).unwrap().cells[2].ch, 'z');

        g.resize(8, 14);

        // After resize on alt, every visible cell should be cleared.
        for r_idx in 0..g.rows() {
            let row = g.row(r_idx).unwrap();
            for cell in &row.cells {
                assert_eq!(
                    cell.ch, ' ',
                    "cell at row {r_idx} not cleared post-resize on alt"
                );
            }
        }
        // Cursor reset to home.
        let cur = g.cursor();
        assert_eq!(cur.row, 0);
        assert_eq!(cur.col, 0);
    }

    #[test]
    fn resize_on_primary_does_not_clear_primary() {
        let mut g = Grid::new(5, 10, 0);
        // We're on primary by default. Paint something.
        for ch in ['p', 'q', 'r'] {
            g.print(ch, Attrs::DEFAULT);
        }
        g.resize(8, 14);
        // Primary content is preserved by naive truncate/pad — only the alt
        // buffer is wiped on resize (§1.22). Cells stay anchored to their
        // (row, col) coordinates within the new bounds.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'p');
    }

    // §1.25 (2026-05-06): the kernel never reflows. While alt is active,
    // primary's `saved_cursor` (DECSC'd by `?1049h`) must stay anchored to
    // its original (row, col) so `?1049l` lands on the prompt line. Naive
    // truncate/pad clamps the saved coordinates inside the new bounds but
    // never moves them across rows.
    #[test]
    fn resize_on_alt_screen_preserves_primary_saved_cursor() {
        use super::super::cursor::SavedCursor;
        let mut g = Grid::new(10, 80, 100);
        g.cursor_to(5, 12);
        g.primary.saved_cursor = Some(SavedCursor {
            row: 5,
            col: 12,
            attr: AttrId::DEFAULT,
            origin: false,
            pending_wrap: false,
        });
        g.enter_alt_screen(true);
        assert!(g.is_alt_screen());

        g.resize(10, 40); // cols shrink while alt is active

        let s = g.primary.saved_cursor.expect("saved_cursor preserved");
        assert_eq!(s.row, 5, "row preserved (within new bounds)");
        assert_eq!(s.col, 12, "col preserved (within new bounds)");
    }

    #[test]
    fn naive_resize_clamps_saved_cursor() {
        use super::super::cursor::SavedCursor;
        let mut g = Grid::new(10, 10, 0);
        g.enter_alt_screen(true); // forces naive path on primary too
        g.primary.saved_cursor = Some(SavedCursor {
            row: 8,
            col: 8,
            attr: AttrId::DEFAULT,
            origin: false,
            pending_wrap: false,
        });

        g.resize(3, 3);

        let s = g.primary.saved_cursor.expect("still Some");
        assert_eq!((s.row, s.col), (2, 2));
    }

    #[test]
    fn ri_at_scroll_top_scrolls_down() {
        let mut g = Grid::new(3, 3, 0);
        g.print('a', Attrs::DEFAULT);
        g.linefeed();
        g.carriage_return();
        g.print('b', Attrs::DEFAULT);
        g.cursor_to(0, 0);
        g.reverse_linefeed();
        // After RI at top, row 0 is blank, 'a' moved to row 1, 'b' to row 2.
        assert_eq!(g.row(0).unwrap().cells[0].ch, ' ');
        assert_eq!(g.row(1).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(2).unwrap().cells[0].ch, 'b');
    }

    #[test]
    fn insert_delete_lines_within_region() {
        let mut g = Grid::new(4, 3, 0);
        // Place 'a','b','c','d' on rows 0..3 without triggering the
        // bottom-of-region scroll. Print + LF + CR for the first three;
        // for the last, only print (no trailing LF) so 'a' isn't ejected.
        for ch in ['a', 'b', 'c'] {
            g.print(ch, Attrs::DEFAULT);
            g.linefeed();
            g.carriage_return();
        }
        g.print('d', Attrs::DEFAULT);
        // Sanity: setup placed all four rows correctly.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(3).unwrap().cells[0].ch, 'd');

        // IL at row 1: insert one blank, push 'b','c' down, 'd' lost.
        g.cursor_to(1, 0);
        g.insert_lines(1);
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(1).unwrap().cells[0].ch, ' ');
        assert_eq!(g.row(2).unwrap().cells[0].ch, 'b');
        assert_eq!(g.row(3).unwrap().cells[0].ch, 'c');
    }

    // ---- Naive resize (§1.25) -----------------------------------------
    // The kernel never reflows on resize. Both screens use truncate/pad.
    // Any post-resize re-layout is the responsibility of the running
    // application via its SIGWINCH redraw — this avoids the race between
    // kernel-driven cell migration and the TUI's own diff repaint.

    /// Helper: read the printable text of a row (stripping trailing blanks).
    fn row_text(g: &Grid, r: usize) -> String {
        let row = g.row(r).expect("row in range");
        let mut s: String = row.cells.iter().map(|c| c.ch).collect();
        while s.ends_with(' ') {
            s.pop();
        }
        s
    }

    #[test]
    fn naive_resize_rows_only_preserves_content() {
        // Rows-only grow must keep existing rows untouched and pad blanks
        // at the bottom — no rewrap at all (cols unchanged anyway).
        let mut g = Grid::new(5, 20, 100);
        for ch in "hello".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        g.resize(8, 20);
        assert_eq!(g.rows(), 8);
        assert_eq!(g.cols(), 20);
        assert_eq!(row_text(&g, 0), "hello");
        for r in 1..8 {
            assert_eq!(row_text(&g, r), "");
        }
    }

    #[test]
    fn naive_resize_shrink_cols_clips_long_line() {
        // 80-col grid with a single line of 70 'a's. Shrinking to 40 cols
        // must NOT rewrap onto row 1 — the line is clipped to the first 40
        // cells of row 0, and row 1 stays blank. The TUI / shell that
        // owns the line will get a SIGWINCH and may emit its own redraw,
        // but the kernel itself moves no cells between rows.
        let mut g = Grid::new(5, 80, 100);
        for _ in 0..70 {
            g.print('a', Attrs::DEFAULT);
        }
        g.resize(5, 40);
        assert_eq!(g.cols(), 40);
        assert_eq!(
            row_text(&g, 0),
            "a".repeat(40),
            "row 0 clipped to new width"
        );
        assert_eq!(g.row(0).unwrap().wrapped, false, "no synthetic wrap flag");
        for r in 1..5 {
            assert_eq!(row_text(&g, r), "", "row {r} untouched (no rewrap)");
        }
    }

    #[test]
    fn naive_resize_grow_cols_pads_with_blanks() {
        // Grow from 5 cols → 10 cols: existing cells stay where they are,
        // new cells on the right are blank (no unwrapping, no stitching).
        let mut g = Grid::new(3, 5, 0);
        for ch in "abc".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        g.resize(3, 10);
        assert_eq!(g.cols(), 10);
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(g.row(0).unwrap().cells[1].ch, 'b');
        assert_eq!(g.row(0).unwrap().cells[2].ch, 'c');
        for c in 3..10 {
            assert!(g.row(0).unwrap().cells[c].is_blank(), "col {c} blank");
        }
    }

    #[test]
    fn naive_resize_clears_pending_wrap_unconditionally() {
        // Cursor parked at the right edge with pending_wrap=true (after
        // 10 prints into a 10-col grid). Resize must always clear
        // pending_wrap on both screens — the "park one-past-last-col"
        // semantic is anchored to the OLD column boundary, which the
        // resize has just moved. Clearing it is what naive_resize_screen
        // does at line 335; this test pins that contract.
        let mut g = Grid::new(5, 10, 100);
        for ch in "0123456789".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        assert_eq!(g.cursor().col, 9);
        assert!(g.cursor().pending_wrap);

        // Shrink: cursor clamps from col 9 to col 4 (new last col).
        g.resize(5, 5);
        assert_eq!(
            g.cursor().col,
            4,
            "cursor clamped to new last col on shrink"
        );
        assert!(
            !g.cursor().pending_wrap,
            "pending_wrap cleared even when cursor lands AT new last col on shrink"
        );

        // Grow back: pending_wrap must stay false. The application that
        // owns the cursor (shell / TUI) will re-establish its own state
        // via SIGWINCH redraw if it cares. The kernel never re-derives
        // pending_wrap across a resize.
        g.cursor_to(0, 4); // keep cursor at last col of current 5-col grid
                           // print one more char to push pending_wrap=true at col 4.
        g.print('!', Attrs::DEFAULT);
        assert!(
            g.cursor().pending_wrap,
            "print at last col sets pending_wrap"
        );
        g.resize(5, 20);
        assert!(
            !g.cursor().pending_wrap,
            "pending_wrap cleared on grow regardless of cursor position"
        );
    }

    #[test]
    fn resize_alt_clears_buffer_no_reflow() {
        // §1.22 + §1.25: alt-screen resize wipes the alt buffer so the
        // application's SIGWINCH-driven redraw paints on a clean canvas.
        // No row should claim wrapped=true (no reflow ever runs).
        let mut g = Grid::new(3, 10, 100);
        g.enter_alt_screen(true);
        for ch in "abcdefghij".chars() {
            g.print(ch, Attrs::DEFAULT);
        }
        g.resize(3, 5);
        assert_eq!(g.cols(), 5);
        for r_idx in 0..g.rows() {
            assert_eq!(
                row_text(&g, r_idx),
                "",
                "alt row {r_idx} not cleared after resize"
            );
            assert_eq!(g.row(r_idx).unwrap().wrapped, false);
        }
    }

    #[test]
    fn resize_diag_reports_naive_branch_only() {
        // §1.25: ResizeBranch collapses to a single Naive variant. Whether
        // alt was active at resize time (and therefore whether §1.22 wipe
        // ran) is conveyed by the is_alt + wipe_fired fields.
        let mut g = Grid::new(5, 10, 0);
        g.resize(5, 8);
        let last = g.last_resize_diags().last().expect("one resize recorded");
        assert_eq!(last.branch, ResizeBranch::Naive);
        assert!(!last.is_alt);
        assert!(!last.wipe_fired);

        g.enter_alt_screen(true);
        g.resize(5, 12);
        let last = g.last_resize_diags().last().expect("alt resize recorded");
        assert_eq!(last.branch, ResizeBranch::Naive);
        assert!(last.is_alt);
        assert!(
            last.wipe_fired,
            "wipe runs when alt is active and dims change"
        );
    }

    // ---- §A.3 inline-TUI primary wipe ---------------------------------
    // Claude Code's input box renders inline on primary (cursor hidden +
    // CSI absolute positioning, no `?1049h`). On shrink the §1.22 alt
    // wipe doesn't fire and the §1.26 cursor-row+below partial cleanup
    // leaves rows ABOVE the cursor stale — the input box's top border
    // typically sits there, so wrapped border garbage stays visible.
    // §A.3 wipes the entire visible primary region in this case so
    // Ink's diff redraw paints on a blank canvas.

    #[test]
    fn inline_tui_resize_full_wipes_primary_visible_region() {
        let mut g = Grid::new(6, 20, 100);
        // Simulate Ink-style render: place a `╮` at col 18 of row 1
        // (top-right corner of an old-width input box) plus a body
        // character at row 4 col 0 — both must vanish after the full
        // wipe, NOT just the row below the cursor.
        g.cursor_to(1, 18);
        g.print('╮', Attrs::DEFAULT);
        g.cursor_to(4, 0);
        g.print('x', Attrs::DEFAULT);
        // Park cursor at row 5 (BELOW the border) so the §1.26 partial
        // cleanup would never have touched row 1 — only the §A.3 full
        // wipe can.
        g.cursor_to(5, 0);

        g.resize_with_inline_tui(6, 12, true);

        for r in 0..g.rows() {
            assert_eq!(
                row_text(&g, r),
                "",
                "primary row {r} should be wiped under inline-TUI resize"
            );
            assert_eq!(g.row(r).unwrap().wrapped, false);
        }
        assert_eq!(g.cursor().row, 0, "cursor homed on inline-TUI wipe");
        assert_eq!(g.cursor().col, 0, "cursor homed on inline-TUI wipe");

        let diag = g.last_resize_diags().last().expect("resize recorded");
        assert!(diag.inline_tui_wipe, "inline_tui_wipe diag fired");
        assert!(diag.inline_tui_active, "heuristic snapshot recorded");
        assert!(!diag.wipe_fired, "alt-screen wipe did NOT fire on primary");
        assert!(
            !diag.cleared_below_cursor,
            "partial cleanup skipped when full wipe ran"
        );
    }

    #[test]
    fn plain_primary_resize_skips_inline_tui_wipe() {
        // No inline-TUI flag → existing §1.26 partial cleanup applies,
        // §A.3 full wipe stays off, content above cursor preserved.
        let mut g = Grid::new(5, 20, 100);
        g.print('p', Attrs::DEFAULT);
        g.print('s', Attrs::DEFAULT);
        // Park cursor at row 2 col 0 — `prev` row 0 'ps' must survive
        // both the partial cleanup and the (non-firing) full wipe.
        g.cursor_to(2, 0);

        g.resize_with_inline_tui(5, 10, false);

        assert_eq!(row_text(&g, 0), "ps", "row above cursor preserved");
        let diag = g.last_resize_diags().last().expect("resize recorded");
        assert!(!diag.inline_tui_wipe, "no full wipe without heuristic");
        assert!(!diag.inline_tui_active, "heuristic snapshot stays off");
        assert!(
            diag.cleared_below_cursor,
            "§1.26 partial cleanup still runs"
        );
    }

    #[test]
    fn inline_tui_heuristic_decays_after_idle() {
        // Heuristic depends on (a) NOT alt screen, (b) cursor hidden,
        // (c) absolute-positioning timestamp within INLINE_TUI_DECAY_MS.
        // We drive the timestamp directly via `note_absolute_positioning`
        // to keep the test wall-clock-independent.
        let mut g = Grid::new(5, 20, 0);
        let now = 100_000_i64;

        // Fresh stamp → heuristic on (cursor_visible=false simulates ?25l).
        g.note_absolute_positioning(now);
        assert!(g.is_inline_tui_active_at(now + 500, false));
        assert!(g.is_inline_tui_active_at(now + 1_999, false));

        // Past the 2 s decay window → off.
        assert!(!g.is_inline_tui_active_at(now + 2_001, false));

        // Cursor visible → heuristic off regardless of fresh stamp.
        assert!(!g.is_inline_tui_active_at(now + 500, true));

        // Alt screen → heuristic off regardless of stamp / cursor.
        g.enter_alt_screen(false);
        g.note_absolute_positioning(now + 100);
        assert!(!g.is_inline_tui_active_at(now + 200, false));

        // Never observed (sentinel 0) → off.
        let mut g2 = Grid::new(5, 20, 0);
        assert!(!g2.is_inline_tui_active_at(50_000, false));
        // Even a fresh `note` followed by cursor-visible should be off.
        g2.note_absolute_positioning(50_000);
        assert!(!g2.is_inline_tui_active_at(50_500, true));
    }

    #[test]
    fn ctrl_c_grace_window_disables_inline_tui_heuristic() {
        // Scenario: Ink-style TUI hides cursor (`?25l`) and keeps
        // re-emitting absolute-positioning CSIs as the user types. User
        // hits Ctrl+C; TUI dies without sending `?25h` so
        // `cursor_visible` stays false. PSReadLine then writes
        // `\x1b[G` on every keystroke, keeping the abs-CSI timestamp
        // fresh — without the grace window, the heuristic would stay
        // wedged "on" forever and the shell-history IME helper
        // wouldn't re-enable. Verify the grace window short-circuits
        // the heuristic for exactly CTRL_C_GRACE_MS.
        let mut g = Grid::new(5, 20, 0);
        let now = 100_000_i64;

        // Set up the "wedged" state: cursor hidden, abs-CSI fresh.
        g.note_absolute_positioning(now);
        assert!(g.is_inline_tui_active_at(now + 500, false), "heuristic on pre-Ctrl+C");

        // User sends Ctrl+C.
        g.note_ctrl_c_sent(now + 500);

        // Within grace window → heuristic forced off even though
        // PSReadLine keeps emitting abs-CSIs.
        g.note_absolute_positioning(now + 1_000);
        assert!(!g.is_inline_tui_active_at(now + 1_500, false), "grace window suppresses heuristic");
        g.note_absolute_positioning(now + 3_000);
        assert!(!g.is_inline_tui_active_at(now + 3_400, false), "still suppressed near grace boundary");

        // Past grace window (3 s) AND fresh abs-CSI → heuristic re-engages.
        g.note_absolute_positioning(now + 4_000);
        assert!(g.is_inline_tui_active_at(now + 4_100, false), "heuristic re-engages after grace expires");

        // Cursor visible during grace → off regardless (no regression).
        let mut g2 = Grid::new(5, 20, 0);
        g2.note_absolute_positioning(now);
        g2.note_ctrl_c_sent(now);
        assert!(!g2.is_inline_tui_active_at(now + 500, true), "visible cursor still wins");
    }

    #[test]
    fn is_inline_tui_active_at_after_redraw_csi() {
        // §A.4 — EL/ED/CUU/CUD must independently activate the heuristic
        // (without any absolute-positioning CSI), so log-update's
        // `(\x1b[2K\x1b[1A)*N` walk fires §1.27 from the first iteration.
        let mut g = Grid::new(5, 20, 0);
        let now = 100_000_i64;

        // No CSI yet → off.
        assert!(!g.is_inline_tui_active_at(now, false));

        // Redraw CSI alone activates it.
        g.note_redraw_csi(now);
        assert!(g.is_inline_tui_active_at(now + 500, false));
        assert!(g.is_inline_tui_active_at(now + 1_999, false));

        // Same 2 s decay window.
        assert!(!g.is_inline_tui_active_at(now + 2_001, false));

        // Cursor-visible / alt-screen guards still apply.
        assert!(!g.is_inline_tui_active_at(now + 500, true));
    }

    #[test]
    fn redraw_csi_does_not_corrupt_abs_position() {
        // §A.4 — the new redraw timestamp must NOT touch the IME anchor
        // payload (`last_abs_csi_position`). Tracking redraw CSIs and
        // absolute CSIs in separate fields preserves the IME helper's
        // "last absolute LANDING" semantics.
        let mut g = Grid::new(5, 20, 0);

        assert!(g.last_abs_csi_position().is_none());

        // After many redraw CSIs, the absolute position is still untouched.
        g.note_redraw_csi(10);
        g.note_redraw_csi(20);
        g.note_redraw_csi(30);
        assert!(
            g.last_abs_csi_position().is_none(),
            "redraw CSIs must not register as absolute landings"
        );
    }

    // ------------------------------------------------------------------
    // §1.28 (2026-05-07): wide-cell pair invariant under cell-edit ops.
    //
    // These tests guard the chain that produced "中文字符只渲染一半",
    // "字符消失只剩占位", "改色文本多余字符" symptoms when running
    // claude/Ink inside ridge-term. Root cause: cell-edit ops (print
    // overwrite, EL/ECH, ICH/DCH) used to leave half of a wide-cell
    // pair behind when the other half was overwritten/erased/shifted.
    // ------------------------------------------------------------------

    /// `assert_no_orphan_pair_in(row)` — for every cell in the row,
    /// width==0 must be immediately preceded by a width==2; width==2
    /// must be immediately followed by a width==0. Either invariant
    /// being violated means a wide-cell half is dangling.
    fn assert_no_orphan_pair_in(g: &Grid, row_idx: usize) {
        let row = g.row(row_idx).expect("row in range");
        let cells = &row.cells;
        for (i, cell) in cells.iter().enumerate() {
            if cell.width == 0 {
                assert!(
                    i > 0,
                    "row {row_idx} col {i}: width==0 at column 0 has no possible main"
                );
                let prev = cells[i - 1];
                assert_eq!(
                    prev.width, 2,
                    "row {row_idx} col {i}: width==0 (continuation) without width==2 main at col {}",
                    i - 1,
                );
            }
            if cell.width == 2 {
                assert!(
                    i + 1 < cells.len(),
                    "row {row_idx} col {i}: width==2 main at last col has no continuation slot",
                );
                let next = cells[i + 1];
                assert_eq!(
                    next.width, 0,
                    "row {row_idx} col {i}: width==2 (main) without width==0 continuation at col {}",
                    i + 1,
                );
            }
        }
    }

    #[test]
    fn print_narrow_over_wide_main_clears_continuation() {
        let mut g = Grid::new(2, 10, 0);
        g.print('中', Attrs::DEFAULT);
        // Sanity: '中' occupies cols 0..=1.
        assert_eq!(g.row(0).unwrap().cells[0].ch, '中');
        assert_eq!(g.row(0).unwrap().cells[0].width, 2);
        assert_eq!(g.row(0).unwrap().cells[1].width, 0);

        // Move back to col 0 and overwrite with a narrow ASCII char.
        g.cursor_to(0, 0);
        g.print('A', Attrs::DEFAULT);

        // The trailing continuation must be cleared, not orphaned.
        assert_eq!(g.row(0).unwrap().cells[0].ch, 'A');
        assert_eq!(g.row(0).unwrap().cells[0].width, 1);
        assert_eq!(g.row(0).unwrap().cells[1].width, 1);
        assert_eq!(g.row(0).unwrap().cells[1].ch, ' ');
        assert_no_orphan_pair_in(&g, 0);
    }

    #[test]
    fn print_narrow_at_continuation_does_not_wipe_freshly_written_main() {
        // The exact Ink-redraw chain from the bug report:
        //   1. write '中' at col 0..=1
        //   2. (Ink frame redraw) cursor back to col 0, write 'A'
        //   3. cursor advances, write 'B' at col 1
        // Pre-§1.28: step 3's "I see width==0, clean main at col-1"
        // branch fires and overwrites the 'A' from step 2 with a
        // default-attr blank.
        let mut g = Grid::new(2, 10, 0);
        g.print('中', Attrs::DEFAULT);
        g.cursor_to(0, 0);
        g.print('A', Attrs::DEFAULT);
        // After fix, cursor is now at col 1 — but instead of trusting
        // that, set explicitly so the test is layout-agnostic.
        g.cursor_to(0, 1);
        g.print('B', Attrs::DEFAULT);

        assert_eq!(
            g.row(0).unwrap().cells[0].ch,
            'A',
            "the 'A' from the prior write must NOT be wiped by writing 'B' to col 1",
        );
        assert_eq!(g.row(0).unwrap().cells[1].ch, 'B');
        assert_no_orphan_pair_in(&g, 0);
    }

    #[test]
    fn el_clearing_through_wide_main_clears_continuation() {
        // ECH 3 from cursor=col 1 against "中文" (cols 0..=3): the
        // erase range [1, 4) cuts the main of '文' (col 2) inside but
        // leaves nothing wide outside. Result: row [c, EMPTY, EMPTY, EMPTY,
        // ...] — except the FIRST cell at col 0 was '中' main, whose
        // continuation at col 1 falls inside the erase. The boundary
        // guard must clear cells[0] too.
        let mut g = Grid::new(2, 10, 0);
        g.print('中', Attrs::DEFAULT); // cols 0..=1
        g.print('文', Attrs::DEFAULT); // cols 2..=3
        g.cursor_to(0, 1);
        g.erase_chars(3); // erase [1, 4)

        assert_no_orphan_pair_in(&g, 0);
        // cells[0] should also be EMPTY because '中' main at col 0
        // would otherwise be an orphan.
        assert_eq!(g.row(0).unwrap().cells[0].width, 1);
        assert_eq!(g.row(0).unwrap().cells[0].ch, ' ');
    }

    #[test]
    fn ich_at_wide_continuation_clears_paired_main() {
        // '中' at cols 0..=1, cursor on the continuation (col 1),
        // ICH 2 → the shift would push the continuation right by 2,
        // leaving '中' main at col 0 with two blanks before its
        // displaced continuation. Boundary guard must clear both
        // halves of the split pair.
        let mut g = Grid::new(2, 10, 0);
        g.print('中', Attrs::DEFAULT);
        g.cursor_to(0, 1);
        g.insert_chars(2);

        assert_no_orphan_pair_in(&g, 0);
        // Col 0 must NOT remain a width=2 main.
        assert_eq!(g.row(0).unwrap().cells[0].width, 1);
    }

    #[test]
    fn dch_at_wide_main_clears_paired_continuation() {
        // DCH 1 against a wide pair starting at the cursor: the main
        // is deleted, the continuation gets shifted left into the
        // main's slot — without the guard it lands as a width=0 with
        // no width=2 to its left.
        let mut g = Grid::new(2, 10, 0);
        g.print('中', Attrs::DEFAULT); // cols 0..=1
        g.print('A', Attrs::DEFAULT); // col  2
        g.cursor_to(0, 0);
        g.delete_chars(1); // delete cells[0], shifting [1..] left

        assert_no_orphan_pair_in(&g, 0);
        // After fix, col 0 is the cleared continuation slot (now
        // EMPTY width=1), col 1 is the shifted-in 'A'.
        assert_eq!(g.row(0).unwrap().cells[0].width, 1);
    }

    #[test]
    fn backspace_skips_wide_placeholder_to_main() {
        // §B.4 (2026-05-08) — placeholder normalization. After a wide
        // char (🎂 or 中) is at cols 0..=1 with cursor at col 2,
        // BS strict-VT moves cursor to col 1 (placeholder). With
        // normalization, cursor lands at col 0 (the main) so the next
        // SP overwrites correctly via §1.28 branch (b) in one step.
        //
        // Pre-fix: PSReadLine sending a single BS for delete-char
        // left the cursor wedged on the placeholder, the wide glyph
        // still painted full-width, and a "first BS shows residual"
        // user complaint on 🎂.
        let mut g = Grid::new(1, 10, 0);
        g.print('中', Attrs::DEFAULT); // cols 0..=1
        // Cursor should now be at col 2.
        assert_eq!(g.cursor().col, 2);
        g.backspace();
        // Pre-fix this would land at col 1 (placeholder); post-fix
        // it lands at col 0 (main).
        assert_eq!(
            g.cursor().col,
            0,
            "BS over wide placeholder must normalize to the main col"
        );
    }

    #[test]
    fn backspace_over_narrow_unchanged() {
        // §B.4 — narrow chars BS exactly one step. The placeholder
        // normalization only fires when the cursor lands on width=0,
        // which never happens for narrow chars.
        let mut g = Grid::new(1, 10, 0);
        g.print('a', Attrs::DEFAULT);
        g.print('b', Attrs::DEFAULT);
        g.print('c', Attrs::DEFAULT);
        // cursor at col 3.
        g.backspace();
        assert_eq!(g.cursor().col, 2);
        g.backspace();
        assert_eq!(g.cursor().col, 1);
        g.backspace();
        assert_eq!(g.cursor().col, 0);
    }

    #[test]
    fn backspace_at_col_zero_clamps() {
        // §B.4 — BS at col 0 stays at col 0 (no underflow), even with
        // the new normalization step (which is gated on cur_col > 0).
        let mut g = Grid::new(1, 10, 0);
        g.cursor_to(0, 0);
        g.backspace();
        assert_eq!(g.cursor().col, 0);
    }

    #[test]
    fn cluster_sidecar_survives_scroll_into_scrollback() {
        // §B.3 invariant lock: when a row carrying a multi-codepoint
        // cluster scrolls off the top, the cluster sidecar must
        // travel with the Row into the scrollback ring (preserved as
        // historical content for pgup / search / select). The Row
        // type owns clusters as a Vec field, so `rows.remove +
        // scrollback.push` is a whole-row move that automatically
        // preserves it — but a future refactor that splits cells from
        // clusters at the row boundary could break this. Lock with
        // a test.
        let mut g = Grid::new(3, 10, 100);
        // Row 0 carries a ZWJ cluster at col 0..=1.
        g.print_grapheme("\u{1F468}\u{200D}\u{1F469}", Attrs::DEFAULT);
        // Force two scrolls so row 0 ends up in scrollback.
        g.cursor_to(2, 0);
        g.linefeed();
        g.linefeed();
        g.linefeed();

        // Scrollback row 0 is the original "row 0" with cluster intact.
        let sb_row = g.scrollback.get(0).expect("row should be in scrollback");
        let cluster = sb_row.clusters.iter().find(|c| c.col == 0);
        assert!(
            cluster.is_some(),
            "cluster sidecar must travel with the row into scrollback"
        );
    }

    #[test]
    fn cluster_sidecar_dropped_when_row_recycled_for_new_blank() {
        // §B.3 invariant lock: when a row scrolls off the top and the
        // ring buffer at capacity returns its allocation for recycling
        // as the new bottom row, `Row::clear()` is called — which MUST
        // drop the cluster sidecar (otherwise the recycled blank row
        // would carry a stale sidecar pointing at evicted content).
        let mut g = Grid::new(2, 10, 1); // capacity 1 — tight rollover
        g.print_grapheme("\u{1F468}\u{200D}\u{1F469}", Attrs::DEFAULT);
        g.cursor_to(1, 0);
        // Two LFs: first pushes original row 0 into scrollback (capacity
        // 1, no eviction yet); second evicts it and recycles the
        // allocation back as the new bottom row.
        g.linefeed();
        g.linefeed();
        g.linefeed();

        // The recycled bottom row should be blank — no stale clusters.
        let bottom = g.row(1).expect("row 1 exists");
        assert!(
            bottom.clusters.is_empty(),
            "recycled row must be blank — clusters dropped by Row::clear()"
        );
    }

    #[test]
    fn print_at_wide_continuation_drops_paired_clusters_sidecar() {
        // §B.2 (2026-05-08) regression test. Pre-fix:
        //   1. print_grapheme("👨‍👩‍👧") → cell[0] (width=2 main with
        //      cluster sidecar pointing at the multi-codepoint emoji
        //      string), cell[1] (width=0 spacer), cursor at col 2.
        //   2. shell echoes BS+SP+BS to erase the cluster:
        //        - BS  (0x08) → cursor 2→1 (lands on continuation).
        //        - SP  print(' ') at col 1: §1.28 branch (a) fires —
        //          orphan main at col 0 cleared to (' ', w=1).
        //   3. The renderer's draw loop scans each col, finds a cluster
        //      sidecar at col 0 pointing at "👨‍👩‍👧", atlas-keys it as
        //      a cluster glyph, and paints the wide emoji bitmap into
        //      the now-1-cell-wide quad — visible "退格一次出现乱码" symptom.
        //
        // Post-fix: branch (a) calls clear_cluster_at(col-1) before
        // overwriting the orphan main, so no stale sidecar survives
        // the BS+SP echo. Subsequent renders show a blank cell.
        let mut g = Grid::new(1, 10, 0);
        g.print_grapheme("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", Attrs::DEFAULT);
        // Family ZWJ cluster placed at col 0..=1 (width 2). Cluster
        // sidecar points at the full multi-codepoint string.
        assert_eq!(g.row(0).unwrap().cells[0].width, 2);
        assert!(g.row(0).unwrap().cluster_at(0).is_some());

        // Simulate the BS+SP that drops the cursor on the continuation
        // and overwrites it.
        g.cursor_to(0, 1);
        g.print(' ', Attrs::DEFAULT);

        // Orphan main at col 0 must be cleared AND its cluster sidecar
        // dropped — the renderer must see a plain ' ' at col 0, not a
        // sidecar resurrection of the original emoji.
        assert_eq!(g.row(0).unwrap().cells[0].width, 1);
        assert_eq!(g.row(0).unwrap().cells[0].ch, ' ');
        assert!(
            g.row(0).unwrap().cluster_at(0).is_none(),
            "stale cluster sidecar must not survive wide-pair orphan clear"
        );
    }

    #[test]
    fn erase_chars_drops_clusters_in_range() {
        // §B.2 — ECH inside a row carrying multi-codepoint clusters
        // must drop their sidecars. Pre-fix: the cells were wiped to
        // EMPTY but the cluster sidecar persisted, and any subsequent
        // narrow write in the same col rendered the original emoji
        // glyph through the cluster sidecar.
        let mut g = Grid::new(2, 10, 0);
        g.print_grapheme("\u{1F468}\u{200D}\u{1F469}", Attrs::DEFAULT); // 👨‍👩 at 0..=1
        g.print_grapheme("\u{1F1FA}\u{1F1F8}", Attrs::DEFAULT); // 🇺🇸 at 2..=3
        assert!(g.row(0).unwrap().cluster_at(0).is_some());
        assert!(g.row(0).unwrap().cluster_at(2).is_some());

        g.cursor_to(0, 0);
        g.erase_chars(4); // wipe both clusters

        assert!(g.row(0).unwrap().cluster_at(0).is_none());
        assert!(g.row(0).unwrap().cluster_at(2).is_none());
    }

    #[test]
    fn delete_chars_shifts_clusters_left() {
        // §B.2 — DCH must shift cluster sidecars along with the cells
        // they describe. Pre-fix: cells shifted but sidecars stayed
        // anchored at their original cols, so the cluster lookup found
        // an emoji glyph at the wrong position.
        let mut g = Grid::new(2, 10, 0);
        // Layout: [A][B][🇺🇸 main][🇺🇸 cont][C][...]
        g.print('A', Attrs::DEFAULT);
        g.print('B', Attrs::DEFAULT);
        g.print_grapheme("\u{1F1FA}\u{1F1F8}", Attrs::DEFAULT); // 🇺🇸 at 2..=3
        g.print('C', Attrs::DEFAULT);
        assert!(g.row(0).unwrap().cluster_at(2).is_some());

        // DCH 1 at col 0 → row becomes [B][🇺🇸 main][🇺🇸 cont][C][...]
        g.cursor_to(0, 0);
        g.delete_chars(1);

        // Cluster sidecar must have moved from col 2 → col 1.
        assert!(
            g.row(0).unwrap().cluster_at(2).is_none(),
            "stale sidecar at original col"
        );
        assert!(
            g.row(0).unwrap().cluster_at(1).is_some(),
            "sidecar must shift to new main col"
        );
    }

    #[test]
    fn delete_chars_drops_clusters_inside_deletion_range() {
        // §B.2 — DCH inside a cluster must drop its sidecar entirely;
        // the shifted cells past the deletion range carry their own
        // sidecars (already covered above).
        let mut g = Grid::new(2, 10, 0);
        g.print_grapheme("\u{1F1FA}\u{1F1F8}", Attrs::DEFAULT); // 🇺🇸 at 0..=1
        g.print('A', Attrs::DEFAULT);
        assert!(g.row(0).unwrap().cluster_at(0).is_some());

        g.cursor_to(0, 0);
        g.delete_chars(2); // delete the whole flag

        assert!(
            g.row(0).unwrap().cluster_at(0).is_none(),
            "cluster sidecar inside DCH range must be dropped"
        );
    }

    #[test]
    fn insert_chars_shifts_clusters_right() {
        // §B.2 — ICH must shift cluster sidecars right along with the
        // cells they describe.
        let mut g = Grid::new(2, 10, 0);
        // Layout: [A][🇺🇸 main][🇺🇸 cont][B][...]
        g.print('A', Attrs::DEFAULT);
        g.print_grapheme("\u{1F1FA}\u{1F1F8}", Attrs::DEFAULT); // 🇺🇸 at 1..=2
        g.print('B', Attrs::DEFAULT);
        assert!(g.row(0).unwrap().cluster_at(1).is_some());

        // ICH 2 at col 1 → [A][_][_][🇺🇸 main][🇺🇸 cont][B][...]
        g.cursor_to(0, 1);
        g.insert_chars(2);

        // Cluster sidecar must have moved from col 1 → col 3.
        assert!(g.row(0).unwrap().cluster_at(1).is_none());
        assert!(
            g.row(0).unwrap().cluster_at(3).is_some(),
            "sidecar must shift right by ICH count"
        );
    }

    #[test]
    fn insert_chars_drops_clusters_pushed_off_right_margin() {
        // §B.2 — ICH that pushes cells past cols-1 must also drop the
        // cluster sidecars on those cells.
        let mut g = Grid::new(2, 4, 0); // narrow 4-cell row
        g.print('A', Attrs::DEFAULT);
        // Wide cluster anchored at col 1..=2.
        g.print_grapheme("\u{1F1FA}\u{1F1F8}", Attrs::DEFAULT);
        g.print('B', Attrs::DEFAULT); // col 3
        assert!(g.row(0).unwrap().cluster_at(1).is_some());

        // ICH 2 at col 1 — would shift the cluster from col 1→3, but
        // the cluster's continuation at col 4 doesn't exist (cols=4),
        // so the cluster gets pushed entirely off the row. The cells
        // are also clamped — but the SIDECAR must drop too.
        g.cursor_to(0, 1);
        g.insert_chars(2);

        // The §1.28 right-margin orphan-clear at cols-n-1=1 should
        // have killed the wide pair AND its sidecar.
        assert!(
            g.row(0).unwrap().cluster_at(1).is_none(),
            "sidecar at orphan-cleared right margin must drop"
        );
        // No surviving sidecars anywhere — the cluster is GONE.
        for col in 0..g.row(0).unwrap().cells.len() {
            assert!(
                g.row(0).unwrap().cluster_at(col).is_none(),
                "no sidecar should survive ICH-overflow at col {col}"
            );
        }
    }

    #[test]
    fn wide_print_over_existing_wide_drops_overwritten_cluster_sidecar() {
        // §B.2 — branch (c) cluster-sidecar cleanup. Layout:
        //   col 0..=1: wide cluster A ("🇺🇸" RIS pair)
        //   col 1..=2: would be a second wide whose main lands at col 1
        // The wide-write path at cur_col=0 lays cell[0] = main, cell[1] =
        // wide_spacer. If a previous cluster anchored at col=1 (because
        // an earlier write left a wide main there) survives, the
        // renderer paints it on top of the spacer.
        let mut g = Grid::new(1, 10, 0);
        // Establish: write a wide cluster starting at col 1 first.
        g.cursor_to(0, 1);
        g.print_grapheme("\u{1F1FA}\u{1F1F8}", Attrs::DEFAULT); // 🇺🇸
        assert!(g.row(0).unwrap().cluster_at(1).is_some());

        // Now overwrite from col 0 with a different wide cluster — the
        // wide-write spacer at col 1 must drop the sidecar at col 1.
        g.cursor_to(0, 0);
        g.print_grapheme("\u{1F468}\u{200D}\u{1F469}", Attrs::DEFAULT); // 👨‍👩
        assert!(g.row(0).unwrap().cluster_at(0).is_some());
        assert!(
            g.row(0).unwrap().cluster_at(1).is_none(),
            "cluster sidecar at orphaned col must not survive wide overwrite"
        );
    }
}
