// src/lib/stores/paneTree.ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { get, writable } from 'svelte/store';
import type { PaneNode } from '$lib/types';
import { reportDevIssue } from '$lib/devIssue';

function normalizeSplitRatios(sizes: number[]): number[] {
  const s = sizes.reduce((a, b) => a + b, 0);
  if (s <= 1e-9) return sizes.map(() => 100 / Math.max(sizes.length, 1));
  return sizes.map((x) => (x / s) * 100);
}

/** 仅更新 `path` 所指 `Split` 的 `ratios`（path 为空表示根为 Split）。 */
function applyRatiosAtPath(
  root: PaneNode,
  path: number[],
  sizes: number[]
): PaneNode {
  if (path.length === 0) {
    if (root.type !== 'split' || root.children.length !== sizes.length)
      return root;
    return { ...root, ratios: normalizeSplitRatios(sizes) };
  }
  if (root.type !== 'split') return root;
  const [head, ...tail] = path;
  if (head < 0 || head >= root.children.length) return root;
  return {
    ...root,
    children: root.children.map((child, i) =>
      i === head ? applyRatiosAtPath(child, tail, sizes) : child
    ),
  };
}
/** 占位；首屏 hydrate 前不挂载终端。根 pane 的 id 由后端按工作区生成唯一 UUID。 */
export const paneTreeStore = writable<PaneNode>({
  type: 'leaf',
  id: '',
});

/** 最近一次点击/聚焦的终端窗格；分屏针对此 id（与 layout 中 leaf id 一致）。 */
export const activePaneId = writable<string>('');

/** 正在拖拽重组的源窗格 id（标题栏 dragstart 设置，dragend 清空）。 */
export const paneDragSourceId = writable<string | null>(null);

export type DockRegion = 'left' | 'right' | 'top' | 'bottom' | 'center';
export type SplitterAxis = 'x' | 'y';

export interface SplitterRef {
  splitPath: number[];
  splitterIndex: number;
  axis: SplitterAxis;
  basisPx: number;
}

interface SplitterSnapshot {
  ref: SplitterRef;
  ratios: number[];
  isPrimary: boolean;
}

export type SplitResizeUiState =
  | { phase: 'idle' }
  | {
      phase: 'pending' | 'junction';
      primary: SplitterRef;
      orthogonals: SplitterRef[];
      sameAxisCandidates: SplitterRef[];
      pointer: { x: number; y: number };
      snapState: JunctionSnapState | null;
    }
  | {
      phase: 'drag';
      pointer: { x: number; y: number };
      dragStart: { x: number; y: number };
      snapshots: SplitterSnapshot[];
      pendingUpdates: SplitRatioUpdate[];
      snapState: JunctionSnapState | null;
    };

export interface SplitRatioUpdate {
  path: number[];
  ratios: number[];
}

export interface JunctionSplitterRef {
  splitPath: number[];
  splitterIndex: number;
  axis: SplitterAxis;
  basisPx: number;
  side: 'before' | 'after';
}

export interface JunctionRef {
  id: string;
  positionPx: { x: number; y: number };
  axis: SplitterAxis;
  splitters: JunctionSplitterRef[];
}

export interface JunctionSnapState {
  junction: JunctionRef;
  coupledSplitters: SplitterRef[];
}

export const SNAP_THRESHOLD_PX = 10;

const HOVER_DEBOUNCE_MS = 90;
const MIN_PANE_RATIO = 6;
let splitHoverTimer: ReturnType<typeof setTimeout> | undefined;
export const splitResizeUiState = writable<SplitResizeUiState>({
  phase: 'idle',
});

export const activeWorkspaceId = writable<string>('');

export const workspacesList = writable<
  { id: string; index: number; name?: string }[]
>([]);

// 工作区名称映射（用于UI显示）
export const workspaceNames = writable<Record<string, string>>({});

function pathKey(path: number[]): string {
  return path.join('/');
}

function clearSplitHoverTimer() {
  if (splitHoverTimer !== undefined) {
    clearTimeout(splitHoverTimer);
    splitHoverTimer = undefined;
  }
}

