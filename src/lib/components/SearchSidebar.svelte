<script lang="ts">
  // src/lib/components/SearchSidebar.svelte
  //
  // Global file search / replace UI for the Ridge sidebar. Mirrors VS Code's
  // "Search" view: query box, optional replace box, include/exclude globs,
  // case/regex/word toggles, results grouped by file with line previews.
  //
  // Scope selection: we search inside every **active workspace CWD** the user
  // currently has open (one explorer column per pane's cwd, round 1 design).
  // That matches "global file search" semantics for the *current session*
  // without surprising users by crawling their whole disk.
  //
  // Backend: `text_search` (per-root) + `replace_in_files`. We fan out
  // `text_search` across each root and merge the results. `replace_in_files`
  // is invoked on demand for the files the user keeps ticked.

  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import {
    Search,
    Replace,
    Regex,
    CaseSensitive,
    WholeWord,
    ChevronRight,
    ChevronDown,
    X,
    RefreshCw,
    Loader2,
  } from 'lucide-svelte';
  import { paneCwdStore } from '$lib/stores/paneTree';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { alertDialog, confirmDialog } from './RidgeDialog.svelte';
  import { searchFolderStore, clearSearchFolder } from '$lib/stores/searchState';
  import { settingsStore } from '$lib/stores/settings';
  import { get } from 'svelte/store';

  interface SearchResult {
    file: string;
    line: number;
    column: number;
    content: string;
    match_text?: string;
  }

  interface InvalidGlob {
    pattern: string;
    error: string;
    /** "include" or "exclude" — which input to decorate. */
    field: string;
  }

  interface ReplaceStats {
    files_processed: number;
    files_modified: number;
    replacements: number;
    errors: string[];
  }

  // ─── Query state ─────────────────────────────────────────────────────────
  let query = $state('');
  let replaceText = $state('');
  let showReplace = $state(false);
  let caseSensitive = $state(false);
  let useRegex = $state(false);
  let wholeWord = $state(false);
  /**
   * Comma-separated glob patterns. Include = only match files whose relative
   * path matches ≥ 1 pattern; exclude = drop files matching any pattern.
   * Filtering runs client-side after the search returns — the backend doesn't
   * take a pattern list yet. When both inputs are empty we skip the filter
   * entirely so perf is unchanged from the previous implementation.
   */
  // 默认 globs 来自全局 settings —— 用户在设置中心配置后，第一次打开搜索框
  // 就预填上次保存的值。仅在初始化时取一次快照，避免用户在搜索框临时改值
  // 时被 settings 写入覆盖。
  let includeGlobs = $state(get(settingsStore).searchIncludeGlobs);
  let excludeGlobs = $state(get(settingsStore).searchExcludeGlobs);

  // T3：文件树右键 → "在此文件夹中搜索"。`searchFolderStore` 一旦置为非空路径，
  // 就把 `includeGlobs` 显式写为该文件夹的 glob —— 让用户在搜索框里直接看见
  // 当前正在限定的范围，并可以手动调整 / 删除。仅在 store 从空 → 非空时
  // 写入，避免用户编辑过 includeGlobs 后被反复覆盖。
  let lastSeenFolder: string | null = null;
  $effect(() => {
    const folder = $searchFolderStore;
    if (folder === lastSeenFolder) return;
    lastSeenFolder = folder;
    if (folder) {
      const norm = folder.replace(/\\/g, '/').replace(/\/+$/, '');
      includeGlobs = `${norm}/**`;
    }
  });

  // ─── Results state ───────────────────────────────────────────────────────
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let lastRunQuery = $state('');
  let replacing = $state(false);
  /**
   * Bad glob patterns reported by the backend's `text_search_diagnostics`
   * command. Surfaced as a red ring on the offending input + a tooltip
   * listing the parse error — matches VS Code's "files.exclude" decoration
   * style. Empty list = both inputs accepted (or no input at all).
   */
  let invalidGlobs = $state<InvalidGlob[]>([]);
  const includeGlobErrors = $derived(invalidGlobs.filter((g) => g.field === 'include'));
  const excludeGlobErrors = $derived(invalidGlobs.filter((g) => g.field === 'exclude'));
  // Monotonic counter: ensures a stale diagnostics .then() from a previous
  // search cannot overwrite the results of the current one.
  let _diagGen = 0;
  let collapsedFiles = $state(new Set<string>());
  /**
   * Files checked for replace. On a fresh search every unique file starts
   * selected; user toggles off whatever they want to skip.
   */
  let selectedFiles = $state(new Set<string>());

  /** Distinct workspace-backing CWDs — restricted to folderRoot when set. */
  const roots = $derived.by(() => {
    const folder = $searchFolderStore;
    if (folder) return [folder];
    const set = new Set<string>();
    for (const cwd of Object.values($paneCwdStore)) {
      if (cwd) set.add(cwd);
    }
    return Array.from(set).sort();
  });

  // ─── Auto-search debounce ────────────────────────────────────────────────
  // Mirrors VS Code's Search view: typing pauses for ~400ms trigger a fresh
  // run automatically. Any immediate trigger (Enter, toggle change) cancels
  // the pending timer so we never run twice for the same input.
  const AUTO_SEARCH_MS = 400;
  let autoTimer: ReturnType<typeof setTimeout> | null = null;

  function clearAutoTimer(): void {
    if (autoTimer !== null) {
      clearTimeout(autoTimer);
      autoTimer = null;
    }
  }

  function scheduleAutoSearch(): void {
    clearAutoTimer();
    // Empty → clear results immediately; no point in a deferred run.
    if (!query.trim()) {
      void runSearch();
      return;
    }
    autoTimer = setTimeout(() => {
      autoTimer = null;
      void runSearch();
    }, AUTO_SEARCH_MS);
  }

  // Re-run when query / toggles / globs change. $effect tracks each dep.
  // `runSearch` itself is a no-op on empty query so typing 3 chars quickly
  // doesn't spam backend calls — only the trailing timer fires.
  $effect(() => {
    void query;
    void caseSensitive;
    void useRegex;
    void wholeWord;
    void includeGlobs;
    void excludeGlobs;
    scheduleAutoSearch();
  });

  /**
   * Parse a comma/newline-separated list of glob patterns and compile each
   * into a RegExp. Empty / whitespace-only patterns are skipped. Translation:
   *   `**` → `.*` (crosses directory boundaries)
   *   `*`  → `[^/\\]*` (stops at path separator)
   *   `?`  → `[^/\\]`
   * Other regex metachars are escaped. Matches against the full absolute path.
   */
  function compileGlobList(raw: string): RegExp[] {
    const parts = raw
      .split(/[,\n]/)
      .map((p) => p.trim())
      .filter(Boolean);
    const patterns: RegExp[] = [];
    for (const p of parts) {
      // Normalise slashes so users can write `src/**/*.ts` on Windows.
      const escaped = p
        .replace(/[.+^${}()|[\]]/g, '\\$&')
        .replace(/\\\*\\\*/g, '__DOUBLESTAR__')
        .replace(/\\\*/g, '[^/\\\\]*')
        .replace(/__DOUBLESTAR__/g, '.*')
        .replace(/\\\?/g, '[^/\\\\]');
      try {
        patterns.push(new RegExp(escaped, 'i'));
      } catch {
        /* malformed pattern — silently drop */
      }
    }
    return patterns;
  }

  /**
   * Apply include/exclude filters to a result list. Include (any match) →
   * keep; exclude (any match) → drop. Include empty → allow all, exclude
   * empty → deny nothing.
   */
  function applyGlobFilters(rows: SearchResult[]): SearchResult[] {
    const includes = compileGlobList(includeGlobs);
    const excludes = compileGlobList(excludeGlobs);
    if (includes.length === 0 && excludes.length === 0) return rows;
    return rows.filter((r) => {
      // Normalise to forward-slash so the same pattern works on Windows.
      const normalised = r.file.replace(/\\/g, '/');
      if (includes.length > 0 && !includes.some((re) => re.test(normalised))) {
        return false;
      }
      if (excludes.some((re) => re.test(normalised))) return false;
      return true;
    });
  }

  /** Results grouped by file for the accordion view. */
  const groupedResults = $derived.by(() => {
    const map = new Map<string, SearchResult[]>();
    for (const r of results) {
      const list = map.get(r.file) ?? [];
      list.push(r);
      map.set(r.file, list);
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
  });

  // ─── Display limit ───────────────────────────────────────────────────────
  const DISPLAY_LIMIT = 100;
  let showAll = $state(false);

  // Reset showAll whenever results change (new search).
  $effect(() => {
    void results;
    showAll = false;
  });

  /** Truncated groups structure: at most DISPLAY_LIMIT match rows total. */
  const visibleGroups = $derived((() => {
    const allGroups = groupedResults.map(([file, rows]) => ({ file, rows }));
    if (showAll) return allGroups;
    let remaining = DISPLAY_LIMIT;
    const out: { file: string; rows: SearchResult[] }[] = [];
    for (const g of allGroups) {
      if (remaining <= 0) break;
      const rows = g.rows.slice(0, remaining);
      out.push({ ...g, rows });
      remaining -= rows.length;
    }
    return out;
  })());

  /**
   * Run `text_search` across every root and merge results. If the query is
   * empty we just clear the list.
   */
  async function runSearch(): Promise<void> {
    const trimmed = query.trim();
    if (!trimmed) {
      results = [];
      lastRunQuery = '';
      // Clear stale glob errors too — otherwise the red ring lingers
      // after the user empties the query and re-typing a valid pattern
      // would still look broken until the next non-empty search.
      invalidGlobs = [];
      return;
    }
    if (!isTauri()) return;
    searching = true;
    lastRunQuery = trimmed;
    try {
      // Backend (round 23) honours include / exclude globs at walk time so
      // we don't pay for IO on filtered files. Client-side `applyGlobFilters`
      // below stays as a defence-in-depth pass — it also catches any glob
      // forms our local compiler accepts but the rust `glob` crate rejects.
      const includeGlobsList = includeGlobs
        .split(/[,\n]/)
        .map((s) => s.trim())
        .filter(Boolean);
      const excludeGlobsList = excludeGlobs
        .split(/[,\n]/)
        .map((s) => s.trim())
        .filter(Boolean);
      // Diagnostics runs in parallel with the per-root search.  The backend
      // command only parses the glob strings (no IO) so it resolves in
      // microseconds.  Surface the red-ring decoration immediately when it
      // resolves instead of waiting for the (potentially slow) search loop to
      // finish — especially important on large monorepos with many roots.
      const diagGen = ++_diagGen;
      invoke<InvalidGlob[]>('text_search_diagnostics', {
        includeGlobs: includeGlobsList,
        excludeGlobs: excludeGlobsList,
      })
        .then((globs) => { if (_diagGen === diagGen) invalidGlobs = globs; })
        .catch(() => {}); // non-fatal — user just loses the red-ring hint

      // Fire all per-root searches concurrently. Serial iteration was the old
      // behaviour — with N workspace roots each search blocked the next, so
      // total latency scaled linearly. Promise.allSettled lets the Tauri IPC
      // layer (and the Rust backend) process roots in parallel; individual
      // root failures are non-fatal (empty contribution, warning logged).
      const parts = await Promise.allSettled(
        roots.map((root) =>
          invoke<SearchResult[]>('text_search', {
            root,
            query: trimmed,
            caseSensitive,
            useRegex,
            wholeWord,
            maxResults: 500,
            includeGlobs: includeGlobsList,
            excludeGlobs: excludeGlobsList,
          })
        )
      );
      const all: SearchResult[] = [];
      parts.forEach((p, i) => {
        if (p.status === 'fulfilled') all.push(...p.value);
        else console.warn('[search] root failed', roots[i], p.reason);
      });
      // De-dup identical file+line+column results (two panes in the same cwd
      // can each reply; the tree is still the same one on disk).
      const seen = new Set<string>();
      const unique = all.filter((r) => {
        const k = `${r.file}:${r.line}:${r.column}`;
        if (seen.has(k)) return false;
        seen.add(k);
        return true;
      });
      // Apply client-side include/exclude glob filter (empty lists no-op).
      results = applyGlobFilters(unique);
      selectedFiles = new Set(results.map((r) => r.file));
    } finally {
      searching = false;
    }
  }

  /** Commit the replace across `selectedFiles` using whichever root owns each file. */
  async function runReplace(): Promise<void> {
    if (!showReplace || replacing || results.length === 0) return;
    if (!isTauri()) return;
    const fileList = Array.from(selectedFiles);
    if (fileList.length === 0) return;
    const confirmed = await confirmDialog({
      title: '确认替换',
      message: `将在 ${fileList.length} 个文件中把 "${query}" 替换为 "${replaceText}"，继续？`,
      okLabel: '替换',
    });
    if (!confirmed) return;
    replacing = true;
    try {
      // Bucket files by the root they sit under (longest-prefix match), then
      // invoke replace_in_files once per root.
      const buckets = new Map<string, string[]>();
      for (const file of fileList) {
        let bestRoot = roots[0] ?? '';
        let bestLen = 0;
        for (const r of roots) {
          if (file.startsWith(r) && r.length > bestLen) {
            bestRoot = r;
            bestLen = r.length;
          }
        }
        const arr = buckets.get(bestRoot) ?? [];
        arr.push(file);
        buckets.set(bestRoot, arr);
      }
      let totalReplacements = 0;
      let totalFiles = 0;
      const errors: string[] = [];
      for (const [root, files] of buckets) {
        try {
          const stats = await invoke<ReplaceStats>('replace_in_files', {
            root,
            search: query,
            replace: replaceText,
            files,
            caseSensitive,
            useRegex,
          });
          totalReplacements += stats.replacements;
          totalFiles += stats.files_modified;
          errors.push(...stats.errors);
        } catch (err) {
          errors.push(`${root}: ${err}`);
        }
      }
      await alertDialog({
        title: '替换完成',
        message: `完成：${totalFiles} 个文件，${totalReplacements} 处替换${
          errors.length ? `\n\n错误:\n${errors.join('\n')}` : ''
        }`,
      });
      // Re-run search so stale matches disappear.
      await runSearch();
    } finally {
      replacing = false;
    }
  }

  function toggleFileCollapsed(file: string): void {
    const next = new Set(collapsedFiles);
    if (next.has(file)) next.delete(file);
    else next.add(file);
    collapsedFiles = next;
  }

  function toggleFileSelected(file: string): void {
    const next = new Set(selectedFiles);
    if (next.has(file)) next.delete(file);
    else next.add(file);
    selectedFiles = next;
  }

  function basename(p: string): string {
    return p.split(/[/\\]/).filter(Boolean).pop() || p;
  }

  function dirname(p: string): string {
    const parts = p.split(/[/\\]/).filter(Boolean);
    if (parts.length <= 1) return '';
    return parts.slice(0, -1).join('/');
  }

  /**
   * Render a single result line with the matched span highlighted.
   *
   * The backend reports `r.column` (1-based) and `r.match_text` for the exact
   * hit location. Using those directly avoids the "multiple matches per line
   * highlight the first one" bug that a plain `indexOf` causes — important
   * when a single line contains the query more than once (common in e.g.
   * tag-attribute match lines).
   *
   * Falls back to an indexOf slice when the backend didn't supply a
   * `match_text` (older server builds) or the column is out of range.
   */
  function splitHighlightAt(
    line: string,
    match: string,
    column: number
  ): { before: string; hit: string; after: string } {
    // Convert to 0-based; clamp inside line bounds.
    const start = Math.max(0, Math.min(line.length, column - 1));
    if (match && line.slice(start, start + match.length) === match) {
      return {
        before: line.slice(0, start),
        hit: match,
        after: line.slice(start + match.length),
      };
    }
    // Case-insensitive verify if exact slice didn't match (file on disk had
    // casing that query didn't).
    if (
      match &&
      line.slice(start, start + match.length).toLowerCase() === match.toLowerCase()
    ) {
      return {
        before: line.slice(0, start),
        hit: line.slice(start, start + match.length),
        after: line.slice(start + match.length),
      };
    }
    // Fall back: indexOf of match (respecting case-sensitive flag).
    const needle = match || query;
    if (!needle) return { before: line, hit: '', after: '' };
    const idx = caseSensitive
      ? line.indexOf(needle)
      : line.toLowerCase().indexOf(needle.toLowerCase());
    if (idx < 0) return { before: line, hit: '', after: '' };
    return {
      before: line.slice(0, idx),
      hit: line.slice(idx, idx + needle.length),
      after: line.slice(idx + needle.length),
    };
  }

  function openAt(file: string, line: number, column: number): void {
    void fileEditorStore.openFile(file, { line, column });
  }

  // Enter in the query input kicks off a search; Ctrl+R toggles replace row.
  function onQueryKeydown(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      // Cancel the pending auto-search so we don't run it twice.
      clearAutoTimer();
      void runSearch();
    }
  }

  onMount(() => {
    // Nothing to do on mount — searches are user-driven.
  });
