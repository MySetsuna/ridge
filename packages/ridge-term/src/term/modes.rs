//! Terminal modes — DEC private (`CSI ? n h/l`) + ANSI public (`CSI n h/l`).
//!
//! Modes are *state* the parser flips on/off. The renderer reads them
//! (e.g. `cursor_visible`, `cursor_blink`); the input encoder reads them
//! (e.g. `bracketed_paste`, `mouse_*` to wrap user events appropriately).
//!
//! This module is intentionally a struct-of-bools. There are ~20 distinct
//! modes terminals support; we only model the ones xterm-compatible apps
//! actually depend on. Adding a new mode is one bool + one match arm.

/// Cursor shape requested by the application via DECSCUSR (`CSI <n> SP q`).
/// Mirrors the variants the render backend already supports — kept in this
/// module so the parser can flip it without depending on the render layer.
/// vim sets Bar in insert mode and Block in normal mode; many readline
/// configs do the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Copy)]
pub struct Modes {
    /// DECAWM (mode ?7) — autowrap on right margin. xterm default = on.
    pub autowrap: bool,
    /// DECTCEM (mode ?25) — cursor visibility. xterm default = on.
    pub cursor_visible: bool,
    /// AT&T 610 (mode ?12) — cursor blink. Renderer-only.
    pub cursor_blink: bool,
    /// DECOM (mode ?6) — origin mode. When on, cursor addressing is
    /// relative to the scroll region. Used by some ncurses apps.
    pub origin: bool,
    /// IRM (mode 4, *not* ?4) — insert mode. When on, printing shifts
    /// existing cells right instead of overwriting. Off in modern shells.
    pub insert: bool,
    /// LNM (mode 20) — line feed mode. When on, LF also performs CR.
    /// Off by default; shells emit explicit CRLF.
    pub linefeed_newline: bool,

    // Mouse reporting. Off by default; many TUIs request these.
    pub mouse_x10: bool,          // ?9
    pub mouse_normal: bool,       // ?1000
    pub mouse_button_event: bool, // ?1002 (drag)
    pub mouse_any_event: bool,    // ?1003 (motion)
    pub mouse_sgr: bool,          // ?1006 (extended SGR encoding)
    pub mouse_focus: bool,        // ?1004 (focus in/out events)

    /// Bracketed paste (?2004). When on, the input encoder must wrap
    /// pasted text as `\x1b[200~ <text> \x1b[201~` so shells distinguish
    /// paste from typed input.
    pub bracketed_paste: bool,

    /// Application cursor keys (?1). When on, arrow keys send `\x1bO[A-D]`
    /// instead of `\x1b[[A-D]`. Required for vim, less, fzf to work.
    pub app_cursor_keys: bool,
    /// Application keypad (DECPAM, ESC =). Numpad sends `\x1bO<x>` instead
    /// of digits. Less-commonly tested but worth modeling.
    pub app_keypad: bool,

    /// Synchronous output mode (`CSI ? 2026 h/l`) — Contour/iTerm2/Kitty
    /// extension. While `true`, the kernel still parses and mutates the
    /// grid, but the renderer should HOLD frames so the user never sees
    /// a torn intermediate state during multi-step redraws (Ink, lazygit,
    /// bottom). The renderer's timeout enforcement (default 150ms) lives
    /// on the JS side so the wasm kernel doesn't have to depend on a
    /// monotonic clock.
    pub sync_output: bool,

