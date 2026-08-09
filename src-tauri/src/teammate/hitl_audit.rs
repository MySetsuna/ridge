//! HITL decision audit ring (AC4-C7).
//!
//! Extends product path for **remote-readable redacted history** without
//! command full text. Desktop remains authority for full audit; remote gets
//! a capped projection via `list_hitl_audit_remote`.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

const AUDIT_CAP: usize = 50;

#[derive(Clone, Debug)]
pub struct AuditEntry {
    pub id: String,
    pub ts_ms: u64,
    pub source: String, // "desktop" | "remote" | "timeout"
    pub initiator: String,
    pub verdict: String, // approve | reject | timeout
    pub risk_level: String,
    pub reason_summary: String,
    pub outcome: String,
}

static AUDIT: Mutex<VecDeque<AuditEntry>> = Mutex::new(VecDeque::new());
static SEQ: AtomicU64 = AtomicU64::new(1);

pub fn append_audit(mut e: AuditEntry) {
    if e.id.is_empty() {
        e.id = format!("aud_{}", SEQ.fetch_add(1, Ordering::SeqCst));
    }
    let mut g = AUDIT.lock();
    g.push_back(e);
    while g.len() > AUDIT_CAP {
        g.pop_front();
    }
}

/// Redacted projection for remote controllers (no command body).
pub fn list_audit_remote(limit: usize) -> Value {
    let lim = limit.clamp(1, AUDIT_CAP);
    let g = AUDIT.lock();
    let items: Vec<Value> = g
        .iter()
        .rev()
        .take(lim)
        .map(|e| {
            json!({
                "id": e.id,
                "ts": e.ts_ms,
                "source": e.source,
                "initiator": e.initiator,
                "verdict": e.verdict,
                "riskLevel": e.risk_level,
                "reasonSummary": redact_reason_summary(&e.reason_summary, 120),
                "outcome": e.outcome,
            })
        })
        .collect();
    json!({ "items": items, "cap": AUDIT_CAP })
}

pub fn audit_len() -> usize {
    AUDIT.lock().len()
}

#[cfg(test)]
pub fn clear_audit_for_test() {
    AUDIT.lock().clear();
}

/// Whether a remote may see audit list (always true for read projection policy).
pub fn remote_audit_allowed() -> bool {
    true
}

/// Redact free-text that might leak secrets (desktop + remote projection).
pub fn redact_reason_summary(raw: &str, max_len: usize) -> String {
    let mut s: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    // api_key=... / token: ...
    let re_pairs = [(
        r"(?i)(api[_-]?key|token|secret|password|authorization)\s*[:=]\s*\S+",
        "$1=***",
    )];
    for (pat, rep) in re_pairs {
        if let Ok(re) = regex_lite_replace(pat, &s, rep) {
            s = re;
        }
    }
    // long opaque tokens
    s = strip_long_tokens(&s);
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        s
    }
}

fn strip_long_tokens(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            if w.len() >= 32
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                "***"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Minimal replace without full regex crate dependency: simple substring heuristics.
fn regex_lite_replace(_pat: &str, s: &str, _rep: &str) -> Result<String, ()> {
    let lower = s.to_ascii_lowercase();
    let keys = [
        "api_key",
        "apikey",
        "token",
        "secret",
        "password",
        "authorization",
    ];
    let mut out = s.to_string();
    for k in keys {
        if let Some(idx) = lower.find(k) {
            // find = or : after key
            let rest = &out[idx + k.len()..];
            let rest_l = rest.to_ascii_lowercase();
            if let Some(rel) = rest_l.find([':', '=']) {
                let start = idx + k.len() + rel + 1;
                let mut end = start;
                let bytes = out.as_bytes();
                while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
                    end += 1;
                }
                if end > start {
                    out = format!("{}{}***{}", &out[..start], "", &out[end..]);
                    // recompute lower for next keys on mutated string — restart once
                    return Ok(redact_reason_summary_simple(&out));
                }
            }
        }
    }
    Ok(out)
}

