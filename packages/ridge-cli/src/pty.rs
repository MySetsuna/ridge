//! CLI shell projection of the kernel PTY primitive.
//!
//! PTY spawn/read/write/resize semantics live in `ridge-kernel`; rdg owns only
//! terminal presentation and remote protocol routing.

pub use ridge_kernel::pty::PtyBridge;
