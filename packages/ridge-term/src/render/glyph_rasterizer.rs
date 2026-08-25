//!
//! Rasterizes terminal glyphs with the browser's native Canvas2D font stack.
//!
//! The canvas is detached from the document and exists only as a glyph source
//! for the WebGPU atlas. Browser/OS font fallback handles CJK, symbols, emoji,
//! combining marks, and grapheme clusters without shipping or requesting raw
//! local font files.

#![cfg(feature = "webgpu")]

use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use super::glyph_atlas::GlyphKey;
use crate::term::wcwidth::is_emoji_presentation;

const SYSTEM_EMOJI_FONT_FAMILY: &str =
    "emoji,'Segoe UI Emoji','Apple Color Emoji','Noto Color Emoji'";

#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    /// Tightly packed premultiplied RGBA8 pixels for the painted bitmap,
    /// row-major. The GPU upload adds only the row padding required by wgpu.
    pub rgba: Vec<u8>,
    /// Painted bounds inside the slot.
    pub width: u16,
    pub height: u16,
    /// Horizontal advance in CSS pixels.
    pub advance: f32,
    /// Device-pixel offset from cell top to the cropped bitmap top.
    pub ascent_offset: f32,
    /// Device-pixel offset from the cell origin to the packed bitmap left.
    /// May be negative for italic or emoji overhang.
    pub left_offset: f32,
    /// True when Canvas2D supplied native color pixels (normally emoji).
    pub is_color: bool,
}

pub struct GlyphRasterizer {
    _canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    slot_w: u16,
    slot_h: u16,
}

