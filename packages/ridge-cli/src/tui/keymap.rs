//! 把 crossterm 键盘事件编码成终端字节序列（xterm/VT 约定），回送给会话 PTY。
//!
//! 纯函数、可单测——这是 passthrough TUI 里唯一需要"翻译"的环节（输出方向是
//! 原样透传，无需翻译）。覆盖常用键：可打印字符、Ctrl+字母、方向/编辑键、F1–F12。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const ESC: u8 = 0x1b;

/// 编码一个按键为待发送字节；返回 `None` 表示该键无对应序列（忽略）。
pub fn encode_key(ev: &KeyEvent) -> Option<Vec<u8>> {
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    let alt = ev.modifiers.contains(KeyModifiers::ALT);

    let base: Vec<u8> = match ev.code {
        KeyCode::Char(c) => {
            if ctrl {
                ctrl_byte(c)?
            } else {
                char_bytes(c)
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => vec![ESC, b'[', b'Z'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![ESC],
        KeyCode::Left => csi(b'D'),
        KeyCode::Right => csi(b'C'),
        KeyCode::Up => csi(b'A'),
        KeyCode::Down => csi(b'B'),
        KeyCode::Home => csi(b'H'),
        KeyCode::End => csi(b'F'),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::F(n) => function_key(n)?,
        _ => return None,
    };

    // Alt（Meta）：xterm 约定在序列前加 ESC。仅对字符方向有意义，特殊键也照加无害。
    if alt {
        let mut out = Vec::with_capacity(base.len() + 1);
        out.push(ESC);
        out.extend_from_slice(&base);
        Some(out)
    } else {
        Some(base)
    }
}

/// 可打印字符的 UTF-8 字节。
fn char_bytes(c: char) -> Vec<u8> {
    let mut buf = [0u8; 4];
    c.encode_utf8(&mut buf).as_bytes().to_vec()
}

/// Ctrl+<字符> → 控制字节（C0）。无对应者返回 None。
fn ctrl_byte(c: char) -> Option<Vec<u8>> {
    let b = match c {
        'a'..='z' => (c as u8 - b'a') + 1,        // Ctrl+A..Z → 0x01..0x1a
        'A'..='Z' => (c as u8 - b'A') + 1,
        '@' | ' ' => 0x00,                         // Ctrl+@ / Ctrl+Space → NUL
        '[' => 0x1b,                               // Ctrl+[ → ESC
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' | '/' => 0x1f,
        '?' => 0x7f,
        _ => return None,
    };
    Some(vec![b])
}

/// CSI 单字母终止序列：`ESC [ <final>`。
fn csi(final_byte: u8) -> Vec<u8> {
    vec![ESC, b'[', final_byte]
}

/// CSI 数字 + `~` 序列：`ESC [ <n> ~`。
fn tilde(n: u8) -> Vec<u8> {
    let mut v = vec![ESC, b'['];
    v.extend_from_slice(n.to_string().as_bytes());
    v.push(b'~');
    v
}

/// F1–F12 的 xterm 序列。
fn function_key(n: u8) -> Option<Vec<u8>> {
    let seq = match n {
        1 => vec![ESC, b'O', b'P'],
        2 => vec![ESC, b'O', b'Q'],
        3 => vec![ESC, b'O', b'R'],
        4 => vec![ESC, b'O', b'S'],
        5 => tilde(15),
        6 => tilde(17),
        7 => tilde(18),
        8 => tilde(19),
        9 => tilde(20),
        10 => tilde(21),
        11 => tilde(23),
        12 => tilde(24),
        _ => return None,
    };
    Some(seq)
}

/// 检测按键是否为控制快捷键（不应发往 PTY，应由 pager 拦截处理）。
pub fn is_control_shortcut(ev: &KeyEvent) -> bool {
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);

    if !ctrl {
        return false;
    }

    let shift = ev.modifiers.contains(KeyModifiers::SHIFT);

    // Ctrl+Shift+方向键 → pane 切换
    if shift {
        return matches!(
            ev.code,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
        );
    }

    // Ctrl+F1..F12 → 工作区切换
    matches!(ev.code, KeyCode::F(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: mods,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn plain_char() {
        assert_eq!(encode_key(&key(KeyCode::Char('a'), KeyModifiers::NONE)), Some(vec![b'a']));
        assert_eq!(encode_key(&key(KeyCode::Char('A'), KeyModifiers::SHIFT)), Some(vec![b'A']));
    }

    #[test]
    fn ctrl_letters() {
        assert_eq!(encode_key(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(vec![0x03]));
        assert_eq!(encode_key(&key(KeyCode::Char('d'), KeyModifiers::CONTROL)), Some(vec![0x04]));
        // Ctrl+] = 0x1d（也是 TUI 退出热键，由循环在编码前拦截）。
        assert_eq!(encode_key(&key(KeyCode::Char(']'), KeyModifiers::CONTROL)), Some(vec![0x1d]));
    }

    #[test]
    fn editing_and_arrows() {
        assert_eq!(encode_key(&key(KeyCode::Enter, KeyModifiers::NONE)), Some(vec![b'\r']));
        assert_eq!(encode_key(&key(KeyCode::Backspace, KeyModifiers::NONE)), Some(vec![0x7f]));
        assert_eq!(encode_key(&key(KeyCode::Up, KeyModifiers::NONE)), Some(vec![ESC, b'[', b'A']));
        assert_eq!(encode_key(&key(KeyCode::Delete, KeyModifiers::NONE)), Some(vec![ESC, b'[', b'3', b'~']));
    }

    #[test]
    fn alt_prefixes_esc() {
        assert_eq!(encode_key(&key(KeyCode::Char('x'), KeyModifiers::ALT)), Some(vec![ESC, b'x']));
    }

    #[test]
    fn utf8_char() {
        // 中文字符按 UTF-8 多字节透传。
        assert_eq!(encode_key(&key(KeyCode::Char('好'), KeyModifiers::NONE)), Some("好".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_shift_arrows_are_shortcuts() {
        for dir in &[KeyCode::Left, KeyCode::Right, KeyCode::Up, KeyCode::Down] {
            let ev = key(*dir, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
            assert!(is_control_shortcut(&ev), "Ctrl+Shift+{dir:?} should be shortcut");
        }
    }

    #[test]
    fn plain_arrows_not_shortcuts() {
        let ev = key(KeyCode::Left, KeyModifiers::NONE);
        assert!(!is_control_shortcut(&ev));
        let ev = key(KeyCode::Right, KeyModifiers::SHIFT);
        assert!(!is_control_shortcut(&ev));
        let ev = key(KeyCode::Up, KeyModifiers::CONTROL);
        assert!(!is_control_shortcut(&ev), "Ctrl+Up alone should NOT be shortcut");
    }

    #[test]
    fn ctrl_f_keys_are_shortcuts() {
        for n in 1..=12 {
            let ev = key(KeyCode::F(n), KeyModifiers::CONTROL);
            assert!(is_control_shortcut(&ev), "Ctrl+F{n} should be shortcut");
        }
    }

    #[test]
    fn plain_f_keys_not_shortcuts() {
        let ev = key(KeyCode::F(1), KeyModifiers::NONE);
        assert!(!is_control_shortcut(&ev));
        let ev = key(KeyCode::F(3), KeyModifiers::SHIFT);
        assert!(!is_control_shortcut(&ev));
    }

    #[test]
    fn other_ctrl_keys_not_shortcuts() {
        let ev = key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(!is_control_shortcut(&ev), "Ctrl+C should NOT be shortcut");
        let ev = key(KeyCode::Char(']'), KeyModifiers::CONTROL);
        assert!(!is_control_shortcut(&ev), "Ctrl+] should NOT be shortcut");
        let ev = key(KeyCode::Enter, KeyModifiers::CONTROL);
        assert!(!is_control_shortcut(&ev));
    }
}
