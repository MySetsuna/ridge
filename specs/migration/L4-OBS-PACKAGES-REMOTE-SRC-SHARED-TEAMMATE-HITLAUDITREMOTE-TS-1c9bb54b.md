---
id: L4-OBS-PACKAGES-REMOTE-SRC-SHARED-TEAMMATE-HITLAUDITREMOTE-TS-1c9bb54b
level: L4
parent: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-TEAMMATE-c31105a3
title: hitlAuditRemote.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/teammate/hitlAuditRemote.ts
test_targets:
  - packages/remote/src/shared/teammate/hitlAuditRemote.test.ts
public_interface:
  - "export async function fetchHitlAuditRemote( invoke: HitlInvoke, limit = 20,
    ): Promise<HitlAuditList>"
  - "export function countTerminalOutcomes(items: HitlAuditItem[]):"
  - "export function formatAuditLine(i: HitlAuditItem): string"
  - "export function isRedactedAuditItem(i: Record<string, unknown>): boolean"
  - export interface HitlAuditItem
  - export interface HitlAuditList
  - export type HitlInvoke
verified_by:
  - TEST-OBS-PACKAGES-REMOTE-SRC-SHARED-TEAMMATE-HITLAUDITREMOTE-TEST-TS-27ed6157
---

# hitlAuditRemote.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
