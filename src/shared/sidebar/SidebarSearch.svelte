<script lang="ts">
  import { Search } from 'lucide-svelte';
  import { onDestroy } from 'svelte';
  import type { SidebarProvider, SearchHit } from './types';
  import { t } from '$lib/i18n';

  let { provider, onOpenFile, query = $bindable('') }: {
    provider: SidebarProvider;
    onOpenFile?: (path: string, line?: number) => void;
    query?: string;
  } = $props();

  let results = $state<SearchHit[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let debounce: ReturnType<typeof setTimeout> | undefined;
  let requestGeneration = 0;
  let requestController: AbortController | null = null;

  function basename(p: string): string {
    const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return i >= 0 ? p.slice(i + 1) : p;
  }

  async function run(q: string, force = false) {
    requestController?.abort();
    const controller = new AbortController();
    requestController = controller;
    const generation = ++requestGeneration;
    if (q.trim().length < 2) {
      results = [];
      loading = false;
      error = null;
      return;
    }
    loading = true;
    error = null;
    try {
      const next = force && provider.refreshSearch ? await provider.refreshSearch(q, controller.signal) : await provider.search(q, controller.signal);
      if (controller.signal.aborted || generation !== requestGeneration || q !== query) return;
      results = next;
    } catch (e) {
      if (controller.signal.aborted || generation !== requestGeneration) return;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      if (generation === requestGeneration) loading = false;
    }
  }

  $effect(() => {
    const q = query;
    const eligible = q.trim().length >= 2;
    requestController?.abort(); requestGeneration++;
    results = []; error = null; loading = eligible;
    if (debounce) clearTimeout(debounce);
    if (eligible) debounce = setTimeout(() => run(q), 250);
    return () => { if (debounce) clearTimeout(debounce); requestController?.abort(); requestGeneration++; };
  });

  onDestroy(() => {
    if (debounce) clearTimeout(debounce);
    requestController?.abort();
    requestController = null;
    requestGeneration += 1;
  });
</script>

<div class="search">
  <div class="search-bar">
    <Search class="w-4 h-4 shrink-0 ico" />
    <input
      class="search-input"
      type="search"
      placeholder={$t('ui.sidebarSearchPlaceholder')}
      value={query}
      oninput={event => query = event.currentTarget.value}
    />
  </div>

  <div class="search-body">
    {#if error}
      <span class="msg err" role="alert">{error}<button onclick={() => run(query, true)}>{$t('mobile.refresh')}</button></span>
    {:else if loading}
      <span class="msg">{$t('ui.sidebarSearching')}</span>
    {:else if query.trim().length >= 2 && results.length === 0}
      <span class="msg">{$t('ui.sidebarNoResults')}</span>
    {:else}
      {#each results as r, i (r.file + ':' + r.line + ':' + r.column + ':' + i)}
        <button class="hit" onclick={() => onOpenFile?.(r.file, r.line)}>
          <div class="hit-head">
            <span class="hit-name">{basename(r.file)}</span>
            <span class="hit-loc">{r.line}:{r.column}</span>
          </div>
          <div class="hit-line">{r.content}</div>
        </button>
      {/each}
    {/if}
  </div>
</div>

<style>
  .search { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .search-bar { display: flex; align-items: center; gap: 6px; padding: 8px; border-bottom: 1px solid var(--rg-border-bright); }
  :global(.search-bar .ico) { color: var(--rg-fg-muted); }
  .search-input { flex: 1; min-width: 0; padding: 8px 10px; border: 1px solid var(--rg-border-bright); border-radius: 8px; background: var(--rg-bg); color: var(--rg-fg); font-size: 14px; outline: none; }
  .search-input:focus { border-color: var(--rg-accent); }

  .search-body { flex: 1; min-height: 0; overflow-y: auto; padding: 4px; -webkit-overflow-scrolling: touch; display: flex; flex-direction: column; gap: 1px; }
  .msg { color: var(--rg-fg-muted); font-size: 12px; padding: 10px; }
  .msg.err { color: var(--rg-ansi-red); }
  .msg button { margin-left:8px;min-height:32px;padding:4px 10px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-surface);color:var(--rg-fg);cursor:pointer; }

  .hit { display: flex; flex-direction: column; gap: 2px; width: 100%; text-align: left; background: none; border: none; padding: 8px 10px; border-radius: 6px; cursor: pointer; }
  .hit:active { background: var(--rg-surface-2); }
  .hit-head { display: flex; align-items: baseline; gap: 8px; }
  .hit-name { font-size: 13px; color: var(--rg-fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hit-loc { font-size: 11px; color: var(--rg-fg-muted); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .hit-line { font-size: 12px; color: var(--rg-fg-muted); font-family: monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
