//! Keyboard input encoder.
//!
//! Translates a `KeyEvent` (a thin Rust mirror of browser `KeyboardEvent`)
//! into the byte sequence a Unix terminal expects on stdin. The encoding
//! depends on the terminal's current modes (especially `app_cursor_keys`
//! and `app_keypad`).
//!
//! ## Coverage (round 2.3)
//!
//! Implemented:
//!   - Printable ASCII / Unicode (single char path)
//!   - Enter (CR by default, CRLF if LNM mode active)
//!   - Backspace (DEL 0x7f — matches xterm default; some configs want BS 0x08)
//!   - Tab / Shift+Tab
//!   - Escape
//!   - Arrow keys (with app-cursor-keys mode awareness)
//!   - Home / End / PageUp / PageDown / Insert / Delete
//!   - F1-F12 (xterm sequences)
//!   - Ctrl + ASCII letter (collapse to control byte)
//!   - Alt + char (prefix with ESC)
//!
//! Deferred to round 4:
//!   - modifyOtherKeys / CSI u protocol (Ctrl+Shift combos beyond the basic set)
//!   - Mouse encoding (CSI M / SGR mouse)
//!   - Numpad app-keypad mode (DECPAM): we handle digits, NOT all keypad ops
//!
//! ## Why the API takes a struct, not browser event
//!
//! Browser KeyboardEvent has 30+ fields; we need 5. JS layer flattens
//! event → KeyEvent before calling `encode`. This decouples the encoder
//! from web-sys (so it can be unit-tested on native target) AND lets
//! the JS side normalize quirks (composition state, IME flags) before
//! handing off.

use crate::term::modes::Modes;

/// Browser `KeyboardEvent` reduced to fields we actually care about.
#[derive(Debug, Clone)]
pub struct KeyEvent {
    /// `event.key` — the logical character or named key ("a", "Enter",
    /// "ArrowUp", "F1", "Backspace", etc.). Case-sensitive: shift+a → "A".
    pub key: String,
    /// True iff Ctrl held. (Cmd on macOS is reported as `meta`, not `ctrl`,
    /// but most terminal apps want Cmd treated like Ctrl on macOS — JS layer
    /// normalizes this before construction.)
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Held meta key (Cmd on macOS). Usually JS layer translates to `ctrl`.
    pub meta: bool,
}

/// Output of `encode`: bytes to write to PTY plus a "consumed" flag.
/// `consumed = false` means the encoder didn't recognise the key — the
/// caller should fall back to its own handling (e.g. let the browser
/// process Tab for focus navigation if app didn't claim it).
#[derive(Debug, Clone)]
pub struct EncodeResult {
    pub bytes: Vec<u8>,
    pub consumed: bool,
}

impl EncodeResult {
    fn ignored() -> Self {
        Self {
            bytes: vec![],
            consumed: false,
        }
    }
    fn bytes(b: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: b.into(),
            consumed: true,
        }
    }
}

/// Main entry point.
pub fn encode(ev: &KeyEvent, modes: &Modes) -> EncodeResult {
    let key = ev.key.as_str();

    // 1. Named keys first — they have explicit escape sequences.
    if let Some(named) = encode_named_key(key, ev, modes) {
        return named;
    }

    // 2. Single-char keys.
    let chars: Vec<char> = key.chars().collect();
    if chars.len() != 1 {
        // Multi-char `key` we don't recognize (e.g. "Dead", "Process" during IME).
        // IME handling lives at JS layer / round 4.
        return EncodeResult::ignored();
    }
    let c = chars[0];

    // 3. Ctrl + ASCII letter → control byte.
    if ev.ctrl && !ev.alt {
        if let Some(b) = ctrl_byte(c, ev.shift) {
            return EncodeResult::bytes(vec![b]);
        }
        // Ctrl + non-letter ASCII (Ctrl+Space → NUL, etc.)
        if let Some(b) = ctrl_special(c) {
            return EncodeResult::bytes(vec![b]);
        }
    }

    // 4. Alt + char → ESC + char(s).
    if ev.alt && !ev.ctrl {
        let mut buf = vec![0x1b];
        let mut tmp = [0u8; 4];
        let s = c.encode_utf8(&mut tmp);
        buf.extend_from_slice(s.as_bytes());
        return EncodeResult::bytes(buf);
    }

    // 5. Plain printable: encode the char as UTF-8.
    let mut tmp = [0u8; 4];
    let s = c.encode_utf8(&mut tmp);
    EncodeResult::bytes(s.as_bytes().to_vec())
}

