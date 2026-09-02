---
id: L4-OBS-PACKAGES-RIDGE-REMOTE-SRC-UA-RS-22c6c8b3
level: L4
parent: L3-OBS-PACKAGES-RIDGE-REMOTE-SRC-e755ff2a
title: ua.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-remote/src/ua.rs
test_targets:
  - packages/ridge-remote/tests/ua_fork_serve.rs
  - scripts/quality-helpers.test.mjs
  - src/lib/stores/gitGuardStats.test.ts
  - src/lib/stores/processGuardPolicy.test.ts
  - src/remote/lib/generationGuard.test.ts
verified_by:
  - TEST-OBS-PACKAGES-RIDGE-REMOTE-TESTS-UA-FORK-SERVE-RS-db3d9335
  - TEST-OBS-SCRIPTS-QUALITY-HELPERS-TEST-MJS-5d2356c4
  - TEST-OBS-SRC-LIB-STORES-GITGUARDSTATS-TEST-TS-680ef77e
  - TEST-OBS-SRC-LIB-STORES-PROCESSGUARDPOLICY-TEST-TS-a94798ee
  - TEST-OBS-SRC-REMOTE-LIB-GENERATIONGUARD-TEST-TS-954c7584
---

# ua.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
