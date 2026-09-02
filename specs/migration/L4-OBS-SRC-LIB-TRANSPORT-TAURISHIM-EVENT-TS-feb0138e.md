---
id: L4-OBS-SRC-LIB-TRANSPORT-TAURISHIM-EVENT-TS-feb0138e
level: L4
parent: L3-OBS-SRC-LIB-TRANSPORT-TAURISHIM-0c7e68c9
title: event.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/tauriShim/event.ts
test_targets:
  - src/lib/stores/fsEvents.test.ts
  - src/lib/teammate/layoutEvent.test.ts
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - src/lib/transport/tauriShim/opener.test.ts
  - src/lib/transport/tauriShim/window.test.ts
public_interface:
  - "export async function emit(_event: string, _payload?: unknown):
    Promise<void>"
  - "export async function emitTo( _target: string, _event: string, _payload?:
    unknown, ): Promise<void>"
  - export type Event
verified_by:
  - TEST-OBS-SRC-LIB-STORES-FSEVENTS-TEST-TS-f628ded6
  - TEST-OBS-SRC-LIB-TEAMMATE-LAYOUTEVENT-TEST-TS-b130fa00
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TEST-TS-77e048ec
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-COMPATIBILITY-TEST-TS-ca9e9ee4
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TEST-TS-4fc60727
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-WINDOW-TEST-TS-d57bdd41
---

# event.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
