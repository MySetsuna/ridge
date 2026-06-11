// src/lib/stores/paneTree.ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { get, writable } from 'svelte/store';
import type { PaneNode } from '$lib/types';
// Re-export so downstream consumers (+page.svelte, paneTree.test.ts) can
// `import type { PaneNode } from '$lib/stores/paneTree'` without reaching into
// the types barrel. Matches the pattern used for other store-owned types.
export type { PaneNode };
import { reportDevIssue } from '$lib/devIssue';
import { fileExplorerStore } from '$lib/stores/fileExplorer';
import { TerminalManager } from '$lib/terminal/manager';
import { teardownPtyBridge } from '$lib/terminal/ptyBridge';

function normalizeSplitRatios(sizes: number[]): number[] {
  const s = sizes.reduce((a, b) => a + b, 0);
  if (s <= 1e-9) return sizes.map(() => 100 / Math.max(sizes.length, 1));
  return sizes.map((x) => (x / s) * 100);
}

/** 仅更�?`path` 所�?`Split` �?`ratios`（path 为空表示根为 Split）�?*/
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
/** 占位；首�?hydrate 前不挂载终端。根 pane �?id 由后端按工作区生成唯一 UUID�?*/
export const paneTreeStore = writable<PaneNode>({
  type: 'leaf',
  id: '',
});

/**
 * Per-workspace pane tree cache. Populated on switchWorkspace /
 * refreshWorkspaces / syncPaneLayoutFromBackend / split-ratio mutations
 * via `setActiveTree` / `updateActiveTree` helpers below. The +page.svelte
 * template iterates this map to render every known workspace's
 * SplitContainer in parallel (CSS display:none for inactive), so workspace
 * tab switches are CSS-only and panes stay mounted across switches �?
 * eliminating the black-screen + reload that the prior `{#key
 * activeWorkspaceId}` block forced.
 *
 * Invariant: the active workspace's entry always equals `paneTreeStore`.
 * Mutations go through the helpers, never `paneTreeStore.set/update`
 * directly, so the two stay in sync.
 */
export const workspacePaneTrees = writable<Map<string, PaneNode>>(new Map());

/**
 * Set the active workspace's tree in BOTH `paneTreeStore` (legacy single-
 * tree consumers like SplitContainer for the active workspace) and the
 * per-workspace cache (new keep-alive renderer). Callers must pass the
 * correct workspace id �?usually the just-switched-to one or `get(activeWorkspaceId)`.
 */
function setActiveTree(wsId: string, tree: PaneNode): void {
  paneTreeStore.set(tree);
  if (!wsId) return;
  workspacePaneTrees.update((m) => {
    const next = new Map(m);
    next.set(wsId, tree);
    return next;
  });
}

/** Update the active workspace's tree via a transform fn; mirrors the
 *  result into the per-workspace cache. */
function updateActiveTree(wsId: string, fn: (root: PaneNode) => PaneNode): void {
  paneTreeStore.update((root) => {
    const next = fn(root);
    if (wsId) {
      workspacePaneTrees.update((m) => {
        const m2 = new Map(m);
        m2.set(wsId, next);
        return m2;
      });
    }
    return next;
  });
}

/** Drop a workspace from the per-workspace tree cache (used on workspace close). */
export function forgetWorkspaceTree(wsId: string): void {
  if (!wsId) return;
  workspacePaneTrees.update((m) => {
    if (!m.has(wsId)) return m;
    const next = new Map(m);
    next.delete(wsId);
    return next;
  });
}

/** 最近一次点�?聚焦的终端窗格；分屏针对�?id（与 layout �?leaf id 一致）�?*/
export const activePaneId = writable<string>('');

/** 正在拖拽重组的源窗格 id（标题栏 dragstart 设置，dragend 清空）�?*/
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
  dragStart: { x: number; y: number };
  /** mousedown �?dragStart 沿轴坐标 - splitter 视觉中心沿轴坐标�?
      用于吸附时的 effectivePointer 偏移补偿，避�?A 被吸附到偏离 B 中线的位置�?*/
  mousedownOffsetAxis: number;
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
      /** 未命中联�?gating、但位于吸附阈值内的同向兄弟，用于拖动中视觉吸�?*/
      sameAxisAttractors: SplitterRef[];
      /** Px-anchor 计划：拖主分隔线时，descendant 同向 split �?absorber
       *  child 吞下尺寸变化，其�?children 保持 mousedown 时的像素宽度�?*/
      pxAnchors: PxAnchorPlan[];
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

/**
 * 同向兄弟分隔线的垂直距离吸附阈值�?
 * 当拖动中的分隔线 A 与另一条同向分隔线 B 的中线（�?A 的拖动轴方向）距�?�?此值时�?
 *   - 视觉�?A 自动吸附�?B 的中线位置（真实改动 ratio，松手后定格）；
 *   - 且若鼠标距离 BC 交点的两�?axis 距离�?�?`INTERSECTION_PROXIMITY_PX`�?
 *     则触�?A、B 联动拖动�?
 */
export const SAME_AXIS_ATTRACT_PX = 35;

/** 联动 mousedown 触发距离：鼠标距 BC/ABC 交点欧几里得距离 �?此值时，同向兄弟被纳入联动（圆形热区） */
export const INTERSECTION_PROXIMITY_PX = 50;

/**
 * 同向联动的中线对齐阈值：主线与候选兄弟线的屏幕中线差 �?此值才视为"AB 中线对齐"�?
 * 才可能触发联动。与 INTERSECTION_PROXIMITY_PX 一致，�?联动范围 = �?BC 端点
 * 为圆心的�?成立 —�?不再�?perpDistance 过大提前 reject 圆内�?mousedown�?
 */
export const SAME_AXIS_ALIGN_EPSILON_PX = 30;

/**
 * 判定鼠标是否落在某个同向兄弟 B �?BC 交点圆形热区"（半�?INTERSECTION_PROXIMITY_PX）内�?
 * 触发条件 (用于 mousedown 联动 gating + hover 高亮)�?
 *   - perpDistance(primary, sibling) �?SAME_AXIS_ALIGN_EPSILON_PX (中线对齐)
 *   - 鼠标�?B 离鼠标更近端点的欧几里得距离 �?INTERSECTION_PROXIMITY_PX
 */
export function pointerInCoupleZone(
  primary: SplitterRef,
  sibling: SplitterRef,
  pointer: { x: number; y: number }
): boolean {
  const primaryCenter = getSplitterScreenCenter(primary);
  const siblingCenter = getSplitterScreenCenter(sibling);
  const endpoints = getSplitterLineEndpoints(sibling);
  if (primaryCenter == null || siblingCenter == null || !endpoints) return false;
  const perpDistance = Math.abs(siblingCenter - primaryCenter);
  if (perpDistance > SAME_AXIS_ALIGN_EPSILON_PX) return false;
  const onAxis = primary.axis === 'x' ? pointer.x : pointer.y;
  const alongLine = primary.axis === 'x' ? pointer.y : pointer.x;
  const dxOnAxis = siblingCenter - onAxis;
  const nearestEndpoint =
    Math.abs(endpoints.start - alongLine) < Math.abs(endpoints.end - alongLine)
      ? endpoints.start
      : endpoints.end;
  const dyAlongLine = nearestEndpoint - alongLine;
  return Math.sqrt(dxOnAxis * dxOnAxis + dyAlongLine * dyAlongLine) <=
    INTERSECTION_PROXIMITY_PX;
}

/**
 * Issue 3: how far the primary must travel along its own axis before same-axis
 * coupled partners are dropped from the active snapshot set, so they stop
 * following and only the primary continues moving.
 * 设置为极大值，使高亮线段在拖动过程中不会掉�?
 */
const UNSNAP_THRESHOLD_PX = 9999;

/**
 * Split-drag coupling is two separate behaviours; we gate them independently
 * so the (A|B)/(C|D) §1.12 regression doesn't force us to disable the whole
 * feature.
 *
 * 1. Orthogonal coupling �?when the pointer is at a true `+` junction (a
 *    perpendicular splitter is within ORTHOGONAL_TRIGGER_PX of the pointer at
 *    mousedown), dragging the primary also moves the perpendicular splitter
 *    so the junction stays glued to the cursor. This is the "4-way feel".
 *    Enabling this is what users mean by "联动拖拽".
 *
 * 2. Same-axis coupling �?when a parallel sibling splitter is geometrically
 *    aligned with the primary (centre within SAME_AXIS_ALIGN_EPSILON_PX,
 *    endpoint within INTERSECTION_PROXIMITY_PX of the pointer), dragging the
 *    primary also moves the sibling. In a nested `(A|B)/(C|D)` layout at
 *    50/50 ratio C/D is automatically aligned with A/B and gets coupled �?
 *    the §1.12 (2026-05-03) regression. User explicitly does NOT want this.
 *
 * 2026-05-07 (revised twice): user reverted §1.12. They now WANT both forms
 * of coupling on so a 2x2 `(A|B)/(C|D)` grid resizes all four panes when the
 * shared central junction is dragged, AND a same-axis sibling line follows
 * the primary. Both flags are now `true`. The §1.12 side-effect (C/D moving
 * when dragging A|B) is accepted as intended behaviour now.
 *
 * Visual attract previews (sameAxisAttractors UI state) and hover detection
 * stay wired regardless �?only the actual ratio fan-out is gated.
 */
