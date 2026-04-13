<!-- src/routes/+page.svelte -->
<script lang="ts">
  import SplitContainer from '$lib/components/SplitContainer.svelte';
  import GitGraph from '$lib/components/GitGraph.svelte';
  import {
    paneTreeStore,
    splitActivePane,
    syncPaneLayoutFromBackend,
    refreshWorkspaces,
    workspacesList,
    activeWorkspaceId,
    createWorkspace,
    switchWorkspace,
    getAllPaneIds
  } from '$lib/stores/paneTree';
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';

  let rootNode = $derived($paneTreeStore);
  let hasPaneLayout = $derived(getAllPaneIds(rootNode).length > 0);

  type SidebarTab = 'terminal' | 'git' | 'files';
  let sidebarTab = $state<SidebarTab>('terminal');

  onMount(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    void (async () => {
      await refreshWorkspaces();
      unlisten = await listen('teammate-layout-changed', () => {
        void syncPaneLayoutFromBackend();
      });
    })();
    return () => {
      unlisten?.();
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
</script>

<div
  class="flex h-screen w-screen overflow-hidden bg-[var(--wf-bg)] text-[var(--wf-fg)] selection:bg-violet-500/25"
>
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
  </aside>

  <aside
    class="w-72 shrink-0 border-r border-[var(--wf-border)] bg-[var(--wf-surface-2)]/55 backdrop-blur-xl flex flex-col min-h-0 wf-scroll overflow-y-auto"
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

  <div class="flex-1 flex flex-col min-w-0 min-h-0">
    <header
      class="h-11 shrink-0 flex items-center gap-2 px-2 border-b border-[var(--wf-border)] bg-[var(--wf-glass)] backdrop-blur-md min-w-0"
    >
      <div class="flex items-center gap-1 overflow-x-auto min-w-0 flex-1 py-1 wf-scroll">
        {#each $workspacesList as ws (ws.id)}
          <button
            type="button"
            title="切换到工作区 {ws.index + 1}"
            class="shrink-0 rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors border {ws.id ===
            $activeWorkspaceId
              ? 'bg-violet-500/15 text-violet-100 border-violet-400/35'
              : 'text-[var(--wf-fg-muted)] border-transparent hover:bg-white/[0.05] hover:text-[var(--wf-fg)]'}"
            onclick={() => switchWorkspace(ws.id)}
          >
            工作区 {ws.index + 1}
          </button>
        {/each}
        <button
          type="button"
          class="shrink-0 flex h-8 w-8 items-center justify-center rounded-lg border border-dashed border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:border-violet-400/40 hover:text-violet-200 hover:bg-violet-500/[0.06] transition-colors"
          title="新建根工作区（独立分屏树与终端）"
          onclick={() => createWorkspace()}
        >
          <span class="text-lg leading-none">+</span>
        </button>
      </div>
      <span class="hidden sm:inline text-[11px] text-[var(--wf-fg-muted)] shrink-0 pr-2 truncate max-w-[8rem]"
        >WarpForge</span
      >
    </header>

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

      <!-- 终端区域右上角：当前选中窗格分屏 + 新建根工作区快捷入口 -->
      <div
        class="pointer-events-none absolute top-2 right-2 z-20 flex items-center gap-1"
        aria-hidden="true"
      >
        <div class="pointer-events-auto flex items-center gap-1 rounded-xl border border-[var(--wf-border)] bg-[var(--wf-surface)]/85 backdrop-blur-md p-1 shadow-lg shadow-black/40">
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
          <button
            type="button"
            class={toolBtn}
            title="新建根工作区"
            onclick={() => void createWorkspace()}
          >
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
              <rect x="3" y="3" width="18" height="18" rx="2" />
              <path d="M12 8v8M8 12h8" stroke-linecap="round" />
            </svg>
          </button>
        </div>
      </div>
    </div>
  </div>
</div>
