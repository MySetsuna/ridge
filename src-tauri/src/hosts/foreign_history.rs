//! Foreign pane scrollback seed (AC4-C6).
//!
//! On attach, host may provide a **tail** of recent PTY bytes. We store per
//! remote session and inject into the local parser once on attach — no full
//! CRDT, no entire history dump (product: first screen tail only).

use parking_lot::RwLock;
use std::collections::HashMap;

/// Default max bytes retained per remote session for attach seed.
pub const DEFAULT_HISTORY_TAIL_CAP: usize = 64 * 1024;

#[derive(Default)]
pub struct ForeignHistoryStore {
    /// (host_id, remote_pane_id) → ring-tail bytes
    tails: RwLock<HashMap<(String, String), Vec<u8>>>,
    cap: RwLock<usize>,
}

impl ForeignHistoryStore {
    pub fn new() -> Self {
        Self {
            tails: RwLock::new(HashMap::new()),
            cap: RwLock::new(DEFAULT_HISTORY_TAIL_CAP),
        }
    }

    pub fn set_cap(&self, cap: usize) {
        *self.cap.write() = cap.max(1);
    }

    pub fn cap(&self) -> usize {
        *self.cap.read()
    }

    /// Append live output into history tail (capped).
    pub fn append(&self, host_id: &str, remote_pane_id: &str, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let cap = self.cap();
        let key = (host_id.to_string(), remote_pane_id.to_string());
        let mut map = self.tails.write();
        let buf = map.entry(key).or_default();
        crate::hosts::outbound::append_capped(buf, bytes, cap);
    }

