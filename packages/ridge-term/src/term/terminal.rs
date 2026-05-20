//! Top-level Terminal: stitches the byte-level parser to the grid + modes.

use vte::Parser;

use super::attrs::Attrs;
use super::grid::{Grid, ResizeDiag};
use super::modes::Modes;
use super::parser::Performer;

/// Structured events the parser surfaces to the JS layer. Distinct from
/// `pending_response` which carries raw bytes back to the PTY.
///
/// These events represent semantic changes that need to flow to the UI
/// (pane title bar, Explorer cwd column, audible bell) rather than back
/// to the shell. The JS layer drains them after each `feed()` and
/// dispatches to the relevant Svelte stores.
///
/// Serialization shape (for `serde_wasm_bindgen` → JS):
///   `{ type: "TitleChanged", value: "hello" }`
///   `{ type: "CwdChanged", value: "/C:/code/wind" }`
///   `{ type: "Bell" }`
///
/// **Note on OSC 8 hyperlinks**: open/close used to be reported as
/// `HyperlinkOpen` / `HyperlinkClose` events but the variants were
/// removed in TASKS §3.2 — every consumer reads the per-cell
/// `HyperlinkSpan` annotation via `kernel.hyperlinkAt(row, col)`
/// instead. The cell annotation is what matters; the open/close
/// transitions were redundant noise.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", content = "value")]
pub enum KernelEvent {
    /// OSC 0 (icon + window title) or OSC 2 (window title only).
    TitleChanged(String),
    /// OSC 1 (icon name only). Most apps don't distinguish, but some do.
    IconNameChanged(String),
    /// OSC 7 — current working directory. The wire format is a `file://`
    /// URL; we strip scheme + hostname and pass the local path. On
    /// Windows the path starts with `/C:/...` after stripping; the JS
    /// layer normalizes that.
    CwdChanged(String),
    /// BEL (0x07) outside of OSC string-terminator context. Renderer can
    /// flash, play sound, or no-op depending on UX preference.
    Bell,
}

pub struct Terminal {
    parser: Parser,
    grid: Grid,
    /// Live SGR state. Lives outside the grid because it's parser state,
    /// not screen state. Saved/restored by DECSC/DECRC alongside cursor.
    current_attrs: Attrs,
    /// Terminal modes (DECAWM, cursor visibility, mouse, bracketed paste...).
    modes: Modes,
    /// Viewport scroll offset, in rows. 0 = looking at the live grid;
    /// `n > 0` = pull `n` rows from the top of scrollback in front of
    /// the visible grid (so the user sees history). Capped at
    /// `scrollback.len()`. Reset to 0 on any new output unless
    /// `user_scroll_locked` is set.
    scroll_offset: usize,
    /// User has explicitly scrolled into history (via `scroll_up_view`).
    /// While this is set, incoming PTY bytes do NOT auto-snap the
    /// viewport back to the live grid — so a TUI like `claude` or `top`
    /// that keeps writing to the primary screen can't yank the viewport
    /// out from under a user paging through history.
    ///
    /// Cleared by `scroll_to_bottom` (explicit "follow tail"), by
    /// `scroll_down_view` once `scroll_offset` reaches 0, and on every
    /// alt-screen enter/leave (alt-screen has no scrollback; carrying
    /// the lock across the screen-switch produces a confusing mixed
    /// viewport). Honored only on primary screen — alt-screen feed
    /// bytes still auto-snap.
    user_scroll_locked: bool,
    /// Bytes the parser produced that must be sent BACK to the PTY.
    /// Populated by DSR / DA query responses (CSI 5n, 6n, c, >c). The
    /// JS layer drains this after every feed() and writes it to the PTY
    /// just like user keystrokes — without it, PSReadLine + ConPTY can't
    /// figure out where the cursor is after a child process exits and
    /// renders the prompt at a stale row, on top of the program's output.
    pending_response: Vec<u8>,
    /// Structured events (title, cwd, hyperlinks, bell) the parser
    /// surfaced. Drained alongside `pending_response` by the JS layer.
    pending_events: Vec<KernelEvent>,
    /// Most recently `print()`-ed (char, attrs) pair. REP `CSI <n> b`
    /// repeats this. None resets at LF/CR/erase/control bytes per spec
    /// (we choose to reset only after explicit erase/scroll, not after
    /// CR/LF, to match xterm's "REP after newline still works" behavior).
    last_printed: Option<(char, Attrs)>,
    /// Currently-open OSC 8 hyperlink (uri, optional id). Persists across
    /// feed batches because a TUI may emit `\x1b]8;;uri\x07` in one
    /// chunk and the closing `\x1b]8;;\x07` in another. Cells printed
    /// while this is `Some` get annotated via `Grid::annotate_cell_with_link`.
    current_link: Option<(String, Option<String>)>,
    /// §4.7 (2026-05-07) — grapheme cluster buffer used by the parser
    /// to coalesce multi-codepoint extended grapheme clusters (emoji
    /// ZWJ sequences, RIS pairs, VS-modified emoji) before emitting a
    /// single visual unit to the grid. Persists across feed batches —
    /// a TUI may end one chunk mid-cluster (e.g. "👨\u{200d}") and
    /// finish it in the next chunk ("👩"). Flushed (a) on every
    /// non-print Perform event and (b) at the end of `feed()`.
    grapheme_buf: String,
    /// P3.10 (2026-05-20) — RIS observed since the last drain. Set by
    /// the parser's `esc_dispatch` arm for `ESC c`; drained via
    /// `take_pending_reset` so the producer side (`PaneParser`) can
    /// emit `GridDelta::Reset` ahead of the next frame's diff. Wasm
    /// callers can ignore this — the RIS handler already applies the
    /// full reset inline during `feed`, so the wasm Terminal stays
    /// consistent without consulting the flag.
    pending_reset: bool,
}

impl Terminal {
    pub fn new(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        Self {
            parser: Parser::new(),
            grid: Grid::new(rows, cols, scrollback_lines),
            current_attrs: Attrs::DEFAULT,
            modes: Modes::default(),
            scroll_offset: 0,
            user_scroll_locked: false,
            pending_response: Vec::new(),
            pending_events: Vec::new(),
            last_printed: None,
            current_link: None,
            grapheme_buf: String::new(),
            pending_reset: false,
        }
    }

    /// P3.10 — drain the RIS-observed flag. Used by `PaneParser` to
    /// decide whether the next frame should be prefixed with a
    /// `GridDelta::Reset` so the mirror can clear its state before
    /// applying the post-reset Cells deltas. Returns `false` when
    /// no RIS has happened since the last drain.
    pub fn take_pending_reset(&mut self) -> bool {
        std::mem::replace(&mut self.pending_reset, false)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        // Auto-snap the viewport back to the live grid on new output —
        // matches xterm so log streams don't disappear behind the user
        // while they're paging — UNLESS the user has explicitly paged
        // up via `scroll_up_view`. The lock is only honored on the
        // primary screen; alt-screen TUIs (vim, less) need every byte
        // to land in the viewport they're redrawing.
        let alt_before = self.grid.is_alt_screen();
        if !bytes.is_empty() && self.scroll_offset != 0 {
            let respect_lock = self.user_scroll_locked && !alt_before;
            if !respect_lock {
                self.scroll_offset = 0;
            }
        }
        let mut perf = Performer {
            grid: &mut self.grid,
            current_attrs: &mut self.current_attrs,
            modes: &mut self.modes,
            pending_response: &mut self.pending_response,
            pending_events: &mut self.pending_events,
            last_printed: &mut self.last_printed,
            current_link: &mut self.current_link,
            grapheme_buf: &mut self.grapheme_buf,
            pending_reset: &mut self.pending_reset,
        };
        for &b in bytes {
            self.parser.advance(&mut perf, b);
        }
        // §4.7: flush whatever's in the grapheme buffer at end of feed.
        // Mid-cluster bytes legitimately span feed batches (a multi-MB
        // PTY chunk may split inside a ZWJ sequence) so we don't
        // unconditionally drain on every feed; but at end-of-feed any
        // leftover trailing grapheme should at least be visible. The
        // buffer is preserved for next feed if it ends with an
        // extending codepoint (so a cluster that genuinely spans feeds
        // still resolves correctly when the partner arrives).
        perf.flush_buffer_if_complete();
        // Crossing the alt-screen boundary invalidates the viewport
        // lock: alt-screen has no scrollback, and after we leave it
        // the user expects to be at the live tail of the primary
        // screen (closing vim shouldn't dump them back into stale
        // history they were paging an hour ago).
        if self.grid.is_alt_screen() != alt_before {
            self.user_scroll_locked = false;
            self.scroll_offset = 0;
        }
    }

