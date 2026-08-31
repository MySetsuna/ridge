---
id: L4-OBS-PACKAGES-RIDGE-REMOTE-SRC-MDNS-RS-6f4472a5
level: L4
parent: L3-OBS-PACKAGES-RIDGE-REMOTE-SRC-e755ff2a
title: mdns.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-remote/src/mdns.rs
---

# mdns.rs

During the bounded pairing window, DNS-SD announcements are sent on every discovered LAN IPv4 to the RFC 6762 multicast group. Each announcement carries an SRV target and matching A record for that interface so clients on Ethernet, Wi-Fi, VPN, or overlay interfaces can resolve a reachable endpoint.
