//! Discriminated envelope for the `teammate-layout-changed` Tauri event.
//!
//! Historically each backend emit site sent an ad-hoc payload shape
//! (`{trace_id}` / `{reused, pane_id}` / `{detached_pane}` / `()` / `null`),
//! which forced the front-end to blindly re-sync on every notification. This
//! module converges every emit onto a single tagged envelope so the front-end
//! can dispatch deterministically on `kind`.
//!
//! Serialized wire shape: `{ "kind": "<snake_case>", ...payload }`.

use serde::Serialize;

/// Tauri event name for all teammate layout-change notifications.
pub(crate) const TEAMMATE_LAYOUT_CHANGED: &str = "teammate-layout-changed";

/// A single teammate layout-change notification.
///
/// Optional payload fields are omitted from the wire form when absent so the
/// envelope stays minimal. The `kind` tag is the single source of truth for
/// the discriminant shared with the front-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum LayoutChange {
    /// A brand-new pane was created by splitting an existing leaf.
    Split { trace_id: String },
    /// An idle pane was re-used instead of creating a new leaf.
    Reused { pane_id: String },
    /// A native (summoned) pane was detached or its child process died.
    Detached { pane_id: String },
    /// A pane was torn down (activation failure, spawn watchdog, or kill).
    Removed {
        #[serde(skip_serializing_if = "Option::is_none")]
        pane_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
    },
    /// Generic pane-state refresh with no pane-specific payload (agent
    /// register/release, rename, new-window, summon). The front-end simply
    /// re-syncs the layout from authoritative backend state.
    State,
}

impl LayoutChange {
    /// New pane created via split; carries the split trace id.
    pub(crate) fn split(trace_id: impl Into<String>) -> Self {
        Self::Split {
            trace_id: trace_id.into(),
        }
    }

    /// Idle pane re-used in place of a new leaf.
    pub(crate) fn reused(pane_id: impl Into<String>) -> Self {
        Self::Reused {
            pane_id: pane_id.into(),
        }
    }

    /// Native pane detached or its child process died.
    pub(crate) fn detached(pane_id: impl Into<String>) -> Self {
        Self::Detached {
            pane_id: pane_id.into(),
        }
    }

    /// Pane torn down; carries the affected pane id.
    pub(crate) fn removed(pane_id: impl Into<String>) -> Self {
        Self::Removed {
            pane_id: Some(pane_id.into()),
            trace_id: None,
        }
    }

    /// Pane torn down during activation; carries both the pane id and the
    /// originating split trace id for cross-referencing the failed spawn.
    pub(crate) fn removed_with_trace(
        pane_id: impl Into<String>,
        trace_id: impl Into<String>,
    ) -> Self {
        Self::Removed {
            pane_id: Some(pane_id.into()),
            trace_id: Some(trace_id.into()),
        }
    }

    /// Generic layout re-sync with no pane-specific payload.
    pub(crate) const fn state() -> Self {
        Self::State
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_name_is_stable() {
        assert_eq!(TEAMMATE_LAYOUT_CHANGED, "teammate-layout-changed");
    }

    #[test]
    fn split_serializes_with_kind_and_trace_id() {
        let value = serde_json::to_value(LayoutChange::split("trace-1")).unwrap();
        assert_eq!(value, json!({ "kind": "split", "trace_id": "trace-1" }));
    }

    #[test]
    fn reused_serializes_with_kind_and_pane_id() {
        let value = serde_json::to_value(LayoutChange::reused("pane-uuid")).unwrap();
        assert_eq!(value, json!({ "kind": "reused", "pane_id": "pane-uuid" }));
    }

    #[test]
    fn detached_serializes_with_kind_and_pane_id() {
        let value = serde_json::to_value(LayoutChange::detached("pane-uuid")).unwrap();
        assert_eq!(value, json!({ "kind": "detached", "pane_id": "pane-uuid" }));
    }

    #[test]
    fn removed_serializes_with_pane_id_only() {
        let value = serde_json::to_value(LayoutChange::removed("pane-uuid")).unwrap();
        assert_eq!(value, json!({ "kind": "removed", "pane_id": "pane-uuid" }));
    }

    #[test]
    fn removed_with_trace_serializes_both_fields() {
        let value =
            serde_json::to_value(LayoutChange::removed_with_trace("pane-uuid", "trace-9")).unwrap();
        assert_eq!(
            value,
            json!({ "kind": "removed", "pane_id": "pane-uuid", "trace_id": "trace-9" })
        );
    }

    #[test]
    fn state_serializes_to_kind_only() {
        let value = serde_json::to_value(LayoutChange::state()).unwrap();
        assert_eq!(value, json!({ "kind": "state" }));
    }

    #[test]
    fn every_variant_carries_a_kind_tag() {
        for change in [
            LayoutChange::split("t"),
            LayoutChange::reused("p"),
            LayoutChange::detached("p"),
            LayoutChange::removed("p"),
            LayoutChange::removed_with_trace("p", "t"),
            LayoutChange::state(),
        ] {
            let value = serde_json::to_value(&change).unwrap();
            assert!(
                value.get("kind").and_then(|k| k.as_str()).is_some(),
                "variant {change:?} must serialize with a string `kind` tag"
            );
        }
    }

    /// Shared single-source-of-truth fixture, also consumed by the front-end
    /// parser test (`src/lib/teammate/layoutEvent.test.ts`). Reconstruct each
    /// variant from the golden entry's own fields, re-serialize, and assert it
    /// equals the entry byte-for-byte — proving the Rust emitter and the golden
    /// (hence the front-end) agree on the wire shape. See M1.
    const GOLDEN_JSON: &str =
        include_str!("../../../src/lib/teammate/layoutChange.golden.json");

    #[test]
    fn golden_envelopes_round_trip() {
        let golden: serde_json::Value = serde_json::from_str(GOLDEN_JSON).unwrap();
        let obj = golden.as_object().expect("golden is a JSON object");
        let mut checked = 0;
        for (name, entry) in obj {
            if name.starts_with('_') {
                continue; // metadata (e.g. `_comment`), not an envelope
            }
            let kind = entry["kind"].as_str().unwrap_or_else(|| panic!("case {name}: no kind"));
            let pane_id = entry.get("pane_id").and_then(|v| v.as_str());
            let trace_id = entry.get("trace_id").and_then(|v| v.as_str());
            let change = match kind {
                "split" => LayoutChange::split(trace_id.expect("split needs trace_id")),
                "reused" => LayoutChange::reused(pane_id.expect("reused needs pane_id")),
                "detached" => LayoutChange::detached(pane_id.expect("detached needs pane_id")),
                "removed" => match trace_id {
                    Some(t) => LayoutChange::removed_with_trace(
                        pane_id.expect("removed needs pane_id"),
                        t,
                    ),
                    None => LayoutChange::removed(pane_id.expect("removed needs pane_id")),
                },
                "state" => LayoutChange::state(),
                other => panic!("case {name}: golden has unknown kind {other}"),
            };
            let reserialized = serde_json::to_value(&change).unwrap();
            assert_eq!(&reserialized, entry, "golden case {name} round-trip mismatch");
            checked += 1;
        }
        assert!(checked >= 6, "expected ≥6 golden envelopes, checked {checked}");
    }
}
