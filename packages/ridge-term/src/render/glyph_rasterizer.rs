//! Glyph rasterizer — Round 3 §4.1.b.
//!
//! ## Why OffscreenCanvas instead of fontdue / cosmic-text
//!
//! Two classes of glyph rasterizers exist for a wasm32 target:
//!
//! 1. **Pure-Rust crates** (`fontdue`, `ab_glyph`): take a TTF/OTF font
//!    file at construction, output bitmap glyphs. Pros: no browser
//!    coupling, predictable. Cons: must bundle a font asset (500 KB-2 MB
//!    even for a CJK-trimmed subset) and we'd need a fallback chain
//!    rasterizer (cosmic-text adds ~5 MB) to support Ridge's mixed
//!    ASCII + CJK + emoji output.
//!
//! 2. **Browser canvas-based** (`OffscreenCanvas` + `fillText`):
//!    delegates to the browser's font system. Pros: ~0 KB extra wasm,
//!    full system-font + user-configured fallback chain for free,
//!    matches what Canvas2dBackend already does so visual consistency
//!    is automatic. Cons: per-glyph rasterization is a sync browser
//!    call; not ideal but fine since the GlyphAtlas (§4.2) caches
//!    every result.
//!
//! Ridge picks (2). The atlas's LRU + the renderer's per-row dirty
//! hash mean glyphs get rasterized once per (font, size, ch) and
//! cached for the rest of the session.
//!
//! ## Pipeline
//!
//! `GlyphRasterizer::new(slot_w, slot_h)` creates an OffscreenCanvas
//! sized to a single glyph slot. `rasterize(font, size_px, ch)` calls
//! `set_font` + `fill_text` + `get_image_data`, yielding a
//! `RasterizedGlyph` with RGBA pixel bytes, advance width, and
//! ascent offset. The caller (future §4.1.c `WebGpuBackend::draw_row`
//! cache-miss path) uploads the bytes to a wgpu texture-array layer
//! and stores the layer index in the matching `GlyphEntry`.
//!
//! ## Status
//!
//! Slice 1: struct + new() construct an OffscreenCanvas; rasterize()
//! is a stub returning Err so the next iteration can fill in the
//! `set_font` / `fill_text` / `get_image_data` body without also
//! needing to debug surface acquisition. Cargo check --features webgpu
//! must compile cleanly with the slot allocation in place.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]
#![allow(dead_code)] // round-3 §4.1.b first slice; rasterize() body is stubbed.

use wasm_bindgen::{JsCast, JsValue};
use web_sys::{console, CanvasRenderingContext2d, HtmlCanvasElement};

use super::glyph_atlas::GlyphKey;

/// True iff `localStorage.RIDGE_DIAG === '1'` at call time. Used to gate
/// the emoji-rasterisation diagnostic log so users can capture a
/// per-glyph trace via `localStorage.RIDGE_DIAG='1'; location.reload()`
/// when investigating "emoji renders blank" reports without paying the
/// pixel-scan cost on the hot path. Cheap (3 JS calls + a string
/// compare) and emoji-codepoint-gated at the call site.
fn ridge_diag_enabled() -> bool {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("RIDGE_DIAG").ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Translate a style_flags bitset (matches `GlyphKey::STYLE_BOLD/ITALIC`)
/// into the leading CSS-font keyword used by the canvas 2D `font` shorthand.
/// `""` for plain, `"bold "`, `"italic "`, `"bold italic "`. Trailing space
/// included so callers can `format!("{prefix}{size}px {family}")`.
fn css_font_style_prefix(style_flags: u8) -> &'static str {
    let bold = style_flags & GlyphKey::STYLE_BOLD != 0;
    let italic = style_flags & GlyphKey::STYLE_ITALIC != 0;
    match (bold, italic) {
        (false, false) => "",
        (true, false) => "bold ",
        (false, true) => "italic ",
        (true, true) => "bold italic ",
    }
}

