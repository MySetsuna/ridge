//! GPU adapter to browser glyphs or the optional selected-font WASM.
use std::cell::RefCell;
use js_sys::{Array, Float32Array, Function, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
pub use ridge_term_font::RasterizedGlyph;
use ridge_term_font::browser::BrowserFontRasterizer;

thread_local! { static FONT_FACTORY: RefCell<Option<Function>> = const { RefCell::new(None) }; }
pub fn install_factory(factory: Function) { FONT_FACTORY.with(|slot| *slot.borrow_mut() = Some(factory)); }
enum Engine { Browser(BrowserFontRasterizer), Selected(JsValue) }
pub struct GlyphRasterizer { engine: Engine, width: u16, height: u16 }
fn call(target: &JsValue, method: &str, arguments: &[JsValue]) -> Result<JsValue, String> {
    let function = Reflect::get(target, &method.into()).map_err(error)?.dyn_into::<Function>().map_err(error)?;
    let array = Array::new();
    for argument in arguments { array.push(argument); }
    function.apply(target, &array).map_err(error)
}
fn error(value: JsValue) -> String { format!("FONT_ENGINE_FAILED: {value:?}") }
impl GlyphRasterizer {
    pub fn new(width: u16, height: u16) -> Result<Self, String> {
        let factory = FONT_FACTORY.with(|slot| slot.borrow().clone());
        let engine = match factory {
            Some(factory) => Engine::Selected(factory.call2(&JsValue::NULL, &width.into(), &height.into()).map_err(error)?),
            None => Engine::Browser(BrowserFontRasterizer::new(width, height)?),
        };
        Ok(Self { engine, width, height })
    }
    pub fn sync_registered_fonts(&mut self) -> Result<bool, String> {
        match &self.engine {
            Engine::Browser(_) => Ok(false),
            Engine::Selected(target) => call(target, "syncRegisteredFonts", &[])?.as_bool().ok_or_else(|| "invalid font sync result".into()),
        }
    }
    pub fn slot_dimensions(&self) -> (u16, u16) { (self.width, self.height) }
    pub fn measure(&mut self, family: &str, size: f32) -> Result<(f32, f32), String> {
        match &mut self.engine {
            Engine::Browser(browser) => browser.measure(family, size),
            Engine::Selected(target) => {
                let value = call(target, "measure", &[family.into(), size.into()])?;
                if !value.is_instance_of::<Float32Array>() { return Err("invalid font metrics".into()); }
                let metrics = Float32Array::new(&value).to_vec();
                if metrics.len() != 2 || metrics.iter().any(|value| !value.is_finite() || *value <= 0.0) { return Err("invalid font metrics".into()); }
                Ok((metrics[0], metrics[1]))
            }
        }
    }
    pub fn rasterize(&mut self, family: &str, size: f32, dpr: f32, flags: u8, text: &str) -> Result<RasterizedGlyph, String> {
        match &mut self.engine {
            Engine::Browser(browser) => browser.rasterize(family, size, dpr, flags, text),
            Engine::Selected(target) => {
                let value = call(target, "rasterize", &[family.into(), size.into(), dpr.into(), flags.into(), text.into()])?;
                if !value.is_instance_of::<Uint8Array>() { return Err("invalid font bitmap packet".into()); }
                let bytes = Uint8Array::new(&value).to_vec();
                if bytes.len() > usize::from(self.width) * usize::from(self.height) * 4 + 64 { return Err("font bitmap exceeds slot".into()); }
                let glyph: RasterizedGlyph = postcard::from_bytes(&bytes).map_err(|error| error.to_string())?;
                glyph.validate(self.width, self.height)?;
                Ok(glyph)
            }
        }
    }
}
impl Drop for GlyphRasterizer {
    fn drop(&mut self) { if let Engine::Selected(target) = &self.engine { let _ = call(target, "free", &[]); } }
}
