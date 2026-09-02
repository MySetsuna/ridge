---
id: L4-OBS-SRC-LIB-STORES-PANETREE-TS-deea5908
level: L4
parent: L3-OBS-SRC-LIB-STORES-c2ef71cf
title: paneTree.ts
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/stores/paneTree.ts
test_targets:
  - src/lib/stores/clipboardResolve.test.ts
  - src/lib/stores/contextMenu.test.ts
  - src/lib/stores/dockRegionPicker.test.ts
  - src/lib/stores/editorWindow.test.ts
  - src/lib/stores/explorerLayout.test.ts
  - src/lib/stores/fileEditor.test.ts
  - src/lib/stores/fileExplorer.test.ts
  - src/lib/stores/fileWatcherSync.test.ts
  - src/lib/stores/fsEvents.test.ts
  - src/lib/stores/gitGuardStats.test.ts
  - src/lib/stores/hostReconnect.test.ts
  - src/lib/stores/hostReconnectProduct.test.ts
  - src/lib/stores/hosts.connect.test.ts
  - src/lib/stores/hosts.refresh.test.ts
  - src/lib/stores/hostsMutation.test.ts
  - src/lib/stores/hostsOutbound.test.ts
  - src/lib/stores/hostsOutboundProduct.test.ts
  - src/lib/stores/hostsPublic.test.ts
  - src/lib/stores/imagePreviewVersion.test.ts
  - src/lib/stores/paneGitStatus.test.ts
  - src/lib/stores/paneTree.coverage.test.ts
  - src/lib/stores/paneTree.test.ts
  - src/lib/stores/processGuardPolicy.test.ts
  - src/lib/stores/project.test.ts
  - src/lib/stores/remoteStatus.test.ts
  - src/lib/stores/scmCache.test.ts
  - src/lib/stores/searchState.test.ts
  - src/lib/stores/settings.test.ts
  - src/lib/stores/terminalHistory.test.ts
  - src/lib/stores/termSettings.test.ts
  - src/lib/stores/themes.slug.test.ts
  - src/lib/stores/themes.test.ts
