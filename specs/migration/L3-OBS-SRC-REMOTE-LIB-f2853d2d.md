---
id: L3-OBS-SRC-REMOTE-LIB-f2853d2d
level: L3
parent: L2-OBS-SRC-25a66342
title: src/remote/lib module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/remote/lib/clipboard.ts
  - src/remote/lib/cloudRemote.ts
  - src/remote/lib/FileViewer.svelte
  - src/remote/lib/generationGuard.ts
  - src/remote/lib/keyboardOffset.ts
  - src/remote/lib/listenerCleanup.ts
  - src/remote/lib/mobileRemoteUiState.svelte.ts
  - src/remote/lib/modState.svelte.ts
  - src/remote/lib/paneFeedScheduler.ts
  - src/remote/lib/paneLifecycle.ts
  - src/remote/lib/PaneShellPicker.svelte
  - src/remote/lib/paneSwitchBuffer.ts
  - src/remote/lib/remoteGitActions.ts
  - src/remote/lib/RemoteGitPanel.svelte
  - src/remote/lib/remoteQueries.ts
  - src/remote/lib/RemoteSidebar.svelte
  - src/remote/lib/scrollbackWorker.ts
  - src/remote/lib/sidebarProvider.ts
  - src/remote/lib/SidebarTeamRoster.svelte
  - src/remote/lib/teamRosterScope.ts
  - src/remote/lib/TerminalCanvas.svelte
  - src/remote/lib/theme.ts
  - src/remote/lib/treeState.svelte.ts
  - src/remote/lib/VirtualKeyboard.svelte
  - src/remote/lib/WorkspaceTree.svelte
