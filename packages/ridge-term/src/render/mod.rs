//! Rendering layer.
//!
//! WebGPU presentation modules are gated on `target_arch = "wasm32"` because
//! they use web-sys.
//! The `term` module (VT kernel) stays target-agnostic so unit tests
//! run on the host with `cargo test --lib`.

pub mod backend;
pub mod glyph_atlas;
#[cfg(any(
    all(target_arch = "wasm32", feature = "webgpu"),
    all(test, feature = "webgpu")
))]
pub mod glyph_rasterizer;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod gpu_context;
#[cfg(feature = "webgpu")]
mod gpu_limits;
pub mod renderer;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod surface_host;
pub mod wallpaper;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod webgpu;

/// Aspect-preserving "contain" fit of a glyph bitmap into a cell box.
///
/// Returns `(draw_x, draw_y, draw_w, draw_h)` for the glyph quad. The
/// bitmap (`px_w` × `px_h`, logical px) is uniformly scaled by
/// `s = min(box_w/px_w, box_h/px_h)` so it never exceeds the box on
/// either axis, then centered inside `(anchor + box)`. Because the
/// scaled extent is bounded by the box on both axes, the glyph can
/// never spill into the neighbouring cell — the next character is
/// always safe.
///
/// This is how GPU terminals (Warp / Kitty / WezTerm) keep an emoji —
/// which comes from a square, oversized color font — inside the two
/// cells `wcwidth` reserves for it, instead of drawing it at its raw
/// rasterized advance and overflowing onto the next glyph.
///
/// `allow_upscale = true` lets a glyph smaller than its box grow to
/// fill it (emoji into the near-square 2-cell box). `false` clamps
/// `s <= 1.0` (shrink-only), which leaves a gap around an under-sized
/// glyph but never blurs.
pub fn fit_glyph_box(
    px_w: f32,
    px_h: f32,
    box_w: f32,
    box_h: f32,
    anchor_x: f32,
    anchor_y: f32,
    allow_upscale: bool,
) -> (f32, f32, f32, f32) {
    let nw = px_w.max(1.0);
    let nh = px_h.max(1.0);
    let mut s = (box_w / nw).min(box_h / nh);
    if !allow_upscale {
        s = s.min(1.0);
    }
    let draw_w = nw * s;
    let draw_h = nh * s;
    let draw_x = anchor_x + (box_w - draw_w) * 0.5;
    let draw_y = anchor_y + (box_h - draw_h) * 0.5;
    (draw_x, draw_y, draw_w, draw_h)
}

#[cfg(test)]
mod fit_glyph_box_tests {
    use super::fit_glyph_box;

    // A near-square glyph wider than tall is bounded by width; it must
    // never exceed the box on either axis and must sit centered.
    #[test]
    fn contain_never_exceeds_box_and_centers() {
        // Glyph 40×20 into a 20×20 box → width-bound, s = 0.5.
        let (x, y, w, h) = fit_glyph_box(40.0, 20.0, 20.0, 20.0, 100.0, 200.0, true);
        assert!((w - 20.0).abs() < 1e-3, "w={w}");
        assert!((h - 10.0).abs() < 1e-3, "h={h}");
        assert!(w <= 20.0 + 1e-3 && h <= 20.0 + 1e-3);
        // Centered: x flush (w fills box), y offset by (20-10)/2 = 5.
        assert!((x - 100.0).abs() < 1e-3, "x={x}");
        assert!((y - 205.0).abs() < 1e-3, "y={y}");
    }

    // A tall-narrow glyph is bounded by height.
    #[test]
    fn height_bound_case() {
        // Glyph 10×40 into a 20×20 box → height-bound, s = 0.5.
        let (x, _y, w, h) = fit_glyph_box(10.0, 40.0, 20.0, 20.0, 0.0, 0.0, true);
        assert!((w - 5.0).abs() < 1e-3, "w={w}");
        assert!((h - 20.0).abs() < 1e-3, "h={h}");
        // Centered horizontally: (20-5)/2 = 7.5.
        assert!((x - 7.5).abs() < 1e-3, "x={x}");
    }

    // Upscale enabled grows a small glyph to fill the box.
    #[test]
    fn upscale_fills_box() {
        // Glyph 10×10 into a 20×20 box → s = 2.0 when upscaling allowed.
        let (_x, _y, w, h) = fit_glyph_box(10.0, 10.0, 20.0, 20.0, 0.0, 0.0, true);
        assert!(
            (w - 20.0).abs() < 1e-3 && (h - 20.0).abs() < 1e-3,
            "w={w} h={h}"
        );
    }

    // Upscale disabled clamps s <= 1.0 (shrink-only).
    #[test]
    fn no_upscale_clamps() {
        let (x, y, w, h) = fit_glyph_box(10.0, 10.0, 20.0, 20.0, 0.0, 0.0, false);
        assert!(
            (w - 10.0).abs() < 1e-3 && (h - 10.0).abs() < 1e-3,
            "w={w} h={h}"
        );
        // Centered: (20-10)/2 = 5 on both axes.
        assert!((x - 5.0).abs() < 1e-3 && (y - 5.0).abs() < 1e-3);
    }

    // Degenerate zero dims must not divide-by-zero or NaN.
    #[test]
    fn zero_dims_are_safe() {
        let (x, y, w, h) = fit_glyph_box(0.0, 0.0, 16.0, 16.0, 0.0, 0.0, true);
        assert!(w.is_finite() && h.is_finite() && x.is_finite() && y.is_finite());
        assert!(w <= 16.0 + 1e-3 && h <= 16.0 + 1e-3);
    }
}

// Shared GPU context (Round 3 §4.3 Phase A): one Device / Queue /
// pipeline / atlas for the whole process. Per-pane WebGpuPaneBackend
// borrows it via Rc<RefCell<>> instead of constructing its own copies.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
// Shared swap-chain host (Round 3 §4.3 Phase B): one wgpu::Surface
// bound to the global host canvas in +page.svelte. Per-pane
// WebGpuPaneBackend instances record each pane's draw clipped by its own
// scissor rect. Single submit + present per frame regardless of pane count.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
// Glyph rasterizer (Round 3 §4.1.b). The host supplies selected system-font
// bytes; cosmic-text/Swash rasterizes atlas misses without a browser 2D context.
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub use backend::{CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme};
pub use renderer::Renderer;

// ─── Static WGSL validation (host-target only) ─────────────────────────
//
// `cell.wgsl` is `include_str!`'d into the binary and only validated by
// wgpu at `device.create_shader_module()` time — i.e. inside the
// browser, on the first WebGPU pane attach. A typo there would otherwise
// become a runtime initialization failure surfaced to the pane UI.
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
