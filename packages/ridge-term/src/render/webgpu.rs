//! WebGPU rendering backend — Round 3 §4.1 placeholder.
//!
//! ## Status
//!
//! **Scaffold only.** `WebGpuBackend::new()` returns `Err` so no instance
//! ever exists outside a future implementation that wires up wgpu. The
//! `RenderBackend` impl is here so:
//!   - the trait surface is type-checked against the real WebGPU
//!     contract right now (catches signature drift in `backend.rs`)
//!   - flipping `lib.rs::RenderHandle` to `Box<dyn RenderBackend>` for
//!     runtime backend selection only requires landing the `new()`
//!     body, not also re-typing the consumer
//!
//! ## Real implementation outline (deferred to next iteration)
//!
//! 1. Add `wgpu = "23.0"` (or matching) under `[target.'cfg(target_arch = "wasm32")'.dependencies]`
//!    in `Cargo.toml`. The wasm target uses the browser's WebGPU API
//!    via wgpu's `webgpu` backend, no extra plumbing.
//! 2. `new(canvas: HtmlCanvasElement)` → request adapter + device,
//!    create surface, configure swap chain. Return `Err` on adapter
//!    miss so callers can fall back to Canvas2D.
//! 3. Hold a `super::glyph_atlas::GlyphAtlas` (§4.2 — already landed).
//! 4. Rasterize new glyphs via `cosmic-text` or `fontdue`; upload
//!    bitmap to a texture array layer; populate `GlyphEntry`.
//! 5. `draw_row` builds an instance buffer of `(cell_xy, atlas_uv,
//!    fg_rgba, bg_rgba)` tuples and submits one indirect draw call
//!    per row.
//! 6. Cursor / selection / hyperlink overlays are separate small
//!    pipeline passes (full-quad shader + scissor rect).
//!
//! ## Why Err-on-construction instead of cfg-gating the whole module
//!
//! Cfg-gating a feature flag like `webgpu` would make the trait surface
//! invisible to `cargo check` on the default build. That defeats the
//! main point of having the scaffold: catching breaking changes in
//! `RenderBackend` early. The Err-construction pattern keeps the trait
//! impl in the type system without shipping any GPU code.

#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]
#![allow(dead_code)] // round-3 scaffold; real wiring lands in §4.1 implementation iteration.

use crate::render::backend::{
    CursorDraw, FrameMetrics, RenderBackend, RowDraw, Theme,
};
use crate::term::attr_table::AttrTable;
use super::glyph_atlas::GlyphAtlas;

/// WebGPU backend. Currently inert — `new()` errs and the trait impl is
/// reachable only on an instance that cannot exist. Future iteration
/// (TASKS §4.1) replaces the body of `new()` with adapter/device/surface
/// setup and fills in the `unreachable!()` slots.
pub struct WebGpuBackend {
    _atlas: GlyphAtlas,
    _metrics: Option<FrameMetrics>,
}

impl WebGpuBackend {
    /// Always errs at present. Caller should fall back to Canvas2D.
    pub fn new() -> Result<Self, String> {
        Err("WebGpuBackend not yet implemented — see TASKS §4.1".to_string())
    }
}

impl RenderBackend for WebGpuBackend {
    fn measure_font(&self, _font_family: &str, _font_size_px: f32) -> Result<(f32, f32), String> {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn resize_surface(&mut self, _width_css: u32, _height_css: u32, _dpr: f32) -> Result<(), String> {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn begin_frame(&mut self, _metrics: FrameMetrics, _theme: &Theme) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn clear(&mut self) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn draw_row(&mut self, _row: &RowDraw<'_>, _attrs_table: &AttrTable) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn draw_cursor(&mut self, _cursor: &CursorDraw, _attrs_table: &AttrTable) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn draw_selection_overlay(&mut self, _rects: &[(usize, usize, usize)]) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn draw_hyperlink_underlines(&mut self, _rects: &[(usize, usize, usize)]) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }

    fn end_frame(&mut self) {
        unreachable!("WebGpuBackend::new always errs (TASKS §4.1)")
    }
}
