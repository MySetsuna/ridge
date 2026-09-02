---
id: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-HOSTS-c13ddbad
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/remote/src/shared/hosts module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/hosts/foreignPaneStatus.ts
  - packages/remote/src/shared/hosts/liveBackpressure.ts
  - packages/remote/src/shared/hosts/livePumpPolicy.ts
  - packages/remote/src/shared/hosts/outboundLifecycle.ts
  - packages/remote/src/shared/hosts/outboundReconnect.ts
public_interface:
  - "export function aggregateDropped( snaps: BackpressureSnapshot[], ):"
  - "export function aggregateDrops(states: PumpState[]): number"
  - "export function applyPumpBatch( state: PumpState, batch: PumpBatch, ):"
  - "export function assertNoCrossHostFanout( sessions: OutboundSession[],
    hostId: string, paneId: string, ): boolean"
  - "export function backpressureLevel(s: BackpressureSnapshot):
    BackpressureLevel"
  - "export function bufferFillPercent(s: BackpressureSnapshot): number"
  - "export function bytesToDropOnAppend( currentLen: number, incomingLen:
    number, cap: number, ): number"
  - "export function createSession(hostId: string, remotePaneId: string):
    OutboundSession"
  - "export function decideForeignPaneBadge(input: ForeignPaneStatusInput):
    ForeignPaneBadge"
  - "export function decideOutboundReconnect(opts: { attempt: number;
    hostReachable: boolean; attachedPaneIds: string[]; intentionalClose:
    boolean; }): OutboundReconnectDecision"
  - "export function foreignCloseConfirmMessage(hostLabel: string): string"
  - "export function formatAggregateDropBadge(agg: { totalDropped: number;
    sheddingHosts: number; }): string"
  - "export function formatBackpressureBadge(s: BackpressureSnapshot): string"
  - "export function formatPumpBadge(state: PumpState): string"
  - "export function hostsLineAlert(opts: { backpressure: BackpressureSnapshot;
    reconnectAttempt: number; }): string | null"
  - "export function initialPumpState(capBytes: number): PumpState"
  - "export function lifecycleSummary(s: OutboundSession): string"
  - "export function mergeOutboundIntoSnapshot( prev: BackpressureSnapshot |
    null, st: { liveBufferCap?: number; liveBufferBytes?: number;
    liveDroppedBytes?: number; }, ): BackpressureSnapshot"
  - "export function orderSessionsForPump( sessions: { sessionId: string;
    bufferedBytes: number; capBytes: number }[], ): string[]"
  - "export function outboundReconnectDelayMs(attempt: number): number | null"
  - "export function pumpIntervalMs(level: ReturnType<typeof backpressureLevel>,
    base = 100): number"
  - "export function reduceLifecycle(s: OutboundSession, ev: LifecycleEvent):
    OutboundSession"
  - "export function safeSubscribe(s: OutboundSession, paneId: string):
    OutboundSession"
  - "export function shouldAccelerateHostsPoll(s: BackpressureSnapshot): boolean"
  - "export function shouldWarnOperator(s: BackpressureSnapshot): boolean"
  - "export function simulateDetachDoesNotError(hostId: string, paneId: string):
    OutboundSession"
  - "export function simulateHappyPath(hostId: string, paneId: string):
    OutboundSession"
  - "export function snapshotFromOutboundStats(st: { liveBufferCap?: number;
    liveBufferBytes?: number; liveDroppedBytes?: number; }):
    BackpressureSnapshot"
  - "export function uniquePaneIds(ids: string[]): string[]"
  - export interface BackpressureSnapshot
  - export interface ForeignPaneStatusInput
  - export interface OutboundSession
  - export interface PumpBatch
  - export interface PumpDecision
  - export interface PumpState
  - export type BackpressureLevel
  - export type ForeignHostLink
  - export type ForeignPaneBadge
  - export type LifecycleEvent
  - export type OutboundPhase
  - export type OutboundReconnectDecision
---

# packages/remote/src/shared/hosts module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