public_interface:
  - "export async function clearRecentWorkspaces(): Promise<void>"
  - "export async function closePane(paneId: string)"
  - "export async function closeWorkspace(workspaceId: string)"
  - export async function createWorkspace()
  - "export async function deleteSavedWorkspace(id: string)"
  - "export async function deleteSavedWorkspaceFile(path: string): Promise<void>"
  - "export async function deleteWorkspaceFile(workspaceId: string):
    Promise<void>"
  - "export async function dockPane( sourcePaneId: string, targetPaneId: string,
    region: DockRegion )"
  - "export async function getDefaultWorkspaceSaveDir(): Promise<string>"
  - "export async function getLastOpenedWorkspacePath(): Promise<string | null>"
  - "export async function getRestoreSet(): Promise<string[]>"
  - "export async function getStartupContext(): Promise<StartupContext | null>"
  - "export async function listRecentWorkspaces(): Promise<string[]>"
  - "export async function listSavedWorkspaceFiles():
    Promise<SavedWorkspaceEntry[]>"
  - export async function loadSavedWorkspaces()
  - "export async function openWorkspaceFromFile(path: string): Promise<string>"
  - "export async function persistSplitRatios(splitPath: number[], sizes:
    number[])"
  - "export async function persistSplitRatiosBatch(updates: SplitRatioUpdate[])"
  - "export async function refreshWorkspaceSaveInfo(): Promise<void>"
  - "export async function refreshWorkspaces(options: RefreshWorkspacesOptions =
    {})"
  - "export async function renameSavedWorkspace(id: string, name: string)"
  - "export async function renameWorkspace(workspaceId: string, name: string)"
  - "export async function reorderWorkspaces(fromIndex: number, toIndex: number)"
  - export async function saveCurrentWorkspace()
  - "export async function saveWorkspaceToFile( workspaceId: string, name:
    string, path?: string ): Promise<string>"
  - "export async function setupPaneCwdListeners( workspaceId: string,
    treeOverride?: PaneNode ): Promise<void>"
  - "export async function splitActivePane(direction: 'horizontal' | 'vertical')"
  - "export async function splitPane( paneId: string, direction: 'horizontal' |
    'vertical' )"
  - "export async function switchWorkspace(workspaceId: string):
    Promise<boolean>"
  - export async function syncPaneLayoutFromBackend()
  - "export async function toggleEditor(paneId: string, filePath?: string)"
  - "export function beginDesktopKernelReattachGate(): () => void"
  - "export function buildPxAnchorPlans( root: PaneNode, primary: SplitterRef,
    primaryBasisPx: number ): PxAnchorPlan[]"
  - "export function clearAgentPaneAttention(workspaceId: string, paneId:
    string): void"
  - export function clearJunctionRegistry()
  - export function clearSplitResizeUi()
  - "export function collapseCwd(cwd: string): string"
  - "export function extractCwdsFromLayout( node: PaneNode, workspaceId: string
    ): Record<string, string>"
  - "export function findJunctionsNearPosition( axis: SplitterAxis, positionPx:
    number, threshold: number = SNAP_THRESHOLD_PX ): JunctionRef[]"
  - "export function findSameAxisRefs( primary: SplitterRef, threshold: number =
    SNAP_THRESHOLD_PX ): SameAxisCandidate[]"
  - "export function finishSplitResizeDrag(): SplitRatioUpdate[]"
  - "export function focusPane(paneId: string, workspaceId?: string): void"
  - "export function forgetWorkspaceTree(wsId: string): void"
  - "export function getAllPaneIds(node: PaneNode): string[]"
  - "export function getPaneCwd(workspaceId: string, paneId: string): string |
    undefined"
  - "export function getSplitterLineEndpoints( ref: SplitterRef ):"
  - "export function getSplitterScreenCenter(ref: SplitterRef): number | null"
  - "export function paneIdsFromRatioUpdates( root: PaneNode, updates:
    SplitRatioUpdate[] ): string[]"
  - "export function pointerInCoupleZone( primary: SplitterRef, sibling:
    SplitterRef, pointer: { x: number; y: number } ): boolean"
  - "export function pxAnchorRatios( plan: PxAnchorPlan, signedDeltaPx: number
    ): number[]"
  - "export function queueSplitResizeJunction( primary: SplitterRef,
    orthogonals: SplitterRef[], pointer: { x: number; y: number },
    sameAxisCandidates: SplitterRef[] = [], snapState: JunctionSnapState | null
    = null )"
  - "export function registerJunction(junction: JunctionRef)"
  - "export function scheduleForceFitActivePanes(): void"
  - "export function scheduleForceFitAfterSplit(sourcePaneId: string, newPaneId:
    string): void"
  - "export function setAgentPaneAttention( workspaceId: string, paneId: string,
    attention: AgentPaneAttention | null ): void"
  - "export function setAgentPaneStatus( workspaceId: string, paneId: string,
    status: AgentPaneStatus | null, ): void"
  - "export function setPaneCwd(workspaceId: string, paneId: string, cwd: string
    | null | undefined): void"
  - "export function startSplitResizeDrag(pointer: { x: number; y: number })"
  - "export function updateSplitResizeDrag(pointer: { x: number; y: number })"
  - "export function waitForDesktopKernelReattach(): Promise<void>"
  - export interface JunctionRef
  - export interface JunctionSnapState
  - export interface JunctionSplitterRef
  - export interface PxAnchorPlan
  - export interface RefreshWorkspacesOptions
  - export interface SameAxisCandidate
  - export interface SavedWorkspace
  - export interface SavedWorkspaceEntry
  - export interface SplitRatioUpdate
  - export interface SplitterRef
  - export interface StartupContext
  - export interface WorkspaceSaveInfo
  - export type AgentPaneAttention
  - export type AgentPaneStatus
  - export type DockRegion
  - export type SplitResizeUiState
  - export type SplitterAxis