    /// §B.6 (2026-05-08) — Unicode Core mode (`CSI ? 2027 h/l`). When
    /// `true`, the application has been told the terminal handles
    /// extended grapheme clusters with the cluster's *visual* width,
    /// not the sum of its codepoints' wcwidths. PSReadLine 2.3.6+,
    /// oh-my-posh, starship, and modern shell prompts query this mode
    /// (DECRQM `CSI ? 2027 $p`) and switch their internal column
    /// accounting to match — fixing the canonical "non-BMP emoji
    /// renders at column 4 because .NET counts surrogate pair as 2
    /// chars × 2 cells each" symptom on Windows.
    ///
    /// Wind ALWAYS uses grapheme cluster widths internally (see
    /// `wcwidth_grapheme` + `print_grapheme`); this mode is just a
    /// signal to the application that the terminal does so. Setting
    /// it to `true` is harmless on apps that ignore it.
    ///
    /// Reference: contour-terminal/terminal-unicode-core spec.
    pub unicode_core_2027: bool,

    /// Cursor shape set via DECSCUSR `CSI <n> SP q`. Renderer reads this
    /// to decide between block/underline/bar in `compute_cursor_draw`.
    /// Note: the same DECSCUSR sub-code also encodes blink — we mirror
    /// that into `cursor_blink` (vim insert-mode bar typically blinks).
    pub cursor_shape: CursorShape,
}

impl Default for Modes {
    fn default() -> Self {
        Self {
            autowrap: true,
            cursor_visible: true,
            cursor_blink: true,
            origin: false,
            insert: false,
            linefeed_newline: false,
            mouse_x10: false,
            mouse_normal: false,
            mouse_button_event: false,
            mouse_any_event: false,
            mouse_sgr: false,
            mouse_focus: false,
            bracketed_paste: false,
            app_cursor_keys: false,
            app_keypad: false,
            sync_output: false,
            // §B.6 — default ON. Wind's grapheme-cluster width
            // semantics match Mode 2027's spec at construction time;
            // advertising it eagerly means PSReadLine queries this
            // mode at startup (oh-my-posh / starship inject the query
            // into PROMPT_COMMAND) and gets the right answer without
            // the user manually enabling it. Apps that DON'T speak
            // 2027 are unaffected.
            unicode_core_2027: true,
            cursor_shape: CursorShape::Block,
        }
    }
}

impl Modes {
    /// P3.12 (2026-05-20) — diff against `prev` and return a list of
    /// `(mode_code, on)` pairs covering every field that changed. The
    /// codes are the DEC-private numbers an application would write
    /// to flip the mode (e.g. `?1049` is reported as `1049`, mode 4
    /// IRM as `4`). Cursor visibility / blink / shape are NOT in the
    /// output — those flow through `GridDelta::Cursor` and would
    /// double up here.
    ///
    /// Synthetic code `1066` is used for `app_keypad` (xterm's
    /// "alternate keypad" — DECPAM / DECPNM are ESC = / ESC > escape
    /// sequences, no DEC private number, so we pick 1066 by analogy
    /// with the way some xterm forks identify the bit).
    pub fn diff(&self, prev: &Self) -> Vec<(u32, bool)> {
        let mut out: Vec<(u32, bool)> = Vec::new();
        if self.autowrap != prev.autowrap { out.push((7, self.autowrap)); }
        if self.origin != prev.origin { out.push((6, self.origin)); }
        if self.insert != prev.insert { out.push((4, self.insert)); }
        if self.linefeed_newline != prev.linefeed_newline { out.push((20, self.linefeed_newline)); }
        if self.mouse_x10 != prev.mouse_x10 { out.push((9, self.mouse_x10)); }
        if self.mouse_normal != prev.mouse_normal { out.push((1000, self.mouse_normal)); }
        if self.mouse_button_event != prev.mouse_button_event { out.push((1002, self.mouse_button_event)); }
        if self.mouse_any_event != prev.mouse_any_event { out.push((1003, self.mouse_any_event)); }
        if self.mouse_sgr != prev.mouse_sgr { out.push((1006, self.mouse_sgr)); }
        if self.mouse_focus != prev.mouse_focus { out.push((1004, self.mouse_focus)); }
        if self.bracketed_paste != prev.bracketed_paste { out.push((2004, self.bracketed_paste)); }
        if self.app_cursor_keys != prev.app_cursor_keys { out.push((1, self.app_cursor_keys)); }
        if self.app_keypad != prev.app_keypad { out.push((1066, self.app_keypad)); }
        if self.sync_output != prev.sync_output { out.push((2026, self.sync_output)); }
        if self.unicode_core_2027 != prev.unicode_core_2027 { out.push((2027, self.unicode_core_2027)); }
        out
    }

