//! Pane 分屏布局树 —— 已移入 `ridge_core::workspace::pane_tree`（D11 Wave A / P1）。
//!
//! 桌面保留本模块路径为**薄 re-export**，使 `crate::engine::pane_tree::*` 调用点
//! （`state.rs` 的 `Workspace.pane_tree`、`commands/pane.rs`、`commands/ridge_file.rs`
//! 的 serde、`teammate`/`terminal`）保持不变；逻辑 + 测试现活在（并运行于）
//! `ridge-core`，跨平台单测不再受桌面 cdylib test 崩溃（0xc0000139）影响。
//! 方法错误从 `AppError` 改 `ridge_core::CoreError`，经 `From<CoreError> for AppError`
//! （`utils::error`）在 `?`-路径上还原同样的错误串。
pub use ridge_core::workspace::pane_tree::{DockRegion, Pane, PaneNode, PaneTree, SplitDirection};
