---
id: L3-OBS-PACKAGES-REMOTE-SRC-SHARED-TERMINAL-b1abe747
level: L3
parent: L2-OBS-PACKAGES-b28b1ed9
title: packages/remote/src/shared/terminal module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - packages/remote/src/shared/terminal/clipboardImage.ts
  - packages/remote/src/shared/terminal/cssColor.ts
  - packages/remote/src/shared/terminal/dropPaste.ts
  - packages/remote/src/shared/terminal/fontDataService.ts
  - packages/remote/src/shared/terminal/fontStack.ts
  - packages/remote/src/shared/terminal/hostRemountPolicy.ts
  - packages/remote/src/shared/terminal/imeAnchor.ts
  - packages/remote/src/shared/terminal/imeDelta.ts
  - packages/remote/src/shared/terminal/initialPaneFit.ts
  - packages/remote/src/shared/terminal/linkAffordance.ts
  - packages/remote/src/shared/terminal/linkOpenHost.ts
  - packages/remote/src/shared/terminal/linkSpans.ts
  - packages/remote/src/shared/terminal/manager.ts
  - packages/remote/src/shared/terminal/mobileCopy.ts
  - packages/remote/src/shared/terminal/mobileTouchScroll.ts
  - packages/remote/src/shared/terminal/mouseForwardPolicy.ts
  - packages/remote/src/shared/terminal/paneDockResolve.ts
  - packages/remote/src/shared/terminal/paneGeometry.ts
  - packages/remote/src/shared/terminal/paneInputGate.ts
  - packages/remote/src/shared/terminal/paneOrigin.ts
  - packages/remote/src/shared/terminal/paneShell.ts
  - packages/remote/src/shared/terminal/perfTrace.ts
  - packages/remote/src/shared/terminal/ports.ts
  - packages/remote/src/shared/terminal/ptyBridge.ts
  - packages/remote/src/shared/terminal/renderTransaction.ts
  - packages/remote/src/shared/terminal/sentenceBuffer.ts
  - packages/remote/src/shared/terminal/shellInputSnapshot.ts
  - packages/remote/src/shared/terminal/terminalFeedPolicy.ts
  - packages/remote/src/shared/terminal/terminalFocus.ts
  - packages/remote/src/shared/terminal/terminalMemoryPolicy.ts
  - packages/remote/src/shared/terminal/themeBridge.ts
  - packages/remote/src/shared/terminal/tuiGate.ts
  - packages/remote/src/shared/terminal/types.ts
