---
id: L4-OBS-SRC-LIB-HOSTS-HOSTSESSIONISOLATION-TS-aac1c3cf
level: L4
parent: L3-OBS-SRC-LIB-HOSTS-71cf76ee
title: hostSessionIsolation.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/hosts/hostSessionIsolation.ts
test_targets:
  - src/lib/hosts/compositionHarness.test.ts
  - src/lib/hosts/foreignHistorySession.test.ts
  - src/lib/hosts/hostConnectFlow.test.ts
  - src/lib/hosts/hostControlSurface.test.ts
  - src/lib/hosts/hostForest.test.ts
  - src/lib/hosts/hostSessionIsolation.test.ts
  - src/lib/hosts/remotePaneBindings.test.ts
public_interface:
  - "export function canStep(task: HostTask): boolean"
  - "export function cancelHostTask(task: HostTask): HostTask"
  - "export function checkHostTaskIsolation(tasks: HostTask[]): IsolationCheck"
  - "export function isolationHeader(tasks: HostTask[]): string"
  - "export function onIntentionalDisconnect(task: HostTask | undefined):
    HostTask | null"
  - "export function reconnectBadge(task: HostTask | null | undefined): string"
  - "export function scheduleReconnectTask( existing: HostTask | undefined,
    hostId: string, attachedPaneIds: string[], ): HostTask"
  - "export function stepHostTask( task: HostTask, opts: { hostReachable:
    boolean; maxAttempts?: number }, ): HostTask"
  - "export function unique(ids: string[]): string[]"
  - export interface HostTask
  - export interface IsolationCheck
  - export type ReconnectPhaseUi
verified_by:
  - TEST-OBS-SRC-LIB-HOSTS-COMPOSITIONHARNESS-TEST-TS-911c519a
  - TEST-OBS-SRC-LIB-HOSTS-FOREIGNHISTORYSESSION-TEST-TS-0de097ce
  - TEST-OBS-SRC-LIB-HOSTS-HOSTCONNECTFLOW-TEST-TS-a765764d
  - TEST-OBS-SRC-LIB-HOSTS-HOSTCONTROLSURFACE-TEST-TS-3e384126
  - TEST-OBS-SRC-LIB-HOSTS-HOSTFOREST-TEST-TS-b3e38a29
  - TEST-OBS-SRC-LIB-HOSTS-HOSTSESSIONISOLATION-TEST-TS-a2500470
  - TEST-OBS-SRC-LIB-HOSTS-REMOTEPANEBINDINGS-TEST-TS-2377e089
---

# hostSessionIsolation.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
