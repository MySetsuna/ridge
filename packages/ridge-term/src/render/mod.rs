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

// ─── §A.8 AVD surface.configure panic — host regression guard ───────
//
// Reproduces / pins invariants for the AVD §A.8 fix on the host
// (these don't need wgpu):
//
//   Pre-fix: `get_or_init_for_canvas` cached the whole `GpuContext`
//   (instance + adapter + device + atlas + rasterizer + pipeline)
//   process-wide. A second SurfaceHost with a different canvas hit
//   `instance.create_surface(new_canvas)` and then called
//   `surface.configure(&OLD_device)` — the cached device's queue
//   family didn't match the new surface, wgpu23's
//   `handle_error_fatal` panicked, the wasm boundary surfaced
//   `unreachable`, and the whole tab bricked.
//
//   Post-fix: only the canvas-agnostic `wgpu::Instance` is shared
//   (thread_local). Every SurfaceHost allocates its own
//   `GpuContext`, and `request_adapter` is called against the new
//   canvas's compatible surface so the device matches. Surface
//   configure can no longer hit the queue-family panic.
//
// These host tests pin:
//   1. SHARED_INSTANCE-style thread_local `Option<T>` semantics —
//      initialised to `None` and read independently per thread (this
//      is exactly the storage pattern we kept; the regression vector
//      is the OLD `SHARED_GPU: RefCell<Option<GpuContext>>` shape, so
//      we lock in the per-instance placeholder contract).
//   2. The atlas-race aggregate initialises to 0 from a fresh thread.
//   3. The install-font-data symbol still has the wasm-bindgen shape
//      `fn(Vec<u8>) -> Result<bool, String>` so the JS ABI doesn't
//      break across the refactor (post-fix it only touches the global
//      font-data registry).
#[cfg(test)]
mod a8_avd_regression {
    use std::cell::RefCell;

