---
id: L4-OBS-SRC-LIB-TERMINAL-PTYWRITEQUEUE-TS-0440e203
level: L4
parent: L3-OBS-SRC-LIB-TERMINAL-033d05be
title: ptyWriteQueue.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/terminal/ptyWriteQueue.ts
test_targets:
  - src/lib/terminal/desktopPaneResize.test.ts
  - src/lib/terminal/hostPorts.test.ts
  - src/lib/terminal/paneSizeSync.test.ts
  - src/lib/terminal/ptyRuntimeSnapshot.test.ts
  - src/lib/terminal/ptyWriteQueue.test.ts
  - src/lib/terminal/refreshEntrypoints.test.ts
  - src/lib/terminal/rendererBackend.test.ts
public_interface:
  - export class PtyWriteQueueFullError
  - export class PtyWriteQueueRetiredError
  - "export function enqueuePtyInput( key: string, data: string, write: (data:
    string)"
  - "export function enqueuePtyWrite( key: string, write: ()"
  - "export function flushPtyInput(key: string): void"
  - "export function retirePtyWriteQueue(key: string): void"
  - "export function retirePtyWriteQueuesForPane(paneId: string): void"
  - export interface PtyWriteQueueOptions
verified_by:
  - TEST-OBS-SRC-LIB-TERMINAL-DESKTOPPANERESIZE-TEST-TS-38627314
  - TEST-OBS-SRC-LIB-TERMINAL-HOSTPORTS-TEST-TS-bed9ff49
  - TEST-OBS-SRC-LIB-TERMINAL-PANESIZESYNC-TEST-TS-8a1e46ba
  - TEST-OBS-SRC-LIB-TERMINAL-PTYRUNTIMESNAPSHOT-TEST-TS-a223e946
  - TEST-OBS-SRC-LIB-TERMINAL-PTYWRITEQUEUE-TEST-TS-b49b9e91
  - TEST-OBS-SRC-LIB-TERMINAL-REFRESHENTRYPOINTS-TEST-TS-dcf05e4d
  - TEST-OBS-SRC-LIB-TERMINAL-RENDERERBACKEND-TEST-TS-3fecf7c5
---

# ptyWriteQueue.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