    /// P3.12 — apply a single mode change. Routes the numeric code
    /// (from `GridDelta::ModeChange`) to the matching `Modes` field.
    /// Unknown codes are silently ignored so a newer producer can ship
    /// modes an older mirror doesn't yet recognise without breaking
    /// the rest of the frame.
    ///
    /// Cursor visibility / blink / shape are intentionally NOT routed
    /// here — those changes flow through `GridDelta::Cursor` and
    /// applying them via two paths would risk drift.
    pub fn apply_mode_change(&mut self, code: u32, on: bool) {
        match code {
            1 => self.app_cursor_keys = on,
            4 => self.insert = on,
            6 => self.origin = on,
            7 => self.autowrap = on,
            9 => self.mouse_x10 = on,
            20 => self.linefeed_newline = on,
            1000 => self.mouse_normal = on,
            1002 => self.mouse_button_event = on,
            1003 => self.mouse_any_event = on,
            1004 => self.mouse_focus = on,
            1006 => self.mouse_sgr = on,
            1066 => self.app_keypad = on,
            2004 => self.bracketed_paste = on,
            2026 => self.sync_output = on,
            2027 => self.unicode_core_2027 = on,
            _ => {}
        }
    }
}

/// Result of `set_mode(num, value, is_private)`. The terminal facade may
/// need to react to certain mode changes (e.g. ?1049 toggles alt screen),
/// so the parser bridge returns a side-effect tag rather than executing
/// directly. Keeps the parser/grid/modes layering clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeEffect {
    None,
    /// ?47 — switch to alt screen WITHOUT clearing or cursor management.
    EnterAltScreen,
    /// ?1047 — switch to alt screen AND clear it. No cursor save.
    EnterAltScreenAndClear,
    /// ?1049 — xterm composite: DECSC (save primary cursor) + switch to alt
    /// + clear alt + move cursor to (0,0). TUIs (vim, less, fzf, claude code)
    /// rely on the save half; without it the prompt returns at a stale row
    /// when the TUI exits and overwrites whatever was on screen.
    EnterAltScreenSaveCursor,
    /// ?47 / ?1047 with `l` — switch back to primary, no cursor restore.
    LeaveAltScreen,
    /// ?1049 with `l` — switch back to primary AND DECRC (restore primary
    /// cursor saved by the matching ?1049h). Pair with EnterAltScreenSaveCursor.
    LeaveAltScreenRestoreCursor,
    /// ?6 (DECOM) set or cleared. Per xterm spec, toggling origin mode
    /// also moves the cursor to the new home position: when origin is
    /// now ON, home = (scroll_top, 0); when origin is now OFF, home =
    /// (0, 0). The dispatcher emits this for both `?6h` and `?6l`; the
    /// applier reads `modes.origin` (already updated to the post-toggle
    /// value) to pick the destination.
    JumpToOrigin,
}

