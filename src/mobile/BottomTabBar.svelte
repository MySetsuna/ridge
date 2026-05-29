<script lang="ts">
  import { Plus, X, Folder, GitBranch, Search, Layers, ChevronUp } from 'lucide-svelte';
  import type { PaneInfo, WorkspaceInfo, RemoteConnection } from './lib/wsRemote';

  let { panes, activePaneId = $bindable(), workspaces = [], activeWorkspaceId = '', ws,
    sidebarTab = null as 'files' | 'git' | 'search' | null, onSidebarToggle
  }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    ws?: RemoteConnection;
    sidebarTab?: 'files' | 'git' | 'search' | null;
    onSidebarToggle?: (tab: 'files' | 'git' | 'search') => void;
  } = $props();

  let wsSwitching = $state(false);
  let menuOpen = $state(false);
  let paneMenuOpen = $state(false);
  let paneBusy = $state(false);

  const activeWs = $derived(workspaces.find((w) => w.id === activeWorkspaceId));
  const activeWsName = $derived(activeWs?.name || '工作区');
  const activePane = $derived(panes.find((p) => p.id === activePaneId));
  const activePaneName = $derived(activePane?.title || '终端');

  async function handleSwitchWorkspace(wsId: string) {
    if (wsSwitching || !ws) return;
    wsSwitching = true;
    try {
      await ws.switchWorkspace(wsId);
      ws.listPanes();
    } finally {
      wsSwitching = false;
      menuOpen = false;
    }
  }

  async function handleCreateWorkspace() {
    if (!ws) return;
    const id = await ws.createWorkspace();
    if (id) ws.listPanes();
    menuOpen = false;
  }

  async function handleCloseWorkspace(e: Event, wsId: string) {
    e.stopPropagation();
    if (!ws) return;
    await ws.closeWorkspace(wsId);
    ws.listPanes();
  }

  // ── Terminal (pane) menu — §6 ─────────────────────────────────────
  function handleSelectPane(paneId: string) {
    activePaneId = paneId;
    paneMenuOpen = false;
  }

  async function handleCreatePane() {
    if (!ws || paneBusy) return;
    paneBusy = true;
    try {
      const id = await ws.createPane();
      ws.listPanes();
      if (id) activePaneId = id; // drives subscribe + kernel reset in MainApp
    } finally {
      paneBusy = false;
      paneMenuOpen = false;
    }
  }

  async function handleClosePane(e: Event, paneId: string) {
    e.stopPropagation();
    if (!ws || paneBusy) return;
    paneBusy = true;
    try {
      await ws.closePane(paneId);
      ws.listPanes();
      // If we closed the active pane, MainApp's `panes` handler falls back to
      // the first remaining pane on the next list-panes.
    } finally {
      paneBusy = false;
    }
  }
</script>

