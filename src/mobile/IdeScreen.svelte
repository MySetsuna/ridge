<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import type { RemoteConnection, PaneInfo, FileEntry, GitStatus } from './lib/wsRemote';

  let { ws, panes, activePaneId = $bindable() }: {
    ws: RemoteConnection;
    panes: PaneInfo[];
    activePaneId?: string | null;
  } = $props();

  let canvasRef: TerminalCanvas | undefined = $state();
  let sidebarTab: 'files' | 'git' | 'search' = $state('files');
  let files: FileEntry[] = $state([]);
  let currentPath = $state('');
  let gitStatus = $state<GitStatus>({ staged: [], unstaged: [], commits: [] });
  let searchQuery = $state('');
  let searchResults: FileEntry[] = $state([]);
  let unsub: (() => void) | undefined;

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
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

  onMount(() => {
    ws.listFiles('');
    ws.listGitStatus();
    unsub = ws.onMessage((msg) => {
      if (msg.type === 'files') {
        files = msg.entries;
      } else if (msg.type === 'git-status') {
        gitStatus = msg;
      } else if (msg.type === 'output' && msg.paneId === activePaneId && canvasRef) {
        canvasRef.feed(msg.data);
      }
    });
    return () => unsub?.();
  });
</script>

<div class="ide-layout">
  <aside class="sidebar">
    <div class="sidebar-tabs">
      <button class="sbtb" class:active={sidebarTab === 'files'} onclick={() => sidebarTab = 'files'}>📁</button>
      <button class="sbtb" class:active={sidebarTab === 'git'} onclick={() => sidebarTab = 'git'}>⎇</button>
      <button class="sbtb" class:active={sidebarTab === 'search'} onclick={() => sidebarTab = 'search'}>🔍</button>
    </div>
    <div class="sidebar-content">
      {#if sidebarTab === 'files'}
        <div class="file-header">
          <span class="path">{currentPath || '/'}</span>
          {#if currentPath}
            <button class="up-btn" onclick={goUp}>..</button>
          {/if}
        </div>
        <div class="file-list">
          {#each files as entry}
            <button
              class="file-entry"
              class:dir={entry.type === 'dir'}
              onclick={() => entry.type === 'dir' && navigateDir(entry.path)}
            >
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
        />
        <div class="search-results">
          {#each searchResults as r}<p class="search-item">{r.path}</p>{/each}
        </div>
      {/if}
    </div>
  </aside>
  <main class="term-panel">
    <TerminalCanvas
      bind:this={canvasRef}
      paneId={activePaneId ?? null}
      {onStdin}
    />
  </main>
</div>

<style>
  .ide-layout{display:flex;flex:1;overflow:hidden}
  .sidebar{width:260px;display:flex;flex-direction:column;border-right:1px solid #30363d;background:#161b22;flex-shrink:0}
  .sidebar-tabs{display:flex;border-bottom:1px solid #30363d}
  .sbtb{flex:1;background:none;border:none;color:#8b949e;padding:8px;font-size:14px;cursor:pointer;transition:all .15s}
  .sbtb.active{color:#e6edf3;background:#21262d;border-bottom:2px solid #58a6ff}
  .sidebar-content{flex:1;overflow-y:auto;padding:8px}
  .file-header{display:flex;align-items:center;gap:4px;margin-bottom:8px}
  .path{font-size:11px;color:#8b949e;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1}
  .up-btn{background:none;border:1px solid #30363d;border-radius:4px;color:#8b949e;padding:2px 8px;font-size:11px;cursor:pointer}
  .file-entry{display:flex;align-items:center;gap:6px;width:100%;background:none;border:none;color:#e6edf3;padding:4px 8px;border-radius:4px;font-size:13px;cursor:pointer;text-align:left}
  .file-entry:hover{background:#21262d}
  .file-entry.dir{color:#58a6ff}
  .name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .empty-hint{color:#484f58;font-size:12px;padding:8px}
  .section-title{font-size:11px;color:#8b949e;text-transform:uppercase;margin:8px 0 4px;letter-spacing:.5px}
  .git-item{font-size:13px;color:#e6edf3;padding:2px 4px}
  .git-item.staged{color:#3fb950}
  .commit-item{font-size:12px;color:#8b949e;padding:2px 4px;display:flex;gap:6px}
  .hash{color:#d2a8ff;font-family:monospace}
  .msg{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .search-item{font-size:13px;color:#e6edf3;padding:3px 4px}
  .term-panel{flex:1;display:flex;flex-direction:column;overflow:hidden}
</style>
