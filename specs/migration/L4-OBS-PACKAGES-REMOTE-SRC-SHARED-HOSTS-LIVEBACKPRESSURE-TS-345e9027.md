---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEBACKPRESSURE-TS-345e9027
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-c13ddbad
title: liveBackpressure.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/hosts/liveBackpressure.ts
test_targets:
  - packages/remote/src/shared/hosts/foreignPaneStatus.test.ts
  - packages/remote/src/shared/hosts/liveBackpressure.test.ts
  - packages/remote/src/shared/hosts/livePumpPolicy.test.ts
  - packages/remote/src/shared/hosts/outboundLifecycle.test.ts
  - packages/remote/src/shared/hosts/outboundReconnect.test.ts
public_interface:
  - "export function aggregateDropped( snaps: BackpressureSnapshot[], ):"
  - "export function backpressureLevel(s: BackpressureSnapshot):
    BackpressureLevel"
  - "export function bufferFillPercent(s: BackpressureSnapshot): number"
  - "export function bytesToDropOnAppend( currentLen: number, incomingLen:
    number, cap: number, ): number"
  - "export function formatAggregateDropBadge(agg: { totalDropped: number;
    sheddingHosts: number; }): string"
  - "export function formatBackpressureBadge(s: BackpressureSnapshot): string"
  - "export function hostsLineAlert(opts: { backpressure: BackpressureSnapshot;
    reconnectAttempt: number; }): string | null"
  - "export function mergeOutboundIntoSnapshot( prev: BackpressureSnapshot |
    null, st: { liveBufferCap?: number; liveBufferBytes?: number;
    liveDroppedBytes?: number; }, ): BackpressureSnapshot"
  - "export function shouldAccelerateHostsPoll(s: BackpressureSnapshot): boolean"
  - "export function shouldWarnOperator(s: BackpressureSnapshot): boolean"
  - "export function snapshotFromOutboundStats(st: { liveBufferCap?: number;
    liveBufferBytes?: number; liveDroppedBytes?: number; }):
    BackpressureSnapshot"
  - export interface BackpressureSnapshot
  - export type BackpressureLevel
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-FOREIGNPANESTATUS-TEST-TS-9fbe3344
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEBACKPRESSURE-TEST-TS-cfea6cb6
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-LIVEPUMPPOLICY-TEST-TS-1ed634a5
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDLIFECYCLE-TEST-TS-9e8e8887
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-OUTBOUNDRECONNECT-TEST-TS-1dd36e13
---

# liveBackpressure.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
