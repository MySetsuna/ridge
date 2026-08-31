---
id: L4-OBS-PACKAGES-RIDGE-REMOTE-BUILD-RS-7f72e4a2
level: L4
parent: L3-OBS-PACKAGES-RIDGE-REMOTE-01dd6917
title: build.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-remote/build.rs
test_targets:
  - packages/ridge-remote/tests/ua_fork_serve.rs
  - scripts/build-ridge-mcp-sidecar.test.mjs
  - scripts/build-ridge.test.mjs
verified_by:
  - TEST-OBS-PACKAGES-RIDGE-REMOTE-TESTS-UA-FORK-SERVE-RS-db3d9335
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-MCP-SIDECAR-TEST-MJS-a86f29b5
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-TEST-MJS-86e854f4
---

# build.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