<div class="bar">
  <!-- Workspace menu button: current workspace + collapsed switch/create/delete -->
  <div class="ws-menu">
    <button class="ws-btn" class:open={menuOpen} onclick={() => (menuOpen = !menuOpen)} title="工作区">
      <Layers class="w-3.5 h-3.5 shrink-0" />
      <span class="ws-name">{activeWsName}</span>
      <span class="chev" class:flip={!menuOpen}><ChevronUp class="w-3 h-3 shrink-0" /></span>
    </button>

    {#if menuOpen}
      <div class="ws-overlay" onclick={() => (menuOpen = false)} role="presentation"></div>
      <div class="ws-popup" role="menu">
        <div class="ws-popup-head">工作区</div>
        {#each workspaces as wsp (wsp.id)}
          <div class="ws-row" class:active={wsp.id === activeWorkspaceId}>
            <button class="ws-row-main" onclick={() => handleSwitchWorkspace(wsp.id)} disabled={wsSwitching}>
              <span class="dot" class:on={wsp.id === activeWorkspaceId}></span>
              <span class="nm">{wsp.name || '工作区'}</span>
            </button>
            {#if workspaces.length > 1}
              <button class="ws-del" title="关闭" onclick={(e) => handleCloseWorkspace(e, wsp.id)}>
                <X class="w-3.5 h-3.5" />
              </button>
            {/if}
          </div>
        {/each}
        {#if workspaces.length === 0}
          <div class="ws-empty">无工作区</div>
        {/if}
        <button class="ws-create" onclick={handleCreateWorkspace}>
          <Plus class="w-3.5 h-3.5" /> 新建工作区
        </button>
      </div>
    {/if}
  </div>

  <!-- Terminal menu: current terminal + collapsed select/create/close (§6) -->
  <div class="pane-menu">
    <button class="pane-btn" class:open={paneMenuOpen} onclick={() => (paneMenuOpen = !paneMenuOpen)} title="终端">
      <span class="pdot">▸</span>
      <span class="pane-name">{panes.length === 0 ? '无终端' : activePaneName}</span>
      <span class="chev" class:flip={!paneMenuOpen}><ChevronUp class="w-3 h-3 shrink-0" /></span>
    </button>

    {#if paneMenuOpen}
      <div class="ws-overlay" onclick={() => (paneMenuOpen = false)} role="presentation"></div>
      <div class="ws-popup" role="menu">
        <div class="ws-popup-head">终端</div>
        {#each panes as pane (pane.id)}
          <div class="ws-row" class:active={pane.id === activePaneId}>
            <button class="ws-row-main" onclick={() => handleSelectPane(pane.id)}>
              <span class="dot" class:on={pane.id === activePaneId}></span>
              <span class="nm">{pane.title || '终端'}</span>
            </button>
            {#if panes.length > 1}
              <button class="ws-del" title="关闭" onclick={(e) => handleClosePane(e, pane.id)} disabled={paneBusy}>
                <X class="w-3.5 h-3.5" />
              </button>
            {/if}
          </div>
        {/each}
        {#if panes.length === 0}
          <div class="ws-empty">无终端</div>
        {/if}
        <button class="ws-create" onclick={handleCreatePane} disabled={paneBusy}>
          <Plus class="w-3.5 h-3.5" /> 新建终端
        </button>
      </div>
    {/if}
  </div>

  <div class="bar-spacer"></div>

  <!-- Right controls -->
  <div class="right-controls">
    <button class="ctrl-btn" class:active={sidebarTab === 'files'} onclick={() => onSidebarToggle?.('files')} title="文件">
      <Folder class="w-3.5 h-3.5" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'git'} onclick={() => onSidebarToggle?.('git')} title="Git">
      <GitBranch class="w-3.5 h-3.5" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'search'} onclick={() => onSidebarToggle?.('search')} title="搜索">
      <Search class="w-3.5 h-3.5" />
    </button>
  </div>
</div>

<style>
  .bar{display:flex;align-items:center;gap:6px;padding:5px 8px;background:#161b22;border-top:1px solid #30363d;flex-shrink:0;min-height:44px}

  /* Workspace menu */
  .ws-menu{position:relative;flex-shrink:0}
  .ws-btn{display:flex;align-items:center;gap:4px;max-width:130px;padding:4px 8px;border:1px solid #30363d;border-radius:7px;background:#0d1117;color:#e6edf3;font-size:12px;cursor:pointer}
  .ws-btn.open{border-color:#58a6ff;color:#58a6ff}
  .ws-name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .chev{display:inline-flex;align-items:center;transition:transform .15s}
  .chev.flip{transform:rotate(180deg)}

  .ws-overlay{position:fixed;inset:0;z-index:70}
  .ws-popup{position:absolute;bottom:calc(100% + 6px);left:0;z-index:71;min-width:200px;max-height:50vh;overflow-y:auto;background:#161b22;border:1px solid #30363d;border-radius:10px;box-shadow:0 8px 24px rgba(0,0,0,.5);padding:6px}
  .ws-popup-head{font-size:10px;color:#8b949e;text-transform:uppercase;letter-spacing:.5px;padding:4px 8px}
  .ws-row{display:flex;align-items:center;border-radius:6px}
  .ws-row.active{background:rgba(88,166,255,.1)}
  .ws-row-main{flex:1;min-width:0;display:flex;align-items:center;gap:8px;background:none;border:none;color:#e6edf3;padding:9px 8px;font-size:14px;cursor:pointer;text-align:left}
  .ws-row-main:disabled{opacity:.5}
  .dot{width:7px;height:7px;border-radius:50%;background:#30363d;flex-shrink:0}
  .dot.on{background:#3fb950}
  .nm{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .ws-del{display:flex;align-items:center;justify-content:center;width:28px;height:28px;background:none;border:none;color:#8b949e;border-radius:6px;cursor:pointer;flex-shrink:0}
  .ws-del:active{background:#21262d;color:#f85149}
  .ws-empty{color:#484f58;font-size:12px;padding:8px}
  .ws-create{display:flex;align-items:center;gap:6px;width:100%;margin-top:4px;padding:9px 8px;background:none;border:none;border-top:1px solid #21262d;color:#58a6ff;font-size:13px;cursor:pointer;text-align:left}
  .ws-create:active{background:#21262d}

  /* Terminal menu (mirrors the workspace menu) */
  .pane-menu{position:relative;flex-shrink:0}
  .pane-btn{display:flex;align-items:center;gap:4px;max-width:150px;padding:4px 8px;border:1px solid #30363d;border-radius:7px;background:#0d1117;color:#e6edf3;font-size:12px;cursor:pointer}
  .pane-btn.open{border-color:#58a6ff;color:#58a6ff}
  .pane-name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .pdot{color:#58a6ff;font-weight:700;font-size:10px;flex-shrink:0}
  .bar-spacer{flex:1;min-width:0}

  /* Right controls */
  .right-controls{display:flex;align-items:center;gap:2px;flex-shrink:0}
  .ctrl-btn{display:flex;align-items:center;justify-content:center;width:28px;height:28px;background:none;border:none;border-radius:6px;color:#8b949e;cursor:pointer}
  .ctrl-btn.active{color:#58a6ff;background:rgba(88,166,255,.1)}
  .ctrl-btn:active{background:#21262d;color:#e6edf3}
</style>
