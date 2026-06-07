//! 无头 tmux 会话引擎现已抽取到独立的、**Tauri-free** 的 `ridge-tmux` crate
//! （`packages/ridge-tmux`），使桌面端（Tauri host）与无头 `ridge-cli` host 能共享
//! **同一份引擎**。本文件保留为薄 re-export，让 `crate::teammate::native::*` 引用零改动。
//!
//! 为什么能抽取：该引擎是纯逻辑——只依赖 `portable-pty`（真实子进程）+ `ridge-term`
//! 的 `Terminal`（capture 重渲当前屏）+ `tokio::broadcast`，**零 `AppState` / 零 Tauri**。
//! 因此可被任何 host link。需要工作区的「召唤」(summon into workspace) 仍由 host 侧的
//! `teammate::server` 负责（它持有 `AppState`/`AppHandle`），引擎只暴露共享的 PTY 句柄
//! 与首屏 replay。
//!
//! 引擎源码与单元测试现居 `ridge-tmux`；改动引擎请编辑 `packages/ridge-tmux/src/lib.rs`。

pub use ridge_tmux::*;
