<script lang="ts">
  import { X, FileText, GitBranch, Copy, Pencil, Eye, Save } from 'lucide-svelte';
  import { t, tr } from '$lib/i18n';
  import type { SidebarProvider } from '../../shared/sidebar/types';

  let { provider, kind, path, line, onClose }: {
    provider: SidebarProvider;
    kind: 'file' | 'diff';
    path: string;
    line?: number;
    onClose: () => void;
  } = $props();

  // Cap how many lines we render in the read view — a viewer should never lock
  // the mobile tab turning a huge file/diff into tens of thousands of DOM rows.
  const MAX_LINES = 5000;

  let loading = $state(true);
  let error = $state<string | null>(null);
  let lines = $state<string[]>([]);        // read-view rows (file or diff)
  let content = $state('');                // full text, edited in place (file only)
  let truncated = $state(false);
  let bodyEl = $state<HTMLDivElement | null>(null);

  // Edit state (file kind only).
  let editing = $state(false);
  let dirty = $state(false);
  let saving = $state(false);
  let saveError = $state<string | null>(null);

  const canEdit = $derived(kind === 'file' && !loading && !error);

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

  function splitLines(text: string): { rows: string[]; truncated: boolean } {
    let arr = text.split('\n');
    if (arr.length > 1 && arr[arr.length - 1] === '') arr = arr.slice(0, -1);
    if (arr.length > MAX_LINES) return { rows: arr.slice(0, MAX_LINES), truncated: true };
    return { rows: arr, truncated: false };
  }

  async function copyPath() {
    try { await navigator.clipboard.writeText(path); } catch { /* clipboard blocked */ }
  }

  async function load() {
    loading = true;
    error = null;
    truncated = false;
    editing = false;
    dirty = false;
    saveError = null;
    try {
      const text = kind === 'diff' ? await provider.gitDiff(path) : await provider.readFile(path);
      content = text;
      const { rows, truncated: tr2 } = splitLines(text);
      lines = rows;
      truncated = tr2;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function save() {
    if (saving || !dirty) return;
    saving = true;
    saveError = null;
    try {
      await provider.writeFile(path, content);
      dirty = false;
      // Refresh the read view from the saved content.
      const { rows, truncated: tr2 } = splitLines(content);
      lines = rows;
      truncated = tr2;
    } catch (e) {
      saveError = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  function onEditInput(e: Event) {
    content = (e.target as HTMLTextAreaElement).value;
    dirty = true;
  }

  // Leave edit mode → rebuild the read view from the CURRENT content so it
  // reflects unsaved edits (otherwise the view showed the pre-edit text).
  function exitEdit() {
    const { rows, truncated: t2 } = splitLines(content);
    lines = rows;
    truncated = t2;
    editing = false;
  }

  function requestClose() {
    if (dirty && !confirm(tr('mobile.viewerUnsavedConfirm'))) return;
    onClose();
  }

  $effect(() => {
    // Reload whenever the target file/kind changes.
    void path; void kind;
    void load();
  });

  // Scroll a 'file' read view open to the search-hit line once content is painted.
  $effect(() => {
    if (kind !== 'file' || editing || loading || error || !line || !bodyEl) return;
    const row = bodyEl.querySelector<HTMLElement>(`[data-ln="${line}"]`);
    if (row) row.scrollIntoView({ block: 'center' });
  });
</script>

<div class="viewer" role="dialog" aria-label={basename(path)}>
  <div class="v-header">
    <span class="v-ico">
      {#if kind === 'diff'}<GitBranch class="w-4 h-4" />{:else}<FileText class="w-4 h-4" />{/if}
    </span>
    <span class="v-title" title={path}>{basename(path)}{#if dirty}<span class="dirty-dot" title={tr('mobile.viewerUnsaved')}>●</span>{/if}</span>
    {#if canEdit}
      {#if editing}
        <button class="v-btn" class:armed={dirty} onclick={save} disabled={!dirty || saving} title={tr('mobile.viewerSave')} tabindex="-1"><Save class="w-4 h-4" /></button>
        <button class="v-btn" onclick={exitEdit} title={tr('mobile.viewerView')} tabindex="-1"><Eye class="w-4 h-4" /></button>
      {:else}
        <button class="v-btn" onclick={() => editing = true} title={tr('mobile.viewerEdit')} tabindex="-1"><Pencil class="w-4 h-4" /></button>
      {/if}
    {/if}
    <button class="v-btn" onclick={copyPath} title={tr('mobile.viewerCopyPath')} tabindex="-1"><Copy class="w-4 h-4" /></button>
    <button class="v-btn" onclick={requestClose} aria-label={tr('mobile.sidebarClose')} tabindex="-1"><X class="w-5 h-5" /></button>
  </div>

  {#if saveError}<div class="v-saveerr">{saveError}</div>{/if}

  {#if editing}
    <textarea
      class="editor"
      value={content}
      oninput={onEditInput}
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
    ></textarea>
  {:else}
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
  {/if}
</div>

<style>
  .viewer { position: fixed; inset: 0; z-index: 55; display: flex; flex-direction: column; background: var(--rg-bg); animation: vfade .15s ease-out; }
  @keyframes vfade { from { opacity: 0 } to { opacity: 1 } }
  .v-header { display: flex; align-items: center; gap: 6px; padding: 8px 10px; padding-top: calc(8px + env(safe-area-inset-top, 0px)); border-bottom: 1px solid var(--rg-border-bright); background: var(--rg-surface); min-height: 48px; }
  .v-ico { display: inline-flex; align-items: center; color: var(--rg-accent); flex-shrink: 0; }
  .v-title { flex: 1; min-width: 0; font-size: 12px; font-weight: 600; color: var(--rg-fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dirty-dot { color: var(--rg-accent); margin-left: 6px; font-size: 10px; }
  .v-btn { display: flex; align-items: center; justify-content: center; width: 34px; height: 34px; border: none; background: none; color: var(--rg-fg-muted); border-radius: 8px; cursor: pointer; flex-shrink: 0; }
  .v-btn:active { background: var(--rg-surface-2); color: var(--rg-fg); }
  .v-btn:disabled { opacity: .35; }
  .v-btn.armed { color: var(--rg-accent); }

  .v-saveerr { padding: 6px 12px; font-size: 11px; color: var(--rg-ansi-red); background: color-mix(in srgb, var(--rg-ansi-red) 10%, transparent); white-space: pre-wrap; word-break: break-word; }

  .v-body { flex: 1; min-height: 0; overflow: auto; -webkit-overflow-scrolling: touch; padding: 6px 0 calc(6px + env(safe-area-inset-bottom, 0px)); }
  .v-msg { color: var(--rg-fg-muted); font-size: 12px; padding: 16px; }
  .v-msg.err { color: var(--rg-ansi-red); white-space: pre-wrap; word-break: break-word; }
  .v-trunc { color: var(--rg-fg-muted); font-size: 10px; padding: 10px 14px; border-top: 1px dashed var(--rg-border-bright); }

  /* File read view: gutter + code, horizontally scrollable lines. Compact font. */
  .code { font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 10.5px; line-height: 1.45; min-width: max-content; }
  .code-row { display: flex; align-items: flex-start; }
  .code-row.hit { background: color-mix(in srgb, var(--rg-accent) 16%, transparent); }
  .ln { flex-shrink: 0; width: 38px; padding: 0 8px; text-align: right; color: var(--rg-fg-muted); opacity: .55; user-select: none; position: sticky; left: 0; background: var(--rg-bg); }
  .lc { white-space: pre; color: var(--rg-fg); padding-right: 16px; }

  /* Editable textarea: monospace, compact, soft-wrapped for phone editing. */
  .editor { flex: 1; min-height: 0; width: 100%; box-sizing: border-box; resize: none; border: none; outline: none; background: var(--rg-bg); color: var(--rg-fg); font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 11px; line-height: 1.5; padding: 8px 12px calc(8px + env(safe-area-inset-bottom, 0px)); -webkit-overflow-scrolling: touch; tab-size: 2; }

  /* Diff view: prefix-coloured unified diff. Compact font. */
  .diff-pre { margin: 0; font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; font-size: 10.5px; line-height: 1.45; min-width: max-content; }
  .d-line { display: block; white-space: pre; padding: 0 12px; }
  .d-add { color: var(--rg-ansi-green); background: color-mix(in srgb, var(--rg-ansi-green) 10%, transparent); }
  .d-del { color: var(--rg-ansi-red); background: color-mix(in srgb, var(--rg-ansi-red) 10%, transparent); }
  .d-hunk { color: var(--rg-accent); background: color-mix(in srgb, var(--rg-accent) 8%, transparent); }
  .d-meta { color: var(--rg-fg-muted); }
  .d-ctx { color: var(--rg-fg); }
</style>
