pub mod pane_tree;
// P3.3 (2026-05-20): per-pane parser that turns raw PTY bytes into
// GridDelta frames on the Rust side. Not wired into pty.rs yet — that's
// P3.4. Adding the module + tests now lets the diff logic land and
// accrete coverage independent of the IPC plumbing.
pub mod parser;
pub mod pty;

// `cwd`(OSC 7) 与 `title`(OSC 0/1/2) 这两个**纯字节流解析器**已下沉到
// `ridge_core::pty::{cwd,title}`（D11 切片，无头 host 可复用以上报 cwd/title）。
// 桌面读线程经 `use ridge_core::pty::{cwd, title};` 委托调用，行为零变化。
