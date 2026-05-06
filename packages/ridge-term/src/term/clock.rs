//! Wall-clock helper for time-sensitive heuristics inside the kernel.
//!
//! The inline-TUI detector (`Grid::is_inline_tui_active_at`) needs to know
//! "did the application emit an absolute-positioning CSI within the last N
//! seconds". In the wasm runtime that's `js_sys::Date::now()`; in native
//! `cargo test --lib` builds we fall back to `SystemTime`.
//!
//! All values are unix-epoch milliseconds as `i64` so callers can compare
//! freshness with simple subtraction.

#[inline]
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}
