---
id: L4-OBS-SRC-TAURI-SRC-COMMANDS-TERMINAL-RS-12565300
level: L4
parent: L3-OBS-SRC-TAURI-SRC-COMMANDS-7ed73efa
title: terminal.rs
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src-tauri/src/commands/terminal.rs
test_targets:
  - packages/remote/src/shared/terminal/terminalFeedPolicy.test.ts
  - packages/remote/src/shared/terminal/terminalFocus.test.ts
  - packages/remote/src/shared/terminal/terminalMemoryPolicy.test.ts
  - src/lib/stores/terminalHistory.test.ts
  - src/remote/lib/TerminalCanvas.test.ts
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TERMINALFEEDPOLICY-TEST-TS-a929e750
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TERMINALFOCUS-TEST-TS-ff98409c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-TERMINALMEMORYPOLICY-TEST-TS-19d7b4ce
  - TEST-OBS-SRC-LIB-STORES-TERMINALHISTORY-TEST-TS-49edc8b5
  - TEST-OBS-SRC-REMOTE-LIB-TERMINALCANVAS-TEST-TS-e83a9e51
---

# terminal.rs

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
