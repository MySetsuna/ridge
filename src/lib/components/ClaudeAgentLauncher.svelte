<script module lang="ts">
  import { writable, get } from 'svelte/store';

  /**
   * Module-level state for the "Run Claude Code here" prompt modal. A single
   * instance lives at the app root (`+page.svelte`); any Bot button across the
   * pane tree calls `openClaudeAgentLauncher(paneId)` to raise the modal,
   * regardless of how deeply nested the pane is in SplitContainer recursion.
   *
   * The modal coordinates the lifecycle that was previously inlined in
   * SplitContainer.svelte:
   *   1. Let the user type a prompt (or skip straight to `claude` CLI)
   *   2. Register a teammate agent against the pane so the AGENT indicator
   *      appears (round 9 feature)
   *   3. Send-keys into the PTY — escaping the prompt as a double-quoted arg
   */
  interface LauncherRequest {
    paneId: string;
    /**
     * When true, caller wants to skip the prompt UI and just send `claude\r`.
     * Used for Shift/Alt-click on the Bot button.
     */
    skipPrompt: boolean;
  }

  const pending = writable<LauncherRequest | null>(null);

  export function openClaudeAgentLauncher(paneId: string, skipPrompt = false): void {
    pending.set({ paneId, skipPrompt });
  }

  export const claudeAgentLauncherPending = {
    subscribe: pending.subscribe,
  };
</script>