verified_by:
  - TEST-OBS-SRC-LIB-STORES-CLIPBOARDRESOLVE-TEST-TS-d8cab7c6
  - TEST-OBS-SRC-LIB-STORES-CONTEXTMENU-TEST-TS-c5d624d2
  - TEST-OBS-SRC-LIB-STORES-DOCKREGIONPICKER-TEST-TS-b7ce6df6
  - TEST-OBS-SRC-LIB-STORES-EDITORWINDOW-TEST-TS-965f4022
  - TEST-OBS-SRC-LIB-STORES-EXPLORERLAYOUT-TEST-TS-489203e2
  - TEST-OBS-SRC-LIB-STORES-FILEEDITOR-TEST-TS-396c0017
  - TEST-OBS-SRC-LIB-STORES-FILEEXPLORER-TEST-TS-8df44237
  - TEST-OBS-SRC-LIB-STORES-FILEWATCHERSYNC-TEST-TS-9e135a5d
  - TEST-OBS-SRC-LIB-STORES-FSEVENTS-TEST-TS-f628ded6
  - TEST-OBS-SRC-LIB-STORES-GITGUARDSTATS-TEST-TS-680ef77e
  - TEST-OBS-SRC-LIB-STORES-HOSTRECONNECT-TEST-TS-e82e61be
  - TEST-OBS-SRC-LIB-STORES-HOSTRECONNECTPRODUCT-TEST-TS-9598a61c
  - TEST-OBS-SRC-LIB-STORES-HOSTS-CONNECT-TEST-TS-3c065b9c
  - TEST-OBS-SRC-LIB-STORES-HOSTS-REFRESH-TEST-TS-57fb3707
  - TEST-OBS-SRC-LIB-STORES-HOSTSMUTATION-TEST-TS-ef74d67d
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUND-TEST-TS-afc6c610
  - TEST-OBS-SRC-LIB-STORES-HOSTSOUTBOUNDPRODUCT-TEST-TS-c9cf71df
  - TEST-OBS-SRC-LIB-STORES-HOSTSPUBLIC-TEST-TS-b49d4adf
  - TEST-OBS-SRC-LIB-STORES-IMAGEPREVIEWVERSION-TEST-TS-c18c0477
  - TEST-OBS-SRC-LIB-STORES-PANEGITSTATUS-TEST-TS-73bc40e2
  - TEST-OBS-SRC-LIB-STORES-PANETREE-COVERAGE-TEST-TS-747ca055
  - TEST-OBS-SRC-LIB-STORES-PANETREE-TEST-TS-8fb828ce
  - TEST-OBS-SRC-LIB-STORES-PROCESSGUARDPOLICY-TEST-TS-a94798ee
  - TEST-OBS-SRC-LIB-STORES-PROJECT-TEST-TS-b97586fc
  - TEST-OBS-SRC-LIB-STORES-REMOTESTATUS-TEST-TS-a66e6e00
  - TEST-OBS-SRC-LIB-STORES-SCMCACHE-TEST-TS-69c519de
  - TEST-OBS-SRC-LIB-STORES-SEARCHSTATE-TEST-TS-7c629e13
  - TEST-OBS-SRC-LIB-STORES-SETTINGS-TEST-TS-ebb9f869
  - TEST-OBS-SRC-LIB-STORES-TERMINALHISTORY-TEST-TS-49edc8b5
  - TEST-OBS-SRC-LIB-STORES-TERMSETTINGS-TEST-TS-222ac7cd
  - TEST-OBS-SRC-LIB-STORES-THEMES-SLUG-TEST-TS-146943ed
  - TEST-OBS-SRC-LIB-STORES-THEMES-TEST-TS-8b9fb08d
---

# paneTree.ts

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
