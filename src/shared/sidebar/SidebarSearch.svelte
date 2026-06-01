<script lang="ts">
  import { Search } from 'lucide-svelte';
  import type { SidebarProvider, SearchHit } from './types';

  let { provider, onOpenFile }: {
    provider: SidebarProvider;
    onOpenFile?: (path: string, line?: number) => void;
  } = $props();

  let query = $state('');
  let results = $state<SearchHit[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let debounce: ReturnType<typeof setTimeout> | undefined;

  function basename(p: string): string {
    const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return i >= 0 ? p.slice(i + 1) : p;
  }

  async function run(q: string) {
    if (q.trim().length < 2) { results = []; return; }
    loading = true;
    error = null;
    try {
      results = await provider.search(q);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function onInput(e: Event) {
    query = (e.target as HTMLInputElement).value;
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(() => run(query), 250);
  }
</script>

<div class="search">
  <div class="search-bar">
    <Search class="w-4 h-4 shrink-0 ico" />
    <input
      class="search-input"
      type="search"
      placeholder="搜索文件内容…"
      value={query}
      oninput={onInput}
    />
  </div>

  <div class="search-body">
    {#if error}
      <span class="msg err">{error}</span>
    {:else if loading}
      <span class="msg">搜索中…</span>
    {:else if query.trim().length >= 2 && results.length === 0}
      <span class="msg">无匹配结果</span>
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

  .hit { display: flex; flex-direction: column; gap: 2px; width: 100%; text-align: left; background: none; border: none; padding: 8px 10px; border-radius: 6px; cursor: pointer; }
  .hit:active { background: var(--rg-surface-2); }
  .hit-head { display: flex; align-items: baseline; gap: 8px; }
  .hit-name { font-size: 13px; color: var(--rg-fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hit-loc { font-size: 11px; color: var(--rg-fg-muted); flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .hit-line { font-size: 12px; color: var(--rg-fg-muted); font-family: monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
