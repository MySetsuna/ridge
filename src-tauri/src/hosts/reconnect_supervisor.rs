//! Outbound reconnect supervisor (AC4-C5).
//!
//! Product: multi-host disconnect → scheduled resubscribe with **cancelable**
//! attempts; drop supervisor kills in-flight work (no orphan reconnect loops).
//! Uses shared reconnect_policy delays; does not spawn OS git (that stays in
//! process_guard) — this supervisor owns **logical** host reconnect tasks.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::outbound::OutboundClient;

/// Max automatic attempts before marking host failed (matches outbound delay schedule).
pub const MAX_RECONNECT_ATTEMPTS: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorPhase {
    Idle,
    Waiting,
    Resubscribing,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug)]
struct HostReconnectState {
    phase: SupervisorPhase,
    attempt: u32,
    attached_panes: Vec<String>,
    cancelled: Arc<AtomicBool>,
    last_error: Option<String>,
}

/// Process-wide supervisor keyed by host_id.
#[derive(Default)]
pub struct ReconnectSupervisor {
    hosts: Mutex<HashMap<String, HostReconnectState>>,
    pub stats: SupervisorStats,
}

#[derive(Default)]
pub struct SupervisorStats {
    pub schedules: AtomicU64,
    pub successes: AtomicU64,
    pub failures: AtomicU64,
    pub cancels: AtomicU64,
    pub attempts: AtomicU64,
}

