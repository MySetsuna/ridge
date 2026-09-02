---
id: L4-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TS-98949507
level: L4
parent: L3-OBS-SRC-LIB-TRANSPORT-TAURISHIM-0c7e68c9
title: bridge.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/tauriShim/bridge.ts
test_targets:
  - packages/remote/src/shared/cloud/cloudHostBridge.test.ts
  - packages/remote/src/shared/terminal/ptyBridge.test.ts
  - packages/remote/src/shared/terminal/themeBridge.test.ts
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - src/lib/transport/tauriShim/opener.test.ts
  - src/lib/transport/tauriShim/window.test.ts
public_interface:
  - export class TauriBridge
  - export interface TauriEvent
  - export type EventCallback
  - export type UnlistenFn
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-CLOUDHOSTBRIDGE-TEST-TS-731be498
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-PTYBRIDGE-TEST-TS-3f30eb8e
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-THEMEBRIDGE-TEST-TS-4fb837c0
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TEST-TS-77e048ec
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-COMPATIBILITY-TEST-TS-ca9e9ee4
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TEST-TS-4fc60727
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-WINDOW-TEST-TS-d57bdd41
---

# bridge.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
