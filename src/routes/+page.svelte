<!-- src/routes/+page.svelte -->
<script lang="ts">
import SplitContainer from '$lib/components/SplitContainer.svelte';
import GitGraph from '$lib/components/GitGraph.svelte';
import WorkspaceTabs from '$lib/components/WorkspaceTabs.svelte';
import WorkspaceHistory from '$lib/components/WorkspaceHistory.svelte';
import {
  paneTreeStore,
  activePaneId,
  splitActivePane,
  syncPaneLayoutFromBackend,
  refreshWorkspaces,
  workspacesList,
  activeWorkspaceId,
  createWorkspace,
  switchWorkspace,
  getAllPaneIds,
  closeWorkspace,
  reorderWorkspaces,
  renameWorkspace,
  workspaceHistoryList,
  loadWorkspaceHistory,
  restoreWorkspaceFromHistory,
  deleteWorkspaceHistory,
  togglePinWorkspaceHistory,
  renameWorkspaceHistory
} from '$lib/stores/paneTree';
import { reportDevIssue } from '$lib/devIssue';
import { dev } from '$app/environment';
import { get } from 'svelte/store';
import { onMount } from 'svelte';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

let rootNode = $derived($paneTreeStore);
let hasPaneLayout = $derived(getAllPaneIds(rootNode).length > 0);

type SidebarTab = 'terminal' | 'git' | 'files' | 'history';
let sidebarTab = $state<SidebarTab>('terminal');

// 侧边栏宽度状态（用于可拖拽调整大小）
let sidebarWidth = $state(288); // 默认 w-72 = 288px
const MIN_SIDEBAR_WIDTH = 200;
const MAX_SIDEBAR_WIDTH = 600;
let isResizingSidebar = $state(false);

function onSidebarResizerMouseDown(e: MouseEvent) {
 e.preventDefault();
 isResizingSidebar = true;
 const startX = e.clientX;
 const startWidth = sidebarWidth;

 function onMouseMove(ev: MouseEvent) {
   const delta = ev.clientX - startX;
   sidebarWidth = Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, startWidth + delta));
 }

 function onMouseUp() {
   isResizingSidebar = false;
   window.removeEventListener('mousemove', onMouseMove);
   window.removeEventListener('mouseup', onMouseUp);
 }

 window.addEventListener('mousemove', onMouseMove);
 window.addEventListener('mouseup', onMouseUp);
}

// 切换侧边栏tab时加载历史工作区
$effect(() => {
  if (sidebarTab === 'history') {
    void loadWorkspaceHistory();
  }
});

// 窗口控制
let isMaximized = $state(false);

async function handleMinimize() {
  if (!isTauri()) return;
  const win = getCurrentWindow();
  await win.minimize();
}

async function handleMaximize() {
  if (!isTauri()) return;
  const win = getCurrentWindow();
  await win.toggleMaximize();
  isMaximized = await win.isMaximized();
}

async function handleClose() {
  if (!isTauri()) return;
  const win = getCurrentWindow();
  await win.close();
}

function openDevIssueHelp() {
  reportDevIssue({
    title: 'Wind Dev',
    message:
      '排障入口：切换工作区报错请先看运行 wind / cargo tauri dev 的终端日志（搜索 [wind][pty]）。Claude split 需在 Wind 内建终端中运行，并确保 tmux 指向 wind-tmux shim。若出现 0xc0000142 这类进程级崩溃，需同时查看 Windows 事件查看器（应用程序日志）。'
  });
}

onMount(() => {
  if (!isTauri()) return;
  let unlisten: (() => void) | undefined;
  let unlistenResized: (() => void) | undefined;
  void (async () => {
    await refreshWorkspaces();
    // 检查初始最大化状态
    const win = getCurrentWindow();
    isMaximized = await win.isMaximized();
    unlistenResized = await win.onResized(async () => {
      isMaximized = await getCurrentWindow().isMaximized();
    });

    unlisten = await listen('teammate-layout-changed', () => {
      void (async () => {
        await syncPaneLayoutFromBackend();
        if (!dev) return;
        requestAnimationFrame(() => {
          const storeCount = getAllPaneIds(get(paneTreeStore)).length;
          const domCount = document.querySelectorAll('.wf-pane-root').length;
          if (storeCount > 0 && domCount !== storeCount) {
            reportDevIssue({
              title: 'Layout sync mismatch',
              message: `teammate-layout-changed 后 store panes=${storeCount}, mounted panes=${domCount}`
            });
          }
        });
      })();
    });

    const unlistenActive = await listen<string>('teammate-active-pane-changed', (e) => {
      const id = typeof e.payload === 'string' ? e.payload : '';
      if (!id) return;
      void (async () => {
        await syncPaneLayoutFromBackend();
        activePaneId.set(id);
      })();
    });

    const prevUnlisten = unlisten;
    unlisten = () => {
      prevUnlisten?.();
      unlistenActive();
    };
  })();
  return () => {
    unlisten?.();
    unlistenResized?.();
  };
});

