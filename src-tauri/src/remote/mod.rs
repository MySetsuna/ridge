pub mod auth;
pub mod mdns;
mod server;
mod tls;

use std::net::ToSocketAddrs;

pub use server::spawn_remote_server;

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

    let compname = std::env::var("COMPUTERNAME")
        .unwrap_or_else(|_| "localhost".to_string());
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
