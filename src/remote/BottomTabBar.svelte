<script lang="ts">
  import { Plus, X, Folder, GitBranch, Search } from 'lucide-svelte';
  import type { PaneInfo, WorkspaceInfo, RemoteConnection, ConnectionState } from './lib/wsRemote';

  let { panes, activePaneId = $bindable(), workspaces = [], activeWorkspaceId = '', ws,
    sidebarTab = null as 'files' | 'git' | 'search' | null, onSidebarToggle, wsState = 'disconnected' as ConnectionState,
    onRefresh, showKeyboard = $bindable(false)
  }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    ws?: RemoteConnection;
    sidebarTab?: 'files' | 'git' | 'search' | null;
    onSidebarToggle?: (tab: 'files' | 'git' | 'search') => void;
    wsState?: ConnectionState;
    onRefresh?: () => void;
    showKeyboard?: boolean;
  } = $props();

  let wsSwitching = $state(false);

  async function handleSwitchWorkspace(wsId: string) {
    if (wsSwitching || !ws) return;
    wsSwitching = true;
    try {
      await ws.switchWorkspace(wsId);
      ws.listPanes();
    } finally {
      wsSwitching = false;
    }
  }

  async function handleCreateWorkspace() {
    if (!ws) return;
    const id = await ws.createWorkspace();
    if (id) { ws.listPanes(); }
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
          {#if pane.cwd}
            <span class="cwd">{pane.cwd.split('/').pop() || pane.cwd.split('\\').pop()}</span>
          {/if}
        </button>
      {/each}
      {#if panes.length === 0}
        <span class="empty-msg">无终端</span>
      {/if}
    </div>
  </div>

  <div class="right-controls">
    <button class="ctrl-btn" class:active={sidebarTab === 'files'} onclick={() => onSidebarToggle?.('files')} title="文件">
      <Folder class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'git'} onclick={() => onSidebarToggle?.('git')} title="Git">
      <GitBranch class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'search'} onclick={() => onSidebarToggle?.('search')} title="搜索">
      <Search class="w-4 h-4" />
    </button>

    <div class="ctrl-sep"></div>

    <button class="ctrl-btn" class:active={showKeyboard} onclick={() => showKeyboard = !showKeyboard} title="键盘">
      <span class="kb-icon">⌨</span>
    </button>

    <div class="ctrl-sep"></div>

    <button class="ctrl-btn action-btn" onclick={onRefresh} title="锁定渲染尺寸到本端并刷新">
      <span class="refresh-icon">↻</span>
    </button>
    <button class="ctrl-btn action-btn" onclick={handleCreateWorkspace} title="新建工作区">
      <Plus class="w-4 h-4" />
    </button>

    <span class="status-dot" class:connected={wsState === 'connected'} class:error={wsState === 'error'} title={wsState}>
      {wsState === 'connected' ? '●' : wsState === 'error' ? '●' : '○'}
    </span>
  </div>
</div>

<style>
  .bar{display:flex;align-items:center;gap:2px;padding:4px 6px;background:var(--rg-surface);border-top:1px solid var(--rg-border-bright);flex-shrink:0;min-height:52px}
  .bar-inner{display:flex;flex-direction:column;flex:1;min-width:0;gap:2px}
  .ws-tabs{display:flex;gap:2px;overflow-x:auto;overflow-y:hidden;min-height:22px;scrollbar-width:none;-webkit-overflow-scrolling:touch}
  .ws-tabs::-webkit-scrollbar{display:none}
  .ws-tab{display:flex;align-items:center;gap:3px;padding:1px 6px;border:1px solid var(--rg-border-bright);border-radius:4px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:10px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0;max-width:100px}
  .ws-tab.active{border-color:var(--rg-accent);color:var(--rg-fg);background:color-mix(in srgb, var(--rg-accent) 12%, transparent)}
  .ws-tab:disabled{opacity:.5}
  .ws-label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1}
  .ws-close{display:flex;align-items:center;justify-content:center;width:14px;height:14px;border-radius:3px;opacity:.5;flex-shrink:0}
  .ws-close:active{background:rgba(255,255,255,.1);opacity:1}

  .pane-tabs{display:flex;gap:2px;overflow-x:auto;overflow-y:hidden;min-height:24px;scrollbar-width:none;-webkit-overflow-scrolling:touch}
  .pane-tabs::-webkit-scrollbar{display:none}
  .pane-tab{display:flex;align-items:center;gap:3px;padding:2px 8px;border:1px solid var(--rg-border-bright);border-radius:5px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:11px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0;max-width:180px}
  .pane-tab.active{border-color:var(--rg-accent);color:var(--rg-fg);background:color-mix(in srgb, var(--rg-accent) 10%, transparent)}
  .dot{color:var(--rg-accent);font-weight:700;font-size:10px;flex-shrink:0}
  .label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:500}
  .cwd{font-size:9px;color:var(--rg-fg-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:60px;flex-shrink:1}
  .empty-msg{color:var(--rg-fg-muted);font-size:10px;padding:1px 4px}

  .right-controls{display:flex;align-items:center;gap:1px;flex-shrink:0;padding-left:2px}
  .ctrl-btn{display:flex;align-items:center;justify-content:center;width:24px;height:24px;background:none;border:none;border-radius:4px;color:var(--rg-fg-muted);cursor:pointer;transition:all .15s}
  .ctrl-btn.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 10%, transparent)}
  .ctrl-btn:active{background:var(--rg-surface-2);color:var(--rg-fg)}
  .ctrl-sep{width:1px;height:16px;background:var(--rg-border-bright);margin:0 2px}
  .kb-icon{font-size:12px;line-height:1}
  .refresh-icon{font-size:14px;line-height:1;font-weight:700}
  .action-btn{color:var(--rg-fg-muted)}

  .status-dot{font-size:8px;color:var(--rg-fg-muted);margin-left:2px;line-height:1}
  .status-dot.connected{color:var(--rg-ansi-green)}
  .status-dot.error{color:var(--rg-ansi-red)}
</style>