/// One rasterized glyph: pixel bytes + the metrics WebGpuBackend
/// needs to position the bitmap inside a cell box. Sized so a future
/// texture-array upload can `write_texture` directly with the rgba
/// slice + (width, height).
#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    /// Tightly-packed RGBA8 pixels, row-major, length == width * height * 4.
    pub rgba: Vec<u8>,
    /// Bitmap dimensions in device pixels (already multiplied by DPR).
    pub width: u16,
    pub height: u16,
    /// Horizontal advance in CSS pixels (post-DPR-divide). Renderer
    /// uses this to confirm a width-2 cell actually got a wide glyph.
    pub advance: f32,
    /// Vertical offset from cell top to glyph baseline, CSS pixels.
    pub ascent_offset: f32,
    /// True when the rasterized pixels carry the font's native color
    /// palette (color emoji from COLR / CPAL / sbix / SVG fonts) rather
    /// than the white-on-transparent coverage mask the rasterizer
    /// requested via `fillStyle = "#ffffff"`. The renderer uses this to
    /// stretch the cell quad to the full 2-cell width for wide emoji
    /// (their natural advance ≈ 1em is narrower than 2 latin cells and
    /// would otherwise leave a visible gap on the right).
    pub is_color: bool,
}

/// Browser-canvas-based glyph rasterizer.
///
/// Owns one DOM `HTMLCanvasElement` + 2D context, sized to fit any
/// single glyph the terminal ever asks for. Slot is intentionally
/// generous (square = max cell-height × 2) because the canvas backing
/// store is a one-time allocation per WebGpuBackend lifetime and
/// resizing it mid-session would clear the in-flight rendering state.
///
/// **Why a DOM canvas instead of `OffscreenCanvas`:** WebView2 / older
/// Chromium revisions resolve `measureText` against a permissive
/// font-fallback fast path but punt `fillText` to a stricter pipeline
/// that doesn't see system emoji fonts (Segoe UI Emoji) nor the
/// document's `@font-face`-declared faces (Noto Color Emoji). Symptom
/// captured 2026-05-08 via the §A.7 RIDGE_DIAG trace: every emoji
/// codepoint reported `advance_dev > 0` AND `ascent_dev == font_size`
/// (browser placeholder when no font matched) AND `non_zero_px == 0`
/// (zero pixels actually painted) — `fillText` silently no-op'd. A
/// detached-from-DOM `HTMLCanvasElement` exhibits the same gap on some
/// Chromium versions, but a canvas inserted into `document.body` does
/// inherit the document's full font-fallback chain reliably. We hide
/// it via inline `position:absolute; left:-9999px; visibility:hidden`
/// so layout / pointer-events don't see it.
pub struct GlyphRasterizer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    slot_w: u16,
    slot_h: u16,
}