const SPLIT_DRAG_ORTHOGONAL_COUPLING_ENABLED = true;
const SPLIT_DRAG_SAMEAXIS_COUPLING_ENABLED = true;

/**
 * Px-anchor: when an outer divider resizes a pane that internally hosts a
 * same-axis split (e.g. dragging A|B in `(C|D)|B`), only the child closest
 * to the moving divider absorbs the delta �?siblings keep their absolute
 * pixel widths instead of all scaling proportionally.
 *
 * Concretely: dragging A|B grows A by ΔPx �?D (rightmost child of A, the
 * one adjacent to A|B) absorbs the entire ΔPx �?C's pixel width is locked.
 *
 * Only triggers when the inner split's axis matches the primary divider's
 * axis. Recurses into the absorber so deeper nesting (`((C|D)|E)|B`) keeps
 * C and D both anchored. Disabled when the inner split's axis differs
 * (e.g. dragging A|B and A internally is C/D vertical) �?proportional
 * scaling there is already correct.
 */
const SPLIT_DRAG_PX_ANCHOR_ENABLED = true;

export interface PxAnchorPlan {
  /** Path to the descendant split whose ratios will be fan-adjusted on drag. */
  splitPath: number[];
  /** Index of the child that absorbs the entire outer-size delta. */
  absorberIndex: number;
  /** Pixel widths of each child at mousedown �?non-absorbers are restored
   *  verbatim each tick, the absorber takes whatever remains. */
  childPxAtMousedown: number[];
  /** Outer pixel size of this split's container at mousedown. */
  outerPxAtMousedown: number;
  /** Sign of the delta that scales the outer container:
   *  - 'before' = container is on the BEFORE side of the primary splitter,
   *    so deltaPx > 0 (pointer moves right/down) GROWS the container
   *  - 'after'  = container is on the AFTER side, so deltaPx > 0 SHRINKS it */
  primaryAdjacentSide: 'before' | 'after';
}

const HOVER_DEBOUNCE_MS = 20;
const MIN_PANE_RATIO = 6;
let splitHoverTimer: ReturnType<typeof setTimeout> | undefined;
export const splitResizeUiState = writable<SplitResizeUiState>({
  phase: 'idle',
});

export const activeWorkspaceId = writable<string>('');

export const workspacesList = writable<
  { id: string; index: number; name?: string; displaySeq: number }[]
>([]);

// 工作区名称映射（用于UI显示�?
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

export interface SameAxisCandidate {
  ref: SplitterRef;
  center: number;
  distance: number;
}

/** �?DOM 里按 splitPath + axis �?.rg-split�?
 *  Keep-alive 工作区架构下，所�?workspace �?SplitContainer 同时挂在 DOM 中，
 *  非活动工作区�?`display:none` 隐藏。多�?workspace �?root split 都用
 *  `data-split-path=""`，querySelector 只会返回 DOM 顺序�?*第一�?*——也就是
 *  tab index 0 �?splitRoot。当用户在非 tab-0 的工作区拖拽 splitter 时，
 *  这里若不挑可见的，就会拿�?tab-0 那个 display:none �?root，clientWidth=0
 *  �?basisPx 退化为 1 �?drag 立刻�?ratios 推到极端，splitter 看似"不能拖动"�?
 *
 *  优先�?`offsetParent !== null` 的（display:none �?offsetParent �?null），
 *  没有时退回第一个匹配，保留 SSR / 测试场景的旧行为�?*/
function findVisibleSplitRoot(splitPath: number[], axis: SplitterAxis): HTMLElement | null {
  if (typeof document === 'undefined') return null;
  const matches = document.querySelectorAll<HTMLElement>(
    `.rg-split[data-split-path="${pathKey(splitPath)}"][data-split-axis="${axis}"]`
  );
  if (matches.length === 0) return null;
  for (const el of matches) {
    if (el.checkVisibility()) return el;
  }
  return matches[0] ?? null;
}

/** 通过 DOM 查询获取分割条在屏幕上的中线坐标（无 DOM 时返�?null）�?*/
export function getSplitterScreenCenter(ref: SplitterRef): number | null {
  if (typeof document === 'undefined') return null;
  const splitRoot = findVisibleSplitRoot(ref.splitPath, ref.axis);
  if (!splitRoot) return null;
  const splitters = Array.from(
    splitRoot.querySelectorAll<HTMLElement>(':scope > .splitpanes__splitter')
  );
  const splitter = splitters[ref.splitterIndex];
  if (!splitter) return null;
  const rect = splitter.getBoundingClientRect();
  return ref.axis === 'x'
    ? rect.left + rect.width / 2
    : rect.top + rect.height / 2;
}

/**
 * 返回分隔线沿�?长度方向"的两个端点屏幕坐标�?
 * - 水平方向分隔线（axis='x'，拖动轴�?x）：其长度方向沿 y；返�?top/bottom�?
 * - 垂直方向分隔线（axis='y'，拖动轴�?y）：其长度方向沿 x；返�?left/right�?
 *
 * 注：此处�?端点"�?split 容器沿线方向两端，通常与正交分隔线或容器边界重合�?
 */
export function getSplitterLineEndpoints(
  ref: SplitterRef
): { start: number; end: number } | null {
  if (typeof document === 'undefined') return null;
  const splitRoot = findVisibleSplitRoot(ref.splitPath, ref.axis);
  if (!splitRoot) return null;
  const splitters = Array.from(
    splitRoot.querySelectorAll<HTMLElement>(':scope > .splitpanes__splitter')
  );
  const splitter = splitters[ref.splitterIndex];
  if (!splitter) return null;
  const rect = splitter.getBoundingClientRect();
  return ref.axis === 'x'
    ? { start: rect.top, end: rect.bottom }
    : { start: rect.left, end: rect.right };
}

/**
 * 在主分割条同方向上、屏幕坐标距�?�?threshold 像素的兄弟分割条�?
 * 用于�?1) 悬停时识别已对齐的同向分割条�?2) 拖拽中发现新进入吸附区的分割条�?
 */
export function findSameAxisRefs(
  primary: SplitterRef,
  threshold: number = SNAP_THRESHOLD_PX
): SameAxisCandidate[] {
  if (typeof document === 'undefined') return [];
  const primaryCenter = getSplitterScreenCenter(primary);
  if (primaryCenter == null) return [];
  const allSplitters = Array.from(
    document.querySelectorAll<HTMLElement>('.rg-split > .splitpanes__splitter')
  );
  const candidates: SameAxisCandidate[] = [];
  for (const splitter of allSplitters) {
    const splitRoot = splitter.parentElement;
    if (!(splitRoot instanceof HTMLElement)) continue;
    const axisAttr = splitRoot.dataset.splitAxis;
    if (axisAttr !== primary.axis) continue;
    const pathRaw = splitRoot.dataset.splitPath;
    const path =
      pathRaw === undefined || pathRaw === ''
        ? []
        : pathRaw
            .split('/')
            .map((s) => Number(s))
            .filter((n) => Number.isFinite(n));
    const splitters = Array.from(
      splitRoot.querySelectorAll<HTMLElement>(':scope > .splitpanes__splitter')
    );
    const splitterIndex = splitters.indexOf(splitter);
    if (splitterIndex < 0) continue;
    if (
      splitterIndex === primary.splitterIndex &&
      path.length === primary.splitPath.length &&
      path.every((p, i) => p === primary.splitPath[i])
    ) {
      continue;
    }
    const basisPx = Math.max(
      1,
      axisAttr === 'x' ? splitRoot.clientWidth : splitRoot.clientHeight
    );
    const rect = splitter.getBoundingClientRect();
    const center =
      axisAttr === 'x'
        ? rect.left + rect.width / 2
        : rect.top + rect.height / 2;
    const distance = Math.abs(center - primaryCenter);
    if (distance <= threshold) {
      candidates.push({
        ref: { splitPath: path, splitterIndex, axis: axisAttr, basisPx },
        center,
        distance,
      });
    }
  }
  return candidates.sort((a, b) => a.distance - b.distance);
}

/**
 * Build px-anchor plans for the descendants on each side of `primary`.
 *
 * Walks down children[splitterIndex] (BEFORE side) and children[splitterIndex+1]
 * (AFTER side) of primary's split. For each side, if the immediate child is
 * itself a split with the SAME axis as primary, snapshot its current per-child
 * pixel widths and mark the child closest to the primary divider as the
 * absorber (last child for BEFORE side, first child for AFTER side). Recurses
 * into the absorber so deeper nesting is also anchored.
 *
 * Returns empty list when:
 *   - the feature is disabled (SPLIT_DRAG_PX_ANCHOR_ENABLED = false)
 *   - primary's split node can't be resolved
 *   - primary's split has fewer than 2 children
 *   - both adjacent panes are leaves (or splits with mismatched axis)
 */
