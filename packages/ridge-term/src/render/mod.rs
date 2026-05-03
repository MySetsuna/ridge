//! Rendering layer.
//!
//! Gated on `target_arch = "wasm32"` because the backends use web-sys.
//! The `term` module (VT kernel) stays target-agnostic so unit tests
//! run on the host with `cargo test --lib`.

pub mod backend;
pub mod renderer;

#[cfg(target_arch = "wasm32")]
pub mod canvas2d;

pub use backend::{
    CursorDraw, CursorStyle, FrameMetrics, RenderBackend, RowDraw, Theme,
};
pub use renderer::Renderer;

#[cfg(target_arch = "wasm32")]
pub use canvas2d::Canvas2dBackend;