public_interface:
  - "export async function acquireClipboardImagePath(): Promise<string | null>"
  - "export async function changePaneShell(paneId: string, shell: ShellInfo):
    Promise<void>"
  - "export async function enableDeltaModeThenFit( paneId: string, fit: ()"
  - "export async function getShells(): Promise<ShellInfo[]>"
  - "export async function imagePathFromClipboardEvent(e: ClipboardEvent):
    Promise<string | null>"
  - "export async function loadHostTerminalFonts( stack: string, install:
    FontDataInstaller, invokeCommand: InvokeFn = invoke, ): Promise<number>"
  - "export async function probePathWithCache( key: string, inspect: (signal:
    AbortSignal)"
  - "export async function setPaneDeltaMode( paneId: string, enabled: boolean,
    workspaceId?: string, ): Promise<boolean>"
  - export class LinkSpanIndex
  - export class PaneInputGateFullError
  - export class PaneInputGateRetiredError
  - export class SentenceBuffer
  - export class TerminalManager
  - "export function activateIme(ops: { scrollToBottom(): void;"
  - "export function attachDirectionAt( clientX: number, clientY: number, el: {
    getBoundingClientRect(): DOMRect }"
  - "export function buildOpenPlanFromHit(opts: { text: string; kind:
    LinkSpanKind | 'osc8'; paneCwd?: string | null; workspaceRoot?: string |
    null; }): HostOpenAction"
  - "export function cellDeviceOffsetPx(cellIndex: number, cellSizeCss: number,
    dpr: number): number"
  - "export function cellFromClientPoint( geometry: PaneGeometry, clientX:
    number, clientY: number, rows = geometry.rows, cols = geometry.cols, ):"
  - "export function cellFromVisualClientPoint( geometry: PaneGeometry, clientX:
    number, clientY: number, visualOffsetY = 0, geometryVisualOffsetY = 0, rows
    = geometry.rows, cols = geometry.cols, ):"
  - "export function clearPaneShellSelection(paneId: string): void"
  - "export function clearPathProbeCache(): void"
  - "export function computePaneGeometry(input: PaneGeometryInput): PaneGeometry
    | null"
  - "export function copySelectionOnly( text: string, fx: CopySideEffects, ):
    boolean"
  - "export function decideHoverUnderline(opts: { hasLinkHit: boolean;
    modifierHeld: boolean; isMac?: boolean; spanText?: string | null; }):
    HoverUnderlineDecision"
  - "export function decideLinkClick(opts: { mouseReportingOn: boolean;
    modifierHeld: boolean; hasLinkHit: boolean; primaryButton: boolean; }):
    LinkClickDecision"
  - "export function decideTouchMouseGesture( phase: 'press' | 'drag' |
    'release', ):"
  - "export function decideTouchScroll(input: { deltaY: number;
    isMouseReporting: boolean; isAltScreen: boolean; /** When true, treat deltaY
    as pixel-like (touch accum)"
  - "export function decodeBase64Font(data: string): Uint8Array"
  - "export function decodeUnderlineDataset( value: string | undefined, ):"
  - "export function dropPendingFeedBuffers( entry: PendingFeedBuffers,
    cancelTimer: (timer: ReturnType<typeof setTimeout>)"
  - "export function encodeUnderlineDataset( row: number, c0: number | 'osc8',
    c1?: number, ): string"
  - "export function enqueueDeferredFeed( entry: PendingFeedBuffers, bytes:
    Uint8Array, ): DeferredFeedResult"
  - "export function enqueuePaneInput( key: string, operation: ()"
  - "export function ensurePtyBridge(paneId: string, workspaceId: string):
    Promise<void>"
  - "export function fileUrlToPath(href: string): string | null"
  - "export function focusActiveTerminal(): boolean"
  - "export function focusTerminalPane(paneId: string): boolean"
  - "export function formatDroppedPathsForPaste(paths: string[]): string"
  - "export function hasDeferredFeed(entry: PendingFeedBuffers): boolean"
  - "export function hasLiveTuiSignal( s: Pick<TuiSnapshot, 'isAltScreen' |
    'isInlineTuiActive' | 'isMouseReporting' | 'isAppCursorKeys' |
    'cursorVisible'>, ): boolean"
  - "export function hasPtyBridge(paneId: string, workspaceId?: string): boolean"
  - "export function hex8(input: string): string | null"
  - "export function hex8WithAlpha(input: string, alpha: number): string | null"
  - "export function imeCommitDelta(recentTyped: string, commit: string): string"
  - "export function imeHelperCssPosition(input: ImeAnchorInput):"
  - "export function isBrowserHeapUnderPressure(memory?: BrowserHeapSnapshot |
    null): boolean"
  - "export function isForeignOrigin(origin: PaneOrigin | undefined): origin is
    PaneOrigin"
  - "export function isPathSpanKind( kind: LinkSpanKind | 'osc8' | null |
    undefined, ): boolean"
  - "export function isProbablyDirectory(path: string): boolean"
  - "export function isSafeHttpUrl(href: string): boolean"
  - "export function isTuiActive(s: TuiSnapshot): boolean"
  - "export function loadTerminalFonts(stack: string, install:
    FontDataInstaller): Promise<number>"
  - "export function looksOutsideWorkspace(path: string, root: string): boolean"
  - "export function needsInitialPaneFit(measurement: InitialFitMeasurement):
    boolean"
  - "export function osc8UnderlineRegions( grid: Osc8LinkGrid, row: number, col:
    number, uri: string | null, ):"
  - "export function ownsTabKey(el: Element | null): boolean"
  - "export function paneOriginBadge(origin: PaneOrigin): PaneOriginBadge"
  - "export function parseCssFontFamilies(stack: string): string[]"
  - "export function parsePathLineCol(text: string): ReturnType<typeof
    parsePathWithLocation>"
  - "export function parsePathWithLocation(text: string):"
  - "export function passedDragThreshold( startX: number, startY: number, x:
    number, y: number, threshold = 4 ): boolean"
  - "export function pendingPaneInputIntents(key: string): number"
  - "export function pendingWordBackspace(prev: string): string"
  - "export function pinImeCaretToAnchor(input: Pick< HTMLTextAreaElement,
    'scrollLeft' | 'scrollTop' | 'scrollWidth' | 'clientWidth' >): void"
  - "export function planFromTarget(target: LinkOpenTarget, ctx: OpenContext =
    {}): HostOpenAction"
  - "export function planHostOpen( text: string, kind: LinkSpanKind | 'osc8',
    ctx: OpenContext = {}, ): HostOpenAction"
  - "export function planTerminalMemoryReclaim(args: { candidates: readonly
    TerminalMemoryCandidate[]; rowBudget: number; heapPressure: boolean;
    documentHidden: boolean; }): TerminalMemoryPlan"
  - "export function prependDeferredFeed( entry: PendingFeedBuffers, bytes:
    Uint8Array, ): DeferredFeedResult"
  - "export function pushTerminalThemeNow(): void"
  - "export function reconstructInputSnapshot( preCursorCells: readonly
    CellLike[], postCursorCells: readonly CellLike[], ): ShellInputSnapshot"
  - "export function regionAtPoint( clientX: number, clientY: number, el: {
    getBoundingClientRect(): DOMRect }"
  - "export function resolveDockTarget( el: Element | null, sourcePaneId:
    string, clientX: number, clientY: number ):"
  - "export function resolveOpenTarget( text: string, kind: LinkSpanKind |
    'osc8', ): LinkOpenTarget"
  - "export function resolvePathAgainstCwd( path: string, paneCwd: string | null
    | undefined, workspaceRoot: string | null | undefined, ): string"
  - "export function retirePaneInput(key: string): void"
  - "export function retirePaneInputsForPane(paneId: string): void"
  - "export function scissorOriginDevicePx( input: Pick< ImeAnchorInput,
    'containerLeft' | 'containerTop' | 'hostLeft' | 'hostTop' | 'padL' | 'padT'
    | 'dpr' >, ):"
  - "export function setupTerminalThemeBridge(): () => void"
  - "export function sgrReleaseButton(button: number, lastButtons = 0): number"
  - "export function shouldDrainDeferredFeed(length: number): boolean"
  - "export function shouldFlushFeedBuffer(length: number): boolean"
  - "export function shouldForwardPointerMotion(modes: number, buttons: number):
    boolean"
  - "export function shouldWipeHostOnPaneRemount(retainRenderer: boolean):
    boolean"
  - "export function snapshotLiveSignals( isAltScreen: boolean,
    isInlineTuiActive: boolean, isMouseReporting: boolean, isAppCursorKeys:
    boolean, ): TuiSnapshot"
  - "export function takeDeferredFeed(entry: PendingFeedBuffers): Uint8Array |
    null"
  - "export function teardownPtyBridge(paneId: string, workspaceId?: string):
    void"
  - "export function terminalScrollbackBudgetRows(deviceMemoryGb?: number):
    number"
  - "export function trailingWord(recentTyped: string): string"
  - "export function trimTrailingPunct(s: string): string"
  - "export function trimTrailingSeparators(s: string): string"
  - "export function tryEnqueuePaneInput( key: string, operation: ()"
  - "export function tryEnqueuePaneInputImmediate( key: string, operation: ()"
  - "export function underlineCssTokens(opts: { show: boolean; kind:
    LinkSpanKind | 'osc8' | null; }): string[]"
  - "export function underlineRegionsFromSpan(span: Pick<LinkSpan, 'row' | 'c0'
    | 'c1'>):"
  - "export function updatePendingWord(prev: string, sentText: string): string"
  - "export function withEmojiFallback(family: string): string"
  - export interface ActiveWallpaperGpu
  - export interface BrowserHeapSnapshot
  - export interface CellLike
  - export interface CwdPort
  - export interface DeferredFeedResult
  - export interface HostPorts
  - export interface HoverUnderlineDecision
  - export interface ImeAnchorInput
  - export interface InitialFitMeasurement
  - export interface InputBufferState
  - export interface LinkClickDecision
  - export interface LinkSpan
  - export interface ManagerOptions
  - export interface OpenContext
  - export interface Osc8LinkGrid
  - export interface PaneGeometry
  - export interface PaneGeometryInput
  - export interface PaneOriginBadge
  - export interface PanePadding
  - export interface PathProbeResult
  - export interface PendingFeedBuffers
  - export interface RectLike
  - export interface SettingsPort
  - export interface ShellInfo
  - export interface ShellInputSnapshot
  - export interface TermSettingsPort
  - export interface TerminalLinkOpenRequest
  - export interface TerminalLinkOpenResult
  - export interface TerminalMemoryCandidate
  - export interface TerminalMemoryPlan
  - export interface TerminalPathOrigin
  - export interface TerminalSettingsSnapshot
  - export interface ThemesPort
  - export interface TuiSnapshot
  - export interface WorkspacePort
  - export type CopySideEffects
  - export type DockRegion
  - export type FontDataInstaller
  - export type HostOpenAction
  - export type KernelEvent
  - export type LinkOpenTarget
  - export type LinkSpanKind
  - export type PaneOrigin
  - export type TouchScrollDecision
---

# packages/remote/src/shared/terminal module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
