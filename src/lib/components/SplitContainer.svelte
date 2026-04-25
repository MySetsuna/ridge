<script lang="ts">
  import { get } from 'svelte/store';
  import { onDestroy } from 'svelte';
  import Pane from './Pane.svelte';
  import SplitLayout from './SplitContainer.svelte';
  import { Splitpanes, Pane as SPane } from 'svelte-splitpanes';
  import { isTauri } from '@tauri-apps/api/core';
  import { Bot, History } from 'lucide-svelte';
  import { openClaudeAgentLauncher } from './ClaudeAgentLauncher.svelte';
  import { alertDialog } from './WindDialog.svelte';
  import { openScrollbackHistory } from './ScrollbackHistoryModal.svelte';
  import { trackPaneGitStatus } from '$lib/stores/paneGitStatus';
  import PaneGitPill from './PaneGitPill.svelte';
  import PaneDiffPill from './PaneDiffPill.svelte';
  import PaneRepoSwitcher from './PaneRepoSwitcher.svelte';
  import { settingsStore } from '$lib/stores/settings';
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
    getAllPaneIds,
    closePane as closePaneApi,
    activePaneId,
    paneDragSourceId,
    dockPane,
    persistSplitRatios,
    persistSplitRatiosBatch,
    splitResizeUiState,
    queueSplitResizeJunction,
    clearSplitResizeUi,
    startSplitResizeDrag,
    updateSplitResizeDrag,
    finishSplitResizeDrag,
    SAME_AXIS_ATTRACT_PX,
    findJunctionsNearPosition,
    registerJunction,
    clearJunctionRegistry,
    findSameAxisRefs,
    terminalTitles,
    paneCwdStore,
    paneForegroundProcessStore,
    collapseCwd,
  } from '$lib/stores/paneTree';

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

  let leafCount = $derived(getAllPaneIds($paneTreeStore).length);
  const splitPathKey = $derived(splitPath.join('/'));
  const splitAxis = $derived(
    node.type === 'split' ? (node.direction === 'horizontal' ? 'x' : 'y') : ''
  );

  /** 当前叶节点上的停靠预览（仅拖拽他格悬停时）。 */
  let dockHover: DockRegion | null = $state(null);

  /**
   * svelte-splitpanes: horizontal=true → flex 纵向 → 上下分屏（横条分割）；
   * horizontal=false → flex 横向 → 左右分屏（竖条分割）。
   * 与后端：vertical → 上下；horizontal → 左右。
   */
  function splitpanesHorizontal(dir: 'horizontal' | 'vertical'): boolean {
    return dir === 'vertical';
  }

  async function onClosePane(id: string) {
    try {
      await closePaneApi(id);
    } catch (e) {
      console.error(e);
      await alertDialog({ title: '操作失败', message: e instanceof Error ? e.message : String(e), danger: true });
    }
  }

  function regionAtPoint(
    clientX: number,
    clientY: number,
    el: HTMLElement
  ): DockRegion {
    const r = el.getBoundingClientRect();
    const x = (clientX - r.left) / Math.max(r.width, 1);
    const y = (clientY - r.top) / Math.max(r.height, 1);
    const m = 0.18;
    if (x < m) return 'left';
    if (x > 1 - m) return 'right';
    if (y < m) return 'top';
    if (y > 1 - m) return 'bottom';
    return 'center';
  }

  function dockHintClass(h: DockRegion | null): string {
    if (!h) return '';
    if (h === 'left') return 'shadow-[inset_5px_0_0_0_var(--wf-accent)]';
    if (h === 'right') return 'shadow-[inset_-5px_0_0_0_var(--wf-accent)]';
    if (h === 'top') return 'shadow-[inset_0_5px_0_0_var(--wf-accent)]';
    if (h === 'bottom') return 'shadow-[inset_0_-5px_0_0_var(--wf-accent)]';
    return 'ring-2 ring-[var(--wf-accent)] ring-inset';
  }

  type SplitPaneSizeEvent = {
    min: number;
    max: number;
    size: number;
    snap: number;
  };

  function onSplitResized(e: CustomEvent<SplitPaneSizeEvent[]>) {
    if (node.type !== 'split') return;
    if ($splitResizeUiState.phase === 'drag') return;
    const panes = e.detail;
    if (!panes?.length || panes.length !== node.children.length) return;
    const sizes = panes.map((p) => p.size);
    void persistSplitRatios(splitPath, sizes);
  }

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
    return splitHost.querySelector<HTMLElement>(':scope > .wf-split');
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
        '.wf-split > .splitpanes__splitter'
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
    if (ui.phase === 'junction') {
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
      if (updates.length) await persistSplitRatiosBatch(updates);
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
    ).map((c) => c.ref);
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
      if (rel.closest('.wf-split')) return;
    }
    if (splitEngaged(splitPath, $splitResizeUiState)) clearSplitResizeUi();
  }

  function onSplitMouseDown(e: MouseEvent) {
    if (node.type !== 'split' || e.button !== 0) return;
    if (!isSplitterTargetInHost(e.target)) return;
    if ($splitResizeUiState.phase !== 'junction') return;
    if (!splitHighlighted(splitPath, $splitResizeUiState)) return;
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
      splitHost.querySelector<HTMLElement>(':scope > .wf-split');
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

  async function onDockDrop(e: DragEvent, targetId: string) {
    e.preventDefault();
    const src = get(paneDragSourceId);
    dockHover = null;
    if (!src || src === targetId) return;
    const t = e.currentTarget;
    if (!(t instanceof HTMLElement)) return;
    const region = regionAtPoint(e.clientX, e.clientY, t);
    try {
      await dockPane(src, targetId, region);
    } catch (err) {
      console.error(err);
      await alertDialog({ title: '操作失败', message: err instanceof Error ? err.message : String(err), danger: true });
    }
  }
</script>

<div
  bind:this={splitHost}
  class="h-full w-full min-h-0 min-w-0"
  role="presentation"
  onmousemove={onSplitMouseMove}
  onmouseleave={onSplitMouseLeave}
>
  <Splitpanes
    theme=""
    horizontal={node.type === 'split'
      ? splitpanesHorizontal(node.direction)
      : false}
    class="wf-split h-full w-full min-h-0 min-w-0 bg-[var(--wf-bg)] {splitHighlighted(
      splitPath,
      $splitResizeUiState
    )
      ? 'wf-split--junction'
      : ''} {splitDragging(splitPath, $splitResizeUiState)
      ? 'wf-split--junction-dragging'
      : ''} {splitAligned(splitPath, $splitResizeUiState)
      ? 'wf-split--aligned'
      : ''}"
    on:resized={onSplitResized}
  >
    {#if node.type === 'leaf'}
      <SPane>
        <div
          class="relative flex flex-col h-full min-h-0 min-w-0 overflow-hidden bg-[var(--wf-surface)]/90 shadow-[0_8px_32px_rgba(0,0,0,0.35)] backdrop-blur-md"
        >
          {#if $paneDragSourceId && $paneDragSourceId !== node.id}
            <div
              class="absolute inset-0 z-30 rounded-lg bg-black/25 transition-shadow {dockHintClass(
                dockHover
              )}"
              role="region"
              aria-label="将窗格停靠到此处"
              ondragover={(e) => {
                e.preventDefault();
                if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
                const t = e.currentTarget;
                if (t instanceof HTMLElement) {
                  dockHover = regionAtPoint(e.clientX, e.clientY, t);
                }
              }}
              ondragleave={(e) => {
                const rel = e.relatedTarget;
                const cur = e.currentTarget;
                if (
                  cur instanceof HTMLElement &&
                  rel instanceof Node &&
                  !cur.contains(rel)
                ) {
                  dockHover = null;
                }
              }}
              ondrop={(e) => onDockDrop(e, node.id)}
            ></div>
          {/if}
          <header
            class="wf-pane-header flex items-center justify-between gap-2 px-3 h-9 shrink-0 border-b border-[var(--wf-border)] bg-[var(--wf-glass)] backdrop-blur-md z-10"
          >
            <div
              class="flex-1 min-w-0 cursor-grab active:cursor-grabbing py-1 -my-1 select-none"
              draggable="true"
              title="拖拽到其它窗格：靠边分屏，靠中间与目标互换"
              onclick={() => activePaneId.set(node.id)}
              onkeydown={(e) => e.key === 'Enter' && activePaneId.set(node.id)}
              role="presentation"
              ondragstart={(e) => {
                e.dataTransfer?.setData('text/plain', node.id);
                if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
                paneDragSourceId.set(node.id);
              }}
              ondragend={() => {
                paneDragSourceId.set(null);
                dockHover = null;
              }}
            >
              {#if node.id !== undefined}
                {@const proc = $paneForegroundProcessStore[node.id]}
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
                      title={agentId ? `Claude Code agent 运行中：${agentId}` : 'teammate agent 运行中'}
                    >
                      <span class="inline-block h-1.5 w-1.5 rounded-full bg-emerald-400 animate-pulse"></span>
                      AGENT
                    </span>
                    {#if agentId}
                      <span class="text-emerald-300/80 truncate max-w-[120px]" title={agentId}>
                        {agentId}
                      </span>
                      <span class="text-[var(--wf-title-sep)] select-none">·</span>
                    {/if}
                  {:else if agentState === 'starting'}
                    <span
                      class="flex items-center gap-1 shrink-0 rounded-full bg-amber-500/15 text-amber-300 border border-amber-400/40 px-1.5 h-4 text-[9px] font-semibold uppercase tracking-wider"
                      title="teammate pane 启动中"
                    >
                      <span class="inline-block h-1.5 w-1.5 rounded-full bg-amber-400 animate-pulse"></span>
                      STARTING
                    </span>
                  {/if}
                  {#if proc}
                    <span class="text-[var(--wf-title-proc)] font-semibold truncate">{proc}</span>
                  {/if}
                  {#if proc && displayCwd}
                    <span class="text-[var(--wf-title-sep)] select-none">·</span>
                  {/if}
                  {#if displayCwd}
                    <span class="text-[var(--wf-title-cwd)] truncate">{displayCwd}</span>
                  {:else if !proc}
                    {#if node.title}
                      <span class="text-[var(--wf-fg)] truncate">{node.title}</span>
                    {:else}
                      <span class="text-[var(--wf-fg-muted)]">终端</span>
                    {/if}
                  {/if}
                </span>
              {/if}
            </div>
            {#if node.type === 'leaf'}
              {@const leafAgentState = node.agent_state}
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
              <!-- "Run Claude Code here" button — seeds a teammate agent on this
                   pane and kicks `claude` in the PTY. Busy panes hide the button
                   so users don't stack agents; click again releases + relaunches
                   only via the backend release_teammate_agent path.
                   Hidden entirely when the Claude Code extension is disabled —
                   the user gets a clean header without the Bot affordance. -->
              {#if $settingsStore.claudeExtensionEnabled}
              <button
                type="button"
                title={leafAgentState === 'busy'
                  ? '此窗格已有 agent 运行'
                  : '在此窗格启动 Claude Code agent（Shift-Click 直接启动）'}
                disabled={leafAgentState === 'busy' || !isTauri()}
                class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--wf-fg-muted)] transition-colors hover:bg-emerald-500/10 hover:text-emerald-300 disabled:opacity-25 disabled:pointer-events-none"
                onclick={(e) => {
                  if (!isTauri()) return;
                  // Shift / Alt-Click skips the prompt modal and launches bare
                  // `claude` directly — matches the round-10 behaviour for
                  // users who've already memorised the shortcut.
                  openClaudeAgentLauncher(node.id, e.shiftKey || e.altKey);
                }}
              >
                <Bot class="h-3.5 w-3.5" />
              </button>
              {/if}
              <!-- History browser — read-only viewer for bytes that scrolled past
                   xterm's own 8000-line buffer. Lives in a modal because the
                   pane header is already busy with branch / agent affordances. -->
              <button
                type="button"
                title="查看终端历史记录（包含已滚出 xterm 视窗的早期输出）"
                disabled={!isTauri()}
                class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--wf-fg-muted)] transition-colors hover:bg-[var(--wf-accent)]/10 hover:text-[var(--wf-accent)] disabled:opacity-25 disabled:pointer-events-none"
                onclick={() => openScrollbackHistory(node.id)}
              >
                <History class="h-3.5 w-3.5" />
              </button>
            {/if}
            <button
              type="button"
              title={leafCount <= 1 ? '至少保留一个窗格' : '关闭此窗格'}
              disabled={leafCount <= 1}
              class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--wf-fg-muted)] text-base leading-none transition-colors hover:bg-white/[0.06] hover:text-[var(--wf-fg)] disabled:opacity-25 disabled:pointer-events-none"
              onclick={() => onClosePane(node.id)}
            >
              ×
            </button>
          </header>
          <div class="flex-1 min-h-0 min-w-0">
            <Pane paneId={node.id} {workspaceId} />
          </div>
        </div>
      </SPane>
    {:else}
      {#each node.children as child, i (child.id)}
        <SPane size={node.ratios?.[i] ?? 100 / node.children.length}>
          <SplitLayout
            node={child}
            {workspaceId}
            splitPath={[...splitPath, i]}
          />
        </SPane>
      {/each}
    {/if}
  </Splitpanes>
</div>

<style>
  /*
   * 去掉 default-theme 的宽白条；布局上仅占 1px（padding+负 margin 扩大拖拽命中区）。
   * 常态：主题色细线；悬停 / 拖拽：scale 加粗为拖拽条，不改变分屏比例。
   */
  :global(.wf-split.splitpanes--vertical) > :global(.splitpanes__splitter) {
    box-sizing: content-box;
    width: 1px;
    min-width: 0;
    padding: 0 5px;
    margin: 0 -5px;
    border: none;
    background: transparent;
    cursor: col-resize;
    position: relative;
    z-index: 1;
    flex-shrink: 0;
    overflow: visible;
  }
  :global(.wf-split.splitpanes--vertical)
    > :global(.splitpanes__splitter::after) {
    content: none;
  }
  :global(.wf-split.splitpanes--vertical)
    > :global(.splitpanes__splitter::before) {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 1px;
    transform: translateX(-50%) scaleX(1);
    transform-origin: center;
    background: var(--wf-border);
    border-radius: 1px;
    transition:
      transform 0.12s ease,
      background 0.12s ease,
      box-shadow 0.12s ease;
    pointer-events: none;
    box-shadow: none;
  }
  :global(.wf-split.splitpanes--vertical)
    > :global(.splitpanes__splitter:hover::before),
  :global(.wf-split.splitpanes--vertical.splitpanes--dragging)
    > :global(.splitpanes__splitter::before) {
    transform: translateX(-50%) scaleX(4);
    background: var(--wf-accent);
    box-shadow: 0 0 12px var(--wf-accent-glow);
  }

  :global(.wf-split.splitpanes--horizontal) > :global(.splitpanes__splitter) {
    box-sizing: content-box;
    height: 1px;
    min-height: 0;
    padding: 5px 0;
    margin: -5px 0;
    border: none;
    background: transparent;
    cursor: row-resize;
    position: relative;
    z-index: 1;
    flex-shrink: 0;
    overflow: visible;
  }
  :global(.wf-split.splitpanes--horizontal)
    > :global(.splitpanes__splitter::after) {
    content: none;
  }
  :global(.wf-split.splitpanes--horizontal)
    > :global(.splitpanes__splitter::before) {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 1px;
    transform: translateY(-50%) scaleY(1);
    transform-origin: center;
    background: var(--wf-border);
    border-radius: 1px;
    transition:
      transform 0.12s ease,
      background 0.12s ease,
      box-shadow 0.12s ease;
    pointer-events: none;
    box-shadow: none;
  }
  :global(.wf-split.splitpanes--horizontal)
    > :global(.splitpanes__splitter:hover::before),
  :global(.wf-split.splitpanes--horizontal.splitpanes--dragging)
    > :global(.splitpanes__splitter::before) {
    transform: translateY(-50%) scaleY(4);
    background: var(--wf-accent);
    box-shadow: 0 0 12px var(--wf-accent-glow);
  }

  :global(.wf-split.wf-split--junction)
    > :global(.splitpanes__splitter::before) {
    background: var(--wf-accent);
    box-shadow: 0 0 10px var(--wf-accent-glow);
  }

  :global(.wf-split.wf-split--junction-dragging)
    > :global(.splitpanes__splitter::before) {
    background: var(--wf-accent);
    box-shadow: 0 0 14px var(--wf-accent-glow);
  }

  /* 同向吸附对齐：高亮加亮、阴影更厚，区别于普通 4-way junction。 */
  :global(.wf-split.wf-split--aligned)
    > :global(.splitpanes__splitter::before) {
    background: var(--wf-accent);
    box-shadow: 0 0 16px var(--wf-accent-glow);
  }

  :global(body.wf-resize-junction-cursor),
  :global(body.wf-resize-junction-cursor .splitpanes__splitter),
  :global(body.wf-resize-junction-cursor .splitpanes__splitter *) {
    cursor: move !important;
  }
</style>
