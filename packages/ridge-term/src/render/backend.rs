//! Rendering backend abstraction.
//!
//! ## Design philosophy
//!
//! The trait is **terminal-shaped, not GPU-shaped**. A backend gets told:
//! "draw cell at (row, col) with this glyph + fg + bg + flags". How the
//! backend turns that into pixels is its own business.
//!
//! Two backends will implement this:
//! - `Canvas2dBackend` (round 2.2, this round): correctness oracle, slow but obviously right
//! - `WebGpuBackend` (round 3): performance, validated against Canvas2D output
//!
//! ## Two-pass draw
//!
//! `draw_frame` is implemented in this module (default impl) as two passes:
//!   1. Clear the entire viewport with the theme's background
//!   2. For each cell, paint its bg rectangle if it differs from theme bg
//!   3. For each cell, paint its glyph
//!   4. Paint the cursor (if visible)
//!
//! Why two passes: Canvas2D's `fillText` produces anti-aliased glyph edges
//! that can be partially over the *next* cell's pixels. If we draw each
//! cell's bg+glyph together, the next cell's bg will overwrite the previous
//! cell's anti-aliased trailing pixels. Two passes (all bgs first, all
//! glyphs after) avoids this. WebGPU has the same constraint with
//! sub-pixel-positioned SDF rendering, so the discipline transfers.
//!
//! ## Dirty rows
//!
//! Rather than re-rendering the full grid every frame, the renderer keeps
//! a snapshot of the last drawn state and computes a per-row dirty bit
//! before the draw. The backend doesn't see this — it just gets a list
//! of rows to redraw plus the data for those rows.

use crate::term::attrs::Attrs;
use crate::term::cell::{Cell, ClusterSpan};

/// A single row of cells handed to the backend, plus its grid row index.
/// §4.7 (2026-05-07): `clusters` carries the row's grapheme-cluster
/// overrides so backends can paint multi-codepoint clusters (👨‍👩‍👧,
/// 🏳️‍🌈, 🇺🇸) as a single visual unit instead of just the first
/// codepoint stored in `cells[col].ch`. Empty for the common case.
pub struct RowDraw<'a> {
    pub row_index: usize,
    pub cells: &'a [Cell],
    pub clusters: &'a [ClusterSpan],
}

/// Cursor descriptor passed each frame. `None` = don't draw cursor
/// (DECTCEM off, blink off-phase, terminal blurred, etc.).
#[derive(Debug, Clone, Copy)]
pub struct CursorDraw {
    pub row: usize,
    pub col: usize,
    pub style: CursorStyle,
    /// When the cursor box would overlap a cell with a glyph, the renderer
    /// needs the cell's character so it can paint it on top of the inverse-
    /// colored cursor block. Carried inline to avoid a second grid lookup
    /// inside the backend.
    pub ch: char,
    pub ch_attr: crate::term::attr_table::AttrId,
    pub width: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    /// Filled rectangle covering the cell. Default and most common.
    Block,
    /// Vertical bar at the left edge. Used by some shells in insert mode.
    Bar,
    /// Underline under the bottom of the cell.
    Underline,
}

/// Theme colors. Backend reads this to resolve `Color::Default` and
/// indexed colors. Index 0..16 is ANSI/bright, 16..232 is the 6×6×6 cube,
/// 232..256 is the gray ramp — matching xterm's xterm-256color palette.
#[derive(Debug, Clone)]
pub struct Theme {
    pub bg: [u8; 4],
    pub fg: [u8; 4],
    pub cursor_color: [u8; 4],
    /// Color used to draw the glyph **inside** the cursor block (cursor
    /// inverts fg/bg by default). Often equals `bg`.
    pub cursor_text_color: [u8; 4],
    /// Translucent overlay color painted on top of selected cells. ~37%
    /// alpha keeps the glyphs underneath readable. Override via theme key
    /// `selectionBackground`.
    pub selection_bg: [u8; 4],
    /// Color of the underline drawn beneath OSC 8 hyperlink spans.
    /// Override via theme key `hyperlinkColor`. Solid by default — links
    /// should be obviously different from regular underlined text.
    pub hyperlink_color: [u8; 4],
    /// 256-entry palette: ANSI 0..15 + 6×6×6 cube + 24-step gray ramp.
    pub palette: [[u8; 4]; 256],
}