export function buildPxAnchorPlans(
  root: PaneNode,
  primary: SplitterRef,
  primaryBasisPx: number
): PxAnchorPlan[] {
  if (!SPLIT_DRAG_PX_ANCHOR_ENABLED) return [];
  const plans: PxAnchorPlan[] = [];
  const primarySplit = getSplitNodeByPath(root, primary.splitPath);
  if (!primarySplit || primarySplit.children.length < 2) return plans;
  if (primary.splitterIndex < 0 || primary.splitterIndex >= primarySplit.children.length - 1) {
    return plans;
  }

  const ratios = primarySplit.ratios.slice();
  // Pixel size of the pane on each side of the splitter at mousedown.
  // For multi-child splits, these are the widths of the SINGLE pane directly
  // adjacent to the splitter �?not the whole before/after block.
  const beforePaneIdx = primary.splitterIndex;
  const afterPaneIdx = primary.splitterIndex + 1;
  const beforePanePx = primaryBasisPx * (ratios[beforePaneIdx] ?? 0) / 100;
  const afterPanePx = primaryBasisPx * (ratios[afterPaneIdx] ?? 0) / 100;

  walkSide(
    primarySplit.children[beforePaneIdx],
    [...primary.splitPath, beforePaneIdx],
    beforePanePx,
    'before'
  );
  walkSide(
    primarySplit.children[afterPaneIdx],
    [...primary.splitPath, afterPaneIdx],
    afterPanePx,
    'after'
  );
  return plans;

  function walkSide(
    pane: PaneNode | undefined,
    panePath: number[],
    panePx: number,
    side: 'before' | 'after'
  ) {
    if (!pane || pane.type !== 'split') return;
    if (pane.children.length < 2) return;
    // Only anchor when descendant axis matches primary �?perpendicular
    // descendants stack the other way and proportional scaling is correct.
    const paneAxis = pane.direction === 'horizontal' ? 'x' : 'y';
    if (paneAxis !== primary.axis) return;

    const absorberIndex =
      side === 'before' ? pane.children.length - 1 : 0;
    const childPx = pane.ratios.map((r) => panePx * (r / 100));
    plans.push({
      splitPath: panePath,
      absorberIndex,
      childPxAtMousedown: childPx,
      outerPxAtMousedown: panePx,
      primaryAdjacentSide: side,
    });
    // Recurse into the absorber �?only its outer size changes downstream.
    walkSide(
      pane.children[absorberIndex],
      [...panePath, absorberIndex],
      childPx[absorberIndex],
      side
    );
  }
}

/**
 * Compute new ratios for a px-anchor plan given the signed primary delta.
 *
 * Non-absorber children retain their `childPxAtMousedown[i]`; absorber takes
 * whatever's left of the new outer size. If the absorber would go below the
 * MIN_PANE_RATIO floor, clamp it and let the non-absorbers shrink
 * proportionally so the split stays valid.
 */
export function pxAnchorRatios(
  plan: PxAnchorPlan,
  signedDeltaPx: number
): number[] {
  const sideSign = plan.primaryAdjacentSide === 'before' ? 1 : -1;
  const outerPxNew = Math.max(
    1,
    plan.outerPxAtMousedown + sideSign * signedDeltaPx
  );
  const minAbsorberPx = (outerPxNew * MIN_PANE_RATIO) / 100;
  const nonAbsorberSum = plan.childPxAtMousedown.reduce(
    (acc, px, i) => (i === plan.absorberIndex ? acc : acc + px),
    0
  );
  const desiredAbsorberPx = outerPxNew - nonAbsorberSum;

  const childPxNew = plan.childPxAtMousedown.slice();
  if (desiredAbsorberPx >= minAbsorberPx) {
    childPxNew[plan.absorberIndex] = desiredAbsorberPx;
  } else {
    // Absorber hit the floor �?shrink non-absorbers proportionally so the
    // outer size constraint still holds.
    childPxNew[plan.absorberIndex] = minAbsorberPx;
    const remainingPx = Math.max(0, outerPxNew - minAbsorberPx);
    const scale = nonAbsorberSum > 0 ? remainingPx / nonAbsorberSum : 0;
    for (let i = 0; i < childPxNew.length; i += 1) {
      if (i !== plan.absorberIndex) {
        childPxNew[i] = plan.childPxAtMousedown[i] * scale;
      }
    }
  }

  const ratios = childPxNew.map((px) => (px / outerPxNew) * 100);
  // Enforce per-child MIN_PANE_RATIO floor and re-normalize, mirroring
  // adjustRatiosBySplitterDelta's invariant.
  const floored = ratios.map((r) => Math.max(MIN_PANE_RATIO, r));
  return normalizeWithin100(floored);
}

