//! Transport-neutral kernel discovery contract shared by every Ridge shell.
//!
//! This crate deliberately contains no Tauri or CLI dependency. The
//! `ridge-kernel` binary owns the process; shells consume this small contract.

pub mod agent_profiles;
pub mod client;
pub mod host;
pub mod host_core;
pub mod host_fs;
mod domain;
pub mod kernel_backed_handle;
pub mod lifecycle;
mod kernel_mcp;
pub mod metrics;
pub mod pty;
pub mod registry;
pub mod rtp1;
pub mod rtp1_session;
pub mod rtp1_ws;
pub mod server;