/// Map Ctrl + letter (case-insensitive on letters; case-sensitive on
/// some other ASCII) to the control byte the terminal expects.
///
/// Ctrl+A → 0x01, Ctrl+B → 0x02, ..., Ctrl+Z → 0x1a.
/// Ctrl+@ → 0x00, Ctrl+[ → 0x1b (Esc), Ctrl+\ → 0x1c, Ctrl+] → 0x1d,
/// Ctrl+^ → 0x1e, Ctrl+_ → 0x1f, Ctrl+? → 0x7f (DEL).
fn ctrl_byte(c: char, _shift: bool) -> Option<u8> {
    let b = c as u32;
    match c {
        'a'..='z' => Some((b - 'a' as u32 + 1) as u8),
        'A'..='Z' => Some((b - 'A' as u32 + 1) as u8),
        _ => None,
    }
}

fn ctrl_special(c: char) -> Option<u8> {
    match c {
        ' ' | '@' => Some(0x00),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '?' => Some(0x7f),
        _ => None,
    }
}

/// Returns Some if the key name is recognized, including its full encoding.
/// Returns None if it's a single printable character (caller continues).
fn encode_named_key(key: &str, ev: &KeyEvent, modes: &Modes) -> Option<EncodeResult> {
    // Cursor keys: branch on app_cursor_keys mode. With modifiers, we use
    // the xterm "modifyCursorKeys=1" form: CSI 1 ; <mod> <letter>.
    if let Some(letter) = arrow_letter(key) {
        let modifier = encode_modifier(ev);
        if let Some(m) = modifier {
            // Modified arrows always use CSI form (not SS3), with explicit
            // params. Pattern: ESC [ 1 ; <m> <letter>
            return Some(EncodeResult::bytes(
                format!("\x1b[1;{}{}", m, letter).into_bytes(),
            ));
        }
        // Unmodified — depends on app mode.
        return Some(if modes.app_cursor_keys {
            EncodeResult::bytes(format!("\x1bO{}", letter).into_bytes())
        } else {
            EncodeResult::bytes(format!("\x1b[{}", letter).into_bytes())
        });
    }

    // Function keys F1..F4: SS3 form. F5..F12: CSI <num> ~.
    if let Some(seq) = function_key(key, ev) {
        return Some(EncodeResult::bytes(seq.into_bytes()));
    }

    Some(match key {
        "Enter" => {
            // Ctrl+Enter → LF (0x0a, ^J). 这是 Claude Code 的 Ink TextInput、
            // lazygit 提交框、以及其他区分"提交 vs. 行内换行"的 CLI 所识别
            // 的换行字节。普通 Enter 仍然发 CR（被 Ink 视为"提交"），这样
            // 用户在 Claude `claude` 等 inline TUI 中可用 Ctrl+Enter 插入
            // 新行而不触发提交。
            if ev.ctrl && !ev.alt && !ev.shift {
                EncodeResult::bytes(vec![0x0a])
            } else if modes.linefeed_newline {
                // LNM mode: CR+LF.
                EncodeResult::bytes(b"\r\n".to_vec())
            } else {
                // Default: CR only. (xterm default = CR.)
                EncodeResult::bytes(b"\r".to_vec())
            }
        }
        "Backspace" => {
            // xterm default sends DEL (0x7f). If shell wants BS (0x08),
            // it can be set via stty. Modern shells expect 0x7f.
            // Ctrl+Backspace → 0x17 (^W) for word-erase, by convention.
            if ev.ctrl && !ev.alt && !ev.shift {
                EncodeResult::bytes(vec![0x17])
            } else if ev.alt && !ev.ctrl {
                // Alt+Backspace: ESC + DEL (also word-erase in many shells).
                EncodeResult::bytes(vec![0x1b, 0x7f])
            } else {
                EncodeResult::bytes(vec![0x7f])
            }
        }
        "Tab" => {
            if ev.shift {
                // Shift+Tab → CSI Z (back tab)
                EncodeResult::bytes(b"\x1b[Z".to_vec())
            } else {
                EncodeResult::bytes(vec![0x09])
            }
        }
        "Escape" => EncodeResult::bytes(vec![0x1b]),

        // Editing keys. xterm sequences:
        //   Insert  → CSI 2 ~
        //   Delete  → CSI 3 ~
        //   Home    → SS3 H or CSI H (mode-dependent), with modifiers CSI 1;m H
        //   End     → SS3 F or CSI F  (likewise)
        //   PageUp  → CSI 5 ~
        //   PageDn  → CSI 6 ~
        "Insert" => editing_seq("2", ev),
        "Delete" => editing_seq("3", ev),
        "PageUp" => editing_seq("5", ev),
        "PageDown" => editing_seq("6", ev),
        "Home" => {
            if let Some(m) = encode_modifier(ev) {
                EncodeResult::bytes(format!("\x1b[1;{}H", m).into_bytes())
            } else if modes.app_cursor_keys {
                EncodeResult::bytes(b"\x1bOH".to_vec())
            } else {
                EncodeResult::bytes(b"\x1b[H".to_vec())
            }
        }
        "End" => {
            if let Some(m) = encode_modifier(ev) {
                EncodeResult::bytes(format!("\x1b[1;{}F", m).into_bytes())
            } else if modes.app_cursor_keys {
                EncodeResult::bytes(b"\x1bOF".to_vec())
            } else {
                EncodeResult::bytes(b"\x1b[F".to_vec())
            }
        }

        // Anything else (Fn keys named differently, IME composition keys,
        // media keys, etc.) — let the caller decide.
        _ => return None,
    })
}

