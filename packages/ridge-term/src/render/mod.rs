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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Generates procedural rectangles for Box Drawing (U+2500..=U+257F) and
/// Block Elements (U+2580..=U+259F). Returns None if the character is not supported.
///
/// Coverage of the Block Elements range (U+2580..=U+259F):
///   - Half blocks (▀ ▄ ▌ ▐) + full block (█)
///   - Lower N/8 blocks (▁ ▂ ▃ ▅ ▆ ▇)
///   - Left N/8 blocks (▉ ▊ ▋ ▍ ▎ ▏)
///   - Upper 1/8 (▔) + right 1/8 (▕)
///   - Quadrants (▖ ▗ ▘ ▙ ▚ ▛ ▜ ▝ ▞ ▟)
///
/// Shade characters (U+2591..=U+2593) are intentionally NOT handled here —
/// they need an alpha-modulated full-cell quad rather than opaque
/// rectangles, so the caller (`webgpu::draw_row_texts`) special-cases them
/// with a scaled fg alpha before falling through to this lookup.
pub fn procedural_box(c: char, cell_x: f32, cell_y: f32, cell_w: f32, cell_h: f32) -> Option<Vec<Rect>> {
    let mut rects = Vec::with_capacity(2);

    // Procedural drawing: use the exact provided bounds.
    // Rounding and snapping happen in the renderer's pixel-coordinate space,
    // not here, to avoid double-rounding artifacts.
    //
    // LIGHT vs HEAVY stroke widths. Unicode separates U+2500-U+250B/2502-3
    // (light) from U+2501/2503 (heavy) plus the heavy stub set
    // (U+2578..U+257B) — they're meant to render visibly thicker. Earlier
    // versions of this function collapsed both weights onto the same `lw`
    // and `lh`, so opencode's ThickBorder (┃ ╹) drew at the same hairline
    // weight as a normal │ vt100 box. The thicker stroke is what users
    // notice on PowerShell / Windows Terminal too.
    let lw = cell_w * 0.15;
    let lh = cell_h * 0.1;
    let lw_heavy = cell_w * 0.38;
    let lh_heavy = cell_h * 0.28;

    // Centers for line drawing
    let cx = cell_x + (cell_w - lw) / 2.0;
    let cy = cell_y + (cell_h - lh) / 2.0;
    let cy_heavy = cell_y + (cell_h - lh_heavy) / 2.0;
    // HEAVY vertical strokes ┃ ╹ ╻ shift right of the cell centre by the
    // "extra weight" the stroke carries over LIGHT (`lw_heavy - lw`). This
    // partially closes the seam against the cell-right neighbour (opencode
    // ThickBorder's input-box interior) without going all the way to a
    // flush-right placement — which would make `┃` look conspicuously
    // off-centre when used as a plain text character (rare but valid).
    let cx_heavy = cell_x + (cell_w - lw_heavy) / 2.0 + (lw_heavy - lw);

    // Half-cell helpers for the quadrant block characters (U+2596..=U+259F).
    // A quadrant is `cell_w/2 × cell_h/2` anchored at one of the four
    // corners of the cell.
    let hw = cell_w * 0.5;
    let hh = cell_h * 0.5;
    let q_tl = Rect { x: cell_x,      y: cell_y,      w: hw, h: hh }; // top-left
    let q_tr = Rect { x: cell_x + hw, y: cell_y,      w: hw, h: hh }; // top-right
    let q_bl = Rect { x: cell_x,      y: cell_y + hh, w: hw, h: hh }; // bottom-left
    let q_br = Rect { x: cell_x + hw, y: cell_y + hh, w: hw, h: hh }; // bottom-right

    match c {
        // --- Block Elements (U+2580 - U+259F) ---
        '\u{2588}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w, h: cell_h }), // Full block
        '\u{2580}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w, h: cell_h / 2.0 }), // Upper half block
        '\u{2584}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h / 2.0, w: cell_w, h: cell_h / 2.0 }), // Lower half block
        '\u{258C}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w / 2.0, h: cell_h }), // Left half block
        '\u{2590}' => rects.push(Rect { x: cell_x + cell_w / 2.0, y: cell_y, w: cell_w / 2.0, h: cell_h }), // Right half block
        '\u{2581}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h * 0.875, w: cell_w, h: cell_h * 0.125 }), // Lower one eighth
        '\u{2582}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h * 0.75, w: cell_w, h: cell_h * 0.25 }), // Lower one quarter
        '\u{2583}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h * 0.625, w: cell_w, h: cell_h * 0.375 }), // Lower three eighths
        '\u{2585}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h * 0.375, w: cell_w, h: cell_h * 0.625 }), // Lower five eighths
        '\u{2586}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h * 0.25, w: cell_w, h: cell_h * 0.75 }), // Lower three quarters
        '\u{2587}' => rects.push(Rect { x: cell_x, y: cell_y + cell_h * 0.125, w: cell_w, h: cell_h * 0.875 }), // Lower seven eighths

        // Left N/8 blocks — grow leftward as N increases. ▉ is 7/8 wide
        // (mirror of ▁), ▏ is 1/8 wide (mirror of ▔).
        '\u{2589}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w * 0.875, h: cell_h }),
        '\u{258A}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w * 0.75,  h: cell_h }),
        '\u{258B}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w * 0.625, h: cell_h }),
        '\u{258D}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w * 0.375, h: cell_h }),
        '\u{258E}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w * 0.25,  h: cell_h }),
        '\u{258F}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w * 0.125, h: cell_h }),

        // Upper 1/8 (▔) and right 1/8 (▕).
        '\u{2594}' => rects.push(Rect { x: cell_x, y: cell_y, w: cell_w, h: cell_h * 0.125 }),
        '\u{2595}' => rects.push(Rect { x: cell_x + cell_w * 0.875, y: cell_y, w: cell_w * 0.125, h: cell_h }),

        // Quadrant blocks (U+2596..=U+259F) — 1, 2 or 3 quarter-cell rects.
        '\u{2596}' => rects.push(q_bl),                              // ▖ lower-left
        '\u{2597}' => rects.push(q_br),                              // ▗ lower-right
        '\u{2598}' => rects.push(q_tl),                              // ▘ upper-left
        '\u{2599}' => { rects.push(q_tl); rects.push(q_bl); rects.push(q_br); } // ▙ all except upper-right
        '\u{259A}' => { rects.push(q_tl); rects.push(q_br); }        // ▚ diagonal (TL+BR)
        '\u{259B}' => { rects.push(q_tl); rects.push(q_tr); rects.push(q_bl); } // ▛ all except lower-right
        '\u{259C}' => { rects.push(q_tl); rects.push(q_tr); rects.push(q_br); } // ▜ all except lower-left
        '\u{259D}' => rects.push(q_tr),                              // ▝ upper-right
        '\u{259E}' => { rects.push(q_tr); rects.push(q_bl); }        // ▞ diagonal (TR+BL)
        '\u{259F}' => { rects.push(q_tr); rects.push(q_bl); rects.push(q_br); } // ▟ all except upper-left


        // --- Box Drawing (U+2500 - U+257F) Core set ---
        // For straight horizontal / vertical lines we extend the rect by
        // 1 procedural-px past the cell boundary on the "outgoing" side.
        // webgpu.rs::draw_row_texts pixel-snaps these to device-px integer
        // bounds, so the +1 lands as a 1 device-px overlap with the next
        // cell's rect. Without it, opencode-style multi-row ┃ stacks
        // (gocui / charmbracelet ThickBorder = ┃ left + ╹ bottom-left)
        // showed a visible gap between cells on a subset of GPUs — the
        // mathematically-exact tile boundary at `(N+1) * cell_h_dev`
        // rasterised to no pixel coverage on neither row's quad. The
        // overlap is invisible (both quads paint the same fg color) and
        // restores seam-free vertical / horizontal runs.
        '\u{2500}' => rects.push(Rect { x: cell_x, y: cy,       w: cell_w + 1.0, h: lh }),       // ─ LIGHT
        '\u{2501}' => rects.push(Rect { x: cell_x, y: cy_heavy, w: cell_w + 1.0, h: lh_heavy }), // ━ HEAVY
        '\u{2502}' => rects.push(Rect { x: cx,             y: cell_y, w: lw,           h: cell_h + 1.0 }), // │ LIGHT (centred)
        '\u{2503}' => rects.push(Rect { x: cx_heavy, y: cell_y, w: lw_heavy, h: cell_h + 1.0 }), // ┃ HEAVY (centred)

        // Stub-ends (U+2574..U+257B) — single-direction half-cell lines.
        // LIGHT variants (╴╵╶╷) use the thin stroke, HEAVY variants
        // (╸╹╺╻) the thick one. Each stub extends by 1 procedural-px
        // toward the adjacent cell to overlap with whatever continues
        // the line (matches the ─/│ overlap).
        // LIGHT left/right (horizontal stub, thin)
        '\u{2574}' => rects.push(Rect { x: cell_x,                 y: cy, w: cell_w / 2.0 + 1.0, h: lh }), // ╴
        '\u{2576}' => rects.push(Rect { x: cell_x + cell_w / 2.0,  y: cy, w: cell_w / 2.0 + 1.0, h: lh }), // ╶
        // HEAVY left/right (horizontal stub, thick)
        '\u{2578}' => rects.push(Rect { x: cell_x,                 y: cy_heavy, w: cell_w / 2.0 + 1.0, h: lh_heavy }), // ╸
        '\u{257A}' => rects.push(Rect { x: cell_x + cell_w / 2.0,  y: cy_heavy, w: cell_w / 2.0 + 1.0, h: lh_heavy }), // ╺
        // LIGHT up/down (vertical stub, thin)
        '\u{2575}' => rects.push(Rect { x: cx, y: cell_y,                 w: lw, h: cell_h / 2.0 + 1.0 }), // ╵
        '\u{2577}' => rects.push(Rect { x: cx, y: cell_y + cell_h / 2.0,  w: lw, h: cell_h / 2.0 + 1.0 }), // ╷
        // HEAVY up/down (vertical stub, thick) — centred like ┃ so they
        // align with ┃ above/below in a vertical chain (opencode's
        // L-shape input box draws ╹ as the bottom-left corner attached
        // to a column of ┃; centred ╹ keeps the column straight).
        '\u{2579}' => rects.push(Rect { x: cx_heavy, y: cell_y,                 w: lw_heavy, h: cell_h / 2.0 + 1.0 }), // ╹
        '\u{257B}' => rects.push(Rect { x: cx_heavy, y: cell_y + cell_h / 2.0,  w: lw_heavy, h: cell_h / 2.0 + 1.0 }), // ╻
        
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
        // Rounded corners ╭ ╮ ╯ ╰ (U+256D..U+2570) intentionally fall
        // through to atlas rendering: a procedural rect can't draw a true
        // radius, and forcing them to share sharp-corner geometry visibly
        // degraded every TUI that uses lipgloss/bubbletea defaults
        // (lazygit, gh, opencode) where the rounded edge is part of the
        // design language. Atlas rendering preserves the radius; the
        // matching ┃/│ run terminates at the rounded corner with a +1 px
        // overlap (see 2500..2503 above) so the seam stays gap-free.
        
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
        
        '\u{253C}' | '\u{253D}' | '\u{253E}' | '\u{253F}' | '\u{2540}' | '\u{2541}' | '\u{2542}' | '\u{2543}' | '\u{2544}' | '\u{2545}' | '\u{2546}' | '\u{2547}' | '\u{2548}' | '\u{2549}' | '\u{254A}' | '\u{254B}' => {
            rects.push(Rect { x: cell_x, y: cy, w: cell_w, h: lh }); 
            rects.push(Rect { x: cx, y: cell_y, w: lw, h: cell_h });
        }
        
        _ => return None,
    }
    Some(rects)
}

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
/// fill it (emoji into the near-square 2-cell box); the atlas is
/// supersampled (`ATLAS_SUPERSAMPLE`) so a modest upscale stays crisp.
/// `false` clamps `s <= 1.0` (shrink-only), which leaves a gap around
/// an under-sized glyph but never blurs.
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
        assert!((w - 20.0).abs() < 1e-3 && (h - 20.0).abs() < 1e-3, "w={w} h={h}");
    }

    // Upscale disabled clamps s <= 1.0 (shrink-only).
    #[test]
    fn no_upscale_clamps() {
        let (x, y, w, h) = fit_glyph_box(10.0, 10.0, 20.0, 20.0, 0.0, 0.0, false);
        assert!((w - 10.0).abs() < 1e-3 && (h - 10.0).abs() < 1e-3, "w={w} h={h}");
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

    /// §atlas-pin: protect a cached pane's glyph layers from mid-frame
    /// eviction by another pane's glyph admission. No-op for Canvas2D
    /// (no shared atlas / `frame_written` mask).
    pub fn pin_cached_layers(&mut self) {
        match self {
            AnyBackend::Canvas2d(_) => {}
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.pin_cached_layers(),
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

    fn draw_preedit_overlay(
        &mut self,
        text: &str,
        row: usize,
        col: usize,
        theme: &Theme,
    ) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_preedit_overlay(text, row, col, theme),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_preedit_overlay(text, row, col, theme),
        }
    }

    fn draw_history_overlay(
        &mut self,
        overlay: &crate::render::renderer::HistoryOverlay,
        theme: &Theme,
    ) {
        match self {
            AnyBackend::Canvas2d(b) => b.draw_history_overlay(overlay, theme),
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.draw_history_overlay(overlay, theme),
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

#[cfg(test)]
mod procedural_box_tests {
    use super::{procedural_box, Rect};

    // Unit-cell bounds keep the assertions simple: every fraction maps to
    // an exact f32 with no rounding required.
    const CX: f32 = 0.0;
    const CY: f32 = 0.0;
    const CW: f32 = 8.0;
    const CH: f32 = 16.0;

    fn box_for(c: char) -> Vec<Rect> {
        procedural_box(c, CX, CY, CW, CH).unwrap_or_else(|| panic!("char {:?} should be procedurally drawn", c))
    }

    /// Full block must paint a single cell-sized rect — the renderer
    /// relies on this for solid-block run-length output (btop CPU bars
    /// at 100%, `printf "█"` smoke tests).
    #[test]
    fn full_block_covers_entire_cell() {
        let rects = box_for('\u{2588}');
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        assert_eq!((r.x, r.y, r.w, r.h), (CX, CY, CW, CH));
    }

    /// Left N/8 blocks: ▉ (7/8) ▊ (6/8) ▋ (5/8) ▍ (3/8) ▎ (2/8) ▏ (1/8).
    /// All anchor at the cell's left edge and extend rightward; height
    /// is the full cell. Regression guard for the 2026-05 procedural_box
    /// gap that left these characters falling through to atlas / font
    /// glyphs and rendering at the wrong size in btop / mc.
    #[test]
    fn left_eighth_blocks_anchor_left_and_scale_width() {
        for (ch, fraction) in [
            ('\u{2589}', 0.875),
            ('\u{258A}', 0.75),
            ('\u{258B}', 0.625),
            ('\u{258D}', 0.375),
            ('\u{258E}', 0.25),
            ('\u{258F}', 0.125),
        ] {
            let rects = box_for(ch);
            assert_eq!(rects.len(), 1, "{:?} should be one rect", ch);
            let r = rects[0];
            assert_eq!(r.x, CX, "{:?} x", ch);
            assert_eq!(r.y, CY, "{:?} y", ch);
            assert!((r.w - CW * fraction).abs() < 1e-3, "{:?} w expected {} got {}", ch, CW * fraction, r.w);
            assert_eq!(r.h, CH, "{:?} h", ch);
        }
    }

    /// ▔ upper 1/8 — top strip; ▕ right 1/8 — right strip. Symmetric
    /// counterparts to the existing ▁ and ▏.
    #[test]
    fn upper_and_right_one_eighth_blocks() {
        let upper = box_for('\u{2594}');
        assert_eq!(upper.len(), 1);
        assert!((upper[0].h - CH * 0.125).abs() < 1e-3);
        assert_eq!(upper[0].y, CY);
        assert_eq!(upper[0].w, CW);

        let right = box_for('\u{2595}');
        assert_eq!(right.len(), 1);
        assert!((right[0].w - CW * 0.125).abs() < 1e-3);
        assert!((right[0].x - (CX + CW * 0.875)).abs() < 1e-3);
        assert_eq!(right[0].h, CH);
    }

    /// Single-quadrant blocks: ▖ ▗ ▘ ▝ — exactly one half-cell rect,
    /// positioned at one of the four corners.
    #[test]
    fn single_quadrant_blocks_use_half_cell_corner() {
        let hw = CW * 0.5;
        let hh = CH * 0.5;
        // ▘ top-left
        let r = box_for('\u{2598}');
        assert_eq!(r, vec![Rect { x: CX, y: CY, w: hw, h: hh }]);
        // ▝ top-right
        let r = box_for('\u{259D}');
        assert_eq!(r, vec![Rect { x: CX + hw, y: CY, w: hw, h: hh }]);
        // ▖ bottom-left
        let r = box_for('\u{2596}');
        assert_eq!(r, vec![Rect { x: CX, y: CY + hh, w: hw, h: hh }]);
        // ▗ bottom-right
        let r = box_for('\u{2597}');
        assert_eq!(r, vec![Rect { x: CX + hw, y: CY + hh, w: hw, h: hh }]);
    }

    /// Diagonal quadrants (▚ TL+BR, ▞ TR+BL) emit exactly two rects
    /// covering opposite corners — they must NOT overlap.
    #[test]
    fn diagonal_quadrant_blocks_emit_two_opposite_corners() {
        let r = box_for('\u{259A}'); // ▚
        assert_eq!(r.len(), 2);
        // Either ordering is fine; sort by x for stability.
        let mut xs: Vec<f32> = r.iter().map(|q| q.x).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(xs, vec![CX, CX + CW * 0.5]);

        let r = box_for('\u{259E}'); // ▞
        assert_eq!(r.len(), 2);
    }

    /// Three-quadrant blocks (▙ ▛ ▜ ▟) — exactly three half-cell rects
    /// covering all corners except one.
    #[test]
    fn three_quadrant_blocks_emit_three_rects() {
        for ch in ['\u{2599}', '\u{259B}', '\u{259C}', '\u{259F}'] {
            assert_eq!(box_for(ch).len(), 3, "{:?}", ch);
        }
    }

    /// Half blocks: ▀ (upper) and ▄ (lower) cover half the cell height.
    #[test]
    fn half_blocks_cover_half_cell_height() {
        let upper = box_for('\u{2580}');
        assert_eq!(upper.len(), 1);
        assert_eq!(upper[0].h, CH * 0.5);
        assert_eq!(upper[0].y, CY);

        let lower = box_for('\u{2584}');
        assert_eq!(lower.len(), 1);
        assert_eq!(lower[0].h, CH * 0.5);
        assert_eq!(lower[0].y, CY + CH * 0.5);
    }

    /// Shade characters (U+2591..=U+2593) are intentionally returned as
    /// `None` so the renderer's alpha-modulated path takes over —
    /// otherwise we'd over-paint with opaque rectangles and lose the
    /// shading effect entirely. Regression guard.
    #[test]
    fn shade_chars_return_none() {
        assert!(procedural_box('\u{2591}', CX, CY, CW, CH).is_none());
        assert!(procedural_box('\u{2592}', CX, CY, CW, CH).is_none());
        assert!(procedural_box('\u{2593}', CX, CY, CW, CH).is_none());
    }

    /// Out-of-coverage chars (regular ASCII, CJK, emoji) must fall back
    /// to the atlas path — i.e. `procedural_box` returns `None` so the
    /// caller's `if let Some(rects) = …` branch is skipped.
    #[test]
    fn non_block_chars_return_none() {
        assert!(procedural_box('a', CX, CY, CW, CH).is_none());
        assert!(procedural_box('中', CX, CY, CW, CH).is_none());
        assert!(procedural_box('😀', CX, CY, CW, CH).is_none());
    }

    /// Rounded corners ╭ ╮ ╯ ╰ (U+256D..U+2570) MUST fall through to atlas
    /// rendering — procedural sharp-rect approximation discards the radius
    /// and is a visible regression for lipgloss/bubbletea/lazygit/gh UIs.
    /// Regression guard against re-introducing the procedural mapping.
    #[test]
    fn rounded_corners_fall_through_to_atlas() {
        for ch in ['\u{256D}', '\u{256E}', '\u{256F}', '\u{2570}'] {
            assert!(
                procedural_box(ch, CX, CY, CW, CH).is_none(),
                "{:?} must return None so the atlas path renders the radius",
                ch
            );
        }
    }

    /// Straight ─/━/│/┃ runs must overlap their neighbour cell by 1
    /// procedural-px on the continuation axis. opencode draws a ThickBorder
    /// frame as a vertical stack of ┃ cells finishing at a ╹ stub; without
    /// the overlap the device-px tile boundary at `(N+1) * cell_h_dev`
    /// rasterised to zero coverage on a subset of GPUs, producing the
    /// visible gap users reported between adjacent ┃ cells. Regression
    /// guard so a future refactor doesn't drop the +1.
    #[test]
    fn straight_lines_extend_past_cell_boundary_by_one_px() {
        for ch in ['\u{2500}', '\u{2501}'] {
            let h = box_for(ch);
            assert_eq!(h.len(), 1, "{:?}", ch);
            assert_eq!(h[0].w, CW + 1.0, "{:?} must extend +1px rightward", ch);
        }
        for ch in ['\u{2502}', '\u{2503}'] {
            let v = box_for(ch);
            assert_eq!(v.len(), 1, "{:?}", ch);
            assert_eq!(v[0].h, CH + 1.0, "{:?} must extend +1px downward", ch);
        }
    }

    /// HEAVY variants ━/┃ and the HEAVY stub set ╸╹╺╻ must render with
    /// a visibly thicker stroke than their LIGHT counterparts ─/│/╴╵╶╷.
    /// Earlier this function collapsed both weights onto the same `lw`/`lh`,
    /// so opencode's ThickBorder looked identical to a vt100 │ — the user
    /// expected the heavier line they get in PowerShell / Windows Terminal.
    #[test]
    fn heavy_strokes_are_thicker_than_light() {
        let light_h = box_for('\u{2500}')[0].h; // ─
        let heavy_h = box_for('\u{2501}')[0].h; // ━
        assert!(heavy_h > light_h, "━ ({}) must be thicker than ─ ({})", heavy_h, light_h);

        let light_w = box_for('\u{2502}')[0].w; // │
        let heavy_w = box_for('\u{2503}')[0].w; // ┃
        assert!(heavy_w > light_w, "┃ ({}) must be thicker than │ ({})", heavy_w, light_w);

        // Heavy vertical stubs (╹╻) thicker than light (╵╷).
        assert!(box_for('\u{2579}')[0].w > box_for('\u{2575}')[0].w, "╹ vs ╵");
        assert!(box_for('\u{257B}')[0].w > box_for('\u{2577}')[0].w, "╻ vs ╷");
        // Heavy horizontal stubs (╸╺) thicker than light (╴╶).
        assert!(box_for('\u{2578}')[0].h > box_for('\u{2574}')[0].h, "╸ vs ╴");
        assert!(box_for('\u{257A}')[0].h > box_for('\u{2576}')[0].h, "╺ vs ╶");
    }

    /// Stub characters (U+2574..U+257B) — ╴╵╶╷╸╹╺╻ — must produce
    /// procedural rects (not None / atlas fallback) anchored at the
    /// correct half-cell edge with +1 px overlap toward the line they
    /// terminate. Stroke-thickness vs light/heavy separation is covered
    /// by `heavy_strokes_are_thicker_than_light`; this test just checks
    /// position/anchor.
    #[test]
    fn stub_chars_have_procedural_half_cell_geometry() {
        // Point UP — anchored at cell top, half-cell height + 1 overlap.
        for ch in ['\u{2575}', '\u{2579}'] {
            let r = box_for(ch);
            assert_eq!(r.len(), 1, "{:?}", ch);
            assert_eq!(r[0].y, CY, "{:?} y", ch);
            assert_eq!(r[0].h, CH / 2.0 + 1.0, "{:?} h", ch);
        }
        // Point DOWN — anchored at cell midline.
        for ch in ['\u{2577}', '\u{257B}'] {
            let r = box_for(ch);
            assert_eq!(r[0].y, CY + CH / 2.0, "{:?} y", ch);
            assert_eq!(r[0].h, CH / 2.0 + 1.0, "{:?} h", ch);
        }
        // Point LEFT — anchored at cell-left.
        for ch in ['\u{2574}', '\u{2578}'] {
            let r = box_for(ch);
            assert_eq!(r[0].x, CX, "{:?} x", ch);
            assert_eq!(r[0].w, CW / 2.0 + 1.0, "{:?} w", ch);
        }
        // Point RIGHT — anchored at cell midline.
        for ch in ['\u{2576}', '\u{257A}'] {
            let r = box_for(ch);
            assert_eq!(r[0].x, CX + CW / 2.0, "{:?} x", ch);
            assert_eq!(r[0].w, CW / 2.0 + 1.0, "{:?} w", ch);
        }
    }
}
