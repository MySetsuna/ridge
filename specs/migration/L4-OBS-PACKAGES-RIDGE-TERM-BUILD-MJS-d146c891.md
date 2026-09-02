---
id: L4-OBS-PACKAGES-RIDGE-TERM-BUILD-MJS-d146c891
level: L4
parent: L3-OBS-PACKAGES-RIDGE-TERM-866f3176
title: build.mjs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-term/build.mjs
test_targets:
  - packages/ridge-term/tests/common/mod.rs
  - packages/ridge-term/tests/protocol_smoke.rs
  - scripts/build-ridge-mcp-sidecar.test.mjs
  - scripts/build-ridge.test.mjs
verified_by:
  - TEST-OBS-PACKAGES-RIDGE-TERM-TESTS-COMMON-MOD-RS-23bc0a12
  - TEST-OBS-PACKAGES-RIDGE-TERM-TESTS-PROTOCOL-SMOKE-RS-62f9359a
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-MCP-SIDECAR-TEST-MJS-a86f29b5
  - TEST-OBS-SCRIPTS-BUILD-RIDGE-TEST-MJS-86e854f4
---

# build.mjs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
