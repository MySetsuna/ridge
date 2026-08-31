---
id: L4-OBS-SRC-LIB-TEAMMATE-ORCHCONTROLPLANE-TS-abcb9735
level: L4
parent: L3-OBS-SRC-LIB-TEAMMATE-8177ace9
title: orchControlPlane.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/teammate/orchControlPlane.ts
test_targets:
  - src/lib/teammate/agentCommuneModel.test.ts
  - src/lib/teammate/agentPaneHighlightSync.test.ts
  - src/lib/teammate/hitlAuditFilter.test.ts
  - src/lib/teammate/hitlAuditPanel.test.ts
  - src/lib/teammate/layoutEvent.test.ts
  - src/lib/teammate/memberTasks.test.ts
  - src/lib/teammate/orchControlPlane.test.ts
  - src/lib/teammate/teammateGroups.test.ts
  - src/lib/teammate/teammateModel.test.ts
  - src/lib/teammate/workspaceMemory.test.ts
public_interface:
  - "export function buildOrchControlModel(dto: OrchHealthDto | null |
    undefined): OrchControlModel"
  - "export function formatOrchHeader(model: OrchControlModel): string"
  - "export function healthPollMs(model: OrchControlModel): number"
  - "export function normalizeLevel(raw: string | undefined, degraded: boolean):
    ControlPlaneLevel"
  - "export function remoteRosterDot( model: OrchControlModel, agentSuspended:
    boolean, ): 'ok' | 'warn' | 'bad'"
  - "export function shouldRefreshHealth( prevGen: number, nextGen: number,
    pollMs: number, lastPollAt: number, now: number, ): boolean"
  - export interface OrchControlModel
  - export interface OrchHealthDto
  - export type ControlPlaneLevel
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
---

# orchControlPlane.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
