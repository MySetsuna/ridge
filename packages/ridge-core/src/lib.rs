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
pub mod capability_matrix_guard;
pub mod clipboard;
pub mod commands;
pub mod ctx;
pub mod device_identity;
pub mod dispatch;
pub mod error;
pub mod fs;
pub mod protocol_guard;
/// MCP 已独立成 `ridge-mcp` crate（桌面与 rdg 共用同一份实现）。这里保留再导出，
/// 历史调用点 `ridge_core::mcp::…` 无需改动。
pub use ridge_mcp as mcp;
pub mod external_spawn_registry;
pub mod grant_store;
pub mod process_guard;
pub mod pty;
pub mod remote;
pub mod sandbox;
mod seed_store;
pub mod teammate;
pub mod terminal_font;
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
// 运行时接线（server 路由 / PTY 注入 / Tauri 事件）在 src-tauri 复用这些纯类型。
pub use mcp::registry::{ToolRegistry, ToolSpec};
pub use mcp::resource::{RidgeUri, StashStore};
pub use teammate::circuit_breaker::{LoopBreaker, LoopSignal};
pub use teammate::communication::{
    choose_delivery_adapter, validate_target, AckState, AgentEnvelope, AgentIdentity,
    AgentLifecycle, AgentRef, AgentTarget, CommunicationError, DeliveryAdapter,
    DeliveryCapabilities, DeliveryDecision, DeliveryReliability, MessageKind, MessagePriority,
    PtySafety, TypedError,
};
pub use teammate::model::{recognize_capability, AgentRole, AgentTier, Teammate, TeammateStatus};
pub use teammate::risk::{classify_method, classify_shell_command, RiskAssessment, RiskLevel};
pub use teammate::topology::{elect_leader, TaskEdge, TopologyError, TopologyGraph};
pub use teammate::write_lock::{LockOutcome, WriteLockRegistry};
