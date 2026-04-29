<script module lang="ts">
  import { writable } from 'svelte/store';

  /**
   * Module-level open state. Single instance lives at the app chrome level
   * (`+page.svelte`); any pane header opens it via `openScrollbackHistory(paneId)`
   * regardless of nesting depth in `SplitContainer`.
   *
   * Showing terminal history while xterm keeps streaming is a read-only
   * complement to the live pane — users get to review / copy / search
   * material that has scrolled past xterm's own 8000-line buffer.
   */
  interface OpenRequest {
    paneId: string;
  }

  const _pending = writable<OpenRequest | null>(null);
  export const scrollbackHistoryPending = { subscribe: _pending.subscribe };

  export function openScrollbackHistory(paneId: string): void {
    _pending.set({ paneId });
  }
</script>

<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { alertDialog } from './RidgeDialog.svelte';
  import { showToast } from '$lib/stores/toast';
  import {
    History,
    X,
    ArrowUpToLine,
    Loader2,
    Search,
    ChevronUp,
    ChevronDown,
    CaseSensitive,
    Download,
    Copy,
    Check,
  } from 'lucide-svelte';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { stripAnsi } from '$lib/utils/ansi';

  interface ScrollbackChunk {
    bytes: string;
    /** start_seq comes back as a number from the bridge (it fits in JS Number
     *  for any plausible session — capped at 4 MiB cap × ~thousand mounts). */
    start_seq: number;
    at_oldest: boolean;
  }

  /** Initial tail size to fetch on open. ~256 KiB ≈ a healthy `cat` of logs. */
  const TAIL_BYTES = 256 * 1024;
  /** Per-page pull when user clicks "load older". */
  const PAGE_BYTES = 128 * 1024;

  const req = $derived($scrollbackHistoryPending);

  let bytes = $state('');
  let startSeq = $state(0);
  let atOldest = $state(false);
  let loading = $state(false);
  let scroller: HTMLDivElement | undefined = $state();

  /**
   * On open: pull the tail. We watch `req` directly — when the user closes
   * and reopens for a different pane, this resets the buffer and pulls fresh.
   */
  let lastPaneId: string | null = null;
  $effect(() => {
    const r = req;
    if (!r) {
      lastPaneId = null;
      return;
    }
    if (r.paneId === lastPaneId) return;
    lastPaneId = r.paneId;
    void loadInitial(r.paneId);
  });

  async function loadInitial(paneId: string): Promise<void> {
    if (!isTauri()) {
      bytes = '(非 Tauri 环境，无法读取 scrollback)';
      atOldest = true;
      return;
    }
    loading = true;
    try {
      const chunk = await invoke<ScrollbackChunk>('get_pane_scrollback_tail', {
        paneId,
        maxBytes: TAIL_BYTES,
      });
      bytes = chunk.bytes;
      startSeq = chunk.start_seq;
      atOldest = chunk.at_oldest;
      // Scroll viewer to the bottom (newest content) on initial load — same
      // mental model as a regular tail, then user pages up.
      queueMicrotask(() => {
        if (scroller) scroller.scrollTop = scroller.scrollHeight;
      });
    } catch (err) {
      bytes = `(读取失败: ${err})`;
      atOldest = true;
    } finally {
      loading = false;
    }
  }

  /**
   * Auto-page guard. Prevents the scroll handler from firing loadOlder()
   * multiple times while one request is in-flight or just resolved (xterm /
   * overlayscrollbars sometimes emits 2-3 scroll events per frame near the
   * boundary). Cleared 200 ms after a successful loadOlder.
   */
  let recentlyAutoLoaded = false;

  function onScrollerScroll(): void {
    if (!scroller || atOldest || loading || recentlyAutoLoaded) return;
    // 32 px ≈ 2-3 lines at 12 px font, comfortably wider than the user's
    // single-frame scroll delta so we trigger before they hit the absolute top.
    if (scroller.scrollTop <= 32) {
      void loadOlder();
    }
  }

  async function loadOlder(): Promise<void> {
    const r = req;
    if (!r || atOldest || loading || !isTauri()) return;
    loading = true;
    recentlyAutoLoaded = true;
    setTimeout(() => {
      recentlyAutoLoaded = false;
    }, 200);
    try {
      const chunk = await invoke<ScrollbackChunk>('get_pane_scrollback_before', {
        paneId: r.paneId,
        beforeSeq: startSeq,
        maxBytes: PAGE_BYTES,
      });
      if (chunk.bytes.length === 0) {
        atOldest = true;
        return;
      }
      // Preserve viewport position relative to current top — record current
      // scrollHeight, prepend, then restore by adding the delta.
      const prevHeight = scroller?.scrollHeight ?? 0;
      const prevTop = scroller?.scrollTop ?? 0;
      bytes = chunk.bytes + bytes;
      startSeq = chunk.start_seq;
      atOldest = chunk.at_oldest;
      queueMicrotask(() => {
        if (!scroller) return;
        const delta = scroller.scrollHeight - prevHeight;
        scroller.scrollTop = prevTop + delta;
      });
    } catch (err) {
      console.warn('[scrollback-modal] loadOlder failed', err);
    } finally {
      loading = false;
    }
  }

  function dismiss(): void {
    if (loading) return;
    _pending.set(null);
  }

  /**
   * Trigger a download of the currently-loaded scrollback as a `.log` file.
   * We export the cleaned (ANSI-stripped) text — the same thing the user
   * can copy out of the viewer — so external tools (grep, less, editors)
   * see exactly what's on screen. Filename embeds an ISO-ish timestamp +
   * the pane id so multiple downloads are distinguishable.
   *
   * Uses `URL.createObjectURL` instead of a Tauri save-file dialog because
   * the Blob path round-trips through the browser-native download chrome
   * (works in dev mode too) without depending on `plugin-dialog::save`.
   */
  /**
   * Copy the cleaned scrollback to the clipboard. Mirror the download path
   * but routed through the browser clipboard so users can paste into chat /
   * issue trackers without writing a temp file. Shows a 1.5s checkmark.
   *
   * `navigator.clipboard.writeText` requires a secure context — Tauri's
   * webview qualifies, dev `localhost` does too, so no fallback path needed.
   */
  let copiedFlash = $state(false);
  async function copyAll(): Promise<void> {
    if (!cleaned) return;
    try {
      await navigator.clipboard.writeText(cleaned);
      copiedFlash = true;
      showToast('已复制到剪贴板');
      setTimeout(() => {
        copiedFlash = false;
      }, 1500);
    } catch (err) {
      console.warn('[scrollback-modal] clipboard write failed', err);
      await alertDialog({ title: '复制失败', message: `复制到剪贴板失败: ${err}`, danger: true });
    }
  }

  function downloadAsLog(): void {
    const r = req;
    if (!r || !cleaned) return;
    const stamp = new Date()
      .toISOString()
      .replace(/[:.]/g, '-')
      .slice(0, 19);
    const shortPane = r.paneId.slice(0, 8);
    const blob = new Blob([cleaned], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `ridge-scrollback-${shortPane}-${stamp}.log`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    // Free the blob; small delay so click() finishes before revoke.
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function onKeydown(e: KeyboardEvent): void {
    if (!req) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      dismiss();
    }
  }

  onDestroy(() => {
    _pending.set(null);
  });

  // ANSI/OSC stripping moved to `$lib/utils/ansi.ts` for reuse + unit tests.
  const cleaned = $derived(stripAnsi(bytes));

  // ─── In-modal search ────────────────────────────────────────────────────
  // Local-only — the global Ctrl+F still works as a browser fallback. This
  // bar gives match counts + n/N navigation that browser-search lacks.
  let searchText = $state('');
  let searchCaseSensitive = $state(false);
  /** Active match index (0-based). Highlighted in stronger orange. */
  let activeMatch = $state(0);
  let searchInput: HTMLInputElement | undefined = $state();

  /**
   * Compute byte-aligned match start indices into `cleaned`. We use plain
   * `indexOf` (no regex) since the modal is meant for grep-style scanning,
   * not pattern matching. Empty needle ⇒ no matches (avoid degenerate
   * "every gap is a hit" case).
   */
  function findMatches(haystack: string, needle: string, cs: boolean): number[] {
    if (!needle) return [];
    const positions: number[] = [];
    const h = cs ? haystack : haystack.toLowerCase();
    const n = cs ? needle : needle.toLowerCase();
    let i = 0;
    while (true) {
      const idx = h.indexOf(n, i);
      if (idx < 0) break;
      positions.push(idx);
      i = idx + Math.max(1, n.length);
    }
    return positions;
  }

  const matches = $derived(findMatches(cleaned, searchText, searchCaseSensitive));
  const matchCount = $derived(matches.length);

  // Reset cursor when query / content changes; clamp into range.
  $effect(() => {
    void searchText;
    void cleaned;
    if (matchCount === 0) activeMatch = 0;
    else if (activeMatch >= matchCount) activeMatch = matchCount - 1;
  });

  // Scroll the active match's `<mark>` into view after each change.
  $effect(() => {
    void activeMatch;
    void matches;
    if (matchCount === 0 || !scroller) return;
    queueMicrotask(() => {
      const el = scroller?.querySelector<HTMLElement>(
        `[data-match-idx="${activeMatch}"]`
      );
      if (el) el.scrollIntoView({ block: 'center', behavior: 'smooth' });
    });
  });

  /**
   * Slice `cleaned` into a typed list of plain/highlight runs. Active match
   * carries `active=true` so the template can pick the stronger highlight.
   */
  type Segment =
    | { kind: 'plain'; text: string }
    | { kind: 'match'; text: string; idx: number; active: boolean };
  const segments = $derived.by<Segment[]>(() => {
    if (matchCount === 0) return [{ kind: 'plain', text: cleaned }];
    const out: Segment[] = [];
    let cursor = 0;
    for (let i = 0; i < matches.length; i += 1) {
      const start = matches[i];
      const end = start + searchText.length;
      if (start > cursor) {
        out.push({ kind: 'plain', text: cleaned.slice(cursor, start) });
      }
      out.push({
        kind: 'match',
        text: cleaned.slice(start, end),
        idx: i,
        active: i === activeMatch,
      });
      cursor = end;
    }
    if (cursor < cleaned.length) {
      out.push({ kind: 'plain', text: cleaned.slice(cursor) });
    }
    return out;
  });

  function nextMatch(): void {
    if (matchCount === 0) return;
    activeMatch = (activeMatch + 1) % matchCount;
  }
  function prevMatch(): void {
    if (matchCount === 0) return;
    activeMatch = (activeMatch - 1 + matchCount) % matchCount;
  }

  function onSearchKeydown(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      // First Esc clears the search; second Esc closes modal. We stop
      // propagation only on the clear path so the global onKeydown above
      // (which dismisses on Esc) doesn't double-close.
      if (searchText) {
        searchText = '';
        e.stopPropagation();
      }
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (e.shiftKey) prevMatch();
      else nextMatch();
    }
  }

  // Auto-focus search input when modal first opens (after the data load).
  $effect(() => {
    if (req && !loading) {
      void (async () => {
        await tick();
        searchInput?.focus();
      })();
    }
  });
