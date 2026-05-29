pub mod attr_table;
pub mod attrs;
pub mod cell;
pub mod clock;
pub mod cursor;
// P3.2 (2026-05-20): wire format for the future Rust-side parser
// engine → wasm frontend channel. Pure data + serde derives, no
// platform gating — both ends compile from the same source.
pub mod delta;
pub mod grid;
pub mod modes;
pub mod parser;
pub mod scrollback;
pub mod terminal;
pub mod wcwidth;

pub use terminal::Terminal;
