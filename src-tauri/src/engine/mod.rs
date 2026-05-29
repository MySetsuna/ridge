pub mod cwd;
pub mod pane_tree;
// P3.3 (2026-05-20): per-pane parser that turns raw PTY bytes into
// GridDelta frames on the Rust side. Not wired into pty.rs yet — that's
// P3.4. Adding the module + tests now lets the diff logic land and
// accrete coverage independent of the IPC plumbing.
pub mod parser;
pub mod pty;
pub mod title;