</script>

<svelte:window onkeydown={onKeydown} />

{#if req}
  <!-- Backdrop dismiss + ESC. Inner card stops propagation. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9996] flex items-center justify-center bg-black/55 backdrop-blur-[2px]"
    onclick={dismiss}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label="终端历史记录"
      tabindex="-1"
      class="w-[min(960px,92vw)] h-[min(720px,85vh)] flex flex-col bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-xl shadow-2xl overflow-hidden"
      onclick={(e) => e.stopPropagation()}
    >
      <header class="flex flex-col shrink-0 border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/60">
        <!-- Top row: title + page controls -->
        <div class="flex items-center gap-2 h-9 px-3">
          <span class="flex h-6 w-6 items-center justify-center rounded-md bg-[var(--rg-accent)]/15 text-[var(--rg-accent)]">
            <History class="h-3.5 w-3.5" />
          </span>
          <div class="flex-1 min-w-0">
            <div class="text-[12px] font-semibold text-[var(--rg-fg)]">终端历史记录</div>
            <div class="text-[10px] text-[var(--rg-fg-muted)] font-mono truncate">
              {atOldest ? '已到最早' : `seq ≥ ${startSeq.toLocaleString()}`} · {bytes.length.toLocaleString()} 字节
            </div>
          </div>
          <button
            type="button"
            class="flex items-center gap-1 h-7 px-2 rounded text-[11px] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] disabled:opacity-40 disabled:pointer-events-none"
            onclick={() => void loadOlder()}
            disabled={atOldest || loading}
            title={atOldest ? '已经是最早记录' : '向前加载更早的输出'}
          >
            {#if loading}
              <Loader2 class="h-3 w-3 animate-spin" />
            {:else}
              <ArrowUpToLine class="h-3 w-3" />
            {/if}
            加载更早
          </button>
          <button
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors disabled:opacity-30 disabled:pointer-events-none"
            title={copiedFlash ? '已复制到剪贴板' : '复制全部到剪贴板'}
            disabled={cleaned.length === 0}
            onclick={() => void copyAll()}
          >
            {#if copiedFlash}
              <Check class="h-3.5 w-3.5 text-emerald-400" />
            {:else}
              <Copy class="h-3.5 w-3.5" />
            {/if}
          </button>
          <button
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors disabled:opacity-30 disabled:pointer-events-none"
            title="另存为 .log 文件"
            disabled={cleaned.length === 0}
            onclick={downloadAsLog}
          >
            <Download class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
            title="关闭 (Esc)"
            onclick={dismiss}
          >
            <X class="h-3.5 w-3.5" />
          </button>
        </div>
        <!-- Bottom row: in-text search bar (Enter / Shift+Enter to navigate) -->
        <div class="flex items-center gap-1.5 h-8 px-3 border-t border-[var(--rg-border)]/40">
          <Search class="h-3 w-3 text-[var(--rg-fg-muted)] shrink-0" />
          <input
            type="text"
            bind:this={searchInput}
            bind:value={searchText}
            onkeydown={onSearchKeydown}
            placeholder="在历史记录中查找…"
            class="flex-1 min-w-0 px-2 py-0.5 text-[12px] bg-transparent border-0 focus:outline-none text-[var(--rg-fg)] placeholder:text-[var(--rg-fg-muted)]/70 font-mono"
          />
          <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded border text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors
              {searchCaseSensitive
              ? 'border-[var(--rg-accent)]/60 bg-[var(--rg-accent)]/15 !text-[var(--rg-accent)]'
              : 'border-[var(--rg-border)] hover:bg-[var(--rg-surface)]'}"
            title="区分大小写"
            aria-pressed={searchCaseSensitive}
            onclick={() => (searchCaseSensitive = !searchCaseSensitive)}
          >
            <CaseSensitive class="h-3 w-3" />
          </button>
          <span class="text-[10px] font-mono text-[var(--rg-fg-muted)] tabular-nums w-16 text-right">
            {#if searchText.length === 0}
              &nbsp;
            {:else if matchCount === 0}
              0
            {:else}
              {activeMatch + 1} / {matchCount}
            {/if}
          </span>
          <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] disabled:opacity-30 disabled:pointer-events-none"
            disabled={matchCount === 0}
            title="上一处 (Shift+Enter)"
            onclick={prevMatch}
          >
            <ChevronUp class="h-3 w-3" />
          </button>
          <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] disabled:opacity-30 disabled:pointer-events-none"
            disabled={matchCount === 0}
            title="下一处 (Enter)"
            onclick={nextMatch}
          >
            <ChevronDown class="h-3 w-3" />
          </button>
        </div>
      </header>

      <!-- Read-only history viewer. ANSI is stripped because we want
           selection / copy / browser-Ctrl+F to work cleanly. The live xterm
           pane in the background still shows colour.
           Segmented render: each search hit becomes a `<mark>` so we can
           scrollIntoView the active one without re-laying-out the entire
           buffer on every Enter. -->
      <div
        bind:this={scroller}
        class="flex-1 min-h-0 bg-[var(--rg-bg)]"
        use:overlayScroll
        onscroll={onScrollerScroll}
      >
        <pre
          class="m-0 p-3 text-[12px] leading-[1.5] font-mono text-[var(--rg-fg)] whitespace-pre-wrap break-words selection:bg-[var(--rg-accent)]/30"
        >{#each segments as seg, i (i)}{#if seg.kind === 'plain'}{seg.text}{:else}<mark
              data-match-idx={seg.idx}
              class="rounded-sm px-0 py-0 transition-colors {seg.active
                ? 'bg-amber-400 text-black'
                : 'bg-amber-500/30 text-[var(--rg-fg)]'}"
            >{seg.text}</mark>{/if}{/each}</pre>
      </div>

      <footer class="flex items-center gap-2 h-7 px-3 border-t border-[var(--rg-border)] bg-[var(--rg-surface)]/60 shrink-0 text-[10px] text-[var(--rg-fg-muted)]">
        <kbd class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">Esc</kbd>
        关闭
        <span class="select-none opacity-50">·</span>
        <kbd class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">Ctrl</kbd>+<kbd
          class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">F</kbd
        >
        浏览器内查找
        <span class="ml-auto select-none opacity-60">
          ANSI 控制字符已剥离用于复制；live 终端保持原样
        </span>
      </footer>
    </div>
  </div>
{/if}