public_interface:
  - "export async function confirmedWorkspaceTarget( switchWorkspace:
    (workspaceId: string)"
  - "export async function runRemoteGitAction( options: RemoteGitActionOptions,
    ): Promise<RemoteGitActionResult>"
  - "export async function writeClipboard(text: string): Promise<boolean>"
  - export class CloudRemoteConnection
  - export class MobileRemoteUiState
  - export class PaneFeedScheduler
  - export class PaneSwitchBuffer
  - export class ScrollbackDecoder
  - "export const remoteSidebarQueryPrefix = (sessionId: number, scope?:
    RemoteSidebarScope) =>"
  - "export function anyMod(): boolean"
  - "export function applyTheme(theme: Record<string, string>)"
  - "export function applyThemeVars(colors: Record<string, string>): void"
  - "export function buildKernelTheme(colors: Record<string, string>):
    Record<string, string>"
  - export function claimPaneSize()
  - export function clearMods()
  - "export function clearPendingFeed(targetPaneId: string)"
  - "export function clearRemoteNonGitRoots(): void"
  - "export function consumeMods(): Mods"
  - export function createGenerationGuard()
  - export function createTeamRosterScopeGuard()
  - "export function createWsSidebarProvider( cwd: string, dataProvider?:
    DataProvider, options: WsSidebarProviderOptions = {}, ): SidebarProvider"
  - "export function cycleMod(m: ModKey): 'off' | 'armed' | 'locked'"
  - "export function decodeScrollback(request: ScrollbackWorkerRequest):
    ScrollbackWorkerDecoded | null"
  - "export function detachPaneRefs( refs: readonly PaneRef[], detach: (paneKey:
    string)"
  - "export function feedPane(targetPaneId: string, bytes: Uint8Array)"
  - "export function feedUtf8(bytes: Uint8Array)"
  - "export function fetchRemoteAgentHistory( link: RemoteLink, queryClient:
    RemoteQueryClientLike | undefined, sessionId: number, limit = 24, signal?:
    AbortSignal, offset = 0, query = '', ): Promise<AgentHistoryReply[]>"
  - "export function fetchRemoteTeamRoster( link: RemoteLink, queryClient:
    RemoteQueryClientLike | undefined, sessionId: number, workspaceId?: string,
    signal?: AbortSignal, ): Promise<RemoteTeamRosterSnapshot>"
  - export function fitPaneNow()
  - export function getDims()
  - "export function handleVirtualKey(key: string, ctrlKey: boolean, alt:
    boolean, shift: boolean)"
  - "export function hasRemoteGitWriteCapability(provider: SidebarProvider):
    boolean"
  - "export function installScrollbackWorker(scope: WorkerScope): void"
  - "export function isWsExpanded(id: string): boolean"
  - "export function normalizeRemotePath(value: string): string"
  - "export function normalizeTeamRosterWorkspaceId(workspaceId: string): string
    | undefined"
  - "export function onceCleanup(stops: readonly (()"
  - export function openSystemKeyboard()
  - export function pasteClipboard()
  - "export function pasteText(text: string)"
  - "export function peekMods(): Mods"
  - "export function prependScrollback(bytes: Uint8Array)"
  - "export function prependScrollbackForPane(targetPaneId: string, bytes:
    Uint8Array)"
  - "export function pruneExpanded(liveIds: Set<string>): void"
  - "export function remoteSessionId(link: RemoteLink): number"
  - "export function requestPaneSnapshot( link: RemoteLink, workspaceId: string,
    signal?: AbortSignal, timeoutMs = REMOTE_QUERY_TIMEOUT_MS, ):
    Promise<PaneInfo[]>"
  - "export function requestWorkspaceSnapshot( link: RemoteLink, signal?:
    AbortSignal, timeoutMs = REMOTE_QUERY_TIMEOUT_MS, ):
    Promise<WorkspaceInfo[]>"
  - "export function resizeKernel(rows: number, cols: number)"
  - "export function resolveInputAnchor( cursor: InputAnchorPoint | null,
    bounds: InputAnchorBounds, ): InputAnchorPoint"
  - "export function seedActiveWorkspace(id: string): void"
  - "export function setWsExpanded(id: string, expanded: boolean): void"
  - "export function stabilizeTerminalVisualShiftPx( targetPx: number,
    previousPx: number | undefined, { hysteresisPx = 0, maxStepPx =
    Number.POSITIVE_INFINITY }: VisualShiftStabilizationOptions = {}, ): number"
  - "export function teamRosterScopeKey( sessionId: number, workspaceId: string,
    panes: readonly Pick<PaneInfo, 'id' | 'cwd'>[], ): string"
  - "export function terminalVisualShiftPx({ layoutHeightPx, visualHeightPx,
    visualOffsetTopPx, stageTopPx, cursorYPx, cellHeightPx, contextRows = 3,
    inputTopPx, inputBottomPx, keyboardTopPx, safeGapPx, previousShiftPx,
    hysteresisPx = 0, maxStepPx = Number.POSITIVE_INFINITY, maxShiftPx =
    Number.POSITIVE_INFINITY, }: TerminalVisualShiftInput): number"
  - "export function themeChromeColor(colors: Record<string, string>): string |
    null"
  - "export function toggleWsExpanded(id: string): boolean"
  - export interface DrainedPaneFrames
  - export interface InputAnchorBounds
  - export interface InputAnchorPoint
  - export interface Mods
  - export interface PaneFeedDelivery
  - export interface PaneFeedSchedulerOptions
  - export interface RemoteGitActionOptions
  - export interface RemoteQueryClientLike
  - export interface RemoteSidebarScope
  - export interface RemoteTeamRosterSnapshot
  - export interface TerminalVisualShiftInput
  - export interface VisualShiftStabilizationOptions
  - export interface WsSidebarProviderOptions
  - export type ModKey
  - export type PaneFeedFn
  - export type RemoteCapabilities
  - export type RemoteGitAction
  - export type RemoteGitActionResult
  - export type RemoteViewer
  - export type ScrollbackWorkerDecoded
  - export type ScrollbackWorkerRequest
  - export type ScrollbackWorkerResult
---

# src/remote/lib module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
