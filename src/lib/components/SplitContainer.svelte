<script lang="ts">
  import { get } from 'svelte/store';
  import { onDestroy } from 'svelte';
  import Pane from './RidgePane.svelte';
  import SplitLayout from './SplitContainer.svelte';
  // T20 重做：弃用 svelte-splitpanes，改用自家 @ridge/split。后者纯渲染，无内部
  // 状态机，paneTreeStore.ratios 是唯一真相源；Ridge 的 startSplitResizeDrag /
  // updateSplitResizeDrag 写 ratios 后能立刻经 RgPane 的 size prop 反映到 DOM，
  // 不再被库内部 sz 状态覆盖。Class 名 `splitpanes__pane`/`splitpanes__splitter`
  // 通过 class prop forward，沿用 findSameAxisRefs 等查询逻辑。
  import { t, tr } from '$lib/i18n';
  import { Bot } from 'lucide-svelte';
  import { RgSplit, RgPane, RgSplitter } from '@ridge/split';
  import { isTauri, invoke } from '@tauri-apps/api/core';
  import { settingsStore } from '$lib/stores/settings';
  import { TerminalManager } from '$lib/terminal/manager';
  import { alertDialog } from './RidgeDialog.svelte';
  import { trackPaneGitStatus } from '$lib/stores/paneGitStatus';
  import PaneGitPill from './PaneGitPill.svelte';
  import PaneDiffPill from './PaneDiffPill.svelte';
  import PaneRepoSwitcher from './PaneRepoSwitcher.svelte';
  import PaneShellSwitcher from './PaneShellSwitcher.svelte';
  import type { PaneNode } from '$lib/types';
  import type {
    DockRegion,
    SplitResizeUiState,
    SplitterRef,
    JunctionRef,
    JunctionSnapState,
  } from '$lib/stores/paneTree';