function updatesFromSnapshots(
  snapshots: SplitterSnapshot[],
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
    for (const snap of refs) {
      const { ref, isPrimary, dragStart } = snap;
      if (ref.basisPx <= 1) continue;
      const rawDeltaPx =
        ref.axis === 'x' ? pointer.x - dragStart.x : pointer.y - dragStart.y;
      // 正交联动轴更容易受手部微抖影响，给更大的 deadzone，减少“乱飘”�?
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

/** 拖动 / hover-junction 期间锁定 body cursor，使其不随子元素 hover 变化�?
 *   - 'move'：全方向�?-way / orthogonal 联动�?
 *   - 'col' / 'row'：双方向（仅沿主轴的 same-axis 联动或单�?resize�?
 *   - null：释放，恢复正常 hover 行为
 *
 * 三个模式互斥：toggle 一个为 true 时其他两个会被关掉�?
 */
type SplitResizeCursorMode = 'move' | 'col' | 'row' | null;
function setGlobalSplitResizeCursor(mode: SplitResizeCursorMode) {
  if (typeof document === 'undefined') return;
  document.body.classList.toggle('rg-resize-junction-cursor', mode === 'move');
  document.body.classList.toggle('rg-resize-col-cursor', mode === 'col');
  document.body.classList.toggle('rg-resize-row-cursor', mode === 'row');
}

export function queueSplitResizeJunction(
  primary: SplitterRef,
  orthogonals: SplitterRef[],
  pointer: { x: number; y: number },
  sameAxisCandidates: SplitterRef[] = [],
  snapState: JunctionSnapState | null = null
) {
  clearSplitHoverTimer();
  // 去重但保持类型分离：先前实现 dedupeRefs([primary, ...orthos, ...sameAxis])
  // 然后 [first, ...rest] �?sameAxis 也塞�?ui.orthogonals，导�?
  // startSplitResizeDrag �?refs = [primary, ...ui.orthogonals] 把同向兄�?
  // 无条件加入联动，绕过圆形 gating。这里只去掉 primary 的重复引用�?
  const refKey = (r: SplitterRef) =>
    `${pathKey(r.splitPath)}:${r.splitterIndex}:${r.axis}`;
  const primaryKey = refKey(primary);
  const orthos = dedupeRefs(orthogonals).filter(
    (r) => refKey(r) !== primaryKey
  );
  const sameAxis = dedupeRefs(sameAxisCandidates).filter(
    (r) => refKey(r) !== primaryKey && !orthos.some((o) => refKey(o) === refKey(r))
  );
  splitResizeUiState.set({
    phase: 'pending',
    primary,
    orthogonals: orthos,
    sameAxisCandidates: sameAxis,
    pointer,
    snapState,
  });
  splitHoverTimer = setTimeout(() => {
    splitResizeUiState.set({
      phase: 'junction',
      primary,
      orthogonals: orthos,
      sameAxisCandidates: sameAxis,
      pointer,
      snapState,
    });
    // 仅在存在 orthogonal�? 方向联动）时切到 move cursor�?
    // sameAxis-only 联动仍是沿主轴双向，保持 splitter 默认 col/row-resize�?
    if (orthos.length > 0) setGlobalSplitResizeCursor('move');
  }, HOVER_DEBOUNCE_MS);
}

export function clearSplitResizeUi() {
  clearSplitHoverTimer();
  setGlobalSplitResizeCursor(null);
  if (typeof document !== 'undefined') {
    document.body.classList.remove('rg-resize-4way');
  }
  splitResizeUiState.set({ phase: 'idle' });
}

export function startSplitResizeDrag(pointer: { x: number; y: number }) {
  const ui = get(splitResizeUiState);
  if (ui.phase !== 'junction' && ui.phase !== 'pending') return;
  clearSplitHoverTimer();
  const root = get(paneTreeStore);

  // Check if 4-way junction snap (3+ coupled splitters at same junction).
  // Only used for visual feedback (rg-resize-4way body class) below; the
  // ratio-update fan-out is gated on the orthogonal flag so the visual hint
  // stays consistent with whether we actually couple at the +-junction.
  const is4WaySnap =
    SPLIT_DRAG_ORTHOGONAL_COUPLING_ENABLED &&
    ui.snapState !== null &&
    ui.snapState.coupledSplitters.length >= 3;

  // Build the snapshot ref set. Orthogonal partners (perpendicular splitters
  // at the same +-junction) and snapState siblings recover the "4-way feel".
  // Same-axis fan-out (next block) stays disabled by default.
  let refs: SplitterRef[] = SPLIT_DRAG_ORTHOGONAL_COUPLING_ENABLED
    ? dedupeRefs([ui.primary, ...ui.orthogonals])
    : [ui.primary];
  if (SPLIT_DRAG_ORTHOGONAL_COUPLING_ENABLED && ui.snapState) {
    refs = dedupeRefs([...refs, ...ui.snapState.coupledSplitters]);
  }

  // 同向兄弟联动 gating（圆�?15px 区域）：
  //   (a) 端点完全对齐：B �?A 的屏幕中线差 �?SAME_AXIS_ALIGN_EPSILON_PX
  //       （B 的端点恰好落�?A 的延长线上）�?
  //   (b) 鼠标�?BC 交点的欧几里得距�?�?INTERSECTION_PROXIMITY_PX
  //       （以 BC 交点为圆心、半�?15px 的圆形热区，不分横纵）�?
  // 两者同时满足，B 才被纳入联动；否则保留为 attractor（仅视觉吸附，不联动）�?
  const pointerAlongLine =
    ui.primary.axis === 'x' ? pointer.y : pointer.x;
  const pointerOnAxis = ui.primary.axis === 'x' ? pointer.x : pointer.y;
  const primaryCenter = getSplitterScreenCenter(ui.primary);
  const coupledSameAxis: SplitterRef[] = [];
  const attractOnlySameAxis: SplitterRef[] = [];
  for (const sibling of ui.sameAxisCandidates) {
    const endpoints = getSplitterLineEndpoints(sibling);
    const siblingCenter = getSplitterScreenCenter(sibling);
    if (!endpoints || siblingCenter == null || primaryCenter == null) {
      attractOnlySameAxis.push(sibling);
      continue;
    }
    const perpDistance = Math.abs(siblingCenter - primaryCenter);
    // BC 交点 = (B 沿轴方向中线坐标, B 离鼠标更近的端点沿线方向坐标)
    const dxOnAxis = siblingCenter - pointerOnAxis;
    const nearestEndpoint =
      Math.abs(endpoints.start - pointerAlongLine) <
      Math.abs(endpoints.end - pointerAlongLine)
        ? endpoints.start
        : endpoints.end;
    const dyAlongLine = nearestEndpoint - pointerAlongLine;
    const distToBC = Math.sqrt(dxOnAxis * dxOnAxis + dyAlongLine * dyAlongLine);
    const eligible =
      perpDistance <= SAME_AXIS_ALIGN_EPSILON_PX &&
      distToBC <= INTERSECTION_PROXIMITY_PX;
    // When sameAxis coupling is OFF (default), eligible siblings still get
    // routed to the visual attractor list �?the user keeps the highlight
    // hint without unwanted ratio updates on the sibling split.
    if (eligible && SPLIT_DRAG_SAMEAXIS_COUPLING_ENABLED) {
      coupledSameAxis.push(sibling);
    } else {
      attractOnlySameAxis.push(sibling);
    }
  }
  if (coupledSameAxis.length > 0) {
    refs = dedupeRefs([...refs, ...coupledSameAxis]);
  }

  // 4-way junction 全方向跟随：每条 orthogonal C 也可能有自己的同向兄�?D�?
  // �?D �?C 中线对齐 (�?px) 且鼠标到 CD 端点（即 ABCD 交汇点）的欧几里�?
  // 距离 �?INTERSECTION_PROXIMITY_PX 时，D 同样加入联动�?
  //
  // Skip the entire loop when sameAxis coupling is off �?there's no visual
  // attractor consumer for ortho-sibling proximity (unlike sameAxis), so
  // computing it would be pure waste. Gated on the same-axis flag because
  // ortho-siblings are themselves a parallel-fan-out variant.
  const coupledOrthoSiblings: SplitterRef[] = [];
  if (SPLIT_DRAG_SAMEAXIS_COUPLING_ENABLED) for (const ortho of ui.orthogonals) {
    const orthoCenter = getSplitterScreenCenter(ortho);
    if (orthoCenter == null) continue;
    // ortho.axis �?primary.axis，所�?�?ortho 拖动�? = "�?primary 沿线方向"
    const orthoPointerOnAxis = ortho.axis === 'x' ? pointer.x : pointer.y;
    const orthoPointerAlongLine = ortho.axis === 'x' ? pointer.y : pointer.x;
    const siblings = findSameAxisRefs(ortho, SAME_AXIS_ATTRACT_PX);
    for (const candidate of siblings) {
      const sibling = candidate.ref;
      const endpoints = getSplitterLineEndpoints(sibling);
      const siblingCenter = getSplitterScreenCenter(sibling);
      if (!endpoints || siblingCenter == null) continue;
      const perpDistance = Math.abs(siblingCenter - orthoCenter);
      const dxOnAxis = siblingCenter - orthoPointerOnAxis;
      const nearestEndpoint =
        Math.abs(endpoints.start - orthoPointerAlongLine) <
        Math.abs(endpoints.end - orthoPointerAlongLine)
          ? endpoints.start
          : endpoints.end;
      const dyAlongLine = nearestEndpoint - orthoPointerAlongLine;
      const distToCD = Math.sqrt(
        dxOnAxis * dxOnAxis + dyAlongLine * dyAlongLine
      );
      if (
        perpDistance <= SAME_AXIS_ALIGN_EPSILON_PX &&
        distToCD <= INTERSECTION_PROXIMITY_PX
      ) {
        coupledOrthoSiblings.push(sibling);
      }
    }
  }
  if (coupledOrthoSiblings.length > 0) {
    refs = dedupeRefs([...refs, ...coupledOrthoSiblings]);
  }

  const snapshots: SplitterSnapshot[] = [];
  for (let i = 0; i < refs.length; i += 1) {
    const ref = refs[i];
    const split = getSplitNodeByPath(root, ref.splitPath);
    if (!split) continue;
    let basisPx = ref.basisPx;
    if (typeof document !== 'undefined') {
      const splitRoot = findVisibleSplitRoot(ref.splitPath, ref.axis);
      if (splitRoot) {
        basisPx = Math.max(
          1,
          ref.axis === 'x' ? splitRoot.clientWidth : splitRoot.clientHeight
        );
      }
    }
    // 计算 mousedown �?pointer 相对 splitter 视觉中心的偏移（沿轴方向）�?
    // hit area 11px �?visual line �?1px，鼠标可能偏 ±5px�?
    const splitterCenter = getSplitterScreenCenter(ref);
    const dragStartAxis = ref.axis === 'x' ? pointer.x : pointer.y;
    const mousedownOffsetAxis =
      splitterCenter != null ? dragStartAxis - splitterCenter : 0;
    snapshots.push({
      ref: { ...ref, basisPx },
      ratios: split.ratios.slice(),
      isPrimary: i === 0,
      dragStart: pointer,
      mousedownOffsetAxis,
    });
  }
  if (!snapshots.length) return;
  // Build px-anchor plans using the primary's recently-measured basisPx so
  // the descendant outer sizes reflect the live container at mousedown.
  const primarySnapshot = snapshots[0];
  const pxAnchors = buildPxAnchorPlans(
    root,
    primarySnapshot.ref,
    primarySnapshot.ref.basisPx
  );
  splitResizeUiState.set({
    phase: 'drag',
    pointer,
    dragStart: pointer,
    snapshots,
    pendingUpdates: [],
    snapState: ui.snapState,
    sameAxisAttractors: attractOnlySameAxis,
    pxAnchors,
  });
  // 拖动期间强制锁定 cursor，使其不随鼠标移�?splitter / 经过其他元素而变化：
  //   - 含正交联�?�?move 全方�?
  //   - 仅同主轴联动或单�?resize �?col-resize / row-resize 双方�?
  // 这一帧立即生效，�?finishSplitResizeDrag 在松手时清除�?
  const hasOrthogonalCoupled = snapshots.some(
    (s) => s.ref.axis !== ui.primary.axis
  );
  setGlobalSplitResizeCursor(
    hasOrthogonalCoupled
      ? 'move'
      : ui.primary.axis === 'x'
        ? 'col'
        : 'row'
  );
  if (is4WaySnap && typeof document !== 'undefined') {
    document.body.classList.add('rg-resize-4way');
  }
}

export function updateSplitResizeDrag(pointer: { x: number; y: number }) {
  const ui = get(splitResizeUiState);
  if (ui.phase !== 'drag') return;

  // The coupled snapshot set is frozen at mousedown (startSplitResizeDrag).
  // Do not add new same-axis candidates mid-drag: coupling is gated on
  // endpoint proximity at drag start, and dynamic additions during drag
  // caused non-intersection splitters to be incorrectly coupled.
  //
  // Issue 3: if the primary has moved far enough along its drag axis, drop
  // same-axis non-primary entries so only the primary continues to move.
  // Orthogonal entries are intentionally kept: they move on the perpendicular
  // axis (their delta is pointer.perp - dragStart.perp), so the along-axis
  // drag distance of the primary is irrelevant to whether they should track.
  const primary = ui.snapshots.find((s) => s.isPrimary);
  let workingSnapshots = ui.snapshots;
  if (primary) {
    const dragDistance =
      primary.ref.axis === 'x'
        ? Math.abs(pointer.x - primary.dragStart.x)
        : Math.abs(pointer.y - primary.dragStart.y);
    if (dragDistance > UNSNAP_THRESHOLD_PX) {
      workingSnapshots = ui.snapshots.filter(
        (s) => s.isPrimary || s.ref.axis !== primary.ref.axis
      );
    }
  }

  // 视觉吸附：用户语�?�?C 方向�?BC 交点 �?SAME_AXIS_ATTRACT_PX �?A 吸附"—�?
  // "�?C 方向" 即沿 A 的拖动轴 (perp to A's line)，所以触发条件是 A 拖动后中�?
  // (= pointer 沿轴位置) �?B 中线 �?SAME_AXIS_ATTRACT_PX，与沿线方向无关�?
  //
  // 偏移补偿：updatesFromSnapshots 计算 deltaPx = effectivePointer.axis - dragStart.axis�?
  // �?dragStart.axis �?mousedown 时的 pointer 坐标，可能偏�?splitter 视觉中心
  // 多达 ±5px (RgSplitter hit area 11px / 视觉�?1px)。若直接�?B.center 替换�?
  // A 最终位�?= A.start_center + (B.center - dragStart.axis) = B.center - offset�?
  // 导致吸附�?A 偏离 B 中线 offset 像素 (用户报告"基本向上和向左偏")�?
  // 修复：effectivePointer.axis = B.center + offset，让 deltaPx = perpDistance�?
  // A 中线精确落在 B 中线上�?
  let effectivePointer = pointer;
  if (primary && ui.sameAxisAttractors.length > 0) {
    const axis = primary.ref.axis;
    const pointerOnAxis = axis === 'x' ? pointer.x : pointer.y;
    let bestCenter: number | null = null;
    let bestDist = SAME_AXIS_ATTRACT_PX + 1;
    for (const attractor of ui.sameAxisAttractors) {
      const bCenter = getSplitterScreenCenter(attractor);
      if (bCenter == null) continue;
      const dxOnAxis = Math.abs(bCenter - pointerOnAxis);
      if (dxOnAxis > SAME_AXIS_ATTRACT_PX) continue;
      if (dxOnAxis < bestDist) {
        bestDist = dxOnAxis;
        bestCenter = bCenter;
      }
    }
    if (bestCenter != null) {
      // �?mousedown 时记录的偏移补偿 effectivePointer，使 deltaPx 严格等于
      // perpDistance(A, B)。snapshot �?startSplitResizeDrag 时保存的
      // mousedownOffsetAxis 就是 dragStart.axis - A.center_at_mousedown�?
      // 此时 A 还未拖动，是真正的起始中心�?
      const effectiveAxisCoord = bestCenter + primary.mousedownOffsetAxis;
      effectivePointer =
        axis === 'x'
          ? { x: effectiveAxisCoord, y: pointer.y }
          : { x: pointer.x, y: effectiveAxisCoord };
    }
  }

  const updates = updatesFromSnapshots(workingSnapshots, effectivePointer);

  // Px-anchor: fan an extra ratio update onto each anchored descendant
  // split. Uses the SAME effectivePointer / deadzone semantics as the
  // primary so absorber tracking stays in lock-step with the divider.
  if (primary && ui.pxAnchors.length > 0) {
    const axis = primary.ref.axis;
    const rawDeltaPx =
      axis === 'x'
        ? effectivePointer.x - primary.dragStart.x
        : effectivePointer.y - primary.dragStart.y;
    // Mirror updatesFromSnapshots's primary deadzone (0.8 px) so a still
    // pointer doesn't flutter ratios between mousedown and the first move.
    const deltaPx = Math.abs(rawDeltaPx) <= 0.8 ? 0 : rawDeltaPx;
    for (const plan of ui.pxAnchors) {
      const ratios = pxAnchorRatios(plan, deltaPx);
      // Skip anchor updates whose path collides with an existing primary
      // update (defensive �?shouldn't happen because plans always live on
      // a descendant path strictly deeper than primary's split).
      const collision = updates.some(
        (u) => pathKey(u.path) === pathKey(plan.splitPath)
      );
      if (collision) continue;
      updates.push({ path: plan.splitPath, ratios });
    }
  }

  updateActiveTree(get(activeWorkspaceId), (root: PaneNode) =>
    applyRatioUpdates(root, updates)
  );

  splitResizeUiState.set({
    ...ui,
    pointer,
    pendingUpdates: updates,
    snapshots: workingSnapshots,
  });
}

export function finishSplitResizeDrag(): SplitRatioUpdate[] {
  const ui = get(splitResizeUiState);
  clearSplitHoverTimer();
  setGlobalSplitResizeCursor(null);
  if (typeof document !== 'undefined') {
    document.body.classList.remove('rg-resize-4way');
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

/** �?SplitRatioUpdate[] 中提取所有受影响�?leaf pane ids�?
 *  每个 update �?path 指向一�?Split 节点，该 Split 下的所�?
 *  leaf panes �?resize 后尺寸都发生了变化�?*/
export function paneIdsFromRatioUpdates(
  root: PaneNode,
  updates: SplitRatioUpdate[]
): string[] {
  const set = new Set<string>();
  for (const update of updates) {
    // Navigate to the split node at the given path
    let node: PaneNode = root;
    for (const idx of update.path) {
      if (node.type !== 'split' || idx < 0 || idx >= node.children.length) {
        node = root; // path misaligned �?fall back to root
        break;
      }
      node = node.children[idx];
    }
    // Collect all leaf panes under the reached node
    for (const id of getAllPaneIds(node)) {
      set.add(id);
    }
  }
  return [...set];
}

/** 当前 activePaneId 若不在树内（切换工作区等），回退到第一�?leaf�?*/
function reconcileActivePaneId(layout: PaneNode) {
  const ids = getAllPaneIds(layout);
  if (!ids.length) return;
  const cur = get(activePaneId);
  if (!cur || !ids.includes(cur)) activePaneId.set(ids[0]);
}

/**
 * 比较两棵 pane 树是否结构等�?—�?用于跳过"layout 变化但实际无差异"�?store
 * 触发。split / dock / resize 等操作回填时如果布局未变（例如：split 操作被取�?
 * 后回拉一次最新状态），不应让 paneTreeStore �?reference，否则所有订阅�?
 * （SplitContainer / Pane / Explorer）都被迫重算 + 终端 fit + Monaco reflow�?
 * �?JSON 串作为指纹是足够的：树深度有限，序列�?cost 远小于无谓的 DOM 重排�?
 */
function paneLayoutsEquivalent(a: PaneNode, b: PaneNode): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export async function syncPaneLayoutFromBackend() {
  if (!isTauri()) return;
  let layout: PaneNode;
  try {
    // §pane-delete-refresh fix: key the layout write on the HOST's authoritative
    // active workspace id, NOT the local `activeWorkspaceId` store. The keep-alive
    // renderer (+page.svelte) mounts each workspace's SplitContainer from
    // `workspacePaneTrees.get(ws.id)`; if the local store ever diverges from the
    // host's active id (notably over web-remote, where the store is seeded
    // asynchronously), `setActiveTree(localId, …)` writes the WRONG key and the
    // rendered tree stays stale — a closed pane lingers and its title falls back
    // to the default. Re-deriving the id from the host (mirrors refreshWorkspaces)
    // guarantees the refresh lands in the rendered key. On desktop the host id
    // already equals the store, so this is just one extra (cheap) IPC.
    layout = await invoke<PaneNode>('get_pane_layout');
    // Prefer the host's authoritative active workspace id for the render key,
    // but fall back to the local store if the host is unreachable or returns an
    // unexpected (non-string/empty) value — never clobber the store with a
    // non-string result (keeps behaviour correct on desktop and in tests).
    let wsId = get(activeWorkspaceId);
    try {
      const hostActive = await invoke<string>('get_active_workspace_id');
      if (typeof hostActive === 'string' && hostActive) wsId = hostActive;
    } catch {
      /* keep local store value */
    }
    const cached = (wsId ? get(workspacePaneTrees).get(wsId) : undefined) ?? get(paneTreeStore);
    if (!paneLayoutsEquivalent(cached, layout)) {
      setActiveTree(wsId, layout);
    } else if (wsId && !get(workspacePaneTrees).has(wsId)) {
      // Layout structure unchanged but the cache lacks an entry (first time we
      // see this workspace, e.g. after refreshWorkspaces) — seed cache only.
      workspacePaneTrees.update((m) => {
        const m2 = new Map(m);
        m2.set(wsId, layout);
        return m2;
      });
    }
    // Keep the local store in lock-step with the host so downstream consumers
    // (and the cwd-prune block below) operate on the correct workspace.
    if (wsId && get(activeWorkspaceId) !== wsId) activeWorkspaceId.set(wsId);
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
    // Atomically prune stale pane entries AND seed new ones from the layout.
    //
    // Two cases this handles:
    //   1. DELETED pane: its cwd key lingers in paneCwdStore after closePane,
    //      causing Explorer to keep rendering the column. �?prune it.
    //   2. NEW pane (e.g., split): backend inherits cwd from parent pane so
    //      no `pane-cwd-changed` event fires, meaning the new pane's cwd never
    //      gets seeded into paneCwdStore. Explorer never sees it �?never merges
    //      it into the shared column. �?seed it from the layout.
    const livePaneIds = new Set(getAllPaneIds(layout));
    const prefix = `${active}:`;
    const layoutCwds = extractCwdsFromLayout(layout, active);
    paneCwdStore.update((store) => {
      let mutated = false;
      const next: Record<string, string> = {};
      // Pass 1: keep live panes, drop dead ones.
      for (const [k, v] of Object.entries(store)) {
        if (k.startsWith(prefix)) {
          const paneId = k.slice(prefix.length);
          if (livePaneIds.has(paneId)) {
            next[k] = v;
          } else {
            // deleted pane �?drop
            mutated = true;
          }
        } else {
          next[k] = v; // other workspaces: untouched
        }
      }
      // Pass 2: seed cwds for panes present in layout but not yet in store
      // (new split panes, or panes restored from saved workspace).
      for (const [k, v] of Object.entries(layoutCwds)) {
        if (!(k in next)) {
          next[k] = v;
          mutated = true;
        }
      }
      // Identity-preserving early return: when nothing was dropped or
      // seeded, the new object would be byte-for-byte identical to the
      // existing store. Svelte writable strict-equals �?returning `store`
      // skips subscriber fire on every layout sync that didn't actually
      // change pane membership (TASKS §1.11 follow-up: this site was
      // missed in 971f7fa, fan-out still firing on every split/close/
      // dock that didn't change pane membership counts).
      return mutated ? next : store;
    });
    await setupPaneCwdListeners(active);
  }
}

/**
 * §4a workspace keep-alive: load every workspace's pane tree into the
 * `workspacePaneTrees` cache so the +page.svelte template can mount
 * each workspace's SplitContainer in parallel. Active workspace is
 * skipped �?caller already wrote it.
 *
 * Failures per-workspace are non-fatal: we just leave that workspace's
 * cache slot unset, which makes its first switch fall back to the prior
 * IPC-driven path. Idempotent �?safe to call repeatedly.
 */
async function prefetchAllWorkspaceTrees(
  list: { id: string }[],
  activeId: string,
  activeLayout: PaneNode
): Promise<void> {
  if (!isTauri()) return;
  // Active is already cached by caller via setActiveTree; ensure it's
  // there in case the caller path skipped (defensive).
  workspacePaneTrees.update((m) => {
    const m2 = new Map(m);
    m2.set(activeId, activeLayout);
    return m2;
  });
  await Promise.all(
    list
      .filter((w) => w.id && w.id !== activeId)
      .map(async (w) => {
        try {
          const layout = await invoke<PaneNode>('get_pane_layout_for', {
            workspaceId: w.id,
          });
          workspacePaneTrees.update((m) => {
            const m2 = new Map(m);
            m2.set(w.id, layout);
            return m2;
          });
        } catch (err) {
          console.warn('prefetchAllWorkspaceTrees', w.id, err);
        }
      })
  );
}

export async function refreshWorkspaces() {
  if (!isTauri()) return;
  try {
    const list = await invoke<
      { id: string; index: number; name?: string; displaySeq: number }[]
    >('list_workspaces');
    const active = await invoke<string>('get_active_workspace_id');
    const layout = await invoke<PaneNode>('get_pane_layout');
    workspacesList.set(list);
    setActiveTree(active, layout);
    activeWorkspaceId.set(active);
    reconcileActivePaneId(layout);
    // §4a workspace keep-alive: prefetch every workspace's layout so the
    // +page.svelte template can render their SplitContainers in parallel
    // (CSS hidden for inactive). First switch to a previously-untouched
    // workspace becomes a CSS class flip + one frame instead of an IPC
    // round-trip + remount + atlas warm-up.
    void prefetchAllWorkspaceTrees(list, active, layout);
    const cwds = extractCwdsFromLayout(layout, active);
    paneCwdStore.update((store) => mergePaneCwds(store, cwds));
    await setupPaneCwdListeners(active);
    // Save info changes on workspace add/remove/rename; keep UI badges accurate.
    await refreshWorkspaceSaveInfo();
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
    setActiveTree(workspaceId, layout);
    activeWorkspaceId.set(workspaceId);
    reconcileActivePaneId(layout);
    const cwds = extractCwdsFromLayout(layout, workspaceId);
    paneCwdStore.update((store) => mergePaneCwds(store, cwds));
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
  const result = await invoke<{ pane_id: string; initial_cwd: string | null }>('split_pane', {
    paneId,
    direction,
  });
  // Seed paneCwdStore synchronously so Explorer shows the new column immediately,
  // without waiting for the first pane-cwd-changed event from shell integration.
  if (result.initial_cwd) {
    const wsId = get(activeWorkspaceId);
    if (wsId) {
      setPaneCwd(wsId, result.pane_id, result.initial_cwd);
    }
  }
  await syncPaneLayoutFromBackend();
  // §split-fit (2026-05-21): after the layout sync, the source pane has
  // shrunk from filling its parent to ~50 %, and Svelte will mount the
  // new pane on the next microtask. attach() (new pane) and unpark()
  // (source pane, re-mounted at the new tree position) each schedule
  // their own initial fitPane on the next animation frame, but that
  // single RAF races SvelteKit's component mount and the wasm
  // `manager.ready()` await �?when the race goes the wrong way the
  // kernel grid stays at its attach-time 24×80 default while the
  // container is already 50 % wide, leaving the visible "黑边/空行"
  // the user sees as "拆出来的终端不是占满�?. Queue a second forced fit
  // two animation frames out so the new RidgePane has reliably finished
  // its async attach pipeline before we ask the manager to size against
  // the settled DOM.
  scheduleForceFitAfterSplit(paneId, result.pane_id);
  return result.pane_id;
}

/**
 * Belt-and-suspenders fit after a split.
 *
 * Multi-retry rationale:
 *   - Frame ~2: Svelte reconciles the store update and mounts the new
 *     RidgePane component. onMount fires; the async `manager.ready()`
 *     await begins. Container may still be 0×0.
 *   - Frame ~5 (50ms): `manager.attach(paneId, container, workspaceId)`
 *     has finished, the new entry is in `manager.panes`, and the
 *     container may now have its post-split bounding rect.
 *   - 150ms / 400ms: fallback windows for slow layout (heavy DOM,
 *     webfont loading, WebGPU adapter init) — fitPaneNow is a no-op
 *     when the computed rows×cols haven't changed, so retries are
 *     cheap until the DOM actually settles.
 *
 * Exported (re-exported below) so unit tests can mock TerminalManager
 * and assert on the per-pane call without going through the full
 * `splitPane` IPC dance.
 */
function scheduleForceFitAfterSplit(sourcePaneId: string, newPaneId: string): void {
  if (typeof requestAnimationFrame === 'undefined') return;
  const fitBoth = () => {
    const mgr = TerminalManager.instance();
    mgr.fitPaneNow(sourcePaneId);
    mgr.fitPaneNow(newPaneId);
  };
  requestAnimationFrame(() => {
    requestAnimationFrame(() => fitBoth());
    setTimeout(() => fitBoth(), 50);
    setTimeout(() => fitBoth(), 150);
    setTimeout(() => fitBoth(), 400);
  });
}

/** Test-only: exported so paneTree.test.ts can drive the post-split fit
 *  scheduling against a mocked TerminalManager. Not for production use. */
export const __test_scheduleForceFitAfterSplit = scheduleForceFitAfterSplit;

/**
 * Belt-and-suspenders fit after a BACKEND-driven layout change (teammate
 * `split` / `reused` / `detached` / `removed` arriving via the
 * `teammate-layout-changed` event), mirroring `scheduleForceFitAfterSplit`
 * for the front-end `splitPane` path.
 *
 * Why the teammate path needs its own variant: a teammate split creates the
 * pane in the BACKEND and the front-end only re-syncs the tree, so the new
 * RidgePane relies SOLELY on its single attach-time fit — which races
 * SvelteKit mount + `manager.ready()`. When that race loses, the kernel grid
 * stays at the 24×80 attach default while the container already has its
 * post-split width, leaving the dead strip on the right the user reported
 * （普通 split 由 `scheduleForceFitAfterSplit` 补偿，故无此症状）。The split
 * event carries only a `trace_id` (not the new pane id), and removal/detach
 * also grows the surviving siblings, so we force-fit EVERY pane in the active
 * workspace's tree; `fitPaneNow` is a no-op when a pane's rows×cols are
 * unchanged, so refitting the untouched panes (incl. the shrunk source) is
 * cheap. Retry cadence is identical to `scheduleForceFitAfterSplit` so both
 * the slow-DOM and WebGPU-init windows are covered.
 */
export function scheduleForceFitActivePanes(): void {
  if (typeof requestAnimationFrame === 'undefined') return;
  const fitAll = () => {
    const mgr = TerminalManager.instance();
    for (const id of getAllPaneIds(get(paneTreeStore))) {
      mgr.fitPaneNow(id);
    }
  };
  requestAnimationFrame(() => {
    requestAnimationFrame(() => fitAll());
    setTimeout(() => fitAll(), 50);
    setTimeout(() => fitAll(), 150);
    setTimeout(() => fitAll(), 400);
  });
}

/** Test-only: exported so paneTree.test.ts can drive the teammate-path
 *  force-fit scheduling against a mocked TerminalManager. */
export const __test_scheduleForceFitActivePanes = scheduleForceFitActivePanes;

/** 将源窗格拖到目标上：四边为分栏，中间为与目标互换位置�?*/
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

/** 拖拽分割条结束后：更新本地树并写回后端（嵌套横纵各自一�?path）�?*/
export async function persistSplitRatios(splitPath: number[], sizes: number[]) {
  const norm = normalizeSplitRatios(sizes);
  updateActiveTree(get(activeWorkspaceId), (root) =>
    applyRatiosAtPath(root, splitPath, norm)
  );
  if (!isTauri()) return;
  try {
    await invoke('set_split_ratios_at_path', { path: splitPath, ratios: norm });
  } catch (e) {
    console.error('persistSplitRatios', e);
    await syncPaneLayoutFromBackend();
  }
}

/** 一次性持久化多个 split �?ratios（用于横纵联动拖拽松手提交）�?*/
export async function persistSplitRatiosBatch(updates: SplitRatioUpdate[]) {
  if (!updates.length) return;
  updateActiveTree(get(activeWorkspaceId), (root) =>
    applyRatioUpdates(root, updates)
  );
  if (!isTauri()) return;
  try {
    await invoke('set_split_ratios_batch', { updates });
  } catch (e) {
    console.error('persistSplitRatiosBatch', e);
    await syncPaneLayoutFromBackend();
  }
}

/** 对当前焦点窗格分屏（若无有效 id 则回退第一�?leaf）�?*/
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
  // Real-close cleanup (TASKS §5.1). Manager.park stays mounted across
  // split / reparent unmount, so detach must happen here when the pane
  // is genuinely gone from the backend tree.
  //
  // Order matters:
  //   1. Tear down PTY bridge �?no more pty-output events delivered
  //      to a kernel we're about to free.
  //   2. Manager.detach �?frees wasm kernel + render handle.
  //   3. Drop title-store entries so SplitContainer / Explorer don't
  //      keep showing a label for a pane that no longer exists.
  // 拆除 PTY 连接 �?不再投�?pty-output 事件到即将释放的 kernel
  teardownPtyBridge(paneId);
  TerminalManager.instance().detach(paneId);
  paneOscTitleStore.update((s) => {
    if (!(paneId in s)) return s;
    const c = { ...s };
    delete c[paneId];
    return c;
  });
  terminalTitles.update((t) => {
    if (!(paneId in t)) return t;
    const c = { ...t };
    delete c[paneId];
    return c;
  });
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

/** 关闭工作�?*/
export async function closeWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  try {
    await invoke('close_workspace', { workspaceId });
    // 在拉取新的工作区快照之前就清理本地资源，避免残留�?
    // 1) 拆除该工作区�?pane-cwd 监听�?
    // 2) �?paneCwdStore 删除所�?`${workspaceId}:*` 键；
    // 3) 清空 fileExplorerStore 在该工作区下的所有列（即资源管理器的文件树列）；
    //    �?SourceControl 的仓库列表由 paneCwdStore 衍生，随之自然收敛�?
    const unlisten = activeCwdListeners.get(workspaceId);
    if (unlisten) {
      unlisten();
      activeCwdListeners.delete(workspaceId);
    }
    paneCwdStore.update((store) => {
      const prefix = `${workspaceId}:`;
      let mutated = false;
      const next: Record<string, string> = {};
      for (const [k, v] of Object.entries(store)) {
        if (k.startsWith(prefix)) {
          mutated = true;
          continue;
        }
        next[k] = v;
      }
      return mutated ? next : store;
    });
    fileExplorerStore.clearWorkspace(workspaceId);
    forgetWorkspaceTree(workspaceId);
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

/** 重新排序工作区�?
 *
 *  乐观更新：在 await invoke 之前�?**同步** �?`workspacesList` 改成新顺序，
 *  这样 WorkspaceTabs �?`$effect`（用 workspacesEqual 判断是否需要重写本�?
 *  mirror）能在落位动画后第一�?tick �?bail，与 FileEditor �?`setOrder`
 *  同步语义对齐，避免出�?拖完先弹回旧顺序、后端返回再跳到新顺�?�?
 *  �?FLIP 闪烁。后�?round-trip 完成�?`refreshWorkspaces` 再次 set�?
 *  内容相同 �?bail，无视觉副作用�?*/
export async function reorderWorkspaces(fromIndex: number, toIndex: number) {
  // 同步乐观更新：仅在边界合法时才动；保留旧序列以便后端失败时回滚�?
  let rolledBack: { id: string; index: number; name?: string; displaySeq: number }[] | null = null;
  workspacesList.update((list) => {
    if (
      fromIndex < 0 || toIndex < 0 ||
      fromIndex >= list.length || toIndex > list.length ||
      fromIndex === toIndex
    ) return list;
    rolledBack = list;
    const next = [...list];
    const [moved] = next.splice(fromIndex, 1);
    next.splice(toIndex, 0, moved);
    // 重新分配 index 字段，保持与 backend list_workspaces 的语义一致�?
    return next.map((w, i) => ({ ...w, index: i }));
  });

  if (!isTauri()) return;
  try {
    await invoke('reorder_workspaces', { fromIndex, toIndex });
    await refreshWorkspaces();
  } catch (e) {
    // 回滚到拖拽前的顺序，�?UI 与后端真实状态保持一致�?
    if (rolledBack) workspacesList.set(rolledBack);
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

// Pane cwds ARE preserved in the .ridge format: the backend PaneTree struct
// serialises Pane.cwd (Option<PathBuf>) into JSON, so openWorkspaceFromFile
// �?refreshWorkspaces �?get_pane_layout �?extractCwdsFromLayout restores them.
// `SavedWorkspace.paneCwds` below is kept for future use but is currently
// not populated by list_saved_workspaces (workspace-history path), which is
// fine because that path is not yet exposed in the frontend restore UI.

export interface SavedWorkspace {
  id: string;
  name: string;
  paneTree: PaneNode;
  paneCwds: Record<string, string>;
  savedAt: string;
}

/** Keyed by "${workspaceId}:${paneId}" �?cwd string. */
export const paneCwdStore = writable<Record<string, string>>({});

/** Keyed by paneId �?当前展示标题（合并后）。优先级：teammate > OSC > 进程名�?*/
export const terminalTitles = writable<Record<string, string>>({});

/** Keyed by paneId �?�?OSC 0/1/2 序列报告的标题（shell PS1 / Claude Code 等）�?
 *  Pane.svelte 订阅 `pane-title-changed-...` 事件后写入。值非空时覆盖 polling
 *  得到的进程名�?*/
export const paneOscTitleStore = writable<Record<string, string>>({});

/** Keyed by paneId �?foreground process name (polled every 1.5s from backend). */
export const paneForegroundProcessStore = writable<Record<string, string>>({});

/** Per-workspace save info: `{ workspaceId �?{ file_path, name } }`. Populated by
 *  `get_workspace_save_info` / `list_workspace_save_info`. Empty `file_path` means
 *  the workspace has never been saved (UI shows "Save" button); present `file_path`
 *  means it's associated with a .ridge file (UI shows "Delete" button). */
export interface WorkspaceSaveInfo {
  workspace_id: string;
  file_path?: string | null;
  name?: string | null;
}
export const workspaceSaveInfoStore = writable<Record<string, WorkspaceSaveInfo>>({});

export async function refreshWorkspaceSaveInfo(): Promise<void> {
  if (!isTauri()) return;
  try {
    const list = await invoke<WorkspaceSaveInfo[]>('list_workspace_save_info');
    const map: Record<string, WorkspaceSaveInfo> = {};
    for (const info of list) map[info.workspace_id] = info;
    workspaceSaveInfoStore.set(map);
  } catch (e) {
    console.error('refreshWorkspaceSaveInfo', e);
  }
}

export async function saveWorkspaceToFile(
  workspaceId: string,
  name: string,
  path?: string
): Promise<string> {
  const out = await invoke<string>('save_workspace_to_file', {
    workspaceId,
    name,
    path: path ?? null,
  });
  // 刷新 workspacesList 以便标签�?Explorer 头部能立刻显示新名字�?
  // refreshWorkspaces 内部已串行调�?refreshWorkspaceSaveInfo()�?
  await refreshWorkspaces();
  return out;
}

export async function openWorkspaceFromFile(path: string): Promise<string> {
  const id = await invoke<string>('open_workspace_from_file', { path });
  await refreshWorkspaces();
  await refreshWorkspaceSaveInfo();
  return id;
}

export async function deleteWorkspaceFile(workspaceId: string): Promise<void> {
  await invoke('delete_workspace_file', { workspaceId });
  await refreshWorkspaceSaveInfo();
}

export async function getDefaultWorkspaceSaveDir(): Promise<string> {
  return await invoke<string>('get_default_workspace_save_dir');
}

export async function getLastOpenedWorkspacePath(): Promise<string | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<string | null>('get_last_opened_workspace_path');
  } catch {
    return null;
  }
}

export interface StartupContext {
  cwd: string;
  wind_file_in_cwd: string | null;
  /** "cli" �?process inherited a real working dir from a terminal.
   *  "menu" �?process current_dir equals ridge.exe parent (双击 / 开始菜�?.
   *  Used to gate auto-restore: cli launch should NOT auto-open the saved
   *  workspace set, since the user signalled intent via the cwd. */
  kind: 'cli' | 'menu';
}

/** 启动上下文：进程 cwd + cwd 顶层第一�?.ridge 文件（若存在�? 启动模式�?*/
export async function getStartupContext(): Promise<StartupContext | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<StartupContext>('get_startup_context');
  } catch {
    return null;
  }
}

export async function listRecentWorkspaces(): Promise<string[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<string[]>('list_recent_workspaces');
  } catch {
    return [];
  }
}

/** 关闭时被后端写下的「下次启动应自动恢复的已保存工作区路径」列表�?
 *  �?cli 启动 + 列表非空 �?前端依次 openWorkspaceFromFile，再关掉默认�?workspace�?*/
export async function getRestoreSet(): Promise<string[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<string[]>('get_restore_set');
  } catch {
    return [];
  }
}

export interface SavedWorkspaceEntry {
  name: string;
  path: string;
  mtime_secs: number;
}

/** 默认 ~/ridge-workspaces/ 下的所�?.ridge 文件，按 mtime 倒序�?*/
export async function listSavedWorkspaceFiles(): Promise<SavedWorkspaceEntry[]> {
  if (!isTauri()) return [];
  try {
    return await invoke<SavedWorkspaceEntry[]>('list_saved_workspace_files');
  } catch {
    return [];
  }
}

export async function clearRecentWorkspaces(): Promise<void> {
  if (!isTauri()) return;
  try {
    await invoke('clear_recent_workspaces');
  } catch {
    /* ignore */
  }
}

/** Collapse home directory prefix to ~ in a cwd path. */
export function collapseCwd(cwd: string): string {
  if (!cwd) return '';
  try {
    const home =
      (typeof window !== 'undefined' && ((window as unknown) as Record<string, unknown>).__Ridge_HOME__ as string) ||
      undefined;
    if (home && cwd.startsWith(home)) {
      return '~' + cwd.slice(home.length);
    }
  } catch {
    /* ignore */
  }
  const parts = cwd.replace(/\\/g, '/').split('/').filter(Boolean);
  if (parts.length <= 2) return cwd;
  if (parts[0] === 'home' || parts[0] === 'Users' || parts[0] === 'c:' || parts[0] === 'C:') {
    const tail = parts.slice(2);
    if (tail.length === 0) return '~';
    return '~/' + tail.join('/');
  }
  return cwd;
}

/** Update the cwd for a specific pane. */
/** Normalize a cwd string into a single canonical form so the Tauri
 *  backend's emit and the wasm kernel's OSC 7 parser converge on the
 *  SAME literal even when their wire shapes differ.
 *
 *  - **Backslash �?slash**: Git Bash emits "C:/code" while PowerShell
 *    shell-integration emits "C:\\code" for the same directory.
 *  - **Drop leading "/" before a Windows drive letter**: backend
 *    `engine/cwd.rs:138-145` strips a leading `/` after URL parsing
 *    (`file:///C:/...` �?`C:/...`), but the wasm parser at
 *    `parser.rs::parse_file_uri_path` returns the path verbatim from
 *    the first `/` after the host (`file:///C:/...` �?`/C:/...`). Both
 *    fire on every OSC 7 emit and ALTERNATELY write to `paneCwdStore`
 *    with strings differing only in the leading slash �?identity
 *    guard is defeated �?Explorer cwd-effect runs twice per Enter �?
 *    file tree flickers. Funnel both writers to the same canonical
 *    form here. (User report 2026-05-05 �?root cause of the
 *    repeat-flicker traced this round.)
 *  - **Trailing slash trim**: some shells emit OSC 7 with a trailing
 *    "/" once and without it the next time �?same identity-guard
 *    defeat. Trim except when it IS the root (POSIX "/", Windows "C:/").
 */
function normalizeCwd(cwd: string): string {
  let out = cwd.replace(/\\/g, '/');
  // Drop leading "/" before a Windows drive letter ("/C:/..." �?"C:/...").
  // The check is positional: only the very first three chars of "/X:"
  // where X is alphabetic count.
  if (out.length >= 3 && out[0] === '/' && /[A-Z]/i.test(out[1]) && out[2] === ':') {
    out = out.slice(1);
  }
  if (out.length > 1 && out.endsWith('/')) {
    // Don't strip a Windows drive root like "C:/" or POSIX root "/".
    const isWindowsRoot = /^[A-Z]:\/$/i.test(out);
    if (!isWindowsRoot) {
      out = out.replace(/\/+$/, '');
    }
  }
  return out;
}

/**
 * Merge `additions` into `target`, returning the SAME reference when no
 * value actually changed. Svelte's `writable` uses strict-equality on
 * the value returned from `update(...)` �?if we always allocated a new
 * object via `{...target, ...additions}`, every subscriber would fire
 * on every call, regardless of whether content changed.
 *
 * This matters most on the cwd hot path: shell prompt redraws (Ctrl+C,
 * Enter, every `cd`-then-`cd`-back) emit OSC 7 �?`setPaneCwd` �?
 * `mergePaneCwds`. Without identity preservation, the file tree, SCM,
 * sidebar plugins, etc. all re-run their cwd subscribers on every
 * keystroke that produces a prompt redraw.
 */
function mergePaneCwds(
  target: Record<string, string>,
  additions: Record<string, string>,
): Record<string, string> {
  let next = target;
  let mutated = false;
  for (const [k, v] of Object.entries(additions)) {
    if (target[k] === v) continue;
    if (!mutated) {
      next = { ...target };
      mutated = true;
    }
    next[k] = v;
  }
  return next;
}

export function setPaneCwd(workspaceId: string, paneId: string, cwd: string): void {
  const key = `${workspaceId}:${paneId}`;
  const normalized = normalizeCwd(cwd);
  paneCwdStore.update((store) => {
    // Identity-preserving early return: same value means no subscribers
    // need to fire. Critical for the Ctrl+C / Enter prompt-redraw loop
    // �?see mergePaneCwds doc above.
    if (store[key] === normalized) return store;
    return { ...store, [key]: normalized };
  });
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
        result[`${workspaceId}:${n.id}`] = normalizeCwd(n.cwd);
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

/** 获取已保存的工作区列�?*/
export async function loadSavedWorkspaces() {
  if (!isTauri()) return;
  try {
    const list = await invoke<SavedWorkspace[]>('list_saved_workspaces');
    savedWorkspacesList.set(list);

    // Populate paneCwdStore from the persisted paneTree layouts.
    // The layout's LayoutNode::Leaf carries cwd from the backend.
    for (const sw of list) {
      const cwds = extractCwdsFromLayout(sw.paneTree, sw.id);
      paneCwdStore.update((store) => mergePaneCwds(store, cwds));
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

/** 保存当前工作区。优先使用工作区已命名的名字作为 history 条目�?/ .ridge 文件名；
 *  仅当工作区未命名时由后端 fallback 到时间戳。这�?cwd / 布局变更触发的自�?checkpoint
 *  会按用户给的工作区名归档，而不是堆出一串时间戳�?*/
export async function saveCurrentWorkspace() {
  if (!isTauri()) return;
  try {
    const activeId = get(activeWorkspaceId);
    const names = get(workspaceNames);
    const name = activeId ? names[activeId]?.trim() : '';
    await invoke('save_workspace', name ? { name } : {});
    await loadSavedWorkspaces();
  } catch (e) {
    console.error('saveCurrentWorkspace', e);
  }
}

/** 删除已保存的工作�?*/
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
