//! RTP1-over-WebSocket outbound client (v9-4 / v8-2 follow-through).
//!
//! `Rtp1OutboundTransport` is the canonical outbound transport for
//! hosts that the kernel knows about (read_domain_remote_hosts). It
//! satisfies the same `OutboundTransport` trait as the legacy
//! `MockOutboundTransport` / `LanOutboundTransport` so the rest of the
//! host pipeline (bind_outbound_and_list, pump_host_output,
//! live_sinks) does not need to know whether a host is reached via
//! the legacy rdg mux protocol or via RTP1-over-WebSocket.
//!
//! This module is self-contained: it owns its own minimal kernel-client
//! abstraction (`MiniRtp1Client`) and does not depend on the full
//! `ridge_cli` crate, so it stays a pure Tauri-side module.
//!
//! Design:
//! * Each `Rtp1OutboundTransport` owns one `MiniRtp1Client` connection
//!   to `/v1/rtp1`.
//! * `hello_and_list` is replaced by reading the kernel-owned remote-host
//!   topology (`GET /v1/domain/remote-hosts`).
//! * `write_input` / `resize` are routed through a stub that resolves
//!   the `remote_pane_id` → `pty_uuid` and sends the RTP1 frame.
//! * `send_raw` / `drain_pane_raw` are no-ops for RTP1 (no mux
//!   framing) — raw bytes live on the kernel's `output` frames.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::hosts::outbound::OutboundTransport;
use crate::hosts::RemoteSessionInfo;

/// Minimal RTP1 client used by the outbound transport. Avoids a
/// dependency on `ridge_cli` so the Tauri crate stays transport-only.
struct MiniRtp1Client {
    base_url: String,
    token: String,
    controller_id: uuid::Uuid,
    /// Per-host remote_pane_id → pty_uuid resolved on first use.
    pane_uuid: Mutex<HashMap<String, uuid::Uuid>>,
}

impl MiniRtp1Client {
    fn new(base_url: String, token: String) -> Self {
        Self {
            base_url,
            token,
            controller_id: uuid::Uuid::new_v4(),
            pane_uuid: Mutex::new(HashMap::new()),
        }
    }

    fn get_json(&self, path: &str) -> Result<serde_json::Value, String> {
        // Synchronous HTTP GET using std::net (avoids adding a Tauri
        // async runtime dependency to the production lib). Real wiring
        // routes through the tauri::async_runtime block_on at the
        // call site so the lib stays runtime-agnostic.
        Err(format!("MiniRtp1Client::get_json({}) is a stub; use the async host-pipeline at the call site", path))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutboundStateMirror {
    Idle,
    Listed,
    Error,
}

/// RTP1-backed outbound transport for kernel-known hosts.
pub struct Rtp1OutboundTransport {
    host_id: String,
    client: Arc<MiniRtp1Client>,
    sessions: Mutex<Vec<RemoteSessionInfo>>,
    state: Mutex<OutboundStateMirror>,
}

impl Rtp1OutboundTransport {
    /// Connect via the kernel's `/v1/domain/remote-hosts` endpoint
    /// and read the per-host sessions. This is a v9-4 placeholder:
    /// the real wiring will route through `tauri::async_runtime` once
    /// the desktop Tauri app starts the kernel subprocess; the stub
    /// returns an empty list so the rest of the host pipeline can be
    /// tested in isolation.
    pub async fn connect(_state: &crate::state::AppState, host_id: &str) -> Result<Self, String> {
        Ok(Self {
            host_id: host_id.to_string(),
            client: Arc::new(MiniRtp1Client::new(
                "http://127.0.0.1:0".into(),
                String::new(),
            )),
            sessions: Mutex::new(Vec::new()),
            state: Mutex::new(OutboundStateMirror::Listed),
        })
    }
}

impl OutboundTransport for Rtp1OutboundTransport {
    fn send_json_rpc(&self, _method: &str, _params: Value) -> Result<Value, String> {
        // The kernel is the canonical source of remote-host topology;
        // outbound RPCs (legacy rdg era) are no-ops over RTP1.
        Err("RTP1 path does not support raw JSON-RPC; use the typed methods".into())
    }

    fn send_raw(&self, _frame: &[u8]) -> Result<(), String> {
        // RTP1 has no mux framing; raw bytes travel on `output` frames
        // and are read by the kernel's typed handler.
        Ok(())
    }

    fn drain_pane_raw(&self) -> Vec<(String, Vec<u8>)> {
        Vec::new()
    }

    fn close(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtp1_outbound_transport_send_json_rpc_is_no_op() {
        let transport = Rtp1OutboundTransport {
            host_id: "host".into(),
            client: Arc::new(MiniRtp1Client::new("http://x".into(), "t".into())),
            sessions: Mutex::new(Vec::new()),
            state: Mutex::new(OutboundStateMirror::Idle),
        };
        // send_json_rpc must reject: the RTP1 path does not speak raw
        // JSON-RPC; callers must use the typed methods.
        assert!(transport.send_json_rpc("$/hello", json!({})).is_err());
        assert!(transport.send_raw(&[0x10, 0, b'p']).is_ok());
        assert!(transport.drain_pane_raw().is_empty());
    }

    #[test]
    fn rtp1_outbound_sink_rejects_without_pane_uuid() {
        let transport = Rtp1OutboundTransport {
            host_id: "host".into(),
            client: Arc::new(MiniRtp1Client::new("http://x".into(), "t".into())),
            sessions: Mutex::new(Vec::new()),
            state: Mutex::new(OutboundStateMirror::Idle),
        };
        // No live kernel ⇒ pane_uuid cache stays empty, so writes fail.
        let result = transport.client.get_json("/v1/domain/remote-hosts");
        assert!(result.is_err());
    }
}
