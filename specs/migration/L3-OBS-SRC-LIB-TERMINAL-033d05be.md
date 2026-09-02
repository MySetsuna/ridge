---
id: L3-OBS-SRC-LIB-TERMINAL-033d05be
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/terminal module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/terminal/desktopPaneResize.ts
  - src/lib/terminal/hostPorts.ts
  - src/lib/terminal/paneSizeSync.ts
  - src/lib/terminal/ptyRuntimeSnapshot.ts
  - src/lib/terminal/ptyWriteQueue.ts
public_interface:
  - export class PtyWriteQueueFullError
  - export class PtyWriteQueueRetiredError
  - "export function buildPtyRuntimeSnapshot( input: PtyRuntimeSnapshotInput, ):
    PtyRuntimeSnapshotPayload"
  - "export function enqueuePtyInput( key: string, data: string, write: (data:
    string)"
  - "export function enqueuePtyWrite( key: string, write: ()"
  - "export function flushPtyInput(key: string): void"
  - "export function makeHostPorts(): HostPorts"
  - "export function retirePtyWriteQueue(key: string): void"
  - "export function retirePtyWriteQueuesForPane(paneId: string): void"
  - "export function scheduleForcedPaneResize( scheduler: Pick<PaneRpcScheduler,
    'scheduleResizeAndWait'>, pane: PaneRef, rows: number, cols: number,
    params?: Readonly<Record<string, unknown>>, ): Promise<void>"
  - "export function schedulePaneSizeSynchronization(paneId: string): void"
  - "export function synchronizePaneSize(paneId: string): boolean"
  - export interface PtyRuntimeSnapshotInput
  - export interface PtyRuntimeSnapshotPayload
  - export interface PtyWriteQueueOptions
---

# src/lib/terminal module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