impl Theme {
    /// xterm default dark palette. Used as fallback when JS hasn't set a theme.
    pub fn default_dark() -> Self {
        Self {
            bg: [0x07, 0x10, 0x09, 0xff],
            fg: [0xc8, 0xe8, 0xd4, 0xff],
            cursor_color: [0x36, 0xc2, 0x6e, 0xff],
            cursor_text_color: [0x07, 0x10, 0x09, 0xff],
            selection_bg: [0x55, 0xaa, 0xff, 0x60],
            hyperlink_color: [0x66, 0xb3, 0xff, 0xff],
            palette: build_xterm_palette(),
        }
    }

    /// Apply xterm-style theme overrides from a name → "#rrggbb" or
    /// "#rrggbbaa" map. Keys not recognized are ignored. Keys absent from
    /// the input leave the existing palette entry alone — so callers can
    /// pass partial themes (e.g. just `{ background, foreground, accent }`)
    /// without losing the ANSI colors.
    ///
    /// Recognized keys (mirrors xterm.js ITheme):
    ///   background, foreground, cursor, cursorAccent
    ///   black, red, green, yellow, blue, magenta, cyan, white
    ///   brightBlack, brightRed, brightGreen, brightYellow,
    ///   brightBlue, brightMagenta, brightCyan, brightWhite
    pub fn apply_partial(&mut self, get: impl Fn(&str) -> Option<String>) {
        if let Some(c) = get("background").and_then(|s| parse_hex_color(&s)) {
            self.bg = c;
            // Cursor text color defaults to bg when the user changes bg
            // and didn't set cursorAccent explicitly. Match xterm behavior.
            self.cursor_text_color = c;
        }
        if let Some(c) = get("foreground").and_then(|s| parse_hex_color(&s)) {
            self.fg = c;
        }
        if let Some(c) = get("cursor").and_then(|s| parse_hex_color(&s)) {
            self.cursor_color = c;
        }
        if let Some(c) = get("cursorAccent").and_then(|s| parse_hex_color(&s)) {
            self.cursor_text_color = c;
        }
        if let Some(c) = get("selectionBackground").and_then(|s| parse_hex_color(&s)) {
            self.selection_bg = c;
        }
        if let Some(c) = get("hyperlinkColor").and_then(|s| parse_hex_color(&s)) {
            self.hyperlink_color = c;
        }

        // ANSI 16 — order matches palette indices.
        let ansi_keys = [
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "white",
            "brightBlack",
            "brightRed",
            "brightGreen",
            "brightYellow",
            "brightBlue",
            "brightMagenta",
            "brightCyan",
            "brightWhite",
        ];
        for (i, name) in ansi_keys.iter().enumerate() {
            if let Some(c) = get(name).and_then(|s| parse_hex_color(&s)) {
                self.palette[i] = c;
            }
        }
    }

    /// Resolve a `Color` to an RGBA tuple.
    pub fn resolve(&self, c: crate::term::attrs::Color, is_fg: bool) -> [u8; 4] {
        use crate::term::attrs::ColorKind;
        match c.kind() {
            ColorKind::Default => {
                if is_fg {
                    self.fg
                } else {
                    self.bg
                }
            }
            ColorKind::Indexed(i) => self.palette[i as usize],
            ColorKind::Rgb(r, g, b) => [r, g, b, 0xff],
        }
    }
}