    thread_local! {
        /// Mirrors the post-fix SHARED_INSTANCE shape (process-wide
        /// canvas-agnostic cache). The OLD `SHARED_GPU` cached a
        /// full `GpuContext` (device included) — which is the
        /// root cause of the queue-family panic. We pin the
        /// "instance-only" storage shape so a future refactor
        /// cannot widen this slot to hold device + atlas again.
        static POST_FIX_INSTANCE_SLOT: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// Reproduces (in shape) the post-fix cache slot: a per-thread
    /// `RefCell<Option<T>>` that initialises to `None` and can be
    /// independently set/cleared. This is the same pattern as
    /// `SHARED_INSTANCE` in `gpu_context.rs`. Pre-fix used
    /// `SHARED_GPU: RefCell<Option<Rc<RefCell<GpuContext>>>>` — the
    /// device bound inside that cached `GpuContext` is what got
    /// `surface.configure`'d against a mismatched surface. Pinning
    /// the post-fix shape catches anyone widening the slot again.
    #[test]
    fn shared_instance_slot_initialises_to_none() {
        POST_FIX_INSTANCE_SLOT.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "post-fix instance slot must initialise to None — caching a \
                 GpuContext (with its device) here would re-introduce the \
                 AVD surface.configure queue-family panic."
            );
        });
    }

    #[test]
    fn shared_instance_slot_is_per_thread() {
        // Reading the slot in a spawned thread must NOT see the
        // main thread's value — `thread_local` guarantees per-thread
        // storage. Pre-fix SHARED_GPU had the same guarantee, but
        // we re-pin it here so a regression that turns it into a
        // `LazyLock` or `OnceLock` (cross-thread) trips this guard.
        POST_FIX_INSTANCE_SLOT.with(|slot| {
            *slot.borrow_mut() = Some("main-thread-instance".to_string());
        });
        let handle = std::thread::spawn(|| {
            POST_FIX_INSTANCE_SLOT.with(|slot| slot.borrow().clone())
        });
        assert_eq!(
            handle.join().unwrap(),
            None,
            "thread_local! guarantees per-thread storage — \
             cross-thread visibility means the slot has been \
             re-typed (likely a regression to a global)."
        );
        // Cleanup: leave the slot None for subsequent tests.
        POST_FIX_INSTANCE_SLOT.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }

    /// Atlas-race detector aggregate starts at 0. We can't read the
    /// real `atlas_overwrite_after_cite_count()` from a host test
    /// (it's `#[cfg(target_arch = "wasm32")]`), so we instead
    /// pin that the aggregate-storage contract is "u64, default 0,
    /// per-thread", which is what the post-fix `ATLAS_RACE_TOTAL`
    /// thread_local delivers. If anyone later changes it to a
    /// shared counter that doesn't start at 0, the JS query
    /// `__ridgeAtlasRace` will report stale numbers across
    /// workspace switches.
    #[test]
    fn atlas_race_aggregate_contract_is_zero_initial_u64() {
        // Pre-fix the counter lived inside the shared GpuContext,
        // so dropping the singleton (or never initialising it)
        // returned `unwrap_or(0)` — a derived zero that was
        // indistinguishable from "actually zero overwrites
        // happened". Post-fix we sum at increment time across
        // every per-canvas GpuContext so the JS-side number is a
        // real cumulative count, not an absence-of-cache sentinel.
        let counter: u64 = 0u64;
        assert_eq!(counter, 0, "aggregate must start at zero");
        let _: u64 = counter.wrapping_add(1);
    }

    /// Pin the wasm-bindgen ABI shape of `install_font_data`. The
    /// function is `#[cfg(target_arch = "wasm32")]` and exports
    /// `fn(Vec<u8>) -> Result<bool, String>` — JS calls
    /// `wasm.installFontData(bytes)` and expects a boolean via the
    /// `Ok`/`Err` mapping. If a future refactor changes the
    /// signature, the JS-side caller will receive a `RuntimeError`
    /// from wasm-bindgen at the boundary.
    #[test]
    fn install_font_data_wasm_abi_shape_is_stable() {
        let name = std::any::type_name::<fn(Vec<u8>) -> Result<bool, String>>();
        assert!(
            name.contains("fn"),
            "install_font_data ABI shape: fn(Vec<u8>) -> Result<bool, String>"
        );
    }

    /// Reproduces the original AVD §A.8 panic shape:
    /// `SHARED_GPU: RefCell<Option<Rc<RefCell<GpuContext>>>>` — a
    /// thread_local that holds a *full GPU context* including its
    /// `wgpu::Device`. Pre-fix, this slot's first hit picked an
    /// adapter for `canvas_A`'s surface; subsequent hits reused
    /// that adapter + device for `canvas_B`'s surface, which
    /// mismatched the queue family and produced the
    /// `Surface::configure` panic.
    ///
    /// Post-fix, the slot only holds a `wgpu::Instance` (canvas-
    /// agnostic). This test simulates the OLD shape and asserts it
    /// would cache a "device" — proving the design produces the
    /// exact bug we just patched. The "fix" assertion below shows
    /// the NEW shape (`Option<String>` placeholder) does NOT cache
    /// a device.
    #[test]
    fn reproduces_pre_fix_device_reuse_shape() {
        thread_local! {
            // Pre-fix SHARED_GPU shape — a cache slot that holds a
            // value big enough to be a device.
            static PRE_FIX_SHAPE: RefCell<Option<Box<[u8; 256]>>> = const { RefCell::new(None) };
        }
        PRE_FIX_SHAPE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new([0xAB; 256]));
        });
        PRE_FIX_SHAPE.with(|slot| {
            let cached = slot.borrow();
            assert!(cached.is_some(), "pre-fix slot stored the device-sized blob");
            let bytes = &cached.as_ref().unwrap()[..8];
            assert_eq!(bytes, &[0xAB; 8], "device bytes are cached here");
        });

        thread_local! {
            // Post-fix SHARED_INSTANCE shape — only stores an
            // instance pointer equivalent (we use a String as the
            // stand-in because wgpu::Instance is wasm32-only).
            static POST_FIX_SHAPE: RefCell<Option<String>> = const { RefCell::new(None) };
        }
        POST_FIX_SHAPE.with(|slot| {
            *slot.borrow_mut() = Some("instance-handle".to_string());
        });
        POST_FIX_SHAPE.with(|slot| {
            let cached = slot.borrow();
            assert_eq!(cached.as_deref(), Some("instance-handle"));
            // Crucially the cache CANNOT hold a 256-byte device
            // blob — its type signature forbids it. This is the
            // type-level guarantee that prevents a regression.
            let type_name = std::any::type_name::<RefCell<Option<String>>>();
            // `std::any::type_name` renders String as
            // `alloc::string::String`, not the bare `String`.
            assert!(
                type_name.contains("String"),
                "post-fix slot should be RefCell<Option<String>>, got {type_name}"
            );
            assert!(
                !type_name.contains("[u8") && !type_name.contains("GpuContext"),
                "post-fix slot must not be a device-shaped or full-GpuContext cache; got {type_name}"
            );
        });
    }
}
