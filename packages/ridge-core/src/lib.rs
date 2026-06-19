//! # ridge-core
//!
//! Runtime-agnostic command + workspace domain core shared by the desktop
//! (Tauri) host and the headless `ridge-cli` host. **Zero Tauri dependency**;
//! background tasks depend on `tokio` directly (never `tauri::async_runtime`).
//!
//! This crate is the geological foundation (地基) of the unified-remote
//! architecture (see
//! `docs/plans/unified-remote-architecture-handoff-final.md`, decisions
//! D4/D7/D8/D11). It exposes one entry point — [`dispatch::dispatch`] — that
//! every host funnels its `invoke`-style requests through, with a single
//! shared command implementation, a single capability policy layer (D8), and a
//! runtime-agnostic execution context ([`ctx::Ctx`]).
//!
//! ## The four `Ctx` abstraction faces (§5.1)
//!
//! 1. **State handle** — [`ctx::CoreState`], an `Arc`-held host state the host
//!    owns and handlers downcast back to a concrete type.
//! 2. **Event emitter** — [`ctx::EventSink`], distinguishing **broadcast** vs
//!    **single-connection** routing ([`ctx::EventScope`], D11).
//! 3. **Background task spawn** — [`ctx::TaskSpawner`] (default
//!    [`ctx::TokioSpawner`]), wrapping `tokio` directly (R3).
//! 4. **Error mapping** — [`error::CoreError`], independent of Tauri
//!    serialization, with explicit JSON-RPC and command-string boundary maps.
//!
//! ## Capability policy (D8)
//!
//! The command-admission whitelist is **data** ([`capability::CapabilitySet`]),
//! held on the `Ctx` and enforced once at the `dispatch` entry — never
//! re-implemented per host.

pub mod capability;
pub mod clipboard;
pub mod commands;
pub mod ctx;
pub mod device_identity;
pub mod dispatch;
pub mod error;
pub mod fs;
pub mod mcp;
pub mod pty;
pub mod sandbox;
mod seed_store;
pub mod teammate;
pub mod totp;
pub mod workspace;

// ── Curated public surface ──
pub use capability::{CapabilitySet, REMOTE_ALLOWLIST};
pub use ctx::{ConnectionId, CoreState, Ctx, EventScope, EventSink, TaskSpawner, TokioSpawner};
pub use device_identity::DeviceIdentity;
pub use dispatch::dispatch;
pub use error::{CoreError, CoreResult};
pub use sandbox::RootScope;
pub use totp::RemoteTotp;

// ── Domain Zero: 端侧多智能体协同核心（teammate / MCP 纯逻辑层）──
// 见 docs/superpowers/specs/2026-06-19-domain-zero-teammate-design.md。运行时
// 接线（server 路由 / PTY 注入 / Tauri 事件）在 src-tauri 复用这些纯类型。
pub use mcp::registry::{ToolRegistry, ToolSpec};
pub use mcp::resource::{RidgeUri, StashStore};
pub use teammate::model::{
    AgentCapabilities, AgentPersonality, AgentRole, Teammate, TeammateStatus,
};
pub use teammate::circuit_breaker::{LoopBreaker, LoopSignal};
pub use teammate::risk::{classify_method, classify_shell_command, RiskAssessment, RiskLevel};
pub use teammate::stream_cleaner::{CleanOutput, StreamCleaner};
pub use teammate::write_lock::{LockOutcome, WriteLockRegistry};
pub use teammate::tml::{TmlAction, TmlHeader, TmlMessage};
pub use teammate::topology::{TaskEdge, TopologyError, TopologyGraph};
