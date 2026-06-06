<script lang="ts">
  import { t } from '$lib/i18n';
  import type { PaneInfo, WorkspaceInfo, ConnectionState } from './lib/wsRemote';

  // §item1（移动端导航重构）：工作区/终端的选择与增删已迁到底部导航条最右的
  // 树形级联控件（见 WorkspaceTree.svelte）。顶栏退化为「活动工作区 › 活动终端」
  // 面包屑 + 连接状态点，并承载 iPhone 灵动岛的顶部安全区内边距（item4）。
  let {
    panes = [],
    activePaneId = null,
    workspaces = [],
    activeWorkspaceId = '',
    wsState = 'disconnected' as ConnectionState,
  }: {
    panes?: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    wsState?: ConnectionState;
  } = $props();

  const activeWs = $derived(workspaces.find((w) => w.id === activeWorkspaceId));
  const activePane = $derived(panes.find((p) => p.id === activePaneId));
</script>

<div class="topbar">
  <div class="crumb">
    <span class="ws">{activeWs?.name || $t('mobile.workspaceDefault')}</span>
    {#if panes.length > 0}
      <span class="sep">›</span>
      <span class="pane">{activePane?.title || $t('mobile.terminalDefault')}</span>
    {/if}
  </div>
  <span
    class="status-dot"
    class:connected={wsState === 'connected'}
    class:error={wsState === 'error'}
    title={wsState}
  >
    {wsState === 'connected' ? '●' : wsState === 'error' ? '●' : '○'}
  </span>
</div>

<style>
  /* §safe-area: 顶部内边距叠加 env(safe-area-inset-top)，让面包屑避开 iPhone
     灵动岛/刘海；桌面/无安全区时 inset 为 0，等同 4px。 */
  .topbar{display:flex;align-items:center;gap:8px;padding:calc(5px + env(safe-area-inset-top,0px)) 12px 5px;background:var(--rg-surface);border-bottom:1px solid var(--rg-border-bright);flex-shrink:0;min-height:36px;overflow:hidden}
  .crumb{flex:1;min-width:0;display:flex;align-items:center;gap:6px;font-size:12px;overflow:hidden}
  .ws{color:var(--rg-fg-muted);font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:42%;flex-shrink:0}
  .sep{color:var(--rg-fg-muted);opacity:.55;flex-shrink:0}
  .pane{color:var(--rg-fg);font-weight:500;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;flex:1;min-width:0}
  .status-dot{font-size:9px;color:var(--rg-fg-muted);flex-shrink:0;line-height:1}
  .status-dot.connected{color:var(--rg-ansi-green)}
  .status-dot.error{color:var(--rg-ansi-red)}
</style>
