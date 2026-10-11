//! `ridge-acp` — Ridge Agent product layer.
//!
//! Single source of truth for the Agent product model. The crate is intentionally
//! runtime-agnostic and zero-Tauri: it speaks stdio JSON-RPC 2.0 to any external
//! ACP-compatible agent and exposes a `Serialize` event stream that future Desktop
//! or Mobile consumers can read without parsing shell ANSI or touching Tauri invokes.

pub mod error;
pub mod events;
pub mod lifecycle;
pub mod prompt_loop;
pub mod provider;
pub mod protocol;
pub mod session;
pub mod task;
pub mod transport;
pub mod turn;

pub use error::{AcpErrorWrap, AgentSessionError};
pub use events::AgentSessionEvent;
pub use lifecycle::LifecycleView;
pub use provider::{ModelSpec, ProviderKind, ProviderSpec};
pub use protocol::{
    AcpErrorObj, AcpNotification, AcpRequest, AcpResponse, InitializeParams, InitializeResult,
    SessionCancelParams, SessionNewParams, SessionNewResult, SessionPromptParams,
    SessionPromptResult, SessionUpdate, ACP_PROTOCOL_VERSION, METHOD_INITIALIZE,
    METHOD_SESSION_APPROVE, METHOD_SESSION_CANCEL, METHOD_SESSION_NEW, METHOD_SESSION_PROMPT,
    NOTIFY_AGENT_MESSAGE_CHUNK, NOTIFY_AGENT_REQUEST_APPROVAL, NOTIFY_AGENT_THINKING_CHUNK,
    NOTIFY_AGENT_TOOL_CALL, NOTIFY_AGENT_TOOL_RESULT, NOTIFY_SESSION_UPDATE,
};
pub use session::{
    AgentSession, AgentSessionCommand, AgentSessionConfig, SessionId, SessionStatus,
    StaffAssignment,
};
pub use turn::{
    AgentSessionSnapshot, PlanEntryView, PlanSnapshot, ToolCallRecord, ToolCallView, TurnId,
    TurnPhase, TurnRecord, TurnResult,
};
pub use transport::{AcpTransport, Framing};

pub use task::{
    blocked_by_failed_deps, ready_tasks, validate_dag, HandoffPayload, Plan, PlanId,
    PlanStore, Story, StoryId, Task, TaskId, TaskPoint, TaskResult, TaskStatus,
    WorkspaceRef,
};
