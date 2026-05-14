//! Rendering layer.
//!
//! Gated on `target_arch = "wasm32"` because the backends use web-sys.
//! The `term` module (VT kernel) stays target-agnostic so unit tests
//! run on the host with `cargo test --lib`.

pub mod backend;
#[cfg(target_arch = "wasm32")]
pub mod canvas2d;
pub mod glyph_atlas;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod glyph_rasterizer;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod gpu_context;
pub mod renderer;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod surface_host;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod webgpu;

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Generates procedural rectangles for Box Drawing (U+2500..=U+257F) and 
/// Block Elements (U+2580..=U+259F). Returns None if the character is not supported.
pub fn procedural_box(c: char, cell_x: f32, cell_y: f32, cell_w: f32, cell_h: f32) -> Option<Vec<Rect>> {
    let mut rects = Vec::with_capacity(2);

    let lw = (cell_w * 0.15).max(1.0).round();
    let lh = (cell_h * 0.1).max(1.0).round();
    
    // Centers for line drawing
    let cx = cell_x + (cell_w / 2.0).floor() - (lw / 2.0).floor();
    let cy = cell_y + (cell_h / 2.0).floor() - (lh / 2.0).floor();

    match c {
        // --- Block Elements (U+2580 - U+259F) ---
        '\u{2588}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w, h: cell_h }), // Full block
        '\u{2580}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w, h: (cell_h / 2.0).ceil() }), // Upper half block
        '\u{2584}' => rects.push(Rect { x: cell_x, y: cell_y + (cell_h / 2.0).floor(), w: cell_w, h: (cell_h / 2.0).ceil() }), // Lower half block
        '\u{258C}' => rects.push(Rect { x: cell_x, y: cell_y, w: (cell_w / 2.0).ceil(), h: cell_h }), // Left half block
        '\u{2590}' => rects.push(Rect { x: cell_x + (cell_w / 2.0).floor(), y: cell_y, w: (cell_w / 2.0).ceil(), h: cell_h }), // Right half block
        '\u{2581}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h - (cell_h * 0.125).ceil(), w: cell_w, h: (cell_h * 0.125).ceil() }), // Lower one eighth
        '\u{2582}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h - (cell_h * 0.25).ceil(), w: cell_w, h: (cell_h * 0.25).ceil() }), // Lower one quarter
        '\u{2583}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h - (cell_h * 0.375).ceil(), w: cell_w, h: (cell_h * 0.375).ceil() }), // Lower three eighths
        '\u{2585}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h - (cell_h * 0.625).ceil(), w: cell_w, h: (cell_h * 0.625).ceil() }), // Lower five eighths
        '\u{2586}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h - (cell_h * 0.75).ceil(), w: cell_w, h: (cell_h * 0.75).ceil() }), // Lower three quarters
        '\u{2587}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h - (cell_h * 0.875).ceil(), w: cell_w, h: (cell_h * 0.875).ceil() }), // Lower seven eighths

        // --- Box Drawing (U+2500 - U+257F) Core set ---
        // Horizontal
        '\u{2500}' | '\u{2501}' => rects.push(Rect { x: cell_x, y: cy, w: cell_w, h: lh }),
        // Vertical
        '\u{2502}' | '\u{2503}' => rects.push(Rect { x: cx, y: cell_y, w: lw, h: cell_h }),
        
        // Corners
        '\u{250C}' | '\u{250D}' | '\u{250E}' | '\u{250F}' => { // Top-left
            rects.push(Rect { x: cx, y: cy, w: cell_w - (cx - cell_x), h: lh }); 
            rects.push(Rect { x: cx, y: cy, w: lw, h: cell_h - (cy - cell_y) });
        }
        '\u{2510}' | '\u{2511}' | '\u{2512}' | '\u{2513}' => { // Top-right
            rects.push(Rect { x: cell_x, y: cy, w: cx - cell_x + lw, h: lh }); 
            rects.push(Rect { x: cx, y: cy, w: lw, h: cell_h - (cy - cell_y) });
        }
        '\u{2514}' | '\u{2515}' | '\u{2516}' | '\u{2517}' => { // Bottom-left
            rects.push(Rect { x: cx, y: cy, w: cell_w - (cx - cell_x), h: lh }); 
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cy - cell_y + lh });
        }
        '\u{2518}' | '\u{2519}' | '\u{251A}' | '\u{251B}' => { // Bottom-right
            rects.push(Rect { x: cell_x, y: cy, w: cx - cell_x + lw, h: lh }); 
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cy - cell_y + lh });
        }
        
        // T-shapes
        '\u{251C}' | '\u{251D}' | '\u{251E}' | '\u{251F}' | '\u{2520}' | '\u{2521}' | '\u{2522}' | '\u{2523}' => { // Vertical-right
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cell_h }); 
            rects.push(Rect { x: cx, y: cy, w: cell_w - (cx - cell_x), h: lh });
        }
        '\u{2524}' | '\u{2525}' | '\u{2526}' | '\u{2527}' | '\u{2528}' | '\u{2529}' | '\u{252A}' | '\u{252B}' => { // Vertical-left
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cell_h }); 
            rects.push(Rect { x: cell_x, y: cy, w: cx - cell_x + lw, h: lh });
        }
        '\u{252C}' | '\u{252D}' | '\u{252E}' | '\u{252F}' | '\u{2530}' | '\u{2531}' | '\u{2532}' | '\u{2533}' => { // Horizontal-down
            rects.push(Rect { x: cell_x, y: cy, w: cell_w, h: lh }); 
            rects.push(Rect { x: cx, y: cy, w: lw, h: cell_h - (cy - cell_y) });
        }
        '\u{2534}' | '\u{2535}' | '\u{2536}' | '\u{2537}' | '\u{2538}' | '\u{2539}' | '\u{253A}' | '\u{253B}' => { // Horizontal-up
            rects.push(Rect { x: cell_x, y: cy, w: cell_w, h: lh }); 
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cy - cell_y + lh });
        }
        
        // Cross
        '\u{253C}' | '\u{253D}' | '\u{253E}' | '\u{253F}' | '\u{2540}' | '\u{2541}' | '\u{2542}' | '\u{2543}' | '\u{2544}' | '\u{2545}' | '\u{2546}' | '\u{2547}' | '\u{2548}' | '\u{2549}' | '\u{254A}' | '\u{254B}' => {
            rects.push(Rect { x: cell_x, y: cy, w: cell_w, h: lh }); 
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cell_h });
        }
        
        _ => return None,
    }
    Some(rects)
}

