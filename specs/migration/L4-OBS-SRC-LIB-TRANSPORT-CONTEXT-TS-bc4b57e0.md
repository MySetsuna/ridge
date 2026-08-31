---
id: L4-OBS-SRC-LIB-TRANSPORT-CONTEXT-TS-bc4b57e0
level: L4
parent: L3-OBS-SRC-LIB-TRANSPORT-0b23356e
title: context.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/context.ts
test_targets:
  - src/lib/stores/contextMenu.test.ts
  - src/lib/transport/tauri.test.ts
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - src/lib/transport/tauriShim/opener.test.ts
  - src/lib/transport/tauriShim/window.test.ts
  - src/lib/transport/ws.test.ts
public_interface:
  - "export function getTransport(): DataProvider"
  - "export function hasTransport(): boolean"
  - "export function setTransport(provider: DataProvider): void"
verified_by:
  - TEST-OBS-SRC-LIB-STORES-CONTEXTMENU-TEST-TS-c5d624d2
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURI-TEST-TS-c25dafa7
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TEST-TS-77e048ec
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-COMPATIBILITY-TEST-TS-ca9e9ee4
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TEST-TS-4fc60727
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-WINDOW-TEST-TS-d57bdd41
  - TEST-OBS-SRC-LIB-TRANSPORT-WS-TEST-TS-fa7eff79
---

# context.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
