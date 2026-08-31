---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-SIGNALING-INDEX-TS-e05a443d
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-SIGNALING-54afd28c
title: index.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/cloud/signaling/index.ts
test_targets:
  - packages/remote/src/shared/cloud/signaling/conformance.test.ts
  - packages/remote/src/shared/cloud/signaling/drift.test.ts
public_interface:
  - "export function isInboundSignal(msg: SignalMsg | { t: string }): msg is
    SignalIn"
  - "export function parseSignal(text: string): SignalMsg |"
  - export type SignalIn
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-SIGNALING-CONFORMANCE-TEST-TS-50aa4f26
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-CLOUD-SIGNALING-DRIFT-TEST-TS-52f30651
---

# index.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
