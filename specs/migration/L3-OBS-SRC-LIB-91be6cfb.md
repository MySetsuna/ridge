---
id: L3-OBS-SRC-LIB-91be6cfb
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/devIssue.ts
  - src/lib/types.ts
public_interface:
  - "export function clearDevIssue(): void"
  - "export function reportDevIssue(payload: DevIssuePayload): void"
  - export type AgentState
  - export type DevIssuePayload
  - export type PaneNode
  - export type PaneOrigin
---

# src/lib module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
