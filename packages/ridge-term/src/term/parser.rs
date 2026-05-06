//! VT/ANSI parser bridge.
//!
//! `vte::Parser` is a feed-bytes-get-callbacks state machine; we implement
//! `vte::Perform` to translate callbacks into grid + mode mutations.
//!
//! ## What's wired
//! - print(char)
//! - C0 controls: BS, HT, LF/VT/FF, CR
//! - CSI:
//!     A/B/C/D       cursor up/down/forward/back
//!     H/f           cursor position (1-based)
//!     J / K         erase display / line
//!     S / T         scroll up/down (region-aware)
//!     L / M         insert / delete lines (IL/DL)
//!     r             DECSTBM scroll region
//!     h / l         set/reset mode (DEC private with `?` intermediate)
//!     m             SGR (full SGR including 38/48 truecolor)
//! - ESC:
//!     7 / 8         DECSC / DECRC
//!     D / E / M     IND / NEL / RI
//!     = / >         DECPAM / DECPNM (application keypad)
//!
//! ## Mode side-effects
//! Setting/clearing certain DEC private modes triggers grid actions
//! (`?47/?1047/?1049` for alt screen). The parser doesn't call into the
//! grid directly for those — `Modes::set()` returns a `ModeEffect` tag,
//! and the parser executes the corresponding grid op. This keeps mode
//! state and screen state cleanly separated.

use unicode_segmentation::UnicodeSegmentation;
use vte::{Params, Perform};

use super::attrs::{Attrs, Color, Flags};
use super::clock;
use super::grid::{EraseMode, Grid};
use super::modes::{CursorShape, ModeEffect, Modes};
use super::wcwidth::{could_extend_grapheme, wcwidth, wcwidth_grapheme};

/// §4.7 — safety cap on how big the grapheme buffer can grow before
/// the parser force-flushes it. A well-formed extended grapheme cluster
/// is typically 1–7 codepoints (the longest common cluster is family
/// emoji like 👨‍👩‍👧‍👦 = 7 codepoints). 32 chars gives ~4× headroom for
/// pathological inputs without unbounded growth on garbage bytes.
const MAX_GRAPHEME_BUF_CHARS: usize = 32;

pub struct Performer<'a> {
    pub grid: &'a mut Grid,
    pub current_attrs: &'a mut Attrs,
    pub modes: &'a mut Modes,
    /// Outbound buffer the parser writes responses into (DSR, DA). Owner
    /// drains this after the feed call and forwards to the PTY.
    pub pending_response: &'a mut Vec<u8>,
    /// Structured semantic events (title, cwd, hyperlinks, bell) the
    /// parser surfaced from OSC + control bytes. Owner drains these
    /// alongside `pending_response` and routes to UI stores.
    pub pending_events: &'a mut Vec<super::terminal::KernelEvent>,
    /// Last printed (char, attrs). Populated by `print`; consumed by REP
    /// `CSI <n> b`. Lives on the Terminal so it survives across feeds.
    pub last_printed: &'a mut Option<(char, Attrs)>,
    /// Currently-open OSC 8 hyperlink. While Some, every printed cell is
    /// annotated with this (uri, id) on its row via `Grid::annotate_cell_with_link`.
    /// Persists across feed batches.
    pub current_link: &'a mut Option<(String, Option<String>)>,
    /// §4.7 — grapheme cluster buffer (owned by Terminal so it persists
    /// across feed batches). Accumulates codepoints from `print` until a
    /// cluster boundary is reached, then emits each completed cluster as
    /// a single unit via `emit_grapheme`.
    pub grapheme_buf: &'a mut String,
}

impl<'a> Performer<'a> {
    /// §4.7 — drain all but the LAST grapheme from the buffer. Called
    /// after every `print(c)` so completed clusters land on the grid as
    /// soon as the next codepoint disambiguates them. The trailing
    /// grapheme stays buffered because more codepoints may yet extend
    /// it (ZWJ partners, variation selectors, combining marks).
    fn drain_complete_graphemes(&mut self) {
        let n = self.grapheme_buf.graphemes(true).count();
        if n < 2 { return; }
        // Collect to owned strings so we can call `&mut self` methods
        // while emitting (the iterator borrows `grapheme_buf`).
        let graphemes: Vec<String> = self
            .grapheme_buf
            .graphemes(true)
            .map(str::to_string)
            .collect();
        let last = graphemes.last().expect("n >= 2").clone();
        for g in &graphemes[..graphemes.len() - 1] {
            self.emit_grapheme(g);
        }
        self.grapheme_buf.clear();
        self.grapheme_buf.push_str(&last);
    }

    /// §4.7 — force-flush the entire buffer. Called BEFORE every
    /// non-print Perform event so any pending cluster lands on the
    /// grid before a CSI / OSC / control byte takes effect, and on
    /// the buffer-overflow safety path. Empty buffer is a no-op.
    fn flush_grapheme_buf(&mut self) {
        if self.grapheme_buf.is_empty() { return; }
        let buf = std::mem::take(self.grapheme_buf);
        // Buffer may legitimately hold multiple complete clusters
        // (the drain step keeps only the trailing one, but a force-
        // flush has to emit them all). Iterate and emit each.
        let graphemes: Vec<String> = buf.graphemes(true).map(str::to_string).collect();
        for g in &graphemes {
            self.emit_grapheme(g);
        }
    }