/// Map "ArrowUp"/"ArrowDown"/"ArrowLeft"/"ArrowRight" to their letter (A/B/D/C).
fn arrow_letter(key: &str) -> Option<&'static str> {
    match key {
        "ArrowUp" => Some("A"),
        "ArrowDown" => Some("B"),
        "ArrowRight" => Some("C"),
        "ArrowLeft" => Some("D"),
        _ => None,
    }
}

/// xterm modifier param (1=none-omitted, 2=Shift, 3=Alt, 4=Alt+Shift,
/// 5=Ctrl, 6=Ctrl+Shift, 7=Ctrl+Alt, 8=Ctrl+Alt+Shift).
/// Returns None if no modifier is held (caller emits unmodified form).
fn encode_modifier(ev: &KeyEvent) -> Option<u32> {
    let mut m: u32 = 1;
    if ev.shift {
        m += 1;
    }
    if ev.alt {
        m += 2;
    }
    if ev.ctrl {
        m += 4;
    }
    if m == 1 {
        None
    } else {
        Some(m)
    }
}

/// Editing-block keys (Insert/Delete/PageUp/PageDown). Pattern:
/// CSI <num> ~ unmodified, CSI <num> ; <mod> ~ modified.
fn editing_seq(num: &str, ev: &KeyEvent) -> EncodeResult {
    if let Some(m) = encode_modifier(ev) {
        EncodeResult::bytes(format!("\x1b[{};{}~", num, m).into_bytes())
    } else {
        EncodeResult::bytes(format!("\x1b[{}~", num).into_bytes())
    }
}

/// F1-F12 sequences. xterm uses SS3 for F1-F4 (legacy) and CSI for F5+.
/// Modified function keys use CSI 1 ; <mod> P/Q/R/S for F1-F4, and the
/// `<num> ; <mod> ~` form for F5+.
fn function_key(key: &str, ev: &KeyEvent) -> Option<String> {
    let (n, letter) = match key {
        "F1" => (None, Some("P")),
        "F2" => (None, Some("Q")),
        "F3" => (None, Some("R")),
        "F4" => (None, Some("S")),
        "F5" => (Some(15), None),
        "F6" => (Some(17), None),
        "F7" => (Some(18), None),
        "F8" => (Some(19), None),
        "F9" => (Some(20), None),
        "F10" => (Some(21), None),
        "F11" => (Some(23), None),
        "F12" => (Some(24), None),
        _ => return None,
    };
    let modifier = encode_modifier(ev);
    Some(match (n, letter, modifier) {
        (None, Some(l), None) => format!("\x1bO{}", l),
        (None, Some(l), Some(m)) => format!("\x1b[1;{}{}", m, l),
        (Some(num), None, None) => format!("\x1b[{}~", num),
        (Some(num), None, Some(m)) => format!("\x1b[{};{}~", num, m),
        _ => unreachable!(),
    })
}