<script lang="ts">
  import { tick, onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { Bot, X } from 'lucide-svelte';
  import { alertDialog } from './RidgeDialog.svelte';
  import { paneTreeStore, paneForegroundProcessStore } from '$lib/stores/paneTree';
  import type { PaneNode } from '$lib/types';

  /** Textarea focus target — focus on modal open. */
  let textarea: HTMLTextAreaElement | undefined = $state();
  /** Typed prompt text. Empty → launch bare `claude` without arg. */
  let promptText = $state('');
  /** Prevents double-submit while the Tauri invoke chain is in flight. */
  let inFlight = $state(false);

  // ─── Recent prompts history ──────────────────────────────────────────────
  // Last N successful submissions persisted to localStorage. Re-using a prompt
  // from history promotes it to the head, so frequently-used prompts bubble up.
  // De-dupe on exact string so resubmit doesn't bloat the list.
  const RECENT_KEY = 'ridge-claude-recent-prompts';
  const MAX_RECENT = 5;

  function loadRecentPrompts(): string[] {
    if (typeof localStorage === 'undefined') return [];
    try {
      const raw = localStorage.getItem(RECENT_KEY);
      if (!raw) return [];
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed)) return [];
      return parsed.filter((v): v is string => typeof v === 'string').slice(0, MAX_RECENT);
    } catch {
      return [];
    }
  }

  function persistRecentPrompts(list: string[]): void {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(list.slice(0, MAX_RECENT)));
    } catch {
      /* quota / privacy errors: drop silently — recent list is best-effort */
    }
  }

  let recentPrompts = $state<string[]>(loadRecentPrompts());

  function pushRecentPrompt(text: string): void {
    const trimmed = text.trim();
    if (!trimmed) return;
    const next = [trimmed, ...recentPrompts.filter((p) => p !== trimmed)].slice(0, MAX_RECENT);
    recentPrompts = next;
    persistRecentPrompts(next);
  }

  /** A compact, single-line preview of a longer prompt for chip display. */
  function previewOf(text: string, max = 48): string {
    const oneLine = text.replace(/\s+/g, ' ').trim();
    return oneLine.length > max ? `${oneLine.slice(0, max - 1)}…` : oneLine;
  }

  // ─── Auto-release watchdog ────────────────────────────────────────────────
  // When `claude` exits inside a pane, the backend's `teammate_pane_states`
  // stays Busy because nobody explicitly calls release. We close that loop by
  // watching the foreground-process store (already polled by Pane.svelte every
  // 1.5s): for each pane marked busy in the layout, track how long its
  // foreground has been a shell/non-claude process. After a grace period we
  // invoke `release_teammate_agent` so the AGENT indicator clears.
  //
  // Grace logic:
  //   - On registration (new busy pane detected), initialise sinceNonClaude[pane] = null
  //   - Each tick, if foreground process name still looks claude-ish → reset to null
  //   - Otherwise set to Date.now() when first seen; if older than NON_CLAUDE_GRACE_MS, release
  //
  // The 4s grace covers: (a) terminal echoing the `claude\r` typing while the
  // shell is still foreground, (b) the claude process's own post-exit shell
  // redraw — users never see a false release.
  const NON_CLAUDE_GRACE_MS = 4000;
  const CLAUDE_PROCESS_RE = /(^|[\\/])(claude|claude-code|claude\.exe)$/i;

  /** Walk the layout tree and return `{ paneId, agentState }` for every leaf. */
  function flattenLeaves(node: PaneNode): Array<{ id: string; state?: string }> {
    if (node.type === 'leaf') {
      return [{ id: node.id, state: node.agent_state }];
    }
    return node.children.flatMap(flattenLeaves);
  }

  const sinceNonClaude = new Map<string, number | null>();
  let watchdogTimer: ReturnType<typeof setInterval> | null = null;

  async function tickAutoRelease(): Promise<void> {
    if (!isTauri()) return;
    const layout = $paneTreeStore;
    const fg = $paneForegroundProcessStore;
    const leaves = flattenLeaves(layout);
    const seen = new Set<string>();
    for (const leaf of leaves) {
      seen.add(leaf.id);
      if (leaf.state !== 'busy') {
        // Not busy → drop tracker entry so re-registration starts fresh.
        sinceNonClaude.delete(leaf.id);
        continue;
      }
      const proc = fg[leaf.id] ?? '';
      const stillClaude = proc && CLAUDE_PROCESS_RE.test(proc);
      if (stillClaude) {
        sinceNonClaude.set(leaf.id, null);
        continue;
      }
      // Not (or no longer) claude. Start / continue the countdown.
      const firstSeen = sinceNonClaude.get(leaf.id);
      if (firstSeen == null) {
        sinceNonClaude.set(leaf.id, Date.now());
        continue;
      }
      if (Date.now() - firstSeen >= NON_CLAUDE_GRACE_MS) {
        // Claude has been gone long enough — release. Removing from the map
        // before the invoke prevents a retrigger race if this call is slow.
        sinceNonClaude.delete(leaf.id);
        try {
          await invoke('release_teammate_agent', { paneId: leaf.id });
        } catch (err) {
          console.warn('[agent-launcher] auto-release failed', leaf.id, err);
        }
      }
    }
    // Garbage-collect entries for panes that no longer exist.
    for (const id of Array.from(sinceNonClaude.keys())) {
      if (!seen.has(id)) sinceNonClaude.delete(id);
    }
  }

  onMount(() => {
    // 1s cadence; foreground polling is every 1.5s so ~1 extra frame latency
    // to detect the transition is fine. Grace period 4s dominates either way.
    watchdogTimer = setInterval(() => void tickAutoRelease(), 1000);
  });
  onDestroy(() => {
    if (watchdogTimer !== null) clearInterval(watchdogTimer);
  });

  const req = $derived($claudeAgentLauncherPending);

  // Open / close side-effects: clear prompt when modal dismisses, focus
  // textarea when it opens, and kick off the skip-prompt path immediately
  // when the caller asked for it.
  $effect(() => {
    if (!req) {
      promptText = '';
      return;
    }
    if (req.skipPrompt) {
      void submit();
      return;
    }
    // Defer focus so the textarea is in the DOM.
    void (async () => {
      await tick();
      textarea?.focus();
    })();
  });

  function dismiss(): void {
    if (inFlight) return;
    pending.set(null);
  }

  /**
   * Shell-escape a prompt string for inclusion inside double-quoted
   * `claude "<prompt>"`. We escape backslash and double-quote only — sufficient
   * for bash / zsh / PowerShell's common double-quote semantics. The user's
   * shell is whatever's running in the pane; if they've picked something exotic
   * they can always Shift-click to skip the modal and type the prompt manually.
   */
  function shellQuote(s: string): string {
    const escaped = s.replace(/\\/g, '\\\\').replace(/"/g, '\\"');
    return `"${escaped}"`;
  }

  async function submit(): Promise<void> {
    if (inFlight) return;
    const request = get(claudeAgentLauncherPending);
    if (!request) return;
    if (!isTauri()) {
      pending.set(null);
      return;
    }
    inFlight = true;
    try {
      const trimmed = promptText.trim();
      // agent_id: short, visible in the pane title (see round 9 indicator).
      const agentId = `agent-${Date.now().toString(36)}-${Math.random()
        .toString(36)
        .slice(2, 6)}`;
      await invoke('register_teammate_agent', {
        paneId: request.paneId,
        agentId,
      });
      const command = trimmed && !request.skipPrompt ? `claude ${shellQuote(trimmed)}\r` : 'claude\r';
      await invoke('write_to_pty', { paneId: request.paneId, data: command });
      // On a successful submission with non-empty text, promote to the recent
      // history. skip-prompt path (Shift-click) also contributes nothing here
      // because `trimmed` is always '' in that branch.
      if (trimmed) pushRecentPrompt(trimmed);
      // Always record the run (even empty REPL launches) in the per-pane
      // Claude history plugin so users can see "this pane hosted an agent
      // at time X". Fires lazy-imported to avoid a hard plugin dep from
      // the launcher — the plugin registers itself, the launcher just feeds it.
      void (async () => {
        const { pushHistoryEntry } = await import('$lib/plugins/claudeHistory/store');
        pushHistoryEntry(request.paneId, {
          prompt: trimmed,
          at: Date.now(),
          agentId,
        });
      })();
      pending.set(null);
    } catch (err) {
      console.error('[agent-launcher] submit', err);
      await alertDialog({ title: '启动失败', message: `启动 Claude Code 失败: ${err}`, danger: true });
    } finally {
      inFlight = false;
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      dismiss();
      return;
    }
    // Chat-app idiom (ChatGPT / Slack / WeChat 输入框)：
    //   Enter          → 提交
    //   Ctrl/Cmd+Enter → 在光标处换行
    //   Shift+Enter    → 浏览器原生换行（保留为兼容快捷键）
    // Ctrl+C / V / Z / Y 等编辑快捷键交给浏览器默认处理，不在此拦截。
    if (e.key === 'Enter') {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        const ta = e.currentTarget as HTMLTextAreaElement;
        const start = ta.selectionStart;
        const end = ta.selectionEnd;
        // setRangeText preserves the native undo stack so Ctrl+Z still
        // walks back through the inserted newline; reassigning value would
        // wipe it.
        ta.setRangeText('\n', start, end, 'end');
        // bind:value reads through the input event — synthesize one so
        // promptText stays in sync with the DOM.
        ta.dispatchEvent(new Event('input', { bubbles: true }));
        return;
      }
      if (!e.shiftKey) {
        e.preventDefault();
        void submit();
      }
    }
  }
