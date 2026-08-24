//! Rendering layer.
//!
//! WebGPU presentation modules are gated on `target_arch = "wasm32"` because
//! they use web-sys.
//! The `term` module (VT kernel) stays target-agnostic so unit tests
//! run on the host with `cargo test --lib`.

pub mod backend;
pub mod glyph_atlas;
#[cfg(feature = "webgpu")]
pub mod glyph_rasterizer;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod gpu_context;
pub mod renderer;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod surface_host;
pub mod wallpaper;
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub mod webgpu;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Generates procedural rectangles for the common core of Box Drawing
/// (U+2500..=U+257F) and Block Elements (U+2580..=U+259F).
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
#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

fn append_rounded_corner(
    rects: &mut Vec<Rect>,
    corner: Corner,
    cell_x: f32,
    cell_y: f32,
    cell_w: f32,
    cell_h: f32,
    line_w: f32,
    line_h: f32,
) {
    let radius = (cell_w.min(cell_h) * 0.42).max(line_w.max(line_h) * 1.5);
    let cx = cell_x + (cell_w - line_w) / 2.0;
    let cy = cell_y + (cell_h - line_h) / 2.0;
    let (center_x, center_y, start, end, from_left, from_top) = match corner {
        Corner::TopLeft => (
            cx + radius,
            cy + radius,
            std::f32::consts::PI,
            std::f32::consts::PI * 1.5,
            false,
            false,
        ),
        Corner::TopRight => (
            cx + line_w - radius,
            cy + radius,
            std::f32::consts::PI * 1.5,
            std::f32::consts::PI * 2.0,
            true,
            false,
        ),
        Corner::BottomRight => (
            cx + line_w - radius,
            cy + line_h - radius,
            0.0,
            std::f32::consts::PI * 0.5,
            true,
            true,
        ),
        Corner::BottomLeft => (
            cx + radius,
            cy + line_h - radius,
            std::f32::consts::PI * 0.5,
            std::f32::consts::PI,
            false,
            true,
        ),
    };
    if from_left {
        rects.push(Rect {
            x: cell_x,
            y: cy,
            w: (center_x - cell_x).max(line_w),
            h: line_h,
        });
    } else {
        rects.push(Rect {
            x: center_x,
            y: cy,
            w: (cell_x + cell_w - center_x).max(line_w) + 1.0,
            h: line_h,
        });
    }
    if from_top {
        rects.push(Rect {
            x: cx,
            y: cell_y,
            w: line_w,
            h: (center_y - cell_y).max(line_h),
        });
    } else {
        rects.push(Rect {
            x: cx,
            y: center_y,
            w: line_w,
            h: (cell_y + cell_h - center_y).max(line_h) + 1.0,
        });
    }
    let steps = 6_u32;
    let span = end - start;
    for i in 0..=steps {
        let t = start + span * (i as f32 / steps as f32);
        rects.push(Rect {
            x: center_x + radius * t.cos() - line_w * 0.5,
            y: center_y + radius * t.sin() - line_h * 0.5,
            w: line_w,
            h: line_h,
        });
    }
}

