use crate::net::detect_lan_ips;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_PAIRING_WINDOW: Duration = Duration::from_secs(5 * 60);
const MDNS_ADDR: &str = "224.0.0.251:5353";

fn pairing_window() -> Option<Duration> {
    match std::env::var("RIDGE_REMOTE_MDNS_WINDOW_SECS") {
        Ok(v) => match v.trim().parse::<u64>() {
            Ok(0) => None,
            Ok(secs) => Some(Duration::from_secs(secs)),
            Err(_) => Some(DEFAULT_PAIRING_WINDOW),
        },
        Err(_) => Some(DEFAULT_PAIRING_WINDOW),
    }
}

/// Advertise `_ridge._tcp.local.` on every usable IPv4 interface during the
/// bounded pairing window. Each packet carries that interface's own A record.
pub fn spawn_mdns_broadcast(port: u16) -> (thread::JoinHandle<()>, Arc<AtomicBool>) {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let flag = stop_flag.clone();
    let handle = thread::Builder::new()
        .name("ridge-mdns".into())
        .spawn(move || run_mdns_broadcast(port, flag))
        .expect("ridge-mdns thread spawn");
    (handle, stop_flag)
}

fn run_mdns_broadcast(port: u16, flag: Arc<AtomicBool>) {
    let Some(window) = pairing_window() else {
        tracing::info!(target: "ridge::remote", "mDNS broadcast disabled (RIDGE_REMOTE_MDNS_WINDOW_SECS=0)");
        return;
    };
    let announcements = bind_announcements(port, detect_lan_ips());
    if announcements.is_empty() {
        tracing::warn!(target: "ridge::remote", "mDNS found no usable IPv4 interfaces");
        return;
    }
    let started = Instant::now();
    tracing::info!(target: "ridge::remote", port, interfaces = announcements.len(), window_secs = window.as_secs(), "mDNS broadcast started");
    broadcast_until_window(&announcements, &flag, started, window);
    tracing::info!(target: "ridge::remote", "mDNS pairing window closed");
}

struct BoundAnnouncement {
    socket: UdpSocket,
    packet: Vec<u8>,
}

fn bind_announcements(port: u16, ips: impl IntoIterator<Item = String>) -> Vec<BoundAnnouncement> {
    announcements(port, ips)
        .into_iter()
        .filter_map(|(ip, packet)| match UdpSocket::bind((ip, 0)) {
            Ok(socket) => {
                socket.set_multicast_ttl_v4(255).ok();
                Some(BoundAnnouncement { socket, packet })
            }
            Err(error) => {
                tracing::warn!(target: "ridge::remote", interface = %ip, error = %error, "mDNS interface bind failed");
                None
            }
        })
        .collect()
}

fn announcements(port: u16, ips: impl IntoIterator<Item = String>) -> Vec<(Ipv4Addr, Vec<u8>)> {
    ips.into_iter()
        .filter_map(|value| value.parse::<Ipv4Addr>().ok())
        .map(|ip| (ip, build_mdns_packet(port, ip)))
        .collect()
}

fn broadcast_until_window(
    announcements: &[BoundAnnouncement],
    flag: &AtomicBool,
    started: Instant,
    window: Duration,
) {
    while !flag.load(Ordering::Relaxed) && started.elapsed() < window {
        for announcement in announcements {
            let _ = announcement.socket.send_to(&announcement.packet, MDNS_ADDR);
        }
        for _ in 0..60 {
            if flag.load(Ordering::Relaxed) || started.elapsed() >= window {
                return;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

/// DNS-SD response: PTR service answer plus SRV and A additional records.
fn build_mdns_packet(port: u16, ip: Ipv4Addr) -> Vec<u8> {
    let service = encode_dns_name_parts(&[b"_ridge", b"_tcp", b"local"]);
    let instance = encode_dns_name_parts(&[b"Ridge Remote Control", b"_ridge", b"_tcp", b"local"]);
    let host = encode_dns_name_parts(&[b"ridge-local", b"local"]);
    let mut packet = Vec::new();
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&0x8400u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&1u16.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&2u16.to_be_bytes());
    push_record(&mut packet, &service, 12, 1, 120, &instance);
    let mut srv = Vec::with_capacity(6 + host.len());
    srv.extend_from_slice(&0u16.to_be_bytes());
    srv.extend_from_slice(&0u16.to_be_bytes());
    srv.extend_from_slice(&port.to_be_bytes());
    srv.extend_from_slice(&host);
    push_record(&mut packet, &instance, 33, 0x8001, 120, &srv);
    push_record(&mut packet, &host, 1, 0x8001, 120, &ip.octets());
    packet
}

fn push_record(packet: &mut Vec<u8>, name: &[u8], kind: u16, class: u16, ttl: u32, data: &[u8]) {
    packet.extend_from_slice(name);
    packet.extend_from_slice(&kind.to_be_bytes());
    packet.extend_from_slice(&class.to_be_bytes());
    packet.extend_from_slice(&ttl.to_be_bytes());
    packet.extend_from_slice(&(data.len() as u16).to_be_bytes());
    packet.extend_from_slice(data);
}

fn encode_dns_name_parts(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.push(part.len() as u8);
        out.extend_from_slice(part);
    }
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::{announcements, bind_announcements, build_mdns_packet, MDNS_ADDR};
    use std::net::Ipv4Addr;

    #[test]
    fn announcement_uses_standard_multicast_and_contains_srv_and_a() {
        let ip = Ipv4Addr::new(192, 168, 1, 11);
        let packet = build_mdns_packet(9527, ip);
        assert_eq!(MDNS_ADDR, "224.0.0.251:5353");
        assert_eq!(u16::from_be_bytes([packet[6], packet[7]]), 1);
        assert_eq!(u16::from_be_bytes([packet[10], packet[11]]), 2);
        assert!(packet
            .windows(2)
            .any(|bytes| bytes == 9527u16.to_be_bytes()));
        assert!(packet.windows(4).any(|bytes| bytes == ip.octets()));
    }

    #[test]
    fn builds_one_interface_specific_packet_per_ip() {
        let packets = announcements(9527, ["192.168.1.11".into(), "100.108.76.113".into()]);
        assert_eq!(packets.len(), 2);
        assert_ne!(packets[0].1, packets[1].1);
    }

    #[test]
    fn binds_every_detected_local_interface() {
        let ips = crate::net::detect_lan_ips();
        let expected = ips
            .iter()
            .filter(|ip| ip.parse::<Ipv4Addr>().is_ok())
            .count();
        assert_eq!(bind_announcements(9527, ips).len(), expected);
    }
}
