//! East-Asian-aware wcwidth, ported from Pane.svelte's `termWcwidth`.
//!
//! Three return values:
//!   0 = combining / control / zero-width (don't advance cursor)
//!   1 = narrow
//!   2 = wide (CJK / emoji)
//!
//! Emoji ranges are *forced* to width 2 even when legacy Unicode tables
//! call them ambiguous-narrow. Without this, color-emoji fonts overflow
//! into adjacent cells on WebView2/Chromium — same fix Pane.svelte makes
//! via the `emoji-wide` unicode registration.

#[inline]
pub fn wcwidth(cp: u32) -> u8 {
    if cp < 0x20 { return 0; }
    if cp < 0x7f { return 1; }
    if cp < 0xa0 { return 0; }
    if cp < 0x300 { return 1; }

    // Combining marks / formatting controls / variation selectors.
    if (0x300..=0x36f).contains(&cp)
        || (0x483..=0x489).contains(&cp)
        || (0x591..=0x5bd).contains(&cp)
        || (0x610..=0x61a).contains(&cp)
        || cp == 0x61c
        || (0x64b..=0x65f).contains(&cp)
        || cp == 0x670
        || (0x200b..=0x200f).contains(&cp)
        || (0x202a..=0x202e).contains(&cp)
        || (0x2060..=0x2064).contains(&cp)
        || (0x206a..=0x206f).contains(&cp)
        || (0xfe00..=0xfe0f).contains(&cp)
        || cp == 0xfeff
        || (0xfff9..=0xfffb).contains(&cp)
        || (0xe0100..=0xe01ef).contains(&cp)
    {
        return 0;
    }

    if cp < 0x1100 { return 1; }

    // Wide ranges. Most-common first for branch prediction.
    if cp <= 0x115f
        || cp == 0x2329 || cp == 0x232a
        || (0x2e80..=0x303e).contains(&cp)
        || (0x3041..=0x33ff).contains(&cp)
        || (0x3400..=0x4dbf).contains(&cp)
        || (0x4e00..=0xa4cf).contains(&cp)
        || (0xa960..=0xa97f).contains(&cp)
        || (0xac00..=0xd7af).contains(&cp)
        || (0xf900..=0xfaff).contains(&cp)
        || (0xfe10..=0xfe19).contains(&cp)
        || (0xfe30..=0xfe6f).contains(&cp)
        || (0xff00..=0xff60).contains(&cp)
        || (0xffe0..=0xffe6).contains(&cp)
        || (0x1b000..=0x1b1ff).contains(&cp)
        || cp == 0x1f004 || cp == 0x1f0cf
        || (0x1f200..=0x1f251).contains(&cp)
        || (0x1f300..=0x1fbff).contains(&cp)
        || (0x2600..=0x27bf).contains(&cp)
        || (0x20000..=0x2fffd).contains(&cp)
        || (0x30000..=0x3fffd).contains(&cp)
    {
        return 2;
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_narrow() {
        assert_eq!(wcwidth(b'a' as u32), 1);
        assert_eq!(wcwidth(b'~' as u32), 1);
    }

    #[test]
    fn controls_are_zero() {
        assert_eq!(wcwidth(0x00), 0);
        assert_eq!(wcwidth(0x1b), 0);
        assert_eq!(wcwidth(0x7f), 0);
    }

    #[test]
    fn cjk_is_wide() {
        assert_eq!(wcwidth('中' as u32), 2);
        assert_eq!(wcwidth('日' as u32), 2);
        assert_eq!(wcwidth('한' as u32), 2);
    }

    #[test]
    fn emoji_is_wide() {
        assert_eq!(wcwidth(0x1f600), 2); // 😀
        assert_eq!(wcwidth(0x1f680), 2); // 🚀
        assert_eq!(wcwidth(0x2705),  2); // ✅
    }

    #[test]
    fn combining_is_zero() {
        assert_eq!(wcwidth(0x0301), 0); // combining acute
        assert_eq!(wcwidth(0x200b), 0); // zero-width space
        assert_eq!(wcwidth(0xfe0f), 0); // VS16
    }
}
