---
id: L4-OBS-SRC-LIB-TRANSPORT-TAURISHIM-CLIPBOARD-TS-9862ffc8
level: L4
parent: L3-OBS-SRC-LIB-TRANSPORT-TAURISHIM-0c7e68c9
title: clipboard.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/tauriShim/clipboard.ts
test_targets:
  - packages/remote/src/shared/terminal/clipboardImage.test.ts
  - src/lib/stores/clipboardResolve.test.ts
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - src/lib/transport/tauriShim/opener.test.ts
  - src/lib/transport/tauriShim/window.test.ts
  - src/remote/lib/clipboard.test.ts
public_interface:
  - "export async function readText(): Promise<string>"
  - "export async function writeText(text: string): Promise<void>"
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-CLIPBOARDIMAGE-TEST-TS-53411e31
  - TEST-OBS-SRC-LIB-STORES-CLIPBOARDRESOLVE-TEST-TS-d8cab7c6
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TEST-TS-77e048ec
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-COMPATIBILITY-TEST-TS-ca9e9ee4
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TEST-TS-4fc60727
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-WINDOW-TEST-TS-d57bdd41
  - TEST-OBS-SRC-REMOTE-LIB-CLIPBOARD-TEST-TS-acf18327
---

# clipboard.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
