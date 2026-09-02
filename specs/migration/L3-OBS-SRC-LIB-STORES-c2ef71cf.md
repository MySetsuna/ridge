---
id: L3-OBS-SRC-LIB-STORES-c2ef71cf
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/stores module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/stores/contextMenu.ts
  - src/lib/stores/dockRegionPicker.ts
  - src/lib/stores/editorWindow.ts
  - src/lib/stores/explorerLayout.ts
  - src/lib/stores/fileEditor.ts
  - src/lib/stores/fileExplorer.ts
  - src/lib/stores/fileWatcherSync.ts
  - src/lib/stores/fsEvents.ts
  - src/lib/stores/gitGuardStats.ts
  - src/lib/stores/hostReconnect.ts
  - src/lib/stores/hosts.ts
  - src/lib/stores/imagePreviewVersion.ts
  - src/lib/stores/paneGitStatus.ts
  - src/lib/stores/paneTree.ts
  - src/lib/stores/processGuardPolicy.ts
  - src/lib/stores/project.ts
  - src/lib/stores/remoteStatus.ts
  - src/lib/stores/scmCache.ts
  - src/lib/stores/searchState.ts
  - src/lib/stores/settings.ts
  - src/lib/stores/sidebarPlugins.ts
  - src/lib/stores/terminalHistory.ts
  - src/lib/stores/termSettings.ts
  - src/lib/stores/themes.ts
  - src/lib/stores/toast.ts
