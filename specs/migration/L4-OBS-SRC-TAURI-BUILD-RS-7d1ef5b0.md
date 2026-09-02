---
id: L4-OBS-SRC-TAURI-BUILD-RS-7d1ef5b0
level: L4
parent: L3-OBS-SRC-TAURI-6056d0ef
title: build.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/build.rs
test_targets:
  - scripts/build-ridge-mcp-sidecar.test.mjs
  - scripts/build-ridge.test.mjs
  - src-tauri/tests/win_manifest_boot.rs
verified_by:
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-MCP-SIDECAR-TEST-MJS-a86f29b5
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-TEST-MJS-86e854f4
  - TEST-OBS-SRC-TAURI-TESTS-WIN-MANIFEST-BOOT-RS-681c2eff
---

# build.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
