//! Domain C — 内置端侧 MCP（Model Context Protocol）增强层纯协议核心。
//!
//! 见 `docs/superpowers/specs/2026-06-19-domain-zero-teammate-design.md`。
//! 只承载**可单测的纯协议逻辑**：JSON-RPC/MCP 报文类型、Tool 注册表、
//! `ridge://` 资源 URI 解析与内存 Stash。传输层（axum WS/SSE 挂载、tools/call
//! 路由到拓扑总线）在 `src-tauri` 接线，复用 `remote/server.rs` 的 WS+JSON-RPC 模式。

pub mod protocol;
pub mod registry;
pub mod resource;
