pub mod auth;
pub mod core_bridge;
pub mod mdns;
mod server;
// TLS cert generation: moved to shared crate ridge-remote.
// Re-exported so super::tls::resolve_config etc. keep working in server.rs.
pub use ridge_remote::tls;

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

/// Enumerate ALL usable LAN IPv4 addresses so the remote panel can list every
/// reachable entry — a phone may sit on a different interface than the primary
/// route (e.g. Wi-Fi 192.168.x vs Tailscale 100.x), and `detect_lan_ip` only
/// returns the route-to-internet one. The primary address is placed FIRST; the
/// rest come from resolving the local hostname (Windows returns every configured
/// IPv4). Loopback / link-local (169.254) / unspecified are excluded. Dedup,
/// primary-first order.
pub fn detect_lan_ips() -> Vec<String> {
    use std::net::{IpAddr, UdpSocket};

    fn push_ip(out: &mut Vec<String>, ip: IpAddr) {
        if let IpAddr::V4(v4) = ip {
            if v4.is_loopback() || v4.is_link_local() || v4.is_unspecified() {
                return;
            }
            let s = v4.to_string();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }

    let mut out: Vec<String> = Vec::new();

    // 1) Primary outgoing-route address first (the UDP-connect trick).
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("1.1.1.1:53").is_ok() {
            if let Ok(local) = socket.local_addr() {
                push_ip(&mut out, local.ip());
            }
        }
    }

    // 2) Everything the local hostname resolves to (all configured IPv4s).
    let compname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string());
    if let Ok(addrs) = (compname.as_str(), 0u16).to_socket_addrs() {
        for addr in addrs {
            push_ip(&mut out, addr.ip());
        }
    }

    // Fall back to the single best-guess address so the panel is never empty.
    if out.is_empty() {
        out.push(detect_lan_ip());
    }
    out
}
