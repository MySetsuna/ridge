---
id: L4-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TS-c94729d7
level: L4
parent: L3-OBS-SRC-LIB-TRANSPORT-TAURISHIM-0c7e68c9
title: opener.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/tauriShim/opener.ts
test_targets:
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - src/lib/transport/tauriShim/opener.test.ts
  - src/lib/transport/tauriShim/window.test.ts
public_interface:
  - "export async function openPath(path: string, _openWith?: string):
    Promise<void>"
  - "export async function openUrl(url: string | URL, _openWith?: string):
    Promise<void>"
  - "export async function revealItemInDir(path: string | string[]):
    Promise<void>"
verified_by:
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TEST-TS-77e048ec
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-COMPATIBILITY-TEST-TS-ca9e9ee4
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TEST-TS-4fc60727
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-WINDOW-TEST-TS-d57bdd41
---

# opener.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
