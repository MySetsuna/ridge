//! Kernel-owned communication contract for Agent/Teammate adapters.
//!
//! This module is deliberately runtime-agnostic. It defines the identity and
//! delivery decisions that MCP, Remote, desktop, and headless hosts must share;
//! transport implementations stay outside the domain model.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Observable Agent lifecycle. A transport may add detail, but must project
/// into these states before routing a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentLifecycle {
    Discovered,
    Spawning,
    Attaching,
    Online,
    Working,
    Waiting,
    Attention,
    Completed,
    Stopped,
    Failed,
}

impl AgentLifecycle {
    /// Whether the Agent can receive a new envelope from the Hub.
    pub fn can_receive(self) -> bool {
        matches!(
            self,
            Self::Online | Self::Working | Self::Waiting | Self::Attention
        )
    }
}

/// Stable identity owned by the Kernel/Teammate registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub agent_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub cwd: String,
    pub executable: String,
    pub argv: Vec<String>,
    pub generation: u64,
    pub lease: String,
    pub lifecycle: AgentLifecycle,
    pub online: bool,
    pub last_seen_unix_ms: u64,
    pub capabilities: Vec<String>,
}

impl AgentIdentity {
    pub fn target(&self) -> AgentTarget {
        AgentTarget {
            agent_id: self.agent_id.clone(),
            workspace_id: self.workspace_id.clone(),
            generation: self.generation,
            lease: self.lease.clone(),
        }
    }
}

/// Minimum target data required for a fenced send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTarget {
    pub agent_id: String,
    pub workspace_id: String,
    pub generation: u64,
    pub lease: String,
}

/// Validate a target against one bounded roster snapshot.
pub fn validate_target(
    target: &AgentTarget,
    snapshot: Option<&AgentIdentity>,
    required_capability: Option<&str>,
) -> Result<(), CommunicationError> {
    let Some(identity) = snapshot else {
        return Err(CommunicationError::TargetMissing(target.agent_id.clone()));
    };
    if identity.agent_id != target.agent_id {
        return Err(CommunicationError::TargetMissing(target.agent_id.clone()));
    }
    if identity.workspace_id != target.workspace_id {
        return Err(CommunicationError::WorkspaceMismatch {
            expected: target.workspace_id.clone(),
            actual: identity.workspace_id.clone(),
        });
    }
    if identity.generation != target.generation {
        return Err(CommunicationError::GenerationMismatch {
            expected: target.generation,
            actual: identity.generation,
        });
    }
    if identity.lease != target.lease {
        return Err(CommunicationError::StaleLease);
    }
    if !identity.online || !identity.lifecycle.can_receive() {
        return Err(CommunicationError::TargetOffline(target.agent_id.clone()));
    }
    if let Some(capability) = required_capability {
        if !identity.capabilities.iter().any(|item| item == capability) {
            return Err(CommunicationError::CapabilityDenied(capability.to_string()));
        }
    }
    Ok(())
}

/// All Agent-to-Agent traffic uses one envelope, regardless of adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRef {
    pub agent_id: String,
    pub session_id: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub generation: u64,
    pub lease: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageKind {
    Message,
    Task,
    Event,
    Control,
    Artifact,
    Reply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessagePriority {
    Control,
    Input,
    Task,
    Event,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AckState {
    None,
    Pending,
    Acked,
    Nacked(TypedError),
}

/// Durable message/task/event/control/artifact/reply contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEnvelope {
    pub message_id: String,
    pub idempotency_key: String,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub conversation_id: Option<String>,
    pub task_id: Option<String>,
    pub from: AgentRef,
    pub to: AgentRef,
    pub workspace_id: String,
    pub kind: MessageKind,
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub priority: MessagePriority,
    pub deadline_unix_ms: Option<u64>,
    pub cancellation_id: Option<String>,
    pub payload: Value,
    pub artifact_ref: Option<String>,
    pub ack: AckState,
}

impl AgentEnvelope {
    /// Reject malformed identity/routing data before a transport is touched.
    pub fn validate(&self) -> Result<(), CommunicationError> {
        for (name, value) in [
            ("message_id", self.message_id.as_str()),
            ("idempotency_key", self.idempotency_key.as_str()),
            ("workspace_id", self.workspace_id.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(CommunicationError::InvalidEnvelope(format!(
                    "{name} must not be empty"
                )));
            }
        }
        validate_agent_ref("from", &self.from)?;
        validate_agent_ref("to", &self.to)?;
        if self.from.workspace_id != self.workspace_id || self.to.workspace_id != self.workspace_id
        {
            return Err(CommunicationError::InvalidEnvelope(
                "envelope and endpoint workspace_id must match".into(),
            ));
        }
        for (name, value) in [
            ("correlation_id", self.correlation_id.as_deref()),
            ("causation_id", self.causation_id.as_deref()),
            ("conversation_id", self.conversation_id.as_deref()),
            ("task_id", self.task_id.as_deref()),
            ("cancellation_id", self.cancellation_id.as_deref()),
            ("artifact_ref", self.artifact_ref.as_deref()),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(CommunicationError::InvalidEnvelope(format!(
                    "{name} must not be empty when present"
                )));
            }
        }
        if self.sequence == 0 {
            return Err(CommunicationError::InvalidEnvelope(
                "sequence must be greater than zero".into(),
            ));
        }
        Ok(())
    }
}

