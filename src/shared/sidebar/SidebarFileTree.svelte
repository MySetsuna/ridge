<script lang="ts">
  import { Folder, File as FileIcon, ChevronUp, RefreshCw } from 'lucide-svelte';
  import { onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import type { SidebarProvider, FileEntry } from './types';

  let { provider, onOpenFile, initialPath = '' }: {
    provider: SidebarProvider;
    onOpenFile?: (path: string) => void;
    initialPath?: string;
  } = $props();

  let path = $state('');
  let parent = $state<string | null>(null);
  let entries = $state<FileEntry[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let requestGeneration = 0;
  let requestController: AbortController | null = null;

  async function load(target: string, force = false) {
    requestController?.abort();
    const controller = new AbortController();
    requestController = controller;
    const generation = ++requestGeneration;
    loading = true;
    error = null;
    try {
      const listing = force && provider.refreshDir
        ? await provider.refreshDir(target, controller.signal)
        : await provider.listDir(target, controller.signal);
      if (controller.signal.aborted || generation !== requestGeneration) return;
      path = listing.path;
      parent = listing.parent ?? null;
      entries = listing.entries;
    } catch (e) {
      if (controller.signal.aborted || generation !== requestGeneration) return;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      if (generation === requestGeneration) loading = false;
    }
  }

  onDestroy(() => {
    requestController?.abort();
    requestController = null;
    requestGeneration += 1;
  });

  function onEntry(entry: FileEntry) {
    if (entry.is_dir) load(entry.path);
    else onOpenFile?.(entry.path);
  }

  // Terminal directory links may supply an origin-validated absolute path;
  // ordinary mounts keep the provider's pane-cwd root.
  $effect(() => {
    void load(initialPath);
  });
</script>

<div class="ft">
  <div class="ft-bar">
    <button class="icon-btn" disabled={!parent} onclick={() => parent && load(parent)} title={$t('mobile.parentDir')}>
      <ChevronUp class="w-4 h-4" />
    </button>
    <span class="ft-path" title={path}>{path || '/'}</span>
    <button class="icon-btn" onclick={() => void load(path, true)} title={$t('mobile.refresh')}>
      <RefreshCw class="w-4 h-4" />
    </button>
  </div>

  <div class="ft-list" role="list">
    {#if error}
      <span class="ft-msg err">{error}</span>
    {:else if loading && entries.length === 0}
      <span class="ft-msg">{$t('mobile.loading')}</span>
    {:else if entries.length === 0}
      <span class="ft-msg">{$t('mobile.emptyDir')}</span>
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
  .ft-bar { display: flex; align-items: center; gap: 4px; padding: 4px 6px; border-bottom: 1px solid var(--rg-border-bright); }
  .ft-path { flex: 1; min-width: 0; font-size: 11px; color: var(--rg-fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; direction: rtl; text-align: left; }
  .icon-btn { display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; border: none; background: none; color: var(--rg-fg-muted); border-radius: 6px; cursor: pointer; }
  .icon-btn:disabled { opacity: .35; }
  .icon-btn:active { background: var(--rg-surface-2); color: var(--rg-fg); }

  .ft-list { flex: 1; min-height: 0; overflow-y: auto; display: flex; flex-direction: column; gap: 1px; padding: 4px; -webkit-overflow-scrolling: touch; }
  .ft-entry { display: flex; align-items: center; gap: 8px; width: 100%; background: none; border: none; color: var(--rg-fg); padding: 9px 10px; border-radius: 6px; font-size: 14px; cursor: pointer; text-align: left; }
  .ft-entry:active { background: var(--rg-surface-2); }
  .ft-entry.dir { color: var(--rg-accent); }
  .ft-entry.ignored { opacity: .5; }
  :global(.ft-entry .ico-dir) { color: var(--rg-accent); }
  :global(.ft-entry .ico-file) { color: var(--rg-fg-muted); }
  .ft-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ft-msg { color: var(--rg-fg-muted); font-size: 12px; padding: 10px; }
  .ft-msg.err { color: var(--rg-ansi-red); }
</style>
