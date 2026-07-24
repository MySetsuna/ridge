//! Live output backpressure registry (AC4-C8 product vertical).
//!
//! Complements `HostRegistry::inject_live_output` / `append_capped` with
//! **per-session** drop counters and aggregate snapshots for Hosts UI.
//! Pure policy helpers mirror TS `liveBackpressure` / `livePumpPolicy`.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Level for operator badges (parity with TS BackpressureLevel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackpressureLevel {
    Ok,
    Elevated,
    Shedding,
}

impl BackpressureLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            BackpressureLevel::Ok => "ok",
            BackpressureLevel::Elevated => "elevated",
            BackpressureLevel::Shedding => "shedding",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SessionBp {
    pub buffered: u64,
    pub dropped: u64,
    pub high_water: u64,
}

#[derive(Default)]
pub struct LiveBackpressureRegistry {
    /// (host_id, session_id) → counters
    sessions: RwLock<HashMap<(String, String), SessionBp>>,
    cap: RwLock<u64>,
    total_dropped: AtomicU64,
    injects: AtomicU64,
}

impl LiveBackpressureRegistry {
    pub fn new(cap: u64) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            cap: RwLock::new(cap.max(1)),
            total_dropped: AtomicU64::new(0),
            injects: AtomicU64::new(0),
        }
    }

    pub fn set_cap(&self, cap: u64) {
        *self.cap.write() = cap.max(1);
    }

    pub fn cap(&self) -> u64 {
        *self.cap.read()
    }

    /// Record an inject outcome (bytes kept in buffer + bytes dropped).
    pub fn record_inject(
        &self,
        host_id: &str,
        session_id: &str,
        buffered_after: u64,
        dropped: u64,
    ) {
        self.injects.fetch_add(1, Ordering::SeqCst);
        if dropped > 0 {
            self.total_dropped.fetch_add(dropped, Ordering::SeqCst);
        }
        let key = (host_id.to_string(), session_id.to_string());
        let mut map = self.sessions.write();
        let e = map.entry(key).or_default();
        e.buffered = buffered_after;
        e.dropped = e.dropped.saturating_add(dropped);
        e.high_water = e.high_water.max(buffered_after);
    }

    pub fn session(&self, host_id: &str, session_id: &str) -> SessionBp {
        self.sessions
            .read()
            .get(&(host_id.to_string(), session_id.to_string()))
            .cloned()
            .unwrap_or_default()
    }

    pub fn total_dropped(&self) -> u64 {
        self.total_dropped.load(Ordering::SeqCst)
    }

    pub fn inject_count(&self) -> u64 {
        self.injects.load(Ordering::SeqCst)
    }

    pub fn clear_host(&self, host_id: &str) {
        self.sessions.write().retain(|(h, _), _| h != host_id);
    }

    pub fn clear_session(&self, host_id: &str, session_id: &str) {
        self.sessions
            .write()
            .remove(&(host_id.to_string(), session_id.to_string()));
    }

    /// Aggregate snapshot for Hosts header / diagnostics.
    pub fn aggregate_for_host(&self, host_id: &str) -> AggregateBp {
        let cap = self.cap();
        let map = self.sessions.read();
        let mut buffered = 0u64;
        let mut dropped = 0u64;
        let mut high = 0u64;
        let mut sessions = 0u32;
        let mut shedding = 0u32;
        for ((h, _), s) in map.iter() {
            if h != host_id {
                continue;
            }
            sessions += 1;
            buffered = buffered.saturating_add(s.buffered);
            dropped = dropped.saturating_add(s.dropped);
            high = high.max(s.high_water);
            if classify_level(s.buffered, cap, s.dropped) == BackpressureLevel::Shedding {
                shedding += 1;
            }
        }
        let effective_cap = cap.saturating_mul(sessions.max(1) as u64);
        AggregateBp {
            host_id: host_id.to_string(),
            cap,
            buffered,
            dropped,
            high_water: high,
            sessions,
            shedding_sessions: shedding,
            level: classify_level(buffered, effective_cap, dropped).as_str(),
            total_dropped_global: self.total_dropped(),
            injects: self.inject_count(),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregateBp {
    pub host_id: String,
    pub cap: u64,
    pub buffered: u64,
    pub dropped: u64,
    pub high_water: u64,
    pub sessions: u32,
    pub shedding_sessions: u32,
    pub level: &'static str,
    pub total_dropped_global: u64,
    pub injects: u64,
}

pub fn classify_level(buffered: u64, cap: u64, dropped: u64) -> BackpressureLevel {
    if cap == 0 {
        return BackpressureLevel::Ok;
    }
    if dropped > 0 || buffered * 100 / cap >= 95 {
        return BackpressureLevel::Shedding;
    }
    if buffered * 100 / cap >= 70 {
        return BackpressureLevel::Elevated;
    }
    BackpressureLevel::Ok
}

/// Bytes to drop from head when appending (parity with TS / append_capped).
pub fn bytes_to_drop_on_append(current: usize, incoming: usize, cap: usize) -> usize {
    if cap == 0 {
        return incoming;
    }
    if incoming >= cap {
        return current + (incoming - cap);
    }
    let next = current + incoming;
    if next <= cap {
        0
    } else {
        next - cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_aggregate_shedding() {
        let reg = LiveBackpressureRegistry::new(100);
        reg.record_inject("h1", "s1", 100, 20);
        reg.record_inject("h1", "s2", 10, 0);
        let agg = reg.aggregate_for_host("h1");
        assert_eq!(agg.sessions, 2);
        assert!(agg.dropped >= 20);
        assert_eq!(agg.shedding_sessions, 1);
        assert_eq!(reg.total_dropped(), 20);
        assert!(reg.inject_count() >= 2);
    }

    #[test]
    fn level_matrix() {
        assert_eq!(classify_level(10, 100, 0), BackpressureLevel::Ok);
        assert_eq!(classify_level(80, 100, 0), BackpressureLevel::Elevated);
        assert_eq!(classify_level(50, 100, 1), BackpressureLevel::Shedding);
        assert_eq!(classify_level(95, 100, 0), BackpressureLevel::Shedding);
    }

    #[test]
    fn drop_math_matches_policy() {
        assert_eq!(bytes_to_drop_on_append(0, 10, 100), 0);
        assert_eq!(bytes_to_drop_on_append(90, 20, 100), 10);
        assert_eq!(bytes_to_drop_on_append(0, 200, 100), 100);
    }

    #[test]
    fn clear_host_isolates() {
        let reg = LiveBackpressureRegistry::new(64);
        reg.record_inject("h1", "s", 1, 0);
        reg.record_inject("h2", "s", 1, 0);
        reg.clear_host("h1");
        assert_eq!(reg.session("h1", "s").buffered, 0);
        assert_eq!(reg.session("h2", "s").buffered, 1);
    }
}
