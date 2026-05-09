<script lang="ts">
  import { tick, untrack, type Snippet } from 'svelte';
  import {
    showContextMenu,
    type ContextMenuItem,
  } from '$lib/stores/contextMenu';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { dndzone, SOURCES, TRIGGERS } from 'svelte-dnd-action';
  import { paneDragSourceId } from '$lib/stores/paneTree';
  import { get } from 'svelte/store';

  interface WorkspaceInfo {
    id: string;
    index: number;
    name?: string;
    displaySeq: number;
  }

  interface Props {
    workspaces: WorkspaceInfo[];
    activeWorkspaceId: string;
    onSwitch: (id: string) => void;
    onClose: (id: string) => void;
    onReorder: (fromIndex: number, toIndex: number) => void;
    onRename: (id: string, name: string) => void;
    actions?: Snippet;
  }

  let {
    workspaces,
    activeWorkspaceId,
    onSwitch,
    onClose,
    onReorder,
    onRename,
    actions,
  }: Props = $props();

  // svelte-dnd-action 在交互期会替换 items 数组（插入 placeholder 等），
  // 我们用本地 mirror 存放可被 dndzone 直接改写的列表，外部 props
  // 变化时再同步过来。
  let localItems: WorkspaceInfo[] = $state([]);
  let dragInProgress = $state(false);

  // 同步外部 workspaces 到本地 mirror，但跳过"内容相同"的赋值：reorder 落位
  // 后 backend 往返结束会导致 workspaces 引用换新；如果这时无脑 [...workspaces]
  // 再写一次，svelte-dnd-action 会因 items 引用变化触发第二轮 FLIP，叠加在
  // 落位动画上产生肉眼可见的"名字闪烁"。
  $effect(() => {
    if (dragInProgress) return;
    const ws = workspaces;
    untrack(() => {
      if (workspacesEqual(localItems, ws)) return;
      localItems = [...ws];
    });
  });

  function workspacesEqual(a: WorkspaceInfo[], b: WorkspaceInfo[]): boolean {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      const x = a[i];
      const y = b[i];
      if (x.id !== y.id) return false;
      if (x.name !== y.name) return false;
      if (x.displaySeq !== y.displaySeq) return false;
    }
    return true;
  }

  let editingId: string | null = $state(null);
  let editingName: string = $state('');
  let renameInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (editingId !== null) {
      void tick().then(() => renameInput?.focus());
    }
  });

  function handleDndConsider(e: CustomEvent<{ items: WorkspaceInfo[]; info: { source: string; trigger: string } }>) {
    dragInProgress = true;
    localItems = e.detail.items;
  }

  function handleDndFinalize(e: CustomEvent<{ items: WorkspaceInfo[]; info: { source: string; trigger: string } }>) {
    dragInProgress = false;
    const next = e.detail.items;
    localItems = next;
    if (e.detail.info.source !== SOURCES.POINTER) return;
    // 对比新顺序与原顺序，找出第一个错位的位置作为 from/to。
    const oldIds = workspaces.map((w) => w.id);
    const newIds = next.map((w) => w.id);
    let fromIndex = -1;
    let toIndex = -1;
    for (let i = 0; i < newIds.length; i++) {
      if (newIds[i] !== oldIds[i]) {
        fromIndex = oldIds.indexOf(newIds[i]);
        toIndex = i;
        break;
      }
    }
    if (fromIndex >= 0 && toIndex >= 0 && fromIndex !== toIndex) {
      onReorder(fromIndex, toIndex);
    }
  }

  function handleContextMenu(e: MouseEvent, ws: WorkspaceInfo) {
    e.preventDefault();
    const items: ContextMenuItem[] = [
      {
        id: 'rename',
        label: '重命名',
        action: () => {
          editingId = ws.id;
          editingName = ws.name || `工作区 ${ws.displaySeq}`;
        },
      },
      { id: 'divider1', divider: true },
      {
        id: 'close',
        label: '关闭',
        disabled: workspaces.length <= 1,
        action: () => onClose(ws.id),
      },
    ];
    showContextMenu(e.clientX, e.clientY, items);
  }

  function handleRenameSubmit(wsId: string) {
    if (editingName.trim()) {
      onRename(wsId, editingName.trim());
    }
    editingId = null;
    editingName = '';
  }

  function handleRenameKeydown(e: KeyboardEvent, wsId: string) {
    if (e.isComposing) return;
    if (e.key === 'Enter') {
      handleRenameSubmit(wsId);
    } else if (e.key === 'Escape') {
      editingId = null;
      editingName = '';
    }
  }

  function getWorkspaceName(ws: WorkspaceInfo): string {
    return ws.name || `工作区 ${ws.displaySeq}`;
  }

  /** 浮动副本视觉强化 + 锁定 Y 轴：tab 拖拽时只能水平移动，Y 始终保持在
   *  tab 条的初始位置，从而不会脱离可放置区域。
   *
   *  svelte-dnd-action 通过 `transform: translate3d(x, y, 0)` 跟随指针，且没有
   *  内建的"轴锁"配置。这里用 MutationObserver 监听 `style` 变化，把库写入的
   *  Y 立刻覆盖回拖拽起点的 Y；X 维持库的值。`transformDraggedElement` 只在
   *  drag 起点调用一次，所以观察者必须在这里挂载。 */
  function transformDraggedTab(el: HTMLElement | undefined) {
    if (!el) return;
    el.style.transition = 'box-shadow 120ms ease-out, opacity 120ms ease-out';
    el.style.boxShadow = '0 12px 28px -6px rgba(0,0,0,0.45), 0 0 0 1px var(--rg-accent)';
    el.style.background = 'var(--rg-surface-2, var(--rg-surface))';
    el.style.opacity = '0.96';
    // 高于 pin 编辑器面板（z-60）；保持低于全部 modal 层。
    el.style.zIndex = '100';

    let lockedY: number | null = null;
    const observer = new MutationObserver(() => {
      const t = el.style.transform;
      const m = t.match(/translate3d\((-?[\d.]+)px,\s*(-?[\d.]+)px,\s*(-?[\d.]+)px\)/);
      if (!m) return;
      const x = m[1];
      const y = parseFloat(m[2]);
      if (lockedY === null) {
        lockedY = y;
        return;
      }
      if (Math.abs(y - lockedY) < 0.5) return;
      el.style.transform = `translate3d(${x}px, ${lockedY}px, 0)`;
    });
    observer.observe(el, { attributes: true, attributeFilter: ['style'] });
    // shadow 元素在 drop / 取消时会从 DOM 中移除，observer 随之被 GC，无需手动 disconnect。
  }

  /** 关闭按钮 / rename 输入需要拦截 pointer 事件，防止 svelte-dnd-action
   *  在它们身上触发拖拽。stopPropagation 同时覆盖 mousedown / touchstart，
   *  和库内部所有可能的拖拽起手监听器对齐。 */
  function blockDragStart(e: Event) {
    e.stopPropagation();
  }

  /** 整个 tab（含内边距 + 名字）作为拖拽起手区，键盘 Enter/Space 触发切换。
   *  rename 模式下不响应（输入会自己 handleRenameKeydown）。 */
  function handleSelectKeydown(e: KeyboardEvent, ws: WorkspaceInfo) {
    if (e.isComposing) return;
    if (editingId === ws.id) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onSwitch(ws.id);
    }
  }

  // 跨工作区拖拽 pane：当 `paneDragSourceId` 非空且 dragover 落在某个非活动 tab 上
  // 超过 HOVER_SWITCH_MS 时，自动切到该 workspace，让用户接着把 pane 放到目标
  // SplitContainer 的 dock 区域。源 pane 仍在原 workspace 后台保持运行；后端 dock_pane
  // 检测到 source/target 跨 workspace 后会迁移节点 + PTY，UI 上 keep-alive 切换无黑屏。
  const HOVER_SWITCH_MS = 250;
  let hoverTimer: ReturnType<typeof setTimeout> | null = null;
  // hoverTimerWsId 用 $state 是为了让 tab 的 highlight ring 在悬停命中时
  // 立即响应；clearHoverTimer 重置 → ring 移除，HOVER_SWITCH_MS 触发后切走。
  let hoverTimerWsId: string | null = $state(null);
  function clearHoverTimer() {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer);
      hoverTimer = null;
      hoverTimerWsId = null;
    }
  }
  function onTabDragOver(e: DragEvent, ws: WorkspaceInfo) {
    if (!get(paneDragSourceId)) return;
    if (ws.id === activeWorkspaceId) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move';
    if (hoverTimerWsId === ws.id) return; // already armed for this tab
    clearHoverTimer();
    hoverTimerWsId = ws.id;
    hoverTimer = setTimeout(() => {
      // 再次检查仍在拖拽中且这个 tab 仍是悬停目标。
      if (get(paneDragSourceId) && hoverTimerWsId === ws.id) {
        onSwitch(ws.id);
      }
      hoverTimer = null;
      hoverTimerWsId = null;
    }, HOVER_SWITCH_MS);
  }
  function onTabDragLeave(e: DragEvent, ws: WorkspaceInfo) {
    if (hoverTimerWsId === ws.id) {
      // dragleave 触发条件较松（指针移到子元素也会触发），用 setTimeout 自然过期已足够，
      // 这里只在切到下一个 tab 时清除（onTabDragOver 也会清）。
      // 防止指针快速划过多个 tab 时误清除：检查 relatedTarget。
      const cur = e.currentTarget;
      const rel = e.relatedTarget;
      if (cur instanceof HTMLElement && rel instanceof Node && cur.contains(rel)) return;
      clearHoverTimer();
    }
  }
