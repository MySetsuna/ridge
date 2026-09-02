---
id: L4-OBS-PACKAGES-RIDGE-CORE-SRC-CLIPBOARD-RS-fac377b1
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CORE-SRC-036e205f
title: clipboard.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-core/src/clipboard.rs
test_targets:
  - packages/remote/src/shared/terminal/clipboardImage.test.ts
  - src/lib/stores/clipboardResolve.test.ts
  - src/remote/lib/clipboard.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-CLIPBOARDIMAGE-TEST-TS-53411e31
  - TEST-OBS-SRC-LIB-STORES-CLIPBOARDRESOLVE-TEST-TS-d8cab7c6
  - TEST-OBS-SRC-REMOTE-LIB-CLIPBOARD-TEST-TS-acf18327
---

# clipboard.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
