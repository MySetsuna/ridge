//! Transport-neutral kernel discovery contract shared by every Ridge shell.
//!
//! This crate deliberately contains no Tauri, CLI, or web dependency. The
//! `ridge-kernel` binary owns the process; shells consume this small contract.

pub mod registry;
pub mod client;
pub mod agent_profiles;
pub mod pty;
mod domain;
mod mcp_min;
pub mod server;