fn validate_agent_ref(name: &str, endpoint: &AgentRef) -> Result<(), CommunicationError> {
    for (field, value) in [
        ("agent_id", endpoint.agent_id.as_str()),
        ("session_id", endpoint.session_id.as_str()),
        ("workspace_id", endpoint.workspace_id.as_str()),
        ("pane_id", endpoint.pane_id.as_str()),
        ("lease", endpoint.lease.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(CommunicationError::InvalidEnvelope(format!(
                "{name}.{field} must not be empty"
            )));
        }
    }
    if endpoint.generation == 0 {
        return Err(CommunicationError::InvalidEnvelope(format!(
            "{name}.generation must be greater than zero"
        )));
    }
    Ok(())
}

/// Errors that adapters expose to callers; do not replace with a silent PTY
/// write or an unbounded retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum CommunicationError {
    #[error("target missing: {0}")]
    TargetMissing(String),
    #[error("target offline: {0}")]
    TargetOffline(String),
    #[error("stale Agent lease")]
    StaleLease,
    #[error("Agent generation mismatch: expected {expected}, actual {actual}")]
    GenerationMismatch { expected: u64, actual: u64 },
    #[error("workspace mismatch: expected {expected}, actual {actual}")]
    WorkspaceMismatch { expected: String, actual: String },
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("invalid envelope: {0}")]
    InvalidEnvelope(String),
    #[error("no delivery adapter is available")]
    DeliveryUnavailable,
}

/// Delivery order is a policy, not an implementation detail of one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryAdapter {
    RuntimeApi,
    A2a,
    McpPull,
    PtyFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryReliability {
    Durable,
    AtLeastOnce,
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryDecision {
    pub adapter: DeliveryAdapter,
    pub reliability: DeliveryReliability,
}

/// The complete proof required before a PTY fallback may touch user input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PtySafety {
    pub agent_idle: bool,
    pub terminal_mode_agent_prompt: bool,
    pub pending_approval: bool,
    pub foreground_is_target_agent: bool,
    pub user_input_competing: bool,
}

impl PtySafety {
    pub fn is_safe(self) -> bool {
        self.agent_idle
            && self.terminal_mode_agent_prompt
            && !self.pending_approval
            && self.foreground_is_target_agent
            && !self.user_input_competing
    }

    #[cfg(test)]
    fn safe() -> Self {
        Self {
            agent_idle: true,
            terminal_mode_agent_prompt: true,
            pending_approval: false,
            foreground_is_target_agent: true,
            user_input_competing: false,
        }
    }
}

/// Capabilities probed by an adapter; no CLI support is hard-coded here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryCapabilities {
    pub runtime_api: bool,
    pub a2a: bool,
    pub mcp_pull: bool,
    pub pty: PtySafety,
}