pub fn procedural_box(
    c: char,
    cell_x: f32,
    cell_y: f32,
    cell_w: f32,
    cell_h: f32,
) -> Option<Vec<Rect>> {
    let mut rects = Vec::with_capacity(2);

    // Keep line art independent of the selected font. Some Windows font
    // stacks render a missing box glyph as a large hollow square.
    let line_w = (cell_w * 0.22).max(2.0);
    let line_h = (cell_h * 0.14).max(2.0);
    let heavy_w = cell_w * 0.38;
    let heavy_h = cell_h * 0.28;
    let cx = cell_x + (cell_w - line_w) / 2.0;
    let cy = cell_y + (cell_h - line_h) / 2.0;
    let cy_heavy = cell_y + (cell_h - heavy_h) / 2.0;
    let cx_heavy = cell_x + (cell_w - heavy_w) / 2.0 + (heavy_w - line_w);

    // Procedural drawing: use the exact provided bounds.
    // Rounding and snapping happen in the renderer's pixel-coordinate space,
    // not here, to avoid double-rounding artifacts.
    // Half-cell helpers for the quadrant block characters (U+2596..=U+259F).
    // A quadrant is `cell_w/2 × cell_h/2` anchored at one of the four
    // corners of the cell.
    let hw = cell_w * 0.5;
    let hh = cell_h * 0.5;
    let q_tl = Rect {
        x: cell_x,
        y: cell_y,
        w: hw,
        h: hh,
    }; // top-left
    let q_tr = Rect {
        x: cell_x + hw,
        y: cell_y,
        w: hw,
        h: hh,
    }; // top-right
    let q_bl = Rect {
        x: cell_x,
        y: cell_y + hh,
        w: hw,
        h: hh,
    }; // bottom-left
    let q_br = Rect {
        x: cell_x + hw,
        y: cell_y + hh,
        w: hw,
        h: hh,
    }; // bottom-right

    match c {
        // --- Block Elements (U+2580 - U+259F) ---
        '\u{2588}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w,
            h: cell_h,
        }), // Full block
        '\u{2580}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w,
            h: cell_h / 2.0,
        }), // Upper half block
        '\u{2584}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h / 2.0,
            w: cell_w,
            h: cell_h / 2.0,
        }), // Lower half block
        '\u{258C}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w / 2.0,
            h: cell_h,
        }), // Left half block
        '\u{2590}' => rects.push(Rect {
            x: cell_x + cell_w / 2.0,
            y: cell_y,
            w: cell_w / 2.0,
            h: cell_h,
        }), // Right half block
        '\u{2581}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h * 0.875,
            w: cell_w,
            h: cell_h * 0.125,
        }), // Lower one eighth
        '\u{2582}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h * 0.75,
            w: cell_w,
            h: cell_h * 0.25,
        }), // Lower one quarter
        '\u{2583}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h * 0.625,
            w: cell_w,
            h: cell_h * 0.375,
        }), // Lower three eighths
        '\u{2585}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h * 0.375,
            w: cell_w,
            h: cell_h * 0.625,
        }), // Lower five eighths
        '\u{2586}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h * 0.25,
            w: cell_w,
            h: cell_h * 0.75,
        }), // Lower three quarters
        '\u{2587}' => rects.push(Rect {
            x: cell_x,
            y: cell_y + cell_h * 0.125,
            w: cell_w,
            h: cell_h * 0.875,
        }), // Lower seven eighths

        // Left N/8 blocks — grow leftward as N increases. ▉ is 7/8 wide
        // (mirror of ▁), ▏ is 1/8 wide (mirror of ▔).
        '\u{2589}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w * 0.875,
            h: cell_h,
        }),
        '\u{258A}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w * 0.75,
            h: cell_h,
        }),
        '\u{258B}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w * 0.625,
            h: cell_h,
        }),
        '\u{258D}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w * 0.375,
            h: cell_h,
        }),
        '\u{258E}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w * 0.25,
            h: cell_h,
        }),
        '\u{258F}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w * 0.125,
            h: cell_h,
        }),

        // Upper 1/8 (▔) and right 1/8 (▕).
        '\u{2594}' => rects.push(Rect {
            x: cell_x,
            y: cell_y,
            w: cell_w,
            h: cell_h * 0.125,
        }),
        '\u{2595}' => rects.push(Rect {
            x: cell_x + cell_w * 0.875,
            y: cell_y,
            w: cell_w * 0.125,
            h: cell_h,
        }),

        // Quadrant blocks (U+2596..=U+259F) — 1, 2 or 3 quarter-cell rects.
        '\u{2596}' => rects.push(q_bl), // ▖ lower-left
        '\u{2597}' => rects.push(q_br), // ▗ lower-right
        '\u{2598}' => rects.push(q_tl), // ▘ upper-left
        '\u{2599}' => {
            rects.push(q_tl);
            rects.push(q_bl);
            rects.push(q_br);
        } // ▙ all except upper-right
        '\u{259A}' => {
            rects.push(q_tl);
            rects.push(q_br);
        } // ▚ diagonal (TL+BR)
        '\u{259B}' => {
            rects.push(q_tl);
            rects.push(q_tr);
            rects.push(q_bl);
        } // ▛ all except lower-right
        '\u{259C}' => {
            rects.push(q_tl);
            rects.push(q_tr);
            rects.push(q_br);
        } // ▜ all except lower-left
        '\u{259D}' => rects.push(q_tr), // ▝ upper-right
        '\u{259E}' => {
            rects.push(q_tr);
            rects.push(q_bl);
        } // ▞ diagonal (TR+BL)
        '\u{259F}' => {
            rects.push(q_tr);
            rects.push(q_bl);
            rects.push(q_br);
        } // ▟ all except upper-left

        // Core Box Drawing characters. Extend straight strokes by one pixel
        // toward the next cell so adjacent rows/columns cannot show seams.
        '\u{2500}' => rects.push(Rect {
            x: cell_x,
            y: cy,
            w: cell_w + 1.0,
            h: line_h,
        }),
        '\u{2501}' => rects.push(Rect {
            x: cell_x,
            y: cy_heavy,
            w: cell_w + 1.0,
            h: heavy_h,
        }),
        '\u{2502}' => rects.push(Rect {
            x: cx,
            y: cell_y,
            w: line_w,
            h: cell_h + 1.0,
        }),
        '\u{2503}' => rects.push(Rect {
            x: cx_heavy,
            y: cell_y,
            w: heavy_w,
            h: cell_h + 1.0,
        }),

        // Light/heavy half-cell stubs.
        '\u{2574}' => rects.push(Rect {
            x: cell_x,
            y: cy,
            w: cell_w / 2.0 + 1.0,
            h: line_h,
        }),
        '\u{2576}' => rects.push(Rect {
            x: cell_x + cell_w / 2.0,
            y: cy,
            w: cell_w / 2.0 + 1.0,
            h: line_h,
        }),
        '\u{2578}' => rects.push(Rect {
            x: cell_x,
            y: cy_heavy,
            w: cell_w / 2.0 + 1.0,
            h: heavy_h,
        }),
        '\u{257A}' => rects.push(Rect {
            x: cell_x + cell_w / 2.0,
            y: cy_heavy,
            w: cell_w / 2.0 + 1.0,
            h: heavy_h,
        }),
        '\u{2575}' => rects.push(Rect {
            x: cx,
            y: cell_y,
            w: line_w,
            h: cell_h / 2.0 + 1.0,
        }),
        '\u{2577}' => rects.push(Rect {
            x: cx,
            y: cell_y + cell_h / 2.0,
            w: line_w,
            h: cell_h / 2.0 + 1.0,
        }),
        '\u{2579}' => rects.push(Rect {
            x: cx_heavy,
            y: cell_y,
            w: heavy_w,
            h: cell_h / 2.0 + 1.0,
        }),
        '\u{257B}' => rects.push(Rect {
            x: cx_heavy,
            y: cell_y + cell_h / 2.0,
            w: heavy_w,
            h: cell_h / 2.0 + 1.0,
        }),

        // Corners and junctions share connection geometry. This path avoids
        // font fallback producing an oversized .notdef square.
        '\u{250C}' | '\u{250D}' | '\u{250E}' | '\u{250F}' => {
            rects.push(Rect {
                x: cx,
                y: cy,
                w: cell_w - (cx - cell_x),
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cy,
                w: line_w,
                h: cell_h - (cy - cell_y),
            });
        }
        '\u{2510}' | '\u{2511}' | '\u{2512}' | '\u{2513}' => {
            rects.push(Rect {
                x: cell_x,
                y: cy,
                w: cx - cell_x + line_w,
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cy,
                w: line_w,
                h: cell_h - (cy - cell_y),
            });
        }
        '\u{2514}' | '\u{2515}' | '\u{2516}' | '\u{2517}' => {
            rects.push(Rect {
                x: cx,
                y: cy,
                w: cell_w - (cx - cell_x),
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cell_y,
                w: line_w,
                h: cy - cell_y + line_h,
            });
        }
        '\u{2518}' | '\u{2519}' | '\u{251A}' | '\u{251B}' => {
            rects.push(Rect {
                x: cell_x,
                y: cy,
                w: cx - cell_x + line_w,
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cell_y,
                w: line_w,
                h: cy - cell_y + line_h,
            });
        }
        '\u{256D}' => append_rounded_corner(
            &mut rects,
            Corner::TopLeft,
            cell_x,
            cell_y,
            cell_w,
            cell_h,
            line_w,
            line_h,
        ),
        '\u{256E}' => append_rounded_corner(
            &mut rects,
            Corner::TopRight,
            cell_x,
            cell_y,
            cell_w,
            cell_h,
            line_w,
            line_h,
        ),
        '\u{256F}' => append_rounded_corner(
            &mut rects,
            Corner::BottomRight,
            cell_x,
            cell_y,
            cell_w,
            cell_h,
            line_w,
            line_h,
        ),
        '\u{2570}' => append_rounded_corner(
            &mut rects,
            Corner::BottomLeft,
            cell_x,
            cell_y,
            cell_w,
            cell_h,
            line_w,
            line_h,
        ),
        '\u{251C}' | '\u{251D}' | '\u{251E}' | '\u{251F}' | '\u{2520}' | '\u{2521}'
        | '\u{2522}' | '\u{2523}' => {
            rects.push(Rect {
                x: cx,
                y: cell_y,
                w: line_w,
                h: cell_h,
            });
            rects.push(Rect {
                x: cx,
                y: cy,
                w: cell_w - (cx - cell_x),
                h: line_h,
            });
        }
        '\u{2524}' | '\u{2525}' | '\u{2526}' | '\u{2527}' | '\u{2528}' | '\u{2529}'
        | '\u{252A}' | '\u{252B}' => {
            rects.push(Rect {
                x: cx,
                y: cell_y,
                w: line_w,
                h: cell_h,
            });
            rects.push(Rect {
                x: cell_x,
                y: cy,
                w: cx - cell_x + line_w,
                h: line_h,
            });
        }
        '\u{252C}' | '\u{252D}' | '\u{252E}' | '\u{252F}' | '\u{2530}' | '\u{2531}'
        | '\u{2532}' | '\u{2533}' => {
            rects.push(Rect {
                x: cell_x,
                y: cy,
                w: cell_w,
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cy,
                w: line_w,
                h: cell_h - (cy - cell_y),
            });
        }
        '\u{2534}' | '\u{2535}' | '\u{2536}' | '\u{2537}' | '\u{2538}' | '\u{2539}'
        | '\u{253A}' | '\u{253B}' => {
            rects.push(Rect {
                x: cell_x,
                y: cy,
                w: cell_w,
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cell_y,
                w: line_w,
                h: cy - cell_y + line_h,
            });
        }
        '\u{253C}' | '\u{253D}' | '\u{253E}' | '\u{253F}' | '\u{2540}' | '\u{2541}'
        | '\u{2542}' | '\u{2543}' | '\u{2544}' | '\u{2545}' | '\u{2546}' | '\u{2547}'
        | '\u{2548}' | '\u{2549}' | '\u{254A}' | '\u{254B}' => {
            rects.push(Rect {
                x: cell_x,
                y: cy,
                w: cell_w,
                h: line_h,
            });
            rects.push(Rect {
                x: cx,
                y: cell_y,
                w: line_w,
                h: cell_h,
            });
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
// Glyph rasterizer (Round 3 §4.1.b). Uses a hidden DOM canvas only as
// selected-system-font glyph raster input feeding the WebGPU atlas; it is
// never a presentation backend and adds no extra wasm bundle weight.
// Owned by the shared WebGPU cache-miss path; gated on the same wasm32 +
// webgpu feature combination.
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
        procedural_box(c, CX, CY, CW, CH)
            .unwrap_or_else(|| panic!("char {:?} should be procedurally drawn", c))
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
            assert!(
                (r.w - CW * fraction).abs() < 1e-3,
                "{:?} w expected {} got {}",
                ch,
                CW * fraction,
                r.w
            );
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
        assert_eq!(
            r,
            vec![Rect {
                x: CX,
                y: CY,
                w: hw,
                h: hh
            }]
        );
        // ▝ top-right
        let r = box_for('\u{259D}');
        assert_eq!(
            r,
            vec![Rect {
                x: CX + hw,
                y: CY,
                w: hw,
                h: hh
            }]
        );
        // ▖ bottom-left
        let r = box_for('\u{2596}');
        assert_eq!(
            r,
            vec![Rect {
                x: CX,
                y: CY + hh,
                w: hw,
                h: hh
            }]
        );
        // ▗ bottom-right
        let r = box_for('\u{2597}');
        assert_eq!(
            r,
            vec![Rect {
                x: CX + hw,
                y: CY + hh,
                w: hw,
                h: hh
            }]
        );
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

    #[test]
    fn core_box_drawing_characters_are_procedural() {
        for codepoint in 0x2500..=0x2503 {
            let ch = char::from_u32(codepoint).expect("Box Drawing scalar");
            assert!(
                procedural_box(ch, CX, CY, CW, CH).is_some(),
                "U+{codepoint:04X} must not depend on a font glyph"
            );
        }
        for codepoint in 0x250C..=0x254B {
            let ch = char::from_u32(codepoint).expect("Box Drawing scalar");
            assert!(
                procedural_box(ch, CX, CY, CW, CH).is_some(),
                "U+{codepoint:04X}"
            );
        }
        for codepoint in 0x256D..=0x2570 {
            let ch = char::from_u32(codepoint).expect("Box Drawing scalar");
            assert!(
                procedural_box(ch, CX, CY, CW, CH).is_some(),
                "U+{codepoint:04X}"
            );
        }
        for codepoint in 0x2574..=0x257B {
            let ch = char::from_u32(codepoint).expect("Box Drawing scalar");
            assert!(
                procedural_box(ch, CX, CY, CW, CH).is_some(),
                "U+{codepoint:04X}"
            );
        }
    }
}