// Junction registry for O(1) snap-to-junction lookup
// Key: "${axis}-${Math.round(positionPx)}" e.g., "x-450"
const junctionRegistry = new Map<string, JunctionRef[]>();

export function clearJunctionRegistry() {
  junctionRegistry.clear();
}

export function registerJunction(junction: JunctionRef) {
  const key = `${junction.axis}-${Math.round(
    junction.positionPx[junction.axis]
  )}`;
  const existing = junctionRegistry.get(key) || [];
  if (!existing.find((j) => j.id === junction.id)) {
    existing.push(junction);
    junctionRegistry.set(key, existing);
  }
}

export function findJunctionsNearPosition(
  axis: SplitterAxis,
  positionPx: number,
  threshold: number = SNAP_THRESHOLD_PX
): JunctionRef[] {
  const candidates: JunctionRef[] = [];
  const minKey = Math.round(positionPx - threshold);
  const maxKey = Math.round(positionPx + threshold);
  for (let k = minKey; k <= maxKey; k++) {
    const junctions = junctionRegistry.get(`${axis}-${k}`);
    if (junctions) {
      for (const j of junctions) {
        const distance = Math.abs(j.positionPx[axis] - positionPx);
        if (distance <= threshold) {
          candidates.push(j);
        }
      }
    }
  }
  return candidates;
}

function normalizeWithin100(values: number[]): number[] {
  const sum = values.reduce((a, b) => a + b, 0);
  if (sum <= 1e-9) return values.map(() => 100 / Math.max(values.length, 1));
  return values.map((v) => (v / sum) * 100);
}

function getSplitNodeByPath(
  root: PaneNode,
  path: number[]
): Extract<PaneNode, { type: 'split' }> | null {
  let cur: PaneNode = root;
  for (const idx of path) {
    if (cur.type !== 'split') return null;
    cur = cur.children[idx];
    if (!cur) return null;
  }
  return cur.type === 'split' ? cur : null;
}

function adjustRatiosBySplitterDelta(
  baseRatios: number[],
  splitterIndex: number,
  deltaPercent: number
): number[] {
  const n = baseRatios.length;
  if (n <= 1) return baseRatios;
  if (splitterIndex < 0 || splitterIndex >= n - 1) return baseRatios;

  const before = baseRatios.slice(0, splitterIndex + 1);
  const after = baseRatios.slice(splitterIndex + 1);
  const beforeSum = before.reduce((a, b) => a + b, 0);
  const afterSum = after.reduce((a, b) => a + b, 0);
  if (beforeSum <= 1e-9 || afterSum <= 1e-9) return baseRatios;

  const minBefore = before.length * MIN_PANE_RATIO;
  const maxBefore = 100 - after.length * MIN_PANE_RATIO;
  const targetBefore = Math.min(
    maxBefore,
    Math.max(minBefore, beforeSum + deltaPercent)
  );
  const targetAfter = 100 - targetBefore;
  const beforeScale = targetBefore / beforeSum;
  const afterScale = targetAfter / afterSum;

  const next = baseRatios.map((ratio, idx) =>
    idx <= splitterIndex ? ratio * beforeScale : ratio * afterScale
  );
  return normalizeWithin100(next);
}

