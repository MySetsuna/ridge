---
id: L3-OBS-SRC-LIB-TRANSPORT-0b23356e
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/transport module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/transport/context.ts
  - src/lib/transport/index.ts
  - src/lib/transport/tauri.ts
  - src/lib/transport/types.ts
  - src/lib/transport/ws.ts
public_interface:
  - export class TauriDataProvider
  - export class WsDataProvider
  - "export function getTransport(): DataProvider"
  - "export function hasTransport(): boolean"
  - "export function setTransport(provider: DataProvider): void"
  - export interface DataProvider
  - export interface GitGraphResult
  - export interface GitStatusResult
  - export interface SearchResult
  - export type DataInvoke
---

# src/lib/transport module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
