use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// SECURITY (audit H2): how long mDNS advertises the control endpoint after the
/// server starts. The broadcast is a discovery convenience for the *initial*
/// pairing only; advertising the shell/file control endpoint to the whole
/// network segment for the server's entire lifetime hands a hostile LAN the
/// target for free (no scanning needed). We time-box it to a short pairing
/// window — after this the server keeps running and remains reachable by anyone
/// who already knows the address, but stops announcing itself. Override the
/// window length (seconds) via `RIDGE_REMOTE_MDNS_WINDOW_SECS`; `0` disables
/// mDNS entirely.
const DEFAULT_PAIRING_WINDOW: Duration = Duration::from_secs(5 * 60);

/// Resolve the pairing-window duration from the environment, falling back to
/// [`DEFAULT_PAIRING_WINDOW`]. `0` → `None` (mDNS disabled).
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

/// RFC 6762 multicast DNS responder for `_ridge._tcp.local.`
///
/// Broadcasts an mDNS announcement so that LAN clients can discover the Ridge
/// remote-control WebSocket server without manual configuration — but only
/// during a short, time-boxed PAIRING WINDOW after start (audit H2), not for
/// the server's whole lifetime.
///
/// Uses raw UDP multicast on 224.0.0.1:5353 (the mDNS well-known
/// address). No external crate needed.
pub fn spawn_mdns_broadcast(port: u16) -> (thread::JoinHandle<()>, Arc<AtomicBool>) {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let flag = stop_flag.clone();

    let handle = thread::Builder::new()
        .name("ridge-mdns".into())
        .spawn(move || {
            // SECURITY (audit H2): `None` → operator disabled mDNS; never advertise.
            let Some(window) = pairing_window() else {
                tracing::info!(target: "ridge::remote", "mDNS broadcast disabled (RIDGE_REMOTE_MDNS_WINDOW_SECS=0)");
                return;
            };
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(target: "ridge::remote", error = %e, "mDNS socket bind failed");
                    return;
                }
            };
            socket.set_broadcast(true).ok();
            let mdns_addr = "224.0.0.1:5353";
            let packet = build_mdns_packet(port);

            let started = Instant::now();
            tracing::info!(target: "ridge::remote", port, window_secs = window.as_secs(), "mDNS broadcast started (time-boxed pairing window)");

            // Announce immediately, then every 60 seconds (with 1s granularity so
            // the stop signal is respected promptly), until the pairing window
            // elapses (audit H2) or we are told to stop.
            loop {
                if flag.load(Ordering::Relaxed) || started.elapsed() >= window {
                    break;
                }
                let _ = socket.send_to(&packet, mdns_addr);
                for _ in 0..60 {
                    if flag.load(Ordering::Relaxed) || started.elapsed() >= window {
                        tracing::info!(target: "ridge::remote", "mDNS pairing window closed — discovery broadcast stopped");
                        return;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            }
            tracing::info!(target: "ridge::remote", "mDNS pairing window closed — discovery broadcast stopped");
        })
        .expect("ridge-mdns thread spawn");

    (handle, stop_flag)
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
    p.push(0x0C);
    p.push(0x1C); // Compression pointer to name at offset 0x001C
    p.extend(&0x000Cu16.to_be_bytes()); // Type: PTR
    p.extend(&0x8001u16.to_be_bytes()); // Class: IN + cache-flush
    p.extend(&120u32.to_be_bytes()); // TTL: 120 seconds
                                     // PTR target name: Ridge Remote Control._ridge._tcp.local.
    let instance = b"Ridge Remote Control";
    let ptr_data = encode_dns_name_parts(&[instance, b"_ridge", b"_tcp", b"local"]);
    p.extend(&(ptr_data.len() as u16).to_be_bytes());
    p.extend(&ptr_data);

    // ── Authority: NSEC (proves no other services from this host) ──
    p.push(0x0C);
    p.push(0x1C); // Pointer to _ridge._tcp.local.
    p.extend(&0x002Fu16.to_be_bytes()); // Type: NSEC
    p.extend(&0x8001u16.to_be_bytes()); // Class: IN + cache-flush
    p.extend(&120u32.to_be_bytes()); // TTL: 120
    let next_domain = encode_dns_name_parts(&[b"_services", b"_dns-sd", b"_udp", b"local"]);
    let nsec_bitmap = [0u8, 0u8, 0u8, 0u8, 0x10u8, 0u8, 0u8, 0u8]; // Type PTR
    let nsec_data = [&next_domain, &nsec_bitmap[..]].concat();
    p.extend(&(nsec_data.len() as u16).to_be_bytes());
    p.extend(&nsec_data);

    // ── Additional: SRV record ──
    p.push(0x0C);
    p.push(0x1C); // Pointer to _ridge._tcp.local.
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
