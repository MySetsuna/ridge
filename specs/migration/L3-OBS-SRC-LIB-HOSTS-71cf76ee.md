---
id: L3-OBS-SRC-LIB-HOSTS-71cf76ee
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/hosts module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/hosts/compositionHarness.ts
  - src/lib/hosts/foreignHistorySession.ts
  - src/lib/hosts/hostControlSurface.ts
  - src/lib/hosts/hostForest.ts
  - src/lib/hosts/hostSessionIsolation.ts
  - src/lib/hosts/remotePaneBindings.ts
public_interface:
  - "export async function deleteRemotePane( hostId: string,
    workspaceIdOrRemotePaneId: string, remotePaneIdOrLink: string |
    HostTopologyLink, linkOrCloseLocal: HostTopologyLink | ((localPaneId:
    string)"
  - "export async function loadHostForest( sources: readonly HostForestSource[],
    onProgress?: HostForestProgressListener, ): Promise<HostForestResult[]>"
  - "export async function settleHostTopologyRefreshes( jobs: readonly
    HostTopologyRefreshJob[], onSettled: (result: HostForestResult)"
  - "export function activateRemotePaneBinding(localPaneId: string): void"
  - "export function appendCappedPlan( currentLen: number, incomingLen: number,
    cap: number, ):"
  - "export function assertSessionIsolation( entries: HistorySessionKey[], ):"
  - "export function badgeForHostSession(opts: { hostStatus: string; hostLabel:
    string; attached: boolean; subscribed: boolean; reconnectAttempt: number;
    lastError?: string; })"
  - "export function bindRemotePane(binding: RemotePaneBinding): void"
  - "export function buildHostRowAlerts(row: HostRowModel): string[]"
  - "export function canStep(task: HostTask): boolean"
  - "export function cancelHostTask(task: HostTask): HostTask"
  - "export function checkHostTaskIsolation(tasks: HostTask[]): IsolationCheck"
  - "export function clampHistoryCap(cap: number): number"
  - "export function compositionAllGreen(): boolean"
  - "export function compositionReport(): string"
  - "export function confirmDetachMessage(hostLabel: string): string"
  - "export function formatHistoryDiag(snap: HistoryTailSnapshot): string"
  - "export function historyKey(hostId: string, sessionId: string): string"
  - "export function historyPullBudget( wantLines: number, cols: number, cap =
    DEFAULT_HISTORY_TAIL_CAP, ): number"
  - "export function hostStatusToLink(status: string): ForeignHostLink"
  - "export function hostTopologyErrorKind(error?: string): 'auth' | 'retryable'"
  - "export function hostsHeaderSummary(rows: HostRowModel[]): string"
  - "export function hostsPollIntervalMs(rows: HostRowModel[], defaultMs =
    5000): number"
  - "export function isolationHeader(tasks: HostTask[]): string"
  - "export function localPaneIdsForRemote(hostId: string, remotePaneId:
    string): string[]"
  - "export function onIntentionalDisconnect(task: HostTask | undefined):
    HostTask | null"
  - "export function planAttachSeed(opts: { localTailBytes: number; rows:
    number; cols: number; reattach: boolean; hostHistoryKnown: boolean; }):
    AttachSeedPlan"
  - "export function planDetachHistory(opts: { keepLocalTail: boolean; }):"
  - "export function promoteRemotePaneBinding(localPaneId: string): void"
  - "export function reconnectBadge(task: HostTask | null | undefined): string"
  - "export function remotePaneBinding(localPaneId: string): RemotePaneBinding |
    undefined"
  - "export function retainHostForest( previous: HostForestResult | undefined,
    next: HostForestResult, ): HostForestResult"
  - "export function runAllCompositionScenarios(): CompositionScenario[]"
  - "export function scenarioGitOrch(): CompositionScenario"
  - "export function scenarioHitlHostsUi(): CompositionScenario"
  - "export function scenarioIsolationPump(): CompositionScenario"
  - "export function scenarioOutboundHistory(): CompositionScenario"
  - "export function scenarioProtocolLink(): CompositionScenario"
  - "export function scheduleReconnectTask( existing: HostTask | undefined,
    hostId: string, attachedPaneIds: string[], ): HostTask"
  - "export function showHistoryStrip(bytes: number, attached: boolean): boolean"
  - "export function showReconnectControls(row: HostRowModel): boolean"
  - "export function sortHostRows(rows: HostRowModel[]): HostRowModel[]"
  - "export function stepHostTask( task: HostTask, opts: { hostReachable:
    boolean; maxAttempts?: number }, ): HostTask"
  - "export function summarizeHistoryBadge(snap: HistoryTailSnapshot | null):
    string"
  - "export function summarizeOutbound(row: HostRowModel): string"
  - "export function terminalPathOrigin(localPaneId: string):"
  - "export function unbindRemotePane(localPaneId: string): void"
  - "export function unique(ids: string[]): string[]"
  - export interface AttachSeedPlan
  - export interface CompositionScenario
  - export interface HistorySessionKey
  - export interface HistoryTailSnapshot
  - export interface HostForestPane
  - export interface HostForestResult
  - export interface HostForestSource
  - export interface HostForestWorkspace
  - export interface HostRowModel
  - export interface HostSessionRowModel
  - export interface HostTask
  - export interface HostTopologyLink
  - export interface HostTopologyRefreshJob
  - export interface IsolationCheck
  - export interface RemotePaneBinding
  - export type HostForestLink
  - export type HostForestProgressListener
  - export type HostKindUi
  - export type ReconnectPhaseUi
---

# src/lib/hosts module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
