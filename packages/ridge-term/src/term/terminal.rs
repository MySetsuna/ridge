//! Top-level Terminal: stitches the byte-level parser to the grid + modes.

use vte::Parser;

use super::attrs::Attrs;
use super::grid::Grid;
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
    /// `scrollback.len()`. Reset to 0 on any new output.
    scroll_offset: usize,
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
}

impl Terminal {
    pub fn new(rows: usize, cols: usize, scrollback_lines: usize) -> Self {
        Self {
            parser: Parser::new(),
            grid: Grid::new(rows, cols, scrollback_lines),
            current_attrs: Attrs::DEFAULT,
            modes: Modes::default(),
            scroll_offset: 0,
            pending_response: Vec::new(),
            pending_events: Vec::new(),
            last_printed: None,
            current_link: None,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        // Any new output snaps the viewport back to the live grid — same
        // behavior as xterm and most terminals (so log streams don't
        // disappear behind the user while they're paging).
        if !bytes.is_empty() && self.scroll_offset != 0 {
            self.scroll_offset = 0;
        }
        let mut perf = Performer {
            grid: &mut self.grid,
            current_attrs: &mut self.current_attrs,
            modes: &mut self.modes,
            pending_response: &mut self.pending_response,
            pending_events: &mut self.pending_events,
            last_printed: &mut self.last_printed,
            current_link: &mut self.current_link,
        };
        for &b in bytes {
            self.parser.advance(&mut perf, b);
        }
    }

    /// Drain bytes the parser produced as query responses (DSR/DA) since
    /// the last call. Caller must forward these to the PTY as if user
    /// input. Without forwarding, PowerShell + ConPTY can't track cursor
    /// position across child-process boundaries.
    pub fn take_pending_response(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending_response)
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
    /// scrollback length.
    pub fn scroll_up_view(&mut self, n: usize) {
        let max = self.grid.scrollback.len();
        self.scroll_offset = (self.scroll_offset + n).min(max);
    }

    /// Scroll viewport down (toward live grid). Clamped at 0.
    pub fn scroll_down_view(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Snap viewport back to live grid (= 0 offset).
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn scroll_offset(&self) -> usize { self.scroll_offset }
    pub fn scrollback_len(&self) -> usize { self.grid.scrollback.len() }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.grid.resize(rows, cols);
    }

    pub fn rows(&self) -> usize { self.grid.rows() }
    pub fn cols(&self) -> usize { self.grid.cols() }
    pub fn grid(&self) -> &Grid { &self.grid }
    pub fn modes(&self) -> &Modes { &self.modes }
    pub fn is_alt_screen(&self) -> bool { self.grid.is_alt_screen() }

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
                if cell.width == 0 { continue; }
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
        assert_eq!(row.hyperlinks.len(), 1, "should coalesce 5 prints into 1 span");
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
        t.feed(b"\x1b[1;2H");  // cursor to col 2 (1-based: 2 → 0-based: 1)
        t.feed(b"\x1b[2@");    // insert 2 blanks at cursor
        let line = &t.dump_visible_text()[0];
        assert_eq!(line, "a  bcdef", "ICH inserts blanks, shifts rest right (last 'f' falls off)");
        assert_eq!(t.grid().cursor().col, 1, "cursor must not move");
    }

    #[test]
    fn dch_deletes_n_cells_shifting_left() {
        let mut t = Terminal::new(1, 10, 0);
        t.feed(b"abcdefghij");
        t.feed(b"\x1b[1;3H");  // cursor to col 3 (0-based 2)
        t.feed(b"\x1b[3P");    // delete 3 cells at cursor
        let line = &t.dump_visible_text()[0];
        assert_eq!(line, "abfghij", "DCH deletes 'cde', shifts rest left, blanks fill right");
        assert_eq!(t.grid().cursor().col, 2);
    }

    #[test]
    fn sco_save_restore_aliases_decsc_decrc() {
        let mut t = Terminal::new(5, 5, 0);
        t.feed(b"\x1b[3;3H");  // cursor to (2, 2)
        t.feed(b"\x1b[s");     // SCO save
        t.feed(b"\x1b[5;5H");  // cursor elsewhere
        t.feed(b"\x1b[u");     // SCO restore
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
        assert_eq!(t.grid().cursor().row, 5,
            "?1049l must restore primary cursor row (saved by ?1049h)");
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
                if c.width == 0 { continue; }
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
            vec!["older1".to_string(), "older2".to_string(), "recent1".to_string()]
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
        t.prepend_scrollback(
            b"\x1b]0;old-title\x07hi\x07\r\n\x1b]7;file:///old\x07line2",
        );
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
