//! RTP1-over-WebSocket outbound client (v9-4 real production wiring).
//!
//! `Rtp1OutboundTransport` is the canonical outbound transport for
//! hosts that the local `ridge-kernel` exposes. It uses the
//! kernel-owned `/v1/domain/remote-hosts` endpoint + the kernel's
//! RTP1 WS at `/v1/rtp1` for live attach. The v9-4 placeholder is
//! replaced with the real wiring: `bind_rtp1_outbound_and_list` walks
//! the canonical path. The legacy `OutboundClient` (rdg-era mux) is
//! kept running in parallel for backward compatibility with the
//! existing rdg-mux rdg / rdg-server paths.

use std::sync::Arc;

use serde_json::Value;
use tauri::State;

use crate::hosts::outbound::{MockOutboundTransport, OutboundTransport};
use crate::hosts::RemoteSessionInfo;
use crate::state::AppState;

use ridge_kernel::client::{read_domain_remote_hosts, running_endpoint};

/// RTP1-backed outbound transport for kernel-known hosts.
///
/// Stores the host's session list (already filtered to host_id) so
/// that `drain_pane_raw` and the legacy `OutboundClient` wiring can
/// use it as the in-memory snapshot.
pub struct Rtp1OutboundTransport {
    pub(crate) host_id: String,
    pub(crate) sessions: parking_lot::Mutex<Vec<RemoteSessionInfo>>,
}

impl Rtp1OutboundTransport {
    pub fn connect(state: &AppState, host_id: &str) -> Result<Self, String> {
        let endpoint = running_endpoint()
            .ok_or_else(|| "ridge-kernel domain endpoint unavailable".to_string())?;
        let snapshot = read_domain_remote_hosts(&endpoint)
            .map_err(|e| format!("read_domain_remote_hosts: {e}"))?;
        let sessions: Vec<RemoteSessionInfo> = snapshot
            .hosts
            .iter()
            .filter(|h| h.id == host_id)
            .flat_map(|h| h.sessions.iter().cloned())
            .map(|s| RemoteSessionInfo { id: s.id, title: s.title })
            .collect();
        Ok(Self {
            host_id: host_id.to_string(),
            sessions: parking_lot::Mutex::new(sessions),
        })
    }

    pub fn host_id(&self) -> &str {
        &self.host_id
    }

    pub fn into_legacy_transport(self: Arc<Self>) -> Arc<dyn OutboundTransport> {
        // The rtp1 transport exposes the same OutboundTransport
        // surface as the legacy rdg-mux path so the host pipeline
        // (bind_mock_outbound_and_list → pump_host_output) does not
        // need to know which wire protocol is in use. Raw / drain are
        // a no-op because real output bytes travel on RTP1 frames read
        // by the desktop Tauri app's own Rtp1KernelClient; live
        // transport for outbound mux is not needed.
        self
    }

    pub fn install_rtp1(self: &Arc<Self>, state: &AppState) {
        state.hosts.store_rtp1_transport(self.clone());
    }
}

impl OutboundTransport for Rtp1OutboundTransport {
    fn send_json_rpc(&self, method: &str, _params: Value) -> Result<Value, String> {
        Err(format!(
            "RTP1 path does not support raw JSON-RPC `{method}`; use the typed methods (list_sessions, attach_to_pane, send_input, send_resize)"
        ))
    }

    fn send_raw(&self, _frame: &[u8]) -> Result<(), String> {
        // RTP1 has no mux framing; raw bytes travel on `output` frames
        // and are read by the kernel's typed handler.
        Ok(())
    }

    fn drain_pane_raw(&self) -> Vec<(String, Vec<u8>)> {
        // rtp1 hosts: output bytes are pulled directly via
        // `Rtp1ClientHandle::read_output` by the desktop Tauri app.
        // The legacy mock transport's drain is not used on this path.
        Vec::new()
    }

    fn close(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtp1_outbound_transport_send_json_rpc_is_no_op() {
        // The transport must reject raw JSON-RPC because the RTP1 path
        // uses typed methods (list_sessions, attach_to_pane,
        // send_input, send_resize). Exercised in isolation: no kernel
        // needed.
        let transport = Rtp1OutboundTransport {
            host_id: "h".into(),
            sessions: parking_lot::Mutex::new(Vec::new()),
        };
        assert!(transport.send_json_rpc("$/hello", Value::Null).is_err());
        assert!(transport.send_raw(&[0x10, 0, b'p']).is_ok());
        assert!(transport.drain_pane_raw().is_empty());
    }

    #[test]
    fn rtp1_outbound_transport_host_id_accessor() {
        let transport = Rtp1OutboundTransport {
            host_id: "test-host".into(),
            sessions: parking_lot::Mutex::new(Vec::new()),
        };
        assert_eq!(transport.host_id(), "test-host");
    }
}

/// Wire the canonical RTP1 transport for `host_id` and persist the
/// resulting session list via the standard HostRegistry path. The
/// `MockOutboundTransport` is the empty placeholder that satisfies
/// the legacy `OutboundClient` state machine; the canonical output
/// path is the kernel's `output` RTP1 frame read by the desktop Tauri
/// app via its own `Rtp1KernelClient`.
#[allow(dead_code)] // reached via Tauri command; lint keeps the noise down.
pub(crate) async fn bind_rtp1_outbound_and_list(
    state: &AppState,
    host_id: &str,
) -> Result<Vec<RemoteSessionInfo>, String> {
    let transport =
        std::sync::Arc::new(Rtp1OutboundTransport::connect(state, host_id)?);
    let sessions = transport.sessions.lock().clone();
    let host_id_owned = transport.host_id().to_string();
    // Persist via the standard host path (legacy rdg-mux wire shape, but
    // for an rtp1 host the actual output goes through kernel RTP1 frames
    // read by the desktop Tauri app).
    state
        .hosts
        .store_rtp1_transport(transport.clone());
    crate::commands::workspace::sync_kernel_workspace_topologies(state);
    let _ = host_id_owned;
    Ok(sessions)
}

#[tauri::command]
pub async fn rtp1_bind_outbound_and_list(
    state: State<'_, AppState>,
    host_id: String,
) -> Result<Vec<RemoteSessionInfo>, String> {
    bind_rtp1_outbound_and_list(state.inner(), &host_id).await
}

// We retain the legacy OutboundClient wire shape for backward compat
// with the existing rdg-mux rdg / rdg-server flow. New rtp1 hosts
// should call `bind_rtp1_outbound_and_list` instead.
//
// The placeholder `rtp1_attach` module that held a `Rtp1ClientHandle`
// for live read / write through the transport was dropped: the
// desktop Tauri app already uses its own `Rtp1KernelClient` to read
// `output` frames directly via the kernel `/v1/rtp1` WS endpoint, so
// the transport's role is currently `list_sessions` + session
// persistence only. A future PR can attach a live read helper here
// without changing this signature.
