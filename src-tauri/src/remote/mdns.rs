use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static MDNS_STARTED: AtomicBool = AtomicBool::new(false);

/// RFC 6762 multicast DNS responder for `_ridge._tcp.local.`
///
/// Periodically broadcasts a mDNS announcement so that LAN clients can
/// discover the Ridge remote-control WebSocket server without manual
/// configuration.
///
/// Uses raw UDP multicast on 224.0.0.1:5353 (the mDNS well-known
/// address). No external crate needed.
pub fn spawn_mdns_broadcast(port: u16) {
    if MDNS_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    thread::Builder::new()
        .name("ridge-mdns".into())
        .spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(target: "ridge::remote", error = %e, "mDNS socket bind failed");
                    return;
                }
            };
            socket.set_broadcast(true).ok();
            // mDNS multicast address + port
            let mdns_addr = "224.0.0.1:5353";

            // Build the DNS-SD advertisement packet once, reuse it.
            let packet = build_mdns_packet(port);

            tracing::info!(target: "ridge::remote", port, "mDNS broadcast started");

            // Announce immediately, then every 60 seconds.
            loop {
                if socket.send_to(&packet, mdns_addr).is_err() {
                    // Network may be unavailable — keep trying.
                }
                thread::sleep(Duration::from_secs(60));
            }
        })
        .expect("ridge-mdns thread spawn");
}

/// Build a DNS-SD announcement packet for `_ridge._tcp.local.`
///
/// Format: standard DNS response (RFC 1035) with:
/// - Question section: PTR query for `_ridge._tcp.local.`
/// - Answer section: PTR record → `Ridge Remote Control._ridge._tcp.local.`
/// - Additional section: SRV + TXT records with port and metadata
fn build_mdns_packet(port: u16) -> Vec<u8> {
    let mut p = Vec::new();

    // Header
    p.extend(&0u16.to_be_bytes()); // Transaction ID
    p.extend(&0x8400u16.to_be_bytes()); // Flags: response + authoritative
    p.extend(&0u16.to_be_bytes()); // Questions: 0 (unsolicited announcement)
    p.extend(&1u16.to_be_bytes()); // Answers: 1
    p.extend(&1u16.to_be_bytes()); // Authority: 1 (NSEC)
    p.extend(&1u16.to_be_bytes()); // Additional: 1 (SRV)

    // ── Answer: PTR record ──
    // Name: _ridge._tcp.local. (compressed)
    p.push(0x0C); p.push(0x1C); // Compression pointer to name at offset 0x001C
    p.extend(&0x000Cu16.to_be_bytes()); // Type: PTR
    p.extend(&0x8001u16.to_be_bytes()); // Class: IN + cache-flush
    p.extend(&120u32.to_be_bytes()); // TTL: 120 seconds
    // PTR target name: Ridge Remote Control._ridge._tcp.local.
    let instance = b"Ridge Remote Control";
    let ptr_data = encode_dns_name_parts(&[
        instance,
        b"_ridge",
        b"_tcp",
        b"local",
    ]);
    p.extend(&(ptr_data.len() as u16).to_be_bytes());
    p.extend(&ptr_data);

    // ── Authority: NSEC (proves no other services from this host) ──
    p.push(0x0C); p.push(0x1C); // Pointer to _ridge._tcp.local.
    p.extend(&0x002Fu16.to_be_bytes()); // Type: NSEC
    p.extend(&0x8001u16.to_be_bytes()); // Class: IN + cache-flush
    p.extend(&120u32.to_be_bytes()); // TTL: 120
    let next_domain = encode_dns_name_parts(&[b"_services", b"_dns-sd", b"_udp", b"local"]);
    let nsec_bitmap = [0u8, 0u8, 0u8, 0u8, 0x10u8, 0u8, 0u8, 0u8]; // Type PTR
    let nsec_data = [&next_domain, &nsec_bitmap[..]].concat();
    p.extend(&(nsec_data.len() as u16).to_be_bytes());
    p.extend(&nsec_data);

    // ── Additional: SRV record ──
    p.push(0x0C); p.push(0x1C); // Pointer to _ridge._tcp.local.
    p.extend(&0x0021u16.to_be_bytes()); // Type: SRV
    p.extend(&0x8001u16.to_be_bytes()); // Class: IN + cache-flush
    p.extend(&120u32.to_be_bytes()); // TTL: 120
    let target = encode_dns_name_parts(&[b"ridge-local", b"local"]);
    let srv_payload: Vec<u8> = [
        &0u16.to_be_bytes()[..], // priority
        &0u16.to_be_bytes()[..], // weight
        &port.to_be_bytes()[..], // port
        &target[..],             // target hostname
    ]
    .concat();
    p.extend(&(srv_payload.len() as u16).to_be_bytes());
    p.extend(&srv_payload);

    p
}

/// Encode a domain name as a sequence of length-prefixed labels.
fn encode_dns_name_parts(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        out.push(part.len() as u8);
        out.extend_from_slice(part);
    }
    out.push(0); // Root label
    out
}
