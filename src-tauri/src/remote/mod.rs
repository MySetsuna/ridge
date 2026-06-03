pub mod auth;
pub mod mdns;
mod server;
mod tls;

use std::net::ToSocketAddrs;

pub use server::spawn_remote_server;

/// Forward a Tauri event to all connected desktop-browser remote clients (the
/// "desktop UI in a browser" mode), so the browser's `listen()` shim dispatches
/// it exactly like a native event. Add a call next to any `app.emit(...)` whose
/// event the desktop UI subscribes to. No-op when AppState isn't managed.
pub fn forward_event<S: serde::Serialize>(app: &tauri::AppHandle, name: &str, payload: S) {
    use tauri::Manager;
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        return;
    };
    let value = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    let _ = state.remote_ui_event_tx.send(crate::types::RemoteUiEvent {
        name: name.to_string(),
        payload: value,
    });
}

/// Detect the LAN IPv4 address for QR-code link generation.
/// Uses a UDP socket trick to find the primary outgoing interface address.
pub fn detect_lan_ip() -> String {
    use std::net::UdpSocket;

    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("1.1.1.1:53").is_ok() {
            if let Ok(local) = socket.local_addr() {
                let ip = local.ip();
                if ip.is_ipv4() && !ip.is_loopback() {
                    return ip.to_string();
                }
            }
        }
    }

    let compname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string());
    if let Ok(addrs) = (compname.as_str(), 0u16).to_socket_addrs() {
        for addr in addrs {
            let ip = addr.ip();
            if ip.is_ipv4() && !ip.is_loopback() {
                return ip.to_string();
            }
        }
    }

    "localhost".to_string()
}