public_interface:
  - "export async function acceptSharedWorkspace(grantId: string): Promise<void>"
  - "export async function attachHostSession(request: HostAttachRequest):
    Promise<string | null>"
  - "export async function attachRemoteHostSession( hostId: string, sessionId:
    string, remoteWorkspaceId?: string, ): Promise<string>"
  - "export async function attachSession( socket: string, target: string,
    workspaceId?: string ): Promise<void>"
  - "export async function attachSessionAt( socket: string, target: string,
    targetPaneId: string, region: AttachRegion ): Promise<void>"
  - "export async function cancelHostReconnect(hostId: string): Promise<boolean>"
  - "export async function changeHostPaneShell( hostId: string, workspaceId:
    string, paneId: string, shellId: string, ): Promise<void>"
  - "export async function clearRecentWorkspaces(): Promise<void>"
  - "export async function closeHostPane(hostId: string, workspaceId: string,
    paneId: string): Promise<void>"
  - "export async function closeHostWorkspace(hostId: string, workspaceId:
    string): Promise<void>"
  - "export async function closePane(paneId: string)"
  - "export async function closeWorkspace(workspaceId: string)"
  - "export async function connectHost( kind: 'remote' | 'rdg', label: string,
    addr: string, token?: string, channel: 'lan' | 'public' = 'lan', ):
    Promise<void>"
  - "export async function createHostPane(hostId: string, workspaceId: string):
    Promise<void>"
  - "export async function createHostWorkspace(hostId: string, name?: string):
    Promise<void>"
  - export async function createWorkspace()
  - "export async function deleteCustomTheme(id: string): Promise<void>"
  - "export async function deleteSavedWorkspace(id: string)"
  - "export async function deleteSavedWorkspaceFile(path: string): Promise<void>"
  - "export async function deleteWorkspaceFile(workspaceId: string):
    Promise<void>"
  - "export async function detachRemoteHostSession(paneId: string):
    Promise<void>"
  - "export async function disconnectHost(hostId: string): Promise<void>"
  - "export async function dockPane( sourcePaneId: string, targetPaneId: string,
    region: DockRegion )"
  - "export async function fetchForeignHistoryTail( hostId: string, sessionId:
    string, ): Promise<HistoryTailSnapshot | null>"
  - "export async function fetchLiveBackpressure( hostId: string, ):
    Promise<LiveBackpressureDto | null>"
  - "export async function fetchOutboundStats(hostId: string):
    Promise<OutboundStats | null>"
  - "export async function filenameSearch(pattern: string): Promise<string[]>"
  - "export async function forgetHost(hostId: string): Promise<void>"
  - "export async function getDefaultWorkspaceSaveDir(): Promise<string>"
  - "export async function getLastOpenedWorkspacePath(): Promise<string | null>"
  - "export async function getRestoreSet(): Promise<string[]>"
  - "export async function getStartupContext(): Promise<StartupContext | null>"
  - "export async function hostShellChoices(hostId: string): Promise<Array<"
  - "export async function initEditorWindowHost(): Promise<UnlistenFn | null>"
  - "export async function initThemeSystem(): Promise<void>"
  - "export async function invalidatePaneGitStatusForRepo(repoRoot: string):
    Promise<void>"
  - "export async function listRecentWorkspaces(): Promise<string[]>"
  - "export async function listSavedWorkspaceFiles():
    Promise<SavedWorkspaceEntry[]>"
  - export async function loadSavedWorkspaces()
  - "export async function markHostPaneAgent( hostId: string, workspaceId:
    string, paneId: string, on: boolean, ): Promise<void>"
  - "export async function newHeadlessSession(name?: string, cwd?: string):
    Promise<string>"
  - "export async function openHostWorkspace(hostId: string, workspaceId:
    string): Promise<void>"
  - "export async function openSharedWorkspace(session: HostSession):
    Promise<void>"
  - "export async function openWorkspaceFromFile(path: string): Promise<string>"
  - "export async function persistSplitRatios(splitPath: number[], sizes:
    number[])"
  - "export async function persistSplitRatiosBatch(updates: SplitRatioUpdate[])"
  - "export async function popOutEditor(attempt = 1): Promise<void>"
  - "export async function pruneStaleExpandedPaths(): Promise<void>"
  - "export async function pumpAllConnectedOutbound(): Promise<number>"
  - "export async function pumpHostOutput(hostId: string): Promise<number>"
  - "export async function readFile(path: string): Promise<string>"
  - "export async function refreshColumnsCovering(dirPath: string):
    Promise<void>"
  - "export async function refreshGitGuardStats(): Promise<GitGuardStats | null>"
  - "export async function refreshHostTopology( hostId: string, options?: {
    supersede?: boolean; onProgress?: HostForestProgressListener }, ):
    Promise<HostForestResult | null>"
  - "export async function refreshHosts(): Promise<void>"
  - "export async function refreshRemoteRunning(): Promise<boolean>"
  - "export async function refreshThemes(): Promise<void>"
  - "export async function refreshWorkspaceSaveInfo(): Promise<void>"
  - "export async function refreshWorkspaces(options: RefreshWorkspacesOptions =
    {})"
  - "export async function renameHostWorkspace( hostId: string, workspaceId:
    string, name: string, ): Promise<void>"
  - "export async function renameSavedWorkspace(id: string, name: string)"
  - "export async function renameWorkspace(workspaceId: string, name: string)"
  - "export async function reorderWorkspaces(fromIndex: number, toIndex: number)"
  - "export async function replaceInFiles( search: string, replace: string,
    files: string[], options: { caseSensitive?: boolean; useRegex?: boolean; } =
    {} ): Promise<ReplaceStats>"
  - "export async function resolveThemeBgUrl(t: ThemeEntry | undefined):
    Promise<string | null>"
  - "export async function retryHostTopology(hostId: string):
    Promise<HostForestResult | null>"
  - "export async function revokeSharedWorkspace(grantId: string): Promise<void>"
  - "export async function runReconnectLoop( hostId: string, opts: { maxSteps:
    number; isReachable: (step: number)"
  - export async function saveCurrentWorkspace()
  - "export async function saveCustomTheme(entry: ThemeEntry):
    Promise<ThemeEntry>"
  - "export async function saveHostWorkspace( hostId: string, workspaceId:
    string, name: string, ): Promise<void>"
  - "export async function saveThemeBgImage(bytes: Uint8Array, ext: string):
    Promise<string>"
  - "export async function saveThemeBgImageFromPath(path: string):
    Promise<string>"
  - "export async function saveWorkspaceToFile( workspaceId: string, name:
    string, path?: string ): Promise<string>"
  - "export async function setActiveBgImage(themeId: string): Promise<void>"
  - "export async function setPaneSelectedRepo(paneId: string, repoRoot:
    string): Promise<void>"
  - "export async function setupPaneCwdListeners( workspaceId: string,
    treeOverride?: PaneNode ): Promise<void>"
  - "export async function splitActivePane(direction: 'horizontal' | 'vertical')"
  - "export async function splitPane( paneId: string, direction: 'horizontal' |
    'vertical' )"
  - "export async function stepHostReconnect( hostId: string, hostReachable:
    boolean, ): Promise<HostReconnectStatus>"
  - "export async function switchWorkspace(workspaceId: string):
    Promise<boolean>"
  - export async function syncPaneLayoutFromBackend()
  - "export async function terminateSession(socket: string, target: string):
    Promise<void>"
  - "export async function textSearch( query: string, options: { caseSensitive?:
    boolean; useRegex?: boolean; wholeWord?: boolean; maxResults?: number; } =
    {} ): Promise<SearchResult[]>"
  - "export async function toggleEditor(paneId: string, filePath?: string)"
  - export class ScmNonGitRepositoryError
  - "export function allRequiredTreeKillSitesCovered(sites: SpawnSitePolicy[] =
    KNOWN_SPAWN_SITES): boolean"
  - "export function applyTheme(themeId: string): void"
  - "export function attachSeedPlanForSession( hostId: string, sessionId:
    string, reattach: boolean, ): ReturnType<typeof planAttachSeed>"
  - "export function beginDesktopKernelReattachGate(): () => void"
  - "export function buildPxAnchorPlans( root: PaneNode, primary: SplitterRef,
    primaryBasisPx: number ): PxAnchorPlan[]"
  - "export function cancelHostTopologyRetry(hostId: string): void"
  - "export function cancelRefreshCopy(stats: ProcessGuardView): string"
  - "export function clampBodyHeight( desired: number, opts: { columnInnerH:
    number; minBody?: number; /** 分隔条以下至少保留（后续区块 / lower 内容） */ minLower?:
    number; minBelow?: number; sepH?: number; }, ): number"
  - "export function clampGitConcurrency(n: number): number"
  - "export function clampRectToViewport(rect: FloatingRect): FloatingRect"
  - "export function clearAgentPaneAttention(workspaceId: string, paneId:
    string): void"
  - export function clearJunctionRegistry()
  - "export function clearScmGraphInfo(repoRoot: string): void"
  - "export function clearScmQuerySingleFlights(): void"
  - "export function clearScmRepoStatus(repoRoot: string): void"
  - "export function clearSearch(): void"
  - "export function clearSearchFolder(): void"
  - export function clearSplitResizeUi()
  - "export function collapseCwd(cwd: string): string"
  - "export function collectAttachedRemotePanes( sessions: readonly
    Pick<HostSession, 'workspaceId' | 'remoteSessionId' | 'attached'>[], ):
    AttachedRemotePaneIndex"
  - "export function computeBodyHeightFromDrag( startH: number, startY: number,
    clientY: number, columnInnerH: number, opts?: { minBody?: number; minLower?:
    number; minBelow?: number; sepH?: number }, ): number"
  - "export function constrainWallpaperSize(width: number, height: number):"
  - "export function dedupKeepFirst(items: readonly string[]): string[]"
  - "export function extractCwdsFromLayout( node: PaneNode, workspaceId: string
    ): Record<string, string>"
  - "export function filterByPrefix(items: readonly string[], query: string):
    string[]"
  - "export function findJunctionsNearPosition( axis: SplitterAxis, positionPx:
    number, threshold: number = SNAP_THRESHOLD_PX ): JunctionRef[]"
  - "export function findSameAxisRefs( primary: SplitterRef, threshold: number =
    SNAP_THRESHOLD_PX ): SameAxisCandidate[]"
  - "export function finishSplitResizeDrag(): SplitRatioUpdate[]"
  - "export function flattenVisiblePaths(column: ExplorerColumn): string[]"
  - "export function focusPane(paneId: string, workspaceId?: string): void"
  - "export function forgetWorkspaceTree(wsId: string): void"
  - "export function getAllPaneIds(node: PaneNode): string[]"
  - "export function getPaneCwd(workspaceId: string, paneId: string): string |
    undefined"
  - "export function getScmCache(): ScmCacheState"
  - "export function getScmQueryDiagnostics(): ScmQueryDiagnostics"
  - "export function getScmSelectedCommit(repoRoot: string): string"
  - "export function getScmSelectedRepo(): string"
  - "export function getSplitterLineEndpoints( ref: SplitterRef ):"
  - "export function getSplitterScreenCenter(ref: SplitterRef): number | null"
  - "export function getTheme(id: string): ThemeEntry | undefined"
  - "export function getThemeIds(): string[]"
  - "export function getThemeLabels(): Record<string, string>"
  - "export function gitGuardNeedsAttention(s: GitGuardStats | null): boolean"
  - "export function hasHostTopologyLink(hostId: string): boolean"
  - "export function historyBadgeForSession(hostId: string, sessionId: string):
    string"
  - "export function hostOperatorAlert( hostId: string, reconnectAttempt:
    number, ): string | null"
  - "export function hostShareDeviceName(hostId: string): string | undefined"
  - "export function imageUrlWithVersion(url: string, version: number |
    undefined): string"
  - "export function initFileExplorer( workspaces: WorkspaceDescriptor[],
    allPaneTitles: Record<string, string> = {} ): void"
  - "export function initFileWatcherSync(): void"
  - "export function initSettingsBoot(): void"
  - "export function invalidateScmQuery( kind: ScmQueryKind | 'all', repoRoot?:
    string, ): number"
  - "export function isCustomTheme(id: string): boolean"
  - "export function isNotGitRepositoryError(error: unknown): boolean"
  - "export function isRecentlyWritten(path: string): boolean"
  - "export function isRemotePaneAttached( index: AttachedRemotePaneIndex,
    workspaceId: string, paneId: string, paneOccurrences: number, ): boolean"
  - "export function isResizeInProgress(): boolean"
  - "export function isScmRepoKnownNonGit(repoRoot: string): boolean"
  - "export function isolationBadge(hostId: string): string"
  - "export function lanConnectionError( host: string, port: number, secure:
    boolean, detail?: string, category?: 'user' | 'parked' | 'channel', ):
    string"
  - "export function lanTrustUrl(host: string, port: number): string"
  - "export function langFromPath(path: string): string"
  - "export function lowerRegionHeight( columnInnerH: number, bodyH: number,
    sepH: number = BODY_SEP_H, ): number"
  - "export function markPaneGitRepoNonGit(repoRoot: string): void"
  - "export function markRecentlyWritten(path: string): void"
  - "export function markScmRepoNonGit(repoRoot: string): void"
  - "export function mergePeak(prev: number, next: number): number"
  - "export function nextHistorySelection(current: number, total: number, delta:
    number): number"
  - "export function nextImageVersion(current: number | undefined): number"
  - "export function noteLifecycleDetach(hostId: string, sessionId: string):
    void"
  - "export function noteLifecycleFanout(hostId: string, bytes: number): void"
  - "export function noteLifecycleSubscribe(hostId: string, sessionId: string):
    void"
  - "export function noteOutboundReconnectAttempt(hostId: string): number"
  - "export function notePumpBatch(hostId: string, byteLength: number, capHint =
    256 * 1024): void"
  - "export function onFsChange(handler: FsChangeHandler): () => void"
  - "export function paneIdsFromRatioUpdates( root: PaneNode, updates:
    SplitRatioUpdate[] ): string[]"
  - "export function parentDirectory(path: string): string"
  - "export function parsePhaseMessage(msg: string):"
  - "export function persistExplorerBodyHeights(): void"
  - "export function pickDockRegion(targetPaneId: string): Promise<AttachRegion
    | null>"
  - "export function pointerInCoupleZone( primary: SplitterRef, sibling:
    SplitterRef, pointer: { x: number; y: number } ): boolean"
  - "export function pressureFromStats(s: GitGuardStats | null):
    ProcessGuardView"
  - "export function pumpBadgeForHost(hostId: string): string"
  - "export function pxAnchorRatios( plan: PxAnchorPlan, signedDeltaPx: number
    ): number[]"
  - "export function queueSplitResizeJunction( primary: SplitterRef,
    orthogonals: SplitterRef[], pointer: { x: number; y: number },
    sameAxisCandidates: SplitterRef[] = [], snapState: JunctionSnapState | null
    = null )"
  - "export function reclampStoredBodyHeight( storedH: number, liveColumnInnerH:
    number, opts?: { minBody?: number; minBelow?: number; sepH?: number }, ):
    number | null"
  - "export function registerHostTopologyLink(source: RegisteredHostLink): () =>
    void"
  - "export function registerJunction(junction: JunctionRef)"
  - "export function registerSidebarPlugin(plugin: SidebarPlugin): void"
  - "export function remainingCutClipboard( clip: ExplorerClipboard,
    failedPaths: Iterable<string>, ): ExplorerClipboard | null"
  - "export function remotePaneKey(workspaceId: string, paneId: string): string"
  - "export function resetOutboundReconnectAttempt(hostId: string): void"
  - "export function resetScmRepositoryDetection(cwdContext?: string): string[]"
  - "export function resolveActiveClipboard( internal: ExplorerClipboard | null,
    currentSeq: number, systemFiles: string[] ): ExplorerClipboard | null"
  - "export function resolveDockRegion(region: AttachRegion | null): void"
  - "export function resolveExplorerStackLayout(input: { bodyHeightPx: number |
    null | undefined; hasLowerContent: boolean; }):"
  - "export function scheduleForceFitActivePanes(): void"
  - "export function scheduleForceFitAfterSplit(sourcePaneId: string, newPaneId:
    string): void"
  - "export function scheduleIsolationTask(hostId: string, attachedPaneIds:
    string[]): HostTask"
  - "export function searchInFolder(path: string): void"
  - "export function setAgentPaneAttention( workspaceId: string, paneId: string,
    attention: AgentPaneAttention | null ): void"
  - "export function setAgentPaneStatus( workspaceId: string, paneId: string,
    status: AgentPaneStatus | null, ): void"
  - "export function setExplorerBodyHeight(cwd: string, height: number): void"
  - "export function setExplorerClipboard(clip: ExplorerClipboard | null): void"
  - "export function setPaneCwd(workspaceId: string, paneId: string, cwd: string
    | null | undefined): void"
  - "export function setScmDirectoryContexts(ownerId: string, directories:
    readonly string[]): string[]"
  - "export function setScmGraphInfo(repoRoot: string, info: GitRepoInfo): void"
  - "export function setScmRepoRoots( repoRoots: string[], cwdSignature: string,
    repoSignature: string, directoryContexts?: readonly string[], ): void"
  - "export function setScmRepoStatus(repoRoot: string, status: ScmRepoStatus):
    void"
  - "export function setScmSelectedCommit(repoRoot: string, hash: string): void"
  - "export function setScmSelectedRepo(repoRoot: string): void"
  - "export function setTermFontSize(value: number): void"
  - "export function setTheme(themeId: string): void"
  - "export function shouldRefreshGraphOnMount(repoRoot: string, maxAgeMs =
    30_000): boolean"
  - "export function shouldRefreshOnMount(maxAgeMs = 30_000): boolean"
  - "export function shouldRefreshScmStatus( repoRoot: string, maxAgeMs =
    SCM_STATUS_POLL_INTERVAL_MS, ): boolean"
  - "export function shouldSurfaceGitGuard(s: GitGuardStats | null): boolean"
  - "export function showToast(message: string, type: ToastType = 'success'):
    void"
  - "export function sleepMsForAttempt(attempt: number): number | null"
  - "export function slugifyThemeId(label: string): string"
  - "export function startSplitResizeDrag(pointer: { x: number; y: number })"
  - "export function toggleColumnCollapsed(columnId: string): void"
  - "export function toggleWorkspaceCollapsed(workspaceId: string): void"
  - "export function trackPaneGitStatus(paneId: string, cwd: string | null):
    void"
  - "export function uniqueChildName( dirPath: string, desired: string,
    existingAbsolute: Set<string> ): string"
  - "export function unregisterSidebarPlugin(id: string): void"
  - "export function updateSplitResizeDrag(pointer: { x: number; y: number })"
  - "export function updateWorkspaceNames(workspaces: WorkspaceDescriptor[]):
    void"
  - "export function waitForDesktopKernelReattach(): Promise<void>"
  - export interface ActiveBgImage
  - export interface ActiveWallpaperGpu
  - export interface AttachedRemotePaneIndex
  - export interface CommitNode
  - export interface ContextMenuItem
  - export interface ContextMenuState
  - export interface DiffFile
  - export interface DirectoryPage
  - export interface ExplorerClipboard
  - export interface ExplorerColumn
  - export interface ExplorerWorkspaceGroup
  - export interface FileEditorState
  - export interface FileExplorerState
  - export interface FileNode
  - export interface FloatingRect
  - export interface FsChangedPayload
  - export interface GitGuardStats
  - export interface GitRepoInfo
  - export interface HandoffPayload
  - export interface Host
  - export interface HostAttachRequest
  - export interface HostConnectProgress
  - export interface HostReconnectStatus
  - export interface HostSession
  - export interface HostWorkspace
  - export interface JunctionRef
  - export interface JunctionSnapState
  - export interface JunctionSplitterRef
  - export interface LiveBackpressureDto
  - export interface LoaderConfig
  - export interface NativeSessionInfo
  - export interface OpenFile
  - export interface OutboundStats
  - export interface PaneGitInfo
  - export interface PendingReveal
  - export interface ProcessGuardView
  - export interface PxAnchorPlan
  - export interface RefreshWorkspacesOptions
  - export interface RegisteredHostLink
  - export interface ReplaceStats
  - export interface SameAxisCandidate
  - export interface SavedWorkspace
  - export interface SavedWorkspaceEntry
  - export interface ScmCacheState
  - export interface ScmFile
  - export interface ScmQueryDiagnostics
  - export interface ScmRepoStatus
  - export interface SearchHit
  - export interface SearchResult
  - export interface SidebarPlugin
  - export interface SidebarPluginProps
  - export interface SpawnSitePolicy
  - export interface SplitRatioUpdate
  - export interface SplitterRef
  - export interface StartupContext
  - export interface ThemeEntry
  - export interface ThemeFile
  - export interface ToastItem
  - export interface UserSettings
  - export interface WorkspaceDescriptor
  - export interface WorkspaceSaveInfo
  - export type AgentPaneAttention
  - export type AgentPaneStatus
  - export type AttachRegion
  - export type ContextMenuTarget
  - export type DockRegion
  - export type EditorDisplayMode
  - export type FsChangeHandler
  - export type HostKind
  - export type HostStatus
  - export type IconComponent
  - export type OpenInterceptor
  - export type OpenRequest
  - export type ReconnectPhase
  - export type ScmQueryKind
  - export type SidebarPluginScope
  - export type SplitResizeUiState
  - export type SplitterAxis
  - export type ToastType
---

# src/lib/stores module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