    /// Drain bytes the parser produced as query responses (DSR/DA) since
    /// the last call. Caller must forward these to the PTY as if user
    /// input. Without forwarding, PowerShell + ConPTY can't track cursor
    /// position across child-process boundaries.
    pub fn take_pending_response(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_response)
    }

    /// P3.4 (2026-05-20) — apply one `GridDelta` produced by a remote
    /// parser to this terminal's state. Counterpart to `feed()` for the
    /// frontend mirror, which receives a stream of deltas from the
    /// Rust-side `PaneParser` over Tauri events instead of running its
    /// own vte parse.
    ///
    /// Variants that affect viewport content (`Cells`, `Cursor`,
    /// `ScreenSwitch`, `Resize`) mutate the grid / cursor / modes
    /// directly. Semantic events (`Title`, `Cwd`, `Bell`) are pushed
    /// into `pending_events` so the JS layer's existing
    /// `take_pending_events` drain wires them to the title/cwd/bell
    /// stores without needing a parallel event channel.
    ///
    /// `ScrollbackAppend`, `ModeChange`, `Reset` are accepted but
    /// only partially applied in v1 — see inline notes. The producer
    /// (`engine::parser::PaneParser`) does not emit those variants yet
    /// either, so the gap is symmetric.
    pub fn apply_delta(&mut self, delta: &crate::term::delta::GridDelta) {
        use super::attrs::Attrs;
        use super::modes::CursorShape as KernelCursorShape;
        use crate::term::delta::{CursorShape as DeltaCursorShape, GridDelta};
        match delta {
            GridDelta::Cells { row, col, cells } => {
                let triples: Vec<(char, Attrs, u8)> = cells
                    .iter()
                    .map(|dc| {
                        (
                            dc.ch,
                            Attrs {
                                fg: dc.fg,
                                bg: dc.bg,
                                flags: dc.flags,
                            },
                            dc.width,
                        )
                    })
                    .collect();
                self.grid
                    .write_delta_cells(*row as usize, *col as usize, &triples);
            }
            GridDelta::Cursor {
                row,
                col,
                visible,
                blink,
                shape,
            } => {
                let cur = self.grid.cursor_mut();
                cur.row = *row as usize;
                cur.col = *col as usize;
                self.modes.cursor_visible = *visible;
                self.modes.cursor_blink = *blink;
                self.modes.cursor_shape = match shape {
                    DeltaCursorShape::Block => KernelCursorShape::Block,
                    DeltaCursorShape::Bar => KernelCursorShape::Bar,
                    DeltaCursorShape::Underline => KernelCursorShape::Underline,
                };
            }
            GridDelta::ScreenSwitch { is_alt } => {
                if *is_alt {
                    // `clear_on_enter = false`: the producer is going
                    // to send Cells deltas describing the alt-screen
                    // contents next. Clearing here would just be
                    // overwritten immediately and waste cycles.
                    self.grid.enter_alt_screen(false);
                } else {
                    self.grid.leave_alt_screen();
                }
            }
            GridDelta::Resize { rows, cols } => {
                self.resize(*rows as usize, *cols as usize);
            }
            GridDelta::Title(t) => {
                self.pending_events
                    .push(KernelEvent::TitleChanged(t.clone()));
            }
            GridDelta::Cwd(p) => {
                self.pending_events
                    .push(KernelEvent::CwdChanged(p.clone()));
            }
            GridDelta::Bell => {
                self.pending_events.push(KernelEvent::Bell);
            }
            GridDelta::Reset => {
                // P3.10 — symmetric counterpart to the parser's RIS
                // handler. Matches `esc_dispatch b'c'` in `parser.rs`:
                // restore all app-controllable state to power-on
                // defaults, clear the visible primary grid, but
                // preserve scrollback (Alacritty-style — see RIS
                // comment in parser.rs:644). The producer always emits
                // Reset BEFORE the post-reset Cells deltas, so by the
                // time those land the mirror is ready to ingest them.
                use super::modes::Modes;
                self.modes = Modes::default();
                self.current_attrs = Attrs::DEFAULT;
                self.grid.set_pen(self.current_attrs);
                self.current_link = None;
                self.last_printed = None;
                self.grid.leave_alt_screen();
                self.grid.set_scroll_region(None, None);
                *self.grid.saved_cursor_mut() = None;
                self.grid.cursor_to(0, 0);
                self.grid.erase_in_display(super::grid::EraseMode::All);
                self.scroll_offset = 0;
                self.user_scroll_locked = false;
                self.pending_reset = false;
            }
            GridDelta::ScrollbackAppend { lines } => {
                // P3.11 — push each line onto the scrollback ring's
                // newest end. Mirror's capacity matches the producer's
                // (both sides see Terminal::new with the same
                // scrollback_lines argument), so when the producer hit
                // an eviction the mirror's `Scrollback::push` hits the
                // same eviction here — no separate eviction signal
                // required on the wire. Attrs get re-interned into the
                // mirror's own AttrTable so the rows are usable through
                // the rest of the grid API (row_at_abs, dump_visible_text,
                // selection text extraction). The intern call dedupes
                // default attrs to AttrId::DEFAULT internally so the
                // ring of blank rows doesn't fragment the table.
                use super::cell::{Cell, Row};
                let cols = self.grid.cols();
                for line in lines {
                    let mut row = Row::new(cols);
                    for (i, dc) in line.iter().take(cols).enumerate() {
                        let attr_id = self.grid.attrs.intern(Attrs {
                            fg: dc.fg,
                            bg: dc.bg,
                            flags: dc.flags,
                        });
                        row.cells[i] = Cell::new(dc.ch, attr_id, dc.width);
                    }
                    let _ = self.grid.scrollback.push(row);
                }
            }
            GridDelta::ModeChange { mode, on } => {
                // P3.12 — symmetric counterpart to PaneParser's
                // `Modes::diff` emission. The mode codes are the same
                // numeric ids an application would use to flip the
                // mode via `CSI ? <n> h/l`, so the apply step is a
                // direct lookup. Unknown codes silently ignored
                // (forward compat — a newer producer ships a mode
                // an older mirror doesn't recognise). Cursor mode
                // bits are NOT routed here because GridDelta::Cursor
                // already carries them.
                self.modes.apply_mode_change(*mode, *on);
            }
        }
    }

    /// Apply every delta in a `DeltaFrame` in order. Convenience
    /// wrapper for the wasm entry point; the version word is checked
    /// against `DeltaFrame::PROTOCOL_VERSION` and a mismatch returns
    /// the encountered version so the caller can log a warning and
    /// skip the frame instead of corrupting the mirror.
    pub fn apply_frame(
        &mut self,
        frame: &crate::term::delta::DeltaFrame,
    ) -> Result<(), u16> {
        if frame.version != crate::term::delta::DeltaFrame::PROTOCOL_VERSION {
            return Err(frame.version);
        }
        for d in &frame.deltas {
            self.apply_delta(d);
        }
        Ok(())
    }

    /// Drain structured semantic events (title / cwd / hyperlinks / bell)
    /// the parser produced. JS layer routes each event to the relevant
    /// Svelte store (paneTitleStore, paneCwdStore, etc.).
    pub fn take_pending_events(&mut self) -> Vec<KernelEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Insert older history rows at the OLDEST end of the scrollback ring.
    ///
    /// The bytes are parsed in an isolated **sandbox** terminal sized the
    /// same as `self`, so the live grid, cursor, attrs, modes, scroll
    /// offset, and pending queues are entirely untouched. Sandbox-side
    /// query responses (DSR/DA) and OSC events (title/cwd/hyperlinks/bell)
    /// are discarded — those describe state at history time, not now.
    ///
    /// AttrIds are remapped from the sandbox's `AttrTable` to `self`'s
    /// (each Terminal owns its own table; the u16 indices don't translate
    /// directly).
    ///
    /// Used by the manager.ts → Tauri `get_pane_scrollback_before` bridge:
    /// when the user pages up past the in-kernel scrollback boundary, we
    /// fetch older bytes from the backend's 4 MiB store and prepend them
    /// here so the viewport can keep scrolling up.
    pub fn prepend_scrollback(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let rows = self.grid.rows();
        let cols = self.grid.cols();

        // Sandbox capacity: enough to absorb every line in `bytes`. Worst
        // case is one row per byte (a stream of bare \n); cap at 64k rows
        // so a 4 MiB chunk doesn't blow up.
        let sandbox_cap = bytes.len().min(64 * 1024).max(rows + 64);
        let mut sandbox = Terminal::new(rows, cols, sandbox_cap);
        sandbox.feed(bytes);

        // Make sure we're on the primary screen before flushing — alt
        // screen output never enters scrollback. If the chunk ended in
        // alt mode we accept losing the trailing alt-screen frame; the
        // history rows the user actually wants are on the primary screen
        // and still in sandbox scrollback at this point.
        if sandbox.is_alt_screen() {
            sandbox.feed(b"\x1b[?1049l");
        }

        // Force-flush the trailing live grid into sandbox scrollback so
        // the very last lines of `bytes` aren't lost. Feeding `rows` LFs
        // scrolls the entire grid up; each LF at the bottom promotes one
        // grid row into scrollback.
        let flush = vec![b'\n'; rows];
        sandbox.feed(&flush);

        // Discard sandbox-side reply queue + event queue; both describe
        // the past, not the live shell session.
        let _ = sandbox.take_pending_response();
        let _ = sandbox.take_pending_events();

        // Trim trailing blank rows produced by the LF flush so the
        // prepended block doesn't end with a slab of empty lines.
        let sandbox_sb_len = sandbox.grid.scrollback.len();
        let mut effective_len = sandbox_sb_len;
        while effective_len > 0 {
            let row = match sandbox.grid.scrollback.get(effective_len - 1) {
                Some(r) => r,
                None => break,
            };
            let blank = row.cells.iter().all(|c| c.is_blank());
            if blank {
                effective_len -= 1;
            } else {
                break;
            }
        }

        // Iterate newest→oldest in the sandbox so each push_front lands
        // the row at the new front of `self.grid.scrollback`. After all
        // inserts the order in self is:
        //   [oldest history, …, newest history, …existing scrollback…]
        for idx in (0..effective_len).rev() {
            if let Some(src) = sandbox.grid.scrollback.get(idx) {
                let mut row = src.clone();
                for cell in row.cells.iter_mut() {
                    if cell.attr != crate::term::attr_table::AttrId::DEFAULT {
                        let attrs = sandbox.grid.attrs.get(cell.attr);
                        cell.attr = self.grid.attrs.intern(attrs);
                    }
                }
                self.grid.scrollback.push_front(row);
            }
        }
    }

    /// Scroll viewport up (toward older history). `n` rows; clamped at
    /// scrollback length. Engages the user-scroll lock so subsequent
    /// PTY output won't auto-snap the viewport back to the tail.
    pub fn scroll_up_view(&mut self, n: usize) {
        let max = self.grid.scrollback.len();
        self.scroll_offset = (self.scroll_offset + n).min(max);
        if self.scroll_offset > 0 {
            self.user_scroll_locked = true;
        }
    }

    /// Scroll viewport down (toward live grid). Clamped at 0. Releases
    /// the lock once the user has paged all the way back to the tail.
    pub fn scroll_down_view(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        if self.scroll_offset == 0 {
            self.user_scroll_locked = false;
        }
    }

    /// Snap viewport back to live grid (= 0 offset). Releases the
    /// user-scroll lock — explicit "follow tail" intent.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
        self.user_scroll_locked = false;
    }

    /// §B.2 (2026-05-08) — drop the entire scrollback ring buffer
    /// (physical clear) and snap the viewport back to the live grid.
    /// Mirrors what `\x1b[3J` (xterm "Erase Saved Lines") does at the
    /// kernel level, but invocable directly from the JS layer so the
    /// right-click "清空" path doesn't need to go through PTY round
    /// trip. Live grid is untouched — caller can pair with
    /// `erase_in_display(All)` + `cursor_to(0,0)` for a full wipe.
    pub fn clear_scrollback(&mut self) {
        self.grid.scrollback.clear();
        self.scroll_offset = 0;
        self.user_scroll_locked = false;
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }
    pub fn scrollback_len(&self) -> usize {
        self.grid.scrollback.len()
    }

    /// §B.2 (2026-05-08) — monotonically-increasing count of rows
    /// evicted from the scrollback's oldest end. Used by the JS-facing
    /// `feed` path to invalidate selection / search anchors only when
    /// a feed actually caused a row to scroll off (preserving them
    /// across TUI redraws that don't push to scrollback at all —
    /// which is most of them).
    pub fn scrollback_eviction_count(&self) -> u64 {
        self.grid.scrollback.eviction_count()
    }
    /// Whether the user has paged into history and PTY output is
    /// currently being held back from auto-snapping the viewport.
    /// JS-side may surface this as a "x lines below — click to follow"
    /// indicator.
    pub fn is_user_scroll_locked(&self) -> bool {
        self.user_scroll_locked
    }

    /// Look up a row by absolute-row coord (matches `search.rs` /
    /// `selection.rs` abs encoding):
    ///   * `0..scrollback_len()` → scrollback row, oldest-first
    ///   * `scrollback_len()..scrollback_len()+rows()` → live grid row
    ///
    /// Used by `Selection::text` so cross-scrollback selections can read
    /// their cells without re-deriving the offset arithmetic each call.
    pub fn row_at_abs(&self, abs_row: usize) -> Option<&crate::term::cell::Row> {
        let sb_len = self.grid.scrollback.len();
        if abs_row < sb_len {
            self.grid.scrollback.get(abs_row)
        } else {
            self.grid.row(abs_row - sb_len)
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        // §A.3 (2026-05-07): drive the inline-TUI heuristic into
        // `Grid::resize_with_inline_tui` so primary-screen Ink apps
        // (Claude Code's input box) get the same SIGWINCH-blank-canvas
        // treatment alt-screen TUIs already have via §1.22. Sampled
        // here (not from JS) so the kernel and the wipe see the same
        // mode snapshot — an OSC arriving between the JS query and
        // the wasm resize call cannot desync the decision.
        let now_ms = super::clock::now_ms();
        let inline_tui_active = self
            .grid
            .is_inline_tui_active_at(now_ms, self.modes.cursor_visible);
        self.grid
            .resize_with_inline_tui(rows, cols, inline_tui_active);
    }

    /// Inline-TUI heuristic snapshot, exposed for the JS layer so
    /// `manager.ts::fitPane` can broaden the wipe-first ordering branch
    /// (§A.3) to cover Ink apps in addition to alt-screen TUIs. See
    /// `Grid::is_inline_tui_active_at` for the heuristic itself.
    pub fn is_inline_tui_mode_at(&self, now_ms: i64) -> bool {
        self.grid
            .is_inline_tui_active_at(now_ms, self.modes.cursor_visible)
    }

    /// Diagnostic accessor — returns the most recent `Grid::resize` calls.
    /// Used by `JsTerminal::lastResizeDiags` to surface live-repro evidence
    /// to frontend devtools, and by integration tests to verify which
    /// branch fired in a given scenario.
    pub fn last_resize_diags(&self) -> &[ResizeDiag] {
        self.grid.last_resize_diags()
    }

    pub fn rows(&self) -> usize {
        self.grid.rows()
    }
    pub fn cols(&self) -> usize {
        self.grid.cols()
    }
    pub fn grid(&self) -> &Grid {
        &self.grid
    }
    pub fn modes(&self) -> &Modes {
        &self.modes
    }
    pub fn is_alt_screen(&self) -> bool {
        self.grid.is_alt_screen()
    }

    /// Renderer entry point: returns the row at viewport-relative index
    /// `vp_row` (0..rows), accounting for `scroll_offset`. When the user
    /// has scrolled into history, the top portion of the viewport pulls
    /// from scrollback (newest-first) and the bottom portion still shows
    /// live grid rows.
    ///
    /// Layout when scroll_offset = N (N > 0):
    ///   vp_row 0     → scrollback[len - N]            (oldest visible)
    ///   vp_row N-1   → scrollback[len - 1]            (most recent scrollback entry)
    ///   vp_row N     → grid.row(0)
    ///   vp_row rows-1 → grid.row(rows - 1 - N)        (live cells, shifted down)
    /// When scroll_offset = 0, every vp_row maps directly to the grid.
    pub fn viewport_row(&self, vp_row: usize) -> Option<&crate::term::cell::Row> {
        if vp_row >= self.grid.rows() {
            return None;
        }
        let offset = self.scroll_offset;
        if offset == 0 {
            return self.grid.row(vp_row);
        }
        if vp_row < offset {
            // Pulled from scrollback. scrollback is oldest-first, so
            // index = len - offset + vp_row gives "the (offset - vp_row)
            // most recent" entry.
            let sb_len = self.grid.scrollback.len();
            // scroll_offset is clamped at sb_len in `scroll_up_view`, so
            // this subtraction is safe.
            let idx = sb_len - offset + vp_row;
            self.grid.scrollback.get(idx)
        } else {
            // Live grid, shifted up by `offset`.
            self.grid.row(vp_row - offset)
        }
    }

    /// Test/debug helper.
    pub fn dump_visible_text(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.grid.rows());
        for r in 0..self.grid.rows() {
            let row = self.grid.row(r).unwrap();
            let mut s = String::with_capacity(row.cells.len());
            for cell in &row.cells {
                if cell.width == 0 {
                    continue;
                }
                s.push(cell.ch);
            }
            out.push(s.trim_end_matches(' ').to_string());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::attrs::{Color, Flags};

    #[test]
    fn apply_delta_scrollback_append_pushes_lines_and_preserves_visible_grid() {
        use crate::term::attrs::{Color, Flags};
        use crate::term::delta::{DeltaCell, GridDelta};
        let mut t = Terminal::new(2, 5, 100);
        // Establish some visible-grid content to make sure apply doesn't
        // disturb it. 'XX' lives on row 0; row 1 is blank.
        t.feed(b"XX");
        let visible_before: Vec<String> = t.dump_visible_text();
        let sb_before = t.scrollback_len();
        // Apply two new scrollback rows. Default attrs everywhere so
        // the intern call hits AttrId::DEFAULT.
        let line_a: Vec<DeltaCell> = "ALPHA"
            .chars()
            .map(|ch| DeltaCell {
                ch,
                fg: Color::DEFAULT,
                bg: Color::DEFAULT,
                flags: Flags::empty(),
                width: 1,
            })
            .collect();
        let line_b: Vec<DeltaCell> = "BETA "
            .chars()
            .map(|ch| DeltaCell {
                ch,
                fg: Color::DEFAULT,
                bg: Color::DEFAULT,
                flags: Flags::empty(),
                width: 1,
            })
            .collect();
        t.apply_delta(&GridDelta::ScrollbackAppend {
            lines: vec![line_a, line_b],
        });
        assert_eq!(
            t.scrollback_len(),
            sb_before + 2,
            "scrollback length must grow by exactly the number of applied lines",
        );
        // Live grid untouched.
        assert_eq!(t.dump_visible_text(), visible_before);
        // Most-recent applied line lands at the newest scrollback end.
        let newest = t.grid().scrollback.get(t.scrollback_len() - 1).unwrap();
        let newest_text: String = newest.cells.iter().map(|c| c.ch).collect();
        assert_eq!(newest_text, "BETA ");
    }

    #[test]
    fn apply_delta_reset_clears_visible_grid_and_modes_but_keeps_scrollback() {
        // P3.10 — `Terminal::apply_delta(Reset)` must mirror the
        // parser's RIS handler exactly: wipe visible grid + reset
        // modes + reset cursor + close any hyperlink span, but
        // PRESERVE scrollback (Alacritty-style retention).
        let mut t = Terminal::new(2, 5, 100);
        // Push enough content to land some rows in scrollback.
        t.feed(b"a\r\nb\r\nc\r\nd");
        // Tweak modes so we can confirm they reset (cursor blink off
        // is a non-default state).
        t.feed(b"\x1b[?25l"); // cursor invisible
        t.feed(b"\x1b[?2004h"); // bracketed paste on
        assert!(!t.modes().cursor_visible);
        assert!(t.modes().bracketed_paste);
        let sb_before = t.scrollback_len();
        assert!(sb_before > 0, "expected non-zero scrollback");

        // Apply Reset directly (simulates a producer-emitted frame).
        t.apply_delta(&crate::term::delta::GridDelta::Reset);

        // Modes back to default.
        assert!(t.modes().cursor_visible, "cursor visibility should be reset to default");
        assert!(!t.modes().bracketed_paste, "bracketed paste should be reset to default");
        // Cursor home.
        assert_eq!(t.grid().cursor().row, 0);
        assert_eq!(t.grid().cursor().col, 0);
        // Visible primary grid wiped — every cell on every row blank.
        for r in 0..2 {
            let row = t.grid().row(r).unwrap();
            for cell in &row.cells {
                assert!(cell.is_blank(), "row {r} cell must be blank after Reset");
            }
        }
        // Scrollback preserved.
        assert_eq!(
            t.scrollback_len(),
            sb_before,
            "Reset must NOT touch scrollback (Alacritty-style retention)",
        );
        // Viewport snaps to live grid (Reset wipes any user-scroll lock).
        assert_eq!(t.scroll_offset(), 0);
        assert!(!t.is_user_scroll_locked());
    }

    #[test]
    fn plain_text_lands_on_first_row() {
        let mut t = Terminal::new(4, 10, 0);
        t.feed(b"hello");
        assert_eq!(t.dump_visible_text()[0], "hello");
        assert_eq!(t.grid().cursor().col, 5);
    }

    #[test]
    fn crlf_moves_to_next_row_col_0() {
        let mut t = Terminal::new(4, 10, 0);
        t.feed(b"ab\r\ncd");
        let lines = t.dump_visible_text();
        assert_eq!(lines[0], "ab");
        assert_eq!(lines[1], "cd");
    }

    #[test]
    fn lf_at_bottom_scrolls_to_scrollback() {
        let mut t = Terminal::new(2, 5, 10);
        t.feed(b"a\r\nb\r\nc");
        let lines = t.dump_visible_text();
        assert_eq!(lines[0], "b");
        assert_eq!(lines[1], "c");
        assert_eq!(t.grid().scrollback.len(), 1);
        assert_eq!(t.grid().scrollback.get(0).unwrap().cells[0].ch, 'a');
    }

    #[test]
    fn sgr_red_applies_to_following_cells() {
        let mut t = Terminal::new(2, 10, 0);
        t.feed(b"\x1b[31mhi\x1b[0mok");
        let row = t.grid().row(0).unwrap();
        let red_attrs = t.grid().attrs.get(row.cells[0].attr);
        assert_eq!(red_attrs.fg, Color::indexed(1));
        let after_attrs = t.grid().attrs.get(row.cells[2].attr);
        assert_eq!(after_attrs, Attrs::DEFAULT);
    }

    #[test]
    fn cursor_position_is_one_based_on_wire() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b[3;3H*");
        assert_eq!(t.dump_visible_text()[2], "  *");
    }

    #[test]
    fn pending_wrap_then_print_wraps_correctly() {
        let mut t = Terminal::new(3, 4, 0);
        t.feed(b"abcde");
        let lines = t.dump_visible_text();
        assert_eq!(lines[0], "abcd");
        assert_eq!(lines[1], "e");
    }

    #[test]
    fn bold_flag_set_by_sgr_1() {
        let mut t = Terminal::new(1, 5, 0);
        t.feed(b"\x1b[1mB");
        let a = t.grid().attrs.get(t.grid().row(0).unwrap().cells[0].attr);
        assert!(a.flags.contains(Flags::BOLD));
    }

    // ─── viewport scroll + viewport_row mixed-mode ────────────────────

    #[test]
    fn decrqm_mode_2027_responds_permanent_set() {
        // §B.6 — DECRQM `CSI ? 2027 $p` must respond `CSI ? 2027 ; 3 $y`
        // (3 = permanently set). PSReadLine 2.3.6+ checks this at
        // startup; Ps=3 is the strongest signal that the terminal
        // handles grapheme-cluster width correctly, fixing the
        // canonical Windows ".NET counts surrogates as 2 chars × 2
        // cells = 4 cell width" cursor drift on non-BMP emoji like
        // 🎂 / 👈 / 🚀 (the user's report).
        let mut t = Terminal::new(2, 80, 0);
        t.feed(b"\x1b[?2027$p");
        let resp = t.take_pending_response();
        assert_eq!(
            resp,
            b"\x1b[?2027;3$y",
            "Mode 2027 query must report permanent-set (3)"
        );
    }

    #[test]
    fn decrqm_mode_2026_responds_actual_state() {
        // Sanity — mode-mutable modes report their actual state (1=set,
        // 2=reset). Sync output (2026) starts off → Ps=2.
        let mut t = Terminal::new(2, 80, 0);
        t.feed(b"\x1b[?2026$p");
        assert_eq!(t.take_pending_response(), b"\x1b[?2026;2$y");
        // Enable, query again.
        t.feed(b"\x1b[?2026h");
        t.feed(b"\x1b[?2026$p");
        assert_eq!(t.take_pending_response(), b"\x1b[?2026;1$y");
    }

    #[test]
    fn decrqm_unknown_mode_responds_zero() {
        // Unknown/unsupported modes get Ps=0 ("not recognised") so
        // apps know not to depend on the feature.
        let mut t = Terminal::new(2, 80, 0);
        t.feed(b"\x1b[?9999$p");
        assert_eq!(t.take_pending_response(), b"\x1b[?9999;0$y");
    }

    #[test]
    fn scrollback_eviction_count_zero_when_under_capacity() {
        // §B.2 — feeds that scroll content into scrollback below
        // capacity must NOT advance the eviction counter. Selection
        // anchors stay valid through this case.
        let mut t = Terminal::new(2, 5, 100);
        t.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\n");
        // Plenty of rows in scrollback, none evicted.
        assert!(t.scrollback_len() > 0);
        assert_eq!(
            t.scrollback_eviction_count(),
            0,
            "scrolling INTO scrollback under capacity must NOT count as eviction"
        );
    }

    #[test]
    fn scrollback_eviction_count_advances_only_on_capacity_rollover() {
        // §B.2 — Three-row capacity, push 5 rows worth of scroll content.
        // First 3 fill, last 2 evict (each overwriting one head slot).
        let mut t = Terminal::new(2, 5, 3);
        t.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng");
        // After feeding 7 lines into a 2-row viewport with 3-row scrollback,
        // some lines have been evicted. Exact count depends on the parser
        // path but must be > 0 since scrollback is full.
        assert_eq!(t.scrollback_len(), 3);
        assert!(
            t.scrollback_eviction_count() > 0,
            "filling past capacity must advance the eviction counter"
        );
    }

    #[test]
    fn alt_screen_redraw_never_advances_eviction_count() {
        // §B.2 regression — real TUI apps (vim/htop/less) swap to the
        // alt screen via DECSET 1049, where scrollback push is
        // unconditionally disabled (`scroll_region_up` checks
        // `!self.is_alt`). Hundreds of full-screen frames must not
        // touch the eviction counter, so JsTerminal::feed keeps
        // selection anchors alive across the whole TUI session.
        let mut t = Terminal::new(10, 80, 100);
        // Generous cols/rows so wrap doesn't accidentally cross the
        // scroll boundary; capacity 100 so we'd notice runaway
        // eviction immediately.
        t.feed(b"\x1b[?1049h"); // swap to alt screen
        let evictions_before = t.scrollback_eviction_count();

        for _ in 0..200 {
            // Realistic TUI frame: cursor home + ED 2 + four lines.
            t.feed(b"\x1b[H\x1b[2Jhello world\r\nline two\r\nline three\r\nline four\r\n");
        }

        assert_eq!(
            t.scrollback_eviction_count(),
            evictions_before,
            "alt-screen redraws must NEVER advance the eviction counter — \
             that's the entire point of alt-screen scrollback isolation"
        );
    }

    #[test]
    fn primary_screen_in_viewport_redraw_does_not_advance_eviction_count() {
        // §B.2 — even on primary screen (where scrollback IS active),
        // a TUI that redraws strictly inside the viewport without
        // crossing scroll_bottom does NOT push to scrollback. This
        // covers shell-like apps (claude code's inline frames, fzf in
        // height mode, etc.) that don't use the alt screen but still
        // produce zero scroll churn.
        let mut t = Terminal::new(10, 80, 100);
        let evictions_before = t.scrollback_eviction_count();

        for _ in 0..200 {
            // Cursor home + 4 lines, no trailing \n on the last line —
            // cursor never reaches scroll_bottom so no LF-driven scroll
            // ever fires.
            t.feed(b"\x1b[H\x1b[2Jhello world\r\nline two\r\nline three\r\nline four");
        }

        assert_eq!(
            t.scrollback_eviction_count(),
            evictions_before,
            "primary-screen in-viewport redraws must NOT advance counter"
        );
    }

    #[test]
    fn ed_3_clears_scrollback_physically_and_keeps_visible_grid() {
        // §B.2 — `\x1b[3J` (Erase Saved Lines) must drop the entire
        // ring buffer but leave the visible grid intact. Pre-fix the
        // parser silently demoted this to ED 2, leaving scrollback
        // untouched — exactly the user-reported "clear 不能完全清理".
        let mut t = Terminal::new(2, 5, 100);
        t.feed(b"a\r\nb\r\nc\r\nd"); // 'a' and 'b' spill into scrollback
        assert!(t.scrollback_len() >= 2);
        let visible_before: Vec<String> = t.dump_visible_text();

        t.feed(b"\x1b[3J");

        assert_eq!(
            t.scrollback_len(),
            0,
            "ED 3 must physically clear the scrollback ring"
        );
        // Visible grid AND cursor untouched.
        assert_eq!(
            t.dump_visible_text(),
            visible_before,
            "ED 3 must NOT touch the visible grid"
        );
    }

    #[test]
    fn ed_2_does_not_touch_scrollback() {
        // §B.2 — sanity: ED 2 (clear screen) must leave scrollback
        // intact. Only ED 3 reaches the saved lines.
        let mut t = Terminal::new(2, 5, 100);
        t.feed(b"a\r\nb\r\nc\r\nd");
        let sb_before = t.scrollback_len();

        t.feed(b"\x1b[2J");

        assert_eq!(
            t.scrollback_len(),
            sb_before,
            "ED 2 must NOT touch scrollback"
        );
    }

    #[test]
    fn clear_scrollback_api_drops_history_and_resets_offset() {
        // §B.2 — direct JS-facing API.
        let mut t = Terminal::new(2, 5, 100);
        t.feed(b"a\r\nb\r\nc\r\nd");
        t.scroll_up_view(2); // page into history
        assert!(t.scroll_offset() > 0);

        t.clear_scrollback();

        assert_eq!(t.scrollback_len(), 0);
        assert_eq!(t.scroll_offset(), 0, "viewport must snap back to live grid");
    }

    #[test]
    fn ed_3_on_alt_screen_preserves_scrollback() {
        // §B.2 — alt screen has no scrollback; ED 3 there must be a
        // no-op so apps that swap back to primary still see their
        // history (kakoune/vim/less rely on this).
        let mut t = Terminal::new(2, 5, 100);
        t.feed(b"a\r\nb\r\nc\r\nd");
        let sb_before = t.scrollback_len();
        // Swap to alt screen (DECSET 1049).
        t.feed(b"\x1b[?1049h");
        t.feed(b"\x1b[3J");

        // Swap back; primary scrollback must still have its rows.
        t.feed(b"\x1b[?1049l");
        assert_eq!(
            t.scrollback_len(),
            sb_before,
            "ED 3 on alt screen must not touch primary scrollback"
        );
    }

    #[test]
    fn scroll_up_view_clamps_at_scrollback_length() {
        let mut t = Terminal::new(2, 5, 5);
        // Push 3 lines so 1 lands in scrollback (3 - 2 viewport rows = 1).
        t.feed(b"a\r\nb\r\nc");
        let sb_len = t.scrollback_len();
        // Try to scroll past the available history.
        t.scroll_up_view(100);
        assert_eq!(
            t.scroll_offset(),
            sb_len,
            "scroll_up_view must clamp at scrollback_len"
        );
    }

    #[test]
    fn scroll_down_view_saturates_at_zero() {
        let mut t = Terminal::new(2, 5, 0);
        t.scroll_down_view(50);
        assert_eq!(t.scroll_offset(), 0);
    }

    #[test]
    fn scroll_to_bottom_resets_offset() {
        let mut t = Terminal::new(2, 5, 5);
        t.feed(b"a\r\nb\r\nc\r\nd");
        t.scroll_up_view(2);
        assert!(t.scroll_offset() > 0);
        t.scroll_to_bottom();
        assert_eq!(t.scroll_offset(), 0);
    }

    // ─── user-scroll lock ─────────────────────────────────────────────

    #[test]
    fn feed_holds_offset_while_user_scroll_locked() {
        // Push enough lines to populate scrollback, page up, then keep
        // feeding. The lock must hold the viewport at the offset the
        // user chose (auto-snap suppressed).
        let mut t = Terminal::new(2, 5, 16);
        t.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf");
        t.scroll_up_view(2);
        let locked_offset = t.scroll_offset();
        assert!(locked_offset > 0);
        assert!(t.is_user_scroll_locked());
        // Feed more content as if a TUI were repainting the live grid.
        t.feed(b"\r\nstreamed-output");
        assert_eq!(
            t.scroll_offset(),
            locked_offset,
            "PTY output must not auto-snap while the user-scroll lock is set",
        );
    }

    #[test]
    fn feed_auto_snaps_without_lock() {
        // Without the lock, behavior matches xterm: any new output
        // pulls the viewport back to the live grid.
        let mut t = Terminal::new(2, 5, 8);
        t.feed(b"a\r\nb\r\nc\r\nd");
        // Set offset directly to simulate a different code path; lock
        // stays clear.
        t.scroll_offset = 1;
        assert!(!t.is_user_scroll_locked());
        t.feed(b"x");
        assert_eq!(t.scroll_offset(), 0);
    }

    #[test]
    fn scroll_to_bottom_releases_lock() {
        let mut t = Terminal::new(2, 5, 5);
        t.feed(b"a\r\nb\r\nc\r\nd");
        t.scroll_up_view(2);
        assert!(t.is_user_scroll_locked());
        t.scroll_to_bottom();
        assert!(!t.is_user_scroll_locked());
    }

    #[test]
    fn scroll_down_view_releases_lock_at_zero() {
        let mut t = Terminal::new(2, 5, 5);
        t.feed(b"a\r\nb\r\nc\r\nd");
        t.scroll_up_view(2);
        assert!(t.is_user_scroll_locked());
        // Step down by 1 — still in history, lock stays.
        t.scroll_down_view(1);
        assert!(t.is_user_scroll_locked() || t.scroll_offset() == 0);
        // Reach the tail — lock releases.
        t.scroll_down_view(99);
        assert_eq!(t.scroll_offset(), 0);
        assert!(!t.is_user_scroll_locked());
    }

    #[test]
    fn alt_screen_enter_clears_lock_and_offset() {
        // User pages up on primary, then a TUI swaps to alt-screen via
        // CSI ?1049h. The lock must drop so the alt-screen feed paints
        // a clean viewport.
        let mut t = Terminal::new(2, 5, 5);
        t.feed(b"a\r\nb\r\nc\r\nd");
        t.scroll_up_view(2);
        assert!(t.is_user_scroll_locked());
        // CSI ?1049h: enter alt-screen + clear.
        t.feed(b"\x1b[?1049h");
        assert!(t.is_alt_screen());
        assert!(!t.is_user_scroll_locked());
        assert_eq!(t.scroll_offset(), 0);
    }

    #[test]
    fn alt_screen_feed_ignores_lock() {
        // While on alt-screen the lock must NOT inhibit auto-snap —
        // alt-screen TUIs assume the entire viewport tracks the live
        // alt buffer.
        let mut t = Terminal::new(2, 5, 5);
        t.feed(b"\x1b[?1049h");
        // Force the (otherwise impossible) state: alt-screen + locked.
        t.user_scroll_locked = true;
        t.scroll_offset = 1;
        t.feed(b"x");
        assert_eq!(
            t.scroll_offset(),
            0,
            "alt-screen feed must auto-snap regardless of lock"
        );
    }

    #[test]
    fn viewport_row_offset_zero_passes_through_grid() {
        let mut t = Terminal::new(2, 3, 0);
        t.feed(b"ab\r\ncd");
        // No scrollback, offset 0 — viewport_row(i) == grid.row(i).
        assert_eq!(t.viewport_row(0).unwrap().cells[0].ch, 'a');
        assert_eq!(t.viewport_row(1).unwrap().cells[0].ch, 'c');
    }

    #[test]
    fn viewport_row_past_rows_returns_none() {
        let t = Terminal::new(2, 5, 0);
        // Only rows 0..2 exist.
        assert!(t.viewport_row(2).is_none());
        assert!(t.viewport_row(99).is_none());
    }

    #[test]
    fn viewport_row_mixed_scrollback_plus_grid_at_offset_1() {
        // 2-row viewport with capacity-5 scrollback. Push 'a' onto its
        // own row, then 'b\r\nc' fills the viewport with [b, c]. After
        // the second LF push 'd': [c, d] viewport, ['a', 'b'] scrollback.
        let mut t = Terminal::new(2, 3, 5);
        t.feed(b"a\r\nb\r\nc\r\nd");
        let sb = t.scrollback_len();
        assert!(sb >= 1);
        t.scroll_up_view(1);
        // offset=1 → vp_row 0 pulls scrollback[sb-1] (most recent
        // scrollback row = 'b'), vp_row 1 pulls grid.row(0) = 'c'.
        let top = t.viewport_row(0).unwrap();
        let bot = t.viewport_row(1).unwrap();
        assert_eq!(top.cells[0].ch, 'b');
        assert_eq!(bot.cells[0].ch, 'c');
    }

    #[test]
    fn viewport_row_at_max_offset_pulls_top_rows_from_scrollback() {
        // Push enough lines so scrollback fills with several entries.
        // Then scroll all the way up and verify the top row is the
        // OLDEST scrollback entry (not the most recent).
        let mut t = Terminal::new(2, 3, 10);
        t.feed(b"a\r\nb\r\nc\r\nd\r\ne");
        // Scrollback now contains ['a', 'b', 'c'] (oldest-first), grid
        // shows ['d', 'e']. Scroll back the full sb_len.
        let sb_len = t.scrollback_len();
        t.scroll_up_view(sb_len);
        // vp_row 0 at full offset pulls scrollback[sb_len - sb_len + 0]
        // = scrollback[0] = oldest = 'a'.
        let top = t.viewport_row(0).unwrap();
        assert_eq!(top.cells[0].ch, 'a');
    }

    #[test]
    fn sgr_4_sets_underline() {
        // Bare CSI 4 m — single underline on. Baseline behaviour.
        let mut t = Terminal::new(1, 5, 0);
        t.feed(b"\x1b[4mU");
        let a = t.grid().attrs.get(t.grid().row(0).unwrap().cells[0].attr);
        assert!(a.flags.contains(Flags::UNDERLINE));
        assert!(!a.flags.contains(Flags::DBL_UNDERLINE));
    }

    #[test]
    fn sgr_4_zero_clears_underline() {
        // Extended-underline OFF (CSI 4:0 m). Modern CLIs including
        // Claude Code use this to release a hyperlink underline cleanly.
        // Pre-fix this routed into the "code == 4" arm and flipped
        // underline ON instead of OFF — see TASKS §1.18.
        let mut t = Terminal::new(1, 5, 0);
        t.feed(b"\x1b[4mA\x1b[4:0mB");
        let row = t.grid().row(0).unwrap();
        let a = t.grid().attrs.get(row.cells[0].attr);
        let b = t.grid().attrs.get(row.cells[1].attr);
        assert!(
            a.flags.contains(Flags::UNDERLINE),
            "first cell must be underlined"
        );
        assert!(
            !b.flags.contains(Flags::UNDERLINE),
            "after CSI 4:0 m the next cell must NOT be underlined"
        );
    }

    #[test]
    fn sgr_4_two_sets_double_underline() {
        // CSI 4:2 m → double underline ON, single underline OFF.
        let mut t = Terminal::new(1, 5, 0);
        t.feed(b"\x1b[4:2mD");
        let a = t.grid().attrs.get(t.grid().row(0).unwrap().cells[0].attr);
        assert!(a.flags.contains(Flags::DBL_UNDERLINE));
        assert!(!a.flags.contains(Flags::UNDERLINE));
    }

    #[test]
    fn sgr_4_curly_degrades_to_single() {
        // CSI 4:3 m (curly) — renderer doesn't yet support curly, but
        // we still treat the cell as underlined (single) so the user
        // sees *something*. Better to over-style than to silently drop
        // the intent until the renderer ships curly.
        let mut t = Terminal::new(1, 5, 0);
        t.feed(b"\x1b[4:3mC");
        let a = t.grid().attrs.get(t.grid().row(0).unwrap().cells[0].attr);
        assert!(a.flags.contains(Flags::UNDERLINE));
        assert!(!a.flags.contains(Flags::DBL_UNDERLINE));
    }

    #[test]
    fn sgr_24_clears_underline_after_4_zero_no_op() {
        // Belt-and-suspenders: if a CLI emits CSI 4:0 m followed by
        // CSI 24 m (the canonical "underline off"), state is consistent.
        let mut t = Terminal::new(1, 5, 0);
        t.feed(b"\x1b[4mA\x1b[4:0m\x1b[24mB");
        let b = t.grid().attrs.get(t.grid().row(0).unwrap().cells[1].attr);
        assert!(!b.flags.contains(Flags::UNDERLINE));
        assert!(!b.flags.contains(Flags::DBL_UNDERLINE));
    }

    #[test]
    fn dec_1049_enters_alt_screen() {
        let mut t = Terminal::new(3, 5, 10);
        t.feed(b"home");
        t.feed(b"\x1b[?1049h"); // enter alt + clear
        assert!(t.is_alt_screen());
        // Alt screen is blank.
        assert_eq!(t.dump_visible_text()[0], "");
        t.feed(b"vim");
        t.feed(b"\x1b[?1049l"); // leave alt
        assert!(!t.is_alt_screen());
        // Primary intact.
        assert_eq!(t.dump_visible_text()[0], "home");
    }

    #[test]
    fn cursor_visibility_mode_25() {
        let mut t = Terminal::new(2, 5, 0);
        assert!(t.modes().cursor_visible);
        t.feed(b"\x1b[?25l");
        assert!(!t.modes().cursor_visible);
        t.feed(b"\x1b[?25h");
        assert!(t.modes().cursor_visible);
    }

    #[test]
    fn bracketed_paste_mode_2004() {
        let mut t = Terminal::new(2, 5, 0);
        assert!(!t.modes().bracketed_paste);
        t.feed(b"\x1b[?2004h");
        assert!(t.modes().bracketed_paste);
    }

    #[test]
    fn decstbm_sets_scroll_region() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b[2;4r"); // rows 2..4 (1-based) = 1..3 (0-based)
                              // After STBM, cursor should be at home.
        assert_eq!(t.grid().cursor().row, 0);
        assert_eq!(t.grid().cursor().col, 0);
    }

    #[test]
    fn dsr_cursor_position_report_matches_cursor_one_based() {
        // CSI 6n must reply CSI <row>;<col> R using 1-based coordinates.
        // PowerShell + ConPTY rely on this to anchor the prompt after a
        // child process exits — the bug repro that motivated this code.
        let mut t = Terminal::new(10, 20, 0);
        // Move cursor to row 5 col 3 (1-based on the wire = 0-based 4,2).
        t.feed(b"\x1b[5;3H");
        // Issue the DSR cursor-position query.
        t.feed(b"\x1b[6n");
        let resp = t.take_pending_response();
        assert_eq!(resp, b"\x1b[5;3R", "DSR-CPR must echo back 1-based row;col");
        // Drain is one-shot.
        assert!(t.take_pending_response().is_empty());
    }

    #[test]
    fn dsr_status_report_replies_zero_n() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b[5n");
        assert_eq!(t.take_pending_response(), b"\x1b[0n");
    }

    #[test]
    fn primary_da_replies_xterm_compatible() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b[c");
        let resp = t.take_pending_response();
        // Must start with CSI ? — that's the discriminator shells look at.
        assert!(resp.starts_with(b"\x1b[?"), "DA reply must be CSI ?... ");
        assert!(resp.ends_with(b"c"));
    }

    #[test]
    fn rep_repeats_last_printed_char() {
        let mut t = Terminal::new(2, 10, 0);
        // Print one char, then REP 4 → should yield 5 of them total.
        t.feed(b"-");
        t.feed(b"\x1b[4b");
        assert_eq!(t.dump_visible_text()[0], "-----");
        // Default count = 1 if missing.
        t.feed(b"\x1b[H"); // cursor home (1;1)
        t.feed(b"X");
        t.feed(b"\x1b[b");
        // Row 0 is now "XX---" (X X then leftover dashes).
        assert_eq!(&t.dump_visible_text()[0][..2], "XX");
    }

    #[test]
    fn decscusr_sets_cursor_shape_and_blink() {
        use crate::term::modes::CursorShape;
        let mut t = Terminal::new(5, 5, 0);
        // Default: block, blink on.
        assert_eq!(t.modes().cursor_shape, CursorShape::Block);
        assert!(t.modes().cursor_blink);

        // CSI 2 SP q  → steady block
        t.feed(b"\x1b[2 q");
        assert_eq!(t.modes().cursor_shape, CursorShape::Block);
        assert!(!t.modes().cursor_blink);

        // CSI 5 SP q  → blinking bar (vim insert)
        t.feed(b"\x1b[5 q");
        assert_eq!(t.modes().cursor_shape, CursorShape::Bar);
        assert!(t.modes().cursor_blink);

        // CSI 4 SP q  → steady underline
        t.feed(b"\x1b[4 q");
        assert_eq!(t.modes().cursor_shape, CursorShape::Underline);
        assert!(!t.modes().cursor_blink);

        // Out-of-range value falls back to default (blink block).
        t.feed(b"\x1b[99 q");
        assert_eq!(t.modes().cursor_shape, CursorShape::Block);
        assert!(t.modes().cursor_blink);
    }

    #[test]
    fn osc_2_emits_title_changed_event() {
        // OSC 2 is the most common: shells emit it for prompt + cmd updates.
        // String terminator (ST) here is BEL (0x07); ESC \ also valid.
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b]2;hello world\x07");
        let events = t.take_pending_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            KernelEvent::TitleChanged(s) => assert_eq!(s, "hello world"),
            other => panic!("expected TitleChanged, got {other:?}"),
        }
        assert!(t.take_pending_events().is_empty(), "drain is one-shot");
    }

    #[test]
    fn osc_7_emits_cwd_changed_with_path_extracted() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b]7;file://hostname/C:/code/wind\x07");
        let events = t.take_pending_events();
        match &events[..] {
            [KernelEvent::CwdChanged(p)] => assert_eq!(p, "/C:/code/wind"),
            other => panic!("unexpected events: {other:?}"),
        }
    }

    #[test]
    fn osc_8_hyperlink_annotates_intermediate_cells() {
        // OSC 8 open → write "hello" → OSC 8 close. Row 0 should have
        // exactly one HyperlinkSpan covering cols 0..5.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07hello\x1b]8;;\x07");
        let row = t.grid().row(0).unwrap();
        assert_eq!(
            row.hyperlinks.len(),
            1,
            "should coalesce 5 prints into 1 span"
        );
        let span = &row.hyperlinks[0];
        assert_eq!(span.col_start, 0);
        assert_eq!(span.col_end, 5);
        assert_eq!(span.uri, "https://example.com");
        assert_eq!(span.id, None);
        // link_at finds the span at any col in the range.
        assert!(row.link_at(0).is_some());
        assert!(row.link_at(4).is_some());
        assert!(row.link_at(5).is_none(), "exclusive end");
    }

    #[test]
    fn osc_8_open_then_close_pair_does_not_emit_events() {
        // OSC 8 open/close used to push KernelEvent variants but those
        // were removed in TASKS §3.2 — the load-bearing state is the
        // per-cell HyperlinkSpan annotation, which is verified by
        // `osc_8_marks_cells_with_link_span` above. Open/close on its
        // own with no printable cells should produce zero events.
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b]8;id=abc;https://example.com\x07");
        t.feed(b"\x1b]8;;\x07");
        assert!(t.take_pending_events().is_empty());
    }

    // ─── OSC 8 hyperlink lifecycle vs partial-erase paths ──────────────
    // TASKS §1.18.b: pre-fix, erase_in_line / erase_in_display / ECH
    // wiped cells but left HyperlinkSpan untouched, so the renderer's
    // hyperlink-underline pass painted underlines on now-blank cells.
    // Claude Code emits these escapes heavily for status-line redraws.
    #[test]
    fn csi_2k_erases_hyperlink_spans_on_line() {
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07hello\x1b]8;;\x07");
        // CSI 2 K: erase entire line. Cursor row is 0 (still on row 0
        // since "hello" was 5 chars in a 20-col grid).
        t.feed(b"\x1b[2K");
        let row = t.grid().row(0).unwrap();
        assert!(
            row.hyperlinks.is_empty(),
            "CSI 2K must clear hyperlink spans"
        );
    }

    #[test]
    fn ech_clips_hyperlink_span_tail_when_erase_overlaps_end() {
        // Hyperlink covers cols 0..5 ("hello"). Move cursor to col 3
        // (CSI 4 G is 1-based → col index 3) and ECH 5 — wipes cols 3..8,
        // which is past the row width but clamped. Span tail clipped to col 3.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07hello\x1b]8;;\x07");
        t.feed(b"\x1b[4G\x1b[5X");
        let row = t.grid().row(0).unwrap();
        assert_eq!(row.hyperlinks.len(), 1);
        let span = &row.hyperlinks[0];
        assert_eq!(span.col_start, 0);
        assert_eq!(span.col_end, 3, "tail clipped to ECH start");
    }

    #[test]
    fn ech_drops_hyperlink_span_when_erase_engulfs_it() {
        // Span at cols 0..5; ECH 10 from col 0 wipes cols 0..10. Span
        // entirely inside erase window — drop.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07hello\x1b]8;;\x07");
        t.feed(b"\x1b[1G\x1b[10X");
        let row = t.grid().row(0).unwrap();
        assert!(row.hyperlinks.is_empty());
    }

    #[test]
    fn ech_drops_hyperlink_span_when_erase_punches_middle_hole() {
        // Span at cols 0..10 ("helloworld"); ECH from col 4 with N=2
        // wipes cols 4..6 — middle hole. We can't split into two spans
        // mid-`retain`, so we drop the whole span (matches xterm UX).
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07helloworld\x1b]8;;\x07");
        t.feed(b"\x1b[5G\x1b[2X");
        let row = t.grid().row(0).unwrap();
        assert!(row.hyperlinks.is_empty(), "middle-hole drops the span");
    }

    #[test]
    fn ech_clips_hyperlink_span_head_when_erase_overlaps_start() {
        // Span at cols 5..10 (move cursor to col 6 first, write "world");
        // ECH 3 from col 4 wipes cols 4..7. Span head clipped forward to col 7.
        let mut t = Terminal::new(2, 20, 0);
        // 5 leading blanks then "world" with hyperlink:
        t.feed(b"     \x1b]8;;https://example.com\x07world\x1b]8;;\x07");
        // Move cursor to col 4 (CSI 5 G is 1-based) and ECH 3 → wipes 4..7.
        t.feed(b"\x1b[5G\x1b[3X");
        let row = t.grid().row(0).unwrap();
        assert_eq!(row.hyperlinks.len(), 1);
        let span = &row.hyperlinks[0];
        assert_eq!(span.col_start, 7, "head clipped forward past ECH end");
        assert_eq!(span.col_end, 10);
    }

    #[test]
    fn ich_invalidates_hyperlinks_at_or_after_cursor() {
        // Span at cols 0..5 ("hello"). Cursor moved to col 2, ICH 3 — shifts
        // "llo" right and inserts 3 blanks. Span now visually wrong, drop.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07hello\x1b]8;;\x07");
        t.feed(b"\x1b[3G\x1b[3@");
        let row = t.grid().row(0).unwrap();
        assert!(
            row.hyperlinks.is_empty(),
            "ICH at edit point invalidates overlapping spans"
        );
    }

    #[test]
    fn ich_keeps_hyperlinks_strictly_before_cursor() {
        // Span at cols 0..3 ("AAA"). Cursor at col 10, ICH 2 — far away.
        // Span is entirely before cursor → keep.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://a\x07AAA\x1b]8;;\x07");
        t.feed(b"       "); // pad cols 3..10
        t.feed(b"\x1b[11G\x1b[2@");
        let row = t.grid().row(0).unwrap();
        assert_eq!(row.hyperlinks.len(), 1);
        assert_eq!(row.hyperlinks[0].col_start, 0);
        assert_eq!(row.hyperlinks[0].col_end, 3);
    }

    #[test]
    fn dch_invalidates_hyperlinks_at_or_after_cursor() {
        // Span at cols 0..5 ("hello"). Cursor at col 2, DCH 2 — shifts left.
        // Span overlaps edit point, drop.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://example.com\x07hello\x1b]8;;\x07");
        t.feed(b"\x1b[3G\x1b[2P");
        let row = t.grid().row(0).unwrap();
        assert!(
            row.hyperlinks.is_empty(),
            "DCH at edit point invalidates overlapping spans"
        );
    }

    #[test]
    fn ech_keeps_hyperlink_outside_erase_range() {
        // Two spans on one row: 0..3 and 10..15. ECH at col 5, N=4
        // wipes cols 5..9 — between the two spans. Both kept intact.
        let mut t = Terminal::new(2, 20, 0);
        t.feed(b"\x1b]8;;https://a\x07AAA\x1b]8;;\x07");
        t.feed(b"       "); // pad cols 3..10 (7 blanks)
        t.feed(b"\x1b]8;;https://b\x07BBBBB\x1b]8;;\x07");
        // Move to col 6 and erase 4 chars (cols 5..9, 0-indexed).
        t.feed(b"\x1b[6G\x1b[4X");
        let row = t.grid().row(0).unwrap();
        assert_eq!(row.hyperlinks.len(), 2, "both spans survive between-erase");
        assert_eq!(row.hyperlinks[0].col_start, 0);
        assert_eq!(row.hyperlinks[0].col_end, 3);
        assert_eq!(row.hyperlinks[1].col_start, 10);
        assert_eq!(row.hyperlinks[1].col_end, 15);
    }

    #[test]
    fn bel_emits_event_outside_osc() {
        // 10 cols so "hithere" fits without soft-wrap noise.
        let mut t = Terminal::new(2, 10, 0);
        t.feed(b"hi\x07there");
        let events = t.take_pending_events();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], KernelEvent::Bell));
        // Surrounding text not lost — BEL must execute, not consume bytes.
        assert_eq!(t.dump_visible_text()[0], "hithere");
    }

    #[test]
    fn ech_erases_n_cells_without_moving_cursor() {
        // CSI <n> X erases N cells starting at cursor; cursor stays put.
        // This is what Ink/PSReadLine use to wipe stale text in-place
        // without a full row redraw — the bug that caused "character
        // residue" in claude code's frame updates.
        let mut t = Terminal::new(2, 10, 0);
        t.feed(b"abcdefghij");
        // Cursor is now at end of row 0 (col 10, pending_wrap=true).
        // Move to col 2 and erase 3 chars.
        t.feed(b"\x1b[1;3H");
        t.feed(b"\x1b[3X");
        let line = &t.dump_visible_text()[0];
        assert_eq!(line, "ab   fghij", "ECH must erase 3 cells in place");
        // Cursor must still be at col 2 (1-based: 3).
        assert_eq!(t.grid().cursor().col, 2);
    }

    #[test]
    fn ich_inserts_n_blank_cells_shifting_right() {
        let mut t = Terminal::new(1, 10, 0);
        t.feed(b"abcdef");
        t.feed(b"\x1b[1;2H"); // cursor to col 2 (1-based: 2 → 0-based: 1)
        t.feed(b"\x1b[2@"); // insert 2 blanks at cursor
        let line = &t.dump_visible_text()[0];
        assert_eq!(
            line, "a  bcdef",
            "ICH inserts blanks, shifts rest right (last 'f' falls off)"
        );
        assert_eq!(t.grid().cursor().col, 1, "cursor must not move");
    }

    #[test]
    fn dch_deletes_n_cells_shifting_left() {
        let mut t = Terminal::new(1, 10, 0);
        t.feed(b"abcdefghij");
        t.feed(b"\x1b[1;3H"); // cursor to col 3 (0-based 2)
        t.feed(b"\x1b[3P"); // delete 3 cells at cursor
        let line = &t.dump_visible_text()[0];
        assert_eq!(
            line, "abfghij",
            "DCH deletes 'cde', shifts rest left, blanks fill right"
        );
        assert_eq!(t.grid().cursor().col, 2);
    }

    #[test]
    fn sco_save_restore_aliases_decsc_decrc() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b[3;3H"); // cursor to (2, 2)
        t.feed(b"\x1b[s"); // SCO save
        t.feed(b"\x1b[5;5H"); // cursor elsewhere
        t.feed(b"\x1b[u"); // SCO restore
        assert_eq!(t.grid().cursor().row, 2);
        assert_eq!(t.grid().cursor().col, 2);
    }

    #[test]
    fn csi_18t_replies_text_area_size() {
        let mut t = Terminal::new(24, 80, 0);
        t.feed(b"\x1b[18t");
        let resp = t.take_pending_response();
        assert_eq!(resp, b"\x1b[8;24;80t");
    }

    #[test]
    fn dec_1049_save_restore_round_trip() {
        // Repro of the user's "prompt above program output" bug:
        // 1. Place cursor at row 5 (where shell prompt would be).
        // 2. Enter alt screen via ?1049h — must save primary cursor.
        // 3. Pretend a TUI moves cursor around inside alt.
        // 4. Leave alt via ?1049l — primary cursor must come back to (5,_).
        let mut t = Terminal::new(20, 20, 50);
        t.feed(b"\x1b[6;1H"); // cursor to (row 5, col 0) on primary
        assert_eq!(t.grid().cursor().row, 5);
        t.feed(b"\x1b[?1049h"); // enter alt + save primary cursor
        assert!(t.is_alt_screen());
        // Cursor on alt starts at (0,0), per xterm semantics.
        assert_eq!(t.grid().cursor().row, 0);
        assert_eq!(t.grid().cursor().col, 0);
        // TUI runs around in alt screen.
        t.feed(b"\x1b[18;10Habc");
        assert_eq!(t.grid().cursor().row, 17);
        // TUI exits — we should land back on the saved primary cursor row.
        t.feed(b"\x1b[?1049l");
        assert!(!t.is_alt_screen());
        assert_eq!(
            t.grid().cursor().row,
            5,
            "?1049l must restore primary cursor row (saved by ?1049h)"
        );
        assert_eq!(t.grid().cursor().col, 0);
    }

    #[test]
    fn il_inserts_blank_line() {
        let mut t = Terminal::new(4, 3, 0);
        t.feed(b"a\r\nb\r\nc\r\nd");
        // Move to row 2 (1-based) = row 1 (0-based), col 0.
        t.feed(b"\x1b[2;1H");
        t.feed(b"\x1b[L");
        let lines = t.dump_visible_text();
        assert_eq!(lines[0], "a");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "b");
    }

    // ----- prepend_scrollback ------------------------------------------------

    /// Helper: read scrollback rows oldest→newest as trimmed strings.
    fn dump_scrollback_text(t: &Terminal) -> Vec<String> {
        let sb = &t.grid().scrollback;
        let mut out = Vec::with_capacity(sb.len());
        for i in 0..sb.len() {
            let row = sb.get(i).unwrap();
            let mut s = String::new();
            for c in &row.cells {
                if c.width == 0 {
                    continue;
                }
                s.push(c.ch);
            }
            out.push(s.trim_end_matches(' ').to_string());
        }
        out
    }

    #[test]
    fn prepend_scrollback_plain_text_lands_at_oldest_end() {
        let mut t = Terminal::new(2, 10, 100);
        t.feed(b"recent1\r\nrecent2\r\nrecent3");
        // Scrollback now: ["recent1"]; grid: ["recent2", "recent3"].
        assert_eq!(dump_scrollback_text(&t), vec!["recent1".to_string()]);

        t.prepend_scrollback(b"older1\r\nolder2");
        // Order should be [older1, older2, recent1].
        assert_eq!(
            dump_scrollback_text(&t),
            vec![
                "older1".to_string(),
                "older2".to_string(),
                "recent1".to_string()
            ]
        );
        // Live grid untouched.
        let lines = t.dump_visible_text();
        assert_eq!(lines[0], "recent2");
        assert_eq!(lines[1], "recent3");
    }

    #[test]
    fn prepend_scrollback_preserves_sgr_colors_via_attr_remap() {
        let mut t = Terminal::new(2, 10, 100);
        t.prepend_scrollback(b"\x1b[31mred\x1b[0m\r\nplain");
        assert_eq!(
            dump_scrollback_text(&t),
            vec!["red".to_string(), "plain".to_string()]
        );
        let row0 = t.grid().scrollback.get(0).unwrap();
        let red_attr = t.grid().attrs.get(row0.cells[0].attr);
        assert_eq!(red_attr.fg, Color::indexed(1)); // ANSI 1 = red
        let row1 = t.grid().scrollback.get(1).unwrap();
        assert_eq!(row1.cells[0].attr, crate::term::attr_table::AttrId::DEFAULT);
    }

    #[test]
    fn prepend_scrollback_does_not_emit_pending_events() {
        let mut t = Terminal::new(2, 10, 100);
        t.prepend_scrollback(b"\x1b]0;old-title\x07hi\x07\r\n\x1b]7;file:///old\x07line2");
        assert!(t.take_pending_events().is_empty());
        assert!(t.take_pending_response().is_empty());
    }

    #[test]
    fn prepend_scrollback_does_not_disturb_live_state() {
        let mut t = Terminal::new(2, 10, 100);
        t.feed(b"\x1b[1;31mLIVE");
        let cursor_before = (t.grid().cursor().row, t.grid().cursor().col);
        let attrs_before = t.current_attrs;
        let alt_before = t.is_alt_screen();

        // History bytes that toggle alt screen and switch SGR.
        t.prepend_scrollback(b"\x1b[?1049h\x1b[32mhistory\x1b[0m");

        assert_eq!(t.grid().cursor().row, cursor_before.0);
        assert_eq!(t.grid().cursor().col, cursor_before.1);
        assert_eq!(t.current_attrs, attrs_before);
        assert!(t.current_attrs.flags.contains(Flags::BOLD));
        assert_eq!(t.current_attrs.fg, Color::indexed(1));
        assert_eq!(t.is_alt_screen(), alt_before);
    }

    #[test]
    fn prepend_scrollback_empty_bytes_is_noop() {
        let mut t = Terminal::new(2, 10, 100);
        t.feed(b"x\r\ny");
        let sb_before = dump_scrollback_text(&t);
        t.prepend_scrollback(b"");
        let sb_after = dump_scrollback_text(&t);
        assert_eq!(sb_before, sb_after);
    }

    #[test]
    fn prepend_scrollback_evicts_newest_when_capacity_exhausted() {
        let mut t = Terminal::new(1, 5, 2);
        t.feed(b"r1\r\nr2\r\nr3");
        // After: scrollback = [r1, r2], grid = ["r3"].
        assert_eq!(dump_scrollback_text(&t), vec!["r1", "r2"]);

        t.prepend_scrollback(b"o1\r\no2");
        // push_front evicts newest each overflow. After prepending o2 then
        // o1, scrollback should be [o1, o2]; r1 and r2 fall off.
        assert_eq!(dump_scrollback_text(&t), vec!["o1", "o2"]);
    }
}
