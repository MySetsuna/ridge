<!-- src/routes/+page.svelte -->
<script lang="ts">
  import SplitContainer from '$lib/components/SplitContainer.svelte';
  import GitGraph from '$lib/components/GitGraph.svelte';
  import WorkspaceTabs from '$lib/components/WorkspaceTabs.svelte';
  import WorkspaceSidebar from '$lib/components/WorkspaceSidebar.svelte';
  import Explorer from '$lib/components/Explorer.svelte';
  import { Terminal, FolderOpen, GitBranch, Layout, ChevronLeft, ChevronRight } from 'lucide-svelte';
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
    saveCurrentWorkspace,
    loadSavedWorkspaces,
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

  type SidebarTab = 'terminal' | 'git' | 'files';
  let sidebarTab = $state<SidebarTab>('files');

  // localStorage 键名
  const SIDEBAR_WIDTH_KEY = 'wind-sidebar-width';
  const SIDEBAR_COLLAPSED_KEY = 'wind-sidebar-collapsed';

  // 侧边栏宽度状态（用于可拖拽调整大小）
  let sidebarWidth = $state(288); // 默认 w-72 = 288px
  // 侧边栏是否折叠
  let sidebarCollapsed = $state(false);
  let isResizingSidebar = $state(false);

  // 计算窗口宽度的40%
  let windowWidth40 = $derived(typeof window !== 'undefined' ? window.innerWidth * 0.4 : 400);

  // 从 localStorage 加载侧边栏设置
  function loadSidebarSettings() {
    if (typeof localStorage === 'undefined') return;
    const savedWidth = localStorage.getItem(SIDEBAR_WIDTH_KEY);
    const savedCollapsed = localStorage.getItem(SIDEBAR_COLLAPSED_KEY);
    if (savedWidth) {
      const parsed = parseInt(savedWidth, 10);
      if (!isNaN(parsed) && parsed > 0) {
        sidebarWidth = Math.min(parsed, windowWidth40);
      }
    }
    if (savedCollapsed === 'true') {
      sidebarCollapsed = true;
    }
  }

  // 保存侧边栏设置到 localStorage
  function saveSidebarSettings() {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidth));
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(sidebarCollapsed));
  }

  // 切换侧边栏折叠状态
  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    saveSidebarSettings();
  }

  // 侧边栏折叠/展开时的宽度
  const COLLAPSED_WIDTH = 0;

  function onSidebarResizerMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    isResizingSidebar = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;

    function onMouseMove(ev: MouseEvent) {
      const delta = ev.clientX - startX;
      const maxWidth = windowWidth40;
      const newWidth = startWidth + delta;
      // 允许拖动到0关闭侧边栏
      sidebarWidth = Math.max(0, Math.min(maxWidth, newWidth));
      // 如果宽度小于20px，自动折叠
      if (sidebarWidth < 20) {
        sidebarCollapsed = true;
        sidebarWidth = 0;
      }
    }

    function onMouseUp() {
      isResizingSidebar = false;
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
      // 保存设置
      saveSidebarSettings();
    }

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  // 键盘快捷键处理
  function handleGlobalKeydown(e: KeyboardEvent) {
    // Ctrl+B: 切换侧边栏
    if (e.ctrlKey && (e.key === 'b' || e.key === 'B')) {
      e.preventDefault();
      toggleSidebar();
      return;
    }
    // Ctrl+A: 全选当前文本输入框的所有文本 (只在输入框/textarea上生效)
    if (e.ctrlKey && (e.key === 'a' || e.key === 'A')) {
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) {
        // 让浏览器默认行为处理全选
        return;
      }
      // 如果不是文本输入元素，阻止默认行为（避免误触）
      e.preventDefault();
    }
  }

  // 切换侧边栏tab时加载历史工作区
  $effect(() => {
    void sidebarTab; // 订阅 sidebarTab 变化
  void loadSavedWorkspaces();
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
        '排障入口：切换工作区报错请先看运行 wind / cargo tauri dev 的终端日志（搜索 [wind][pty]）。Claude split 需在 Wind 内建终端中运行，并确保 tmux 指向 wind-tmux shim。若出现 0xc0000142 这类进程级崩溃，需同时查看 Windows 事件查看器（应用程序日志）。',
    });
  }

  onMount(() => {
    if (!isTauri()) return;
  loadSidebarSettings();
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
                message: `teammate-layout-changed 后 store panes=${storeCount}, mounted panes=${domCount}`,
              });
            }
          });
        })();
      });

      const unlistenActive = await listen<string>(
        'teammate-active-pane-changed',
        (e) => {
          const id = typeof e.payload === 'string' ? e.payload : '';
          if (!id) return;
          void (async () => {
            await syncPaneLayoutFromBackend();
            activePaneId.set(id);
          })();
        }
      );

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
    'relative flex h-10 w-10 items-center justify-center rounded-xl text-lg transition-all duration-200 ' +
    'text-[var(--wf-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--wf-fg)]';
  const actBtnOn =
    ' bg-violet-500/[0.12] text-violet-200 ring-1 ring-violet-400/35 shadow-[0_0_20px_-4px_rgba(167,139,250,0.45)]';

  const toolBtn =
    'flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--wf-border)] ' +
    'bg-[var(--wf-surface)]/90 backdrop-blur-md text-[var(--wf-fg-muted)] ' +
    'hover:border-violet-400/35 hover:text-violet-200 hover:bg-violet-500/[0.08] transition-colors';

  // 窗口控制按钮样式（跟随系统：Windows在右侧，macOS在左侧）
  const winCtrlBtn =
    'flex h-8 w-8 items-center justify-center rounded-lg text-[var(--wf-fg-muted)] hover:bg-white/[0.06] hover:text-[var(--wf-fg)] transition-colors';
