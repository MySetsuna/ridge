---
id: L4-OBS-SRC-LIB-TEAMMATE-LAYOUTEVENT-TS-da106aca
level: L4
parent: L3-OBS-SRC-LIB-TEAMMATE-8177ace9
title: layoutEvent.ts
status: RETIRED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - docs/archive/CHG-070/frontend-runtime-events/layoutEvent.ts
test_targets: []
public_interface:
  - "export function parseLayoutChange(payload: unknown): LayoutChange"
  - export interface LayoutChange
  - export type LayoutChangeKind
verified_by:
  - TEST-OBS-SRC-LIB-TEAMMATE-AGENTCOMMUNEMODEL-TEST-TS-313d1942
  - TEST-OBS-SRC-LIB-TEAMMATE-AGENTPANEHIGHLIGHTSYNC-TEST-TS-68715b97
  - TEST-OBS-SRC-LIB-TEAMMATE-HITLAUDITFILTER-TEST-TS-1bebca23
  - TEST-OBS-SRC-LIB-TEAMMATE-HITLAUDITPANEL-TEST-TS-44e3a8b8
  - TEST-OBS-SRC-LIB-TEAMMATE-LAYOUTEVENT-TEST-TS-b130fa00
  - TEST-OBS-SRC-LIB-TEAMMATE-MEMBERTASKS-TEST-TS-0d803223
  - TEST-OBS-SRC-LIB-TEAMMATE-ORCHCONTROLPLANE-TEST-TS-71f485a7
  - TEST-OBS-SRC-LIB-TEAMMATE-TEAMMATEGROUPS-TEST-TS-e7ea1fa2
  - TEST-OBS-SRC-LIB-TEAMMATE-TEAMMATEMODEL-TEST-TS-99fea951
  - TEST-OBS-SRC-LIB-TEAMMATE-WORKSPACEMEMORY-TEST-TS-ac0bbd3e
lifecycle: ARCHIVED
archived_at: 2026-11-06T11:16:19.635Z
archived_by_change: CHG-090
---

# layoutEvent.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置

该源路径已退出生产树，退役此观察记录，具体业务能力由现役契约承接。 保留原文、原始映射与最后存在的 Git blob，code_targets 仅标记历史证据。

`src/lib/teammate/layoutEvent.ts` → `docs/archive/CHG-070/frontend-runtime-events/layoutEvent.ts`，Git 3c7a18af4bb88aa3a7131cc45b51624a1fda699a / blob 872c965fe4b34a531eb81b364b36c51608fbdc15

原始映射保留如下，历史归档仅用于溯源。

```json
{
  "codeTargets": [
    "src/lib/teammate/layoutEvent.ts"
  ],
  "testTargets": [
    "src/lib/teammate/agentCommuneModel.test.ts",
    "src/lib/teammate/agentPaneHighlightSync.test.ts",
    "src/lib/teammate/hitlAuditFilter.test.ts",
    "src/lib/teammate/hitlAuditPanel.test.ts",
    "src/lib/teammate/layoutEvent.test.ts",
    "src/lib/teammate/memberTasks.test.ts",
    "src/lib/teammate/orchControlPlane.test.ts",
    "src/lib/teammate/teammateGroups.test.ts",
    "src/lib/teammate/teammateModel.test.ts",
    "src/lib/teammate/workspaceMemory.test.ts"
  ]
}
```
