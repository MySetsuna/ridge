//! Shared LAN remote-control server logic.
//!
//! Provides TLS cert management (CA + leaf) and a common "bind → TLS → serve"
//! lifecycle used by both the desktop Tauri app and the `rdg` CLI.

pub mod tls;
pub mod server;
