<script lang="ts">
  import { onMount } from 'svelte';
  import { getTransport } from '$lib/transport';
  import type { FileNode, DirectoryPage } from '$lib/stores/project';
  import { Folder, File, ChevronRight, ChevronDown, RefreshCw, GitBranch, Search, X, ArrowUp, Home } from 'lucide-svelte';

  let { onClose }: { onClose: () => void } = $props();

  let tab = $state<'files' | 'git' | 'search'>('files');
  let files: FileEntry[] = $state([]);
  let currentPath = $state('');
  let loading = $state(false);
  let error = $state('');

  // Git state
  let gitBranch = $state('');
  let gitStaged: string[] = $state([]);
  let gitUnstaged: { name: string; status: string }[] = $state([]);
  let gitCommits: { hash: string; msg: string; time: string }[] = $state([]);
  let gitLoading = $state(false);

  // Search state
  let searchQuery = $state('');
  let searchResults: string[] = $state([]);
  let searchLoading = $state(false);

  interface FileEntry {
    name: string;
    path: string;
    is_dir: boolean;
    is_ignored?: boolean;
    child_count?: number;
  }

  let expandedDirs = $state<Set<string>>(new Set());
  let dirChildren = $state<Map<string, FileEntry[]>>(new Map());

  async function loadDir(path: string) {
    loading = true;
    error = '';
    try {
      const transport = getTransport();
      const tree = await transport.getFileTree(path || '/', 1);
      currentPath = path;
      const entries: FileEntry[] = (tree.children || []).map(c => ({
        name: c.name,
        path: c.path,
        is_dir: c.is_dir,
        is_ignored: (c as unknown as { is_ignored?: boolean }).is_ignored,
        child_count: (c as unknown as { child_count?: number }).child_count,
      }));
      files = entries;
      dirChildren.set(path, entries);
      dirChildren = new Map(dirChildren);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function toggleDir(path: string) {
    if (expandedDirs.has(path)) {
      expandedDirs.delete(path);
      expandedDirs = new Set(expandedDirs);
    } else {
      expandedDirs.add(path);
      expandedDirs = new Set(expandedDirs);
      if (!dirChildren.has(path)) {
        await loadDir(path);
      }
    }
  }

  function goUp() {
    const parts = currentPath.split('/').filter(Boolean);
    parts.pop();
    loadDir('/' + parts.join('/'));
  }

  function goHome() {
    loadDir('/');
  }

  async function loadGitStatus() {
    gitLoading = true;
    try {
      const transport = getTransport();
      const status = await transport.gitStatus(currentPath || '/');
      gitBranch = (status as unknown as { current_branch?: string }).current_branch || '';
      gitStaged = (status as unknown as { staged?: string[] }).staged || [];
      gitUnstaged = (status as unknown as { changes?: { name: string; status: string }[] }).changes || [];
      gitCommits = (status as unknown as { commits?: { hash: string; msg: string; time: string }[] }).commits || [];
    } catch (e) {
      error = String(e);
    } finally {
      gitLoading = false;
    }
  }

  async function doSearch() {
    if (searchQuery.length < 2) return;
    searchLoading = true;
    try {
      const transport = getTransport();
      const results = await transport.searchFiles(searchQuery, currentPath || undefined);
      searchResults = results.map(r => typeof r === 'string' ? r : (r as unknown as { path: string }).path);
    } catch (e) {
      error = String(e);
    } finally {
      searchLoading = false;
    }
  }

  onMount(() => {
    loadDir('/');
  });

  $effect(() => {
    if (tab === 'git' && !gitLoading) loadGitStatus();
  });
</script>

<div class="sidebar" role="dialog" aria-label="Sidebar">
  <div class="sidebar-header">
    <div class="tab-bar">
      <button class="tab-btn" class:active={tab === 'files'} onclick={() => tab = 'files'}>
        <Folder class="w-4 h-4" />
      </button>
      <button class="tab-btn" class:active={tab === 'git'} onclick={() => tab = 'git'}>
        <GitBranch class="w-4 h-4" />
      </button>
      <button class="tab-btn" class:active={tab === 'search'} onclick={() => tab = 'search'}>
        <Search class="w-4 h-4" />
      </button>
    </div>
    <button class="close-btn" onclick={onClose}>
      <X class="w-5 h-5" />
    </button>
  </div>

  <div class="sidebar-body">
    {#if tab === 'files'}
      <div class="file-header">
        <button class="icon-btn" onclick={goHome} title="Root"><Home class="w-3.5 h-3.5" /></button>
        <span class="path">{currentPath || '/'}</span>
        <button class="icon-btn" onclick={goUp} title="Go up"><ArrowUp class="w-3.5 h-3.5" /></button>
        <button class="icon-btn" onclick={() => loadDir(currentPath)} title="Refresh"><RefreshCw class="w-3.5 h-3.5" /></button>
      </div>
      {#if loading}
        <div class="empty-hint">Loading...</div>
      {:else if error}
        <div class="error-hint">{error}</div>
      {:else}
        {#each (dirChildren.get(currentPath) || files) as entry}
          <button
            class="file-entry"
            class:dir={entry.is_dir}
            class:ignored={entry.is_ignored === true}
            onclick={() => entry.is_dir && toggleDir(entry.path)}
          >
            {#if entry.is_dir}
              {#if expandedDirs.has(entry.path)}
                <ChevronDown class="w-3 h-3 shrink-0 text-[#58a6ff]" />
              {:else}
                <ChevronRight class="w-3 h-3 shrink-0 text-[#8b949e]" />
              {/if}
              <Folder class="w-4 h-4 shrink-0 text-[#58a6ff]" />
            {:else}
              <span class="w-3"></span>
              <File class="w-4 h-4 shrink-0 text-[#8b949e]" />
            {/if}
            <span class="name">{entry.name}</span>
          </button>
          {#if entry.is_dir && expandedDirs.has(entry.path)}
            {#each (dirChildren.get(entry.path) || []) as child}
              <button
                class="file-entry nested"
                class:dir={child.is_dir}
                class:ignored={child.is_ignored === true}
                onclick={() => child.is_dir && toggleDir(child.path)}
              >
                {#if child.is_dir}
                  {#if expandedDirs.has(child.path)}
                    <ChevronDown class="w-3 h-3 shrink-0 text-[#58a6ff]" />
                  {:else}
                    <ChevronRight class="w-3 h-3 shrink-0 text-[#8b949e]" />
                  {/if}
                  <Folder class="w-4 h-4 shrink-0 text-[#58a6ff]" />
                {:else}
                  <span class="w-3"></span>
                  <File class="w-4 h-4 shrink-0 text-[#8b949e]" />
                {/if}
                <span class="name">{child.name}</span>
              </button>
            {/each}
          {/if}
        {/each}
        {#if files.length === 0 && !loading}
          <span class="empty-hint">Empty directory</span>
        {/if}
      {/if}
    {:else if tab === 'git'}
      {#if gitLoading}
        <div class="empty-hint">Loading git status...</div>
      {:else}
        {#if gitBranch}
          <div class="branch-info">{gitBranch}</div>
        {/if}
        {#if gitStaged.length > 0}
          <p class="section-title">Staged</p>
          {#each gitStaged as s}<p class="git-item staged">{s}</p>{/each}
        {/if}
        {#if gitUnstaged.length > 0}
          <p class="section-title">Unstaged</p>
          {#each gitUnstaged as u}<p class="git-item">{u.status} {u.name}</p>{/each}
        {/if}
        {#if gitStaged.length === 0 && gitUnstaged.length === 0}
          <span class="empty-hint">Working tree clean</span>
        {/if}
        {#if gitCommits.length > 0}
          <p class="section-title" style="margin-top:12px">Recent commits</p>
          {#each gitCommits as c}
            <p class="commit-item">
              <span class="hash">{c.hash.slice(0, 7)}</span>
              <span class="msg">{c.msg}</span>
            </p>
          {/each}
        {/if}
      {/if}
    {:else}
      <input
        type="search"
        placeholder="Search files..."
        value={searchQuery}
        oninput={(e) => { searchQuery = (e.target as HTMLInputElement).value; }}
        onkeydown={(e) => { if (e.key === 'Enter') doSearch(); }}
        class="search-input"
      />
      <button class="search-btn" onclick={doSearch} disabled={searchLoading}>
        {searchLoading ? 'Searching...' : 'Search'}
      </button>
      <div class="search-results">
        {#each searchResults as r}
          <p class="search-item">{r}</p>
        {/each}
        {#if searchResults.length === 0 && searchQuery.length >= 2 && !searchLoading}
          <span class="empty-hint">No results</span>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .sidebar{position:fixed;inset:0;z-index:50;display:flex;flex-direction:column;background:#161b22;animation:slideIn .2s ease-out}
  @keyframes slideIn{from{transform:translateX(-100%)}to{transform:translateX(0)}}
  .sidebar-header{display:flex;align-items:center;justify-content:space-between;padding:8px 12px;border-bottom:1px solid #30363d;min-height:48px}
  .tab-bar{display:flex;gap:4px}
  .tab-btn{display:flex;align-items:center;justify-content:center;width:32px;height:32px;border:none;border-radius:6px;background:none;color:#8b949e;cursor:pointer;transition:all .12s}
  .tab-btn.active{color:#58a6ff;background:rgba(88,166,255,.12)}
  .tab-btn:active{background:#21262d}
  .close-btn{background:none;border:none;color:#8b949e;padding:4px;border-radius:6px;cursor:pointer}
  .close-btn:active{background:#21262d}
  .sidebar-body{flex:1;overflow-y:auto;padding:8px;-webkit-overflow-scrolling:touch}

  .file-header{display:flex;align-items:center;gap:4px;margin-bottom:8px}
  .path{font-size:11px;color:#8b949e;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1}
  .icon-btn{display:flex;align-items:center;justify-content:center;width:24px;height:24px;background:none;border:1px solid #30363d;border-radius:4px;color:#8b949e;cursor:pointer;padding:0}
  .icon-btn:active{background:#21262d;color:#e6edf3}
  .file-entry{display:flex;align-items:center;gap:4px;width:100%;background:none;border:none;color:#e6edf3;padding:6px 8px;border-radius:4px;font-size:13px;cursor:pointer;text-align:left}
  .file-entry:hover{background:#21262d}
  .file-entry:active{background:#30363d}
  .file-entry.dir{color:#58a6ff}
  .file-entry.ignored{opacity:.5;color:#484f58}
  .file-entry.nested{padding-left:20px}
  .name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .empty-hint{color:#484f58;font-size:12px;padding:8px}
  .error-hint{color:#f85149;font-size:12px;padding:8px;word-break:break-all}
  .section-title{font-size:11px;color:#8b949e;text-transform:uppercase;margin:8px 0 4px;letter-spacing:.5px}
  .git-item{font-size:13px;color:#e6edf3;padding:2px 4px}
  .git-item.staged{color:#3fb950}
  .branch-info{font-size:12px;color:#d2a8ff;padding:4px 8px;margin-bottom:4px;background:rgba(210,168,255,.08);border-radius:4px}
  .commit-item{font-size:12px;color:#8b949e;padding:2px 4px;display:flex;gap:6px}
  .hash{color:#d2a8ff;font-family:monospace}
  .msg{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .search-input{width:100%;padding:8px 10px;border:1px solid #30363d;border-radius:6px;background:#0d1117;color:#e6edf3;font-size:13px;outline:none}
  .search-input:focus{border-color:#58a6ff}
  .search-btn{width:100%;margin-top:6px;padding:6px;border:1px solid #30363d;border-radius:6px;background:#21262d;color:#e6edf3;font-size:13px;cursor:pointer}
  .search-btn:active{background:#30363d}
  .search-btn:disabled{opacity:.5}
  .search-results{display:flex;flex-direction:column;gap:1px;margin-top:8px}
  .search-item{font-size:12px;color:#e6edf3;padding:3px 4px;cursor:pointer}
  .search-item:hover{background:#21262d}
  .w-3{width:12px}.w-3\.5{width:14px}.w-4{width:16px}.w-5{width:20px}.h-3{height:12px}.h-3\.5{height:14px}.h-4{height:16px}.h-5{height:20px}
  .shrink-0{flex-shrink:0}.text-\[\#58a6ff\]{color:#58a6ff}.text-\[\#8b949e\]{color:#8b949e}
</style>