impl ReconnectSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedule reconnect for a disconnected host with currently attached panes.
    pub fn schedule(&self, host_id: &str, attached_panes: Vec<String>) {
        let mut map = self.hosts.lock();
        // Cancel prior loop for this host.
        if let Some(prev) = map.get_mut(host_id) {
            prev.cancelled.store(true, Ordering::SeqCst);
            self.stats.cancels.fetch_add(1, Ordering::SeqCst);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        map.insert(
            host_id.to_string(),
            HostReconnectState {
                phase: SupervisorPhase::Waiting,
                attempt: 0,
                attached_panes,
                cancelled,
                last_error: None,
            },
        );
        self.stats.schedules.fetch_add(1, Ordering::SeqCst);
    }

    pub fn cancel(&self, host_id: &str) -> bool {
        let mut map = self.hosts.lock();
        if let Some(st) = map.get_mut(host_id) {
            st.cancelled.store(true, Ordering::SeqCst);
            st.phase = SupervisorPhase::Cancelled;
            self.stats.cancels.fetch_add(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn phase(&self, host_id: &str) -> Option<SupervisorPhase> {
        self.hosts.lock().get(host_id).map(|s| s.phase)
    }

    pub fn attempt(&self, host_id: &str) -> Option<u32> {
        self.hosts.lock().get(host_id).map(|s| s.attempt)
    }

    pub fn is_cancelled(&self, host_id: &str) -> bool {
        self.hosts
            .lock()
            .get(host_id)
            .map(|s| s.cancelled.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// One synchronous reconnect step (testable without threads).
    /// Returns delay before next step, or None if terminal.
    pub fn step_once(
        &self,
        host_id: &str,
        client: &OutboundClient,
        host_reachable: bool,
    ) -> Option<Duration> {
        let (cancelled, attempt, panes) = {
            let mut map = self.hosts.lock();
            let st = map.get_mut(host_id)?;
            if st.cancelled.load(Ordering::SeqCst) {
                st.phase = SupervisorPhase::Cancelled;
                return None;
            }
            if st.phase == SupervisorPhase::Succeeded
                || st.phase == SupervisorPhase::Failed
                || st.phase == SupervisorPhase::Cancelled
            {
                return None;
            }
            (
                st.cancelled.clone(),
                st.attempt,
                st.attached_panes.clone(),
            )
        };

        if cancelled.load(Ordering::SeqCst) {
            self.set_phase(host_id, SupervisorPhase::Cancelled, None);
            return None;
        }

        if !host_reachable {
            let delay_ms = OutboundClient::reconnect_delay_ms(attempt);
            if delay_ms.is_none() {
                self.set_phase(
                    host_id,
                    SupervisorPhase::Failed,
                    Some("max reconnect attempts".into()),
                );
                self.stats.failures.fetch_add(1, Ordering::SeqCst);
                return None;
            }
            self.bump_attempt(host_id);
            self.set_phase(host_id, SupervisorPhase::Waiting, None);
            return Some(Duration::from_millis(delay_ms.unwrap()));
        }

        // Host reachable: resubscribe
        self.set_phase(host_id, SupervisorPhase::Resubscribing, None);
        self.stats.attempts.fetch_add(1, Ordering::SeqCst);
        if cancelled.load(Ordering::SeqCst) {
            self.set_phase(host_id, SupervisorPhase::Cancelled, None);
            return None;
        }
        match client.reconnect_resubscribe(&panes) {
            Ok(()) => {
                if cancelled.load(Ordering::SeqCst) {
                    // Cancel won the race after success — still mark cancelled.
                    self.set_phase(host_id, SupervisorPhase::Cancelled, None);
                    return None;
                }
                self.set_phase(host_id, SupervisorPhase::Succeeded, None);
                self.stats.successes.fetch_add(1, Ordering::SeqCst);
                None
            }
            Err(e) => {
                let next = {
                    let mut map = self.hosts.lock();
                    let st = map.get_mut(host_id)?;
                    st.attempt = st.attempt.saturating_add(1);
                    st.last_error = Some(e.clone());
                    st.attempt
                };
                let delay_ms = OutboundClient::reconnect_delay_ms(next);
                if delay_ms.is_none() {
                    self.set_phase(host_id, SupervisorPhase::Failed, Some(e));
                    self.stats.failures.fetch_add(1, Ordering::SeqCst);
                    None
                } else {
                    self.set_phase(host_id, SupervisorPhase::Waiting, Some(e));
                    Some(Duration::from_millis(delay_ms.unwrap()))
                }
            }
        }
    }

    fn set_phase(&self, host_id: &str, phase: SupervisorPhase, err: Option<String>) {
        if let Some(st) = self.hosts.lock().get_mut(host_id) {
            st.phase = phase;
            if err.is_some() {
                st.last_error = err;
            }
        }
    }

    fn bump_attempt(&self, host_id: &str) {
        if let Some(st) = self.hosts.lock().get_mut(host_id) {
            st.attempt = st.attempt.saturating_add(1);
        }
        self.stats.attempts.fetch_add(1, Ordering::SeqCst);
    }

    pub fn last_error(&self, host_id: &str) -> Option<String> {
        self.hosts
            .lock()
            .get(host_id)
            .and_then(|s| s.last_error.clone())
    }

    pub fn snapshot_stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.stats.schedules.load(Ordering::SeqCst),
            self.stats.successes.load(Ordering::SeqCst),
            self.stats.failures.load(Ordering::SeqCst),
            self.stats.cancels.load(Ordering::SeqCst),
            self.stats.attempts.load(Ordering::SeqCst),
        )
    }
}

/// Pure decision: should we keep retrying?
pub fn should_continue_reconnect(attempt: u32, cancelled: bool, succeeded: bool) -> bool {
    if cancelled || succeeded {
        return false;
    }
    attempt < MAX_RECONNECT_ATTEMPTS
}

/// Isolation: pane ids must not be claimed by two live reconnect tasks.
pub fn check_pane_isolation(tasks: &[(String, Vec<String>)]) -> Result<(), String> {
    let mut owner: HashMap<String, String> = HashMap::new();
    for (host, panes) in tasks {
        for p in panes {
            if let Some(prev) = owner.get(p) {
                if prev != host {
                    return Err(format!(
                        "pane {p} claimed by {prev} and {host}"
                    ));
                }
            } else {
                owner.insert(p.clone(), host.clone());
            }
        }
    }
    Ok(())
}

impl ReconnectSupervisor {
    /// All host ids currently tracked.
    pub fn tracked_hosts(&self) -> Vec<String> {
        self.hosts.lock().keys().cloned().collect()
    }

    /// Attached panes for a host (empty if unknown).
    pub fn attached_panes(&self, host_id: &str) -> Vec<String> {
        self.hosts
            .lock()
            .get(host_id)
            .map(|s| s.attached_panes.clone())
            .unwrap_or_default()
    }

    /// Isolation check across all scheduled hosts.
    pub fn assert_isolation(&self) -> Result<(), String> {
        let tasks: Vec<(String, Vec<String>)> = self
            .hosts
            .lock()
            .iter()
            .map(|(h, s)| (h.clone(), s.attached_panes.clone()))
            .collect();
        check_pane_isolation(&tasks)
    }

    /// Snapshot phase string for UI.
    pub fn phase_str(&self, host_id: &str) -> Option<&'static str> {
        self.phase(host_id).map(|p| match p {
            SupervisorPhase::Idle => "Idle",
            SupervisorPhase::Waiting => "Waiting",
            SupervisorPhase::Resubscribing => "Resubscribing",
            SupervisorPhase::Succeeded => "Succeeded",
            SupervisorPhase::Failed => "Failed",
            SupervisorPhase::Cancelled => "Cancelled",
        })
    }

    /// After a successful reconnect, collapse to Idle so Hosts poll can stop
    /// stepping (constructs `SupervisorPhase::Idle`).
    pub fn mark_idle(&self, host_id: &str) {
        if let Some(st) = self.hosts.lock().get_mut(host_id) {
            if st.phase == SupervisorPhase::Succeeded || st.phase == SupervisorPhase::Cancelled {
                st.phase = SupervisorPhase::Idle;
                st.attempt = 0;
                st.last_error = None;
                st.cancelled.store(false, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::outbound::{MockOutboundTransport, OutboundClient, RemoteSessionInfo};

    fn client_with_mock() -> (Arc<MockOutboundTransport>, Arc<OutboundClient>) {
        let mock = Arc::new(MockOutboundTransport::new());
        mock.preset_list(&[RemoteSessionInfo {
            id: "main".into(),
            title: "t".into(),
        }]);
        let c = Arc::new(OutboundClient::new("lan:h", mock.clone()));
        c.connect_and_list().unwrap();
        (mock, c)
    }

    #[test]
    fn schedule_then_success_when_reachable() {
        let sup = ReconnectSupervisor::new();
        let (_m, client) = client_with_mock();
        client.subscribe("main").unwrap();
        client.mark_disconnected();
        sup.schedule("lan:h", vec!["main".into()]);
        assert_eq!(sup.phase("lan:h"), Some(SupervisorPhase::Waiting));
        let delay = sup.step_once("lan:h", &client, true);
        assert!(delay.is_none());
        assert_eq!(sup.phase("lan:h"), Some(SupervisorPhase::Succeeded));
        assert!(client.is_subscribed("main"));
        let (sch, ok, _f, _c, _a) = sup.snapshot_stats();
        assert_eq!(sch, 1);
        assert_eq!(ok, 1);
    }

    #[test]
    fn cancel_stops_waiting_loop() {
        let sup = ReconnectSupervisor::new();
        let (_m, client) = client_with_mock();
        sup.schedule("lan:h", vec!["main".into()]);
        assert!(sup.cancel("lan:h"));
        let d = sup.step_once("lan:h", &client, false);
        assert!(d.is_none());
        assert_eq!(sup.phase("lan:h"), Some(SupervisorPhase::Cancelled));
    }

    #[test]
    fn unreachable_exhausts_attempts() {
        let sup = ReconnectSupervisor::new();
        let (_m, client) = client_with_mock();
        sup.schedule("lan:h", vec!["main".into()]);
        let mut steps = 0;
        loop {
            steps += 1;
            if steps > 10 {
                panic!("did not terminate");
            }
            match sup.step_once("lan:h", &client, false) {
                Some(_) => continue,
                None => break,
            }
        }
        assert_eq!(sup.phase("lan:h"), Some(SupervisorPhase::Failed));
        let (_s, _ok, fail, _c, _a) = sup.snapshot_stats();
        assert!(fail >= 1);
    }

    #[test]
    fn should_continue_matrix() {
        assert!(should_continue_reconnect(0, false, false));
        assert!(!should_continue_reconnect(0, true, false));
        assert!(!should_continue_reconnect(0, false, true));
        assert!(!should_continue_reconnect(4, false, false));
    }

    #[test]
    fn reschedule_cancels_previous() {
        let sup = ReconnectSupervisor::new();
        let (_m, client) = client_with_mock();
        sup.schedule("lan:h", vec!["main".into()]);
        let first_cancel = sup.hosts.lock().get("lan:h").unwrap().cancelled.clone();
        sup.schedule("lan:h", vec!["main".into()]);
        assert!(first_cancel.load(Ordering::SeqCst));
        let (_s, _ok, _f, cancels, _) = sup.snapshot_stats();
        assert!(cancels >= 1);
        let _ = client; // keep
    }

    #[test]
    fn multi_host_isolation_and_phase_str() {
        let sup = ReconnectSupervisor::new();
        sup.schedule("h1", vec!["p1".into()]);
        sup.schedule("h2", vec!["p2".into()]);
        assert!(sup.assert_isolation().is_ok());
        assert_eq!(sup.phase_str("h1"), Some("Waiting"));
        assert!(sup.tracked_hosts().len() >= 2);
        assert_eq!(sup.attached_panes("h1"), vec!["p1".to_string()]);

        assert!(check_pane_isolation(&[
            ("h1".into(), vec!["shared".into()]),
            ("h2".into(), vec!["shared".into()]),
        ])
        .is_err());
    }

    #[test]
    fn mark_idle_after_success_uses_idle_phase() {
        let sup = ReconnectSupervisor::new();
        let (_m, client) = client_with_mock();
        client.subscribe("main").unwrap();
        client.mark_disconnected();
        sup.schedule("lan:h", vec!["main".into()]);
        let _ = sup.step_once("lan:h", &client, true);
        assert_eq!(sup.phase("lan:h"), Some(SupervisorPhase::Succeeded));
        assert!(sup.attempt("lan:h").is_some());
        assert!(!sup.is_cancelled("lan:h"));
        sup.mark_idle("lan:h");
        assert_eq!(sup.phase("lan:h"), Some(SupervisorPhase::Idle));
        assert_eq!(sup.phase_str("lan:h"), Some("Idle"));
        assert_eq!(sup.attempt("lan:h"), Some(0));
        assert!(sup.last_error("lan:h").is_none());
    }
}
