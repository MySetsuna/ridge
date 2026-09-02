---
id: L4-OBS-SRC-LIB-ACTIONS-HOSTSESSIONDRAG-TS-9e75b800
level: L4
parent: L3-OBS-SRC-LIB-ACTIONS-531bf1fb
title: hostSessionDrag.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/actions/hostSessionDrag.ts
test_targets:
  - src/lib/actions/hostSessionDrag.test.ts
  - src/lib/actions/overlayScroll.test.ts
  - src/lib/actions/paneDockDrag.test.ts
public_interface:
  - "export function hostAttachRequestAt( params: HostSessionDragParams,
    targetPaneId: string, region: AttachRegion, ): HostAttachRequest"
  - "export function hostSessionDrag(node: HTMLElement, params:
    HostSessionDragParams)"
  - export interface HostSessionDragParams
verified_by:
  - TEST-OBS-SRC-LIB-ACTIONS-HOSTSESSIONDRAG-TEST-TS-fe959003
  - TEST-OBS-SRC-LIB-ACTIONS-OVERLAYSCROLL-TEST-TS-9878339d
  - TEST-OBS-SRC-LIB-ACTIONS-PANEDOCKDRAG-TEST-TS-3c0ebf73
---

# hostSessionDrag.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
