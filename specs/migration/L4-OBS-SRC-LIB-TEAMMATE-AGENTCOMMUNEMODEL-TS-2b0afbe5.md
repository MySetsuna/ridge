---
id: L4-OBS-SRC-LIB-TEAMMATE-AGENTCOMMUNEMODEL-TS-2b0afbe5
level: L4
parent: L3-OBS-SRC-LIB-TEAMMATE-8177ace9
title: agentCommuneModel.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/teammate/agentCommuneModel.ts
test_targets:
  - src/lib/teammate/agentCommuneModel.test.ts
  - src/lib/teammate/agentPaneHighlightSync.test.ts
  - src/lib/teammate/hitlAuditFilter.test.ts
  - src/lib/teammate/hitlAuditPanel.test.ts
  - src/lib/teammate/memberTasks.test.ts
  - src/lib/teammate/orchControlPlane.test.ts
  - src/lib/teammate/teammateGroups.test.ts
  - src/lib/teammate/teammateModel.test.ts
  - src/lib/teammate/workspaceMemory.test.ts
public_interface:
  - "export function agentAttentionForTransition( previousStatus:
    AgentPaneStatus | null | undefined, currentStatus: AgentPaneStatus,
    pendingApproval: boolean, profileStatus?: string, ): AgentAttention | null"
  - "export function agentAttentionPriority(attention: AgentAttention): number"
  - "export function agentCardStatus( profile: AgentStatusProfile | undefined,
    pendingApproval: boolean, ): AgentCardStatus"
  - "export function agentIdentityAliases(agent: string): string[]"
  - "export function agentPaneStatus( profile: AgentStatusProfile,
    pendingApproval: boolean, ): AgentPaneStatus"
  - "export function agentStatusLabel(status: AgentCardStatus): string"
  - "export function aggregateAgentCardStatus(statuses: readonly
    AgentCardStatus[]): AgentCardStatus"
  - "export function isAgentReplyKind(kind: unknown): boolean"
  - "export function latchAgentAttention(input: { previousStatus:
    AgentPaneStatus | null | undefined; currentStatus: AgentPaneStatus; pending:
    boolean; profileStatus?: string; outputSeq: number; existingAttention?:
    AgentAttention | null; seenBefore: boolean; }): AgentAttention | null"
  - "export function normalizeAgentIdentity(agent: string): string"
  - "export function shouldRefreshAgentHistory(lastLoadedAt: number, now =
    Date.now()"
  - "export function workspaceTranscriptScope( workspaceFilePath: string | null
    | undefined, candidateCwds: readonly string[], ): string[]"
  - export interface AgentHistoryGroup
  - export interface AgentHistoryReplyLike
  - export interface AgentReplyLookupProfile
  - export interface AgentStatusProfile
  - export type AgentAttention
  - export type AgentCardStatus
  - export type AgentPaneStatus
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

# agentCommuneModel.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.

## CHG-090 映射处置 — 测试引用

旧测试已随 CHG-070 源链迁出。保留原始测试映射及 Git 归档证据，移除现役失效引用。其余原有测试目标保留，不按名称猜测替代覆盖。

原始 test_targets

```json
[
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
```

`src/lib/teammate/layoutEvent.test.ts` → `docs/archive/CHG-070/frontend-runtime-events/layoutEvent.test.ts`

## CHG-090 映射处置 — 导出快照

CHG-087 的回复类型识别支撑新消息与历史导航。 仅更新迁移观察的确定性导出快照，产品要求保留。按 SpecTree SOP 执行 interface L4-OBS-SRC-LIB-TEAMMATE-AGENTCOMMUNEMODEL-TS-2b0afbe5 --confirm 后重新批准和锁定。

旧声明保留如下。

```json
[
  "export function agentAttentionForTransition( previousStatus: AgentPaneStatus | null | undefined, currentStatus: AgentPaneStatus, pendingApproval: boolean, profileStatus?: string, ): AgentAttention | null",
  "export function agentAttentionPriority(attention: AgentAttention): number",
  "export function agentCardStatus( profile: AgentStatusProfile | undefined, pendingApproval: boolean, ): AgentCardStatus",
  "export function agentIdentityAliases(agent: string): string[]",
  "export function agentPaneStatus( profile: AgentStatusProfile, pendingApproval: boolean, ): AgentPaneStatus",
  "export function agentStatusLabel(status: AgentCardStatus): string",
  "export function aggregateAgentCardStatus(statuses: readonly AgentCardStatus[]): AgentCardStatus",
  "export function latchAgentAttention(input: { previousStatus: AgentPaneStatus | null | undefined; currentStatus: AgentPaneStatus; pending: boolean; profileStatus?: string; outputSeq: number; existingAttention?: AgentAttention | null; seenBefore: boolean; }): AgentAttention | null",
  "export function normalizeAgentIdentity(agent: string): string",
  "export function shouldRefreshAgentHistory(lastLoadedAt: number, now = Date.now()",
  "export function workspaceTranscriptScope( workspaceFilePath: string | null | undefined, candidateCwds: readonly string[], ): string[]",
  "export interface AgentHistoryGroup",
  "export interface AgentHistoryReplyLike",
  "export interface AgentReplyLookupProfile",
  "export interface AgentStatusProfile",
  "export type AgentAttention",
  "export type AgentCardStatus",
  "export type AgentPaneStatus"
]
```
