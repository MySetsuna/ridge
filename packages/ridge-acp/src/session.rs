//! `AgentSession` — the canonical Agent product object.
//!
//! One `AgentSession` per concept (Lead / Staff / solo). It owns:
//! - id, status, capabilities
//! - messages + tool calls + approvals
//! - plan / tasks / artifacts
//! - optional terminal / runtime associations (out of scope for M1 wire shape;
//!   placeholders exist so future slices can attach without breaking the model)
//!
//! Turn-level types (TurnRecord / TurnPhase / ToolCallRecord / record_plan /
//! finalize_turn_success / etc.) live in `turn.rs`. This module owns the
//! `AgentSession` lifecycle and the canonical `SessionStatus` state machine.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::error::{AcpErrorWrap, AgentSessionError};
use crate::events::AgentSessionEvent;
use crate::lifecycle::LifecycleView;
use crate::protocol::{
    AcpResponse, ClientInfoField, InitializeParams, InitializeResult, ProtocolVersionField,
    SessionCancelParams, SessionNewParams, SessionNewResult, SessionPromptParams,
    SessionPromptResult, METHOD_INITIALIZE, METHOD_SESSION_APPROVE, METHOD_SESSION_CANCEL,
    METHOD_SESSION_NEW, METHOD_SESSION_PROMPT,
};
use crate::provider::ProviderSpec;
use crate::transport::AcpTransport;
pub use crate::turn::{PlanSnapshot, ToolCallRecord, ToolCallView, TurnId, TurnPhase, TurnRecord};

/// Stable id (UUID v4) for one `AgentSession`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);
impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }
}
impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Commands accepted by AgentSession via `dispatch`. These are how Communion /
/// future UI tell the session to act. They do NOT contain state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentSessionCommand {
    Prompt {
        session_id: SessionId,
        turn_id: TurnId,
        text: String,
    },
    ApproveTool {
        session_id: SessionId,
        turn_id: TurnId,
        tool_call_id: String,
    },
    DenyTool {
        session_id: SessionId,
        turn_id: TurnId,
        tool_call_id: String,
        reason: String,
    },
    Cancel {
        session_id: SessionId,
        turn_id: Option<TurnId>,
    },
}

/// Lead -> Staff task assignment record (consumed by Communion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaffAssignment {
    pub staff_session_id: SessionId,
    pub task_title: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionConfig {
    pub provider: ProviderSpec,
    pub client_name: String,
    pub client_version: String,
}