impl Modes {
    /// Apply DEC private (?<n>) or ANSI public (n) mode change.
    /// Returns the side effect (if any) the caller must enact.
    pub fn set(&mut self, code: u16, value: bool, is_private: bool) -> ModeEffect {
        if is_private {
            match code {
                7 => {
                    self.autowrap = value;
                    ModeEffect::None
                }
                25 => {
                    self.cursor_visible = value;
                    ModeEffect::None
                }
                12 => {
                    self.cursor_blink = value;
                    ModeEffect::None
                }
                6 => {
                    self.origin = value;
                    ModeEffect::JumpToOrigin
                }
                1 => {
                    self.app_cursor_keys = value;
                    ModeEffect::None
                }

                9 => {
                    self.mouse_x10 = value;
                    ModeEffect::None
                }
                1000 => {
                    self.mouse_normal = value;
                    ModeEffect::None
                }
                1002 => {
                    self.mouse_button_event = value;
                    ModeEffect::None
                }
                1003 => {
                    self.mouse_any_event = value;
                    ModeEffect::None
                }
                1004 => {
                    self.mouse_focus = value;
                    ModeEffect::None
                }
                1006 => {
                    self.mouse_sgr = value;
                    ModeEffect::None
                }

                2004 => {
                    self.bracketed_paste = value;
                    ModeEffect::None
                }
                2026 => {
                    self.sync_output = value;
                    ModeEffect::None
                }
                2027 => {
                    // §B.6 — explicit set/reset is honoured even
                    // though the default is ON; an app that wants to
                    // assert legacy width behaviour can still opt out.
                    self.unicode_core_2027 = value;
                    ModeEffect::None
                }

                47 => {
                    if value {
                        ModeEffect::EnterAltScreen
                    } else {
                        ModeEffect::LeaveAltScreen
                    }
                }
                1047 => {
                    if value {
                        ModeEffect::EnterAltScreenAndClear
                    } else {
                        ModeEffect::LeaveAltScreen
                    }
                }
                1049 => {
                    if value {
                        ModeEffect::EnterAltScreenSaveCursor
                    } else {
                        ModeEffect::LeaveAltScreenRestoreCursor
                    }
                }

                _ => ModeEffect::None, // unknown private mode — ignore
            }
        } else {
            match code {
                4 => {
                    self.insert = value;
                    ModeEffect::None
                }
                20 => {
                    self.linefeed_newline = value;
                    ModeEffect::None
                }
                _ => ModeEffect::None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_xterm() {
        let m = Modes::default();
        assert!(m.autowrap);
        assert!(m.cursor_visible);
        assert!(!m.bracketed_paste);
    }

    #[test]
    fn private_modes_route_alt_screen() {
        let mut m = Modes::default();
        assert_eq!(
            m.set(1049, true, true),
            ModeEffect::EnterAltScreenSaveCursor
        );
        assert_eq!(
            m.set(1049, false, true),
            ModeEffect::LeaveAltScreenRestoreCursor
        );
        assert_eq!(m.set(47, true, true), ModeEffect::EnterAltScreen);
        assert_eq!(m.set(47, false, true), ModeEffect::LeaveAltScreen);
        assert_eq!(m.set(1047, true, true), ModeEffect::EnterAltScreenAndClear);
        assert_eq!(m.set(1047, false, true), ModeEffect::LeaveAltScreen);
    }

    #[test]
    fn unknown_mode_is_noop() {
        let mut m = Modes::default();
        assert_eq!(m.set(9999, true, true), ModeEffect::None);
        assert_eq!(m.set(9999, true, false), ModeEffect::None);
    }

    #[test]
    fn unicode_core_2027_default_on() {
        // §B.6 — Mode 2027 advertises grapheme-cluster width semantics.
        // Wind ALWAYS uses cluster widths internally (`wcwidth_grapheme`
        // + `print_grapheme`); advertising it eagerly means PSReadLine /
        // oh-my-posh / starship pick up the right width path at startup
        // without manual config — fixes the "non-BMP emoji renders at
        // col 4 because .NET counts surrogate pair as 2×2 cells" symptom
        // on Windows.
        let m = Modes::default();
        assert!(m.unicode_core_2027, "Mode 2027 must default to ON");
    }

    #[test]
    fn unicode_core_2027_toggles() {
        let mut m = Modes::default();
        assert_eq!(m.set(2027, false, true), ModeEffect::None);
        assert!(!m.unicode_core_2027);
        assert_eq!(m.set(2027, true, true), ModeEffect::None);
        assert!(m.unicode_core_2027);
    }

    #[test]
    fn synchronous_output_mode_2026_toggles() {
        let mut m = Modes::default();
        assert!(!m.sync_output);
        assert_eq!(m.set(2026, true, true), ModeEffect::None);
        assert!(m.sync_output);
        assert_eq!(m.set(2026, false, true), ModeEffect::None);
        assert!(!m.sync_output);
    }

    #[test]
    fn origin_mode_6_returns_jump_to_origin_effect() {
        // DECOM (origin mode) toggle returns JumpToOrigin both ways
        // — the caller (parser.rs) translates this into a cursor
        // reposition to the scroll region's top-left.
        let mut m = Modes::default();
        assert!(!m.origin);
        assert_eq!(m.set(6, true, true), ModeEffect::JumpToOrigin);
        assert!(m.origin);
        assert_eq!(m.set(6, false, true), ModeEffect::JumpToOrigin);
        assert!(!m.origin);
    }

    #[test]
    fn cursor_visibility_mode_25_toggles() {
        // DECTCEM. Cursor renders or hides depending on this flag —
        // parser flips it via CSI ?25h / CSI ?25l.
        let mut m = Modes::default();
        assert!(m.cursor_visible);
        m.set(25, false, true);
        assert!(!m.cursor_visible);
        m.set(25, true, true);
        assert!(m.cursor_visible);
    }

    #[test]
    fn autowrap_mode_7_toggles() {
        // DECAWM. xterm default = on; we follow.
        let mut m = Modes::default();
        assert!(m.autowrap);
        m.set(7, false, true);
        assert!(!m.autowrap);
        m.set(7, true, true);
        assert!(m.autowrap);
    }

    #[test]
    fn bracketed_paste_mode_2004_toggles() {
        let mut m = Modes::default();
        assert!(!m.bracketed_paste);
        m.set(2004, true, true);
        assert!(m.bracketed_paste);
        m.set(2004, false, true);
        assert!(!m.bracketed_paste);
    }

    #[test]
    fn mouse_modes_route_to_distinct_fields() {
        // Each mouse-related private mode targets its own bool. Pin
        // the routing so future refactors can't collapse them.
        let mut m = Modes::default();
        m.set(9, true, true);
        m.set(1000, true, true);
        m.set(1002, true, true);
        m.set(1003, true, true);
        m.set(1004, true, true);
        m.set(1006, true, true);
        assert!(m.mouse_x10);
        assert!(m.mouse_normal);
        assert!(m.mouse_button_event);
        assert!(m.mouse_any_event);
        assert!(m.mouse_focus);
        assert!(m.mouse_sgr);
    }

    #[test]
    fn app_cursor_keys_mode_1_toggles() {
        // DECCKM. Affects which key sequences encode/cmd produce for
        // arrow keys.
        let mut m = Modes::default();
        assert!(!m.app_cursor_keys);
        m.set(1, true, true);
        assert!(m.app_cursor_keys);
        m.set(1, false, true);
        assert!(!m.app_cursor_keys);
    }

    #[test]
    fn public_mode_4_is_insert_distinct_from_private_4() {
        // CSI 4 h (public) sets insert mode; CSI ?4 h (private) is
        // unknown to us. Pins the is_private dispatch — a future
        // refactor that drops the is_private flag would silently
        // collapse these into the same handler.
        let mut m = Modes::default();
        assert!(!m.insert);
        m.set(4, true, false); // public
        assert!(m.insert);
        // Private 4 is unknown; should NOT touch insert.
        m.set(4, false, true);
        assert!(m.insert, "private mode 4 must not flip public-mode 4 state");
    }

    #[test]
    fn public_mode_20_linefeed_newline_toggles() {
        // LNM (line-feed/new-line). Affects whether LF triggers
        // implicit CR. Public mode, NOT private.
        let mut m = Modes::default();
        assert!(!m.linefeed_newline);
        m.set(20, true, false);
        assert!(m.linefeed_newline);
        m.set(20, false, false);
        assert!(!m.linefeed_newline);
    }
}
