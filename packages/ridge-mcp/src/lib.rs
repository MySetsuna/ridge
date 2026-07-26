//! Ridge 内置 MCP（Model Context Protocol）server —— **桌面与 rdg 唯一的一份实现**。
//!
//! 分层：
//! - [`protocol`] / [`registry`] / [`resource`] / [`addressing`]：可单测的纯协议层
//!   （JSON-RPC 报文、工具规格、`ridge://` URI + 内存 Stash、pane 寻址）。
//! - [`server`]：方法分发 + 工具语义 + 跨 agent 收件箱，宿主经 [`server::McpHost`] 接入。
//! - [`transport`]（feature `axum-transport`）：WebSocket 与 HTTP 两条路由。
//!
//! 目标是让**异构 agent**（Claude Code / Cursor / 自写客户端…）即使没有共同的内部
//! 协议，也能在同一个终端工作区里发现同伴、派活、观察进展、异步回话、传大块产物。
//!
//! `ridge_core::mcp` 以 `pub use ridge_mcp as mcp;` 再导出，历史调用点无需改动。

pub mod addressing;
pub mod protocol;
pub mod registry;
pub mod resource;
pub mod server;
#[cfg(feature = "axum-transport")]
pub mod transport;