/// Wrap a paste string in bracketed-paste delimiters if the mode is on.
/// Caller obtains `bracketed_paste` from `terminal.modes()`.
pub fn wrap_paste(text: &str, bracketed_paste: bool) -> Vec<u8> {
    if bracketed_paste {
        let mut out = Vec::with_capacity(text.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.as_bytes().to_vec()
    }
}

/// Encode a mouse event as an SGR-format terminal sequence.
///
/// `btn`: 0=left, 1=middle, 2=right, 3=release, 64=scroll-up, 65=scroll-down
/// `action`: 0=press, 1=release, 2=motion (drag)
/// `row`/`col`: 0-based viewport cell coordinates
///
/// SGR format: `ESC [ < btn+mods > ; < row+1 > ; < col+1 > M/m`
///   - `M` for press/motion, `m` for release
///   - Modifier flags: +4 shift, +8 alt, +16 ctrl
///   - Motion flag: +32 (0x20) when `action == 2`
pub fn encode_mouse(btn: u8, row: usize, col: usize, action: u8, shift: bool, ctrl: bool, alt: bool, _modes: &Modes) -> Vec<u8> {
    let mut b = btn;
    if shift {
        b |= 4;
    }
    if alt {
        b |= 8;
    }
    if ctrl {
        b |= 16;
    }
    if action == 2 {
        b |= 32; // motion flag
    }
    // SGR: ESC [ < b > ; < row+1 > ; < col+1 > M (press/motion) / m (release)
    let suffix = if action == 1 { 'm' } else { 'M' };
    format!("\x1b[<{};{};{}{}", b, row + 1, col + 1, suffix).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> KeyEvent {
        KeyEvent {
            key: k.into(),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
        }
    }
    fn modes() -> Modes {
        Modes::default()
    }

    #[test]
    fn plain_letter_passthrough() {
        let r = encode(&key("a"), &modes());
        assert_eq!(r.bytes, b"a");
        assert!(r.consumed);
    }

    #[test]
    fn ctrl_letter_collapses_to_control_byte() {
        let mut k = key("c");
        k.ctrl = true;
        let r = encode(&k, &modes());
        assert_eq!(r.bytes, vec![0x03]); // ^C / SIGINT
    }

    #[test]
    fn ctrl_uppercase_letter_also_works() {
        // After Shift+Ctrl on macOS, key may be uppercase.
        let mut k = key("C");
        k.ctrl = true;
        k.shift = true;
        let r = encode(&k, &modes());
        assert_eq!(r.bytes, vec![0x03]);
    }

    #[test]
    fn alt_letter_prefixes_esc() {
        let mut k = key("b");
        k.alt = true;
        let r = encode(&k, &modes());
        assert_eq!(r.bytes, vec![0x1b, b'b']);
    }

    #[test]
    fn enter_is_cr_by_default() {
        assert_eq!(encode(&key("Enter"), &modes()).bytes, b"\r");
    }

    #[test]
    fn enter_under_lnm_is_crlf() {
        let mut m = modes();
        m.linefeed_newline = true;
        assert_eq!(encode(&key("Enter"), &m).bytes, b"\r\n");
    }

    #[test]
    fn ctrl_enter_is_lf_for_inline_tui_newline() {
        // Claude Code Ink / lazygit / readline-style inputs treat LF as
        // "newline within input", CR as "submit". Round 5 maps Ctrl+Enter
        // → LF so users can insert newlines without triggering submit.
        let mut k = key("Enter");
        k.ctrl = true;
        assert_eq!(encode(&k, &modes()).bytes, vec![0x0a]);
    }

    #[test]
    fn ctrl_enter_lf_overrides_lnm_crlf() {
        // Even under LNM mode, Ctrl+Enter must stay LF so the inline-TUI
        // newline behaviour is independent of terminal line-feed mode.
        let mut m = modes();
        m.linefeed_newline = true;
        let mut k = key("Enter");
        k.ctrl = true;
        assert_eq!(encode(&k, &m).bytes, vec![0x0a]);
    }

    #[test]
    fn backspace_is_del_default() {
        assert_eq!(encode(&key("Backspace"), &modes()).bytes, vec![0x7f]);
    }

    #[test]
    fn ctrl_backspace_is_word_erase() {
        let mut k = key("Backspace");
        k.ctrl = true;
        assert_eq!(encode(&k, &modes()).bytes, vec![0x17]);
    }

    #[test]
    fn shift_tab_is_back_tab() {
        let mut k = key("Tab");
        k.shift = true;
        assert_eq!(encode(&k, &modes()).bytes, b"\x1b[Z");
    }

    #[test]
    fn arrow_normal_mode_is_csi() {
        assert_eq!(encode(&key("ArrowUp"), &modes()).bytes, b"\x1b[A");
    }

    #[test]
    fn arrow_app_mode_is_ss3() {
        let mut m = modes();
        m.app_cursor_keys = true;
        assert_eq!(encode(&key("ArrowUp"), &m).bytes, b"\x1bOA");
    }

    #[test]
    fn modified_arrow_uses_csi_1_mod_form() {
        let mut k = key("ArrowUp");
        k.ctrl = true;
        // Ctrl alone = modifier 5 (per xterm).
        assert_eq!(encode(&k, &modes()).bytes, b"\x1b[1;5A");
    }

    #[test]
    fn f1_is_ss3_p() {
        assert_eq!(encode(&key("F1"), &modes()).bytes, b"\x1bOP");
    }

    #[test]
    fn f5_is_csi_15_tilde() {
        assert_eq!(encode(&key("F5"), &modes()).bytes, b"\x1b[15~");
    }

    #[test]
    fn pageup_unmodified() {
        assert_eq!(encode(&key("PageUp"), &modes()).bytes, b"\x1b[5~");
    }

    #[test]
    fn shift_pageup_modified() {
        let mut k = key("PageUp");
        k.shift = true;
        // Shift alone = modifier 2.
        assert_eq!(encode(&k, &modes()).bytes, b"\x1b[5;2~");
    }

    #[test]
    fn home_normal_mode() {
        assert_eq!(encode(&key("Home"), &modes()).bytes, b"\x1b[H");
    }

    #[test]
    fn home_app_mode() {
        let mut m = modes();
        m.app_cursor_keys = true;
        assert_eq!(encode(&key("Home"), &m).bytes, b"\x1bOH");
    }

    #[test]
    fn escape_key() {
        assert_eq!(encode(&key("Escape"), &modes()).bytes, vec![0x1b]);
    }

#[test]
fn unknown_key_returns_ignored() {
        let r = encode(&key("Process"), &modes()); // IME placeholder
        assert!(!r.consumed);
        assert!(r.bytes.is_empty());
    }

    #[test]
    fn paste_wraps_when_bracketed_on() {
        let bytes = wrap_paste("hi", true);
        assert_eq!(bytes, b"\x1b[200~hi\x1b[201~".to_vec());
    }

    #[test]
    fn paste_passthrough_when_bracketed_off() {
        assert_eq!(wrap_paste("hi", false), b"hi");
    }

    // ---- mouse encoding tests ----------------------------------------

    #[test]
    fn mouse_left_click_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(0, 2, 5, 0, false, false, false, &m);
        // btn=0, row=3, col=6 → ESC [ < 0 ; 3 ; 6 M
        assert_eq!(bytes, b"\x1b[<0;3;6M");
    }

    #[test]
    fn mouse_right_click_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(2, 10, 20, 0, false, false, false, &m);
        assert_eq!(bytes, b"\x1b[<2;11;21M");
    }

    #[test]
    fn mouse_release_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(3, 5, 8, 1, false, false, false, &m);
        // release → suffix 'm'
        assert_eq!(bytes, b"\x1b[<3;6;9m");
    }

    #[test]
    fn mouse_motion_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(0, 3, 7, 2, false, false, false, &m);
        // motion → +32 flag → btn 32
        assert_eq!(bytes, b"\x1b[<32;4;8M");
    }

    #[test]
    fn mouse_shift_click_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(0, 1, 1, 0, true, false, false, &m);
        // shift → +4 → btn 4
        assert_eq!(bytes, b"\x1b[<4;2;2M");
    }

    #[test]
    fn mouse_ctrl_alt_click_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(0, 0, 0, 0, false, true, true, &m);
        // ctrl=16 + alt=8 → btn 24
        assert_eq!(bytes, b"\x1b[<24;1;1M");
    }

    #[test]
    fn mouse_scroll_up_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(64, 5, 10, 0, false, false, false, &m);
        assert_eq!(bytes, b"\x1b[<64;6;11M");
    }

    #[test]
    fn mouse_all_modifiers_motion_sgr() {
        let m = Modes::default();
        let bytes = encode_mouse(0, 4, 9, 2, true, true, true, &m);
        // shift(4) + alt(8) + ctrl(16) + motion(32) = 60
        assert_eq!(bytes, b"\x1b[<60;5;10M");
    }
}
