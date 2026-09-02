---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDLIFECYCLE-TS-5737b678
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-c13ddbad
title: outboundLifecycle.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/hosts/outboundLifecycle.ts
test_targets:
  - packages/remote/src/shared/hosts/foreignPaneStatus.test.ts
  - packages/remote/src/shared/hosts/liveBackpressure.test.ts
  - packages/remote/src/shared/hosts/livePumpPolicy.test.ts
  - packages/remote/src/shared/hosts/outboundLifecycle.test.ts
  - packages/remote/src/shared/hosts/outboundReconnect.test.ts
public_interface:
  - "export function assertNoCrossHostFanout( sessions: OutboundSession[],
    hostId: string, paneId: string, ): boolean"
  - "export function createSession(hostId: string, remotePaneId: string):
    OutboundSession"
  - "export function lifecycleSummary(s: OutboundSession): string"
  - "export function reduceLifecycle(s: OutboundSession, ev: LifecycleEvent):
    OutboundSession"
  - "export function safeSubscribe(s: OutboundSession, paneId: string):
    OutboundSession"
  - "export function simulateDetachDoesNotError(hostId: string, paneId: string):
    OutboundSession"
  - "export function simulateHappyPath(hostId: string, paneId: string):
    OutboundSession"
  - export interface OutboundSession
  - export type LifecycleEvent
  - export type OutboundPhase
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-FOREIGNPANESTATUS-TEST-TS-9fbe3344
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEBACKPRESSURE-TEST-TS-cfea6cb6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEPUMPPOLICY-TEST-TS-1ed634a5
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDLIFECYCLE-TEST-TS-9e8e8887
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDRECONNECT-TEST-TS-1dd36e13
---

# outboundLifecycle.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
