---
id: L4-OBS-PACKAGES-RIDGE-CLI-BUILD-RS-b55609d2
level: L4
parent: L3-OBS-PACKAGES-RIDGE-CLI-b39550a8
title: build.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-cli/build.rs
test_targets:
  - packages/ridge-cli/tests/kernel_lifecycle_e2e.rs
  - scripts/build-ridge-mcp-sidecar.test.mjs
  - scripts/build-ridge.test.mjs
verified_by:
  - TEST-OBS-PACKAGES-RIDGE-CLI-TESTS-KERNEL-LIFECYCLE-E2E-RS-e6b6fd06
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-MCP-SIDECAR-TEST-MJS-a86f29b5
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-TEST-MJS-86e854f4
---

# build.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
