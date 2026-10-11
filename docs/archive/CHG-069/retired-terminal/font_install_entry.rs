/// Register host-provided system-font bytes and synchronize a live shared
/// rasterizer. The atlas is invalidated only when the payload is new.
///
/// With per-canvas `GpuContext` (post §A.8 AVD fix) the live rasterizer
/// lives inside the most-recent `GpuContext`, which `SurfaceHost::init`
/// created on demand. New `GpuContext` instances call
/// `sync_registered_fonts()` themselves inside `Self::new`, so this
/// function only needs to register the bytes into the global font-data
/// registry — subsequent contexts will pick them up at construction.
/// The return value still tells JS whether the payload was new (used
/// by `fontLoadPromises` to short-circuit duplicate fetches).
pub fn install_font_data(data: Vec<u8>) -> Result<bool, String> {
    super::glyph_rasterizer::register_font_data(data)
}