/// Build the standard xterm 256-color palette. ANSI 0..15 are the dark
/// slot defaults; index 16..256 is generated procedurally per spec.
fn build_xterm_palette() -> [[u8; 4]; 256] {
    let mut pal = [[0u8, 0, 0, 0xff]; 256];

    // ANSI 0..7 (dim) — xterm defaults
    let ansi = [
        [0x00, 0x00, 0x00, 0xff], // black
        [0xcd, 0x00, 0x00, 0xff], // red
        [0x00, 0xcd, 0x00, 0xff], // green
        [0xcd, 0xcd, 0x00, 0xff], // yellow
        [0x00, 0x00, 0xee, 0xff], // blue
        [0xcd, 0x00, 0xcd, 0xff], // magenta
        [0x00, 0xcd, 0xcd, 0xff], // cyan
        [0xe5, 0xe5, 0xe5, 0xff], // white
    ];
    let bright = [
        [0x7f, 0x7f, 0x7f, 0xff], // bright black
        [0xff, 0x00, 0x00, 0xff], // bright red
        [0x00, 0xff, 0x00, 0xff], // bright green
        [0xff, 0xff, 0x00, 0xff], // bright yellow
        [0x5c, 0x5c, 0xff, 0xff], // bright blue
        [0xff, 0x00, 0xff, 0xff], // bright magenta
        [0x00, 0xff, 0xff, 0xff], // bright cyan
        [0xff, 0xff, 0xff, 0xff], // bright white
    ];
    for i in 0..8 {
        pal[i] = ansi[i];
    }
    for i in 0..8 {
        pal[i + 8] = bright[i];
    }

    // 16..232: 6×6×6 cube. Per xterm: each axis uses values 0, 95, 135,
    // 175, 215, 255 (NOT a uniform 51-step ramp, which is a common bug).
    let levels = [0u8, 95, 135, 175, 215, 255];
    for r in 0..6 {
        for g in 0..6 {
            for b in 0..6 {
                let idx = 16 + 36 * r + 6 * g + b;
                pal[idx] = [levels[r], levels[g], levels[b], 0xff];
            }
        }
    }

    // 232..256: 24-step gray ramp from 8 to 238.
    for i in 0..24 {
        let v = 8 + 10 * i as u8;
        pal[232 + i] = [v, v, v, 0xff];
    }

    pal
}

/// Per-frame dimensions. The backend uses these to position cells.
#[derive(Debug, Clone, Copy)]
pub struct FrameMetrics {
    /// Cell width in CSS pixels (not device pixels).
    pub cell_w: f32,
    /// Cell height in CSS pixels.
    pub cell_h: f32,
    /// Pixel ratio (device_pixel_ratio). Backend multiplies internally
    /// when writing to the actual surface.
    pub dpr: f32,
}

/// What a backend must implement. Methods are called in this order each frame:
///   1. begin_frame(metrics, theme)
///   2. clear()
///   3. draw_row(...) for each dirty row
///   4. draw_cursor(...) (if cursor visible)
///   5. end_frame()
///
/// Backends may freely cache between frames (e.g. font handles, vertex
/// buffers); they're called from the same thread and don't need to be Sync.
pub trait RenderBackend {
    /// Called once when the surface is created. Returns measured cell
    /// dimensions for the requested font + size, in CSS pixels.
    /// `font_family` is a CSS font-family string, including fallbacks.
    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String>;

    /// Whether this backend cannot preserve content across frames and
    /// therefore needs the renderer to mark every visible row dirty
    /// every tick (full redraw).
    ///
    /// Canvas2D returns `false` (default): un-touched rows keep their
    /// previous frame's pixels because we only `fillRect`/`fillText`
    /// where dirty.
    /// WebGPU returns `true`: `LoadOp::Clear` wipes the entire swap-chain
    /// texture each frame; without forcing every row through `draw_row`,
    /// non-dirty rows lose their glyphs and the user sees only the row
    /// they're typing on.
    fn requires_full_frame(&self) -> bool {
        false
    }

    /// Resize the underlying surface. Called when canvas size or DPR changes.
    fn resize_surface(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), String>;

