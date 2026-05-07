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
            cursor_shape: CursorShape::Block,
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
