<script lang="ts">
  import { X, FileText, GitBranch, Copy } from 'lucide-svelte';
  import { t, tr } from '$lib/i18n';
  import type { SidebarProvider } from '../../shared/sidebar/types';

  let { provider, kind, path, line, onClose }: {
    provider: SidebarProvider;
    kind: 'file' | 'diff';
    path: string;
    line?: number;
    onClose: () => void;
  } = $props();

  // Cap how many lines we render — a viewer should never lock the mobile tab
  // turning a huge file/diff into tens of thousands of DOM rows.
  const MAX_LINES = 5000;

  let loading = $state(true);
  let error = $state<string | null>(null);
  let lines = $state<string[]>([]);
  let truncated = $state(false);
  let bodyEl = $state<HTMLDivElement | null>(null);

  function basename(p: string): string {
    const i = Math.max(p.lastIndexOf('/'), p.lastIndexOf('\\'));
    return i >= 0 ? p.slice(i + 1) : p;
  }

  /** Per-line class for the diff view (added / removed / hunk header / meta). */
  function diffClass(l: string): string {
    if (l.startsWith('+++') || l.startsWith('---')) return 'd-meta';
    if (l.startsWith('@@')) return 'd-hunk';
    if (l.startsWith('+')) return 'd-add';
    if (l.startsWith('-')) return 'd-del';
    if (l.startsWith('diff ') || l.startsWith('index ') || l.startsWith('new file') || l.startsWith('deleted')) return 'd-meta';
    return 'd-ctx';
  }

  async function copyPath() {
    try { await navigator.clipboard.writeText(path); } catch { /* clipboard blocked */ }
  }

  async function load() {
    loading = true;
    error = null;
    truncated = false;
    try {
      const text = kind === 'diff' ? await provider.gitDiff(path) : await provider.readFile(path);
      let arr = text.split('\n');
      // Drop a trailing empty line from the final newline so the gutter count is honest.
      if (arr.length > 1 && arr[arr.length - 1] === '') arr = arr.slice(0, -1);
      if (arr.length > MAX_LINES) { arr = arr.slice(0, MAX_LINES); truncated = true; }
      lines = arr;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // Reload whenever the target changes (kind/path captured by reference).
    void path; void kind;
    void load();
  });

  // Scroll a 'file' open to the search-hit line once content is painted.
  $effect(() => {
    if (kind !== 'file' || loading || error || !line || !bodyEl) return;
    const row = bodyEl.querySelector<HTMLElement>(`[data-ln="${line}"]`);
    if (row) row.scrollIntoView({ block: 'center' });
  });
</script>

<div class="viewer" role="dialog" aria-label={basename(path)}>
  <div class="v-header">
    <span class="v-ico">
      {#if kind === 'diff'}<GitBranch class="w-4 h-4" />{:else}<FileText class="w-4 h-4" />{/if}
    </span>
    <span class="v-title" title={path}>{basename(path)}</span>
    <button class="v-btn" onclick={copyPath} title={tr('mobile.viewerCopyPath')} tabindex="-1"><Copy class="w-4 h-4" /></button>
    <button class="v-btn" onclick={onClose} aria-label={tr('mobile.sidebarClose')} tabindex="-1"><X class="w-5 h-5" /></button>
  </div>

  <div class="v-body" bind:this={bodyEl} class:diff={kind === 'diff'}>
    {#if loading}
      <div class="v-msg">{$t('mobile.loading')}</div>
    {:else if error}
      <div class="v-msg err">{error}</div>
    {:else if lines.length === 0}
      <div class="v-msg">{kind === 'diff' ? $t('mobile.viewerNoChanges') : $t('mobile.viewerEmpty')}</div>
    {:else if kind === 'diff'}
      <pre class="diff-pre">{#each lines as l, i (i)}<span class="d-line {diffClass(l)}">{l || ' '}</span>{/each}</pre>
    {:else}
      <div class="code">
        {#each lines as l, i (i)}
          <div class="code-row" data-ln={i + 1} class:hit={line === i + 1}>
            <span class="ln">{i + 1}</span>
            <span class="lc">{l || ' '}</span>
          </div>
        {/each}
      </div>
    {/if}
    {#if truncated}
      <div class="v-trunc">{$t('mobile.viewerTruncated', { max: MAX_LINES })}</div>
    {/if}
  </div>
</div>

<style>
  .viewer { position: fixed; inset: 0; z-index: 55; display: flex; flex-direction: column; background: var(--rg-bg); animation: vfade .15s ease-out; }
  @keyframes vfade { from { opacity: 0 } to { opacity: 1 } }
  .v-header { display: flex; align-items: center; gap: 8px; padding: 8px 10px calc(8px); padding-top: calc(8px + env(safe-area-inset-top, 0px)); border-bottom: 1px solid var(--rg-border-bright); background: var(--rg-surface); min-height: 48px; }
  .v-ico { display: inline-flex; align-items: center; color: var(--rg-accent); flex-shrink: 0; }
  .v-title { flex: 1; min-width: 0; font-size: 13px; font-weight: 600; color: var(--rg-fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .v-btn { display: flex; align-items: center; justify-content: center; width: 36px; height: 36px; border: none; background: none; color: var(--rg-fg-muted); border-radius: 8px; cursor: pointer; flex-shrink: 0; }
  .v-btn:active { background: var(--rg-surface-2); color: var(--rg-fg); }

  .v-body { flex: 1; min-height: 0; overflow: auto; -webkit-overflow-scrolling: touch; padding: 6px 0 calc(6px + env(safe-area-inset-bottom, 0px)); }
  .v-msg { color: var(--rg-fg-muted); font-size: 13px; padding: 16px; }
  .v-msg.err { color: var(--rg-ansi-red); white-space: pre-wrap; word-break: break-word; }
  .v-trunc { color: var(--rg-fg-muted); font-size: 11px; padding: 10px 16px; border-top: 1px dashed var(--rg-border-bright); }

  /* File view: gutter + code, horizontally scrollable lines. */
  .code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.5; min-width: max-content; }
  .code-row { display: flex; align-items: flex-start; }
  .code-row.hit { background: color-mix(in srgb, var(--rg-accent) 16%, transparent); }
  .ln { flex-shrink: 0; width: 44px; padding: 0 8px; text-align: right; color: var(--rg-fg-muted); opacity: .6; user-select: none; position: sticky; left: 0; background: var(--rg-bg); }
  .lc { white-space: pre; color: var(--rg-fg); padding-right: 16px; }

  /* Diff view: prefix-coloured unified diff. */
  .diff-pre { margin: 0; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 12px; line-height: 1.5; min-width: max-content; }
  .d-line { display: block; white-space: pre; padding: 0 12px; }
  .d-add { color: var(--rg-ansi-green); background: color-mix(in srgb, var(--rg-ansi-green) 10%, transparent); }
  .d-del { color: var(--rg-ansi-red); background: color-mix(in srgb, var(--rg-ansi-red) 10%, transparent); }
  .d-hunk { color: var(--rg-accent); background: color-mix(in srgb, var(--rg-accent) 8%, transparent); }
  .d-meta { color: var(--rg-fg-muted); }
  .d-ctx { color: var(--rg-fg); }
</style>
