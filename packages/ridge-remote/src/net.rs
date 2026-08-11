//! LAN IPv4 address detection for QR-code / link generation.
//!
//! Runtime-agnostic (std-only, zero Tauri). Migrated verbatim from the desktop
//! `src-tauri/src/remote/mod.rs` so the desktop LAN host, the `rdg` CLI, and the
//! cloud relay all share ONE implementation instead of drifting copies.

use std::net::{IpAddr, ToSocketAddrs, UdpSocket};

fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".to_string())
}

fn outgoing_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:53").ok()?;
    usable_ipv4(socket.local_addr().ok()?.ip())
}

fn usable_ipv4(ip: IpAddr) -> Option<String> {
    let IpAddr::V4(ip) = ip else { return None };
    if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
        return None;
    }
    Some(ip.to_string())
}

fn hostname_ipv4() -> Option<String> {
    (computer_name().as_str(), 0u16)
        .to_socket_addrs()
        .ok()?
        .find_map(|addr| usable_ipv4(addr.ip()))
}

fn push_unique_ipv4(out: &mut Vec<String>, ip: IpAddr) {
    let Some(ip) = usable_ipv4(ip) else { return };
    if !out.contains(&ip) {
        out.push(ip);
    }
}

/// Detect the LAN IPv4 address for QR-code link generation.
/// Uses a UDP socket trick to find the primary outgoing interface address.
pub fn detect_lan_ip() -> String {
    outgoing_ipv4()
        .or_else(hostname_ipv4)
        .unwrap_or_else(|| "localhost".to_string())
}

/// Enumerate ALL usable LAN IPv4 addresses so the remote panel can list every
/// reachable entry — a phone may sit on a different interface than the primary
/// route (e.g. Wi-Fi 192.168.x vs Tailscale 100.x), and `detect_lan_ip` only
/// returns the route-to-internet one. The primary address is placed FIRST; the
/// rest come from resolving the local hostname (Windows returns every configured
/// IPv4). Loopback / link-local (169.254) / unspecified are excluded. Dedup,
/// primary-first order.
pub fn detect_lan_ips() -> Vec<String> {
    let mut out = Vec::new();

    // 1) Primary outgoing-route address first (the UDP-connect trick).
    if let Some(ip) = outgoing_ipv4() {
        out.push(ip);
    }

    // 2) Everything the local hostname resolves to (all configured IPv4s).
    if let Ok(addrs) = (computer_name().as_str(), 0u16).to_socket_addrs() {
        for addr in addrs {
            push_unique_ipv4(&mut out, addr.ip());
        }
    }

    // Fall back to the single best-guess address so the panel is never empty.
    if out.is_empty() {
        out.push(detect_lan_ip());
    }
    out
}
