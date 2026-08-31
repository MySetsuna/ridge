---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-FOREIGNPANESTATUS-TS-4e34362a
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-c13ddbad
title: foreignPaneStatus.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/hosts/foreignPaneStatus.ts
test_targets:
  - packages/remote/src/shared/hosts/foreignPaneStatus.test.ts
  - packages/remote/src/shared/hosts/liveBackpressure.test.ts
  - packages/remote/src/shared/hosts/livePumpPolicy.test.ts
  - packages/remote/src/shared/hosts/outboundLifecycle.test.ts
  - packages/remote/src/shared/hosts/outboundReconnect.test.ts
public_interface:
  - "export function decideForeignPaneBadge(input: ForeignPaneStatusInput):
    ForeignPaneBadge"
  - "export function foreignCloseConfirmMessage(hostLabel: string): string"
  - export interface ForeignPaneStatusInput
  - export type ForeignHostLink
  - export type ForeignPaneBadge
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-FOREIGNPANESTATUS-TEST-TS-9fbe3344
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEBACKPRESSURE-TEST-TS-cfea6cb6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEPUMPPOLICY-TEST-TS-1ed634a5
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDLIFECYCLE-TEST-TS-9e8e8887
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDRECONNECT-TEST-TS-1dd36e13
---

# foreignPaneStatus.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
