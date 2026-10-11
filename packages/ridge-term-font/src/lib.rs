//! Selected-font engine and its bitmap contract.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RasterizedGlyph {
    /// Tightly packed premultiplied RGBA8 pixels for the painted bitmap.
    pub rgba: Vec<u8>,
    pub width: u16,
    pub height: u16,
    /// Horizontal advance in CSS pixels.
    pub advance: f32,
    /// Device-pixel offset from cell top to the packed bitmap top.
    pub ascent_offset: f32,
    /// Device-pixel offset from the logical cell origin to the bitmap left.
    pub left_offset: f32,
    pub is_color: bool,
    pub is_box_drawing: bool,
}

impl RasterizedGlyph {
    pub fn validate(&self, width: u16, height: u16) -> Result<(), String> {
        if self.width > width || self.height > height || self.rgba.len() != usize::from(self.width) * usize::from(self.height) * 4 ||
            !self.advance.is_finite() || !self.ascent_offset.is_finite() || !self.left_offset.is_finite() {
            return Err("invalid font bitmap geometry".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    #[test]
    fn rejects_truncated_or_oversized_bitmap_and_nonfinite_geometry() {
        let mut glyph = RasterizedGlyph { rgba: vec![255; 16], width: 2, height: 2, advance: 2.0, ascent_offset: 0.0, left_offset: 0.0, is_color: false, is_box_drawing: false };
        assert!(glyph.validate(2, 2).is_ok());
        assert!(glyph.validate(1, 2).is_err());
        glyph.rgba.pop(); assert!(glyph.validate(2, 2).is_err());
        glyph.rgba.push(255); glyph.left_offset = f32::NAN;
        assert!(glyph.validate(2, 2).is_err());
    }
}

#[cfg(any(feature = "browser", feature = "rasterizer"))]
#[allow(unexpected_cfgs, dead_code)]
#[path = "../../ridge-term/src/render/glyph_atlas.rs"]
mod glyph_atlas;
#[cfg(any(feature = "browser", feature = "rasterizer"))]
#[allow(unexpected_cfgs, dead_code)]
#[path = "../../ridge-term/src/term/wcwidth.rs"]
mod wcwidth;
#[cfg(any(feature = "browser", feature = "rasterizer"))]
mod shared;
#[cfg(all(target_arch = "wasm32", feature = "browser"))]
pub mod browser;
#[cfg(feature = "rasterizer")]
mod rasterizer;
#[cfg(feature = "rasterizer")]
pub use rasterizer::{GlyphRasterizer, register_font_data};

#[cfg(all(target_arch = "wasm32", feature = "rasterizer"))]
mod api {
    use super::*;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(js_name = installFontData)]
    pub fn install_font_data(data: Vec<u8>) -> Result<bool, JsValue> {
        rasterizer::register_font_data(data).map_err(JsValue::from)
    }
    #[wasm_bindgen]
    pub struct FontRasterizer(rasterizer::GlyphRasterizer);
    #[wasm_bindgen]
    impl FontRasterizer {
        #[wasm_bindgen(constructor)]
        pub fn new(width: u16, height: u16) -> Result<FontRasterizer, JsValue> {
            rasterizer::GlyphRasterizer::new(width, height).map(Self).map_err(JsValue::from)
        }
        pub fn rasterize(&mut self, family: &str, size: f32, dpr: f32, flags: u8, text: &str) -> Result<Vec<u8>, JsValue> {
            let glyph = self.0.rasterize(family, size, dpr, flags, text).map_err(JsValue::from)?;
            postcard::to_allocvec(&glyph).map_err(|error| JsValue::from_str(&error.to_string()))
        }
        pub fn measure(&mut self, family: &str, size: f32) -> Result<Vec<f32>, JsValue> {
            let (width, height) = self.0.measure(family, size).map_err(JsValue::from)?;
            Ok(vec![width, height])
        }
        #[wasm_bindgen(js_name = syncRegisteredFonts)]
        pub fn sync_registered_fonts(&mut self) -> Result<bool, JsValue> {
            self.0.sync_registered_fonts().map_err(JsValue::from)
        }
    }
}

