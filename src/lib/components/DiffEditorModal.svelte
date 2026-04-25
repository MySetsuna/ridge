<script lang="ts" module>
  // src/lib/components/DiffEditorModal.svelte
  //
  // Monaco-backed side-by-side diff modal — replaces the legacy `<pre>` text
  // diff in SourceControl.svelte. Single instance per session, opened via
  // the `openDiffEditor()` module function so any component can request a
  // diff without owning the modal state.
  //
  // Why module-scope state instead of plain props: the modal is global
  // (z-index registry slot 9998), but its callers (SCM file rows) are deep
  // inside the SourceControl tree. Passing props down would force every
  // ancestor to know about diff state. Module-level matches the same
  // pattern used by ScrollbackHistoryModal / ClaudeAgentLauncher.
  //
  // Behaviour:
  //   - Calls backend `git_get_file_versions` for original/modified blobs.
  //   - Hands them to Monaco DiffEditor (`renderSideBySide` user-toggleable;
  //     defaults to side-by-side at ≥720px viewport, inline below).
  //   - Read-only on both sides (writing-back to git via the diff editor is
  //     a separate UX; out of scope for this round).
  //   - Esc / backdrop click → close. ResizeObserver triggers a layout()
  //     so the editor stays correctly sized when window resizes.
  import { writable } from 'svelte/store';

  export interface OpenDiffArgs {
    repoRoot: string;
    /** Repo-relative path. Used both as title and as the git pathspec. */
    path: string;
    /**
     * `true` → staged view (HEAD vs index). `false` → unstaged view (index
     * vs working tree). Mirrors the boolean SCM passes to git_diff_file
     * historically.
     */
    cached: boolean;
  }

  interface DiffModalState {
    open: boolean;
    args: OpenDiffArgs | null;
  }

  // NB: cannot name this `state` — collides with Svelte 5's `$state` rune
  // when read inside the instance script (compiler resolves $state via
  // identifier lookup and trips on the typed writable).
  const diffModal = writable<DiffModalState>({ open: false, args: null });

  /**
   * Open the diff modal for `args`. If a diff is already open it's
   * replaced (one diff at a time matches every comparable VS Code flow —
   * Quick Diff, SCM file click, etc.).
   */
  export function openDiffEditor(args: OpenDiffArgs): void {
    diffModal.set({ open: true, args });
  }

  export function closeDiffEditor(): void {
    diffModal.set({ open: false, args: null });
  }
</script>

<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import * as monaco from 'monaco-editor';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { X, FileText, Columns, AlignLeft, RotateCw } from 'lucide-svelte';
  import { langFromPath } from '$lib/stores/fileEditor';

  interface FileVersions {
    original: string;
    modified: string;
  }

  let host: HTMLDivElement | undefined = $state();
  let editor: monaco.editor.IStandaloneDiffEditor | null = null;
  let originalModel: monaco.editor.ITextModel | null = null;
  let modifiedModel: monaco.editor.ITextModel | null = null;

  let loading = $state(false);
  let error = $state<string>('');
  /**
   * Default to side-by-side on wide windows; flip to inline at narrow
   * widths so the modal stays usable in a 720-wide split. The user can
   * toggle either way via the header buttons.
   */
  let renderSideBySide = $state(
    typeof window !== 'undefined' ? window.innerWidth >= 900 : true
  );

  const open = $derived($diffModal.open);
  const args = $derived($diffModal.args);
  const titleSuffix = $derived(args?.cached ? '已暂存差异' : '工作区差异');

  /** Dispose Monaco diff resources on close — heavy objects, must release.
   *  Idempotent: $effect cleanup AND onDestroy both call this; the
   *  early-return makes the second call obvious instead of relying on
   *  Monaco's internal idempotency. */
  function disposeEditor(): void {
    if (!editor && !originalModel && !modifiedModel) return;
    editor?.dispose();
    editor = null;
    originalModel?.dispose();
    modifiedModel?.dispose();
    originalModel = null;
    modifiedModel = null;
  }

  /** Load both blobs + (re)build the Monaco diff editor inside `host`.
   *  `await tick()` before `createDiffEditor` makes sure the modal's
   *  flex layout has run at least one frame so the host has a non-zero
   *  measured size — round-26 review caught this as a timing-window
   *  risk that could cause Monaco to size itself to 0. */
  async function reload(): Promise<void> {
    if (!args || !host || !isTauri()) return;
    loading = true;
    error = '';
    try {
      const v = await invoke<FileVersions>('git_get_file_versions', {
        repoRoot: args.repoRoot,
        path: args.path,
        cached: args.cached,
      });
      const lang = langFromPath(args.path);
      // Dispose any previous instance before rebuilding.
      disposeEditor();
      await tick();
      originalModel = monaco.editor.createModel(v.original, lang);
      modifiedModel = monaco.editor.createModel(v.modified, lang);
      editor = monaco.editor.createDiffEditor(host, {
        theme: 'vs-dark',
        automaticLayout: true,
        readOnly: true,
        // NB: not passing renderSideBySide here — the dedicated $effect
        // below applies it via updateOptions, avoiding a full reload on
        // user toggle (round-26 review LOW finding).
        renderOverviewRuler: false,
        minimap: { enabled: false },
        fontFamily:
          '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace',
        fontSize: 12.5,
        // Render whitespace differences cleanly — git diff strips trailing
        // whitespace by default but rendering it helps debug "phantom"
        // diffs caused by EOL or BOM mismatches.
        renderWhitespace: 'boundary',
        scrollBeyondLastLine: false,
      });
      editor.updateOptions({ renderSideBySide });
      editor.setModel({ original: originalModel, modified: modifiedModel });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      disposeEditor();
    } finally {
      loading = false;
    }
  }

  // (Re)build only when args / open / host change — we explicitly do NOT
  // read renderSideBySide here, so toggling the side-by-side button
  // doesn't trigger a full backend roundtrip. The dedicated
  // `updateOptions` effect below handles the cheap layout swap.
  $effect(() => {
    if (open && args && host) {
      void reload();
    }
    if (!open) {
      disposeEditor();
    }
  });

  // Re-apply renderSideBySide via updateOptions without a full rebuild —
  // cheaper than reload + keeps state alignment.
  $effect(() => {
    if (editor) {
      editor.updateOptions({ renderSideBySide });
    }
  });

  onDestroy(disposeEditor);

  function onBackdropClick(): void {
    closeDiffEditor();
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeDiffEditor();
    }
  }

  async function refresh(): Promise<void> {
    await tick();
    await reload();
  }
