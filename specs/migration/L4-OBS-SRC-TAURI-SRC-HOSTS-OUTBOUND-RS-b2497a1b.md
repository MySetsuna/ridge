---
id: L4-OBS-SRC-TAURI-SRC-HOSTS-OUTBOUND-RS-b2497a1b
level: L4
parent: L3-OBS-SRC-TAURI-SRC-HOSTS-728bec8b
title: outbound.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/hosts/outbound.rs
test_targets:
  - packages/remote/src/shared/hosts/outboundLifecycle.test.ts
  - packages/remote/src/shared/hosts/outboundReconnect.test.ts
  - src/lib/stores/hostsOutbound.test.ts
  - src/lib/stores/hostsOutboundProduct.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDLIFECYCLE-TEST-TS-9e8e8887
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDRECONNECT-TEST-TS-1dd36e13
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUND-TEST-TS-afc6c610
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUNDPRODUCT-TEST-TS-c9cf71df
---

# outbound.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