function dedupeRefs(refs: SplitterRef[]): SplitterRef[] {
  const seen = new Set<string>();
  const out: SplitterRef[] = [];
  for (const ref of refs) {
    const key = `${pathKey(ref.splitPath)}:${ref.splitterIndex}:${ref.axis}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(ref);
  }
  return out;
}

function updatesFromSnapshots(
  snapshots: SplitterSnapshot[],
  dragStart: { x: number; y: number },
  pointer: { x: number; y: number }
): SplitRatioUpdate[] {
  const grouped = new Map<string, SplitterSnapshot[]>();
  for (const snap of snapshots) {
    const key = pathKey(snap.ref.splitPath);
    const list = grouped.get(key);
    if (list) list.push(snap);
    else grouped.set(key, [snap]);
  }
  const updates: SplitRatioUpdate[] = [];
  for (const [, refs] of grouped) {
    let merged = refs[0].ratios.slice();
    for (const { ref, isPrimary } of refs) {
      if (ref.basisPx <= 1) continue;
      const rawDeltaPx =
        ref.axis === 'x' ? pointer.x - dragStart.x : pointer.y - dragStart.y;
      // 正交联动轴更容易受手部微抖影响，给更大的 deadzone，减少“乱飘”。
      const deadzone = isPrimary ? 0.8 : 2.8;
      const deltaPx = Math.abs(rawDeltaPx) <= deadzone ? 0 : rawDeltaPx;
      const deltaPercent = (deltaPx / ref.basisPx) * 100;
      merged = adjustRatiosBySplitterDelta(
        merged,
        ref.splitterIndex,
        deltaPercent
      );
    }
    updates.push({ path: refs[0].ref.splitPath, ratios: merged });
  }
  return updates;
}

function applyRatioUpdates(
  root: PaneNode,
  updates: SplitRatioUpdate[]
): PaneNode {
  let next = root;
  for (const update of updates) {
    next = applyRatiosAtPath(next, update.path, update.ratios);
  }
  return next;
}

function setGlobalSplitResizeCursor(enabled: boolean) {
  if (typeof document === 'undefined') return;
  document.body.classList.toggle('wf-resize-junction-cursor', enabled);
}

export function queueSplitResizeJunction(
  primary: SplitterRef,
  orthogonals: SplitterRef[],
  pointer: { x: number; y: number },
  sameAxisCandidates: SplitterRef[] = [],
  snapState: JunctionSnapState | null = null
) {
  clearSplitHoverTimer();
  const allRefs = dedupeRefs([primary, ...orthogonals, ...(sameAxisCandidates ?? [])]);
  const [first, ...rest] = allRefs;
  if (!first) return;
  splitResizeUiState.set({
    phase: 'pending',
    primary: first,
    orthogonals: rest,
    sameAxisCandidates,
    pointer,
    snapState,
  });
  splitHoverTimer = setTimeout(() => {
    splitResizeUiState.set({
      phase: 'junction',
      primary: first,
      orthogonals: rest,
      sameAxisCandidates,
      pointer,
      snapState,
    });
    setGlobalSplitResizeCursor(true);
  }, HOVER_DEBOUNCE_MS);
}

export function clearSplitResizeUi() {
  clearSplitHoverTimer();
  setGlobalSplitResizeCursor(false);
  if (typeof document !== 'undefined') {
    document.body.classList.remove('wf-resize-4way');
  }
  splitResizeUiState.set({ phase: 'idle' });
}

export function startSplitResizeDrag(pointer: { x: number; y: number }) {
  const ui = get(splitResizeUiState);
  if (ui.phase !== 'junction') return;
  const root = get(paneTreeStore);

  // Check if 4-way junction snap (3+ coupled splitters at same junction)
  const is4WaySnap =
    ui.snapState !== null && ui.snapState.coupledSplitters.length >= 3;

  // Include all coupled splitters from snap state for 4-way resize
  let refs = dedupeRefs([ui.primary, ...ui.orthogonals]);
  if (ui.snapState) {
    refs = dedupeRefs([...refs, ...ui.snapState.coupledSplitters]);
  }

  const snapshots: SplitterSnapshot[] = [];
  for (let i = 0; i < refs.length; i += 1) {
    const ref = refs[i];
    const split = getSplitNodeByPath(root, ref.splitPath);
    if (!split) continue;
    let basisPx = ref.basisPx;
    if (typeof document !== 'undefined') {
      const splitRoot = document.querySelector(
        `.wf-split[data-split-path="${pathKey(
          ref.splitPath
        )}"][data-split-axis="${ref.axis}"]`
      ) as HTMLElement;
      if (splitRoot) {
        basisPx = Math.max(
          1,
          ref.axis === 'x' ? splitRoot.clientWidth : splitRoot.clientHeight
        );
      }
    }
    snapshots.push({
      ref: { ...ref, basisPx },
      ratios: split.ratios.slice(),
      isPrimary: i === 0,
    });
  }
  if (!snapshots.length) return;
  splitResizeUiState.set({
    phase: 'drag',
    pointer,
    dragStart: pointer,
    snapshots,
    pendingUpdates: [],
    snapState: ui.snapState,
  });
  setGlobalSplitResizeCursor(true);
  if (is4WaySnap && typeof document !== 'undefined') {
    document.body.classList.add('wf-resize-4way');
  }
}

