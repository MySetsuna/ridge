---
id: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-9518faec
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/remote/src/shared module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/reconnectPolicy.ts
public_interface:
  - "export function backoffMs( attempt: number, baseMs: number =
    RECONNECT_BASE_MS, maxMs: number = RECONNECT_MAX_MS, ): number"
  - "export function shouldRetry(attempt: number, maxAttempts: number): boolean"
---

# packages/remote/src/shared module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