    /// Snapshot current tail (for attach seed).
    pub fn tail(&self, host_id: &str, remote_pane_id: &str) -> Vec<u8> {
        self.tails
            .read()
            .get(&(host_id.to_string(), remote_pane_id.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    pub fn clear_session(&self, host_id: &str, remote_pane_id: &str) {
        self.tails
            .write()
            .remove(&(host_id.to_string(), remote_pane_id.to_string()));
    }

    pub fn clear_host(&self, host_id: &str) {
        self.tails.write().retain(|(h, _), _| h != host_id);
    }

    /// Seed parser with tail bytes. Returns bytes fed.
    pub fn seed_parser_feed(
        &self,
        host_id: &str,
        remote_pane_id: &str,
        feed: impl FnOnce(&[u8]),
    ) -> usize {
        let t = self.tail(host_id, remote_pane_id);
        let n = t.len();
        if n > 0 {
            feed(&t);
        }
        n
    }
}

/// Decide how many bytes of history to request from host (protocol hint).
pub fn history_pull_budget(want_lines: u16, cols: u16) -> usize {
    // Rough: lines * cols * 2 (UTF-8/SGR slack), capped by DEFAULT_HISTORY_TAIL_CAP.
    let raw = (want_lines as usize)
        .saturating_mul(cols as usize)
        .saturating_mul(2);
    raw.min(DEFAULT_HISTORY_TAIL_CAP).max(1024)
}

/// Attach-seed plan (product policy, pure).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachSeedPlan {
    pub seed_before_live: bool,
    pub seed_bytes: usize,
    pub pull_budget: usize,
    pub clear_after_seed: bool,
    pub reason: &'static str,
}

pub fn plan_attach_seed(
    local_tail_bytes: usize,
    rows: u16,
    cols: u16,
    reattach: bool,
    host_history_known: bool,
) -> AttachSeedPlan {
    let pull_budget = history_pull_budget(rows.max(1), cols.max(1));
    if local_tail_bytes > 0 {
        return AttachSeedPlan {
            seed_before_live: true,
            seed_bytes: local_tail_bytes,
            pull_budget,
            clear_after_seed: reattach,
            reason: if reattach {
                "reattach_local_tail"
            } else {
                "first_attach_local_tail"
            },
        };
    }
    if host_history_known {
        return AttachSeedPlan {
            seed_before_live: true,
            seed_bytes: 0,
            pull_budget,
            clear_after_seed: false,
            reason: "host_pull_then_seed",
        };
    }
    AttachSeedPlan {
        seed_before_live: false,
        seed_bytes: 0,
        pull_budget,
        clear_after_seed: false,
        reason: "no_history_live_only",
    }
}

impl ForeignHistoryStore {
    /// Session count (for control-plane diagnostics).
    pub fn session_count(&self) -> usize {
        self.tails.read().len()
    }

    /// Total retained bytes across all sessions.
    pub fn total_bytes(&self) -> usize {
        self.tails.read().values().map(|v| v.len()).sum()
    }

    /// Keys for host (isolation audits).
    pub fn sessions_for_host(&self, host_id: &str) -> Vec<String> {
        self.tails
            .read()
            .keys()
            .filter(|(h, _)| h == host_id)
            .map(|(_, s)| s.clone())
            .collect()
    }

    /// Seed once then optionally clear (reattach policy).
    pub fn seed_parser_feed_once(
        &self,
        host_id: &str,
        remote_pane_id: &str,
        clear_after: bool,
        feed: impl FnOnce(&[u8]),
    ) -> usize {
        let n = self.seed_parser_feed(host_id, remote_pane_id, feed);
        if clear_after && n > 0 {
            self.clear_session(host_id, remote_pane_id);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_caps_and_tail_returns_suffix() {
        let store = ForeignHistoryStore::new();
        store.set_cap(8);
        store.append("h", "p", b"0123456789ABCDEF");
        let t = store.tail("h", "p");
        assert!(t.len() <= 8);
        assert!(t.ends_with(b"F") || t.len() == 8);
    }

    #[test]
    fn seed_feeds_once() {
        let store = ForeignHistoryStore::new();
        store.append("h", "p", b"hello-history");
        let mut got = Vec::new();
        let n = store.seed_parser_feed("h", "p", |b| got.extend_from_slice(b));
        assert_eq!(n, got.len());
        assert!(got.windows(5).any(|w| w == b"hello"));
    }

    #[test]
    fn clear_session_and_host() {
        let store = ForeignHistoryStore::new();
        store.append("h1", "p1", b"a");
        store.append("h1", "p2", b"b");
        store.append("h2", "p1", b"c");
        store.clear_session("h1", "p1");
        assert!(store.tail("h1", "p1").is_empty());
        assert!(!store.tail("h1", "p2").is_empty());
        store.clear_host("h1");
        assert!(store.tail("h1", "p2").is_empty());
        assert!(!store.tail("h2", "p1").is_empty());
    }

    #[test]
    fn history_pull_budget_bounds() {
        assert!(history_pull_budget(24, 80) >= 1024);
        assert!(history_pull_budget(10_000, 200) <= DEFAULT_HISTORY_TAIL_CAP);
    }

    #[test]
    fn plan_attach_and_seed_once_clear() {
        let p = plan_attach_seed(100, 24, 80, true, false);
        assert!(p.seed_before_live);
        assert!(p.clear_after_seed);
        let store = ForeignHistoryStore::new();
        store.append("h", "p", b"seed-data");
        let mut got = Vec::new();
        let n = store.seed_parser_feed_once("h", "p", true, |b| got.extend_from_slice(b));
        assert_eq!(n, got.len());
        assert!(store.tail("h", "p").is_empty());
        assert_eq!(store.session_count(), 0);
    }

    #[test]
    fn multi_host_isolation_keys() {
        let store = ForeignHistoryStore::new();
        store.append("h1", "s1", b"a");
        store.append("h2", "s1", b"b");
        assert_eq!(store.sessions_for_host("h1"), vec!["s1".to_string()]);
        assert_eq!(store.total_bytes(), 2);
        store.clear_host("h1");
        assert_eq!(store.total_bytes(), 1);
    }
}
