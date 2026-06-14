//! Shared LAN remote-control server logic.
//!
//! Provides TLS cert management (CA + leaf) and a common "bind → TLS → serve"
//! lifecycle used by both the desktop Tauri app and the `rdg` CLI.

pub mod server;
pub mod tls;
/// UA→UI 分叉判定（桌面 SPA vs 移动 SPA）的 SSOT，供局域网远控服务端与公网远控
/// 中继共用，避免分叉规则漂移。
pub mod ua;
