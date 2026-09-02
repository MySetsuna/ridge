---
id: L4-OBS-PACKAGES-RIDGE-TERM-SRC-INPUT-RS-9700e2de
level: L4
parent: L3-OBS-PACKAGES-RIDGE-TERM-SRC-14b2a6fb
title: input.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/ridge-term/src/input.rs
test_targets:
  - packages/remote/src/shared/terminal/paneInputGate.test.ts
  - packages/remote/src/shared/terminal/shellInputSnapshot.test.ts
  - src/lib/components/inputBufferTracker.test.ts
  - tests/e2e-shell/input-echo.spec.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PANEINPUTGATE-TEST-TS-06a1530b
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-SHELLINPUTSNAPSHOT-TEST-TS-9e6558fc
  - TEST-OBS-SRC-LIB-COMPONENTS-INPUTBUFFERTRACKER-TEST-TS-a37220a2
  - TEST-OBS-TESTS-E2E-SHELL-INPUT-ECHO-SPEC-TS-566210a7
---

# input.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
