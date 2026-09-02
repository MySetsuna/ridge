---
id: L4-OBS-SRC-TAURI-SRC-ENGINE-PTY-RS-104cc347
level: L4
parent: L3-OBS-SRC-TAURI-SRC-ENGINE-7710ea45
title: pty.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/engine/pty.rs
test_targets:
  - packages/remote/src/shared/terminal/ptyBridge.test.ts
  - scripts/cdp-pty-state.test.mjs
  - src/lib/terminal/ptyRuntimeSnapshot.test.ts
  - src/lib/terminal/ptyWriteQueue.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PTYBRIDGE-TEST-TS-3f30eb8e
  - TEST-OBS-SCRIPTS-CDP-PTY-STATE-TEST-MJS-462b9d95
  - TEST-OBS-SRC-LIB-TERMINAL-PTYRUNTIMESNAPSHOT-TEST-TS-a223e946
  - TEST-OBS-SRC-LIB-TERMINAL-PTYWRITEQUEUE-TEST-TS-b49b9e91
---

# pty.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