fn redact_reason_summary_simple(s: &str) -> String {
    strip_long_tokens(s)
}

/// Filter projection by verdict/source (newest first).
pub fn list_audit_filtered(limit: usize, verdict: Option<&str>, source: Option<&str>) -> Value {
    let lim = limit.clamp(1, AUDIT_CAP);
    let g = AUDIT.lock();
    let items: Vec<Value> = g
        .iter()
        .rev()
        .filter(|e| verdict.map(|v| e.verdict == v).unwrap_or(true))
        .filter(|e| source.map(|s| e.source == s).unwrap_or(true))
        .take(lim)
        .map(|e| {
            json!({
                "id": e.id,
                "ts": e.ts_ms,
                "source": e.source,
                "initiator": e.initiator,
                "verdict": e.verdict,
                "riskLevel": e.risk_level,
                "reasonSummary": redact_reason_summary(&e.reason_summary, 120),
                "outcome": e.outcome,
            })
        })
        .collect();
    json!({ "items": items, "cap": AUDIT_CAP, "filtered": true })
}

/// Counts by verdict for control-plane badges.
pub fn audit_verdict_counts() -> Value {
    let g = AUDIT.lock();
    let mut approve = 0u64;
    let mut reject = 0u64;
    let mut timeout = 0u64;
    let mut other = 0u64;
    for e in g.iter() {
        match e.verdict.as_str() {
            "approve" | "approved" => approve += 1,
            "reject" | "rejected" => reject += 1,
            "timeout" => timeout += 1,
            _ => other += 1,
        }
    }
    json!({ "approve": approve, "reject": reject, "timeout": timeout, "other": other, "total": g.len() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that mutate the process-global AUDIT ring.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn ring_caps_and_redacted_shape() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_audit_for_test();
        for i in 0..60 {
            append_audit(AuditEntry {
                id: String::new(),
                ts_ms: i as u64,
                source: "desktop".into(),
                initiator: format!("a{i}"),
                verdict: "approve".into(),
                risk_level: "Dangerous".into(),
                reason_summary: "rm".into(),
                outcome: "consumed".into(),
            });
        }
        assert_eq!(audit_len(), AUDIT_CAP);
        let v = list_audit_remote(5);
        let items = v["items"].as_array().unwrap();
        assert_eq!(items.len(), 5);
        // newest first
        assert!(items[0]["ts"].as_u64().unwrap() >= items[4]["ts"].as_u64().unwrap());
        // no action/command field
        assert!(items[0].get("action").is_none());
        assert!(items[0].get("command").is_none());
        assert!(remote_audit_allowed());
    }

    #[test]
    fn redact_strips_tokens_and_filter_works() {
        // Pure redact path (no shared ring).
        let secret = "abcdefghijklmnopqrstuvwxyz0123456789";
        let red = redact_reason_summary(&format!("run api_key={secret}"), 120);
        assert!(!red.contains(secret), "redacted={red}");
        assert!(red.contains("***") || red.contains("api_key"), "red={red}");

        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        clear_audit_for_test();
        append_audit(AuditEntry {
            id: "reject_only_marker".into(),
            ts_ms: 9_999_999,
            source: "remote".into(),
            initiator: "b".into(),
            verdict: "reject".into(),
            risk_level: "Dangerous".into(),
            reason_summary: format!("nope api_key={secret}"),
            outcome: "blocked".into(),
        });
        let filtered = list_audit_filtered(50, Some("reject"), Some("remote"));
        let items = filtered["items"].as_array().unwrap();
        assert!(
            items.iter().any(|i| i["id"] == "reject_only_marker"),
            "filter miss: {items:?}"
        );
        for it in items {
            if it["id"] == "reject_only_marker" {
                let rs = it["reasonSummary"].as_str().unwrap_or("");
                assert!(!rs.contains(secret), "leaked secret in {rs}");
            }
        }
        let counts = audit_verdict_counts();
        assert!(counts["total"].as_u64().unwrap() >= 1);
        assert!(counts["reject"].as_u64().unwrap() >= 1);
    }
}
