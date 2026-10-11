use crate::{glyph_atlas::GlyphKey, wcwidth::is_emoji_presentation, shared::*, RasterizedGlyph};
use js_sys::{Array, Function, Reflect, Uint8ClampedArray};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::HtmlCanvasElement;
#[cfg(target_arch = "wasm32")]
const SYSTEM_EMOJI_FONT_FAMILY: &str =
    "emoji,'Segoe UI Emoji','Apple Color Emoji','Noto Color Emoji'";

/// Controller-side rasterizer used by Remote Web. It asks Canvas2D to resolve
/// CSS families against that browser's operating system and never reads font
/// files. Canvas2D supplies atlas pixels only; WebGPU remains the presenter.
#[cfg(target_arch = "wasm32")]
pub struct BrowserFontRasterizer {
    _canvas: HtmlCanvasElement,
    context: JsValue,
    slot_w: u16,
    slot_h: u16,
}

#[cfg(target_arch = "wasm32")]
impl BrowserFontRasterizer {
    pub fn new(slot_w: u16, slot_h: u16) -> Result<Self, String> {
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| "WEBGPU_INIT_FAILED: browser document is unavailable".to_string())?;
        let canvas = document
            .create_element("canvas")
            .map_err(|error| browser_js_error("cannot create glyph canvas", error))?
            .dyn_into::<HtmlCanvasElement>()
            .map_err(|error| {
                browser_js_error("created glyph canvas has wrong type", error.into())
            })?;
        canvas.set_width(slot_w.max(1) as u32);
        canvas.set_height(slot_h.max(1) as u32);
        let context: JsValue = canvas
            .get_context("2d")
            .map_err(|error| browser_js_error("cannot acquire glyph Canvas2D context", error))?
            .ok_or_else(|| "WEBGPU_INIT_FAILED: Canvas2D context is unavailable".to_string())?
            .into();
        browser_set(&context, "textAlign", "left")?;
        browser_set(&context, "textBaseline", "alphabetic")?;
        browser_set(&context, "fillStyle", "#ffffff")?;
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
        self.set_font(font_family, device_size, style_flags)?;
        let width = self.slot_w as f64;
        let height = self.slot_h as f64;
        browser_call(
            &self.context,
            "clearRect",
            &[0.0.into(), 0.0.into(), width.into(), height.into()],
        )?;

        // All fallback faces on a row share the primary monospace baseline.
        let line_metrics = browser_call(&self.context, "measureText", &["M".into()])?;
        let line_ascent = browser_metric(&line_metrics, "actualBoundingBoxAscent")
            .unwrap_or(device_size as f64 * 0.8);
        let line_descent = browser_metric(&line_metrics, "actualBoundingBoxDescent")
            .unwrap_or(device_size as f64 * 0.2);
        let line_height = (line_ascent + line_descent).max(device_size as f64 * 1.2);
        let baseline = ((line_height + line_ascent - line_descent) * 0.5).clamp(0.0, height);

        let emoji_presentation = is_emoji_presentation(glyph_text);
        if emoji_presentation {
            self.set_font(SYSTEM_EMOJI_FONT_FAMILY, device_size, 0)?;
        }
        let metrics = browser_call(&self.context, "measureText", &[glyph_text.into()])?;
        let draw_x = browser_metric(&line_metrics, "width")
            .unwrap_or(device_size as f64 * 0.6)
            .ceil()
            .clamp(1.0, (width - 1.0).max(1.0));
        browser_call(
            &self.context,
            "fillText",
            &[glyph_text.into(), draw_x.into(), baseline.into()],
        )?;
        let image = browser_call(
            &self.context,
            "getImageData",
            &[0.0.into(), 0.0.into(), width.into(), height.into()],
        )?;
        let data = Reflect::get(&image, &JsValue::from_str("data"))
            .map_err(|error| browser_js_error("cannot read terminal glyph pixels", error))?;
        let pixels = Uint8ClampedArray::new(&data).to_vec();
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

