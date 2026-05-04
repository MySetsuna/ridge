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

use wasm_bindgen::JsCast;
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d};

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
}

/// Browser-canvas-based glyph rasterizer.
///
/// Owns one OffscreenCanvas + 2D context, sized to fit any single
/// glyph the terminal ever asks for. Slot is intentionally generous
/// (square = max cell-height × 2) because the OffscreenCanvas backing
/// store is a one-time allocation per WebGpuBackend lifetime and
/// resizing it mid-session would clear the in-flight rendering state.
pub struct GlyphRasterizer {
    canvas: OffscreenCanvas,
    ctx: OffscreenCanvasRenderingContext2d,
    slot_w: u16,
    slot_h: u16,
}

impl GlyphRasterizer {
    /// Create an OffscreenCanvas of `slot_w × slot_h` device pixels +
    /// its 2D context. Returns Err if the browser can't supply
    /// OffscreenCanvas or 2D rendering on this canvas.
    pub fn new(slot_w: u16, slot_h: u16) -> Result<Self, String> {
        let canvas = OffscreenCanvas::new(slot_w as u32, slot_h as u32)
            .map_err(|e| format!("GlyphRasterizer: OffscreenCanvas::new failed: {e:?}"))?;
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| format!("GlyphRasterizer: get_context('2d') threw: {e:?}"))?
            .ok_or_else(|| "GlyphRasterizer: get_context('2d') returned None".to_string())?
            .dyn_into::<OffscreenCanvasRenderingContext2d>()
            .map_err(|_| "GlyphRasterizer: 2D context type mismatch".to_string())?;
        Ok(Self {
            canvas,
            ctx,
            slot_w,
            slot_h,
        })
    }

    /// Rasterize a single glyph to RGBA pixels. Stub for slice 1 —
    /// future iteration will:
    ///   1. `ctx.set_font(format!("{size}px {family}"))`.
    ///   2. `ctx.set_text_baseline("top")`.
    ///   3. `ctx.set_fill_style_str("#ffffff")` — render in white so
    ///      shaders can tint via fg_rgba uniform.
    ///   4. `ctx.clear_rect(0, 0, slot_w, slot_h)` — reset to transparent.
    ///   5. `ctx.fill_text(&ch.to_string(), 0.0, 0.0)`.
    ///   6. `ctx.get_image_data(0, 0, w, h)?.data().to_vec()` →
    ///      `RasterizedGlyph { rgba, .. }`.
    ///   7. `ctx.measure_text(...)` for the advance.
    pub fn rasterize(
        &self,
        _font_family: &str,
        _font_size_px: f32,
        _ch: char,
    ) -> Result<RasterizedGlyph, String> {
        Err("GlyphRasterizer::rasterize not yet implemented — see TASKS §4.1.b".to_string())
    }

    pub fn slot_dimensions(&self) -> (u16, u16) {
        (self.slot_w, self.slot_h)
    }
}
