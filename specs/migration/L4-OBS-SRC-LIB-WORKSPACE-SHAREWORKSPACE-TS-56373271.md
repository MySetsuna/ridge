---
id: L4-OBS-SRC-LIB-WORKSPACE-SHAREWORKSPACE-TS-56373271
level: L4
parent: L3-OBS-SRC-LIB-WORKSPACE-6c1d1495
title: shareWorkspace.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/workspace/shareWorkspace.ts
public_interface:
  - "export async function shareWorkspaceWithAccount(input: { workspaceId:
    string; workspaceName?: string; deviceName?: string; }): Promise<boolean>"
---

# shareWorkspace.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
