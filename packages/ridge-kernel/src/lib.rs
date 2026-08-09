//! Transport-neutral kernel discovery contract shared by every Ridge shell.
//!
//! This crate deliberately contains no Tauri, CLI, or web dependency. The
//! `ridge-kernel` binary owns the process; shells consume this small contract.

pub mod agent_profiles;
pub mod client;
mod domain;
mod kernel_mcp;
pub mod pty;
pub mod registry;
pub mod server;
