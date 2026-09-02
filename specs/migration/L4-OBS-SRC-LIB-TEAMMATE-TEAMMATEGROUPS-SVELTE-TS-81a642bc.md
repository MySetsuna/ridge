---
id: L4-OBS-SRC-LIB-TEAMMATE-TEAMMATEGROUPS-SVELTE-TS-81a642bc
level: L4
parent: L3-OBS-SRC-LIB-TEAMMATE-8177ace9
title: teammateGroups.svelte.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/teammate/teammateGroups.svelte.ts
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
  - "export function addGroup(groups: readonly TeammateGroup[], group:
    TeammateGroup): TeammateGroup[]"
  - "export function addMemberIn( groups: readonly TeammateGroup[], groupId:
    string, agentId: string ): TeammateGroup[]"
  - "export function buildGroup( name: string, color: string, memberAgentIds:
    readonly string[] ): TeammateGroup"
  - "export function buildTask( groupId: string, objective: string, targets:
    readonly string[] ): GroupTask"
  - "export function findGroupByName( groups: readonly TeammateGroup[], name:
    string ): TeammateGroup | undefined"
  - "export function groupOfAgent( groups: readonly TeammateGroup[], agentId:
    string ): TeammateGroup | undefined"
  - "export function groupsStorageKey(stableKey: string): string"
  - "export function parseGroupAddMember(raw: unknown): GroupAddMemberEvent |
    null"
  - "export function parsePersisted(raw: string | null): PersistShape"
  - "export function persistedEquals(a: unknown, b: unknown): boolean"
  - "export function recolorGroupIn( groups: readonly TeammateGroup[], id:
    string, color: string ): TeammateGroup[]"
  - "export function removeGroupIn(groups: readonly TeammateGroup[], id:
    string): TeammateGroup[]"
  - "export function removeMemberIn( groups: readonly TeammateGroup[], groupId:
    string, agentId: string ): TeammateGroup[]"
  - "export function renameGroupIn( groups: readonly TeammateGroup[], id:
    string, name: string ): TeammateGroup[]"
  - "export function resolveMembers( memberAgentIds: readonly string[], roster:
    readonly TeammateProfile[] ): ResolvedGroupMember[]"
  - "export function serializePersisted(state: PersistShape): string"
  - "export function setGroupLeaderIn( groups: readonly TeammateGroup[], id:
    string, agentId: string | null ): TeammateGroup[]"
  - "export function stableWorkspaceKey( workspaceId: string | undefined,
    filePath: string | null | undefined ): string"
  - "export function teammateGroupStore(): TeammateGroupStore"
  - "export function withTask( tasks: readonly GroupTask[], task: GroupTask,
    cap: number = TASK_CAP ): GroupTask[]"
  - export interface GroupAddMemberEvent
  - export interface GroupTask
  - export interface ResolvedGroupMember
  - export interface TeammateGroup
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

# teammateGroups.svelte.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