impl GlyphRasterizer {
    /// Create a hidden DOM `<canvas>` of `slot_w × slot_h` device
    /// pixels + its 2D context. Returns Err if the browser can't
    /// supply a canvas or 2D rendering on it.
    pub fn new(slot_w: u16, slot_h: u16) -> Result<Self, String> {
        let document = web_sys::window()
            .ok_or_else(|| "GlyphRasterizer: no window".to_string())?
            .document()
            .ok_or_else(|| "GlyphRasterizer: no document".to_string())?;
        let canvas: HtmlCanvasElement = document
            .create_element("canvas")
            .map_err(|e| format!("GlyphRasterizer: createElement('canvas') threw: {e:?}"))?
            .dyn_into()
            .map_err(|_| "GlyphRasterizer: createElement returned non-canvas".to_string())?;
        canvas.set_width(slot_w as u32);
        canvas.set_height(slot_h as u32);
        // Hide the canvas off-screen and out of layout. Some
        // Chromium / WebView2 versions only resolve the document's
        // full font-fallback chain (system Segoe UI Emoji + the
        // @font-face Noto Color Emoji subsets) for canvases attached
        // to the document tree — switching here from OffscreenCanvas
        // was the §A.7 fix for "emoji rasterises blank" reports.
        canvas
            .set_attribute(
                "style",
                "position:absolute;left:-9999px;top:-9999px;width:0;height:0;visibility:hidden;pointer-events:none",
            )
            .ok();
        document
            .body()
            .ok_or_else(|| "GlyphRasterizer: no document.body".to_string())?
            .append_child(&canvas)
            .map_err(|e| format!("GlyphRasterizer: append_child failed: {e:?}"))?;
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| format!("GlyphRasterizer: get_context('2d') threw: {e:?}"))?
            .ok_or_else(|| "GlyphRasterizer: get_context('2d') returned None".to_string())?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "GlyphRasterizer: 2D context type mismatch".to_string())?;
        Ok(Self {
            canvas,
            ctx,
            slot_w,
            slot_h,
        })
    }

    /// Rasterize a single glyph to RGBA pixels via the browser's 2D
    /// canvas API. The pipeline:
    ///
    ///   1. Set the CSS font string. `font-size font-family` form —
    ///      family inherits the user's terminal font setting and the
    ///      browser supplies its full fallback chain.
    ///   2. Top-baseline so a glyph drawn at y=0 has its top edge
    ///      flush with the slot's top — simplifies the future shader
    ///      that places the bitmap at cell-top + ascent_offset.
    ///   3. Render in pure white so the shader can tint via the
    ///      cell's `fg_rgba` uniform without re-rasterizing per color.
    ///   4. Clear the slot before each glyph — `fill_text` paints
    ///      additively.
    ///   5. Paint the glyph at (0, 0). Returns Err if the browser
    ///      throws (rare; usually CORS-tainted fonts but our canvas
    ///      is internal so this should never fire).
    ///   6. Read back the entire slot as RGBA pixels. The atlas /
    ///      caller can crop to the actual glyph bounding box later
    ///      if texture-array memory pressure demands it.
    ///   7. Capture the horizontal advance from `measure_text` so
    ///      the renderer can validate width-2 cells got an
    ///      appropriately wide glyph.
    /// §4.7 (2026-05-07): `glyph_text` may be a single codepoint (the
    /// common path, ASCII / CJK / single emoji) OR a multi-codepoint
    /// extended grapheme cluster (👨‍👩‍👧, 🏳️‍🌈, 🇺🇸). The browser's
    /// `fill_text` natively handles ZWJ / variation selectors / RIS
    /// pairs when the font stack includes color-emoji fonts, so the
    /// rasterizer body is identical — only the input width is wider.
    pub fn rasterize(
        &self,
        font_family: &str,
        font_size_px: f32,
        dpr: f32,
        style_flags: u8,
        glyph_text: &str,
    ) -> Result<RasterizedGlyph, String> {
        // The OffscreenCanvas backing store is `slot_w × slot_h` DEVICE
        // pixels. Painting at `{font_size_px}px` (CSS px) without DPR
        // scaling left the glyph occupying only `font_size / dpr` of
        // those device pixels — visibly tiny / thin on HiDPI. Render
        // at `font_size_px * dpr` so the glyph fills DPR-scaled pixels.
        let dpr_eff = if dpr.is_finite() && dpr > 0.0 {
            dpr
        } else {
            1.0
        };
        let device_size_px = (font_size_px * dpr_eff).max(1.0);

        // Build CSS font string with optional `bold ` / `italic ` prefix
        // so the browser actually applies the SGR weight/slant. Without
        // this, BOLD cells get a separate atlas slot (per GlyphKey
        // discriminator) but the painted bitmap is identical to plain —
        // visible weight loss + cache thrash.
        let style_prefix = css_font_style_prefix(style_flags);
        let font_css = format!("{}{}px {}", style_prefix, device_size_px, font_family);
        self.ctx.set_font(&font_css);
        // Use the alphabetic baseline (default) and explicitly position it
        // at `ascent_dev` below the slot top. With `text_baseline = "top"`
        // the EM-box top is at y=0, but `font_bounding_box_ascent` may
        // exceed the EM-box ascent — diacriticals or extreme caps in some
        // fonts then extend ABOVE y=0 and get clipped at the slot top.
        // Anchoring on the alphabetic baseline `ascent_dev` below y=0
        // gives every glyph the full ascent room measureText reported,
        // so the top of any rendered glyph stays inside [0, ascent_dev].
        // (Caught 2026-05-05 from a user report that the top of glyphs
        // looked clipped under WebGPU.)
        self.ctx.set_text_baseline("alphabetic");
        self.ctx.set_fill_style_str("#ffffff");

        let slot_w = self.slot_w as f64;
        let slot_h = self.slot_h as f64;
        self.ctx.clear_rect(0.0, 0.0, slot_w, slot_h);

        // Measure first so we know where to place the baseline. We only
        // need the font-wide ascent here; per-glyph actualBoundingBox
        // is derived after the fill_text call below.
        let metrics = self
            .ctx
            .measure_text(glyph_text)
            .map_err(|e| format!("GlyphRasterizer::measure_text: {e:?}"))?;
        let advance_dev = metrics.width() as f32;
        let advance = advance_dev / dpr_eff;
        let ascent_dev = metrics.font_bounding_box_ascent() as f32;
        let descent_dev = metrics.font_bounding_box_descent() as f32;
        let bbox_h_dev = if ascent_dev > 0.0 && descent_dev > 0.0 {
            ascent_dev + descent_dev
        } else {
            // Browser hasn't populated bbox metrics (rare cold-start).
            // 1.2× line-height matches measure() above + Canvas2dBackend.
            device_size_px * 1.2
        };
        // Baseline y inside the slot. With `text_baseline = "alphabetic"`,
        // `fill_text(text, x, y)` positions the alphabetic baseline at y.
        // Falling back to `device_size_px * 0.8` when the font hasn't
        // populated bbox metrics yet (≈ typical ascent ratio).
        let baseline_y = if ascent_dev > 0.0 {
            ascent_dev as f64
        } else {
            (device_size_px * 0.8) as f64
        };

        self.ctx
            .fill_text(glyph_text, 0.0, baseline_y)
            .map_err(|e| format!("GlyphRasterizer::fill_text: {e:?}"))?;

        let image_data = self
            .ctx
            .get_image_data(0.0, 0.0, slot_w, slot_h)
            .map_err(|e| format!("GlyphRasterizer::get_image_data: {e:?}"))?;
        let rgba: Vec<u8> = image_data.data().to_vec();

        // Device-pixel bounding box of the painted glyph, clamped to
        // slot. The caller crops `atlas_uv` to this rectangle so the
        // cell quad samples only the rendered glyph instead of the
        // entire mostly-empty slot.
        let bbox_w = advance_dev.ceil().clamp(1.0, self.slot_w as f32) as u16;
        let bbox_h = bbox_h_dev.ceil().clamp(1.0, self.slot_h as f32) as u16;

        // Detect whether the browser stamped a color-emoji palette into
        // RGB, or honoured the white fillStyle (monochrome glyph). Scan
        // pixels with non-trivial alpha; if any has at least one channel
        // below ~0.98 (= 250/255) the glyph carries native color. We
        // bail on first hit — typical color emoji has thousands of color
        // pixels so the loop costs ~10s of bytes in practice. Threshold
        // 250 matches the shader's 0.99 cutoff (decoded sRGB white at
        // boundary AA pixels lands ~0.992 = 253/255).
        let mut is_color = false;
        let mut i = 0usize;
        while i + 3 < rgba.len() {
            let a = rgba[i + 3];
            if a >= 8 {
                let r = rgba[i];
                let g = rgba[i + 1];
                let b = rgba[i + 2];
                if r < 250 || g < 250 || b < 250 {
                    is_color = true;
                    break;
                }
            }
            i += 4;
        }

        // Emoji-rasterisation diagnostic. Gated on (a) leading codepoint
        // ≥ U+2600 (covers Dingbats + every SMP emoji block, skips ASCII
        // / CJK / Latin extension hot paths) and (b) RIDGE_DIAG=1, so
        // normal users pay nothing. When user reports "emoji blank", the
        // emitted line tells us in one shot whether the rasterizer
        // received any pixels at all (non_zero_px), whether color was
        // detected (is_color), and which font_css the OffscreenCanvas
        // resolved against — three orthogonal failure modes.
        let first_cp = glyph_text.chars().next().map(|c| c as u32).unwrap_or(0);
        if first_cp >= 0x2600 && ridge_diag_enabled() {
            let total_px = rgba.len() / 4;
            let non_zero_px = rgba.chunks_exact(4).filter(|p| p[3] >= 8).count();
            let msg = format!(
                "[noto-emoji-diag] cp=U+{first_cp:04X} font=\"{font_css}\" \
                 advance_dev={advance_dev:.2} ascent_dev={ascent_dev:.2} \
                 bbox=({bbox_w}x{bbox_h}) is_color={is_color} \
                 non_zero_px={non_zero_px}/{total_px}"
            );
            console::log_1(&JsValue::from_str(&msg));
        }

        Ok(RasterizedGlyph {
            rgba,
            // Was: `self.slot_w / self.slot_h`. That was wrong per the
            // field's documented contract ("Bitmap dimensions in device
            // pixels"); it produced a [0,0,1,1] sample over the whole
            // 32×32 slot and shrank the glyph into a corner of the cell.
            width: bbox_w,
            height: bbox_h,
            advance,
            // `set_text_baseline("top")` plus `fill_text(_, 0.0, 0.0)`
            // means the glyph's top edge sits at y=0, so the offset
            // from cell-top to glyph-top is 0. A future slice that
            // wants pixel-perfect baseline alignment can re-derive
            // this from `metrics.font_bounding_box_ascent()`.
            ascent_offset: 0.0,
            is_color,
        })
    }

    pub fn slot_dimensions(&self) -> (u16, u16) {
        (self.slot_w, self.slot_h)
    }

    /// Measure the cell metrics (cell_w, cell_h) for the given font.
    ///
    /// Mirrors `Canvas2dBackend::measure_font` bit-for-bit so the
    /// WebGPU and Canvas2D paths produce identical cellW/cellH numbers
    /// for the same (family, size_px) — fitPane stays backend-agnostic.
    ///
    /// Algorithm (matches `Canvas2dBackend`):
    /// - `cell_w = advance('M')` rounded to int CSS px (≥ 1).
    /// - `cell_h = font_bounding_box_ascent + font_bounding_box_descent`
    ///   when both are available; falls back to `font_size_px * 1.2`
    ///   if the browser returns zeros (rare; some systems on first
    ///   measurement before font is loaded). Rounded to int (≥ 1).
    ///
    /// Sub-pixel cell sizes cause boundary-alignment issues documented
    /// in `canvas2d.rs`'s module header — rounding here is load-bearing.
    pub fn measure(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        let font_css = format!("{}px {}", font_size_px, font_family);
        self.ctx.set_font(&font_css);
        self.ctx.set_text_baseline("top");
        let metrics = self
            .ctx
            .measure_text("M")
            .map_err(|e| format!("GlyphRasterizer::measure: measure_text threw: {e:?}"))?;
        let w = metrics.width() as f32;
        let ascent = metrics.font_bounding_box_ascent() as f32;
        let descent = metrics.font_bounding_box_descent() as f32;
        let h = if ascent > 0.0 && descent > 0.0 {
            ascent + descent
        } else {
            font_size_px * 1.2
        };
        Ok((w.round().max(1.0), h.round().max(1.0)))
    }
}