</script>

{#if req && !req.skipPrompt}
  <!-- Dismiss-on-backdrop: click outside the inner card closes the modal.
       Inner card stops propagation so clicks on textarea don't close it. -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9997] flex items-center justify-center bg-black/55 backdrop-blur-[2px]"
    onclick={dismiss}
    onkeydown={(e) => e.key === 'Escape' && dismiss()}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label="启动 Claude Code agent"
      tabindex="-1"
      class="w-[min(560px,92vw)] flex flex-col gap-3 p-4 bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-xl shadow-2xl"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          dismiss();
          e.stopPropagation();
        }
      }}
    >
      <header class="flex items-center gap-2">
        <span class="flex h-7 w-7 items-center justify-center rounded-lg bg-emerald-500/15 text-emerald-300">
          <Bot class="h-4 w-4" />
        </span>
        <div class="flex-1 min-w-0">
          <div class="text-[13px] font-semibold text-[var(--rg-fg)]">
            在此窗格启动 Claude Code
          </div>
          <div class="text-[11px] text-[var(--rg-fg-muted)]">
            可留空直接进入交互式 REPL，或填入任务描述直接带进第一轮。
          </div>
        </div>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-white/[0.06]"
          title="取消 (Esc)"
          onclick={dismiss}
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </header>

      <textarea
        bind:this={textarea}
        bind:value={promptText}
        onkeydown={onKeydown}
        rows="4"
        placeholder="例：帮我把 src/lib/components/Explorer.svelte 的 ArrowUp 改成跨列跳转（留空则直接进入 REPL）"
        class="w-full resize-y rounded-lg bg-[var(--rg-bg)] border border-[var(--rg-border)] px-3 py-2 text-[13px] text-[var(--rg-fg)] placeholder:text-[var(--rg-fg-muted)]/70 focus:outline-none focus:border-[var(--rg-accent)]/60"
      ></textarea>

      {#if recentPrompts.length > 0}
        <!-- Last MAX_RECENT successful prompts. Click = fill textarea + focus;
             users can then tweak & submit. Not stored per-workspace — the
             list is user-global, consistent with Warp's "recent commands". -->
        <div class="flex flex-wrap gap-1.5">
          <span class="text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)] self-center mr-1">
            最近
          </span>
          {#each recentPrompts as p (p)}
            <button
              type="button"
              class="max-w-[260px] truncate px-2 py-1 rounded-md text-[11px] bg-white/[0.04] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:bg-[var(--rg-accent)]/10 hover:text-[var(--rg-fg)] hover:border-[var(--rg-accent)]/40 transition-colors"
              title={p}
              onclick={async () => {
                promptText = p;
                await tick();
                textarea?.focus();
                // Place cursor at end so users can keep typing to tweak.
                textarea?.setSelectionRange(p.length, p.length);
              }}
            >
              {previewOf(p)}
            </button>
          {/each}
        </div>
      {/if}

      <div class="flex items-center gap-2 text-[11px] text-[var(--rg-fg-muted)]">
        <kbd class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">Esc</kbd>
        取消
        <span class="select-none">·</span>
        <kbd class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">Enter</kbd>
        提交
        <span class="select-none">·</span>
        <kbd class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">Ctrl</kbd>+<kbd
          class="px-1.5 py-0.5 rounded bg-white/[0.06] border border-[var(--rg-border)] font-mono">Enter</kbd
        >
        换行
        <span class="flex-1"></span>
        <button
          type="button"
          class="px-3 py-1.5 rounded-lg text-[12px] text-[var(--rg-fg)] hover:bg-white/[0.06]"
          onclick={dismiss}
          disabled={inFlight}
        >
          取消
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[12px] bg-emerald-500/15 border border-emerald-400/40 text-emerald-300 hover:bg-emerald-500/25 disabled:opacity-40 disabled:pointer-events-none"
          onclick={() => void submit()}
          disabled={inFlight || !isTauri()}
        >
          <Bot class="h-3.5 w-3.5" />
          {promptText.trim() ? '发送并启动' : '启动 REPL'}
        </button>
      </div>
    </div>
  </div>
{/if}
