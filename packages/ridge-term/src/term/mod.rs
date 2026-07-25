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
/// §mode-reattach — 「仅需终端模式、不渲染」宿主（rdg headless LAN host）的 live
/// modes 追踪复用外壳，复用 `Terminal` 解析核，零重复解析。
pub mod mode_tracker;
pub mod modes;
pub mod parser;
pub mod scrollback;
pub mod terminal;
pub mod wcwidth;

pub use mode_tracker::ModeTracker;
pub use terminal::Terminal;
