//! 端侧多智能体协同 —— 逻辑/物理纯核心层（零 Tauri，可单测）。
//!
//! 见 `docs/superpowers/specs/2026-06-19-domain-zero-teammate-design.md`。
//! 运行时接线（server 路由 / PTY 注入 / Tauri 事件）放在 `src-tauri`；
//! 本模块只承载**可单测的纯逻辑**：TML 协议、PTY 流净化、Teammate 画像、
//! 拓扑/Leader 竞选、风险分级。

pub mod circuit_breaker;
pub mod model;
pub mod risk;
pub mod stream_cleaner;
pub mod tml;
pub mod topology;
pub mod write_lock;