</script>

<!-- Outer wrapper: plain flex row, bounded by flex-1/min-w-0 from parent.
     Inner scroll container holds only the tab items so they can overflow
     and scroll horizontally. Actions ("+" button) sit outside the scroll
     container and remain visible regardless of scroll position.
     wheel → horizontal pan (no Shift needed) is handled by overlayScroll. -->
<div class="min-w-0 flex-1 flex items-center">
  {#if actions}
  <div class="shrink-0 rg-no-drag">
    {@render actions()}
  </div>
  {/if}
  <div data-tauri-drag-region class="min-w-0 flex-1 py-1" use:overlayScroll={{ preset: 'horizontal-tabs' }}>
    <div
      class="rg-ws-dndzone flex items-center gap-1"
      use:dndzone={{
        items: localItems,
        flipDurationMs: 160,
        type: 'workspace-tabs',
        dropTargetStyle: {},
        dragDisabled: editingId !== null,
        transformDraggedElement: transformDraggedTab,
      }}
      onconsider={handleDndConsider}
      onfinalize={handleDndFinalize}
    >
      {#each localItems as ws (ws.id)}
      <div class="rg-no-drag relative shrink-0 flex items-center gap-1 rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors border cursor-grab active:cursor-grabbing select-none
          {ws.id === activeWorkspaceId
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)] border-[var(--rg-accent)]/35'
            : 'text-(--rg-fg-muted) border-transparent hover:bg-white/5 hover:text-(--rg-fg)'}
          {hoverTimerWsId === ws.id ? 'ring-2 ring-[var(--rg-accent)]/60' : ''}"
        title={editingId === ws.id ? undefined : `切换到 ${getWorkspaceName(ws)}`}
        onclick={() => { if (editingId !== ws.id) onSwitch(ws.id); }}
        onkeydown={(e) => handleSelectKeydown(e, ws)}
        oncontextmenu={(e) => handleContextMenu(e, ws)}
        ondragover={(e) => onTabDragOver(e, ws)}
        ondragleave={(e) => onTabDragLeave(e, ws)}
        role="button"
        tabindex="0"
        >
        {#if editingId === ws.id}
        <input
              type="text"
              bind:this={renameInput}
              bind:value={editingName}
              class="w-20 bg-transparent border-b border-[var(--rg-accent)] outline-none text-[var(--rg-fg)] text-[12px]"
              onclick={(e) => e.stopPropagation()}
              onmousedown={blockDragStart}
              ontouchstart={blockDragStart}
              onpointerdown={blockDragStart}
              onblur={() => handleRenameSubmit(ws.id)}
              onkeydown={(e) => handleRenameKeydown(e, ws.id)}
            />
          {:else}
        <span class="pointer-events-none">{getWorkspaceName(ws)}</span>
        {/if}

        {#if workspaces.length > 1}
        <button
              type="button"
              class="ml-1 opacity-60 hover:opacity-100 hover:text-red-400 transition-opacity"
              title="关闭工作区"
              onmousedown={blockDragStart}
              ontouchstart={blockDragStart}
              onpointerdown={blockDragStart}
              onclick={(e) => {
                e.stopPropagation();
                onClose(ws.id);
              }}
            >
              <svg
                class="w-3 h-3"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
              >
                <path d="M18 6L6 18M6 6l12 12" stroke-linecap="round" />
              </svg>
            </button>
        {/if}
      </div>
      {/each}
    </div>
  </div>
</div>

<style>
  /* svelte-dnd-action 默认会在 dropzone 上加描边/阴影；这里用 scoped 样式
     覆盖，与 Ridge 的 tab 视觉风格保持一致。 */
  :global(.rg-ws-dndzone) {
    outline: none !important;
  }
</style>