import {
    paneTreeStore,
    workspacePaneTrees,
    getAllPaneIds,
    closePane as closePaneApi,
    activePaneId,
    paneDragSourceId,
    paneDockHover,
    activeWorkspaceId,
    paneCwdStore,
    terminalTitles,
    paneForegroundProcessStore,
    persistSplitRatios,
    persistSplitRatiosBatch,
    splitResizeUiState,
    queueSplitResizeJunction,
    clearSplitResizeUi,
    startSplitResizeDrag,
    updateSplitResizeDrag,
    finishSplitResizeDrag,
    paneIdsFromRatioUpdates,
    SAME_AXIS_ATTRACT_PX,
    pointerInCoupleZone,
    findJunctionsNearPosition,
    findSameAxisRefs,
    collapseCwd,
  } from '$lib/stores/paneTree';
  import { paneDockDrag } from '$lib/actions/paneDockDrag';


  interface Props {
    node: PaneNode;
    workspaceId: string;
    /** 从根到当前 `Split` 的子下标路径（用于与后端 `set_split_ratios_at_path` 对齐）。 */
    splitPath?: number[];
  }
  let { node, workspaceId, splitPath = [] }: Props = $props();

  // Feed per-pane git status tracking on every cwd change. Runs only when
  // this SplitContainer frame holds a leaf; for `split` frames the children
  // recursion handles their own leaves.
  $effect(() => {
    if (node.type !== 'leaf') return;
    const cwd = $paneCwdStore[`${workspaceId}:${node.id}`] ?? '';
    trackPaneGitStatus(node.id, cwd || null);
  });
  let splitHost: HTMLElement | undefined;
  let dragMoveUnlisten: (() => void) | undefined;
  let dragUpUnlisten: (() => void) | undefined;
  const ORTHOGONAL_TRIGGER_PX = 8;

  // 手动把一个分屏标记为/取消智能体（Domain Zero「手动注册」入口）。已注册
  // (agent_state==='busy') → 调 release_teammate_agent；否则 register_teammate_agent，
  // agentId 取终端标题/前台进程名（缺省 'agent'）。后端会 emit teammate-layout-changed
  // → 重渲染徽章；指挥部 Tab 轮询 get_teammate_topology 即收入花名册。
  async function toggleTeammateAgent(leafId: string, isAgent: boolean): Promise<void> {
    try {
      if (isAgent) {
        await invoke('release_teammate_agent', { workspaceId, paneId: leafId });
      } else {
        const agentId =
          get(terminalTitles)[leafId] || get(paneForegroundProcessStore)[leafId] || 'agent';
        await invoke('register_teammate_agent', { workspaceId, paneId: leafId, agentId });
      }
    } catch (e) {
      await alertDialog({
        title: tr('workspace.opFailed'),
        message: e instanceof Error ? e.message : String(e),
        danger: true,
      });
    }
  }

  // §4a workspace keep-alive: count from THIS workspace's tree, not the
  // global active one. Falls back to paneTreeStore on first paint before
  // workspacePaneTrees is populated.
  let leafCount = $derived(
    getAllPaneIds($workspacePaneTrees.get(workspaceId) ?? $paneTreeStore).length
  );
  const splitPathKey = $derived(splitPath.join('/'));
  const splitAxis = $derived(
    node.type === 'split' ? (node.direction === 'horizontal' ? 'x' : 'y') : ''
  );

  /**
   * svelte-splitpanes: horizontal=true → flex 纵向 → 上下分屏（横条分割）；
   * horizontal=false → flex 横向 → 左右分屏（竖条分割）。
   * 与后端：vertical → 上下；horizontal → 左右。
   */
  // T20：splitpanesHorizontal 已不再需要（@ridge/split 直接吃 'horizontal' / 'vertical'）。

  async function onClosePane(id: string) {
    try {
      await closePaneApi(id);
    } catch (e) {
      console.error(e);
      await alertDialog({ title: tr('workspace.opFailed'), message: e instanceof Error ? e.message : String(e), danger: true });
    }
  }

  // 返回方向半区预览块的定位 class：明确显示拖拽 pane 将落入的区域。
  function dockRegionClass(h: DockRegion | null): string {
    if (!h) return '';
    if (h === 'left') return 'inset-y-0 left-0 w-1/2';
    if (h === 'right') return 'inset-y-0 right-0 w-1/2';
    if (h === 'top') return 'inset-x-0 top-0 h-1/2';
    if (h === 'bottom') return 'inset-x-0 bottom-0 h-1/2';
    return 'inset-[20%]';
  }

  // T20：原 onSplitResized 监听 svelte-splitpanes 的 'resized' event 落盘。
  // 现已弃用 svelte-splitpanes，松手后由 bindDragListeners.onUp 直接 await
  // persistSplitRatiosBatch 持久化，事件桥不再需要。

  function parsePathKey(key: string | null | undefined): number[] {
    if (!key) return [];
    if (!key.length) return [];
    return key
      .split('/')
      .map((seg) => Number(seg))
      .filter((n) => Number.isFinite(n));
  }

  function pathEqual(a: number[], b: number[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i += 1) {
      if (a[i] !== b[i]) return false;
    }
    return true;
  }

  function refEqual(a: SplitterRef, b: SplitterRef): boolean {
    return (
      a.axis === b.axis &&
      a.splitterIndex === b.splitterIndex &&
      pathEqual(a.splitPath, b.splitPath)
    );
  }

  function isSplitterTargetInHost(
    target: EventTarget | null
  ): target is HTMLElement {
    if (!(target instanceof HTMLElement) || !splitHost) return false;
    const splitter = target.closest('.splitpanes__splitter');
    return !!splitter && splitHost.contains(splitter);
  }

  function getLocalSplitRoot(): HTMLElement | null {
    if (!splitHost) return null;
    return splitHost.querySelector<HTMLElement>(':scope > .rg-split');
  }

  function pointToRectDistance(
    clientX: number,
    clientY: number,
    rect: DOMRect
  ): number {
    const dx =
      clientX < rect.left
        ? rect.left - clientX
        : clientX > rect.right
          ? clientX - rect.right
          : 0;
    const dy =
      clientY < rect.top
        ? rect.top - clientY
        : clientY > rect.bottom
          ? clientY - rect.bottom
          : 0;
    return Math.sqrt(dx * dx + dy * dy);
  }

  function splitRefsFromHost(target: HTMLElement): SplitterRef | null {
    if (node.type !== 'split') return null;
    const splitter = target.closest('.splitpanes__splitter');
    if (!(splitter instanceof HTMLElement)) return null;
    const splitRoot = splitter.parentElement;
    if (!(splitRoot instanceof HTMLElement)) return null;
    const splitters = Array.from(
      splitRoot.querySelectorAll<HTMLElement>(':scope > .splitpanes__splitter')
    );
    const splitterIndex = splitters.indexOf(splitter);
    if (splitterIndex < 0) return null;
    const axis = node.direction === 'horizontal' ? 'x' : 'y';
    const basisPx = Math.max(
      1,
      axis === 'x' ? splitRoot.clientWidth : splitRoot.clientHeight
    );
    return {
      splitPath: splitPath.slice(),
      splitterIndex,
      axis,
      basisPx,
    };
  }

  function findOrthogonalRefs(
    pointer: { x: number; y: number },
    primary: SplitterRef
  ): SplitterRef[] {
    if (typeof document === 'undefined') return [];
    const allSplitters = Array.from(
      document.querySelectorAll<HTMLElement>(
        '.rg-split > .splitpanes__splitter'
      )
    );
    const candidates: { ref: SplitterRef; distance: number }[] = [];
    for (const splitter of allSplitters) {
      const splitRoot = splitter.parentElement;
      if (!(splitRoot instanceof HTMLElement)) continue;
      const axisAttr = splitRoot.dataset.splitAxis;
      if (axisAttr !== 'x' && axisAttr !== 'y') continue;
      if (axisAttr === primary.axis) continue;
      const path = parsePathKey(splitRoot.dataset.splitPath);
      const splitters = Array.from(
        splitRoot.querySelectorAll<HTMLElement>(
          ':scope > .splitpanes__splitter'
        )
      );
      const splitterIndex = splitters.indexOf(splitter);
      if (splitterIndex < 0) continue;
      const basisPx = Math.max(
        1,
        axisAttr === 'x' ? splitRoot.clientWidth : splitRoot.clientHeight
      );
      const ref: SplitterRef = {
        splitPath: path,
        splitterIndex,
        axis: axisAttr,
        basisPx,
      };
      if (refEqual(ref, primary)) continue;
      const distance = pointToRectDistance(
        pointer.x,
        pointer.y,
        splitter.getBoundingClientRect()
      );
      if (distance <= ORTHOGONAL_TRIGGER_PX) candidates.push({ ref, distance });
    }
    candidates.sort((a, b) => a.distance - b.distance);
    // 仅联动最近的一条正交分割线，避免多目标竞争导致布局“漂移”。
    return candidates.slice(0, 1).map((x) => x.ref);
  }

  function splitHighlighted(path: number[], ui: SplitResizeUiState): boolean {
    if (ui.phase === 'drag') return false;
    if (ui.phase === 'junction' || ui.phase === 'pending') {
      if (pathEqual(path, ui.primary.splitPath)) return true;
      return ui.orthogonals.some((r) => pathEqual(path, r.splitPath));
    }
    return false;
  }

  function splitEngaged(path: number[], ui: SplitResizeUiState): boolean {
    if (ui.phase === 'pending') {
      if (pathEqual(path, ui.primary.splitPath)) return true;
      return ui.orthogonals.some((r) => pathEqual(path, r.splitPath));
    }
    return splitHighlighted(path, ui);
  }

  function splitDragging(path: number[], ui: SplitResizeUiState): boolean {
    if (ui.phase !== 'drag') return false;
    return ui.snapshots.some((s) => pathEqual(path, s.ref.splitPath));
  }

  function splitAligned(path: number[], ui: SplitResizeUiState): boolean {
    if (ui.phase !== 'pending' && ui.phase !== 'junction') return false;
    if (ui.sameAxisCandidates.length === 0) return false;
    if (pathEqual(path, ui.primary.splitPath)) return true;
    return ui.sameAxisCandidates.some((r) => pathEqual(path, r.splitPath));
  }

  function bindDragListeners() {
    if (dragMoveUnlisten || dragUpUnlisten) return;
    let latestPointer: { x: number; y: number } | null = null;
    let rafId: number | null = null;

    const onMove = (ev: MouseEvent) => {
      latestPointer = { x: ev.clientX, y: ev.clientY };
      if (rafId == null) {
        rafId = requestAnimationFrame(() => {
          rafId = null;
          if (latestPointer) {
            updateSplitResizeDrag(latestPointer);
          }
        });
      }
    };
    const onUp = async (ev: MouseEvent) => {
      if (rafId != null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      if (latestPointer) {
        // Flush final position using real mouseup coords for drop accuracy.
        updateSplitResizeDrag({ x: ev.clientX, y: ev.clientY });
      }
      latestPointer = null;
      const updates = finishSplitResizeDrag();
      unbindDragListeners();
      if (updates.length) {
        await persistSplitRatiosBatch(updates);
        // §pane-resize-reflow (2026-05-09): refresh only the panes
        // affected by this split resize drag.
        const tree = $workspacePaneTrees.get(workspaceId) ?? $paneTreeStore;
        const affectedIds = paneIdsFromRatioUpdates(tree, updates);
        TerminalManager.instance().forceFullRedrawFor(affectedIds);
      }
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp, { once: true });
    dragMoveUnlisten = () => {
      window.removeEventListener('mousemove', onMove);
      if (rafId != null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
    };
    dragUpUnlisten = () => window.removeEventListener('mouseup', onUp);
  }

  function unbindDragListeners() {
    dragMoveUnlisten?.();
    dragUpUnlisten?.();
    dragMoveUnlisten = undefined;
    dragUpUnlisten = undefined;
  }

  function onSplitMouseMove(e: MouseEvent) {
    if (node.type !== 'split') return;
    if ($splitResizeUiState.phase === 'drag') return;
    if (!isSplitterTargetInHost(e.target)) {
      if (
        $splitResizeUiState.phase !== 'idle' &&
        splitEngaged(splitPath, $splitResizeUiState)
      ) {
        clearSplitResizeUi();
      }
      return;
    }
    const target = e.target as HTMLElement;
    const localSplitRoot = getLocalSplitRoot();
    if (!localSplitRoot) return;
    const localSplitter = target.closest('.splitpanes__splitter');
    if (
      !(localSplitter instanceof HTMLElement) ||
      localSplitter.parentElement !== localSplitRoot
    ) {
      return;
    }
    const primary = splitRefsFromHost(target);
    if (!primary) return;
    const orthogonals = findOrthogonalRefs(
      { x: e.clientX, y: e.clientY },
      primary
    );
    const sameAxisCandidates = findSameAxisRefs(
      primary,
      SAME_AXIS_ATTRACT_PX
    ).map((c: any) => c.ref);
    if (!orthogonals.length && !sameAxisCandidates.length) {
      if (splitEngaged(splitPath, $splitResizeUiState)) clearSplitResizeUi();
      return;
    }
    queueSplitResizeJunction(
      primary,
      orthogonals,
      { x: e.clientX, y: e.clientY },
      sameAxisCandidates
    );
  }

  function onSplitMouseLeave(e: MouseEvent) {
    if ($splitResizeUiState.phase === 'drag') return;
    const rel = e.relatedTarget;
    if (rel instanceof Element) {
      if (rel.closest('.rg-split')) return;
    }
    if (splitEngaged(splitPath, $splitResizeUiState)) clearSplitResizeUi();
  }

  function onSplitMouseDown(e: MouseEvent) {
    if (node.type !== 'split' || e.button !== 0) return;
    if (!isSplitterTargetInHost(e.target)) return;
    const target = e.target as HTMLElement;
    // Nested SplitContainers all attach capture-phase mousedown listeners.
    // Only the SplitContainer that DIRECTLY owns the splitter (its immediate
    // .rg-split parent) should run; outer ancestors bail here so capture
    // continues down to the right one.
    const localSplitRoot = getLocalSplitRoot();
    if (!localSplitRoot) return;
    const localSplitter = target.closest('.splitpanes__splitter');
    if (
      !(localSplitter instanceof HTMLElement) ||
      localSplitter.parentElement !== localSplitRoot
    ) {
      return;
    }
    const ui = get(splitResizeUiState);
    // Reuse hover-built junction state when present; otherwise synthesise a
    // junction state on the spot so basic drag works for splitters that have
    // no nearby orthogonal / same-axis siblings (the hover flow never trips
    // queueSplitResizeJunction in that case, leaving phase === 'idle').
    if (ui.phase !== 'junction' || !splitHighlighted(splitPath, ui)) {
      const primary = splitRefsFromHost(target);
      if (!primary) return;
      const orthogonals = findOrthogonalRefs(
        { x: e.clientX, y: e.clientY },
        primary
      );
      const sameAxisCandidates = findSameAxisRefs(
        primary,
        SAME_AXIS_ATTRACT_PX
      ).map((c: any) => c.ref);
      splitResizeUiState.set({
        phase: 'junction',
        primary,
        orthogonals,
        sameAxisCandidates,
        pointer: { x: e.clientX, y: e.clientY },
        snapState: null,
      });
    }
    e.preventDefault();
    e.stopPropagation();
    startSplitResizeDrag({ x: e.clientX, y: e.clientY });
    bindDragListeners();
  }

  onDestroy(() => {
    unbindDragListeners();
    if (splitEngaged(splitPath, get(splitResizeUiState))) {
      clearSplitResizeUi();
    }
  });

  $effect(() => {
    if (!splitHost || node.type !== 'split') return;
    const splitRoot =
      splitHost.querySelector<HTMLElement>(':scope > .rg-split');
    if (!splitRoot) return;
    splitRoot.dataset.splitPath = splitPathKey;
    splitRoot.dataset.splitAxis = splitAxis;
    return () => {
      delete splitRoot.dataset.splitPath;
      delete splitRoot.dataset.splitAxis;
    };
  });

  $effect(() => {
    if (!splitHost) return;
    const handler = (e: MouseEvent) => onSplitMouseDown(e);
    splitHost.addEventListener('mousedown', handler, { capture: true });
    return () =>
      splitHost?.removeEventListener('mousedown', handler, { capture: true });
  });


</script>

<div
  bind:this={splitHost}
  class="h-full w-full min-h-0 min-w-0"
  role="presentation"
  onmousemove={onSplitMouseMove}
  onmouseleave={onSplitMouseLeave}
>
  <RgSplit
    direction={node.type === 'split' ? node.direction : 'horizontal'}
    class="rg-split h-full w-full min-h-0 min-w-0 bg-[var(--rg-bg)] {splitHighlighted(
      splitPath,
      $splitResizeUiState
    )
      ? 'rg-split--junction'
      : ''} {splitDragging(splitPath, $splitResizeUiState)
      ? 'rg-split--junction-dragging'
      : ''} {splitAligned(splitPath, $splitResizeUiState)
      ? 'rg-split--aligned'
      : ''}"
  >
    {#if node.type === 'leaf'}
      <RgPane size={100} class="splitpanes__pane">
        <!-- §4.3 Phase B (2026-05-07): removed `bg-[var(--rg-surface)]/90`
             and `backdrop-blur-md` from this wrapper. The 90 %-opaque
             surface tint sat on top of the global host canvas and hid
             every GPU-drawn pixel ("black screen" symptom). Per-pane
             Canvas2D fallback paints `--rg-term-bg` directly in its own
             child canvas so it doesn't need the wrapper tint either.
             Card outline stays via the box-shadow. -->
        <div
          data-pane-id={node.id}
          class="relative flex flex-col h-full min-h-0 min-w-0 overflow-hidden shadow-[0_8px_32px_rgba(0,0,0,0.35)]"
        >
          {#if $paneDragSourceId && $paneDragSourceId !== node.id}
            {@const hover = $paneDockHover && $paneDockHover.paneId === node.id ? $paneDockHover.region : null}
            <div
              class="absolute inset-0 z-30 rounded-lg bg-black/15 pointer-events-none"
              role="region"
              aria-label={$t('workspace.dockHereLabel')}
            >
              {#if hover}
                <div
                  class="absolute bg-[var(--rg-accent)]/25 border-2 border-[var(--rg-accent)] rounded transition-all duration-100 {dockRegionClass(hover)}"
                ></div>
              {/if}
            </div>
          {/if}
          <header
            class="rg-pane-header opacity-90 flex items-center justify-between gap-2 px-3 h-9 shrink-0 border-b border-[var(--rg-border)] bg-[var(--rg-glass)] backdrop-blur-md z-10"
          >
            <div
              class="flex-1 min-w-0 cursor-grab active:cursor-grabbing py-1 select-none"
              title={$t('workspace.paneDragTitle')}
              onkeydown={(e) => e.key === 'Enter' && activePaneId.set(node.id)}
              role="presentation"
              use:paneDockDrag={{ paneId: node.id }}
            >
              {#if node.id !== undefined}
                <!-- Title source: same as Explorer's pane tag. terminalTitles is
                     OSC (\x1b]0;...\x07) when set, else falls back to the polled
                     foreground process name. Reading the same store keeps the
                     workspace pane header and the sidebar pane chip in sync —
                     e.g. Claude Code's OSC title now shows in both places. -->
                {@const titleStr = $terminalTitles[node.id]}
                {@const fgProc = $paneForegroundProcessStore[node.id]}
                {@const proc = titleStr || fgProc}
                {@const rawCwd = $paneCwdStore[`${workspaceId}:${node.id}`]}
                {@const displayCwd = rawCwd ? collapseCwd(rawCwd) : ''}
                {@const agentState = node.type === 'leaf' ? node.agent_state : undefined}
                {@const agentId = node.type === 'leaf' ? node.agent_id : undefined}
                <span
                  class="flex items-center gap-1.5 text-[11px] font-mono tracking-wide truncate"
                >
                  {#if agentState === 'busy'}
                    <!-- Running teammate agent — green dot + label + agent_id.
                         Always the first glyph so orchestrators see it at a glance. -->
                    <span
                      class="flex items-center gap-1 shrink-0 rounded-full bg-emerald-500/15 text-emerald-300 border border-emerald-400/40 px-1.5 h-4 text-[9px] font-semibold uppercase tracking-wider"
                      title={agentId ? $t('workspace.agentRunning', { agentId }) : $t('workspace.teammateRunning')}
                    >
                      <span class="inline-block h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse"></span>
                      AGENT
                    </span>
                    {#if agentId}
                      <span class="text-emerald-300/80 truncate max-w-[120px]" title={agentId}>
                        {agentId}
                      </span>
                      <span class="text-[var(--rg-title-sep)] select-none">·</span>
                    {/if}
                  {:else if agentState === 'starting'}
                    <span
                      class="flex items-center gap-1 shrink-0 rounded-full bg-amber-500/15 text-amber-300 border border-amber-400/40 px-1.5 h-4 text-[9px] font-semibold uppercase tracking-wider"
                      title={$t('workspace.paneStarting')}
                    >
                      <span class="inline-block h-1.5 w-1.5 rounded-full bg-amber-400 animate-pulse"></span>
                      STARTING
                    </span>
                  {/if}
                  {#if proc}
                    <span class="text-[var(--rg-title-proc)] font-semibold truncate">{proc}</span>
                  {/if}
                  {#if proc && displayCwd}
                    <span class="text-[var(--rg-title-sep)] select-none">·</span>
                  {/if}
                  {#if displayCwd}
                    <span class="text-[var(--rg-title-cwd)] truncate">{displayCwd}</span>
                  {:else if !proc}
                    {#if node.title}
                      <span class="text-[var(--rg-fg)] truncate">{node.title}</span>
                    {:else}
                      <span class="text-[var(--rg-fg-muted)]">{$t('workspace.terminalFallback')}</span>
                    {/if}
                  {/if}
                </span>
              {/if}
            </div>
            {#if node.type === 'leaf'}
              <PaneShellSwitcher paneId={node.id} currentShell={node.shell_kind} />
              <!-- Repo switcher (renders only when cwd hosts >1 git repo);
                   then Branch pill (picker + ahead/behind + upstream warn);
                   then Diff pill (working-tree changed-file count + +N -N).
                   Splitting branch + diff mirrors VS Code's status bar; the
                   switcher in front lets the user pick which repo's data
                   the pair reflects when the cwd hosts multiple repos
                   (round-40 cwd-down semantics). -->
              <PaneRepoSwitcher paneId={node.id} />
              <PaneGitPill paneId={node.id} />
              <PaneDiffPill paneId={node.id} />
              {#if $settingsStore.teammateEnabled}
                {@const isAgent = node.agent_state === 'busy'}
                <button
                  type="button"
                  title={isAgent ? '取消标记智能体' : '把此分屏标记为智能体（纳入指挥部花名册）'}
                  class="flex h-7 w-7 items-center justify-center rounded-lg transition-colors {isAgent
                    ? 'text-emerald-400 hover:bg-emerald-500/10'
                    : 'text-[var(--rg-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--rg-fg)]'}"
                  onclick={() => toggleTeammateAgent(node.id, isAgent)}
                >
                  <Bot class="h-4 w-4" />
                </button>
              {/if}
            {/if}
            <button
              type="button"
              title={leafCount <= 1 ? $t('workspace.keepOnePane') : $t('workspace.closeThisPane')}
              disabled={leafCount <= 1}
              class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] text-base leading-none transition-colors hover:bg-white/[0.06] hover:text-[var(--rg-fg)] disabled:opacity-25 disabled:pointer-events-none"
              onclick={() => onClosePane(node.id)}
            >
              ×
            </button>
          </header>
          <div class="flex-1 min-h-0 min-w-0 z-79">
            <Pane paneId={node.id} {workspaceId} />
          </div>
        </div>
      </RgPane>
    {:else}
      {@const dim = node.direction === 'horizontal' ? 'width' : 'height'}
      {#each node.children as child, i (child.id)}
        {@const ratio = node.ratios?.[i] ?? 100 / node.children.length}
        <!-- T20：内联 div 替代 RgPane —— 排除 RgPane 子组件 props reactive
             链路问题。inline style 直接挂 SplitContainer 内联表达式，paneTreeStore
             更新后 svelte 一定立刻重写 style。 -->
        <div
          class="rg-pane splitpanes__pane relative"
          role="region"
          style="{dim}: {ratio}%; flex-grow: 0; flex-shrink: 0; min-width: 0; min-height: 0; overflow: hidden;"
        >
          <SplitLayout
            node={child}
            {workspaceId}
            splitPath={[...splitPath, i]}
          />
        </div>
        {#if i < node.children.length - 1}
          <RgSplitter class="splitpanes__splitter" />
        {/if}
      {/each}
    {/if}
  </RgSplit>
</div>

<style>
  /*
   * T20 重做：splitter 视觉与拖拽热区已迁移到 @ridge/split 的 RgSplitter；
   * 这里只覆盖 Ridge 业务态高亮（junction / aligned 状态），通过 ::before 加
   * 额外阴影叠加在子包的默认 scale-4 + accent 上。
   *
   * RgSplitter 默认用 `--rg-splitter-color` 为 idle 色、`--rg-splitter-active-color`
   * 为 hover/drag 色 —— Ridge 这里用 CSS var 接到主题：让 idle 用 --rg-border、
   * 激活态用 --rg-accent，与之前视觉一致。
   */
  :global(.rg-split) {
    --rg-splitter-color: var(--rg-border);
    --rg-splitter-active-color: var(--rg-accent);
    --rg-splitter-active-glow: var(--rg-accent-glow);
  }

  /*
   * §4.3 Phase B (2026-05-08): bump splitter above the pane header.
   * The header has `backdrop-blur-md` (creates a stacking context) and
   * `z-10`, both of which would otherwise occlude the splitter's hover/
   * drag glow at the top of the adjacent pane. RgSplitter's library
   * default is `z-index: 1`; override here so the splitter line stays
   * visible AND the wider hit area is reachable when the cursor is
   * over the header strip just below the boundary.
   */
  :global(.splitpanes__splitter),
  :global(.rg-split > .rg-splitter) {
    z-index: 80;
  }

  /* 业务高亮：rg-split--junction (4-way orthogonal hover) 和 rg-split--aligned
     (sameAxis sibling 在 BC 圆 15px 内) 都让该容器内同方向 splitter 完全高亮 ——
     与 RgSplitter 自己的 :hover 视觉一致 (scale 4 + accent)，避免出现 B "半高亮"。 */
  :global(.rg-split.rg-split--junction > .rg-splitter-col::before),
  :global(.rg-split.rg-split--aligned > .rg-splitter-col::before),
  :global(.rg-split.rg-split--junction-dragging > .rg-splitter-col::before) {
    transform: translateX(-50%) scaleX(4);
    background: var(--rg-accent);
    box-shadow: 0 0 12px var(--rg-accent-glow);
  }
  :global(.rg-split.rg-split--junction > .rg-splitter-row::before),
  :global(.rg-split.rg-split--aligned > .rg-splitter-row::before),
  :global(.rg-split.rg-split--junction-dragging > .rg-splitter-row::before) {
    transform: translateY(-50%) scaleY(4);
    background: var(--rg-accent);
    box-shadow: 0 0 12px var(--rg-accent-glow);
  }

  /* 拖动 / 4-way hover 时锁定 cursor，使其不随鼠标 hover 子元素变化。
     三种 cursor 对应三种 body 类，互斥；finishSplitResizeDrag 释放。 */
  :global(body.rg-resize-junction-cursor),
  :global(body.rg-resize-junction-cursor *) {
    cursor: move !important;
  }
  :global(body.rg-resize-col-cursor),
  :global(body.rg-resize-col-cursor *) {
    cursor: col-resize !important;
  }
  :global(body.rg-resize-row-cursor),
  :global(body.rg-resize-row-cursor *) {
    cursor: row-resize !important;
  }
</style>