</script>

<svelte:window onkeydown={handleGlobalKeydown} />
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
      title="工作区"
      onclick={() => (sidebarTab = 'terminal')}
    >
      <Layout class="h-5 w-5" />
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'files' ? actBtnOn : ''}"
      title="文件"
      onclick={() => (sidebarTab = 'files')}
    >
      <FolderOpen class="h-5 w-5" />
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'git' ? actBtnOn : ''}"
      title="Git Graph"
      onclick={() => (sidebarTab = 'git')}
    >
      <GitBranch class="h-5 w-5" />
    </button>
  </aside>

  <!-- 侧边栏区域：wrapper 始终渲染，toggle 按钮始终可见 -->
  <div
    class="relative shrink-0"
    style="width: {sidebarCollapsed ? 0 : sidebarWidth}px; overflow: visible"
  >
    {#if !sidebarCollapsed}
      <aside
        class="h-full border-r border-[var(--wf-border)] bg-[var(--wf-surface-2)]/55 backdrop-blur-xl flex flex-col min-h-0 wf-scroll overflow-y-auto"
      >
        {#if sidebarTab === 'git'}
          <div
            class="px-3 h-11 items-center flex shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]"
          >
            Git Graph
          </div>
          <div class="flex-1 min-h-0 overflow-auto p-3 wf-scroll">
            <GitGraph />
          </div>
        {:else if sidebarTab === 'files'}
          <div
            class="px-3 h-11 items-center flex shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]"
          >
            资源管理器
          </div>
          <div class="flex-1 min-h-0 overflow-hidden">
            {#if $activeWorkspaceId}
              <Explorer workspaceId={$activeWorkspaceId} />
            {:else}
              <div
                class="p-4 text-[13px] leading-relaxed text-[var(--wf-fg-muted)]"
              >
                请先选择一个工作区
              </div>
            {/if}
          </div>
        {:else}
          <WorkspaceSidebar
            workspaces={$workspacesList}
            activeWorkspaceId={$activeWorkspaceId}
            onSelect={switchWorkspace}
            onRename={renameWorkspace}
            onDelete={closeWorkspace}
            onReorder={reorderWorkspaces}
            onSave={saveCurrentWorkspace}
            onCreate={createWorkspace}
          />
        {/if}

        <!-- 侧边栏大小调整手柄 -->
        <div
          class="group absolute h-full right-0 w-1 shrink-0 cursor-col-resize select-none hover:bg-[var(--wf-accent)]/20 active:bg-[var(--wf-accent)]/30 transition-colors {isResizingSidebar
            ? 'bg-[var(--wf-accent)]/40'
            : ''}"
          role="separator"
          aria-orientation="vertical"
          aria-label="拖动调整侧边栏宽度"
          onmousedown={onSidebarResizerMouseDown}
        ></div>
      </aside>
    {/if}

    <!-- 折叠/展开 toggle 按钮：始终渲染，位于 wrapper 右边缘 -->
    <button
      type="button"
      class="absolute top-1/2 -translate-y-1/2 left-full z-20 flex items-center justify-center w-4 h-10 rounded-r bg-[var(--wf-surface)]/80 border border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:border-[var(--wf-accent)] transition-colors opacity-60 hover:opacity-100"
      title={sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏"}
      onclick={toggleSidebar}
    >
      {#if sidebarCollapsed}
        <ChevronRight class="w-3 h-3" />
      {:else}
        <ChevronLeft class="w-3 h-3" />
      {/if}
    </button>
  </div>

  <!-- 主内容区 -->
  <div class="flex-1 flex flex-col min-w-0 min-h-0">
    <!-- 顶部标题栏 -->
    <header
      class="h-11 flex items-center gap-2 px-2 border-b border-[var(--wf-border)] bg-[var(--wf-glass)] backdrop-blur-md min-w-0"
      data-tauri-drag-region
    >
      <!-- 左侧元素组 -->
      <div class="flex items-center gap-2 flex-1" data-tauri-drag-region>
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
        <div
          class="wf-no-drag flex items-center gap-1 rounded-xl backdrop-blur-md"
        >
          <button
            type="button"
            class={toolBtn}
            title="左右分屏（当前选中窗格）"
            data-testid="add-pane-btn"
            onclick={() => void splitActivePane('horizontal')}
          >
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              aria-hidden="true"
            >
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
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              aria-hidden="true"
            >
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
          <svg
            class="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
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
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <rect x="5" y="9" width="10" height="10" rx="1" />
              <path d="M9 9V5h10v10h-4" />
            </svg>
          {:else}
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
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
          <svg
            class="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <path d="M18 6L6 18M6 6l12 12" stroke-linecap="round" />
          </svg>
        </button>
      </div>
    </header>

    <!-- 工作区内容 -->
    <div
      class="relative flex-1 min-h-0 min-w-0 overflow-hidden flex flex-col bg-[var(--wf-bg-raised)]"
    >
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
