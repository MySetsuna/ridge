pub mod auth;
mod mdns;
mod server;

use crate::state::AppState;
use tauri::AppHandle;

/// Start the remote control subsystems: mDNS broadcast + WebSocket server.
///
/// Returns the LAN port the WebSocket server is listening on, or `None`
/// if the server failed to start (non-fatal — the app continues without
/// remote-control capability).
pub fn spawn_remote_control(handle: AppHandle, state: AppState) -> Option<u16> {
    let auth = state.remote_auth.clone();
    let port = server::spawn_remote_server(handle.clone(), state, auth)?;
    mdns::spawn_mdns_broadcast(port);
    Some(port)
}
