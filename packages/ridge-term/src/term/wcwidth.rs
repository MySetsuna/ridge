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

    // §B.8 (2026-05-08) — POLICY REVERSAL of §B.4 / §B.5 / §B.7.
    //
    // The earlier "widen non-Emoji_Presentation Dingbats / Misc Symbols
    // / Misc Technical to width=2" policy was abandoned after the user
    // reported (a) display-length-shorter-than-cursor gaps for
    // monochrome glyphs whose font advance was < 2 cells (✻ ✽ ✶ ✢
    // from JetBrains Mono / Cascadia Code), and (b) Claude Code
    // spinner cursor drift (the original §A.5 reason).
    //
    // Per Gemini's analysis (matching xterm.js's WebGL/WebGPU
    // architecture), the correct model is:
    //
    //     LOGICAL width = strict Unicode wcwidth (Emoji_Presentation=Yes
    //         only; everything else stays at width=1)
    //     VISUAL width  = renderer-side runtime decision driven by
    //         the rasterizer's measured natural advance (`GlyphEntry::
    //         px_w`)
    //
    // The renderer's "narrow cell with wide-rendered glyph" path
    // (see webgpu.rs::draw_row §B.8 branch + canvas2d.rs equivalent)
    // splits the cell into a 1-cell bg quad + a natural-advance glyph
    // quad that is allowed to overflow into the next cell. The next
    // cell's instance, drawn after, naturally over-paints the
    // overflow if it carries content (xterm.js's "allowOverlap" mode
    // — content wins over decoration).
    //
    // This eliminates the ENTIRE class of wcwidth-widening whack-a-
    // mole bugs:
    //   * No more "✔ rendered at width=1 but font draws 1.37em" → the
    //     renderer detects the overflow at runtime and paints natural.
    //   * No more "✻ at width=2 but font draws 1.0em" → cell stays
    //     width=1, no gap.
    //   * Claude Code / npm string-width / .NET LengthInBufferCells
    //     all see width=1 → cursor accounting stays aligned.
    //   * Mode 2027 (advertised in §B.6) tells modern apps to use
    //     grapheme-cluster width — which for these single-codepoint
    //     glyphs IS strict Unicode wcwidth=1.
    //
    // Codepoints widened by §B.4 / §B.5 / §B.7 are intentionally NOT
    // re-added here. The runtime overflow path in the renderer covers
    // them all.

    // §B.2 (2026-05-08) — remaining BMP `Emoji_Presentation=Yes`
    // codepoints scattered outside the Misc Symbols + Dingbats block
    // (0x2600..=0x27BF, handled above) and outside the SMP emoji ranges
    // (0x1F300..=0x1FBFF + friends, handled at line 71). Without these,
    // codepoints like ⌚ U+231A, ⌛ U+231B, ⏰ U+23F0, ⏳ U+23F3,
    // ♈–♓ U+2648..=U+2653, ⬛ U+2B1B, ⭐ U+2B50, ⭕ U+2B55 fell through
    // the chain and returned width=1 — fonts then rendered them at
    // their natural color-emoji ~1.37em advance squashed into a single
    // ~0.6em latin cell, producing the user-visible "emoji 被裁切和挤压
    // 缩小" symptom (kernel says width=1 → renderer's narrow path picks
    // a 1-cell quad → atlas's wide bitmap gets stretched / compressed
    // into it). Source: Unicode 15.1 emoji-data.txt, property
    // `Emoji_Presentation`.
    //
    // Ranges are listed in codepoint order so a future Unicode revision
    // adding a new Emoji_Presentation codepoint here is a single-line
    // diff with diff-friendly context.
    if matches!(
        cp,
        0x231a..=0x231b              // ⌚ ⌛
            | 0x23e9..=0x23ec        // ⏩ ⏪ ⏫ ⏬
            | 0x23f0                 // ⏰
            | 0x23f3                 // ⏳
            | 0x25fd..=0x25fe        // ◽ ◾
            | 0x2648..=0x2653        // ♈ ♉ ♊ ♋ ♌ ♍ ♎ ♏ ♐ ♑ ♒ ♓
            | 0x267f                 // ♿
            | 0x2693                 // ⚓
            // NOTE: 0x2B05..=0x2B07 ⬅⬆⬇ are intentionally omitted —
            // their default presentation per Unicode is TEXT (width 1).
            // The VS16-emoji form (e.g. ⬅️) flows through `print_grapheme`
            // and `wcwidth_grapheme`, which patches width to 2 when the
            // cluster carries Emoji_Presentation. Forcing them wide here
            // would break ASCII-art table boundaries that legitimately
            // use these arrows as 1-cell text glyphs.
            | 0x2b1b..=0x2b1c        // ⬛ ⬜
            | 0x2b50                 // ⭐
            | 0x2b55                 // ⭕
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

    // §B.2 (2026-05-08) — VS16 emoji-presentation promotion. Per
    // Unicode UTR #51, a default-text-presentation codepoint followed
    // by VS16 (U+FE0F) renders as the emoji form, which by convention
    // is full-width in monospaced contexts. Without this override the
    // base codepoint's `wcwidth` (= 1 for ❤ U+2764, ⬅ U+2B05, ☎ U+260E,
    // ☘ U+2618, etc.) wins the `max()` below and the renderer paints
    // the wide emoji glyph squeezed into a 1-cell quad — same root
    // cause as the codepoint-table miss fixed in `wcwidth` itself, but
    // for the cluster path.
    //
    // Gating on Extended_Pictographic-ish range membership keeps a
    // stray VS16 after ASCII (`a\u{FE0F}`) from over-allocating —
    // pragmatic since ASCII / Latin / CJK letters never carry VS16
    // legitimately. Lead-codepoint check uses the same ranges used
    // upstream for color-emoji detection (`is_color_emoji_codepoint`)
    // plus a few text-default emoji blocks not in that set
    // (Letterlike Symbols' ™ U+2122, dingbats text presentation, etc.).
    let leading = s.chars().next().map(|c| c as u32).unwrap_or(0);
    let has_vs16 = s.chars().any(|c| c as u32 == 0xFE0F);
    if has_vs16 && is_emoji_capable_codepoint(leading) {
        return 2;
    }

    s.chars().map(|c| wcwidth(c as u32)).max().unwrap_or(0)
}

