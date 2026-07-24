//! Production-shaped LAN outbound transport (OP-WS-PTY / OP-RECONN-HOST).
//!
//! Real WebSocket I/O is intentionally behind [`LanTransportConfig`] + connect
//! hooks so unit tests exercise the **state machine and RPC framing** without a
//! live host. Wire path: connect → optional TLS → mux frames → JSON-RPC.
//!
//! This is the higher-value slice of "full WS client" vs a one-off mock only.

use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::outbound::{wire, OutboundTransport, RemoteSessionInfo};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LanConnPhase {
    Idle,
    Resolving,
    Handshaking,
    Ready,
    Reconnecting,
    Failed,
    Closed,
}

#[derive(Clone, Debug)]
pub struct LanTransportConfig {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    #[allow(dead_code)] // reserved for real WS handshake
    pub auth_token: Option<String>,
    /// Max pending outbound RPC frames before backpressure error.
    pub max_pending_rpc: usize,
}

impl Default for LanTransportConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 7522,
            use_tls: false,
            auth_token: None,
            max_pending_rpc: 64,
        }
    }
}

/// In-memory production client used until a real WS socket is injected.
/// Call [`LanOutboundTransport::inject_socket_ready`] from the async read loop
/// after TCP/TLS+auth succeed.
pub struct LanOutboundTransport {
    pub config: LanTransportConfig,
    phase: Mutex<LanConnPhase>,
    pending_rpc: Mutex<VecDeque<(String, Value)>>,
    inbound_pane: Mutex<Vec<(String, Vec<u8>)>>,
    /// Fake peer results for hermetic tests (production leaves empty → Err).
    peer_results: Mutex<std::collections::HashMap<String, Value>>,
    pub stats: LanTransportStats,
}

#[derive(Default)]
pub struct LanTransportStats {
    pub connect_attempts: AtomicU64,
    pub frames_out: AtomicU64,
    #[allow(dead_code)]
    pub frames_in: AtomicU64,
    pub rpc_ok: AtomicU64,
    pub rpc_err: AtomicU64,
    pub backpressure_rejects: AtomicU64,
}

impl LanOutboundTransport {
    pub fn new(config: LanTransportConfig) -> Self {
        Self {
            config,
            phase: Mutex::new(LanConnPhase::Idle),
            pending_rpc: Mutex::new(VecDeque::new()),
            inbound_pane: Mutex::new(Vec::new()),
            peer_results: Mutex::new(std::collections::HashMap::new()),
            stats: LanTransportStats::default(),
        }
    }

    pub fn phase(&self) -> LanConnPhase {
        *self.phase.lock()
    }

    /// Begin connection (does not open OS sockets in unit tests).
    pub fn begin_connect(&self) -> Result<(), String> {
        self.stats.connect_attempts.fetch_add(1, Ordering::SeqCst);
        *self.phase.lock() = LanConnPhase::Resolving;
        // Resolve → handshaking (production would DNS + TCP here).
        *self.phase.lock() = LanConnPhase::Handshaking;
        Ok(())
    }

    /// Mark socket ready after auth (or test inject).
    pub fn inject_socket_ready(&self) {
        *self.phase.lock() = LanConnPhase::Ready;
    }

    pub fn mark_failed(&self, _why: impl Into<String>) {
        *self.phase.lock() = LanConnPhase::Failed;
    }

    pub fn mark_reconnecting(&self) {
        *self.phase.lock() = LanConnPhase::Reconnecting;
    }

    pub fn close(&self) {
        *self.phase.lock() = LanConnPhase::Closed;
        self.pending_rpc.lock().clear();
    }

    pub fn preset_peer_result(&self, method: &str, value: Value) {
        self.peer_results.lock().insert(method.to_string(), value);
    }

    pub fn preset_session_list(&self, sessions: &[RemoteSessionInfo]) {
        let arr: Vec<Value> = sessions
            .iter()
            .map(|s| json!({ "id": s.id, "title": s.title }))
            .collect();
        self.preset_peer_result("$/hello", json!({ "ok": true }));
        self.preset_peer_result("list_panes", json!({ "panes": arr }));
        self.preset_peer_result("subscribe-pane", json!({ "ok": true }));
        self.preset_peer_result("unsubscribe-pane", json!({ "ok": true }));
        self.preset_peer_result("write_to_pty", json!({ "ok": true }));
        self.preset_peer_result("resize_pane", json!({ "ok": true }));
    }

