<script lang="ts">
  import { Plus, X } from 'lucide-svelte';
  import type { PaneInfo, WorkspaceInfo, RemoteConnection, ConnectionState } from './lib/wsRemote';

  let { panes, activePaneId = $bindable(), workspaces = [], activeWorkspaceId = $bindable(), ws,
    wsState = 'disconnected' as ConnectionState
  }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    ws?: RemoteConnection;
    wsState?: ConnectionState;
  } = $props();

  let wsSwitching = $state(false);

  async function handleSwitchWorkspace(wsId: string) {
    if (wsSwitching || !ws || wsId === activeWorkspaceId) return;
    wsSwitching = true;
    activePaneId = null;
    activeWorkspaceId = wsId;
    try {
      await ws.switchWorkspace(wsId);
      ws.listPanes();
    } finally {
      wsSwitching = false;
    }
  }

  async function handleCloseWorkspace(e: Event, wsId: string) {
    e.stopPropagation();
    if (!ws) return;
    await ws.closeWorkspace(wsId);
    ws.listPanes();
  }

  async function handleAddPane() {
    if (!ws) return;
    const newId = await ws.createPane();
    if (newId) {
      activePaneId = newId;
      ws.listPanes();
    }
  }

  async function handleRemovePane(e: Event, paneId: string) {
    e.stopPropagation();
    if (!ws) return;
    const idx = panes.findIndex(p => p.id === paneId);
    const ok = await ws.closePane(paneId);
    if (ok) {
      if (paneId === activePaneId) {
        const remaining = panes.filter(p => p.id !== paneId);
        if (remaining.length > 0) {
          const nextIdx = Math.min(idx, remaining.length - 1);
          activePaneId = remaining[nextIdx].id;
        } else {
          activePaneId = null;
        }
      }
      ws.listPanes();
    }
  }
</script>

<div class="topbar">
  <div class="ws-tabs">
    {#each workspaces as wsp (wsp.id)}
      <button
        class="ws-tab"
        class:active={wsp.id === activeWorkspaceId}
        onclick={() => handleSwitchWorkspace(wsp.id)}
        disabled={wsSwitching}
        tabindex="-1"
      >
        <span class="ws-label">{wsp.name || '工作区'}</span>
        {#if workspaces.length > 1}
          <span class="ws-close" role="button" tabindex="-1"
            onclick={(e) => handleCloseWorkspace(e, wsp.id)}
            onkeydown={() => {}}>
            <X class="w-3 h-3" />
          </span>
        {/if}
      </button>
    {/each}
    {#if workspaces.length === 0}
      <span class="empty-msg">无工作区</span>
    {/if}
  </div>

  <div class="pane-tabs">
    {#each panes as pane}
      <button
        class="pane-tab"
        class:active={pane.id === activePaneId}
        onclick={() => activePaneId = pane.id}
      >
        <span class="dot">▸</span>
        <span class="label">{pane.title || '终端'}</span>
        {#if panes.length > 1}
          <span class="pane-close" role="button" tabindex="-1"
            onclick={(e) => handleRemovePane(e, pane.id)}
            onkeydown={() => {}}>
            <X class="w-3 h-3" />
          </span>
        {/if}
      </button>
    {/each}
    {#if panes.length === 0}
      <span class="empty-msg">无终端</span>
    {/if}
    <button class="add-pane-btn" onclick={handleAddPane} title="新建终端">
      <Plus class="w-4 h-4" />
    </button>
  </div>

  <span class="status-dot" class:connected={wsState === 'connected'} class:error={wsState === 'error'} title={wsState}>
    {wsState === 'connected' ? '●' : wsState === 'error' ? '●' : '○'}
  </span>
</div>

<style>
  .topbar{display:flex;align-items:center;gap:8px;padding:4px 8px;background:var(--rg-surface);border-bottom:1px solid var(--rg-border-bright);flex-shrink:0;min-height:36px;overflow:hidden}
  .ws-tabs{display:flex;gap:3px;overflow-x:auto;overflow-y:hidden;scrollbar-width:none;-webkit-overflow-scrolling:touch;flex-shrink:0;max-width:45%}
  .ws-tabs::-webkit-scrollbar{display:none}
  .ws-tab{display:flex;align-items:center;gap:4px;padding:3px 8px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:11px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0;max-width:120px}
  .ws-tab.active{border-color:var(--rg-accent);color:var(--rg-fg);background:color-mix(in srgb, var(--rg-accent) 12%, transparent)}
  .ws-tab:disabled{opacity:.5}
  .ws-label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .ws-close{display:flex;align-items:center;justify-content:center;width:14px;height:14px;border-radius:3px;opacity:.5;flex-shrink:0}
  .ws-close:active{background:rgba(255,255,255,.1);opacity:1}

  .pane-tabs{display:flex;gap:3px;overflow-x:auto;overflow-y:hidden;scrollbar-width:none;-webkit-overflow-scrolling:touch;flex:1;min-width:0}
  .pane-tabs::-webkit-scrollbar{display:none}
  .pane-tab{display:flex;align-items:center;gap:4px;padding:3px 10px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:11px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0;max-width:160px}
  .pane-tab.active{border-color:var(--rg-accent);color:var(--rg-fg);background:color-mix(in srgb, var(--rg-accent) 10%, transparent)}
  .dot{color:var(--rg-accent);font-weight:700;font-size:10px;flex-shrink:0}
  .label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:500}
  .pane-close{display:flex;align-items:center;justify-content:center;width:14px;height:14px;border-radius:3px;opacity:.5;flex-shrink:0;margin-left:2px}
  .pane-close:active{background:rgba(255,255,255,.1);opacity:1}
  .add-pane-btn{display:flex;align-items:center;justify-content:center;width:24px;height:24px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer;transition:all .15s;flex-shrink:0;margin-left:4px}
  .add-pane-btn:active{background:var(--rg-surface-2);color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 40%,transparent)}
  .empty-msg{color:var(--rg-fg-muted);font-size:11px;padding:2px 4px}

  .status-dot{font-size:9px;color:var(--rg-fg-muted);flex-shrink:0;line-height:1;margin-left:auto}
  .status-dot.connected{color:var(--rg-ansi-green)}
  .status-dot.error{color:var(--rg-ansi-red)}
</style>
