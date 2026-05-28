<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Folder, GitBranch, Search, X, ChevronRight } from 'lucide-svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import type { RemoteConnection, PaneInfo, FileEntry, GitStatus } from './lib/wsRemote';

  let { ws, panes, activePaneId = $bindable() }: {
    ws: RemoteConnection;
    panes: PaneInfo[];
    activePaneId?: string | null;
  } = $props();

  let canvasRef: TerminalCanvas | undefined = $state();
  let showKeyboard = $state(false);
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);
  let files: FileEntry[] = $state([]);
  let currentPath = $state('');
  let gitStatus = $state<GitStatus>({ staged: [], unstaged: [], commits: [] });
  let searchQuery = $state('');
  let searchResults: FileEntry[] = $state([]);
  let unsub: (() => void) | undefined;

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
  }

  function onResize(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    ws.resizePane(paneId, rows, cols, pixelWidth, pixelHeight);
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

  function closeSidebar() {
    sidebarTab = null;
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
  <main class="term-panel">
    <TerminalCanvas
      bind:this={canvasRef}
      paneId={activePaneId ?? null}
      {onStdin}
      {onResize}
      {showKeyboard}
    />
  </main>
  {#if showKeyboard}
    <div class="ide-keyboard-toggle" onclick={() => showKeyboard = false}>
      <button class="keyboard-btn">隐藏键盘</button>
    </div>
  {/if}

  {#if sidebarTab !== null}
    <div class="sidebar-overlay" onclick={closeSidebar} role="presentation"></div>
    <div class="sidebar" role="dialog" aria-label="Sidebar">
      <div class="sidebar-header">
        <span class="sidebar-title">
          {sidebarTab === 'files' ? '文件' : sidebarTab === 'git' ? 'Git' : '搜索'}
        </span>
        <button class="close-btn" onclick={closeSidebar}>
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
          />
          <div class="search-results">
            {#each searchResults as r}<p class="search-item">{r.path}</p>{/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  <div class="sidebar-tab-bar">
    <button
      class="stb-btn"
      class:active={sidebarTab === 'files'}
      onclick={() => sidebarTab = sidebarTab === 'files' ? null : 'files'}
    >
      <Folder class="w-5 h-5" />
    </button>
    <button
      class="stb-btn"
      class:active={sidebarTab === 'git'}
      onclick={() => sidebarTab = sidebarTab === 'git' ? null : 'git'}
    >
      <GitBranch class="w-5 h-5" />
    </button>
    <button
      class="stb-btn"
      class:active={sidebarTab === 'search'}
      onclick={() => sidebarTab = sidebarTab === 'search' ? null : 'search'}
    >
      <Search class="w-5 h-5" />
    </button>
    <div class="stb-sep"></div>
    <button
      class="stb-btn"
      class:active={showKeyboard}
      onclick={() => showKeyboard = !showKeyboard}
      title="虚拟键盘"
    >
      <span class="kb-icon">⌨</span>
    </button>
  </div>
</div>

<style>
  .ide-layout{position:relative;display:flex;flex:1;overflow:hidden}

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
  .file-entry:hover{background:#21262d}
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
  .search-item{font-size:13px;color:#e6edf3;padding:3px 4px}

  .term-panel{flex:1;display:flex;flex-direction:column;overflow:hidden}

  .sidebar-tab-bar{position:fixed;left:0;top:50%;transform:translateY(-50%);display:flex;flex-direction:column;gap:2px;z-index:30;background:#161b22;border:1px solid #30363d;border-left:none;border-radius:0 8px 8px 0;padding:4px}
  .stb-btn{background:none;border:none;color:#8b949e;padding:8px;border-radius:6px;cursor:pointer;transition:all .15s}
  .stb-btn.active{color:#e6edf3;background:#21262d}
  .stb-btn:active{background:#30363d}
  .stb-sep{width:100%;height:1px;background:#30363d;margin:2px 0}
  .kb-icon{font-size:16px;line-height:1}
  .ide-keyboard-toggle{flex-shrink:0;padding:4px 8px;background:#161b22;border-top:1px solid #30363d}
  .keyboard-btn{width:100%;padding:10px;border:1px solid #30363d;border-radius:8px;background:#0d1117;color:#8b949e;font-size:14px;cursor:pointer;text-align:center}
  .keyboard-btn:active{background:#21262d}
</style>