    /// §4.7 — at end of feed, flush whatever's in the buffer. We
    /// previously tried to hold an "extending" trailing codepoint
    /// across feed boundaries (so a cluster genuinely split between
    /// PTY chunks would resolve), but the heuristic over-held in the
    /// RIS case (a complete `🇺🇸` flag pair has a trailing RIS, which
    /// the heuristic flagged as extending → buffer held forever → cell
    /// stayed blank). Cross-feed cluster splits are rare in practice
    /// — PTYs deliver full UTF-8 sequences and emoji clusters tend to
    /// align with chunk boundaries — so the simpler always-flush rule
    /// loses very little correctness for a lot of robustness.
    pub(super) fn flush_buffer_if_complete(&mut self) {
        if self.grapheme_buf.is_empty() { return; }
        self.flush_grapheme_buf();
    }

    /// §4.7 — emit a single complete grapheme to the grid + handle
    /// IRM (insert mode), `last_printed` for REP, and OSC 8 hyperlink
    /// annotation. Both single-codepoint and multi-codepoint paths
    /// share this entry so the IRM / link bookkeeping stays in one
    /// place.
    fn emit_grapheme(&mut self, g: &str) {
        if g.is_empty() { return; }
        let mut chars = g.chars();
        let first = match chars.next() {
            Some(c) => c,
            None => return,
        };
        let multi = chars.next().is_some();

        // Visual width: cluster width for multi-codepoint, raw wcwidth
        // for single. ZWJ-only or combining-only clusters have width 0
        // and skip IRM shifting.
        let w = if multi {
            wcwidth_grapheme(g) as usize
        } else {
            wcwidth(first as u32) as usize
        };

        // IRM (CSI 4h). Modern shells don't emit it but the spec
        // requires it; matches the pre-§4.7 behavior.
        if w > 0 && self.modes.insert {
            self.grid.insert_chars(w);
        }

        if multi {
            self.grid.print_grapheme(g, *self.current_attrs);
        } else {
            self.grid.print(first, *self.current_attrs);
        }

        // Record for REP. We only stash the FIRST codepoint of a
        // multi-codepoint cluster — REP repeats one "char" by spec, and
        // upgrading it to repeat full clusters would require widening
        // `last_printed`'s type. Acceptable trade-off: REP after an
        // emoji ZWJ cluster repeats the base emoji, not the cluster;
        // niche enough to defer.
        *self.last_printed = Some((first, *self.current_attrs));

        // OSC 8 hyperlink annotation. After print/print_grapheme, cursor
        // advanced by `w` (or stayed at cols-1 with pending_wrap when
        // w==1 hit the right edge).
        if w > 0 {
            if let Some((uri, id)) = self.current_link.as_ref() {
                let cur = *self.grid.cursor();
                let written_col = if cur.pending_wrap {
                    cur.col
                } else {
                    cur.col.saturating_sub(w)
                };
                let id_ref: Option<&str> = id.as_deref();
                self.grid.annotate_cell_with_link(
                    cur.row, written_col, w, uri.as_str(), id_ref,
                );
            }
        }
    }
}

impl<'a> Perform for Performer<'a> {
    fn print(&mut self, c: char) {
        // §4.7 — buffer the codepoint, drain completed clusters, hold
        // the trailing one for possible extension.
        //
        // Strategy:
        //   1. Push c to the cluster buffer.
        //   2. Drain all but the LAST grapheme — UnicodeSegmentation
        //      already split the buffer into N grapheme clusters, the
        //      first N-1 of which are CLOSED by definition (the next
        //      codepoint can only extend the trailing one). Emit them.
        //   3. The trailing grapheme stays buffered. Even if the
        //      current codepoint LOOKS like a non-extender (e.g. 👨,
        //      width 2, not ZWJ/VS), the NEXT codepoint may still be a
        //      ZWJ that joins it into a multi-codepoint cluster — so
        //      we can't flush yet. The flush happens at:
        //        a. The next non-print Perform event (CSI / OSC / CR
        //           / LF / control bytes), or
        //        b. End of feed (`flush_buffer_if_complete`), if the
        //           trailing codepoint is non-extending.
        //   4. Safety cap: if the buffer balloons past
        //      `MAX_GRAPHEME_BUF_CHARS` (pathological garbage stream),
        //      force-flush so we don't grow unbounded.
        //
        // Single-codepoint output (ASCII, CJK, single emoji) emits at
        // most one codepoint of latency within a feed batch — the
        // typical PTY chunk + end-of-feed flush makes this invisible
        // to users and tests.
        self.grapheme_buf.push(c);
        self.drain_complete_graphemes();
        if self.grapheme_buf.chars().count() > MAX_GRAPHEME_BUF_CHARS {
            self.flush_grapheme_buf();
        }
        // Suppress unused-import warning for `could_extend_grapheme` —
        // it lives in the `flush_buffer_if_complete` end-of-feed path.
        let _ = could_extend_grapheme;
    }

