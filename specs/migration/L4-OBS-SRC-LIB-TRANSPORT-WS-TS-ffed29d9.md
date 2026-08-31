---
id: L4-OBS-SRC-LIB-TRANSPORT-WS-TS-ffed29d9
level: L4
parent: L3-OBS-SRC-LIB-TRANSPORT-0b23356e
title: ws.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/ws.ts
test_targets:
  - packages/remote/src/shared/transport/lanWsAdapter.test.ts
  - packages/remote/src/shared/transport/wsRemote.behavior.test.ts
  - packages/remote/src/shared/transport/wsRemotePending.test.ts
  - packages/remote/src/shared/transport/wsRemoteRpcScheduler.test.ts
  - packages/remote/src/shared/transport/wsRemoteUrl.test.ts
  - scripts/remote-createws-test.mjs
  - src/lib/transport/tauri.test.ts
  - src/lib/transport/tauriShim/bridge.test.ts
  - src/lib/transport/tauriShim/compatibility.test.ts
  - src/lib/transport/tauriShim/opener.test.ts
  - src/lib/transport/tauriShim/window.test.ts
  - src/lib/transport/ws.test.ts
public_interface:
  - export class WsDataProvider
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-LANWSADAPTER-TEST-TS-a2b9af4c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTE-BEHAVIOR-TEST-TS-8b7a590c
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTEPENDING-TEST-TS-ee9c7d29
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTERPCSCHEDULER-TEST-TS-04be1fed
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TRANSPORT-WSREMOTEURL-TEST-TS-a219c8e6
  - TEST-OBS-SCRIPTS-REMOTE-CREATEWS-TEST-MJS-e9b94290
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURI-TEST-TS-c25dafa7
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-BRIDGE-TEST-TS-77e048ec
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-COMPATIBILITY-TEST-TS-ca9e9ee4
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-OPENER-TEST-TS-4fc60727
  - TEST-OBS-SRC-LIB-TRANSPORT-TAURISHIM-WINDOW-TEST-TS-d57bdd41
  - TEST-OBS-SRC-LIB-TRANSPORT-WS-TEST-TS-fa7eff79
---

# ws.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
