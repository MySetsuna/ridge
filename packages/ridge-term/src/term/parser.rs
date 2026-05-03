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

use vte::{Params, Perform};

use super::attrs::{Attrs, Color, Flags};
use super::grid::{EraseMode, Grid};
use super::modes::{CursorShape, ModeEffect, Modes};

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
}

impl<'a> Perform for Performer<'a> {
    fn print(&mut self, c: char) {
        let w = super::wcwidth::wcwidth(c as u32) as usize;
        // IRM (insert mode, CSI 4h). When on, printing shifts existing
        // cells from the cursor rightward by the new char's width, then
        // the print writes into the now-vacated cell. Modern shells
        // don't use IRM (they emit explicit ICH `CSI <n> @` instead) but
        // the spec requires it and `Modes::insert` was already wired by
        // the public `CSI h/l` handler — leaving the bool unread would
        // be a silent doc-vs-code drift. w=0 (combining marks) skips
        // the shift since they don't occupy a cell.
        if w > 0 && self.modes.insert {
            self.grid.insert_chars(w);
        }
        self.grid.print(c, *self.current_attrs);
        // Record for REP. Width-0 chars (combining marks etc.) are dropped
        // by grid.print so we still record them — REP is rare and harmless
        // either way; correctness over micro-tuning.
        *self.last_printed = Some((c, *self.current_attrs));
        // OSC 8 hyperlink annotation. After grid.print, the cursor sits
        // either at (row, col + w) for non-wrap writes, or at (row, cols-1)
        // with pending_wrap=true for w=1 at last column. The just-written
        // cell is therefore at col = (cur.col - w) when no pending_wrap,
        // or col = cur.col when pending_wrap. width is w (1 or 2).
        if w > 0 {
            if let Some((uri, id)) = self.current_link.as_ref() {
                let cur = *self.grid.cursor();
                let written_col = if cur.pending_wrap {
                    cur.col
                } else {
                    cur.col.saturating_sub(w)
                };
                let id_ref: Option<&str> = id.as_deref();
                self.grid.annotate_cell_with_link(cur.row, written_col, w, uri.as_str(), id_ref);
            }
        }
    }

    fn execute(&mut self, byte: u8) {
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
            4  => attrs.flags.insert(Flags::UNDERLINE),
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