        let advance_dev = browser_metric(&metrics, "width").unwrap_or(0.0);
        let advance = (advance_dev / dpr as f64) as f32;
        let is_box_drawing = box_drawing_char(glyph_text).is_some();
        if max_x == 0 || max_y == 0 {
            let packed_width = browser_clamp_u16(advance_dev.ceil(), self.slot_w);
            return Ok(RasterizedGlyph {
                rgba: vec![0; packed_width as usize * 4],
                width: packed_width,
                height: 1,
                advance,
                ascent_offset: 0.0,
                left_offset: 0.0,
                is_color: false,
                is_box_drawing,
            });
        }

        // Keep CSS advance as transparent bearing while retaining overhang.
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
                    rgba[dest..dest + 4].fill(alpha);
                }
            }
        }

        // Canvas2D keeps partially transparent edge pixels for Block Elements.
        // The desktop Swash path snaps that painted coverage before it enters the
        // atlas, otherwise adjacent terminal cells reveal hairline seams on the
        // Remote Web canvas. Keep browser font selection, metrics, and ordinary
        // glyph antialiasing intact; this only gives geometric blocks the same
        // hard coverage contract as the desktop renderer.
        if is_grid_block_element(glyph_text) {
            normalize_grid_block_coverage(&mut rgba);
        }

        Ok(RasterizedGlyph {
            rgba,
            width: packed_width,
            height: packed_height,
            advance,
            ascent_offset: min_y as f32,
            left_offset: crop_left as f32 - draw_x as f32,
            is_color,
            is_box_drawing,
        })
    }

    pub fn measure(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
        let size = font_size_px.max(1.0);
        self.set_font(font_family, size, 0)?;
        let metrics = browser_call(&self.context, "measureText", &["M".into()])?;
        let width = browser_metric(&metrics, "width").unwrap_or(size as f64 * 0.6);
        let ascent = browser_metric(&metrics, "actualBoundingBoxAscent").unwrap_or(0.0);
        let descent = browser_metric(&metrics, "actualBoundingBoxDescent").unwrap_or(0.0);
        let line_height = (ascent + descent).max(size as f64 * 1.2);
        Ok((width.max(1.0) as f32, line_height.max(1.0) as f32))
    }

    fn set_font(&self, family: &str, size_px: f32, style_flags: u8) -> Result<(), String> {
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
        let family = if family.trim().is_empty() {
            "monospace"
        } else {
            family
        };
        browser_set(
            &self.context,
            "font",
            &format!("{style}{weight}{size_px:.3}px {family}"),
        )
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_call(target: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let function = Reflect::get(target, &JsValue::from_str(name))
        .map_err(|error| browser_js_error(&format!("cannot access Canvas2D.{name}"), error))?
        .dyn_into::<Function>()
        .map_err(|error| {
            browser_js_error(&format!("Canvas2D.{name} is not callable"), error.into())
        })?;
    let values = Array::new();
    for value in args {
        values.push(value);
    }
    function
        .apply(target, &values)
        .map_err(|error| browser_js_error(&format!("Canvas2D.{name} failed"), error))
}

#[cfg(target_arch = "wasm32")]
fn browser_set(target: &JsValue, name: &str, value: &str) -> Result<(), String> {
    Reflect::set(target, &JsValue::from_str(name), &JsValue::from_str(value))
        .map(|_| ())
        .map_err(|error| browser_js_error(&format!("cannot set Canvas2D.{name}"), error))
}

#[cfg(target_arch = "wasm32")]
fn browser_metric(metrics: &JsValue, name: &str) -> Option<f64> {
    let value = Reflect::get(metrics, &JsValue::from_str(name))
        .ok()?
        .as_f64()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

#[cfg(target_arch = "wasm32")]
fn browser_clamp_u16(value: f64, max: u16) -> u16 {
    value.max(1.0).min(max as f64) as u16
}

#[cfg(target_arch = "wasm32")]
fn browser_js_error(prefix: &str, value: JsValue) -> String {
    let detail = value
        .as_string()
        .unwrap_or_else(|| "unknown JavaScript error".to_string());
    format!("{prefix}: {detail}")
}

