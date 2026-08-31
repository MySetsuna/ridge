---
id: L4-OBS-SRC-TAURI-SRC-STATE-RS-1194bc9c
level: L4
parent: L3-OBS-SRC-TAURI-SRC-bcb33161
title: state.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/state.rs
test_targets:
  - scripts/cdp-pty-state.test.mjs
  - src/lib/stores/searchState.test.ts
  - src/remote/lib/treeState.test.ts
verified_by:
  - TEST-OBS-SCRIPTS-CDP-PTY-STATE-TEST-MJS-462b9d95
  - TEST-OBS-SRC-LIB-STORES-SEARCHSTATE-TEST-TS-7c629e13
  - TEST-OBS-SRC-REMOTE-LIB-TREESTATE-TEST-TS-bcbf35f8
---

# state.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
