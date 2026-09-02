---
id: L3-OBS-SRC-LIB-TEAMMATE-8177ace9
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/teammate module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/teammate/AgentCenterPanel.svelte
  - src/lib/teammate/agentCommuneModel.ts
  - src/lib/teammate/AgentMemberRow.svelte
  - src/lib/teammate/AgentPaneHighlightSync.svelte
  - src/lib/teammate/agentPaneHighlightSync.ts
  - src/lib/teammate/HitlApprovalModal.svelte
  - src/lib/teammate/hitlAuditFilter.ts
  - src/lib/teammate/hitlAuditPanel.ts
  - src/lib/teammate/layoutChange.golden.json
  - src/lib/teammate/layoutEvent.ts
  - src/lib/teammate/memberTasks.ts
  - src/lib/teammate/orchControlPlane.ts
  - src/lib/teammate/teammateGroups.svelte.ts
  - src/lib/teammate/TeammateGroupsSection.svelte
  - src/lib/teammate/teammateModel.ts
  - src/lib/teammate/teammateSettings.ts
  - src/lib/teammate/workspaceMemory.ts
public_interface:
  - "export async function loadWorkspaceMemory(workspaceId: string | undefined):
    Promise<WorkspaceMemory>"
  - "export async function refreshAgentPaneHighlight(input: { workspaceIds:
    readonly string[]; invoke: (cmd: string, args?: Record<string, unknown>)"
  - "export async function saveWorkspaceMemory( workspaceId: string | undefined,
    memory: WorkspaceMemory, ): Promise<boolean>"
  - "export function addGroup(groups: readonly TeammateGroup[], group:
    TeammateGroup): TeammateGroup[]"
  - "export function addMemberIn( groups: readonly TeammateGroup[], groupId:
    string, agentId: string ): TeammateGroup[]"
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
  - "export function assertRemoteAuditShape(obj: Record<string, unknown>):
    string[]"
  - "export function auditPanelTitle(model: HitlAuditPanelModel): string"
  - "export function auditRiskBadge(risk: string | undefined): string"
  - "export function buildGroup( name: string, color: string, memberAgentIds:
    readonly string[] ): TeammateGroup"
  - "export function buildHitlAuditPanel(items: HitlAuditItem[], maxLines = 12):
    HitlAuditPanelModel"
  - "export function buildOrchControlModel(dto: OrchHealthDto | null |
    undefined): OrchControlModel"
  - "export function buildTask( groupId: string, objective: string, targets:
    readonly string[] ): GroupTask"
  - "export function emptyWorkspaceMemory(): WorkspaceMemory"
  - "export function filterAuditItems( items: HitlAuditItem[], filter:
    Partial<AuditFilter> = {}, ): AuditFilterResult"
  - "export function findGroupByName( groups: readonly TeammateGroup[], name:
    string ): TeammateGroup | undefined"
  - "export function formatAuditTimeline(items: HitlAuditItem[], max = 8):
    string[]"
  - "export function formatOrchHeader(model: OrchControlModel): string"
  - "export function groupByVerdict(items: HitlAuditItem[]): Record<string,
    number>"
  - "export function groupOfAgent( groups: readonly TeammateGroup[], agentId:
    string ): TeammateGroup | undefined"
  - "export function groupsStorageKey(stableKey: string): string"
  - "export function healthPollMs(model: OrchControlModel): number"
  - "export function initTeammateBoot(): void"
  - "export function latchAgentAttention(input: { previousStatus:
    AgentPaneStatus | null | undefined; currentStatus: AgentPaneStatus; pending:
    boolean; profileStatus?: string; outputSeq: number; existingAttention?:
    AgentAttention | null; seenBefore: boolean; }): AgentAttention | null"
  - "export function normalizeAgentIdentity(agent: string): string"
  - "export function normalizeLevel(raw: string | undefined, degraded: boolean):
    ControlPlaneLevel"
  - "export function parseCircuitTripped(payload: unknown): CircuitTrip | null"
  - "export function parseGroupAddMember(raw: unknown): GroupAddMemberEvent |
    null"
  - "export function parseHitlPendingList(raw: unknown):"
  - "export function parseHitlRequest(payload: unknown): HitlRequest | null"
  - "export function parseLayoutChange(payload: unknown): LayoutChange"
  - "export function parsePersisted(raw: string | null): PersistShape"
  - "export function parseTopologySnapshot(payload: unknown): TopologySnapshot"
  - "export function parseWorkspaceMemory(value: unknown): WorkspaceMemory"
  - "export function persistedEquals(a: unknown, b: unknown): boolean"
  - "export function pruneAgentPaneHighlightWorkspaces(workspaceIds: readonly
    string[]): void"
  - "export function recolorGroupIn( groups: readonly TeammateGroup[], id:
    string, color: string ): TeammateGroup[]"
  - "export function recordMemberTask(agentId: string, text: string): void"
  - "export function redactReasonSummary(raw: string, maxLen = 120): string"
  - "export function remoteRosterDot( model: OrchControlModel, agentSuspended:
    boolean, ): 'ok' | 'warn' | 'bad'"
  - "export function removeGroupIn(groups: readonly TeammateGroup[], id:
    string): TeammateGroup[]"
  - "export function removeMemberIn( groups: readonly TeammateGroup[], groupId:
    string, agentId: string ): TeammateGroup[]"
  - "export function renameGroupIn( groups: readonly TeammateGroup[], id:
    string, name: string ): TeammateGroup[]"
  - "export function resetAgentPaneHighlightSync(): void"
  - "export function resolveMembers( memberAgentIds: readonly string[], roster:
    readonly TeammateProfile[] ): ResolvedGroupMember[]"
  - "export function riskLabel(level: RiskLevel): 'L0' | 'L1' | 'L2'"
  - "export function serializePersisted(state: PersistShape): string"
  - "export function setGroupLeaderIn( groups: readonly TeammateGroup[], id:
    string, agentId: string | null ): TeammateGroup[]"
  - "export function setTeammateEnabled(enabled: boolean): void"
  - "export function setTeammateHitlEnabled(enabled: boolean): void"
  - "export function shouldRefreshAgentHistory(lastLoadedAt: number, now =
    Date.now()"
  - "export function shouldRefreshHealth( prevGen: number, nextGen: number,
    pollMs: number, lastPollAt: number, now: number, ): boolean"
  - "export function shouldShowAuditSection(pending: number, historyLen:
    number): boolean"
  - "export function stableWorkspaceKey( workspaceId: string | undefined,
    filePath: string | null | undefined ): string"
  - "export function syncAgentPaneHighlight( members: readonly
    HighlightMember[], isPending: (member: HighlightMember)"
  - "export function syncTeammateBackend(s: UserSettings = get(settingsStore)"
  - "export function teammateGroupStore(): TeammateGroupStore"
  - "export function withTask( tasks: readonly GroupTask[], task: GroupTask,
    cap: number = TASK_CAP ): GroupTask[]"
  - export interface AgentHistoryGroup
  - export interface AgentHistoryReplyLike
  - export interface AgentReplyLookupProfile
  - export interface AgentStatusProfile
  - export interface AuditFilter
  - export interface AuditFilterResult
  - export interface CircuitTrip
  - export interface GroupAddMemberEvent
  - export interface GroupTask
  - export interface HitlAuditPanelModel
  - export interface HitlDecision
  - export interface HitlRequest
  - export interface LayoutChange
  - export interface MemberTask
  - export interface OrchControlModel
  - export interface OrchHealthDto
  - export interface PendingApproval
  - export interface ResolvedGroupMember
  - export interface TeammateGroup
  - export interface TeammateProfile
  - export interface TopologyEdge
  - export interface TopologySnapshot
  - export interface WorkspaceMemory
  - export type AgentActivity
  - export type AgentAttention
  - export type AgentCardStatus
  - export type AgentPaneHighlightRefresh
  - export type AgentPaneStatus
  - export type AgentRole
  - export type AgentTier
  - export type ControlPlaneLevel
  - export type HighlightMember
  - export type HitlVerdict
  - export type LayoutChangeKind
  - export type RiskLevel
  - export type TeammateStatus
---

# src/lib/teammate module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
