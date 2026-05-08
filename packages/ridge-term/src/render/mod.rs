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
// pipeline / atlas for the whole process. Per-pane WebGpuPaneBackend
// borrows it via Rc<RefCell<>> instead of constructing its own copies.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod gpu_context;

// Shared swap-chain host (Round 3 §4.3 Phase B): one wgpu::Surface
// bound to the global host canvas in +page.svelte. Per-pane backends
// record into a shared command encoder via `SurfaceHost::record_pane`,
// each pane's draw clipped by its own scissor rect. Single submit +
// present per frame regardless of pane count.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod surface_host;

// Glyph rasterizer (Round 3 §4.1.b). OffscreenCanvas-based — uses the
// browser's font fallback chain for free, no extra wasm bundle weight.
// Owned by future WebGpuBackend::draw_row cache-miss path; gated on
// the same wasm32 + webgpu feature combination.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod glyph_rasterizer;

pub use backend::{CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme};
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
    Webgpu(webgpu::WebGpuPaneBackend),
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

    /// Phase B: record the pane's `(x, y)` position on the host canvas
    /// in device pixels. Drives `pass.set_viewport` / `set_scissor_rect`
    /// inside the host's shared render pass so the pane's draw lands at
    /// the correct rect on the host canvas.
    ///
    /// No-op for Canvas2D — that backend owns its own per-pane DOM
    /// canvas, positioned by CSS, so JS-driven offsets are not relevant.
    pub fn set_viewport_offset(&mut self, _x: u32, _y: u32) {
        match self {
            AnyBackend::Canvas2d(_) => {
                // Per-pane canvas owns its DOM position; no GPU-side
                // viewport to update.
            }
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => {
                b.set_viewport_offset(_x, _y);
            }
        }
    }

    /// §4b per-pane increment cache (2026-05-08): re-record this pane's
    /// previously-uploaded instance buffer into the host's current
    /// frame without retraversing the kernel grid. Returns `false` for
    /// Canvas2D (no GPU instance buffer to cache — Canvas2D's per-row
    /// dirty diff already gives equivalent cheapness) and for WebGPU
    /// when the cache was invalidated. Caller must fall back to a full
    /// `render` cycle on `false`.
    pub fn record_cached_only(&mut self) -> bool {
        match self {
            AnyBackend::Canvas2d(_) => false,
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.record_cached_only(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl RenderBackend for AnyBackend {
    fn measure_font(&self, font_family: &str, font_size_px: f32) -> Result<(f32, f32), String> {
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

    fn resize_surface(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), String> {
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

    fn on_full_invalidate(&mut self) {
        // Forward so WebGPU's `needs_initial_clear` flag flips when the
        // renderer detects scroll / sel toggle / snapshot growth — the
        // trait default would silently swallow it and the next frame
        // would `LoadOp::Load` over an undefined or stale background.
        match self {
            AnyBackend::Canvas2d(b) => b.on_full_invalidate(),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.on_full_invalidate(),
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

    fn draw_row(&mut self, row: &RowDraw<'_>, attrs_table: &crate::term::attr_table::AttrTable) {
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
