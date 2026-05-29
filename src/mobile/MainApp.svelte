<script lang="ts">
  import { onMount } from 'svelte';
  import { X } from 'lucide-svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import BottomTabBar from './BottomTabBar.svelte';
  import SidebarFileTree from '@shared/sidebar/SidebarFileTree.svelte';
  import SidebarGitPanel from '@shared/sidebar/SidebarGitPanel.svelte';
  import SidebarSearch from '@shared/sidebar/SidebarSearch.svelte';
  import { RemoteConnection, type PaneInfo, type ConnectionState, type WorkspaceInfo } from './lib/wsRemote';

  let { ws }: { ws: RemoteConnection } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  let wsState = $state<ConnectionState>('disconnected');
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');
  let showKeyboard = $state(false);
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);

  let canvasRef: TerminalCanvas | undefined = $state();

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
  }

  function onResize(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    ws.resizePane(paneId, rows, cols, pixelWidth, pixelHeight);
  }

  function handleRefresh() {
    ws.listPanes();
    refreshWorkspaces();
  }

  async function refreshWorkspaces() {
    try {
      const data = await ws.listWorkspaces();
      workspaces = data.workspaces || [];
      const active = workspaces.find(w => w.active);
      if (active) activeWorkspaceId = active.id;
    } catch { /* ignore */ }
  }

  function handleSidebarToggle(tab: 'files' | 'git' | 'search') {
    sidebarTab = sidebarTab === tab ? null : tab;
  }

  function handleOpenFile(_path: string, _line?: number) {
    // Remote currently browses read-only; opening a file in an editor is a
    // follow-up. Close the sidebar so the tap still feels responsive.
    sidebarTab = null;
  }

  onMount(() => {
    ws.onStateChange((s) => wsState = s);
    ws.onMessage((msg) => {
      if (msg.type === 'panes') {
        panes = msg.panes;
        if (!activePaneId && msg.panes.length > 0) {
          activePaneId = msg.panes[0].id;
        }
      }
      if (msg.type === 'switch-workspace-result' || msg.type === 'create-workspace-result' || msg.type === 'close-workspace-result') {
        refreshWorkspaces();
      }
    });
    ws.listPanes();
    refreshWorkspaces();
    return () => { ws.disconnect(); };
  });

  $effect(() => {
    if (activePaneId) {
      ws.subscribePane(activePaneId);
    }
  });
</script>

<div class="app-root">
  {#if panes.length === 0}
    <div class="empty"><p>无活跃终端</p><p class="hint">在桌面端打开一个终端以开始</p></div>
  {:else if activePaneId}
    <TerminalCanvas
      bind:this={canvasRef}
      paneId={activePaneId ?? null}
      {onStdin}
      {onResize}
      {showKeyboard}
    />
  {/if}

  {#if sidebarTab !== null}
    <div class="sidebar-overlay" onclick={() => sidebarTab = null} role="presentation"></div>
    <div class="sidebar" role="dialog" aria-label="Sidebar">
      <div class="sidebar-header">
        <span class="sidebar-title">
          {sidebarTab === 'files' ? '文件' : sidebarTab === 'git' ? 'Git' : '搜索'}
        </span>
        <button class="close-btn" onclick={() => sidebarTab = null}>
          <X class="w-5 h-5" />
        </button>
      </div>
      <div class="sidebar-body">
        {#if sidebarTab === 'files'}
          <SidebarFileTree provider={ws} onOpenFile={handleOpenFile} />
        {:else if sidebarTab === 'git'}
          <SidebarGitPanel provider={ws} />
        {:else}
          <SidebarSearch provider={ws} onOpenFile={handleOpenFile} />
        {/if}
      </div>
    </div>
  {/if}

  <BottomTabBar
    {panes}
    bind:activePaneId
    {workspaces}
    {activeWorkspaceId}
    {ws}
    {sidebarTab}
    onSidebarToggle={handleSidebarToggle}
    {wsState}
    onRefresh={handleRefresh}
    bind:showKeyboard
  />
</div>

<style>
  .app-root{position:fixed;inset:0;display:flex;flex-direction:column;background:#0d1117;color:#e6edf3}
  .empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:#8b949e;gap:8px}
  .empty .hint{font-size:12px;color:#484f58}

  .sidebar-overlay{position:fixed;inset:0;background:rgba(0,0,0,0.5);z-index:40;touch-action:none}
  .sidebar{position:fixed;inset:0;z-index:50;display:flex;flex-direction:column;background:#161b22;animation:slideIn .2s ease-out}
  @keyframes slideIn{from{transform:translateX(-100%)}to{transform:translateX(0)}}
  .sidebar-header{display:flex;align-items:center;justify-content:space-between;padding:12px 16px;border-bottom:1px solid #30363d;min-height:48px;flex-shrink:0}
  .sidebar-title{font-size:15px;font-weight:600;color:#e6edf3}
  .close-btn{background:none;border:none;color:#8b949e;padding:4px;border-radius:6px;cursor:pointer}
  .close-btn:active{background:#21262d}
  .sidebar-body{flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden}
</style>
