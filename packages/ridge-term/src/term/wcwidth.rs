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
    if cp < 0x20 {
        return 0;
    }
    if cp < 0x7f {
        return 1;
    }
    if cp < 0xa0 {
        return 0;
    }
    if cp < 0x300 {
        return 1;
    }

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

    if cp < 0x1100 {
        return 1;
    }

    // Wide ranges. Most-common first for branch prediction.
    if cp <= 0x115f
        || cp == 0x2329
        || cp == 0x232a
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
        || cp == 0x1f004
        || cp == 0x1f0cf
        || (0x1f200..=0x1f251).contains(&cp)
        || (0x1f300..=0x1fbff).contains(&cp)
        || (0x20000..=0x2fffd).contains(&cp)
        || (0x30000..=0x3fffd).contains(&cp)
    {
        return 2;
    }

    // §A.5 (2026-05-08) — Misc Symbols + Dingbats (0x2600..=0x27BF).
    // ONLY the codepoints with Unicode property `Emoji_Presentation=Yes`
    // need width 2 (color-emoji glyphs from system fonts overflow a
    // single cell on WebView2/Chromium). The earlier blanket rule
    // `(0x2600..=0x27BF) => 2` overshot massively: it forced "Neutral"-
    // width Dingbats like `✻` U+273B / `✽` U+273D / `✶` U+2736 / `❯`
    // U+276F (Claude Code's spinner glyphs and prompt arrow) to
    // width 2, while npm's `string-width` library (which Claude Code
    // uses) treats them as width 1 per the canonical `unicode-width`
    // table. Result: Claude's incremental cursor-and-write updates
    // (e.g. `\x1b[14;14Hg`) targeted columns computed against a
    // 1-wide-leading model, while ridge-term's 2-wide-leading shift
    // placed the actual cell one column to the right — visible bug:
    // spinner words like "Tomfoolering" rendered as "Tomfoolerigg".
    // The list below mirrors `emoji-data.txt`'s Emoji_Presentation
    // set restricted to this block.
    if matches!(
        cp,
        0x2614 | 0x2615
            | 0x26a1
            | 0x26aa
            | 0x26ab
            | 0x26bd
            | 0x26be
            | 0x26c4
            | 0x26c5
            | 0x26ce
            | 0x26d4
            | 0x26ea
            | 0x26f2
            | 0x26f3
            | 0x26f5
            | 0x26fa
            | 0x26fd
            | 0x2705
            | 0x270a
            | 0x270b
            | 0x2728
            | 0x274c
            | 0x274e
            | 0x2753
            | 0x2754
            | 0x2755
            | 0x2757
            | 0x2795
            | 0x2796
            | 0x2797
            | 0x27b0
            | 0x27bf
    ) {
        return 2;
    }

    1
}

/// §4.7 (2026-05-07) — width of an extended grapheme cluster as it
/// occupies grid cells. Take the maximum `wcwidth` across all
/// codepoints in the cluster: ZWJ / variation selectors / combining
/// marks all return 0, but the cluster's *visible* glyph width is
/// driven by the widest base codepoint inside it. Examples:
///   "👨"           → 2 (single wide codepoint).
///   "👨\u{200d}👩" → 2 (👨 wide, ZWJ zero, 👩 wide → max 2).
///   "🏳\u{fe0f}\u{200d}🌈" → 2 (rainbow flag with VS16 → max 2).
///   "🇺🇸"           → 2 (RIS pair = flag, special-cased to width 2).
///   "a"             → 1.
///   "à" (a + combining grave) → 1.
/// Empty string returns 0 (caller shouldn't pass empty, but safe default).
///
/// Regional Indicator pair special case: each RIS codepoint by itself
/// is `wcwidth == 1`, but two adjacent RIS codepoints render as a
/// single flag emoji that fonts paint at 2-cell width. Without the
/// special case the cluster would write width=1 and the glyph would
/// overflow into the neighbour cell.
#[inline]
pub fn wcwidth_grapheme(s: &str) -> u8 {
    let mut chars = s.chars();
    if let (Some(a), Some(b)) = (chars.next(), chars.next()) {
        let acp = a as u32;
        let bcp = b as u32;
        if (0x1F1E6..=0x1F1FF).contains(&acp) && (0x1F1E6..=0x1F1FF).contains(&bcp) {
            return 2;
        }
    }
    s.chars().map(|c| wcwidth(c as u32)).max().unwrap_or(0)
}