impl GlyphRasterizer {
    pub fn new(slot_w: u16, slot_h: u16) -> Result<Self, String> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "WEBGPU_INIT_FAILED: browser document is unavailable".to_string())?;
        let canvas = document
            .create_element("canvas")
            .map_err(|error| js_error("cannot create glyph canvas", error))?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|error| js_error("created glyph canvas has wrong type", error.into()))?;
        let width = slot_w.max(1) as u32;
        let height = slot_h.max(1) as u32;
        canvas.set_width(width);
        canvas.set_height(height);
        let context = canvas
            .get_context("2d")
            .map_err(|error| js_error("cannot acquire glyph Canvas2D context", error))?
            .ok_or_else(|| "WEBGPU_INIT_FAILED: Canvas2D context is unavailable".to_string())?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|error| js_error("glyph context has wrong type", error.into()))?;
        context.set_text_align("left");
        context.set_text_baseline("alphabetic");
        context.set_fill_style_str("#ffffff");
        Ok(Self {
            _canvas: canvas,
            context,
            slot_w: slot_w.max(1),
            slot_h: slot_h.max(1),
        })
    }

    pub fn rasterize(
        &mut self,
        font_family: &str,
        font_size_px: f32,
        dpr: f32,
        style_flags: u8,
        glyph_text: &str,
    ) -> Result<RasterizedGlyph, String> {
        let dpr = valid_dpr(dpr);
        let device_size = (font_size_px.max(1.0) * dpr).max(1.0);
        self.set_font(font_family, device_size, style_flags);
        let width = self.slot_w as f64;
        let height = self.slot_h as f64;
        self.context.clear_rect(0.0, 0.0, width, height);

        // All fallback faces in one terminal row share the primary font's
        // alphabetic baseline. A per-glyph baseline makes CJK, box drawing,
        // and symbol faces look vertically independent even though their
        // cells share one line box.
        let line_metrics = self
            .context
            .measure_text("M")
            .map_err(|error| js_error("cannot measure terminal line box", error))?;
        let line_ascent = finite_non_negative(line_metrics.actual_bounding_box_ascent())
            .unwrap_or((device_size as f64) * 0.8);
        let line_descent = finite_non_negative(line_metrics.actual_bounding_box_descent())
            .unwrap_or((device_size as f64) * 0.2);
        let line_height = (line_ascent + line_descent).max((device_size as f64) * 1.2);
        let baseline = ((line_height + line_ascent - line_descent) * 0.5).clamp(0.0, height);

        let emoji_presentation = is_emoji_presentation(glyph_text);
        if emoji_presentation {
            // Ask CSS for the platform emoji face directly. Keeping this out
            // of the general symbol fallback prevents symbol fonts from
            // stealing emoji presentation codepoints.
            self.set_font(SYSTEM_EMOJI_FONT_FAMILY, device_size, 0);
        }

        let metrics = self
            .context
            .measure_text(glyph_text)
            .map_err(|error| js_error("cannot measure terminal glyph", error))?;
        // Reserve one primary-cell advance on the left. Scanning the painted
        // pixels afterwards retains both side bearings and any glyph overhang
        // instead of aligning ink to x=0 and cropping at the logical cell.
        let draw_x = finite_non_negative(line_metrics.width())
            .unwrap_or(device_size as f64 * 0.6)
            .ceil()
            .clamp(1.0, (width - 1.0).max(1.0));
        self.context
            .fill_text(glyph_text, draw_x, baseline)
            .map_err(|error| js_error("cannot rasterize terminal glyph", error))?;

        let image = self
            .context
            .get_image_data(0.0, 0.0, width, height)
            .map_err(|error| js_error("cannot read terminal glyph pixels", error))?;
        let pixels = image.data().0;
        let pixel_count = self.slot_w as usize * self.slot_h as usize;
        if pixels.len() != pixel_count * 4 {
            return Err(
                "WEBGPU_INIT_FAILED: glyph Canvas2D returned an invalid pixel buffer".to_string(),
            );
        }

        let mut min_x = self.slot_w as usize;
        let mut min_y = self.slot_h as usize;
        let mut max_x = 0_usize;
        let mut max_y = 0_usize;
        let mut is_color = emoji_presentation;
        for index in 0..pixel_count {
            let offset = index * 4;
            let alpha = pixels[offset + 3];
            if alpha == 0 {
                continue;
            }
            let x = index % self.slot_w as usize;
            let y = index / self.slot_w as usize;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
            is_color |=
                pixels[offset] != pixels[offset + 1] || pixels[offset + 1] != pixels[offset + 2];
        }

        let advance_dev = finite_non_negative(metrics.width()).unwrap_or(0.0);
        let advance = (advance_dev / dpr as f64) as f32;
        if max_x == 0 || max_y == 0 {
            let width = clamp_u16(advance_dev.ceil(), self.slot_w);
            return Ok(RasterizedGlyph {
                rgba: vec![0; width as usize * 4],
                width,
                height: 1,
                advance,
                ascent_offset: 0.0,
                left_offset: 0.0,
                is_color: false,
            });
        }

        // Preserve the logical advance as transparent side bearing while also
        // retaining painted pixels outside it. The renderer then places this
        // native bitmap without scaling or cell clipping.
        let crop_left = min_x.min(draw_x.floor().max(0.0) as usize);
        let advance_right = (draw_x + advance_dev).ceil().max(1.0) as usize;
        let crop_right = max_x.max(advance_right).min(self.slot_w as usize);
        let packed_width = (crop_right.saturating_sub(crop_left) as u16).max(1);
        let packed_height = (max_y - min_y) as u16;
        let mut rgba = vec![0; packed_width as usize * packed_height as usize * 4];
        for source_y in min_y..max_y {
            let dest_y = source_y - min_y;
            for dest_x in 0..packed_width as usize {
                let source_x = crop_left + dest_x;
                let source = (source_y * self.slot_w as usize + source_x) * 4;
                let dest = (dest_y * packed_width as usize + dest_x) * 4;
                let alpha = pixels[source + 3];
                if alpha == 0 {
                    continue;
                }
                if is_color {
                    rgba[dest] = premultiply(pixels[source], alpha);
                    rgba[dest + 1] = premultiply(pixels[source + 1], alpha);
                    rgba[dest + 2] = premultiply(pixels[source + 2], alpha);
                    rgba[dest + 3] = alpha;
                } else {
                    // The shader uses alpha as coverage and tints monochrome
                    // glyphs with the cell foreground color.
                    rgba[dest..dest + 4].fill(alpha);
                }
            }
        }

        Ok(RasterizedGlyph {
            rgba,
            width: packed_width,
            height: packed_height,
            advance,
            ascent_offset: min_y as f32,
            left_offset: crop_left as f32 - draw_x as f32,
            is_color,
        })
    }

    pub fn slot_dimensions(&self) -> (u16, u16) {
        (self.slot_w, self.slot_h)
    }

    pub fn measure(&mut self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        let size = font_size_px.max(1.0);
        self.set_font(font_family, size, 0);
        let metrics = self
            .context
            .measure_text("M")
            .map_err(|error| js_error("cannot measure terminal font", error))?;
        let width = finite_non_negative(metrics.width()).unwrap_or(size as f64 * 0.6);
        let ascent = finite_non_negative(metrics.actual_bounding_box_ascent()).unwrap_or(0.0);
        let descent = finite_non_negative(metrics.actual_bounding_box_descent()).unwrap_or(0.0);
        let measured_height = ascent + descent;
        let line_height = measured_height.max(size as f64 * 1.2);
        Ok((width.max(1.0) as f32, line_height.max(1.0) as f32))
    }

    fn set_font(&self, font_family: &str, size_px: f32, style_flags: u8) {
        let style = if style_flags & GlyphKey::STYLE_ITALIC != 0 {
            "italic "
        } else {
            ""
        };
        let weight = if style_flags & GlyphKey::STYLE_BOLD != 0 {
            "bold "
        } else {
            ""
        };
        let family = if font_family.trim().is_empty() {
            "monospace"
        } else {
            font_family
        };
        self.context
            .set_font(&format!("{style}{weight}{size_px:.3}px {family}"));
    }
}

fn valid_dpr(dpr: f32) -> f32 {
    if dpr.is_finite() && dpr > 0.0 {
        dpr
    } else {
        1.0
    }
}

fn finite_non_negative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn clamp_u16(value: f64, max: u16) -> u16 {
    value.max(1.0).min(max as f64) as u16
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((channel as u32 * alpha as u32 + 127) / 255) as u8
}

fn js_error(prefix: &str, value: JsValue) -> String {
    let detail = value
        .as_string()
        .unwrap_or_else(|| "unknown JavaScript error".to_string());
    format!("{prefix}: {detail}")
}
