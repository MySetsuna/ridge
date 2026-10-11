//! Authenticated local presentation sessions over the shared Host executor.

use crate::{host::KernelHost, registry::KernelEndpoint, server::AppState};
use axum::{
    extract::{ws::WebSocketUpgrade, ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use ridge_remote::{
    auth::{RemoteAuth, SessionStore, VerifyThrottle, SESSION_TTL},
    host::WsConn,
    serve::UaServeConfig,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientRequest {
    pub device_id: String,
    pub origin: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientGrant {
    pub protocol_version: u64,
    pub host_id: String,
    pub runtime_epoch: String,
    pub device_id: String,
    pub origin: String,
    pub ws_url: String,
    pub token: String,
    pub expires_at_unix_seconds: u64,
    pub capabilities: Vec<String>,
}

fn trusted_origin(origin: &str) -> bool {
    if origin == "tauri://localhost" {
        return true;
    }
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    if authority.contains(['/', '?', '#', '@']) {
        return false;
    }
    if authority == "tauri.localhost" {
        return true;
    }
    if matches!(authority, "127.0.0.1" | "localhost" | "[::1]") {
        return true;
    }
    let (name, port) = authority
        .rsplit_once(':')
        .map_or((authority, None), |(name, port)| (name, Some(port)));
    matches!(name, "127.0.0.1" | "localhost" | "[::1]")
        && port.is_none_or(|p| p.parse::<u16>().is_ok_and(|p| p > 0))
}

fn host(state: &AppState) -> Arc<KernelHost> {
    state
        .local_host
        .get_or_init(|| {
            Arc::new(KernelHost {
                endpoint: KernelEndpoint {
                    pid: state.pid,
                    port: state.port,
                    token: state.token.clone(),
                    started_at_unix: state.started_at_unix,
                },
                totp: Arc::new(RemoteAuth::new()),
                totp_throttle: Arc::new(VerifyThrottle::new()),
                sessions: SessionStore::new(),
                port: state.port,
                lan_ip: "127.0.0.1".into(),
                machine_name: state.host_id.clone(),
                serve_cfg: UaServeConfig::resolve_ui_dirs(),
                tls_enabled: false,
                remote_enabled: Arc::new(AtomicBool::new(true)),
            })
        })
        .clone()
}

pub async fn issue(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<ClientRequest>,
) -> Result<Json<ClientGrant>, StatusCode> {
    if !peer.ip().is_loopback()
        || headers
            .get("x-ridge-kernel-token")
            .and_then(|v| v.to_str().ok())
            != Some(state.token.as_str())
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !trusted_origin(&request.origin)
        || request.device_id.is_empty()
        || request.device_id.len() > 128
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let token = host(&state)
        .sessions
        .create_origin_bound_session(&request.device_id, &peer.ip().to_string(), &request.origin)
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    Ok(Json(ClientGrant {
        protocol_version: crate::client::KERNEL_PROTOCOL_VERSION,
        host_id: state.host_id,
        runtime_epoch: state.runtime_epoch,
        device_id: request.device_id,
        origin: request.origin,
        ws_url: format!("ws://127.0.0.1:{}/v1/local/ws", state.port),
        token,
        expires_at_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + SESSION_TTL.as_secs(),
        capabilities: crate::host::implemented_capabilities()
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientQuery {
    device_id: String,
}

pub async fn websocket(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(query): Query<ClientQuery>,
) -> Result<Response, StatusCode> {
    if !peer.ip().is_loopback() {
        return Err(StatusCode::FORBIDDEN);
    }
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !trusted_origin(origin) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let protocols = headers
        .get("sec-websocket-protocol")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let entries: Vec<_> = protocols.split(',').map(str::trim).collect();
    if entries.len() != 2 || !entries.contains(&"ridge-client") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let token = entries
        .iter()
        .find_map(|p| p.strip_prefix("auth-"))
        .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let host = host(&state);
    if !host.sessions.validate_origin_bound_token(
        token,
        &query.device_id,
        &peer.ip().to_string(),
        origin,
    ) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let connection = WsConn {
        remote_addr: peer.ip().to_string(),
        device_id: query.device_id,
        token: Some(token.into()),
    };
    let origin = origin.to_owned();
    Ok(ws
        .protocols(["ridge-client"])
        .on_upgrade(move |socket| host.serve_local_websocket(socket, connection, origin)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn origins_are_exact_local_applications() {
        for origin in [
            "tauri://localhost",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "http://127.0.0.1:5173",
            "http://localhost:123",
            "http://[::1]:456",
        ] {
            assert!(trusted_origin(origin), "{origin}");
        }
        for origin in [
            "null",
            "file://localhost",
            "https://example.test",
            "http://localhost.evil",
            "http://localhost:0",
            "http://localhost:bad",
            "http://127.0.0.1/",
            "http://user@localhost",
            "http://localhost#x",
        ] {
            assert!(!trusted_origin(origin), "{origin}");
        }
    }
}