/// True when the codepoint is in a Unicode block fonts typically render
/// as a color emoji glyph (COLR / CPAL / sbix / SVG). Used by Canvas2D
/// to decide whether a width-2 cell should stretch its `fillText` output
/// horizontally to fill both cells — emoji glyphs from system fonts
/// have a natural advance ≈ 1em, which is narrower than 2 latin-cell
/// widths, leaving a visible gap on the right of the cell pair.
///
/// Conservative on purpose: covers the major emoji blocks but not every
/// possible color glyph. CJK ideographs (also width-2) are NOT included
/// — their fonts target 1em advance by design and shouldn't be stretched.
///
/// WebGPU has a more accurate per-glyph detection (pixel-scan in the
/// rasterizer, stored as `GlyphEntry::is_color`); Canvas2D draws
/// directly via the browser's `fillText` and never sees the rasterized
/// pixels, so it falls back to this codepoint heuristic.
#[inline]
pub fn is_color_emoji_codepoint(cp: u32) -> bool {
    cp == 0x1F004                            // 🀄
        || cp == 0x1F0CF                      // 🃏
        || (0x1F1E6..=0x1F1FF).contains(&cp)  // Regional Indicators (flag halves)
        || (0x1F200..=0x1F251).contains(&cp)  // Enclosed CJK
        || (0x1F300..=0x1FBFF).contains(&cp)  // Symbols + emoticons + Supplemental Symbols
        || (0x2600..=0x27BF).contains(&cp)    // Misc symbols + Dingbats (✅ ☀ ⚡ etc.)
}

/// §A.6 (2026-05-08) — `true` when a width=1 codepoint should still be
/// RENDERED with a 2-cell visual advance so the glyph isn't horizontally
/// compressed by the renderer's narrow-cell quad.
///
/// Cell layout still treats these as width 1 (matching `string-width` /
/// `unicode-width` and Claude Code's column accounting — see §A.5), but
/// in the draw pipeline they're stretched to a 2-cell quad **only when
/// the next cell is blank** (space at default attrs), so the overflowing
/// half cannot collide with a neighbouring glyph that would otherwise
/// paint over it.
///
/// Initial set: star / asterisk / florette Dingbats commonly used as
/// spinner glyphs. `Emoji_Presentation` codepoints in the same block
/// (✨ U+2728 etc.) are intentionally EXCLUDED — they already get
/// width=2 from `wcwidth`, so they don't need this visual-only path
/// and including them would double-stretch.
///
/// `❯` U+276F (HEAVY RIGHT-POINTING ANGLE QUOTATION MARK) is NOT in
/// the set: it's commonly used as a shell prompt arrow and looks
/// correct at 1-cell natural advance — stretching it would make the
/// prompt feel "fat" relative to surrounding ASCII.
#[inline]
pub fn is_visual_wide_codepoint(cp: u32) -> bool {
    // Star Dingbats (0x2605..=0x2606) — solid + outlined star.
    // Heavy / floral / pinwheel asterisks and stars (0x2726..=0x273F),
    // excluding 0x2728 ✨ (already Emoji_Presentation, width=2).
    // Ornamental stars (0x2740..=0x274D), excluding 0x274C/0x274E
    // (Emoji_Presentation).
    matches!(
        cp,
        0x2605 | 0x2606
            | 0x2726
            | 0x2727
            | 0x2729..=0x273F
            | 0x2740..=0x274B
            | 0x274D
    )
}