impl AgentSessionConfig {
    pub fn mock_default() -> Self {
        Self {
            provider: ProviderSpec::mock_default(),
            client_name: "ridge-acp".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

#[derive(Clone)]
pub struct AgentSession {
    pub(crate) inner: Arc<AsyncMutex<SessionInner>>,
    /// Real transport after `connect_into`. None until then. The Mutex is
    /// held only across synchronous reads/writes (no await), so concurrent
    /// `call` / `notify` / `prompt_loop::run` never deadlock on the lock —
    /// each critical section is short.
    pub(crate) transport: Arc<AsyncMutex<Option<AcpTransport>>>,
    /// Keep the child process handle alive so its stdio pipes stay open.
    /// Dropping the Child kills the process and surfaces as SessionClosed.
    pub(crate) _child: Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
    pub(crate) events: broadcast::Sender<AgentSessionEvent>,
    pub(crate) status: Arc<AsyncMutex<SessionStatus>>,
}

pub(crate) struct SessionInner {
    pub(crate) session_id: SessionId,
    pub(crate) config: AgentSessionConfig,
    pub(crate) lifecycle: LifecycleView,
    pub(crate) turns: HashMap<TurnId, TurnRecord>,
    pub(crate) active_turn: Option<TurnId>,
    pub(crate) pending_approval: Option<(TurnId, String)>,
    pub(crate) remote_session_id: Option<String>,
    pub(crate) initialize_result: Option<InitializeResult>,
}

/// Canonical session status (M2 §4 semantics).
///
/// - `Starting` — handshake in progress (initialize → session/new).
/// - `Running` — open and ready for `prompt`.
/// - `WaitingApproval` — agent has requested approval for a tool call.
/// - `Completed` — session explicitly closed successfully.
/// - `Failed` — session failed (transport error / protocol violation).
/// - `Cancelled` — session cancelled by host or agent.
///
/// Single state machine. UI and Communion MUST consume this enum and MUST NOT
/// derive status from per-event observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Starting,
    Running,
    WaitingApproval,
    Completed,
    Failed,
    Cancelled,
}

impl AgentSession {
    /// Prepare a session with its broadcast channel already wired up.
    pub fn prepare(config: AgentSessionConfig) -> (Self, broadcast::Receiver<AgentSessionEvent>) {
        let session_id = SessionId::new();
        let (tx, rx) = broadcast::channel(256);
        let inner = SessionInner {
            session_id: session_id.clone(),
            config: config.clone(),
            lifecycle: LifecycleView::default(),
            turns: HashMap::new(),
            active_turn: None,
                pending_approval: None,
            remote_session_id: None,
            initialize_result: None,
        };
        let s = Self {
            inner: Arc::new(AsyncMutex::new(inner)),
            transport: Arc::new(AsyncMutex::new(None)),
            _child: Arc::new(tokio::sync::Mutex::new(None)),
            events: tx,
            status: Arc::new(AsyncMutex::new(SessionStatus::Starting)),
        };
        (s, rx)
    }

    /// Current canonical session status. UI / Communion MUST use this and
    /// MUST NOT derive status from raw event observation.
    pub async fn status(&self) -> SessionStatus {
        *self.status.lock().await
    }

    pub(crate) async fn set_status(&self, s: SessionStatus) {
        let mut g = self.status.lock().await;
        *g = s;
    }

    /// Connect to a transport, run `initialize`, then run `session/new`.
    pub async fn connect(
        transport: AcpTransport,
        child: tokio::process::Child,
        config: AgentSessionConfig,
    ) -> Result<Self, AcpErrorWrap> {
        let (s, _rx) = Self::prepare(config);
        if let Err(error) = s.connect_into(transport, child).await { s.shutdown().await; return Err(error); }
        Ok(s)
    }

    /// Like `connect` but assumes `Self::prepare` already ran.
    pub async fn connect_into(
        &self,
        transport: AcpTransport,
        child: tokio::process::Child,
    ) -> Result<(), AcpErrorWrap> {
        {
            let mut t = self.transport.lock().await;
            *t = Some(transport);
        }
        {
            let mut c = self._child.lock().await;
            *c = Some(child);
        }
        let config = self.inner.lock().await.config.clone();

        let init_params = InitializeParams {
            protocol_version: ProtocolVersionField::from(crate::protocol::ACP_PROTOCOL_VERSION),
            client_info: ClientInfoField(serde_json::Map::from_iter([
                ("name".into(), serde_json::json!(config.client_name)),
                ("version".into(), serde_json::json!(config.client_version)),
            ])),
            capabilities: serde_json::Map::new(),
        };
        let resp: AcpResponse = {
            let t = self.transport.lock().await.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            t.call(
                METHOD_INITIALIZE,
                serde_json::to_value(&init_params).unwrap_or_default(),
            )
            .await?
        };
        if let Some(err) = resp.error.clone() {
            self.set_status(SessionStatus::Failed).await;
            return Err(err.into());
        }
        let init: InitializeResult = serde_json::from_value(resp.result.unwrap_or_default())
            .map_err(|e| AcpErrorWrap::InvalidFrame(format!("initialize result: {e}")))?;
        self.inner.lock().await.initialize_result = Some(init);

        let new_params = SessionNewParams::with_cwd(config.provider.config.get("cwd").and_then(|value| value.as_str()).map(str::to_string));
        let resp: AcpResponse = {
            let t = self.transport.lock().await.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            t.call(
                METHOD_SESSION_NEW,
                serde_json::to_value(&new_params).unwrap_or_default(),
            )
            .await?
        };
        if let Some(err) = resp.error.clone() {
            self.set_status(SessionStatus::Failed).await;
            return Err(err.into());
        }
        let new_result: SessionNewResult =
            serde_json::from_value(resp.result.unwrap_or_default())
                .map_err(|e| AcpErrorWrap::InvalidFrame(format!("session/new result: {e}")))?;
        self.inner.lock().await.remote_session_id = Some(new_result.session_id);
        self.inner.lock().await.lifecycle =
            LifecycleView::new(ridge_core::teammate::communication::AgentLifecycle::Online);
        let session_id = self.id().await;
        let _ = self.events.send(AgentSessionEvent::SessionCreated {
            session_id: session_id.clone(),
            provider: config.provider.clone(),
            ts_unix_ms: crate::prompt_loop::now_unix_ms(),
        });
        let _ = self.events.send(AgentSessionEvent::StateChanged {
            session_id,
            lifecycle: LifecycleView::new(
                ridge_core::teammate::communication::AgentLifecycle::Online,
            ),
            ts_unix_ms: crate::prompt_loop::now_unix_ms(),
        });
        self.set_status(SessionStatus::Running).await;
        Ok(())
    }

    pub async fn id(&self) -> SessionId {
        self.inner.lock().await.session_id.clone()
    }

    /// M3: provider spec this session was opened with.
    pub async fn provider(&self) -> ProviderSpec {
        self.inner.lock().await.config.provider.clone()
    }

    /// Captured `initialize` result from the agent. `None` until connect_into
    /// completes successfully.
    pub async fn initialize_result(&self) -> Option<InitializeResult> {
        self.inner.lock().await.initialize_result.clone()
    }

    pub async fn snapshot(&self) -> crate::turn::AgentSessionSnapshot {
        let g = self.inner.lock().await;
        crate::turn::AgentSessionSnapshot {
            session_id: g.session_id.clone(),
            provider: g.config.provider.clone(),
            lifecycle: g.lifecycle,
            turns: g.turns.clone(),
            active_turn: g.active_turn.clone(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentSessionEvent> {
        self.events.subscribe()
    }

    /// Idempotent transport shutdown. Closes the writer channel and signals
    /// the reader + writer + stderr-drain tasks to exit. The next `prompt` /
    /// `approve_tool` / `cancel` call will return `SessionClosed`. Useful
    /// for the M2.1 Tauri bridge lifecycle (clean session teardown on
    /// desktop unmount or route remount) and for E2E tests that need to
    /// force provider EOF without owning the child handle.
    ///
    /// Also emits a `SessionFailed` event on the canonical event bus so
    /// M2.1 §4 bridge listeners observe a terminal signal even when no
    /// prompt was in flight at shutdown time.
    pub async fn shutdown(&self) {
        let session_id = self.id().await;
        self.inner.lock().await.pending_approval = None;
        let t = self.transport.lock().await;
        if let Some(t) = t.as_ref() {
            t.shutdown();
        }
        drop(t);
        // Best-effort: drop the child handle so the OS reaps it even if the
        // agent did not exit cleanly on stdin close.
        if let Some(mut child) = self._child.lock().await.take() {
            let _ = child.kill().await;
        }
        // Emit terminal signal so any consumer subscribed via `subscribe()`
        // observes that the session has died. Without this, the bridge
        // would hang waiting for an event that prompt_loop never emitted
        // (prompt_loop only runs during a prompt).
        let cancelled = self.status().await == SessionStatus::Cancelled;
        if !cancelled { self.set_status(SessionStatus::Failed).await; }
        self.emit(crate::events::AgentSessionEvent::SessionFailed {
            session_id,
            error: crate::protocol::AcpErrorObj {
                code: 0,
                message: if cancelled { "cancelled" } else { "session shutdown" }.into(),
                data: None,
            },
            ts_unix_ms: crate::prompt_loop::now_unix_ms(),
        });
    }

    /// Emit a canonical event. Errors are intentionally swallowed — broadcast
    /// has no back-pressure and the consumer must tolerate lag.
    pub(crate) fn emit(&self, ev: AgentSessionEvent) {
        let _ = self.events.send(ev);
    }

    /// Issue one prompt and drive the session to terminal state.
    pub async fn prompt(&self, text: &str) -> Result<TurnId, AcpErrorWrap> {
        let turn_id = TurnId(Uuid::new_v4().to_string());
        let session_id = self.id().await;
        let remote_id = {
            let g = self.inner.lock().await;
            g.remote_session_id.clone().ok_or(AgentSessionError::SessionNotOpen(
                "remote_session_id missing".into(),
            ))?
        };

        {
            let mut g = self.inner.lock().await;
            if g.active_turn.is_some() {
                return Err(AcpErrorWrap::InvalidFrame("a turn is already active".into()));
            }
            g.turns.insert(
                turn_id.clone(),
                TurnRecord::new(turn_id.clone(), text.to_string()),
            );
            g.active_turn = Some(turn_id.clone());
            g.turns.get_mut(&turn_id).unwrap().phase = TurnPhase::InProgress;
        }

        let _ = self.events.send(AgentSessionEvent::TurnStarted {
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
            prompt: text.to_string(),
            ts_unix_ms: crate::prompt_loop::now_unix_ms(),
        });

        // Subscribe to the notification broadcast BEFORE the wire call: agents
        // such as `live_acp_agent` emit `session/update` notifications (e.g.
        // `plan`, `thinking_chunk`) on stdout BEFORE sending the response to
        // `session/prompt`. If we subscribed after the wire call returned,
        // those early notifications would already be in the broadcast buffer
        // and our receiver would miss them — leading to a missing
        // `PlanChanged` event. (See `real_acp_e2e_full_happy_path` flake.)
        let mut notif_rx = {
            let t = self.transport.lock().await.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            t.subscribe_notifications()
        };

        let params = SessionPromptParams {
            session_id: remote_id,
            prompt: vec![crate::protocol::PromptPart::Text {
                text: text.to_string(),
            }],
        };
        let resp: AcpResponse = {
            let t = self.transport.lock().await.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            t.call(
                METHOD_SESSION_PROMPT,
                serde_json::to_value(&params).unwrap_or_default(),
            )
            .await?
        };
        if let Some(err) = resp.error.clone() {
            let mut g = self.inner.lock().await;
            if let Some(t) = g.turns.get_mut(&turn_id) {
                t.phase = TurnPhase::Failed;
                t.result = crate::turn::TurnResult {
                    phase: TurnPhase::Failed,
                    message: err.message.clone(),
                };
            }
            drop(g);
            let _ = self.events.send(AgentSessionEvent::TurnFailed {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                error: err,
                ts_unix_ms: crate::prompt_loop::now_unix_ms(),
            });
            self.set_status(SessionStatus::Failed).await;
            return Err(AcpErrorWrap::Session(AgentSessionError::AgentFailed {
                code: 0,
                message: "session/prompt failed".into(),
            }));
        }
        let prompt_result: SessionPromptResult =
            serde_json::from_value(resp.result.unwrap_or_default())
                .map_err(|e| AcpErrorWrap::InvalidFrame(format!("session/prompt result: {e}")))?;

        crate::prompt_loop::run(
            self,
            &mut notif_rx,
            &session_id,
            &turn_id,
            &prompt_result.status,
        )
        .await
    }

    pub async fn approve_tool(&self, turn_id: &TurnId, tool_call_id: &str) -> Result<(), AcpErrorWrap> {
        self.resolve_approval(turn_id, tool_call_id, true).await
    }
    pub async fn reject_tool(&self, turn_id: &TurnId, tool_call_id: &str) -> Result<(), AcpErrorWrap> {
        self.resolve_approval(turn_id, tool_call_id, false).await
    }
    async fn resolve_approval(&self, turn_id: &TurnId, tool_call_id: &str, approved: bool) -> Result<(), AcpErrorWrap> {
        let pending = (turn_id.clone(), tool_call_id.to_string());
        let remote_id = {
            let mut inner = self.inner.lock().await;
            if inner.pending_approval.as_ref() != Some(&pending) || inner.active_turn.as_ref() != Some(turn_id) {
                return Err(AcpErrorWrap::InvalidFrame("no live approval for this turn/tool".into()));
            }
            let remote = inner.remote_session_id.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            inner.pending_approval = None; // Atomically consume once, before a wire await.
            remote
        };
        let wire = async {
            let transport = self.transport.lock().await.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            transport.notify(METHOD_SESSION_APPROVE, serde_json::json!({ "session_id": remote_id, "turn_id": turn_id.0, "tool_call_id": tool_call_id, "approved": approved })).await
        }.await;
        if let Err(e) = wire {
            let mut inner = self.inner.lock().await;
            if inner.active_turn.as_ref() == Some(turn_id) && inner.pending_approval.is_none() { inner.pending_approval = Some(pending); }
            return Err(e);
        }
        self.emit(AgentSessionEvent::ApprovalResolved { session_id: self.id().await, turn_id: turn_id.clone(), tool_call_id: tool_call_id.into(), approved, ts_unix_ms: crate::prompt_loop::now_unix_ms() });
        let no_pending = self.inner.lock().await.pending_approval.is_none();
        if no_pending && self.status().await == SessionStatus::WaitingApproval { self.set_status(SessionStatus::Running).await; }
        Ok(())
    }

    /// Host-initiated cancel. Pre-marks the active turn's `TurnPhase` and the
    /// session's `SessionStatus` as `Cancelled` BEFORE issuing the wire call.
    /// This is the canonical signal for any concurrent `prompt_loop`
    /// `TurnFailed` handler to preserve `TurnPhase::Cancelled` (single source
    /// of truth — `TurnPhase` and `SessionStatus` stay in lockstep). The wire
    /// call is best-effort and its result is intentionally ignored.
    pub async fn cancel(&self, turn_id: Option<&TurnId>) -> Result<(), AcpErrorWrap> {
        let remote_id = {
            let g = self.inner.lock().await;
            g.remote_session_id.clone().ok_or(AgentSessionError::SessionNotOpen(
                "remote_session_id missing".into(),
            ))?
        };
        let params = SessionCancelParams {
            session_id: remote_id,
            turn_id: turn_id.cloned().map(|t| t.0),
        };

        // Pre-mark active turn phase as Cancelled. Acquired under the same
        // `inner` lock the prompt loop would touch, so any concurrent
        // `TurnFailed` dispatch sees `TurnPhase::Cancelled` when it reads
        // the turn record.
        {
            let mut g = self.inner.lock().await;
            let target_turn = turn_id
                .cloned()
                .or_else(|| g.active_turn.clone());
            g.pending_approval = None;
            if let Some(tid) = target_turn {
                if let Some(t) = g.turns.get_mut(&tid) {
                    t.phase = TurnPhase::Cancelled;
                    t.result = crate::turn::TurnResult {
                        phase: TurnPhase::Cancelled,
                        message: "cancelled".to_string(),
                    };
                }
            }
        }
        self.set_status(SessionStatus::Cancelled).await;

        // Best-effort wire notification. Even on failure, the host intent
        // (cancel) is already recorded above. We log+ignore transport errors
        // so the caller cannot observe a phantom success.
        let wire_result: Result<(), AcpErrorWrap> = (async {
            let t = self.transport.lock().await.clone().ok_or(AcpErrorWrap::SessionClosed)?;
            t.notify(METHOD_SESSION_CANCEL, serde_json::to_value(&params).unwrap_or_default()).await?;
            Ok(())
        })
        .await;
        if let Err(e) = wire_result {
            eprintln!("ridge-acp cancel: wire call failed (status already Cancelled): {e}");
        }
        Ok(())
    }
}
