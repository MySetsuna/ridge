//! Rendering layer.
//!
//! Gated on `target_arch = "wasm32"` because the backends use web-sys.
//! The `term` module (VT kernel) stays target-agnostic so unit tests
//! run on the host with `cargo test --lib`.

pub mod backend;
pub mod renderer;

#[cfg(target_arch = "wasm32")]
pub mod canvas2d;

// Round 3 scaffold: glyph atlas (host-buildable; pure data structure)
// and a WebGPU backend stub whose constructor errs until §4.1 lands the
// real wgpu wiring. Both are exported so future iterations can swap them
// in without further mod.rs churn.
pub mod glyph_atlas;

// WebGPU backend. Gated on both wasm32 (web-sys / GPU bindings) and the
// `webgpu` cargo feature so default builds skip the trait-impl scaffold
// and stay compact. Flip on with `cargo build --features webgpu` while
// the §4.1 implementation lands.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod webgpu;

// Shared GPU context (Round 3 §4.3 Phase A): one Device / Queue /
// pipeline / atlas for the whole process. Per-pane WebGpuBackend
// borrows it via Rc<RefCell<>> instead of constructing its own copies.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod gpu_context;

// Glyph rasterizer (Round 3 §4.1.b). OffscreenCanvas-based — uses the
// browser's font fallback chain for free, no extra wasm bundle weight.
// Owned by future WebGpuBackend::draw_row cache-miss path; gated on
// the same wasm32 + webgpu feature combination.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod glyph_rasterizer;

pub use backend::{
    CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme,
};
pub use renderer::Renderer;

#[cfg(target_arch = "wasm32")]
pub use canvas2d::Canvas2dBackend;

// ─── AnyBackend (Round 3 §4.1.e infrastructure) ─────────────────────
//
// Enum-dispatch wrapper that holds either a Canvas2dBackend or
// (when the `webgpu` cargo feature is on) a WebGpuBackend. Implements
// `RenderBackend` by forwarding every trait method to the active
// variant. Lets `RenderHandle` use `Renderer<AnyBackend>` and switch
// backends at construction time based on adapter availability without
// changing the `Renderer<B>` generic to `Renderer<dyn RenderBackend>`
// (which would force trait-object dispatch through a vtable for every
// frame's per-row draw call — not what we want on the hot path).
//
// The match arms on each method are mechanical but the compiler
// inlines monomorphized variants on optimization, so the runtime
// cost is one branch + a tail call.
//
// Wiring `RenderHandle` to use this lands in §4.1.e.next; this
// commit just defines the enum + impl so the dispatch code is
// reviewable in isolation.

#[cfg(target_arch = "wasm32")]
pub enum AnyBackend {
    Canvas2d(Canvas2dBackend),
    #[cfg(feature = "webgpu")]
    Webgpu(webgpu::WebGpuBackend),
}

#[cfg(target_arch = "wasm32")]
impl AnyBackend {
    /// Set the font CSS family + pixel size. Translates the unified
    /// (family, size_px) form into whichever shape each backend
    /// expects: Canvas2D wants a single CSS string; WebGPU wants
    /// the two parts separately for the rasterizer.
    pub fn set_font_config(&mut self, font_family: String, font_size_px: f32) {
        match self {
            AnyBackend::Canvas2d(b) => {
                b.set_font(format!("{}px {}", font_size_px, font_family));
            }
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => {
                b.set_font_config(font_family, font_size_px);
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl RenderBackend for AnyBackend {
    fn measure_font(
        &self,
        font_family: &str,
        font_size_px: f32,
    ) -> Result<(f32, f32), String> {
        match self {
            AnyBackend::Canvas2d(b) => b.measure_font(font_family, font_size_px),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.measure_font(font_family, font_size_px),
        }
    }

    fn requires_full_frame(&self) -> bool {
        // CRITICAL: forward to the active variant. Without this override,
        // AnyBackend would fall back to the trait's default `false`
        // regardless of which inner backend is active — and the WebGPU
        // visibility fix (`renderer.rs::tick` uses this to mark every
        // visible row dirty for backends that can't preserve content
        // across frames) would be entirely defeated. Caught 2026-05-05
        // from a user report that non-active rows disappeared with
        // WebGPU active.
        match self {
            AnyBackend::Canvas2d(b) => b.requires_full_frame(),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.requires_full_frame(),
        }
    }

    fn resize_surface(
        &mut self,
        width_css: u32,
        height_css: u32,
        dpr: f32,
    ) -> Result<(), String> {
        match self {
            AnyBackend::Canvas2d(b) => b.resize_surface(width_css, height_css, dpr),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.resize_surface(width_css, height_css, dpr),
        }
    }

    fn invalidate_atlas(&mut self) {
        // CRITICAL: forward to the active variant. Canvas2D's default
        // no-op is fine but WebGPU MUST drop its GlyphAtlas + reset
        // next_layer on resize / font change; without this forward,
        // AnyBackend would fall through to the trait default and
        // stale glyph UVs persist across DPR / font-size changes —
        // exactly the "resize + claude → 字符位置错乱" report.
        match self {
            AnyBackend::Canvas2d(b) => b.invalidate_atlas(),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.invalidate_atlas(),
        }
    }

    fn begin_frame(&mut self, metrics: FrameMetrics, theme: &Theme) {
        match self {
            AnyBackend::Canvas2d(b) => b.begin_frame(metrics, theme),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.begin_frame(metrics, theme),
        }
    }

    fn clear(&mut self) {
        match self {
            AnyBackend::Canvas2d(b) => b.clear(),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.clear(),
        }
    }

    fn draw_row(
        &mut self,
        row: &RowDraw<'_>,
        attrs_table: &crate::term::attr_table::AttrTable,
    ) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_row(row, attrs_table),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_row(row, attrs_table),
        }
    }

    fn draw_cursor(
        &mut self,
        cursor: &CursorDraw,
        attrs_table: &crate::term::attr_table::AttrTable,
    ) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_cursor(cursor, attrs_table),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_cursor(cursor, attrs_table),
        }
    }

    fn draw_selection_overlay(&mut self, rects: &[(usize, usize, usize)]) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_selection_overlay(rects),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_selection_overlay(rects),
        }
    }

    fn draw_hyperlink_underlines(&mut self, rects: &[(usize, usize, usize)]) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_hyperlink_underlines(rects),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_hyperlink_underlines(rects),
        }
    }

    fn end_frame(&mut self) {
        match self {
            AnyBackend::Canvas2d(b) => b.end_frame(),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.end_frame(),
        }
    }
}
