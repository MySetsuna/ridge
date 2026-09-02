---
id: L4-OBS-SRC-LIB-ACTIONS-PANEDOCKDRAG-TS-6c04571c
level: L4
parent: L3-OBS-SRC-LIB-ACTIONS-531bf1fb
title: paneDockDrag.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/actions/paneDockDrag.ts
test_targets:
  - src/lib/actions/hostSessionDrag.test.ts
  - src/lib/actions/overlayScroll.test.ts
  - src/lib/actions/paneDockDrag.test.ts
public_interface:
  - "export function paneDockDrag(node: HTMLElement, params: Params)"
verified_by:
  - TEST-OBS-SRC-LIB-ACTIONS-HOSTSESSIONDRAG-TEST-TS-fe959003
  - TEST-OBS-SRC-LIB-ACTIONS-OVERLAYSCROLL-TEST-TS-9878339d
  - TEST-OBS-SRC-LIB-ACTIONS-PANEDOCKDRAG-TEST-TS-3c0ebf73
---

# paneDockDrag.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