    fn execute(&mut self, byte: u8) {
        // §4.7: flush any pending grapheme cluster so its visual unit
        // lands BEFORE this control byte takes effect (BEL / BS / HT /
        // LF / CR can move the cursor, change the active row, etc.).
        self.flush_grapheme_buf();
        match byte {
            0x07 => {
                // BEL outside of OSC string-terminator context. (vte feeds
                // OSC-terminating BEL through osc_dispatch directly, so what
                // arrives here is a "real" attention bell, not a terminator.)
                self.pending_events.push(super::terminal::KernelEvent::Bell);
            }
            0x08 => self.grid.backspace(),
            0x09 => self.grid.tab(),
            0x0a | 0x0b | 0x0c => {
                // LF/VT/FF: depending on LNM mode, may also CR.
                if self.modes.linefeed_newline {
                    self.grid.carriage_return();
                }
                self.grid.linefeed();
            }
            0x0d => self.grid.carriage_return(),
            // SO/SI (charset switch) intentionally unsupported — emitted by
            // some legacy programs; modern UTF-8 terminals can ignore safely.
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // §4.7: flush pending grapheme so cursor / attrs / mode changes
        // affect the NEXT print, not the just-buffered cluster.
        self.flush_grapheme_buf();
        let is_private = intermediates.first() == Some(&b'?');

        // Private CSI ? h / l — DEC mode set/reset.
        if is_private && (action == 'h' || action == 'l') {
            let value = action == 'h';
            for sub in params.iter() {
                if let Some(&code) = sub.first() {
                    let effect = self.modes.set(code, value, true);
                    self.apply_mode_effect(effect);
                }
            }
            return;
        }

        // Public CSI h / l — ANSI mode set/reset (insert mode, LNM, etc.)
        if !is_private && (action == 'h' || action == 'l') {
            let value = action == 'h';
            for sub in params.iter() {
                if let Some(&code) = sub.first() {
                    let _ = self.modes.set(code, value, false);
                }
            }
            return;
        }

        let p1 = || first_param(params, 1);

        match action {
            'A' => self.grid.cursor_up(p1()),
            'B' | 'e' => self.grid.cursor_down(p1()),  // 'e' = VPR (Vertical Position Relative)
            'C' | 'a' => self.grid.cursor_right(p1()), // 'a' = HPR (Horizontal Position Relative)
            'D' => self.grid.cursor_left(p1()),
            'E' => {
                // CNL: cursor next line + col 0
                self.grid.cursor_down(p1());
                self.grid.carriage_return();
            }
            'F' => {
                // CPL: cursor previous line + col 0
                self.grid.cursor_up(p1());
                self.grid.carriage_return();
            }
            'G' | '`' => {
                // CHA / HPA: cursor horizontal absolute (1-based).
                let col = p1().saturating_sub(1);
                let row = self.grid.cursor().row;
                self.grid.cursor_to(row, col);
                // §A.3: feed the inline-TUI heuristic. CHA / HPA / VPA / CUP
                // / HVP are the cursor primitives Ink-style apps use for
                // partial-diff repaints; tracking the most recent one lets
                // `Grid::resize_with_inline_tui` decide whether to fire the
                // primary full wipe.
                self.grid.note_absolute_positioning(clock::now_ms());
            }
            'd' => {
                // VPA: vertical position absolute. DECOM-aware: when
                // origin mode is on, the input row is relative to
                // scroll_top and clamped to scroll_bottom.
                let r = p1().saturating_sub(1);
                let row = if self.modes.origin {
                    (self.grid.scroll_top() + r).min(self.grid.scroll_bottom())
                } else {
                    r
                };
                let col = self.grid.cursor().col;
                self.grid.cursor_to(row, col);
                self.grid.note_absolute_positioning(clock::now_ms());
            }
            'H' | 'f' => {
                // CUP / HVP: cursor position. DECOM-aware: when origin
                // mode (?6) is on, the row argument is relative to the
                // scroll region's top and clamped to its bottom. The
                // column argument is unaffected (DECOM in this kernel
                // doesn't model horizontal margins via DECSLRM).
                let r = first_param(params, 1).saturating_sub(1);
                let col = nth_param(params, 1, 1).saturating_sub(1);
                let row = if self.modes.origin {
                    (self.grid.scroll_top() + r).min(self.grid.scroll_bottom())
                } else {
                    r
                };
                self.grid.cursor_to(row, col);
                self.grid.note_absolute_positioning(clock::now_ms());
            }
            'I' => {
                // CHT — cursor forward N tab stops. Each step uses the
                // existing 8-col default tab stop. Default n=1.
                let n = p1();
                for _ in 0..n { self.grid.tab(); }
            }
            'Z' => {
                // CBT — cursor backward N tab stops. Default n=1. Stops
                // at column 0 (does NOT wrap to previous row).
                self.grid.cursor_back_tab(p1());
            }
            'J' => self.grid.erase_in_display(parse_erase_mode(params)),
            'K' => self.grid.erase_in_line(parse_erase_mode(params)),
            'S' => self.grid.scroll_up(p1()),
            'T' => self.grid.scroll_down(p1()),
            'L' => self.grid.insert_lines(p1()),
            'M' => self.grid.delete_lines(p1()),
            'X' => self.grid.erase_chars(p1()),  // ECH — Erase Character (no cursor move)
            '@' => self.grid.insert_chars(p1()), // ICH — Insert Character (shift right)
            'P' => self.grid.delete_chars(p1()), // DCH — Delete Character (shift left)
            's' => {
                // SCO Save Cursor — xterm aliases to DECSC. Used by some
                // terminal libraries (e.g. ANSI escape libs that prefer the
                // "all-CSI" style). Same backing as ESC 7.
                let cur = *self.grid.cursor();
                *self.grid.saved_cursor_mut() = Some(super::cursor::SavedCursor {
                    row: cur.row,
                    col: cur.col,
                    attr: cur.attr,
                    origin: self.modes.origin,
                    pending_wrap: cur.pending_wrap,
                });
            }
            'u' => {
                // SCO Restore Cursor — xterm aliases to DECRC.
                if let Some(s) = *self.grid.saved_cursor_mut() {
                    self.modes.origin = s.origin;
                    let cur = self.grid.cursor_mut();
                    cur.row = s.row;
                    cur.col = s.col;
                    cur.attr = s.attr;
                    cur.pending_wrap = s.pending_wrap;
                }
            }
            'b' => {
                // REP `CSI <n> b` — repeat the most recently printed char
                // n times. Used by some apps (toilet, banner-style libs)
                // and very occasionally by drawing libraries that fill a
                // run of identical cells. n defaults to 1 if missing.
                if let Some((ch, attrs)) = *self.last_printed {
                    let n = first_param(params, 1);
                    for _ in 0..n {
                        self.grid.print(ch, attrs);
                    }
                }
            }
            'p' if intermediates.first() == Some(&b'!') => {
                // DECSTR `CSI ! p` — soft terminal reset. Spec-compliant
                // SUBSET of RIS: resets app-controllable state but
                // preserves visible screen content, scrollback, and the
                // active screen choice (alt vs primary). Used by readline,
                // less, and other apps that want a known starting state
                // without the full screen-clear that RIS does.
                //
                // Per xterm: clear DECSC, reset DECSTBM, reset SGR,
                // set IRM=off, DECOM=off, DECAWM=on, DECTCEM=on,
                // DECCKM=off, and home the cursor. Modes we don't model
                // (KAM, DECNRCM) are skipped.
                self.modes.insert = false;
                self.modes.origin = false;
                self.modes.autowrap = true;
                self.modes.cursor_visible = true;
                self.modes.app_cursor_keys = false;
                self.grid.set_scroll_region(None, None);
                *self.grid.saved_cursor_mut() = None;
                *self.current_attrs = Attrs::DEFAULT;
                self.grid.cursor_to(0, 0);
            }
            'q' if intermediates.first() == Some(&b' ') => {
                // DECSCUSR `CSI <n> SP q` — cursor shape + blink.
                //   0/1 → blinking block (default)
                //   2   → steady block
                //   3   → blinking underline
                //   4   → steady underline
                //   5   → blinking bar (vim insert mode)
                //   6   → steady bar
                // Sub-code encodes BOTH shape and blink so we set both.
                // Anything outside 0..=6 → fall back to default (blink block).
                let n = first_param(params, 0);
                let (shape, blink) = match n {
                    0 | 1 => (CursorShape::Block, true),
                    2 => (CursorShape::Block, false),
                    3 => (CursorShape::Underline, true),
                    4 => (CursorShape::Underline, false),
                    5 => (CursorShape::Bar, true),
                    6 => (CursorShape::Bar, false),
                    _ => (CursorShape::Block, true),
                };
                self.modes.cursor_shape = shape;
                self.modes.cursor_blink = blink;
            }
            't' => {
                // Window manipulation. Many sub-codes; we only respond to
                // the size-query variants because (a) most apps use these
                // and (b) the others (resize/move window) don't make sense
                // for an embedded terminal.
                //   CSI 18 t → text area in chars  → CSI 8 ; rows ; cols t
                //   CSI 19 t → root area in chars  → same response (we don't
                //              distinguish root from text area)
                //   CSI 14 t → text area in pixels → no reliable answer
                //              without renderer cell metrics; skip
                let code = first_param(params, 0);
                if code == 18 || code == 19 {
                    let resp = format!(
                        "\x1b[8;{};{}t",
                        self.grid.rows(),
                        self.grid.cols(),
                    );
                    self.pending_response.extend_from_slice(resp.as_bytes());
                }
            }
            'r' => {
                // DECSTBM: CSI top ; bottom r
                let top = first_param_opt(params);
                let bottom = nth_param_opt(params, 1);
                self.grid.set_scroll_region(top, bottom);
            }
            'm' => apply_sgr(self.current_attrs, params),
            'n' => {
                // Device Status Report.
                //   CSI 5 n   → terminal status request → reply CSI 0 n (OK)
                //   CSI 6 n   → cursor position report  → reply CSI <r>;<c> R (1-based)
                //   CSI ? 6 n → DECXCPR (extended)      → reply CSI ? <r>;<c>;0 R
                let code = first_param(params, 0);
                match (is_private, code) {
                    (false, 5) => self.pending_response.extend_from_slice(b"\x1b[0n"),
                    (false, 6) => {
                        let cur = self.grid.cursor();
                        let resp = format!("\x1b[{};{}R", cur.row + 1, cur.col + 1);
                        self.pending_response.extend_from_slice(resp.as_bytes());
                    }
                    (true, 6) => {
                        let cur = self.grid.cursor();
                        let resp = format!("\x1b[?{};{};0R", cur.row + 1, cur.col + 1);
                        self.pending_response.extend_from_slice(resp.as_bytes());
                    }
                    _ => {} // Unknown DSR sub-code — silent
                }
            }
            'c' => {
                // Device Attributes.
                //   CSI c   (or CSI 0 c)  → Primary DA   → "I'm a VT220 with these capabilities"
                //   CSI > c (or CSI > 0 c)→ Secondary DA → terminal id + version
                // We mimic xterm's responses; widely accepted by shells/apps.
                let is_secondary = intermediates.first() == Some(&b'>');
                if is_secondary {
                    // \x1b[>0;<ver>;0c — type 0 (xterm), version 0, no ROM cartridge.
                    self.pending_response.extend_from_slice(b"\x1b[>0;1;0c");
                } else {
                    // \x1b[?62;c — VT220 + minimal capability set. Sufficient for
                    // PSReadLine / readline / ncurses to consider the terminal capable.
                    self.pending_response.extend_from_slice(b"\x1b[?62;c");
                }
            }
            _ => {} // Unknown — silent
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        // §4.7: flush pending grapheme so DECSC / DECRC / IND / NEL /
        // RI etc. don't act on stale cursor state.
        self.flush_grapheme_buf();
        match byte {
            b'7' => {
                // DECSC — save full cursor state per VT spec: position,
                // attrs, DECOM origin mode, and pending-wrap flag.
                let cur = *self.grid.cursor();
                *self.grid.saved_cursor_mut() = Some(super::cursor::SavedCursor {
                    row: cur.row,
                    col: cur.col,
                    attr: cur.attr,
                    origin: self.modes.origin,
                    pending_wrap: cur.pending_wrap,
                });
            }
            b'8' => {
                // DECRC — restore everything DECSC saved, including DECOM
                // and pending-wrap. Without restoring origin, a TUI that
                // toggled origin between DECSC and DECRC would see CUP
                // addressing change semantics under its feet.
                if let Some(s) = *self.grid.saved_cursor_mut() {
                    self.modes.origin = s.origin;
                    let cur = self.grid.cursor_mut();
                    cur.row = s.row;
                    cur.col = s.col;
                    cur.attr = s.attr;
                    cur.pending_wrap = s.pending_wrap;
                }
            }
            b'D' => self.grid.linefeed(),
            b'E' => { self.grid.carriage_return(); self.grid.linefeed(); }
            b'M' => self.grid.reverse_linefeed(),
            b'=' => self.modes.app_keypad = true,  // DECPAM
            b'>' => self.modes.app_keypad = false, // DECPNM
            b'c' => {
                // RIS — full reset. xterm spec resets ALL terminal state
                // back to power-on defaults. Scope per the per-line
                // comments below. Scrollback is the one thing we preserve
                // (xterm clears, Alacritty preserves; we follow Alacritty
                // since users dislike losing history on stray RIS, and
                // TUIs that need a clean screen send ED 2 explicitly).
                *self.modes = Modes::default();        // autowrap, cursor, mouse, etc.
                *self.current_attrs = Attrs::DEFAULT;  // SGR back to default fg/bg
                *self.current_link = None;             // close any open OSC 8 span
                *self.last_printed = None;             // REP has nothing to repeat
                // Order matters: leave alt screen FIRST so the subsequent
                // saved_cursor / scroll_region / cursor_to operations target
                // the primary screen (the one users see post-reset). Idempotent
                // if already on primary.
                self.grid.leave_alt_screen();
                self.grid.set_scroll_region(None, None); // back to full-screen region
                *self.grid.saved_cursor_mut() = None;    // discard any DECSC slot
                self.grid.cursor_to(0, 0);
                self.grid.erase_in_display(EraseMode::All);
            }
            _ => {}
        }
    }

    /// OSC dispatch — title (0/1/2), cwd (7), hyperlinks (8). Surfaces
    /// each as a `KernelEvent` on `pending_events`; the JS layer routes
    /// from there to the relevant Svelte store.
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        // §4.7: flush pending grapheme so OSC 8 hyperlink open/close
        // and OSC 0/2/7/133 events apply to the NEXT print.
        self.flush_grapheme_buf();
        use super::terminal::KernelEvent;

        // OSC always opens with `<command>;<rest...>`. Need at least the
        // command sub-param to do anything meaningful.
        if params.is_empty() { return; }
        let Some(cmd_bytes) = params.first() else { return; };
        let Ok(cmd_str) = std::str::from_utf8(cmd_bytes) else { return; };

        match cmd_str {
            "0" | "2" => {
                // OSC 0 = both icon name + window title; 2 = window title only.
                // We surface as TitleChanged in both cases — most apps don't
                // distinguish and the pane title bar wants the latest one.
                if let Some(title) = osc_string_param(params, 1) {
                    self.pending_events.push(KernelEvent::TitleChanged(title));
                }
            }
            "1" => {
                // OSC 1 = icon name only. Some apps set this for terminal
                // tab labels distinct from window title; surface separately.
                if let Some(name) = osc_string_param(params, 1) {
                    self.pending_events.push(KernelEvent::IconNameChanged(name));
                }
            }
            "7" => {
                // OSC 7 — current working directory. Wire format is a
                // `file://hostname/path` URL (RFC 8089-ish). We strip the
                // scheme + hostname and pass the local path through; JS
                // layer is responsible for URL-decoding %xx escapes and
                // forward/backslash normalization.
                if let Some(uri) = osc_string_param(params, 1) {
                    let path = parse_file_uri_path(&uri);
                    self.pending_events.push(KernelEvent::CwdChanged(path));
                }
            }
            "8" => {
                // OSC 8 = hyperlink. Format: `8;<id-params>;<uri>` — id
                // parameters are `id=value:foo=bar` separated by `:`; we
                // only care about `id=`. Empty URI = close the most
                // recent open span.
                //
                // We do NOT push a KernelEvent for open/close. The
                // load-bearing state is the per-cell HyperlinkSpan
                // annotation written by `Grid::annotate_cell_with_link`
                // for cells printed while `current_link` is `Some`.
                // Every downstream consumer (renderer underline pass,
                // Ctrl+click hit-testing, Ctrl+hover affordance) reads
                // the cell state via `kernel.hyperlinkAt(row, col)` —
                // open/close events were redundant signals nobody used.
                // See TASKS §3.2.
                let id = params.get(1).and_then(|b| {
                    std::str::from_utf8(b).ok().and_then(parse_hyperlink_id)
                });
                let uri = osc_string_param(params, 2).unwrap_or_default();
                if uri.is_empty() {
                    *self.current_link = None;
                } else {
                    *self.current_link = Some((uri, id));
                }
            }
            // Unknown OSC command — silently ignore. Notable codes that
            // INTENTIONALLY land here:
            //   - OSC 133 (FinalTerm prompt protocol: 133;A start, 133;B
            //     command, 133;P cwd, 133;C/D body) — handled by the Tauri
            //     backend's `find_prompt_osc` in src-tauri/src/engine/pty.rs
            //     for two purposes: (a) gating ConPTY resize-silence release,
            //     (b) emitting `PanePromptDetected` for the frontend's diff
            //     fast-path. The wasm kernel doesn't need its own copy because
            //     these are workspace-state signals, not screen state.
            //   - OSC 633 (VS Code shell-integration extension) — same
            //     semantics as 133, also handled by the backend.
            //   - OSC 4/10/11 (xterm color queries/sets) — out of scope.
            //   - OSC 52 (clipboard manipulation) — security-sensitive, not
            //     wired.
            // If you add a kernel-level handler for any of these, also confirm
            // the backend reader doesn't double-process the same bytes.
            _ => {}
        }
    }

    /// DCS / hook — sixel etc. Out of scope.
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}

impl<'a> Performer<'a> {
    fn apply_mode_effect(&mut self, effect: ModeEffect) {
        match effect {
            ModeEffect::None => {}
            ModeEffect::EnterAltScreen => self.grid.enter_alt_screen(false),
            ModeEffect::EnterAltScreenAndClear => self.grid.enter_alt_screen(true),
            ModeEffect::EnterAltScreenSaveCursor => {
                // xterm ?1049h composite: DECSC on primary first, then switch
                // to alt + clear, then cursor home. The DECSC must run while
                // the primary screen is still active so it captures the
                // primary cursor — that's the one we'll restore on ?1049l.
                let cur = *self.grid.cursor();
                *self.grid.saved_cursor_mut() = Some(super::cursor::SavedCursor {
                    row: cur.row,
                    col: cur.col,
                    attr: cur.attr,
                    origin: self.modes.origin,
                    pending_wrap: cur.pending_wrap,
                });
                self.grid.enter_alt_screen(true);
                self.grid.cursor_to(0, 0);
            }
            ModeEffect::LeaveAltScreen => self.grid.leave_alt_screen(),
            ModeEffect::LeaveAltScreenRestoreCursor => {
                // Switch back to primary first so saved_cursor_mut accesses
                // the primary screen's saved slot (set by ?1049h above).
                // Restores full cursor state (DECRC equivalent), including
                // origin mode and pending-wrap, so TUIs that toggled DECOM
                // inside the alt screen don't leak that state to primary.
                self.grid.leave_alt_screen();
                if let Some(s) = *self.grid.saved_cursor_mut() {
                    self.modes.origin = s.origin;
                    let cur = self.grid.cursor_mut();
                    cur.row = s.row;
                    cur.col = s.col;
                    cur.attr = s.attr;
                    cur.pending_wrap = s.pending_wrap;
                }
            }
            ModeEffect::JumpToOrigin => {
                // DECOM toggle: cursor jumps to the new home position.
                // self.modes.origin is already updated to the post-toggle
                // value when we get here. ON → home is at scroll region's
                // top-left; OFF → home is at screen top-left.
                let row = if self.modes.origin {
                    self.grid.scroll_top()
                } else {
                    0
                };
                self.grid.cursor_to(row, 0);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Param helpers
// ---------------------------------------------------------------------

fn first_param(params: &Params, default: usize) -> usize {
    params.iter().next()
        .and_then(|sub| sub.first().copied())
        .map(|v| v as usize)
        .filter(|&v| v != 0)
        .unwrap_or(default)
}

fn nth_param(params: &Params, n: usize, default: usize) -> usize {
    params.iter().nth(n)
        .and_then(|sub| sub.first().copied())
        .map(|v| v as usize)
        .filter(|&v| v != 0)
        .unwrap_or(default)
}

fn first_param_opt(params: &Params) -> Option<usize> {
    params.iter().next()
        .and_then(|sub| sub.first().copied())
        .map(|v| v as usize)
        .filter(|&v| v != 0)
}

fn nth_param_opt(params: &Params, n: usize) -> Option<usize> {
    params.iter().nth(n)
        .and_then(|sub| sub.first().copied())
        .map(|v| v as usize)
        .filter(|&v| v != 0)
}

/// Decode a single OSC sub-parameter as UTF-8 text. Returns None if the
/// param is missing or not valid UTF-8 (e.g. binary data we shouldn't
/// surface to JS as a String).
fn osc_string_param(params: &[&[u8]], idx: usize) -> Option<String> {
    let bytes = params.get(idx)?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Parse a `file://hostname/path` URL down to just the path. We don't
/// fully decode percent-escapes here — JS does that, since it's Url-aware
/// and more battle-tested. Returns the original input verbatim if no
/// `file://` scheme is detected (let JS handle the weird case).
fn parse_file_uri_path(uri: &str) -> String {
    let Some(rest) = uri.strip_prefix("file://") else {
        return uri.to_string();
    };
    // Hostname runs until the next '/'. Skip it; keep everything from
    // the path-start slash. If no slash found, the whole thing was
    // `file://hostname` with no path — return empty path rather than
    // wrong data.
    match rest.find('/') {
        Some(i) => rest[i..].to_string(),
        None => String::new(),
    }
}

/// Extract `id=<value>` from an OSC 8 id-parameter list. Format is
/// colon-separated `key=value` pairs; we only look at the `id` key.
fn parse_hyperlink_id(s: &str) -> Option<String> {
    for kv in s.split(':') {
        if let Some(v) = kv.strip_prefix("id=") {
            return Some(v.to_string());
        }
    }
    None
}

fn parse_erase_mode(params: &Params) -> EraseMode {
    match first_param(params, 0).min(2) {
        0 => EraseMode::Below,
        1 => EraseMode::Above,
        _ => EraseMode::All,
    }
}

// ---------------------------------------------------------------------
// SGR
// ---------------------------------------------------------------------

fn apply_sgr(attrs: &mut Attrs, params: &Params) {
    if params.is_empty() {
        *attrs = Attrs::DEFAULT;
        return;
    }
    let subs: Vec<&[u16]> = params.iter().collect();
    let mut i = 0;
    while i < subs.len() {
        let sub = subs[i];
        let code = sub.first().copied().unwrap_or(0);
        match code {
            0  => *attrs = Attrs::DEFAULT,
            1  => attrs.flags.insert(Flags::BOLD),
            2  => attrs.flags.insert(Flags::DIM),
            3  => attrs.flags.insert(Flags::ITALIC),
            4  => {
                // Extended underline syntax (kitty / iTerm2 / wezterm):
                //   CSI 4 m       → single underline ON      (no sub)
                //   CSI 4:0 m     → underline OFF
                //   CSI 4:1 m     → single underline ON
                //   CSI 4:2 m     → double underline ON
                //   CSI 4:3 m     → curly underline ON  (degrade to single — no curly renderer yet)
                //   CSI 4:4 m     → dotted underline ON (degrade to single)
                //   CSI 4:5 m     → dashed underline ON (degrade to single)
                //
                // Without the sub-parameter check, `CSI 4:0 m` (used by
                // modern CLIs — including Claude Code — to release a
                // hyperlink underline cleanly) routed into the bare-`4`
                // arm and turned underline ON instead of OFF. The user's
                // §1.18 report ("all output gets unexpected underline")
                // was this exact bug.
                let style = sub.get(1).copied().unwrap_or(1);
                match style {
                    0 => {
                        attrs.flags.remove(Flags::UNDERLINE);
                        attrs.flags.remove(Flags::DBL_UNDERLINE);
                    }
                    2 => {
                        attrs.flags.insert(Flags::DBL_UNDERLINE);
                        attrs.flags.remove(Flags::UNDERLINE);
                    }
                    _ => {
                        // 1 / 3 / 4 / 5 / unknown — single underline.
                        // Degrades curly/dotted/dashed to single until
                        // the renderer learns those styles.
                        attrs.flags.insert(Flags::UNDERLINE);
                        attrs.flags.remove(Flags::DBL_UNDERLINE);
                    }
                }
            }
            5  => attrs.flags.insert(Flags::BLINK),
            7  => attrs.flags.insert(Flags::INVERSE),
            8  => attrs.flags.insert(Flags::HIDDEN),
            9  => attrs.flags.insert(Flags::STRIKETHROUGH),
            21 => attrs.flags.insert(Flags::DBL_UNDERLINE),
            22 => { attrs.flags.remove(Flags::BOLD); attrs.flags.remove(Flags::DIM); }
            23 => attrs.flags.remove(Flags::ITALIC),
            24 => { attrs.flags.remove(Flags::UNDERLINE); attrs.flags.remove(Flags::DBL_UNDERLINE); }
            25 => attrs.flags.remove(Flags::BLINK),
            27 => attrs.flags.remove(Flags::INVERSE),
            28 => attrs.flags.remove(Flags::HIDDEN),
            29 => attrs.flags.remove(Flags::STRIKETHROUGH),
            30..=37  => attrs.fg = Color::indexed((code - 30) as u8),
            90..=97  => attrs.fg = Color::indexed((code - 90 + 8) as u8),
            40..=47  => attrs.bg = Color::indexed((code - 40) as u8),
            100..=107 => attrs.bg = Color::indexed((code - 100 + 8) as u8),
            39 => attrs.fg = Color::DEFAULT,
            49 => attrs.bg = Color::DEFAULT,
            38 | 48 => {
                let is_fg = code == 38;
                let parsed = if sub.len() >= 2 {
                    parse_color_from_subs(&sub[1..])
                } else {
                    let kind = subs.get(i + 1).and_then(|s| s.first().copied()).unwrap_or(0);
                    match kind {
                        5 => {
                            let idx = subs.get(i + 2).and_then(|s| s.first().copied());
                            i += 2;
                            idx.map(|v| Color::indexed(v.min(255) as u8))
                        }
                        2 => {
                            let r = subs.get(i + 2).and_then(|s| s.first().copied()).unwrap_or(0) as u8;
                            let g = subs.get(i + 3).and_then(|s| s.first().copied()).unwrap_or(0) as u8;
                            let b = subs.get(i + 4).and_then(|s| s.first().copied()).unwrap_or(0) as u8;
                            i += 4;
                            Some(Color::rgb(r, g, b))
                        }
                        _ => None,
                    }
                };
                if let Some(c) = parsed {
                    if is_fg { attrs.fg = c; } else { attrs.bg = c; }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn parse_color_from_subs(rest: &[u16]) -> Option<Color> {
    match rest.first().copied()? {
        5 => rest.get(1).map(|&i| Color::indexed(i.min(255) as u8)),
        2 => {
            if rest.len() >= 5 && rest[1] == 0 {
                Some(Color::rgb(rest[2] as u8, rest[3] as u8, rest[4] as u8))
            } else if rest.len() >= 4 {
                Some(Color::rgb(rest[1] as u8, rest[2] as u8, rest[3] as u8))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── parse_color_from_subs ───────────────────────────────────────
    //
    // Decodes the trailing portion of a `CSI 38:…m` or `CSI 48:…m`
    // sub-parameter array (the leading 38/48 has already been stripped
    // by `apply_sgr`). Two recognized kinds:
    //   5 → indexed (256-color)
    //   2 → RGB truecolor
    // RGB truecolor has two valid wire layouts:
    //   `2; R; G; B`        (4-element form, common)
    //   `2; 0; R; G; B`     (5-element ITU canonical with colorspace marker)
    // The colorspace-marker form is only triggered when rest[1] == 0 AND
    // rest.len() >= 5; otherwise we fall back to the 4-element form (which
    // means `[2, 1, R, G, B]` is interpreted as a 4-element sequence
    // ignoring the trailing B — matches xterm).

    #[test]
    fn parse_color_empty_returns_none() {
        assert_eq!(parse_color_from_subs(&[]), None);
    }

    #[test]
    fn parse_color_unknown_kind_returns_none() {
        assert_eq!(parse_color_from_subs(&[3, 1, 2, 3]), None);
        assert_eq!(parse_color_from_subs(&[99, 0, 0, 0]), None);
    }

    #[test]
    fn parse_color_indexed_basic() {
        assert_eq!(parse_color_from_subs(&[5, 42]), Some(Color::indexed(42)));
        assert_eq!(parse_color_from_subs(&[5, 0]), Some(Color::indexed(0)));
        assert_eq!(parse_color_from_subs(&[5, 255]), Some(Color::indexed(255)));
    }

    #[test]
    fn parse_color_indexed_clamps_overflow() {
        // `i.min(255) as u8` — so 300 → 255.
        assert_eq!(parse_color_from_subs(&[5, 300]), Some(Color::indexed(255)));
        assert_eq!(parse_color_from_subs(&[5, 65535]), Some(Color::indexed(255)));
    }

    #[test]
    fn parse_color_indexed_missing_index_returns_none() {
        // [5] alone has no index slot.
        assert_eq!(parse_color_from_subs(&[5]), None);
    }

    #[test]
    fn parse_color_rgb_4_element_form() {
        // CSI 38; 2; R; G; B m — common form. `rest` after the leading
        // 38 strip is &[2, R, G, B] (4 elements).
        assert_eq!(
            parse_color_from_subs(&[2, 0x12, 0x34, 0x56]),
            Some(Color::rgb(0x12, 0x34, 0x56)),
        );
    }

    #[test]
    fn parse_color_rgb_5_element_with_colorspace_marker() {
        // CSI 38: 2: : R: G: B m — ITU canonical form, rest[1] = 0 marker.
        assert_eq!(
            parse_color_from_subs(&[2, 0, 0xab, 0xcd, 0xef]),
            Some(Color::rgb(0xab, 0xcd, 0xef)),
        );
    }

    #[test]
    fn parse_color_rgb_5_element_with_nonzero_second_falls_to_4_element_form() {
        // [2, 1, R, G, B] — second arg is non-zero, so it's NOT treated
        // as the ITU colorspace-marker form. Falls to 4-element branch:
        // R/G/B come from positions 1/2/3, last element ignored.
        // Matches xterm: extra trailing args are silently dropped.
        assert_eq!(
            parse_color_from_subs(&[2, 1, 100, 200, 250]),
            Some(Color::rgb(1, 100, 200)),
        );
    }

    #[test]
    fn parse_color_rgb_too_short_returns_none() {
        // [2] alone, [2, R] only, [2, R, G] only — not enough components.
        assert_eq!(parse_color_from_subs(&[2]), None);
        assert_eq!(parse_color_from_subs(&[2, 100]), None);
        assert_eq!(parse_color_from_subs(&[2, 100, 200]), None);
    }

    #[test]
    fn parse_color_rgb_components_truncate_when_over_255() {
        // u16 → u8 cast wraps via the `as u8` truncation. 256 → 0,
        // 257 → 1. This matches what xterm does on overflow input.
        assert_eq!(
            parse_color_from_subs(&[2, 256, 257, 258]),
            Some(Color::rgb(0, 1, 2)),
        );
    }
}