export function updateSplitResizeDrag(pointer: { x: number; y: number }) {
  const ui = get(splitResizeUiState);
  if (ui.phase !== 'drag') return;
  const updates = updatesFromSnapshots(ui.snapshots, ui.dragStart, pointer);
  paneTreeStore.update((root) => applyRatioUpdates(root, updates));
  splitResizeUiState.set({
    ...ui,
    pointer,
    pendingUpdates: updates,
  });
}

export function finishSplitResizeDrag(): SplitRatioUpdate[] {
  const ui = get(splitResizeUiState);
  clearSplitHoverTimer();
  setGlobalSplitResizeCursor(false);
  if (typeof document !== 'undefined') {
    document.body.classList.remove('wf-resize-4way');
  }
  splitResizeUiState.set({ phase: 'idle' });
  if (ui.phase !== 'drag') return [];
  return ui.pendingUpdates;
}

export function getAllPaneIds(node: PaneNode): string[] {
  const ids: string[] = [];
  function traverse(n: PaneNode | undefined | null) {
    if (!n) return;
    if (n.type === 'leaf') {
      if (n.id) ids.push(n.id);
    } else {
      n.children?.forEach(traverse);
    }
  }
  traverse(node);
  return ids;
}

/** 当前 activePaneId 若不在树内（切换工作区等），回退到第一个 leaf。 */
function reconcileActivePaneId(layout: PaneNode) {
  const ids = getAllPaneIds(layout);
  if (!ids.length) return;
  const cur = get(activePaneId);
  if (!cur || !ids.includes(cur)) activePaneId.set(ids[0]);
}

