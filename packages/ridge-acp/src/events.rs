//! Canonical events emitted by `AgentSession`. This is the single contract
//! every consumer (Communion / Desktop / future Mobile) decodes — no parallel
//! state machines, no shell ANSI, no Tauri invokes for session facts.
//!
//! Wire shape: `Serialize` + camelCase + `kind` tag so consumers can switch on
//! `event.kind` without parsing the body.

use serde::{Deserialize, Serialize};

use crate::lifecycle::LifecycleView;
use crate::protocol::AcpErrorObj;
use crate::provider::ProviderSpec;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentSessionEvent {
    SessionCreated {
        session_id: crate::session::SessionId,
        provider: ProviderSpec,
        ts_unix_ms: u64,
    },
    StateChanged {
        session_id: crate::session::SessionId,
        lifecycle: LifecycleView,
        ts_unix_ms: u64,
    },
    TurnStarted {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        prompt: String,
        ts_unix_ms: u64,
    },
    ThinkingChunk {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        text: String,
        ts_unix_ms: u64,
    },
    MessageChunk {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        text: String,
        ts_unix_ms: u64,
    },
    ToolCallStarted {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        tool_call: crate::session::ToolCallView,
        ts_unix_ms: u64,
    },
    ToolCallFinished {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        tool_call_id: String,
        status: String,
        ts_unix_ms: u64,
    },
    ApprovalRequested {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        tool_call_id: String,
        reason: String,
        ts_unix_ms: u64,
    },
    ApprovalResolved {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        tool_call_id: String,
        approved: bool,
        ts_unix_ms: u64,
    },
    PlanChanged {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        plan: crate::session::PlanSnapshot,
        ts_unix_ms: u64,
    },
    TurnCompleted {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        result: crate::turn::TurnResult,
        ts_unix_ms: u64,
    },
    TurnFailed {
        session_id: crate::session::SessionId,
        turn_id: crate::session::TurnId,
        error: AcpErrorObj,
        ts_unix_ms: u64,
    },
    SessionCompleted {
        session_id: crate::session::SessionId,
        ts_unix_ms: u64,
    },
    SessionFailed {
        session_id: crate::session::SessionId,
        error: AcpErrorObj,
        ts_unix_ms: u64,
    },
}

impl AgentSessionEvent {
    /// Stable lowercase identifier for logs / debugging — matches the serde
    /// `kind` tag so consumers can cross-reference.
    pub fn kind_str(&self) -> &'static str {
        match self {
            AgentSessionEvent::SessionCreated { .. } => "session_created",
            AgentSessionEvent::StateChanged { .. } => "state_changed",
            AgentSessionEvent::TurnStarted { .. } => "turn_started",
            AgentSessionEvent::ThinkingChunk { .. } => "thinking_chunk",
            AgentSessionEvent::MessageChunk { .. } => "message_chunk",
            AgentSessionEvent::ToolCallStarted { .. } => "tool_call_started",
            AgentSessionEvent::ToolCallFinished { .. } => "tool_call_finished",
            AgentSessionEvent::ApprovalRequested { .. } => "approval_requested",
            AgentSessionEvent::ApprovalResolved { .. } => "approval_resolved",
            AgentSessionEvent::PlanChanged { .. } => "plan_changed",
            AgentSessionEvent::TurnCompleted { .. } => "turn_completed",
            AgentSessionEvent::TurnFailed { .. } => "turn_failed",
            AgentSessionEvent::SessionCompleted { .. } => "session_completed",
            AgentSessionEvent::SessionFailed { .. } => "session_failed",
        }
    }
}
