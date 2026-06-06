<script lang="ts">
  import { Folder, GitBranch, Search, Keyboard, RefreshCw } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { PaneInfo, WorkspaceInfo, RemoteConnection } from './lib/wsRemote';
  import WorkspaceTree from './lib/WorkspaceTree.svelte';

  // §item1：底部导航条最右用「工作区/终端」树形级联控件取代原渲染类型标签
  // (engine-badge)；工作区与终端的切换/新建/关闭全部收敛到该控件内。渲染引擎
  // 名称作为树弹层底部的小字保留，不再单独占位。
  let {
    ws,
    sidebarTab = null as 'files' | 'git' | 'search' | null,
    onSidebarToggle,
    onRefresh,
    showKeyboard = $bindable(false),
    backendName = 'Canvas2D',
    panes = [],
    activePaneId = $bindable(null),
    workspaces = [],
    activeWorkspaceId = $bindable(''),
    onWorkspacesChanged,
  }: {
    ws?: RemoteConnection;
    sidebarTab?: 'files' | 'git' | 'search' | null;
    onSidebarToggle?: (tab: 'files' | 'git' | 'search') => void;
    onRefresh?: () => void;
    showKeyboard?: boolean;
    backendName?: string;
    panes?: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    onWorkspacesChanged?: () => void;
  } = $props();
</script>

<div class="actionbar">
  <!-- Sidebar toggles -->
  <div class="group">
    <button class="ctrl-btn" class:active={sidebarTab === 'files'} onclick={() => onSidebarToggle?.('files')} title={$t('mobile.filesTitle')} tabindex="-1">
      <Folder class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'git'} onclick={() => onSidebarToggle?.('git')} title="Git" tabindex="-1">
      <GitBranch class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'search'} onclick={() => onSidebarToggle?.('search')} title={$t('mobile.searchTitle')} tabindex="-1">
      <Search class="w-4 h-4" />
    </button>
  </div>

  <!-- View controls -->
  <div class="group">
    <button class="ctrl-btn" class:active={showKeyboard} onclick={() => showKeyboard = !showKeyboard} title={$t('mobile.virtualKeyboard')} tabindex="-1">
      <Keyboard class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onRefresh} title={$t('mobile.lockAndRefresh')} tabindex="-1">
      <RefreshCw class="w-4 h-4" />
    </button>
  </div>

  <!-- 工作区 / 终端 树形级联（最右，原渲染类型标签位置） -->
  <WorkspaceTree
    {panes}
    bind:activePaneId
    {workspaces}
    bind:activeWorkspaceId
    {ws}
    {backendName}
    {onWorkspacesChanged}
  />
</div>

<style>
  /* §safe-area: 底部内边距叠加 env(safe-area-inset-bottom)，让操作条避开 iPhone
     底部 home indicator；无安全区时 inset 为 0，等同 6px。 */
  .actionbar{display:flex;align-items:center;justify-content:space-between;gap:6px;padding:6px 12px calc(6px + env(safe-area-inset-bottom,0px));background:var(--rg-surface);border-top:1px solid var(--rg-border-bright);flex-shrink:0;min-height:48px}
  .group{display:flex;align-items:center;gap:6px;flex-shrink:0}
  .ctrl-btn{display:flex;align-items:center;justify-content:center;width:42px;height:36px;background:none;border:1px solid transparent;border-radius:8px;color:var(--rg-fg-muted);cursor:pointer;transition:all .15s;-webkit-tap-highlight-color:transparent}
  .ctrl-btn:active{background:var(--rg-surface-2);color:var(--rg-fg)}
  .ctrl-btn.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 12%, transparent);border-color:color-mix(in srgb, var(--rg-accent) 40%, transparent)}
</style>
