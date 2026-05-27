<script lang="ts">
  import { Plus, X } from 'lucide-svelte';
  import type { PaneInfo, WorkspaceInfo, RemoteConnection } from './lib/wsRemote';

  let { panes, activePaneId = $bindable(), workspaces = [], activeWorkspaceId = '', ws }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    ws?: RemoteConnection;
  } = $props();

  let wsSwitching = $state(false);

  async function handleSwitchWorkspace(wsId: string) {
    if (wsSwitching || !ws) return;
    wsSwitching = true;
    try {
      await ws.switchWorkspace(wsId);
      // After switching, refresh panes from the new workspace
      ws.listPanes();
    } finally {
      wsSwitching = false;
    }
  }

  async function handleCreateWorkspace() {
    if (!ws) return;
    const id = await ws.createWorkspace();
    if (id) {
      ws.listPanes();
    }
  }

  async function handleCloseWorkspace(e: Event, wsId: string) {
    e.stopPropagation();
    if (!ws) return;
    await ws.closeWorkspace(wsId);
    ws.listPanes();
  }
</script>

<div class="bar">
  <div class="bar-inner">
    <!-- Workspace tabs (scrollable) -->
    <div class="ws-tabs">
      {#each workspaces as wsp (wsp.id)}
        <button
          class="ws-tab"
          class:active={wsp.id === activeWorkspaceId}
          onclick={() => handleSwitchWorkspace(wsp.id)}
          disabled={wsSwitching}
        >
          <span class="ws-label">{wsp.name || '工作区'}</span>
          {#if workspaces.length > 1}
            <span
              class="ws-close"
              role="button"
              tabindex="-1"
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

    <!-- Pane tabs (scrollable) -->
    <div class="pane-tabs">
      {#each panes as pane}
        <button
          class="pane-tab"
          class:active={pane.id === activePaneId}
          onclick={() => activePaneId = pane.id}
        >
          <span class="dot">#</span>
          <span class="label">{pane.title || 'terminal'}</span>
        </button>
      {/each}
      {#if panes.length === 0}
        <span class="empty-msg">无终端</span>
      {/if}
    </div>
  </div>

  <!-- Bottom-right action buttons -->
  <div class="actions">
    <button class="action-btn" title="新建工作区" onclick={handleCreateWorkspace}>
      <Plus class="w-4 h-4" />
    </button>
    {#if workspaces.length > 1}
      <button class="action-btn" title="关闭当前工作区" onclick={(e) => handleCloseWorkspace(e, activeWorkspaceId)}>
        <X class="w-4 h-4" />
      </button>
    {/if}
  </div>
</div>

<style>
  .bar{display:flex;align-items:center;gap:4px;padding:4px 4px 4px 8px;background:#161b22;border-top:1px solid #30363d;flex-shrink:0}
  .bar-inner{display:flex;flex-direction:column;flex:1;min-width:0;gap:2px}
  .ws-tabs{display:flex;gap:3px;overflow-x:auto;overflow-y:hidden;min-height:24px;scrollbar-width:none;-webkit-overflow-scrolling:touch}
  .ws-tabs::-webkit-scrollbar{display:none}
  .ws-tab{display:flex;align-items:center;gap:3px;padding:1px 6px;border:1px solid #30363d;border-radius:4px;background:#0d1117;color:#8b949e;font-size:10px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0}
  .ws-tab.active{border-color:#58a6ff;color:#e6edf3;background:rgba(88,166,255,.15)}
  .ws-tab:disabled{opacity:.5}
  .ws-label{max-width:80px;overflow:hidden;text-overflow:ellipsis}
  .ws-close{display:flex;align-items:center;justify-content:center;width:14px;height:14px;border-radius:3px;opacity:.6}
  .ws-close:active{background:rgba(255,255,255,.1);opacity:1}
  .pane-tabs{display:flex;gap:3px;overflow-x:auto;overflow-y:hidden;min-height:28px;scrollbar-width:none;-webkit-overflow-scrolling:touch}
  .pane-tabs::-webkit-scrollbar{display:none}
  .pane-tab{display:flex;align-items:center;gap:3px;padding:3px 8px;border:1px solid #30363d;border-radius:5px;background:#0d1117;color:#8b949e;font-size:11px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0}
  .pane-tab.active{border-color:#58a6ff;color:#e6edf3;background:rgba(88,166,255,.1)}
  .dot{color:#58a6ff;font-weight:700;font-size:10px}
  .label{max-width:100px;overflow:hidden;text-overflow:ellipsis}
  .empty-msg{color:#484f58;font-size:10px;padding:2px 4px}
  .actions{display:flex;flex-direction:column;gap:2px;padding:2px;flex-shrink:0}
  .action-btn{display:flex;align-items:center;justify-content:center;width:28px;height:28px;border:none;border-radius:6px;background:#0d1117;color:#8b949e;cursor:pointer;border:1px solid #30363d;transition:all .15s}
  .action-btn:active{background:#21262d;color:#e6edf3}
</style>
