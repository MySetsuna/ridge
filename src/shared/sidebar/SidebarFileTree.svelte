<script lang="ts">
  import { Folder, File as FileIcon, ChevronUp, RefreshCw } from 'lucide-svelte';
  import type { SidebarProvider, FileEntry } from './types';

  let { provider, onOpenFile }: {
    provider: SidebarProvider;
    onOpenFile?: (path: string) => void;
  } = $props();

  let path = $state('');
  let parent = $state<string | null>(null);
  let entries = $state<FileEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function load(target: string) {
    loading = true;
    error = null;
    try {
      const listing = await provider.listDir(target);
      path = listing.path;
      parent = listing.parent ?? null;
      entries = listing.entries;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function onEntry(entry: FileEntry) {
    if (entry.is_dir) load(entry.path);
    else onOpenFile?.(entry.path);
  }

  // Initial load uses the provider's default root (pane cwd).
  $effect(() => {
    void load('');
  });
</script>

<div class="ft">
  <div class="ft-bar">
    <button class="icon-btn" disabled={!parent} onclick={() => parent && load(parent)} title="上级目录">
      <ChevronUp class="w-4 h-4" />
    </button>
    <span class="ft-path" title={path}>{path || '/'}</span>
    <button class="icon-btn" onclick={() => load(path)} title="刷新">
      <RefreshCw class="w-4 h-4" />
    </button>
  </div>

  <div class="ft-list" role="list">
    {#if error}
      <span class="ft-msg err">{error}</span>
    {:else if loading && entries.length === 0}
      <span class="ft-msg">加载中…</span>
    {:else if entries.length === 0}
      <span class="ft-msg">空目录</span>
    {:else}
      {#each entries as entry (entry.path)}
        <button
          class="ft-entry"
          class:dir={entry.is_dir}
          class:ignored={entry.is_ignored === true}
          onclick={() => onEntry(entry)}
        >
          {#if entry.is_dir}
            <Folder class="w-4 h-4 shrink-0 ico-dir" />
          {:else}
            <FileIcon class="w-4 h-4 shrink-0 ico-file" />
          {/if}
          <span class="ft-name">{entry.name}</span>
        </button>
      {/each}
    {/if}
  </div>
</div>

<style>
  .ft { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .ft-bar { display: flex; align-items: center; gap: 4px; padding: 4px 6px; border-bottom: 1px solid #21262d; }
  .ft-path { flex: 1; min-width: 0; font-size: 11px; color: #8b949e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl; text-align: left; }
  .icon-btn { display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; border: none; background: none; color: #8b949e; border-radius: 6px; cursor: pointer; }
  .icon-btn:disabled { opacity: .35; }
  .icon-btn:active { background: #21262d; color: #e6edf3; }

  .ft-list { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 1px; padding: 4px; -webkit-overflow-scrolling: touch; }
  .ft-entry { display: flex; align-items: center; gap: 8px; width: 100%; background: none; border: none; color: #e6edf3; padding: 9px 10px; border-radius: 6px; font-size: 14px; cursor: pointer; text-align: left; }
  .ft-entry:active { background: #1c2128; }
  .ft-entry.dir { color: #58a6ff; }
  .ft-entry.ignored { opacity: .5; }
  :global(.ft-entry .ico-dir) { color: #58a6ff; }
  :global(.ft-entry .ico-file) { color: #8b949e; }
  .ft-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ft-msg { color: #484f58; font-size: 12px; padding: 10px; }
  .ft-msg.err { color: #f85149; }
</style>