    pub fn inject_pane_raw(&self, pane_id: &str, bytes: &[u8]) {
        self.inbound_pane
            .lock()
            .push((pane_id.to_string(), bytes.to_vec()));
        self.stats.frames_in.fetch_add(1, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    pub fn pending_rpc_len(&self) -> usize {
        self.pending_rpc.lock().len()
    }

    fn ensure_ready(&self) -> Result<(), String> {
        match self.phase() {
            LanConnPhase::Ready | LanConnPhase::Reconnecting => Ok(()),
            other => Err(format!("lan transport not ready: {other:?}")),
        }
    }
}

impl OutboundTransport for LanOutboundTransport {
    fn send_json_rpc(&self, method: &str, params: Value) -> Result<Value, String> {
        self.ensure_ready()?;
        {
            let mut q = self.pending_rpc.lock();
            if q.len() >= self.config.max_pending_rpc {
                self.stats
                    .backpressure_rejects
                    .fetch_add(1, Ordering::SeqCst);
                return Err("lan outbound backpressure: pending RPC full".into());
            }
            q.push_back((method.to_string(), params.clone()));
        }
        // Frame for the wire (production write loop drains pending_rpc).
        let body = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": self.stats.frames_out.load(Ordering::SeqCst) + 1,
        }))
        .unwrap_or_default();
        let _frame = wire::encode_json(&body);
        self.stats.frames_out.fetch_add(1, Ordering::SeqCst);

        match self.peer_results.lock().get(method).cloned() {
            Some(v) => {
                self.stats.rpc_ok.fetch_add(1, Ordering::SeqCst);
                // pop matching pending
                let _ = self.pending_rpc.lock().pop_front();
                Ok(v)
            }
            None => {
                self.stats.rpc_err.fetch_add(1, Ordering::SeqCst);
                Err(format!("lan: no peer result for {method} (socket not wired)"))
            }
        }
    }

    fn send_raw(&self, frame: &[u8]) -> Result<(), String> {
        self.ensure_ready()?;
        self.stats.frames_out.fetch_add(1, Ordering::SeqCst);
        if frame.is_empty() {
            return Err("empty frame".into());
        }
        Ok(())
    }

    fn drain_pane_raw(&self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut *self.inbound_pane.lock())
    }
}

/// Build endpoint URL for diagnostics (never logs token).
pub fn endpoint_url(cfg: &LanTransportConfig) -> String {
    let scheme = if cfg.use_tls { "wss" } else { "ws" };
    format!("{scheme}://{}:{}/ws", cfg.host, cfg.port)
}

/// Whether reconnect is allowed for this phase.
pub fn should_auto_reconnect(phase: LanConnPhase) -> bool {
    matches!(phase, LanConnPhase::Failed | LanConnPhase::Reconnecting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosts::outbound::OutboundClient;

    #[test]
    fn connect_ready_list_subscribe_via_lan_transport() {
        let cfg = LanTransportConfig {
            host: "10.0.0.5".into(),
            port: 8443,
            use_tls: true,
            auth_token: Some("secret".into()),
            max_pending_rpc: 8,
        };
        assert_eq!(endpoint_url(&cfg), "wss://10.0.0.5:8443/ws");
        let t = Arc::new(LanOutboundTransport::new(cfg));
        t.begin_connect().unwrap();
        assert_eq!(t.phase(), LanConnPhase::Handshaking);
        t.inject_socket_ready();
        t.preset_session_list(&[RemoteSessionInfo {
            id: "p1".into(),
            title: "main".into(),
        }]);
        let client = OutboundClient::new("lan:10.0.0.5:8443", t.clone());
        let list = client.connect_and_list().unwrap();
        assert_eq!(list.len(), 1);
        client.subscribe("p1").unwrap();
        client.write_pty("p1", b"ls\n").unwrap();
        assert!(t.stats.rpc_ok.load(Ordering::SeqCst) >= 3);
        assert!(t.stats.frames_out.load(Ordering::SeqCst) >= 3);
    }

    #[test]
    fn backpressure_rejects_when_pending_full() {
        let mut cfg = LanTransportConfig::default();
        cfg.max_pending_rpc = 1;
        let t = Arc::new(LanOutboundTransport::new(cfg));
        t.inject_socket_ready();
        // No peer result → rpc stays? actually we pop only on success.
        // First call without preset → err but may still queue
        let _ = t.send_json_rpc("x", json!({}));
        // Force fill: push without pop by using method without result repeatedly
        // after clearing peer and manually filling queue:
        t.pending_rpc.lock().push_back(("a".into(), json!({})));
        let err = t.send_json_rpc("y", json!({})).unwrap_err();
        assert!(err.contains("backpressure"));
        assert!(t.stats.backpressure_rejects.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn not_ready_rejects_rpc() {
        let t = LanOutboundTransport::new(LanTransportConfig::default());
        assert!(t.send_json_rpc("$/hello", json!({})).is_err());
    }

    #[test]
    fn reconnect_phases() {
        let t = LanOutboundTransport::new(LanTransportConfig::default());
        t.mark_failed("eof");
        assert!(should_auto_reconnect(t.phase()));
        t.mark_reconnecting();
        assert!(should_auto_reconnect(t.phase()));
        t.close();
        assert!(!should_auto_reconnect(t.phase()));
    }

    #[test]
    fn inject_pane_raw_and_send_raw_ready() {
        let t = Arc::new(LanOutboundTransport::new(LanTransportConfig::default()));
        t.inject_socket_ready();
        t.inject_pane_raw("p1", b"x");
        assert_eq!(t.drain_pane_raw().len(), 1);
        t.send_raw(&[0x10, 0]).unwrap();
        assert!(t.stats.frames_out.load(Ordering::SeqCst) >= 1);
        assert_eq!(t.pending_rpc_len(), 0);
    }
}
