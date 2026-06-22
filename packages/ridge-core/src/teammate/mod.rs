//! 端侧多智能体协同 —— 逻辑/物理纯核心层（零 Tauri，可单测）。
//!
//! 见 `docs/superpowers/specs/2026-06-20-team-agent-upgrade-plan-design.md`（底座化瘦身）。
//! 运行时接线（server 路由 / PTY 注入 / Tauri 事件）放在 `src-tauri`；
//! 本模块只承载**可单测的纯逻辑**：Teammate 名册、拓扑、风险分级、循环熔断、写锁。
//! （TML 线协议 / PTY 流净化 / Leader 竞选已退场。）

pub mod circuit_breaker;
pub mod model;
pub mod risk;
pub mod topology;
pub mod write_lock;
