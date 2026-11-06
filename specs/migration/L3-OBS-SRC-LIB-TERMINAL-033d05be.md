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
  - src/lib/terminal/ptyWriteQueue.ts
public_interface:
  - export class PtyWriteQueueFullError
  - export class PtyWriteQueueRetiredError
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
  - export interface PtyWriteQueueOptions
test_targets: []
---

# src/lib/terminal module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

保留仍存在的观察目标。移除失效的现役映射，已迁出路径留历史证据，不能据观察记录推断新位置已实现同一行为。

`src/lib/terminal/ptyRuntimeSnapshot.ts` → `docs/archive/CHG-090/source/src/lib/terminal/ptyRuntimeSnapshot.ts`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 10cc67ef6863f76e130c5d8399ee28b7c95eedf1

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src/lib/terminal/desktopPaneResize.ts",
    "src/lib/terminal/hostPorts.ts",
    "src/lib/terminal/paneSizeSync.ts",
    "src/lib/terminal/ptyRuntimeSnapshot.ts",
    "src/lib/terminal/ptyWriteQueue.ts"
  ],
  "testTargets": []
}
```

## CHG-090 映射处置 — 导出快照

CHG-070 迁出客户端 PTY snapshot producer，目录观察移除其旧导出。 仅更新迁移观察的确定性导出快照，产品要求保留。按 SpecTree SOP 执行 interface L3-OBS-SRC-LIB-TERMINAL-033d05be --confirm 后重新批准和锁定。

旧声明保留如下。

```json
[
  "export class PtyWriteQueueFullError",
  "export class PtyWriteQueueRetiredError",
  "export function buildPtyRuntimeSnapshot( input: PtyRuntimeSnapshotInput, ): PtyRuntimeSnapshotPayload",
  "export function enqueuePtyInput( key: string, data: string, write: (data: string)",
  "export function enqueuePtyWrite( key: string, write: ()",
  "export function flushPtyInput(key: string): void",
  "export function makeHostPorts(): HostPorts",
  "export function retirePtyWriteQueue(key: string): void",
  "export function retirePtyWriteQueuesForPane(paneId: string): void",
  "export function scheduleForcedPaneResize( scheduler: Pick<PaneRpcScheduler, 'scheduleResizeAndWait'>, pane: PaneRef, rows: number, cols: number, params?: Readonly<Record<string, unknown>>, ): Promise<void>",
  "export function schedulePaneSizeSynchronization(paneId: string): void",
  "export function synchronizePaneSize(paneId: string): boolean",
  "export interface PtyRuntimeSnapshotInput",
  "export interface PtyRuntimeSnapshotPayload",
  "export interface PtyWriteQueueOptions"
]
```