/// True when `cp` belongs to a Unicode block whose codepoints have an
/// emoji presentation (default OR VS16-promoted) in the Unicode 15.1
/// emoji-data.txt `Extended_Pictographic` set. Used by
/// `wcwidth_grapheme` to decide whether `<cp> + VS16` should be widened
/// to 2 cells. Conservative — covers the common emoji ranges plus the
/// scattered BMP codepoints with text-default-but-emoji-capable status
/// (heart ❤, telephone ☎, snowman ☃, sun ☀, etc.). Anything outside
/// these ranges with a stray VS16 stays at its `wcwidth` width to
/// avoid over-allocating cells for non-emoji-with-stray-VS16 input.
#[inline]
fn is_emoji_capable_codepoint(cp: u32) -> bool {
    // Misc Symbols + Dingbats (covers ❤ ☀ ☁ ☂ ☃ ☎ ☘ ☠ ☢ ☣ ☦ ☪ ☮ ☯
    // ☸ ♀ ♂ ♟ ♠ ♣ ♥ ♦ ♨ ♻ ♾ ⚒ ⚔ ⚕ ⚖ ⚗ ⚙ ⚛ ⚜ ⚠ ⚧ ⚰ ⚱ ✂ ✈ ✉ ✏ ✒ ✔
    // ✖ ✝ ✡ ✳ ✴ ❄ ❇ ❣ ❤ ➡ ➰ ➿ etc.). One range catches them all.
    if (0x2600..=0x27BF).contains(&cp) {
        return true;
    }
    // Misc Technical text-default emoji (alarm clock face, eject etc.).
    if (0x2300..=0x23FF).contains(&cp) {
        return true;
    }
    // Letterlike Symbols (™ U+2122, ℹ U+2139).
    if cp == 0x2122 || cp == 0x2139 {
        return true;
    }
    // Arrows + Misc Symbols/Arrows (text-default arrows like ↩ U+21A9,
    // ↪ U+21AA, ↗ U+2197, ↘ U+2198, ↙ U+2199, ↖ U+2196, ↔ U+2194,
    // ↕ U+2195 — all VS16-promoteable to emoji forms).
    if (0x2190..=0x21FF).contains(&cp) {
        return true;
    }
    // Misc Symbols and Arrows (⬅⬆⬇⬛⬜⭐⭕ etc.).
    if (0x2B00..=0x2BFF).contains(&cp) {
        return true;
    }
    // CJK Symbols and Punctuation (〽 U+303D, 〰 U+3030).
    if cp == 0x3030 || cp == 0x303D {
        return true;
    }
    // SMP emoji blocks — already wide via wcwidth, but still legitimate
    // VS16 carriers for explicit emoji-presentation requests.
    if (0x1F000..=0x1FBFF).contains(&cp) {
        return true;
    }
    false
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
    fn dingbat_stars_and_asterisks_are_strict_unicode_narrow() {
        // §B.8 (2026-05-08) — policy reversal of §B.4/§B.5/§B.7.
        // Strict Unicode wcwidth applies: only Emoji_Presentation=Yes
        // codepoints are width=2; everything else (✻ ✽ ✶ ✢ ✔ ⏸ ⚙
        // and the entire Dingbat star/asterisk block) returns 1.
        // The renderer-side runtime-overflow path (see webgpu.rs +
        // canvas2d.rs §B.8 branch) handles the visual widening at
        // draw time without affecting cursor accounting — Claude
        // Code / npm string-width / .NET .Length all see width=1
        // and stay aligned.
        assert_eq!(wcwidth(0x2722), 1, "✢ FOUR TEARDROP-SPOKED ASTERISK");
        assert_eq!(wcwidth(0x273B), 1, "✻ BLACK FOUR POINTED STAR");
        assert_eq!(wcwidth(0x273D), 1, "✽ HEAVY TEARDROP-SPOKED ASTERISK");
        assert_eq!(wcwidth(0x2736), 1, "✶ SIX POINTED BLACK STAR");
        assert_eq!(wcwidth(0x2605), 1, "★ BLACK STAR");
        assert_eq!(wcwidth(0x2606), 1, "☆ WHITE STAR");
        assert_eq!(wcwidth(0x2720), 1, "✠ MALTESE CROSS");
        assert_eq!(wcwidth(0x2740), 1, "❀ WHITE FLORETTE");
        // Common shell-prompt status glyphs — also strict-narrow now.
        assert_eq!(wcwidth(0x2714), 1, "✔ HEAVY CHECK MARK");
        assert_eq!(wcwidth(0x2718), 1, "✘ HEAVY BALLOT X");
        assert_eq!(wcwidth(0x2699), 1, "⚙ GEAR");
        assert_eq!(wcwidth(0x26A0), 1, "⚠ WARNING SIGN");
        assert_eq!(wcwidth(0x23F8), 1, "⏸ DOUBLE VERTICAL BAR (PAUSE)");
        assert_eq!(wcwidth(0x276F), 1, "❯ HEAVY RIGHT-POINTING ANGLE QUOTATION MARK");
    }

    #[test]
    fn emoji_presentation_set_remains_wide() {
        // §B.8 — the strict-Unicode `Emoji_Presentation=Yes` set is
        // unaffected by the policy reversal. These codepoints are
        // wide per Unicode itself (not just rendered wide by fonts);
        // every wcwidth implementation in the wild agrees they're
        // width=2, so cursor accounting stays consistent.
        assert_eq!(wcwidth(0x2705), 2, "✅ WHITE HEAVY CHECK MARK");
        assert_eq!(wcwidth(0x2728), 2, "✨ SPARKLES");
        assert_eq!(wcwidth(0x274C), 2, "❌ CROSS MARK");
        assert_eq!(wcwidth(0x274E), 2, "❎ NEGATIVE SQUARED CROSS MARK");
        assert_eq!(wcwidth(0x2753), 2, "❓ BLACK QUESTION MARK ORNAMENT");
        assert_eq!(wcwidth(0x26A1), 2, "⚡ HIGH VOLTAGE SIGN");
        assert_eq!(wcwidth(0x231A), 2, "⌚ WATCH");
        assert_eq!(wcwidth(0x2B50), 2, "⭐ WHITE MEDIUM STAR");
        // Non-BMP emoji also unaffected.
        assert_eq!(wcwidth(0x1F382), 2, "🎂 BIRTHDAY CAKE");
        assert_eq!(wcwidth(0x1F448), 2, "👈 BACKHAND INDEX POINTING LEFT");
    }

    #[test]
    fn visual_wide_set_kept_for_backwards_compat() {
        // §B.7 — `is_visual_wide_codepoint` is now redundant for the
        // codepoints it lists (they're wcwidth=2 directly), but the
        // function is preserved for backward compatibility with
        // renderer callsites that gate on it. The contract is:
        // `is_visual_wide_codepoint(cp) → true` for the spinner /
        // asterisk Dingbats. The renderer's `cell_span == 1 && ...`
        // check will simply never see these because cell.width is now
        // 2 — but the assertions still hold.
        assert!(is_visual_wide_codepoint(0x273B), "✻");
        assert!(is_visual_wide_codepoint(0x273D), "✽");
        assert!(is_visual_wide_codepoint(0x2736), "✶");
        assert!(is_visual_wide_codepoint(0x2737), "✷");
        assert!(is_visual_wide_codepoint(0x2605), "★");
        // Emoji_Presentation already wide.
        assert!(!is_visual_wide_codepoint(0x2728), "✨ Emoji_Presentation");
        assert!(!is_visual_wide_codepoint(0x274C), "❌ Emoji_Presentation");
        // Prompt arrow stays narrow.
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
    fn bmp_emoji_presentation_outside_dingbats_is_wide() {
        // §B.2 — codepoints with Emoji_Presentation=Yes scattered
        // outside the Dingbats / SMP ranges. Before the fix these
        // returned 1, causing color-emoji fonts to stretch a 1.37em
        // glyph into a 0.6em latin cell at render time.
        // Misc Technical
        assert_eq!(wcwidth(0x231A), 2, "⌚ WATCH");
        assert_eq!(wcwidth(0x231B), 2, "⌛ HOURGLASS");
        assert_eq!(wcwidth(0x23E9), 2, "⏩ FAST FORWARD");
        assert_eq!(wcwidth(0x23EA), 2, "⏪ REWIND");
        assert_eq!(wcwidth(0x23EB), 2, "⏫ UPWARDS DOUBLE TRIANGLE");
        assert_eq!(wcwidth(0x23EC), 2, "⏬ DOWNWARDS DOUBLE TRIANGLE");
        assert_eq!(wcwidth(0x23F0), 2, "⏰ ALARM CLOCK");
        assert_eq!(wcwidth(0x23F3), 2, "⏳ HOURGLASS WITH FLOWING SAND");
        // Geometric Shapes
        assert_eq!(wcwidth(0x25FD), 2, "◽ MEDIUM SMALL WHITE SQUARE");
        assert_eq!(wcwidth(0x25FE), 2, "◾ MEDIUM SMALL BLACK SQUARE");
        // Misc Symbols (zodiac + others)
        assert_eq!(wcwidth(0x2648), 2, "♈ ARIES");
        assert_eq!(wcwidth(0x264F), 2, "♏ SCORPIUS");
        assert_eq!(wcwidth(0x2653), 2, "♓ PISCES");
        assert_eq!(wcwidth(0x267F), 2, "♿ WHEELCHAIR");
        assert_eq!(wcwidth(0x2693), 2, "⚓ ANCHOR");
        // Misc Symbols and Arrows
        assert_eq!(wcwidth(0x2B1B), 2, "⬛ BLACK LARGE SQUARE");
        assert_eq!(wcwidth(0x2B1C), 2, "⬜ WHITE LARGE SQUARE");
        assert_eq!(wcwidth(0x2B50), 2, "⭐ WHITE MEDIUM STAR");
        assert_eq!(wcwidth(0x2B55), 2, "⭕ HEAVY LARGE CIRCLE");
    }

    #[test]
    fn text_presentation_arrows_stay_narrow() {
        // §B.2 — these codepoints have Emoji_Presentation=No (default
        // text presentation). They become emoji only when followed by
        // VS16 (0xFE0F), which is handled by the grapheme cluster path
        // in `wcwidth_grapheme`, not by per-codepoint `wcwidth`. ASCII
        // tables / box-drawing diagrams that use ⬅⬆⬇ as 1-cell glyphs
        // would otherwise break.
        assert_eq!(wcwidth(0x2B05), 1, "⬅ default-text LEFTWARDS BLACK ARROW");
        assert_eq!(wcwidth(0x2B06), 1, "⬆ default-text UPWARDS BLACK ARROW");
        assert_eq!(wcwidth(0x2B07), 1, "⬇ default-text DOWNWARDS BLACK ARROW");
    }

    #[test]
    fn ascii_letters_and_box_drawing_stay_narrow() {
        // §B.4 sanity — the curated widening above must NOT touch
        // ASCII / box-drawing / common 1-cell text glyphs that ASCII-
        // art tables depend on. If a future "make all Emoji=Yes
        // codepoints wide" refactor lands, this test fires loud.
        assert_eq!(wcwidth(b'A' as u32), 1);
        assert_eq!(wcwidth(b'*' as u32), 1);
        assert_eq!(wcwidth(0x2500), 1, "─ BOX DRAWINGS LIGHT HORIZONTAL");
        assert_eq!(wcwidth(0x2502), 1, "│ BOX DRAWINGS LIGHT VERTICAL");
        assert_eq!(wcwidth(0x2514), 1, "└ BOX DRAWINGS LIGHT UP AND RIGHT");
        assert_eq!(wcwidth(0x276F), 1, "❯ HEAVY RIGHT-POINTING ANGLE QUOTATION MARK");
    }

    #[test]
    fn vs16_promotes_text_emoji_to_wide_via_cluster() {
        // §B.2 — sanity check that the VS16 (variation-selector-16)
        // cluster path lifts a default-text codepoint to width 2 so
        // ⬅️ / ❤️ (text presentation alone, emoji with VS16) render
        // at the correct visual width when emitted as a cluster.
        assert_eq!(wcwidth_grapheme("⬅\u{fe0f}"), 2);
        assert_eq!(wcwidth_grapheme("❤\u{fe0f}"), 2);
    }

    #[test]
    fn combining_is_zero() {
        assert_eq!(wcwidth(0x0301), 0); // combining acute
        assert_eq!(wcwidth(0x200b), 0); // zero-width space
        assert_eq!(wcwidth(0xfe0f), 0); // VS16
    }
}
