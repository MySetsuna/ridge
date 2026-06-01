<script lang="ts">
  import { onMount } from 'svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import TopBar from './TopBar.svelte';
  import BottomTabBar from './BottomTabBar.svelte';
  import RemoteSidebar from './lib/RemoteSidebar.svelte';
  import { RemoteConnection, type PaneInfo, type ConnectionState, type WorkspaceInfo } from './lib/wsRemote';
  import { applyThemeVars, buildKernelTheme } from './lib/theme';

  let { ws }: { ws: RemoteConnection } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  let wsState = $state<ConnectionState>('disconnected');
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');
  let showKeyboard = $state(false);
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);

  let canvasRef: TerminalCanvas | undefined = $state();
  // Kernel palette derived from the desktop theme; applied to the canvas once it
  // mounts (the theme push usually arrives before the terminal exists).
  let kernelTheme: Record<string, string> | null = $state(null);

  function applyTheme(colors: Record<string, string>) {
    applyThemeVars(colors);
    kernelTheme = buildKernelTheme(colors);
  }

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
  }

  function onResize(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    ws.resizePane(paneId, rows, cols, pixelWidth, pixelHeight);
  }

  function handleRefresh() {
    // The refresh button locks the shared PTY to this client's current viewport
    // size (persists until the next claim/refresh from any endpoint), then
    // re-pulls the pane / workspace lists.
    if (activePaneId && canvasRef) {
      const d = canvasRef.getDims();
      if (d) ws.refreshPane(activePaneId, d.rows, d.cols, d.pixelWidth, d.pixelHeight);
    }
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
    if (sidebarTab === tab) {
      sidebarTab = null;
    } else {
      sidebarTab = tab;
    }
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
    ws.onRawBytes((paneId, data) => {
      if (paneId === activePaneId) {
        canvasRef?.feedUtf8(data);
      }
    });
    ws.onMetadata((paneId, title) => {
      // Title drives the document/tab title directly — the controller has no
      // title sink of its own. (cwd also arrives in this event but has no
      // consumer in the remote UI yet, so it's ignored here.)
      if (paneId === activePaneId && title != null && title.length > 0) {
        document.title = title;
      }
    });
    ws.onPtyResize((paneId, rows, cols) => {
      if (paneId === activePaneId) {
        canvasRef?.resizeKernel(rows, cols);
      }
    });
    // Theme: apply the snapshot pushed at connect (cached, since it usually
    // arrives before this listener), then follow any later pushes.
    const t0 = ws.lastTheme();
    if (t0) applyTheme(t0.colors);
    ws.onTheme((colors) => applyTheme(colors));
    ws.listPanes();
    refreshWorkspaces();
    return () => { ws.disconnect(); };
  });

  $effect(() => {
    if (activePaneId) {
      ws.subscribePane(activePaneId);
    }
  });

  // Apply the kernel palette once the canvas exists (theme can arrive earlier).
  $effect(() => {
    if (canvasRef && kernelTheme) canvasRef.applyTheme(kernelTheme);
  });
</script>

<div class="app-root">
  <TopBar {panes} bind:activePaneId {workspaces} {activeWorkspaceId} {ws} {wsState} />

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
    <RemoteSidebar onClose={() => sidebarTab = null} />
  {/if}

  <BottomTabBar
    {ws}
    {sidebarTab}
    onSidebarToggle={handleSidebarToggle}
    onRefresh={handleRefresh}
    bind:showKeyboard
  />
</div>

<style>
  .app-root{position:fixed;inset:0;display:flex;flex-direction:column;background:var(--rg-bg);color:var(--rg-fg)}
  .empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:var(--rg-fg-muted);gap:8px}
  .empty .hint{font-size:12px;color:var(--rg-fg-muted)}
  .sidebar-overlay{position:fixed;inset:0;background:rgba(0,0,0,0.5);z-index:40;touch-action:none}
</style>