---
id: L4-OBS-PACKAGES-RIDGE-REMOTE-SRC-NET-RS-6412510e
level: L4
parent: L3-OBS-PACKAGES-RIDGE-REMOTE-SRC-e755ff2a
title: net.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-remote/src/net.rs
test_targets:
  - packages/remote/src/shared/cloud/weakNetLab.test.ts
  - scripts/lib/weakNetMetrics.test.mjs
  - src/lib/stores/paneTree.coverage.test.ts
  - src/lib/stores/paneTree.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-WEAKNETLAB-TEST-TS-e7298069
  - TEST-OBS-SCRIPTS-LIB-WEAKNETMETRICS-TEST-MJS-6b840696
  - TEST-OBS-SRC-LIB-STORES-PANETREE-COVERAGE-TEST-TS-747ca055
  - TEST-OBS-SRC-LIB-STORES-PANETREE-TEST-TS-8fb828ce
---

# net.rs

LAN address discovery returns every usable local IPv4 in deterministic reachability priority, preferring conventional physical-LAN ranges over overlay and virtual-network ranges while retaining all candidates for explicit selection and TLS SAN generation.