export async function syncPaneLayoutFromBackend() {
  if (!isTauri()) return;
  try {
    const layout = await invoke<PaneNode>('get_pane_layout');
    paneTreeStore.set(layout);
    reconcileActivePaneId(layout);
  } catch (e) {
    console.error('syncPaneLayoutFromBackend', e);
    reportDevIssue({
      title: 'Layout sync failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
  // Refresh cwd listeners so new panes from split/close/dock are wired up.
  // activeWorkspaceId is already set by the time this is called from
  // splitPane / closePane / dockPane / etc.
  const active = get(activeWorkspaceId);
  if (active) {
    await setupPaneCwdListeners(active);
  }
}

export async function refreshWorkspaces() {
  if (!isTauri()) return;
  try {
    const list = await invoke<{ id: string; index: number }[]>(
      'list_workspaces'
    );
    const active = await invoke<string>('get_active_workspace_id');
    const layout = await invoke<PaneNode>('get_pane_layout');
    workspacesList.set(list);
    paneTreeStore.set(layout);
    activeWorkspaceId.set(active);
    reconcileActivePaneId(layout);
  } catch (e) {
    console.error('refreshWorkspaces', e);
    reportDevIssue({
      title: 'Workspace refresh failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}

export async function createWorkspace() {
  if (!isTauri()) return;
  try {
    await invoke<string>('create_workspace');
    await refreshWorkspaces();
  } catch (e) {
    console.error('createWorkspace', e);
    reportDevIssue({
      title: 'Workspace create failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}

export async function switchWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  try {
    await invoke('switch_workspace', { workspaceId });
    const layout = await invoke<PaneNode>('get_pane_layout');
    paneTreeStore.set(layout);
    activeWorkspaceId.set(workspaceId);
    reconcileActivePaneId(layout);
    // Re-attach cwd listeners for the new workspace
    await setupPaneCwdListeners(workspaceId);
  } catch (e) {
    console.error('switchWorkspace', workspaceId, e);
    reportDevIssue({
      title: 'Workspace switch failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}

export async function splitPane(
  paneId: string,
  direction: 'horizontal' | 'vertical'
) {
  if (!isTauri()) return '';
  const newId = await invoke<string>('split_pane', {
    paneId,
    direction,
  });
  await syncPaneLayoutFromBackend();
  return newId;
}

/** 将源窗格拖到目标上：四边为分栏，中间为与目标互换位置。 */
export async function dockPane(
  sourcePaneId: string,
  targetPaneId: string,
  region: DockRegion
) {
  if (!isTauri()) return;
  await invoke('dock_pane', {
    sourcePaneId,
    targetPaneId,
    region,
  });
  await syncPaneLayoutFromBackend();
  activePaneId.set(sourcePaneId);
}

/** 拖拽分割条结束后：更新本地树并写回后端（嵌套横纵各自一条 path）。 */
export async function persistSplitRatios(splitPath: number[], sizes: number[]) {
  const norm = normalizeSplitRatios(sizes);
  paneTreeStore.update((root) => applyRatiosAtPath(root, splitPath, norm));
  if (!isTauri()) return;
  try {
    await invoke('set_split_ratios_at_path', { path: splitPath, ratios: norm });
  } catch (e) {
    console.error('persistSplitRatios', e);
    await syncPaneLayoutFromBackend();
  }
}

/** 一次性持久化多个 split 的 ratios（用于横纵联动拖拽松手提交）。 */
export async function persistSplitRatiosBatch(updates: SplitRatioUpdate[]) {
  if (!updates.length) return;
  paneTreeStore.update((root) => applyRatioUpdates(root, updates));
  if (!isTauri()) return;
  try {
    await invoke('set_split_ratios_batch', { updates });
  } catch (e) {
    console.error('persistSplitRatiosBatch', e);
    await syncPaneLayoutFromBackend();
  }
}

/** 对当前焦点窗格分屏（若无有效 id 则回退第一个 leaf）。 */
export async function splitActivePane(direction: 'horizontal' | 'vertical') {
  let id = get(activePaneId);
  const tree = get(paneTreeStore);
  const valid = getAllPaneIds(tree);
  if (!valid.length) return '';
  if (!valid.includes(id)) id = valid[0];
  return splitPane(id, direction);
}

export async function closePane(paneId: string) {
  if (!isTauri()) return;
  await invoke('close_pane', { paneId });
  await syncPaneLayoutFromBackend();
}

export async function toggleEditor(paneId: string, filePath?: string) {
  if (!isTauri()) return;
  await invoke('toggle_mode', {
    paneId,
    mode: {
      Editor: { file_path: filePath || null, language: 'rust' },
    },
  });
}

/** 关闭工作区 */
export async function closeWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  try {
    await invoke('close_workspace', { workspaceId });
    await refreshWorkspaces();
  } catch (e) {
    console.error('closeWorkspace', e);
    reportDevIssue({
      title: 'Workspace close failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}

/** 重新排序工作区 */
export async function reorderWorkspaces(fromIndex: number, toIndex: number) {
  if (!isTauri()) return;
  try {
    await invoke('reorder_workspaces', { fromIndex, toIndex });
    await refreshWorkspaces();
  } catch (e) {
    console.error('reorderWorkspaces', e);
    reportDevIssue({
      title: 'Workspace reorder failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}

/** 重命名工作区 */
export async function renameWorkspace(workspaceId: string, name: string) {
  if (!isTauri()) return;
  try {
    await invoke('rename_workspace', { workspaceId, name });
    // 更新本地名称映射
    workspaceNames.update((names) => ({ ...names, [workspaceId]: name }));
    await refreshWorkspaces();
  } catch (e) {
    console.error('renameWorkspace', e);
    reportDevIssue({
      title: 'Workspace rename failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined,
    });
    throw e;
  }
}

// ============ 已保存工作区相关 ============

// TODO: paneCwds is not persisted — backend WorkspaceHistoryItem lacks this field.
// Restored workspaces will not preserve terminal working directories.

export interface SavedWorkspace {
  id: string;
  name: string;
  paneTree: PaneNode;
  paneCwds: Record<string, string>; // Not yet populated by backend
  savedAt: string;
}

/** Keyed by "${workspaceId}:${paneId}" → cwd string. */
export const paneCwdStore = writable<Record<string, string>>({});

/** Update the cwd for a specific pane. */
export function setPaneCwd(workspaceId: string, paneId: string, cwd: string) {
  paneCwdStore.update((store) => ({ ...store, [`${workspaceId}:${paneId}`]: cwd }));
}

/** Retrieve the cwd for a specific pane, if known. */
export function getPaneCwd(workspaceId: string, paneId: string): string | undefined {
  return get(paneCwdStore)[`${workspaceId}:${paneId}`];
}

/**
 * Recursively extracts all pane CWDs from a PaneNode tree.
 * Produces a map keyed as `"${workspaceId}:${paneId}" -> cwd_string`.
 * Only leaf nodes with a non-null cwd are included.
 */
export function extractCwdsFromLayout(
  node: PaneNode,
  workspaceId: string
): Record<string, string> {
  const result: Record<string, string> = {};
  function traverse(n: PaneNode | undefined | null): void {
    if (!n) return;
    if (n.type === 'leaf') {
      if (n.cwd !== undefined && n.cwd !== null) {
        result[`${workspaceId}:${n.id}`] = n.cwd;
      }
    } else {
      n.children?.forEach(traverse);
    }
  }
  traverse(node);
  return result;
}

/**
 * Sets up Tauri event listeners for pane-cwd-changed-{workspaceId}-{paneId} events
 * for ALL panes in the given workspace's current pane tree.
 * Listeners are tracked so they can be torn down on workspace switch.
 */
const activeCwdListeners = new Map<string, () => void>();

export async function setupPaneCwdListeners(workspaceId: string): Promise<void> {
  if (!isTauri()) return;

  // Tear down any existing listeners for this workspace
  const existing = activeCwdListeners.get(workspaceId);
  if (existing) {
    existing();
    activeCwdListeners.delete(workspaceId);
  }

  // Collect all pane IDs in the current tree
  const tree = get(paneTreeStore);
  const paneIds = getAllPaneIds(tree);

  const unlisteners: Array<() => void> = [];
  for (const paneId of paneIds) {
    if (!paneId) continue; // skip empty IDs (e.g., pre-hydration default leaf)
    const ch = `pane-cwd-changed-${workspaceId}-${paneId}`;
    const unlisten = await listen<{ cwd: string }>(ch, (e) => {
      setPaneCwd(workspaceId, paneId, e.payload.cwd);
    });
    unlisteners.push(unlisten);
  }

  activeCwdListeners.set(workspaceId, () => {
    unlisteners.forEach((u) => u());
  });
}

export const savedWorkspacesList = writable<SavedWorkspace[]>([]);

/** 获取已保存的工作区列表 */
export async function loadSavedWorkspaces() {
  if (!isTauri()) return;
  try {
    const list = await invoke<SavedWorkspace[]>('list_saved_workspaces');
    savedWorkspacesList.set(list);

    // Populate paneCwdStore from the persisted paneTree layouts.
    // The layout's LayoutNode::Leaf carries cwd from the backend.
    for (const sw of list) {
      const cwds = extractCwdsFromLayout(sw.paneTree, sw.id);
      paneCwdStore.update((store) => ({ ...store, ...cwds }));
    }

    // Set up cwd change listeners for the currently active workspace
    const active = get(activeWorkspaceId);
    if (active) {
      await setupPaneCwdListeners(active);
    }
  } catch (e) {
    console.error('loadSavedWorkspaces', e);
 reportDevIssue({
 title: 'Load saved workspaces failed',
 message: String(e),
 stack: e instanceof Error ? e.stack : undefined,
 });
 throw e;
  }
}

/** 保存当前工作区 */
export async function saveCurrentWorkspace() {
  if (!isTauri()) return;
  try {
    await invoke('save_workspace');
    await loadSavedWorkspaces();
  } catch (e) {
    console.error('saveCurrentWorkspace', e);
  }
}

/** 删除已保存的工作区 */
export async function deleteSavedWorkspace(id: string) {
  if (!isTauri()) return;
  try {
    await invoke('delete_saved_workspace', { id });
    await loadSavedWorkspaces();
  } catch (e) {
    console.error('deleteSavedWorkspace', e);
  }
}

/** 重命名已保存的工作区 */
export async function renameSavedWorkspace(id: string, name: string) {
  if (!isTauri()) return;
  try {
    await invoke('rename_saved_workspace', { id, name });
    await loadSavedWorkspaces();
  } catch (e) {
    console.error('renameSavedWorkspace', e);
  }
}
