//! Tauri 桥模块（M2.1 §2 NS-2）。
//!
//! 目前只有一个 `acp_bridge` 子模块；保留 `mod.rs` 是为了未来扩展
//! （remote bridge 已迁至 `crate::remote_bridge`；ACP 桥作为新成员独立）。

pub mod acp_bridge;
