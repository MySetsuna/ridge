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