/// §4.7 — `true` when the codepoint COULD combine with what comes next
/// to extend the current grapheme cluster, so the parser should buffer
/// rather than emitting immediately. Conservative — false positives
/// (buffering one extra char) are harmless but false negatives (split
/// cluster mid-flight) would render the cluster wrong. Catches:
///   - ZWJ (U+200D): emoji ZWJ sequences.
///   - Variation Selectors (U+FE00..=U+FE0F, U+E0100..=U+E01EF).
///   - Regional Indicator Symbols (U+1F1E6..=U+1F1FF) — first half
///     of a flag pair waits for the partner.
///   - Anything else with `wcwidth == 0` (combining marks, etc.).
///   - Hangul L jamo (U+1100..=U+115F): wcwidth=2 already (so caught
///     by the L+V+T composition rule via grapheme segmenter), but the
///     segmenter only sees the extension AFTER it arrives — buffer to
///     give it that chance.
#[inline]
pub fn could_extend_grapheme(c: char) -> bool {
    let cp = c as u32;
    if cp == 0x200D {
        return true;
    }
    if (0xFE00..=0xFE0F).contains(&cp) {
        return true;
    }
    if (0xE0100..=0xE01EF).contains(&cp) {
        return true;
    }
    if (0x1F1E6..=0x1F1FF).contains(&cp) {
        return true;
    }
    if (0x1100..=0x115F).contains(&cp) {
        return true;
    }
    wcwidth(cp) == 0
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
        assert_eq!(wcwidth(0x2705), 2); // ✅
    }

    #[test]
    fn dingbats_neutral_are_narrow() {
        // §A.5 — Dingbats with East Asian Width = Neutral and no
        // Emoji_Presentation property must be width 1, matching what
        // npm `string-width` reports. Earlier blanket rule wrongly
        // returned 2 for the entire 0x2600-0x27BF block.
        assert_eq!(wcwidth(0x273B), 1, "✻ BLACK FOUR POINTED STAR");
        assert_eq!(wcwidth(0x273D), 1, "✽ HEAVY TEARDROP-SPOKED ASTERISK");
        assert_eq!(wcwidth(0x2736), 1, "✶ SIX POINTED BLACK STAR");
        assert_eq!(wcwidth(0x276F), 1, "❯ HEAVY RIGHT-POINTING ANGLE QUOTATION MARK");
    }

    #[test]
    fn visual_wide_set_matches_spinner_glyphs() {
        // §A.6 — narrow-but-visually-wide codepoints. Cell layout
        // returns width 1 (verified above), but the renderer should
        // stretch their quad to 2 cells when the next cell is blank.
        assert!(is_visual_wide_codepoint(0x273B), "✻");
        assert!(is_visual_wide_codepoint(0x273D), "✽");
        assert!(is_visual_wide_codepoint(0x2736), "✶");
        assert!(is_visual_wide_codepoint(0x2737), "✷");
        assert!(is_visual_wide_codepoint(0x2605), "★");
        // Emoji_Presentation: NOT in the visual-wide set (already
        // wide via wcwidth).
        assert!(!is_visual_wide_codepoint(0x2728), "✨ already wcwidth=2");
        assert!(!is_visual_wide_codepoint(0x274C), "❌ already wcwidth=2");
        // Prompt arrow: intentionally excluded — looks fine narrow.
        assert!(!is_visual_wide_codepoint(0x276F), "❯ stays narrow");
        // ASCII / random: untouched.
        assert!(!is_visual_wide_codepoint(b'a' as u32));
        assert!(!is_visual_wide_codepoint(b'*' as u32));
    }

    #[test]
    fn dingbats_emoji_presentation_stay_wide() {
        // §A.5 — Codepoints in the Misc Symbols / Dingbats range that
        // ARE Emoji_Presentation=Yes must still be width 2 so color-
        // emoji fonts paint at full 2-cell glyph advance.
        assert_eq!(wcwidth(0x2614), 2); // ☔
        assert_eq!(wcwidth(0x2615), 2); // ☕
        assert_eq!(wcwidth(0x26A1), 2); // ⚡
        assert_eq!(wcwidth(0x2728), 2); // ✨
        assert_eq!(wcwidth(0x274C), 2); // ❌
        assert_eq!(wcwidth(0x2753), 2); // ❓
    }

    #[test]
    fn combining_is_zero() {
        assert_eq!(wcwidth(0x0301), 0); // combining acute
        assert_eq!(wcwidth(0x200b), 0); // zero-width space
        assert_eq!(wcwidth(0xfe0f), 0); // VS16
    }
}