const actBtn =
  'relative flex h-11 w-11 items-center justify-center rounded-xl text-lg transition-all duration-200 ' +
  'text-[var(--wf-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--wf-fg)]';
const actBtnOn =
  ' bg-violet-500/[0.12] text-violet-200 ring-1 ring-violet-400/35 shadow-[0_0_20px_-4px_rgba(167,139,250,0.45)]';

const toolBtn =
  'flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-[var(--wf-border)] ' +
  'bg-[var(--wf-surface)]/90 backdrop-blur-md text-[var(--wf-fg-muted)] ' +
  'hover:border-violet-400/35 hover:text-violet-200 hover:bg-violet-500/[0.08] transition-colors';

// 窗口控制按钮样式（跟随系统：Windows在右侧，macOS在左侧）
const winCtrlBtn =
  'flex h-8 w-8 items-center justify-center rounded-lg text-[var(--wf-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--wf-fg)] transition-colors';
</script>

<div
  class="flex h-screen w-screen overflow-hidden bg-[var(--wf-bg)] text-[var(--wf-fg)] selection:bg-violet-500/25"
  data-tauri-drag-region
>
  <!-- 左侧图标导航栏 -->
  <aside
    class="w-[52px] shrink-0 flex flex-col items-center py-3 gap-1.5 border-r border-[var(--wf-border)] bg-[var(--wf-surface)]/35 backdrop-blur-2xl"
  >
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'terminal' ? actBtnOn : ''}"
      title="终端工作区"
      onclick={() => (sidebarTab = 'terminal')}
    >
      ⚡
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'files' ? actBtnOn : ''}"
      title="文件（占位）"
      onclick={() => (sidebarTab = 'files')}
    >
      📁
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'git' ? actBtnOn : ''}"
      title="Git Graph"
      onclick={() => (sidebarTab = 'git')}
    >
      📊
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'history' ? actBtnOn : ''}"
      title="历史工作区"
      onclick={() => (sidebarTab = 'history')}
    >
      🕐
    </button>
  </aside>

  <!-- 侧边栏大小调整手柄 -->
<div
 class="group relative w-1 shrink-0 cursor-col-resize select-none hover:bg-[var(--wf-accent)]/20 active:bg-[var(--wf-accent)]/30 transition-colors {isResizingSidebar ? 'bg-[var(--wf-accent)]/40' : ''}"
 role="separator"
 aria-orientation="vertical"
 aria-label="拖动调整侧边栏宽度"
 onmousedown={onSidebarResizerMouseDown}
></div>