// Shared GPU context (Round 3 §4.3 Phase A): one Device / Queue /
// pipeline / atlas for the whole process. Per-pane WebGpuPaneBackend
// borrows it via Rc<RefCell<>> instead of constructing its own copies.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]

// Shared swap-chain host (Round 3 §4.3 Phase B): one wgpu::Surface
// bound to the global host canvas in +page.svelte. Per-pane backends
// each pane's draw clipped by its own scissor rect. Single submit +
// present per frame regardless of pane count.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]

// Glyph rasterizer (Round 3 §4.1.b). OffscreenCanvas-based — uses the
// browser's font fallback chain for free, no extra wasm bundle weight.
// Owned by future WebGpuBackend::draw_row cache-miss path; gated on
// the same wasm32 + webgpu feature combination.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]

pub use backend::{CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme};
pub use renderer::Renderer;

// ─── Static WGSL validation (host-target only) ─────────────────────────
//
// `cell.wgsl` is `include_str!`'d into the binary and only validated by
// wgpu at `device.create_shader_module()` time — i.e. inside the
// browser, on the first WebGPU pane attach. A typo there is a
// production-only failure that surfaces as a JS console error and
// silent fallback to Canvas2D for that pane.
//
// Naga is the parser+validator wgpu uses internally. Pulling it as a
// host dev-dep (see Cargo.toml `[dev-dependencies]`) lets us validate
// the shader on every `cargo test --lib` — synchronously, with the
// CI gate that already exists. If you change `cell.wgsl` and break
// it, this test fires before the browser ever sees the file.
#[cfg(test)]
mod wgsl_validation_tests {
    /// Embed the same source text the WebGPU bootstrap loads at runtime
    /// (`include_str!("shaders/cell.wgsl")` in `gpu_context.rs`). Single
    /// source of truth — if either path drifts the test breaks loudly.
    const CELL_WGSL: &str = include_str!("shaders/cell.wgsl");

    #[test]
    fn cell_wgsl_parses_and_validates() {
        let module = naga::front::wgsl::parse_str(CELL_WGSL)
            .unwrap_or_else(|e| panic!("cell.wgsl parse error:\n{}", e.emit_to_string(CELL_WGSL)));

        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("cell.wgsl validation error: {e:?}"));

        // Sanity: vs_main + fs_main must both be present in the module.
        // (Naga's `ModuleInfo.entry_points` is private; the public list
        // lives on `Module` itself.)
        let names: Vec<&str> = module
            .entry_points
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            names.contains(&"vs_main") && names.contains(&"fs_main"),
            "expected vs_main + fs_main, got {names:?}"
        );
    }
}

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

    fn draw_row_backgrounds(&mut self, row: &RowDraw<'_>, attrs_table: &crate::term::attr_table::AttrTable) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_row_backgrounds(row, attrs_table),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_row_backgrounds(row, attrs_table),
        }
    }

    fn draw_row_texts(&mut self, row: &RowDraw<'_>, attrs_table: &crate::term::attr_table::AttrTable) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_row_texts(row, attrs_table),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_row_texts(row, attrs_table),
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
