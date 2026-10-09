//! Turn-level types and record-keeping helpers for `AgentSession`.
//!
//! Single-responsibility: the canonical `TurnRecord` shape + the mutators
//! `AgentSession` calls when a prompt loop dispatches agent notifications.
//! Separated from `session.rs` to keep the session surface readable.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::lifecycle::LifecycleView;
use crate::session::AgentSession;

/// Stable id for one prompt turn (issued by `session/prompt`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TurnId(pub String);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnPhase {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnResult {
    pub phase: TurnPhase,
    pub message: String,
}

impl Default for TurnResult {
    fn default() -> Self {
        Self {
            phase: TurnPhase::Pending,
            message: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub turn_id: TurnId,
    pub prompt: String,
    pub phase: TurnPhase,
    pub result: TurnResult,
    pub tool_calls: Vec<ToolCallRecord>,
    pub plan: PlanSnapshot,
}

impl TurnRecord {
    pub fn new(turn_id: TurnId, prompt: String) -> Self {
        Self {
            turn_id,
            prompt,
            phase: TurnPhase::Pending,
            result: TurnResult::default(),
            tool_calls: Vec::new(),
            plan: PlanSnapshot::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanSnapshot {
    pub entries: Vec<PlanEntryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanEntryView {
    pub id: String,
    pub title: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Map<String, serde_json::Value>,
    pub kind: String,
    pub status: String,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallView {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Map<String, serde_json::Value>,
    pub kind: String,
}

/// Snapshot of one session — what a consumer renders. Single canonical read
/// surface; there is no parallel state anywhere else.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSessionSnapshot {
    pub session_id: crate::session::SessionId,
    pub provider: crate::provider::ProviderSpec,
    pub lifecycle: LifecycleView,
    pub turns: HashMap<TurnId, TurnRecord>,
    pub active_turn: Option<TurnId>,
}

impl AgentSessionSnapshot {
    pub fn session_id(&self) -> &crate::session::SessionId {
        &self.session_id
    }
}

// ---------- mutators invoked by `prompt_loop` ----------------------------

impl AgentSession {
    /// Generic turn finalization used by `prompt_loop`. The phase is the
    /// canonical `TurnPhase` — `prompt_loop` derives this from the active
    /// `SessionStatus` (e.g. Cancelled preservation) so `TurnPhase` and
    /// `SessionStatus` never disagree.
    pub(crate) async fn finalize_turn_with_phase(
        &self,
        turn_id: &TurnId,
        phase: TurnPhase,
        message: &str,
    ) {
        let mut g = self.inner.lock().await;
        if let Some(t) = g.turns.get_mut(turn_id) {
            t.phase = phase;
            t.result = TurnResult {
                phase,
                message: message.to_string(),
            };
        }
        g.active_turn = None;
    }

    pub(crate) async fn record_plan(&self, turn_id: &TurnId, plan: PlanSnapshot) {
        let mut g = self.inner.lock().await;
        if let Some(t) = g.turns.get_mut(turn_id) {
            t.plan = plan;
        }
    }

    pub(crate) async fn record_tool_call_started(
        &self,
        turn_id: &TurnId,
        rec: ToolCallRecord,
    ) {
        let mut g = self.inner.lock().await;
        if let Some(t) = g.turns.get_mut(turn_id) {
            t.tool_calls.push(rec);
        }
    }

    pub(crate) async fn record_tool_call_finished(
        &self,
        turn_id: &TurnId,
        tool_call_id: &str,
        status: &str,
    ) {
        let mut g = self.inner.lock().await;
        if let Some(t) = g.turns.get_mut(turn_id) {
            if let Some(tc) = t.tool_calls.iter_mut().find(|tc| tc.id == tool_call_id) {
                tc.status = status.to_string();
            }
        }
    }

    pub(crate) async fn finalize_turn_success(
        &self,
        turn_id: &TurnId,
        phase: TurnPhase,
        message: &str,
    ) {
        self.finalize_turn_with_phase(turn_id, phase, message).await;
    }
}
