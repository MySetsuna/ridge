<script lang="ts">
  import { onMount } from 'svelte';
  import { Folder, GitBranch, Search, X } from 'lucide-svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import BottomTabBar from './BottomTabBar.svelte';
  import { RemoteConnection, type PaneInfo, type ConnectionState, type WorkspaceInfo, type FileEntry, type GitStatus } from './lib/wsRemote';

  let { ws }: { ws: RemoteConnection } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  let wsState = $state<ConnectionState>('disconnected');
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');
  let showKeyboard = $state(false);
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);

  let canvasRef: TerminalCanvas | undefined = $state();
  let files: FileEntry[] = $state([]);
  let currentPath = $state('');
  let gitStatus = $state<GitStatus>({ staged: [], unstaged: [], commits: [] });
  let searchQuery = $state('');
  let searchResults: FileEntry[] = $state([]);

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

  function navigateDir(path: string) {
    currentPath = path;
    ws.listFiles(path);
  }

  function goUp() {
    const parts = currentPath.split('/').filter(Boolean);
    parts.pop();
    navigateDir(parts.join('/') || '/');
  }

  function handleSidebarToggle(tab: 'files' | 'git' | 'search') {
    if (sidebarTab === tab) {
      sidebarTab = null;
    } else {
      sidebarTab = tab;
      if (tab === 'files') ws.listFiles(currentPath || '');
      else if (tab === 'git') ws.listGitStatus();
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
      if (msg.type === 'files') {
        files = msg.entries;
      } else if (msg.type === 'git-status') {
        gitStatus = msg;
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
          <div class="file-header">
            <span class="path">{currentPath || '/'}</span>
            {#if currentPath}
              <button class="up-btn" onclick={goUp}>..</button>
            {/if}
          </div>
          <div class="file-list" role="list" ontouchmove={(e) => e.stopPropagation()}>
            {#each files as entry}
              <button
                class="file-entry"
                class:dir={entry.is_dir}
                class:ignored={entry.is_ignored === true}
                onclick={() => entry.is_dir && navigateDir(entry.path)}
              >
                <Folder class="w-4 h-4 shrink-0 text-[#58a6ff]" />
                <span class="name">{entry.name}</span>
              </button>
            {/each}
            {#if files.length === 0}
              <span class="empty-hint">文件列表为空</span>
            {/if}
          </div>
        {:else if sidebarTab === 'git'}
          <div class="git-view">
            {#if gitStatus.staged.length > 0}
              <p class="section-title">暂存</p>
              {#each gitStatus.staged as s}<p class="git-item staged">{s}</p>{/each}
            {/if}
            {#if gitStatus.unstaged.length > 0}
              <p class="section-title">未暂存</p>
              {#each gitStatus.unstaged as u}<p class="git-item">{u.status} {u.name}</p>{/each}
            {/if}
            {#if gitStatus.staged.length === 0 && gitStatus.unstaged.length === 0}
              <span class="empty-hint">工作区干净</span>
            {/if}
            {#if gitStatus.commits.length > 0}
              <p class="section-title" style="margin-top:12px">最近提交</p>
              {#each gitStatus.commits as c}
                <p class="commit-item">
                  <span class="hash">{c.hash.slice(0,7)}</span>
                  <span class="msg">{c.msg}</span>
                </p>
              {/each}
            {/if}
          </div>
        {:else}
          <input
            type="search" placeholder="搜索文件..."
            value={searchQuery}
            oninput={(e) => {
              searchQuery = (e.target as HTMLInputElement).value;
              if (searchQuery.length >= 2) ws.send({ type: 'search-files', query: searchQuery });
            }}
            class="search-input"
          />
          <div class="search-results">
            {#each searchResults as r}<p class="search-item">{r.path}</p>{/each}
          </div>
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
  .sidebar-header{display:flex;align-items:center;justify-content:space-between;padding:12px 16px;border-bottom:1px solid #30363d;min-height:48px}
  .sidebar-title{font-size:15px;font-weight:600;color:#e6edf3}
  .close-btn{background:none;border:none;color:#8b949e;padding:4px;border-radius:6px;cursor:pointer}
  .close-btn:active{background:#21262d}
  .sidebar-body{flex:1;overflow-y:auto;padding:8px;-webkit-overflow-scrolling:touch}

  .file-header{display:flex;align-items:center;gap:4px;margin-bottom:8px}
  .path{font-size:11px;color:#8b949e;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1}
  .up-btn{background:none;border:1px solid #30363d;border-radius:4px;color:#8b949e;padding:2px 8px;font-size:11px;cursor:pointer}
  .file-list{display:flex;flex-direction:column;gap:1px}
  .file-entry{display:flex;align-items:center;gap:8px;width:100%;background:none;border:none;color:#e6edf3;padding:10px 12px;border-radius:6px;font-size:14px;cursor:pointer;text-align:left}
  .file-entry.dir{color:#58a6ff}
  .file-entry.ignored{opacity:.5;color:#484f58}
  .name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .empty-hint{color:#484f58;font-size:12px;padding:8px}
  .section-title{font-size:11px;color:#8b949e;text-transform:uppercase;margin:8px 0 4px;letter-spacing:.5px}
  .git-item{font-size:13px;color:#e6edf3;padding:2px 4px}
  .git-item.staged{color:#3fb950}
  .commit-item{font-size:12px;color:#8b949e;padding:2px 4px;display:flex;gap:6px}
  .hash{color:#d2a8ff;font-family:monospace}
  .msg{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .search-input{width:100%;padding:10px 12px;border:1px solid #30363d;border-radius:8px;background:#0d1117;color:#e6edf3;font-size:14px;outline:none}
  .search-input:focus{border-color:#58a6ff}
  .search-results{display:flex;flex-direction:column;gap:1px;margin-top:8px}
  .search-item{font-size:13px;color:#e6edf3;padding:3px 4px}
</style>
