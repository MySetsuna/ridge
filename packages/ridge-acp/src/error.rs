//! Error types for the ACP layer.

use thiserror::Error;

use crate::protocol::AcpErrorObj;

#[derive(Debug, Clone, Error)]
pub enum AcpErrorWrap {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("rpc error code={0} message={1}")]
    Rpc(i64, String),
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("invalid frame: {0}")]
    InvalidFrame(String),
    #[error("agent session closed unexpectedly")]
    SessionClosed,
    #[error("agent session error: {0}")]
    Session(#[from] AgentSessionError),
}

#[derive(Debug, Clone, Error)]
pub enum AgentSessionError {
    #[error("session not initialized")]
    NotInitialized,
    #[error("session not open: {0}")]
    SessionNotOpen(String),
    #[error("turn not found: {0}")]
    TurnNotFound(String),
    #[error("agent denied: {0}")]
    Denied(String),
    #[error("agent failed: code={code} message={message}")]
    AgentFailed { code: i64, message: String },
    #[error("cancelled by host")]
    Cancelled,
    #[error("default approval decision missing")]
    DefaultApprovalMissing,
}

impl From<AcpErrorObj> for AcpErrorWrap {
    fn from(e: AcpErrorObj) -> Self {
        Self::Rpc(e.code, e.message)
    }
}