</script>

<div class="flex h-full flex-col text-[var(--rg-fg)]">
  <!-- Header bar: Search label + refresh last query -->
  <div
    data-tauri-drag-region
    class="px-3 h-11 shrink-0 flex items-center justify-between border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/40"
  >
    <span class="text-[11px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] flex items-center gap-1.5">
      搜索
      {#if $searchFolderStore}
        <span class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] bg-[var(--rg-accent)]/15 text-[var(--rg-accent)] border border-[var(--rg-accent)]/30 max-w-[140px]">
          <span class="truncate" title={$searchFolderStore}>{$searchFolderStore.replace(/\\/g, '/').split('/').pop()}</span>
          <button type="button" onclick={clearSearchFolder} class="shrink-0 hover:text-[var(--rg-fg)] transition-colors" title="清除文件夹限制">
            <X class="h-2.5 w-2.5" />
          </button>
        </span>
      {/if}
    </span>
    <div class="flex items-center gap-1">
      {#if !$searchFolderStore}
        <span class="text-[10px] text-[var(--rg-fg-muted)]" title="当前会话的工作目录数量">
          {roots.length} 根
        </span>
      {/if}
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] disabled:opacity-40"
        title="重新运行上一次搜索"
        disabled={!lastRunQuery || searching}
        onclick={() => void runSearch()}
      >
        <RefreshCw class="h-3 w-3 {searching ? 'animate-spin' : ''}" />
      </button>
    </div>
  </div>

  <!-- Query + replace inputs + toggles -->
  <div class="p-2 shrink-0 flex flex-col gap-1.5 border-b border-[var(--rg-border)]/60">
    <div class="relative flex items-center">
      <!-- Replace-row toggle, sits flush to the left (VS Code chevron pattern). -->
      <button
        type="button"
        class="flex h-6 w-5 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]"
        title={showReplace ? '收起替换' : '展开替换'}
        onclick={() => (showReplace = !showReplace)}
      >
        {#if showReplace}
          <ChevronDown class="h-3 w-3" />
        {:else}
          <ChevronRight class="h-3 w-3" />
        {/if}
      </button>
      <input
        type="text"
        bind:value={query}
        onkeydown={onQueryKeydown}
        placeholder="搜索…"
        class="flex-1 min-w-0 px-2 py-1 text-[12px] rounded bg-[var(--rg-bg)] border border-[var(--rg-border)] focus:outline-none focus:border-[var(--rg-accent)]/60 placeholder:text-[var(--rg-fg-muted)]/70"
      />
    </div>
    {#if showReplace}
      <div class="flex items-center gap-0 pl-5">
        <input
          type="text"
          bind:value={replaceText}
          placeholder="替换为…"
          class="flex-1 min-w-0 px-2 py-1 text-[12px] rounded bg-[var(--rg-bg)] border border-[var(--rg-border)] focus:outline-none focus:border-[var(--rg-accent)]/60 placeholder:text-[var(--rg-fg-muted)]/70"
        />
        <button
          type="button"
          class="ml-1 flex items-center gap-1 h-6 px-2 rounded text-[11px] bg-[var(--rg-accent)]/15 text-[var(--rg-accent)] border border-[var(--rg-accent)]/30 hover:bg-[var(--rg-accent)]/25 disabled:opacity-40"
          disabled={replacing || results.length === 0 || !query.trim()}
          onclick={() => void runReplace()}
          title="全部替换"
        >
          {#if replacing}
            <Loader2 class="h-3 w-3 animate-spin" />
          {:else}
            <Replace class="h-3 w-3" />
          {/if}
          全部替换
        </button>
      </div>
    {/if}

    <!-- Include / exclude glob filters (client-side). Both optional.
         Bad globs (reported by `text_search_diagnostics`) decorate the
         offending input with a red ring + tooltip — VS Code parity. -->
    <div class="flex items-center gap-1 pl-5">
      <input
        type="text"
        bind:value={includeGlobs}
        placeholder="包含：*.ts, src/**"
        class="flex-1 min-w-0 px-2 py-1 text-[11px] rounded bg-[var(--rg-bg)] border focus:outline-none placeholder:text-[var(--rg-fg-muted)]/70 font-mono
          {includeGlobErrors.length > 0
            ? 'border-rose-500/60 focus:border-rose-500/80 ring-1 ring-rose-500/30'
            : 'border-[var(--rg-border)] focus:border-[var(--rg-accent)]/60'}"
        title={includeGlobErrors.length > 0
          ? `非法 glob：\n${includeGlobErrors.map((g) => `  ${g.pattern} — ${g.error}`).join('\n')}`
          : '只在匹配这些 glob 的文件里搜索。逗号分隔；* 不跨路径分隔符，** 跨任意层级。'}
      />
    </div>
    <div class="flex items-center gap-1 pl-5">
      <input
        type="text"
        bind:value={excludeGlobs}
        placeholder="排除：**/dist/**, **/*.lock"
        class="flex-1 min-w-0 px-2 py-1 text-[11px] rounded bg-[var(--rg-bg)] border focus:outline-none placeholder:text-[var(--rg-fg-muted)]/70 font-mono
          {excludeGlobErrors.length > 0
            ? 'border-rose-500/60 focus:border-rose-500/80 ring-1 ring-rose-500/30'
            : 'border-[var(--rg-border)] focus:border-[var(--rg-accent)]/60'}"
        title={excludeGlobErrors.length > 0
          ? `非法 glob：\n${excludeGlobErrors.map((g) => `  ${g.pattern} — ${g.error}`).join('\n')}`
          : '匹配这些 glob 的文件不进入结果。'}
      />
    </div>

    <!-- Toggle pill row: case-sensitive / whole-word / regex -->
    <div class="flex items-center gap-1 pl-5">
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded border text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors
          {caseSensitive
          ? 'border-[var(--rg-accent)]/60 bg-[var(--rg-accent)]/15 !text-[var(--rg-accent)]'
          : 'border-[var(--rg-border)] hover:bg-[var(--rg-surface)]'}"
        title="区分大小写 (Aa)"
        aria-pressed={caseSensitive}
        onclick={() => (caseSensitive = !caseSensitive)}
      >
        <CaseSensitive class="h-3 w-3" />
      </button>
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded border text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors
          {wholeWord
          ? 'border-[var(--rg-accent)]/60 bg-[var(--rg-accent)]/15 !text-[var(--rg-accent)]'
          : 'border-[var(--rg-border)] hover:bg-[var(--rg-surface)]'}"
        title="匹配完整单词 (\\b)"
        aria-pressed={wholeWord}
        onclick={() => (wholeWord = !wholeWord)}
      >
        <WholeWord class="h-3 w-3" />
      </button>
      <button
        type="button"
        class="flex h-6 w-6 items-center justify-center rounded border text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors
          {useRegex
          ? 'border-[var(--rg-accent)]/60 bg-[var(--rg-accent)]/15 !text-[var(--rg-accent)]'
          : 'border-[var(--rg-border)] hover:bg-[var(--rg-surface)]'}"
        title="正则表达式 (.*)"
        aria-pressed={useRegex}
        onclick={() => (useRegex = !useRegex)}
      >
        <Regex class="h-3 w-3" />
      </button>
      <span class="ml-auto text-[10px] text-[var(--rg-fg-muted)]">
        {#if searching}搜索中…
        {:else if lastRunQuery && results.length === 0}无结果
        {:else if results.length > 0}{results.length} 处 / {groupedResults.length} 文件
        {:else}&nbsp;
        {/if}
      </span>
    </div>
  </div>

  <!-- Results list — grouped by file, click line to open. -->
  <div class="flex-1 min-h-0" use:overlayScroll>
    {#if roots.length === 0}
      <div class="p-4 text-[11px] text-[var(--rg-fg-muted)] text-center">
        当前会话无任何工作目录，打开一个终端或 .ridge 工作区后再试。
      </div>
    {:else if results.length === 0 && lastRunQuery}
      <div class="p-4 text-[11px] text-[var(--rg-fg-muted)] text-center">
        未匹配 "{lastRunQuery}"
      </div>
    {:else if results.length === 0}
      <div class="p-4 text-[11px] text-[var(--rg-fg-muted)] text-center">
        输入关键字后回车执行搜索。
      </div>
    {:else}
      {#each visibleGroups as group (group.file)}
        <div class="group search-file">
          <div class="sticky top-0 z-10 flex items-center gap-1 h-7 px-2 text-[11px] bg-[var(--rg-surface-2)]/92 backdrop-blur-md">
            <!-- Replace-inclusion checkbox — only meaningful when replace UI is visible. -->
            {#if showReplace}
              <input
                type="checkbox"
                class="h-3 w-3 accent-[var(--rg-accent)]"
                checked={selectedFiles.has(group.file)}
                onchange={() => toggleFileSelected(group.file)}
                title="是否包含在替换里"
              />
            {/if}
            <button
              type="button"
              class="flex items-center gap-1 flex-1 min-w-0 text-left"
              onclick={() => toggleFileCollapsed(group.file)}
            >
              {#if collapsedFiles.has(group.file)}
                <ChevronRight class="h-3 w-3 shrink-0" />
              {:else}
                <ChevronDown class="h-3 w-3 shrink-0" />
              {/if}
              <span class="truncate font-medium text-[var(--rg-fg)]">{basename(group.file)}</span>
              {#if dirname(group.file)}
                <span class="truncate text-[10px] text-[var(--rg-fg-muted)]">{dirname(group.file)}</span>
              {/if}
            </button>
            <span class="shrink-0 text-[10px] text-[var(--rg-fg-muted)]">{group.rows.length}</span>
          </div>
          {#if !collapsedFiles.has(group.file)}
            {#each group.rows as r (r.line + ':' + r.column)}
              {@const parts = splitHighlightAt(r.content, r.match_text ?? query, r.column)}
              <button
                type="button"
                class="rg-search-row group/row flex w-full items-start gap-2 pl-7 pr-3 py-1 text-left text-[11px] hover:bg-[var(--rg-surface)]/50 transition-colors"
                onclick={() => openAt(r.file, r.line, r.column)}
                title={`${group.file}:${r.line}:${r.column}`}
              >
                <span class="shrink-0 font-mono text-[10px] text-[var(--rg-fg-muted)] w-8 text-right">
                  {r.line}
                </span>
                <span class="truncate font-mono text-[var(--rg-fg)]">
                  <span>{parts.before}</span>
                  <span class="bg-[var(--rg-accent)]/30 text-[var(--rg-fg)] rounded px-0.5">
                    {parts.hit || query}
                  </span>
                  <span>{parts.after}</span>
                </span>
              </button>
            {/each}
          {/if}
        </div>
      {/each}
      {#if !showAll && results.length > DISPLAY_LIMIT}
        <button
          type="button"
          class="w-full px-4 py-2 text-[11px] text-[var(--rg-accent)] hover:bg-[var(--rg-surface)] transition-colors text-center"
          onclick={() => { showAll = true; }}
        >
          显示全部 {results.length} 条结果（当前显示 {DISPLAY_LIMIT}）
        </button>
      {/if}
    {/if}
  </div>
</div>