</script>

{#if open && args}
  <!-- z-index 9998 matches the modal registry; sits above launcher (9997)
       and history (9996), below ContextMenu (9999). -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9998] bg-black/65 flex items-center justify-center"
    onclick={onBackdropClick}
    onkeydown={onKeydown}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label="文件差异"
      tabindex="-1"
      class="w-[min(1200px,94vw)] h-[min(820px,86vh)] flex flex-col bg-[var(--wf-bg)] border border-[var(--wf-border)] rounded-lg shadow-2xl overflow-hidden"
      onclick={(e) => e.stopPropagation()}
      onkeydown={onKeydown}
    >
      <!-- Header: title + render-mode toggle + refresh + close -->
      <div class="flex items-center gap-2 h-10 px-3 border-b border-[var(--wf-border)] bg-[var(--wf-surface)]/60 shrink-0">
        <FileText class="h-3.5 w-3.5 text-[var(--wf-accent)] shrink-0" />
        <span class="text-[12px] font-mono truncate flex-1" title={args.path}>
          {args.path}
          <span class="text-[10px] text-[var(--wf-fg-muted)] ml-1">· {titleSuffix}</span>
        </span>
        <div class="flex items-center gap-0.5 mr-1 border border-[var(--wf-border)] rounded">
          <button
            type="button"
            class="flex h-6 w-7 items-center justify-center text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-colors {renderSideBySide ? 'bg-[var(--wf-accent)]/20 text-[var(--wf-accent)]' : ''}"
            title="并排（side-by-side）"
            onclick={() => (renderSideBySide = true)}
          >
            <Columns class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="flex h-6 w-7 items-center justify-center text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-colors {!renderSideBySide ? 'bg-[var(--wf-accent)]/20 text-[var(--wf-accent)]' : ''}"
            title="内联（inline）"
            onclick={() => (renderSideBySide = false)}
          >
            <AlignLeft class="h-3.5 w-3.5" />
          </button>
        </div>
        <button
          type="button"
          class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
          title="重新加载"
          onclick={() => void refresh()}
        >
          <RotateCw class="h-3.5 w-3.5 {loading ? 'animate-spin' : ''}" />
        </button>
        <button
          type="button"
          class="flex h-6 w-6 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
          title="关闭 (Esc)"
          onclick={closeDiffEditor}
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>
      <!-- Body: Monaco mount point -->
      <div class="flex-1 min-h-0 relative">
        {#if error}
          <div class="absolute inset-0 p-4 flex items-center justify-center">
            <div class="max-w-[420px] text-center text-[12px] text-rose-300 bg-rose-500/10 border border-rose-500/30 rounded p-3 font-mono whitespace-pre-wrap">
              {error}
            </div>
          </div>
        {/if}
        <!-- Hide host while error overlay is shown — a refresh-after-error
             would otherwise mount a new editor underneath the panel and
             leave both visible until the next layout pass. -->
        <div bind:this={host} class="absolute inset-0" style={error ? 'visibility:hidden' : ''}></div>
        {#if loading && !error}
          <div class="absolute top-2 right-3 text-[10px] text-[var(--wf-fg-muted)] bg-[var(--wf-surface)]/80 px-2 py-0.5 rounded">
            加载中…
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
