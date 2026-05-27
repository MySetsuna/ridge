<script lang="ts">
  import { onMount } from 'svelte';
  import TerminalScreen from './TerminalScreen.svelte';
  import IdeScreen from './IdeScreen.svelte';
  import { RemoteConnection, type PaneInfo, type ConnectionState, type WorkspaceInfo } from './lib/wsRemote';

  let { ws }: { ws: RemoteConnection } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  let wsState = $state<ConnectionState>('disconnected');
  let mode: 'terminal' | 'ide' = $state('terminal');
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');

  async function refreshWorkspaces() {
    try {
      const data = await ws.listWorkspaces();
      workspaces = data.workspaces || [];
      const active = workspaces.find(w => w.active);
      if (active) activeWorkspaceId = active.id;
    } catch { /* ignore */ }
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
    });
    ws.listPanes();
    refreshWorkspaces();
    const wsTimer = setInterval(() => {
      refreshWorkspaces();
    }, 10000);
    return () => { ws.disconnect(); clearInterval(wsTimer); };
  });

  $effect(() => {
    if (activePaneId) {
      ws.subscribePane(activePaneId);
    }
  });
</script>

<div class="app-root">
  <div class="top-bar">
    <div class="tabs">
      <button class="tab" class:active={mode === 'terminal'} onclick={() => mode = 'terminal'}>
        终端
      </button>
      <button class="tab" class:active={mode === 'ide'} onclick={() => mode = 'ide'}>
        IDE
      </button>
    </div>
    <div class="top-right">
      <span class="badge" class:connected={wsState === 'connected'}>{wsState}</span>
      <button class="refresh-btn" onclick={() => { ws.listPanes(); refreshWorkspaces(); }}>↻</button>
    </div>
  </div>

  {#if mode === 'terminal'}
    <TerminalScreen {ws} {panes} bind:activePaneId {workspaces} {activeWorkspaceId} />
  {:else}
    <IdeScreen {ws} {panes} bind:activePaneId />
  {/if}
</div>

<style>
  .app-root{position:fixed;inset:0;display:flex;flex-direction:column;background:#0d1117;color:#e6edf3}
  .top-bar{display:flex;align-items:center;justify-content:space-between;padding:8px 12px;background:#161b22;border-bottom:1px solid #30363d;min-height:44px}
  .tabs{display:flex;gap:4px}
  .tab{background:none;border:none;color:#8b949e;font-size:14px;font-weight:500;padding:6px 16px;border-radius:8px;cursor:pointer;transition:all .15s}
  .tab.active{color:#e6edf3;background:#30363d}
  .top-right{display:flex;align-items:center;gap:8px}
  .badge{font-size:11px;padding:2px 8px;border-radius:10px;background:#21262d;color:#8b949e}
  .badge.connected{color:#3fb950}
  .refresh-btn{background:none;border:none;color:#8b949e;font-size:18px;cursor:pointer;padding:2px 6px;border-radius:4px}
  .refresh-btn:active{background:#30363d}
</style>