pub fn choose_delivery_adapter(
    capabilities: DeliveryCapabilities,
) -> Result<DeliveryDecision, CommunicationError> {
    if capabilities.runtime_api {
        return Ok(DeliveryDecision {
            adapter: DeliveryAdapter::RuntimeApi,
            reliability: DeliveryReliability::Durable,
        });
    }
    if capabilities.a2a {
        return Ok(DeliveryDecision {
            adapter: DeliveryAdapter::A2a,
            reliability: DeliveryReliability::AtLeastOnce,
        });
    }
    if capabilities.mcp_pull {
        return Ok(DeliveryDecision {
            adapter: DeliveryAdapter::McpPull,
            reliability: DeliveryReliability::AtLeastOnce,
        });
    }
    if capabilities.pty.is_safe() {
        return Ok(DeliveryDecision {
            adapter: DeliveryAdapter::PtyFallback,
            reliability: DeliveryReliability::BestEffort,
        });
    }
    Err(CommunicationError::DeliveryUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> AgentIdentity {
        AgentIdentity {
            agent_id: "agent-a".into(),
            session_id: "session-a".into(),
            workspace_id: "workspace-a".into(),
            pane_id: "pane-a".into(),
            cwd: "C:/work".into(),
            executable: "codex".into(),
            argv: vec!["--resume".into()],
            generation: 2,
            lease: "lease-2".into(),
            lifecycle: AgentLifecycle::Online,
            online: true,
            last_seen_unix_ms: 10,
            capabilities: vec!["messages".into()],
        }
    }

    #[test]
    fn target_validation_fences_generation_lease_and_capability() {
        let current = identity();
        let mut target = current.target();
        assert_eq!(
            validate_target(&target, Some(&current), Some("messages")),
            Ok(())
        );

        target.generation = 1;
        assert!(matches!(
            validate_target(&target, Some(&current), None),
            Err(CommunicationError::GenerationMismatch { .. })
        ));
        target = current.target();
        target.lease = "old-lease".into();
        assert_eq!(
            validate_target(&target, Some(&current), None),
            Err(CommunicationError::StaleLease)
        );
        target = current.target();
        assert_eq!(
            validate_target(&target, Some(&current), Some("artifacts")),
            Err(CommunicationError::CapabilityDenied("artifacts".into()))
        );
    }

    #[test]
    fn offline_and_missing_targets_fail_before_delivery() {
        let current = identity();
        let target = current.target();
        assert_eq!(
            validate_target(&target, None, None),
            Err(CommunicationError::TargetMissing("agent-a".into()))
        );
        let mut offline = current.clone();
        offline.online = false;
        assert_eq!(
            validate_target(&target, Some(&offline), None),
            Err(CommunicationError::TargetOffline("agent-a".into()))
        );
    }

    #[test]
    fn delivery_policy_uses_runtime_then_a2a_then_mcp_then_guarded_pty() {
        let cases = [
            (
                DeliveryCapabilities {
                    runtime_api: true,
                    ..Default::default()
                },
                DeliveryAdapter::RuntimeApi,
            ),
            (
                DeliveryCapabilities {
                    a2a: true,
                    ..Default::default()
                },
                DeliveryAdapter::A2a,
            ),
            (
                DeliveryCapabilities {
                    mcp_pull: true,
                    ..Default::default()
                },
                DeliveryAdapter::McpPull,
            ),
            (
                DeliveryCapabilities {
                    pty: PtySafety::safe(),
                    ..Default::default()
                },
                DeliveryAdapter::PtyFallback,
            ),
        ];
        for (capabilities, adapter) in cases {
            assert_eq!(
                choose_delivery_adapter(capabilities).unwrap().adapter,
                adapter
            );
        }
        assert_eq!(
            choose_delivery_adapter(DeliveryCapabilities::default()),
            Err(CommunicationError::DeliveryUnavailable)
        );
    }

    #[test]
    fn pty_fallback_requires_every_safety_condition() {
        let mut unsafe_cases = [
            PtySafety {
                agent_idle: false,
                ..PtySafety::safe()
            },
            PtySafety {
                terminal_mode_agent_prompt: false,
                ..PtySafety::safe()
            },
            PtySafety {
                pending_approval: true,
                ..PtySafety::safe()
            },
            PtySafety {
                foreground_is_target_agent: false,
                ..PtySafety::safe()
            },
            PtySafety {
                user_input_competing: true,
                ..PtySafety::safe()
            },
        ];
        for pty in unsafe_cases.iter_mut() {
            assert!(!pty.is_safe());
            assert_eq!(
                choose_delivery_adapter(DeliveryCapabilities {
                    pty: *pty,
                    ..Default::default()
                }),
                Err(CommunicationError::DeliveryUnavailable)
            );
        }
    }

    #[test]
    fn envelope_rejects_cross_workspace_and_round_trips_typed_fields() {
        let endpoint = AgentRef {
            agent_id: "agent-a".into(),
            session_id: "session-a".into(),
            workspace_id: "workspace-a".into(),
            pane_id: "pane-a".into(),
            generation: 2,
            lease: "lease-2".into(),
        };
        let mut envelope = AgentEnvelope {
            message_id: "message-1".into(),
            idempotency_key: "idem-1".into(),
            correlation_id: Some("correlation-1".into()),
            causation_id: Some("cause-1".into()),
            conversation_id: Some("conversation-1".into()),
            task_id: Some("task-1".into()),
            from: endpoint.clone(),
            to: endpoint,
            workspace_id: "workspace-a".into(),
            kind: MessageKind::Task,
            sequence: 7,
            timestamp_unix_ms: 11,
            priority: MessagePriority::Task,
            deadline_unix_ms: Some(100),
            cancellation_id: Some("cancel-1".into()),
            payload: serde_json::json!({ "text": "hello" }),
            artifact_ref: None,
            ack: AckState::Pending,
        };
        assert_eq!(envelope.validate(), Ok(()));
        let raw = serde_json::to_string(&envelope).unwrap();
        let decoded: AgentEnvelope = serde_json::from_str(&raw).unwrap();
        assert_eq!(decoded, envelope);
        envelope.to.workspace_id = "other-workspace".into();
        assert!(matches!(
            envelope.validate(),
            Err(CommunicationError::InvalidEnvelope(_))
        ));
    }

    #[test]
    fn envelope_rejects_incomplete_identity_and_empty_optional_ids() {
        let endpoint = AgentRef {
            agent_id: "agent-a".into(),
            session_id: "session-a".into(),
            workspace_id: "workspace-a".into(),
            pane_id: "pane-a".into(),
            generation: 2,
            lease: "lease-2".into(),
        };
        let base = || AgentEnvelope {
            message_id: "message-1".into(),
            idempotency_key: "idem-1".into(),
            correlation_id: None,
            causation_id: None,
            conversation_id: None,
            task_id: None,
            from: endpoint.clone(),
            to: endpoint.clone(),
            workspace_id: "workspace-a".into(),
            kind: MessageKind::Message,
            sequence: 1,
            timestamp_unix_ms: 11,
            priority: MessagePriority::Event,
            deadline_unix_ms: None,
            cancellation_id: None,
            payload: serde_json::json!({}),
            artifact_ref: None,
            ack: AckState::None,
        };

        type InvalidFieldCase = (&'static str, fn(&mut AgentEnvelope));
        let cases: [InvalidFieldCase; 7] = [
            ("from.agent_id", |envelope: &mut AgentEnvelope| {
                envelope.from.agent_id = " ".into()
            }),
            ("from.session_id", |envelope: &mut AgentEnvelope| {
                envelope.from.session_id = " ".into()
            }),
            ("from.pane_id", |envelope: &mut AgentEnvelope| {
                envelope.from.pane_id = " ".into()
            }),
            ("from.lease", |envelope: &mut AgentEnvelope| {
                envelope.from.lease = " ".into()
            }),
            ("from.generation", |envelope: &mut AgentEnvelope| {
                envelope.from.generation = 0
            }),
            ("correlation_id", |envelope: &mut AgentEnvelope| {
                envelope.correlation_id = Some(" ".into())
            }),
            ("artifact_ref", |envelope: &mut AgentEnvelope| {
                envelope.artifact_ref = Some(" ".into())
            }),
        ];
        for (label, mutate) in cases {
            let mut envelope = base();
            mutate(&mut envelope);
            let error = envelope.validate().expect_err(label);
            assert!(matches!(error, CommunicationError::InvalidEnvelope(_)));
        }

        let mut zero_sequence = base();
        zero_sequence.sequence = 0;
        assert!(matches!(
            zero_sequence.validate(),
            Err(CommunicationError::InvalidEnvelope(_))
        ));
    }
}