    /// Drop any cached glyph state that becomes stale when cell metrics
    /// change (DPR change, font size change, font family change). Default
    /// is a no-op — Canvas2D rasterizes per-frame and has no state to
    /// drop. WebGPU clears its `GlyphAtlas` LRU and resets the next-free
    /// texture-array layer pointer so the next frame re-rasterizes
    /// against the new metrics instead of pointing the shader at stale
    /// UVs left over from the previous size.
    ///
    /// Called from `Renderer::invalidate_all` so the atlas can never lag
    /// the renderer-side invalidation.
    fn invalidate_atlas(&mut self) {}

    /// Notify the backend that the next frame will be a full redraw and
    /// that any per-frame preserved-content optimization needs to seed a
    /// fresh background. Default no-op — Canvas2D draws bg+glyph
    /// directly per dirty row and doesn't need to know. WebGPU flips
    /// `needs_initial_clear` so `end_frame` switches back to
    /// `LoadOp::Clear` for that one frame; subsequent frames return to
    /// `LoadOp::Load` + row-dirty diff. Called by `Renderer::tick`
    /// whenever `full_redraw_pending` becomes true (first frame, scroll
    /// offset change, selection toggle, snapshot growth).
    fn on_full_invalidate(&mut self) {}

    /// Begin a frame — record metrics + theme for this draw cycle.
    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme);

    /// Wipe the viewport with `theme.bg`. Called once per frame after begin_frame.
    fn clear(&mut self);

    /// Draw one row's cells. The backend should:
    ///   1. Paint each cell's bg (skip cells whose bg matches theme.bg
    ///      to save fill calls — caller doesn't optimize this).
    ///   2. Paint each cell's glyph.
    /// `attrs_table` resolves `AttrId` to colors and flags.
    fn draw_row(&mut self, row: &RowDraw<'_>, attrs_table: &crate::term::attr_table::AttrTable);

    /// Draw the cursor on top of any existing cell content. Coordinates
    /// are in cell units (0..cols, 0..rows).
    fn draw_cursor(
        &mut self,
        cursor: &CursorDraw,
        attrs_table: &crate::term::attr_table::AttrTable,
    );

    /// Paint a translucent overlay over the selected cell ranges using
    /// `theme.selection_bg`. Each tuple is `(row, col_start, col_end)` —
    /// `col_end` exclusive. Caller is responsible for normalizing the
    /// list per-row; backend just paints. No-op when `rects` is empty.
    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]);

    /// Draw a 1-cell-wide underline beneath each (row, col_start, col_end)
    /// hyperlink range using `theme.hyperlink_color`. No-op when empty.
    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]);

    /// Commit the frame to the surface. For Canvas2D this is a no-op
    /// (drawing was already direct); for WebGPU this submits the command
    /// encoder.
    fn end_frame(&mut self);
}

