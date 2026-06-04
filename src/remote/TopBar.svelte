<script lang="ts">
  import { Plus, X } from 'lucide-svelte';
  import { t, tr } from '$lib/i18n';
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
  let paneTabMode: 'inline' | 'select' = $state('inline');
  let paneTabsEl: HTMLDivElement | undefined = $state();

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

  let addPaneError = $state('');

  async function handleAddPane() {
    if (!ws) return;
    addPaneError = '';
    try {
      const newId = await ws.createPane();
      if (newId) {
        activePaneId = newId;
        ws.listPanes();
      } else {
        addPaneError = tr('mobile.createTerminalFail');
      }
    } catch (e) {
      // 不静默吞掉创建失败：记录并通过按钮 title 暴露给用户。
      addPaneError = e instanceof Error ? e.message : tr('mobile.createTerminalFail');
      console.error('createPane failed', e);
    }
  }

  async function handleRemovePane(paneId: string) {
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

  function checkPaneOverflow() {
    if (!paneTabsEl) return;
    paneTabMode = paneTabsEl.scrollWidth > paneTabsEl.clientWidth + 2 ? 'select' : 'inline';
  }

  $effect(() => {
    void panes.length;
    void workspaces.length;
    void activePaneId;
    setTimeout(checkPaneOverflow, 0);
  });
</script>

<svelte:window onresize={checkPaneOverflow} />

<div class="topbar">
  <div class="ws-section">
    {#if workspaces.length > 0}
      <select
        class="ws-select"
        value={activeWorkspaceId}
        onchange={(e) => handleSwitchWorkspace((e.target as HTMLSelectElement).value)}
        disabled={wsSwitching}
      >
        {#each workspaces as wsp (wsp.id)}
          <option value={wsp.id}>{wsp.name || $t('mobile.workspaceDefault')}</option>
        {/each}
      </select>
      {#if workspaces.length > 1}
        <button class="ws-close-btn" onclick={(e) => handleCloseWorkspace(e, activeWorkspaceId!)} title={$t('mobile.closeWorkspace')} tabindex="-1">
          <X class="w-3 h-3" />
        </button>
      {/if}
    {:else}
      <span class="empty-msg">{$t('mobile.noWorkspace')}</span>
    {/if}
  </div>

  <div class="pane-section" bind:this={paneTabsEl}>
    {#if paneTabMode === 'select' && panes.length > 0}
      <select
        class="pane-select"
        value={activePaneId ?? ''}
        onchange={(e) => activePaneId = (e.target as HTMLSelectElement).value}
      >
        {#each panes as pane (pane.id)}
          <option value={pane.id}>{pane.title || $t('mobile.terminalDefault')}</option>
        {/each}
      </select>
      {#if panes.length > 1}
        <button class="pane-close-inline" onclick={() => activePaneId && handleRemovePane(activePaneId)} title={$t('mobile.closeTerminal')} tabindex="-1">
          <X class="w-3 h-3" />
        </button>
      {/if}
    {:else}
      {#each panes as pane (pane.id)}
        <button
          class="pane-tab"
          class:active={pane.id === activePaneId}
          onclick={() => activePaneId = pane.id}
        >
          <span class="dot">▸</span>
          <span class="label">{pane.title || $t('mobile.terminalDefault')}</span>
          {#if panes.length > 1}
            <span class="pane-close" role="button" tabindex="-1"
              onclick={(e) => { e.stopPropagation(); handleRemovePane(pane.id); }}
              onkeydown={() => {}}>
              <X class="w-3 h-3" />
            </span>
          {/if}
        </button>
      {/each}
      {#if panes.length === 0}
        <span class="empty-msg">{$t('mobile.noTerminal')}</span>
      {/if}
    {/if}
    <button class="add-pane-btn" class:err={addPaneError} onclick={handleAddPane} title={addPaneError || $t('mobile.newTerminalBtn')}>
      <Plus class="w-4 h-4" />
    </button>
  </div>

  <span class="status-dot" class:connected={wsState === 'connected'} class:error={wsState === 'error'} title={wsState}>
    {wsState === 'connected' ? '●' : wsState === 'error' ? '●' : '○'}
  </span>
</div>

<style>
  .topbar{display:flex;align-items:center;gap:8px;padding:4px 8px;background:var(--rg-surface);border-bottom:1px solid var(--rg-border-bright);flex-shrink:0;min-height:36px;overflow:hidden}
  .ws-section{display:flex;align-items:center;gap:4px;flex-shrink:0}
  .ws-select{appearance:none;-webkit-appearance:none;background:var(--rg-bg);color:var(--rg-fg);border:1px solid var(--rg-border-bright);border-radius:6px;padding:3px 24px 3px 8px;font-size:11px;font-family:inherit;cursor:pointer;max-width:140px;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6'%3E%3Cpath d='M0 0l5 6 5-6z' fill='%23888'/%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 6px center;line-height:1.4}
  .ws-select:disabled{opacity:.5;cursor:not-allowed}
  .ws-select:focus{outline:none;border-color:var(--rg-accent)}
  .ws-close-btn{display:flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid var(--rg-border-bright);border-radius:4px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0;opacity:.5}
  .ws-close-btn:active{background:var(--rg-surface-2);opacity:1;color:var(--rg-ansi-red)}

  .pane-section{display:flex;align-items:center;gap:3px;flex:1;min-width:0;overflow:hidden}
  .pane-select{appearance:none;-webkit-appearance:none;background:var(--rg-bg);color:var(--rg-fg);border:1px solid var(--rg-border-bright);border-radius:6px;padding:3px 24px 3px 8px;font-size:11px;font-family:inherit;cursor:pointer;flex:1;min-width:60px;max-width:180px;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6'%3E%3Cpath d='M0 0l5 6 5-6z' fill='%23888'/%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 6px center;line-height:1.4}
  .pane-select:focus{outline:none;border-color:var(--rg-accent)}
  .pane-close-inline{display:flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid var(--rg-border-bright);border-radius:4px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0;opacity:.5}
  .pane-close-inline:active{background:var(--rg-surface-2);opacity:1;color:var(--rg-ansi-red)}

  .pane-tab{display:flex;align-items:center;gap:4px;padding:3px 10px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:11px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0;max-width:160px}
  .pane-tab.active{border-color:var(--rg-accent);color:var(--rg-fg);background:color-mix(in srgb, var(--rg-accent) 10%, transparent)}
  .dot{color:var(--rg-accent);font-weight:700;font-size:10px;flex-shrink:0}
  .label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:500}
  .pane-close{display:flex;align-items:center;justify-content:center;width:14px;height:14px;border-radius:3px;opacity:.5;flex-shrink:0;margin-left:2px}
  .pane-close:active{background:rgba(255,255,255,.1);opacity:1}
  .add-pane-btn{display:flex;align-items:center;justify-content:center;width:24px;height:24px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer;transition:all .15s;flex-shrink:0;margin-left:4px}
  .add-pane-btn:active{background:var(--rg-surface-2);color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 40%,transparent)}
  .add-pane-btn.err{border-color:var(--rg-ansi-red);color:var(--rg-ansi-red)}
  .empty-msg{color:var(--rg-fg-muted);font-size:11px;padding:2px 4px}

  .status-dot{font-size:9px;color:var(--rg-fg-muted);flex-shrink:0;line-height:1;margin-left:auto}
  .status-dot.connected{color:var(--rg-ansi-green)}
  .status-dot.error{color:var(--rg-ansi-red)}
</style>