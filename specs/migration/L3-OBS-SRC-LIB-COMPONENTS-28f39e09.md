---
id: L3-OBS-SRC-LIB-COMPONENTS-28f39e09
level: L3
parent: L2-OBS-SRC-25a66342
title: src/lib/components module
status: LOCKED
origin: observed
migration_state: CONFIRMED
confidence: INFERRED
observed_source_hash: 818be4df993b9bce2ea80f7e8c6e71eff235432927f95f50ff567a75e0538727
code_targets:
  - src/lib/components/ContextMenu.svelte
  - src/lib/components/customTheme.ts
  - src/lib/components/CustomThemeModal.svelte
  - src/lib/components/DevIssueDialog.svelte
  - src/lib/components/DiffEditorModal.svelte
  - src/lib/components/EditorWindow.svelte
  - src/lib/components/Explorer.svelte
  - src/lib/components/explorerPaste.ts
  - src/lib/components/FileEditor.svelte
  - src/lib/components/FileTree.svelte
  - src/lib/components/GitGraph.svelte
  - src/lib/components/gitGraphLayout.ts
  - src/lib/components/ImagePreviewOverlay.svelte
  - src/lib/components/inputBufferTracker.ts
  - src/lib/components/LangSwitch.svelte
  - src/lib/components/MarkdownPreview.svelte
  - src/lib/components/PaneDiffPill.svelte
  - src/lib/components/PaneGitPill.svelte
  - src/lib/components/PaneRepoSwitcher.svelte
  - src/lib/components/PaneShellSwitcher.svelte
  - src/lib/components/QuickOpen.svelte
  - src/lib/components/RidgeDialog.svelte
  - src/lib/components/RidgePane.svelte
  - src/lib/components/SaveWorkspaceDialog.svelte
  - src/lib/components/SearchModal.svelte
  - src/lib/components/SearchSidebar.svelte
  - src/lib/components/SettingsPanel.svelte
  - src/lib/components/SidebarPluginRegion.svelte
  - src/lib/components/SourceControl.svelte
  - src/lib/components/SplitContainer.svelte
  - src/lib/components/Toggle.svelte
  - src/lib/components/WindToast.svelte
  - src/lib/components/WorkspaceSidebar.svelte
  - src/lib/components/WorkspaceTabs.svelte
public_interface:
  - "export function alertDialog(opts: DialogOptions): Promise<void>"
  - "export function buildThemeEntry(f: ThemeFormState): ThemeEntry"
  - "export function choiceDialog(opts: DialogOptions & { secondaryLabel: string
    }): Promise<ChoiceResult>"
  - "export function colorForHash(hash: string): string"
  - "export function colorForLane(laneIndex: number): string"
  - "export function computeReplaySequence(state: InputBufferState): string"
  - "export function confirmDialog(opts: DialogOptions): Promise<boolean>"
  - "export function deriveBufferEvent(spec: KeySpec): InputBufferEvent | null"
  - "export function layoutGraph( commits: GraphCommit[], options: { dx?:
    number; dy?: number; padX?: number; padY?: number; /** 当某 commit
    被展开（GitGraph inline 详情）时，在该 commit 下方多腾出的高度。 */ expandedHash?: string;
    expandedExtra?: number; } = {} ): LayoutOutput"
  - "export function openDiffEditor(args: OpenDiffArgs): void"
  - "export function previewStyle(colors: Record<string, string>): string"
  - "export function promptDialog(opts: DialogOptions): Promise<string | null>"
  - "export function summarizeExplorerPaste(outcomes: readonly
    ExplorerPasteOutcome[]): ExplorerPasteSummary"
  - "export function updateInputBuffer( state: InputBufferState, ev:
    InputBufferEvent ): InputBufferState"
  - export interface DialogOptions
  - export interface ExplorerPasteFailure
  - export interface ExplorerPasteSummary
  - export interface GraphCommit
  - export interface InputBufferState
  - export interface KeySpec
  - export interface LayoutOutput
  - export interface OpenDiffArgs
  - export interface RenderedDot
  - export interface RenderedLine
  - export interface ThemeFormState
  - export type ChoiceResult
  - export type ExplorerPasteOutcome
  - export type InputBufferEvent
---

# src/lib/components module

Observed from the existing project. Confirm intended behavior, targets, and interfaces before baseline.