<!-- 侧边栏内容区 -->
  <aside
    class="shrink-0 border-r border-[var(--wf-border)] bg-[var(--wf-surface-2)]/55 backdrop-blur-xl flex flex-col min-h-0 wf-scroll overflow-y-auto"
 style="width: {sidebarWidth}px"
  >
    {#if sidebarTab === 'git'}
      <div
        class="px-4 py-3 shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]"
      >
        Git Graph
      </div>
      <div class="flex-1 min-h-0 overflow-auto p-3 wf-scroll">
        <GitGraph />
      </div>
    {:else if sidebarTab === 'files'}
      <div
        class="px-4 py-3 shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]"
      >
        资源管理器
      </div>
      <div class="p-4 text-[13px] leading-relaxed text-[var(--wf-fg-muted)]">
        文件树尚未接入（架构文档中为待办）。
      </div>
    {:else if sidebarTab === 'history'}
      <div
        class="px-4 py-3 shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]"
      >
        历史工作区
      </div>
<div class="flex-1 min-h-0 overflow-auto wf-scroll">
  <WorkspaceHistory
    history={$workspaceHistoryList}
    onDelete={deleteWorkspaceHistory}
    onPin={togglePinWorkspaceHistory}
    onRename={renameWorkspaceHistory}
    onRestore={restoreWorkspaceFromHistory}
  />
</div>
    {:else}
      <div
        class="px-4 py-3 shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]"
      >
        终端
      </div>
      <div class="p-4 text-[13px] leading-relaxed text-[var(--wf-fg-muted)]">
        点击某个窗格标题或终端即可选中；主区域右上角图标可对<strong>当前选中窗格</strong>分屏。新建根工作区会打开独立会话（各自 PTY）。
      </div>
    {/if}
  </aside>

  <!-- 主内容区 -->
  <div class="flex-1 flex flex-col min-w-0 min-h-0">
    <!-- 顶部标题栏 -->
    <header
      class="h-11 shrink-0 flex justify-between items-center gap-2 px-2 border-b border-[var(--wf-border)] bg-[var(--wf-glass)] backdrop-blur-md min-w-0"
      data-tauri-drag-region
    >
      <!-- 左侧元素组 -->
<div class="flex items-center gap-2">

<!-- 工作区标签区 -->
      <WorkspaceTabs
        workspaces={$workspacesList}
        activeWorkspaceId={$activeWorkspaceId}
        onSwitch={switchWorkspace}
        onClose={closeWorkspace}
        onReorder={reorderWorkspaces}
        onRename={renameWorkspace}
      >
        <!-- 新建工作区按钮 -->
        {#snippet actions()}
          <button
            type="button"
            class="shrink-0 flex h-8 w-8 items-center justify-center rounded-lg border border-dashed border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:border-violet-400/40 hover:text-violet-200 hover:bg-violet-500/[0.06] transition-colors"
            title="新建根工作区（独立分屏树与终端）"
            onclick={() => createWorkspace()}
          >
            <span class="text-lg leading-none">+</span>
          </button>
        {/snippet}
      </WorkspaceTabs>


      <!-- 开发排障入口 -->
      {#if dev}
        <button
          type="button"
          class="wf-no-drag shrink-0 rounded-lg px-2.5 py-1.5 text-[11px] font-medium border border-red-500/30 text-red-300/90 hover:bg-red-500/10 transition-colors"
          title="开发排障入口"
          onclick={openDevIssueHelp}
        >
          Dev Issue
        </button>
      {/if}
      
      <!-- 分屏操作按钮 -->
      <div class="wf-no-drag flex items-center gap-1 rounded-xl border border-[var(--wf-border)] bg-[var(--wf-surface)]/85 backdrop-blur-md p-1 shadow-lg shadow-black/40">
        <button
          type="button"
          class={toolBtn}
          title="左右分屏（当前选中窗格）"
          onclick={() => void splitActivePane('horizontal')}
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <rect x="3" y="5" width="7" height="14" rx="1.5" />
            <rect x="14" y="5" width="7" height="14" rx="1.5" />
          </svg>
        </button>
        <button
          type="button"
          class={toolBtn}
          title="上下分屏（当前选中窗格）"
          onclick={() => void splitActivePane('vertical')}
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <rect x="4" y="4" width="16" height="7" rx="1.5" />
            <rect x="4" y="13" width="16" height="7" rx="1.5" />
          </svg>
        </button>
      </div>

      <!-- 窗口控制按钮（右侧）：wf-no-drag 避免与标题栏拖动区域冲突 -->
      </div>
<div class="wf-no-drag flex items-center gap-1 shrink-0">
        <button
          type="button"
          class={winCtrlBtn}
          title="最小化"
          onclick={handleMinimize}
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M5 12h14" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class={winCtrlBtn}
          title={isMaximized ? '还原' : '最大化'}
          onclick={handleMaximize}
        >
          {#if isMaximized}
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="5" y="9" width="10" height="10" rx="1" />
              <path d="M9 9V5h10v10h-4" />
            </svg>
          {:else}
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="4" y="4" width="16" height="16" rx="2" />
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="{winCtrlBtn} hover:bg-red-500/20 hover:text-red-400"
          title="关闭"
          onclick={handleClose}
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6L6 18M6 6l12 12" stroke-linecap="round" />
          </svg>
        </button>
      </div>
    </header>

    <!-- 工作区内容 -->
    <div class="relative flex-1 min-h-0 min-w-0 overflow-hidden flex flex-col bg-[var(--wf-bg-raised)]">
      {#if $activeWorkspaceId && hasPaneLayout}
        {#key $activeWorkspaceId}
          <SplitContainer workspaceId={$activeWorkspaceId} node={rootNode} />
        {/key}
      {:else}
        <div
          class="flex flex-1 items-center justify-center text-[13px] text-[var(--wf-fg-muted)]"
        >
          正在加载工作区…
        </div>
      {/if}
    </div>
  </div>
</div>