/// Default implementation that drives a backend through one frame using
/// a list of dirty rows + cursor. Renderer state (snapshot, dirty calc)
/// lives in `Renderer` (renderer.rs).
pub fn draw_frame<B: RenderBackend>(
    backend: &mut B,
    metrics: FrameMetrics,
    theme: &Theme,
    rows: &[RowDraw<'_>],
    cursor: Option<&CursorDraw>,
    attrs_table: &crate::term::attr_table::AttrTable,
    full_redraw: bool,
    selection_rects: &[(usize, usize, usize)],
    hyperlink_rects: &[(usize, usize, usize)],
) {
    backend.begin_frame(metrics, theme);
    if full_redraw {
        backend.clear();
    }
    for row in rows {
        backend.draw_row(row, attrs_table);
    }
    // Hyperlink underlines under row content but above row bg — drawn
    // first so the selection overlay (next) can dim them slightly.
    if !hyperlink_rects.is_empty() {
        backend.draw_hyperlink_underlines(hyperlink_rects);
    }
    // Selection overlay sits between row content and cursor — translucent
    // so glyphs underneath remain readable; cursor on top so it's still
    // distinguishable inside a selected region.
    if !selection_rects.is_empty() {
        backend.draw_selection_overlay(selection_rects);
    }
    if let Some(cur) = cursor {
        backend.draw_cursor(cur, attrs_table);
    }
    backend.end_frame();
}

/// Convenience to build attrs given an interned id + table + theme,
/// also handling the inverse / hidden flags. Used by both backends.
pub fn resolve_cell_colors(
    cell: &Cell,
    attrs_table: &crate::term::attr_table::AttrTable,
    theme: &Theme,
) -> (Attrs, [u8; 4], [u8; 4]) {
    let attrs = attrs_table.get(cell.attr);
    let mut fg = theme.resolve(attrs.fg, true);
    let mut bg = theme.resolve(attrs.bg, false);

    use crate::term::attrs::Flags;

    if attrs.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if attrs.flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    // SGR 2 (DIM / faint) intentionally NOT rendered. Earlier iterations
    // tried `(fg + bg) / 2` (50% wash toward bg) and `fg * 0.7`
    // (Alacritty-style luminance cut); both produced visible greyed-out
    // segments in PSReadLine / oh-my-posh prompts that the user perceived
    // as "前景色污染" — especially noticeable when an IME composing
    // overlay collapsed and revealed dim cells underneath. Disabling the
    // attribute matches Windows Terminal's default behaviour and removes
    // the ambiguity entirely. The DIM bit is still parsed and held in
    // the attr table (so SGR 0 / 22 reset semantics stay correct); we
    // just don't translate it to a color change at draw time. BOLD
    // (SGR 1) and the rest of the SGR set are unaffected.
    let _ = Flags::DIM;

    (attrs, fg, bg)
}

/// Parse "#rgb" / "#rrggbb" / "#rrggbbaa" into [r,g,b,a]. Returns None on
/// malformed input so the caller (e.g. `Theme::apply_partial`) can simply
/// leave the existing palette entry alone.
fn parse_hex_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    let bytes: Vec<u8> = match hex.len() {
        3 => {
            // #rgb — duplicate each nibble.
            let chs: Vec<char> = hex.chars().collect();
            let r = u8::from_str_radix(&format!("{}{}", chs[0], chs[0]), 16).ok()?;
            let g = u8::from_str_radix(&format!("{}{}", chs[1], chs[1]), 16).ok()?;
            let b = u8::from_str_radix(&format!("{}{}", chs[2], chs[2]), 16).ok()?;
            return Some([r, g, b, 0xff]);
        }
        6 => (0..3)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?,
        8 => (0..4)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };
    if bytes.len() == 3 {
        Some([bytes[0], bytes[1], bytes[2], 0xff])
    } else {
        Some([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_palette_indices_match_spec() {
        let pal = build_xterm_palette();
        // Index 16 is pure black in the cube
        assert_eq!(pal[16], [0, 0, 0, 0xff]);
        // Index 196 = (5,0,0) = 255,0,0 — pure red corner of cube
        assert_eq!(pal[196], [255, 0, 0, 0xff]);
        // Index 231 = (5,5,5) = 255,255,255 — white corner
        assert_eq!(pal[231], [255, 255, 255, 0xff]);
        // Index 232 = first gray = 8,8,8
        assert_eq!(pal[232], [8, 8, 8, 0xff]);
        // Index 255 = last gray = 238
        assert_eq!(pal[255], [238, 238, 238, 0xff]);
    }

    #[test]
    fn theme_resolve_default_uses_theme_fg_bg() {
        let theme = Theme::default_dark();
        use crate::term::attrs::Color;
        assert_eq!(theme.resolve(Color::DEFAULT, true), theme.fg);
        assert_eq!(theme.resolve(Color::DEFAULT, false), theme.bg);
    }

    #[test]
    fn theme_resolve_indexed_uses_palette() {
        let theme = Theme::default_dark();
        use crate::term::attrs::Color;
        // Index 1 = ANSI red = (0xcd, 0, 0)
        assert_eq!(theme.resolve(Color::indexed(1), true), [0xcd, 0, 0, 0xff]);
        // Index 196 = pure red in cube
        assert_eq!(theme.resolve(Color::indexed(196), true), [255, 0, 0, 0xff]);
    }

    #[test]
    fn parse_hex_handles_3_6_8() {
        assert_eq!(parse_hex_color("#fff"), Some([0xff, 0xff, 0xff, 0xff]));
        assert_eq!(parse_hex_color("#aabbcc"), Some([0xaa, 0xbb, 0xcc, 0xff]));
        assert_eq!(parse_hex_color("#aabbcc80"), Some([0xaa, 0xbb, 0xcc, 0x80]));
        assert_eq!(parse_hex_color("aabbcc"), Some([0xaa, 0xbb, 0xcc, 0xff]));
        assert_eq!(parse_hex_color("#zzz"), None);
        assert_eq!(parse_hex_color("#abcd"), None); // 4-digit form not supported
    }

    #[test]
    fn theme_apply_partial_overrides_only_provided_keys() {
        use std::collections::HashMap;
        let mut t = Theme::default_dark();
        let original_red = t.palette[1];

        let mut m = HashMap::new();
        m.insert("background".to_string(), "#000000".to_string());
        m.insert("foreground".to_string(), "#ffffff".to_string());
        // Don't override red.
        t.apply_partial(|k| m.get(k).cloned());

        assert_eq!(t.bg, [0, 0, 0, 0xff]);
        assert_eq!(t.fg, [0xff, 0xff, 0xff, 0xff]);
        // ANSI red unchanged.
        assert_eq!(t.palette[1], original_red);
    }

    // ─── parse_hex_color edge cases ───────────────────────────────────

    #[test]
    fn parse_hex_color_rejects_empty_and_single_char() {
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#"), None);
        assert_eq!(parse_hex_color("a"), None);
        assert_eq!(parse_hex_color("#a"), None);
    }

    #[test]
    fn parse_hex_color_rejects_odd_lengths_5_and_7() {
        assert_eq!(parse_hex_color("#abcde"), None);
        assert_eq!(parse_hex_color("#abcdefa"), None);
    }

    #[test]
    fn parse_hex_color_accepts_uppercase_and_mixed() {
        assert_eq!(parse_hex_color("#FFAABB"), Some([0xff, 0xaa, 0xbb, 0xff]));
        assert_eq!(parse_hex_color("#fFaAbB"), Some([0xff, 0xaa, 0xbb, 0xff]));
    }

    #[test]
    fn parse_hex_color_accepts_zero_and_max_components() {
        assert_eq!(parse_hex_color("#000000"), Some([0x00, 0x00, 0x00, 0xff]));
        assert_eq!(parse_hex_color("#ffffff"), Some([0xff, 0xff, 0xff, 0xff]));
        assert_eq!(parse_hex_color("#00000000"), Some([0x00, 0x00, 0x00, 0x00]));
    }

    // ─── Theme::resolve ───────────────────────────────────────────────

    #[test]
    fn theme_resolve_default_fg_returns_theme_fg() {
        use crate::term::attrs::Color;
        let t = Theme::default_dark();
        let resolved = t.resolve(Color::DEFAULT, true);
        assert_eq!(resolved, t.fg);
    }

    #[test]
    fn theme_resolve_default_bg_returns_theme_bg() {
        use crate::term::attrs::Color;
        let t = Theme::default_dark();
        let resolved = t.resolve(Color::DEFAULT, false);
        assert_eq!(resolved, t.bg);
    }

    #[test]
    fn theme_resolve_indexed_returns_palette_entry() {
        use crate::term::attrs::Color;
        let t = Theme::default_dark();
        // ANSI 1 (red) — palette[1] is what xterm calls red.
        let resolved = t.resolve(Color::indexed(1), true);
        assert_eq!(resolved, t.palette[1]);

        // 256-color cube entry.
        let resolved_cube = t.resolve(Color::indexed(196), true);
        assert_eq!(resolved_cube, t.palette[196]);
    }

    #[test]
    fn theme_resolve_rgb_returns_literal_with_alpha_ff() {
        use crate::term::attrs::Color;
        let t = Theme::default_dark();
        let resolved = t.resolve(Color::rgb(0x12, 0x34, 0x56), true);
        // RGB resolution always sets alpha = 0xff (24-bit truecolor has no alpha channel).
        assert_eq!(resolved, [0x12, 0x34, 0x56, 0xff]);
    }

    // ─── Theme::apply_partial — coverage gaps ─────────────────────────

    #[test]
    fn theme_apply_partial_background_drives_cursor_text_color() {
        // Documented behavior (line 121-123): when bg changes and the
        // user did NOT set cursorAccent, cursor_text_color tracks bg
        // so the cursor's glyph stays legible against the new bg.
        use std::collections::HashMap;
        let mut t = Theme::default_dark();
        let mut m = HashMap::new();
        m.insert("background".to_string(), "#102030".to_string());
        t.apply_partial(|k| m.get(k).cloned());
        assert_eq!(t.bg, [0x10, 0x20, 0x30, 0xff]);
        assert_eq!(t.cursor_text_color, [0x10, 0x20, 0x30, 0xff]);
    }

    #[test]
    fn theme_apply_partial_cursor_accent_overrides_bg_default() {
        // Explicit cursorAccent wins over the bg-derived default.
        use std::collections::HashMap;
        let mut t = Theme::default_dark();
        let mut m = HashMap::new();
        m.insert("background".to_string(), "#102030".to_string());
        m.insert("cursorAccent".to_string(), "#ffeedd".to_string());
        t.apply_partial(|k| m.get(k).cloned());
        assert_eq!(t.bg, [0x10, 0x20, 0x30, 0xff]);
        assert_eq!(t.cursor_text_color, [0xff, 0xee, 0xdd, 0xff]);
    }

    #[test]
    fn theme_apply_partial_selection_and_hyperlink_routes() {
        use std::collections::HashMap;
        let mut t = Theme::default_dark();
        let mut m = HashMap::new();
        m.insert("selectionBackground".to_string(), "#88aabbcc".to_string());
        m.insert("hyperlinkColor".to_string(), "#ff5500".to_string());
        t.apply_partial(|k| m.get(k).cloned());
        assert_eq!(t.selection_bg, [0x88, 0xaa, 0xbb, 0xcc]);
        assert_eq!(t.hyperlink_color, [0xff, 0x55, 0x00, 0xff]);
    }

    #[test]
    fn theme_apply_partial_all_16_ansi_keys_route_to_palette() {
        // Verify every named ANSI slot routes to its expected palette index.
        use std::collections::HashMap;
        let names = [
            "black",
            "red",
            "green",
            "yellow",
            "blue",
            "magenta",
            "cyan",
            "white",
            "brightBlack",
            "brightRed",
            "brightGreen",
            "brightYellow",
            "brightBlue",
            "brightMagenta",
            "brightCyan",
            "brightWhite",
        ];
        for (idx, name) in names.iter().enumerate() {
            let mut t = Theme::default_dark();
            let mut m = HashMap::new();
            // Use the index as the red byte so each slot's payload is unique.
            let payload = format!("#{:02x}0000", idx);
            m.insert(name.to_string(), payload);
            t.apply_partial(|k| m.get(k).cloned());
            assert_eq!(
                t.palette[idx],
                [idx as u8, 0, 0, 0xff],
                "key {} should route to palette[{}]",
                name,
                idx,
            );
        }
    }

    #[test]
    fn theme_apply_partial_invalid_color_strings_dont_clobber() {
        // Garbage in → entry unchanged. (Verifies parse_hex_color None
        // path doesn't accidentally write a default-zero color.)
        use std::collections::HashMap;
        let mut t = Theme::default_dark();
        let original_bg = t.bg;
        let original_red = t.palette[1];
        let mut m = HashMap::new();
        m.insert("background".to_string(), "not-a-color".to_string());
        m.insert("red".to_string(), "#zz".to_string());
        t.apply_partial(|k| m.get(k).cloned());
        assert_eq!(t.bg, original_bg);
        assert_eq!(t.palette[1], original_red);
    }
}
