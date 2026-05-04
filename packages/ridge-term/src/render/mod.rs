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

#[cfg(target_arch = "wasm32")]
pub mod webgpu;

pub use backend::{
    CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme,
};
pub use renderer::Renderer;

#[cfg(target_arch = "wasm32")]
pub use canvas2d::Canvas2dBackend;
