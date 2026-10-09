//! `LifecycleView` — read-only projection of `AgentLifecycle` for AgentSession
//! snapshots. Wraps (does not duplicate) the upstream state machine.
//!
//! Note: `AgentLifecycle` (from `ridge_core::teammate::communication`) is the
//! single source of truth for lifecycle states. We do not introduce new
//! variants here.

use serde::{Deserialize, Serialize};

pub use ridge_core::teammate::communication::AgentLifecycle;

/// Thin newtype that gives us a stable host-side surface for snapshot
/// serialization without forcing consumers to depend on the upstream enum
/// directly. The inner state is the canonical state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleView {
    pub inner: AgentLifecycle,
}

impl LifecycleView {
    pub const fn new(inner: AgentLifecycle) -> Self {
        Self { inner }
    }

    /// Terminal = Completed | Stopped | Failed. We do NOT treat Working /
    /// Attention as terminal — the agent may still produce more output.
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self.inner,
            AgentLifecycle::Completed | AgentLifecycle::Stopped | AgentLifecycle::Failed
        )
    }

    pub const fn as_str(&self) -> &'static str {
        match self.inner {
            AgentLifecycle::Discovered => "Discovered",
            AgentLifecycle::Spawning => "Spawning",
            AgentLifecycle::Attaching => "Attaching",
            AgentLifecycle::Online => "Online",
            AgentLifecycle::Working => "Working",
            AgentLifecycle::Waiting => "Waiting",
            AgentLifecycle::Attention => "Attention",
            AgentLifecycle::Completed => "Completed",
            AgentLifecycle::Stopped => "Stopped",
            AgentLifecycle::Failed => "Failed",
        }
    }
}

impl Default for LifecycleView {
    fn default() -> Self {
        Self::new(AgentLifecycle::Discovered)
    }
}

impl From<AgentLifecycle> for LifecycleView {
    fn from(inner: AgentLifecycle) -> Self {
        Self::new(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_tag_roundtrip() {
        let v = LifecycleView::new(AgentLifecycle::Online);
        let s = serde_json::to_string(&v).unwrap();
        assert!(s.contains("\"Online\""), "got: {s}");
        let back: LifecycleView = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn terminal_classification() {
        assert!(!LifecycleView::new(AgentLifecycle::Discovered).is_terminal());
        assert!(!LifecycleView::new(AgentLifecycle::Spawning).is_terminal());
        assert!(!LifecycleView::new(AgentLifecycle::Attaching).is_terminal());
        assert!(!LifecycleView::new(AgentLifecycle::Online).is_terminal());
        assert!(!LifecycleView::new(AgentLifecycle::Working).is_terminal());
        assert!(!LifecycleView::new(AgentLifecycle::Waiting).is_terminal());
        assert!(!LifecycleView::new(AgentLifecycle::Attention).is_terminal());
        assert!(LifecycleView::new(AgentLifecycle::Completed).is_terminal());
        assert!(LifecycleView::new(AgentLifecycle::Stopped).is_terminal());
        assert!(LifecycleView::new(AgentLifecycle::Failed).is_terminal());
    }
}
