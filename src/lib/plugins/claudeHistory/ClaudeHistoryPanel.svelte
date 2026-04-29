<script lang="ts">
  import { ChevronDown, ChevronRight, Bot, Trash2 } from 'lucide-svelte';
  import {
    claudeHistoryStore,
    clearHistoryForPane,
    getHistoryForPane,
  } from './store';
  import { openClaudeAgentLauncher } from '$lib/components/ClaudeAgentLauncher.svelte';

  interface Props {
    /** Supplied by `SidebarPluginRegion` for scope='pane'. */
    paneId?: string;
    cwd?: string;
  }

  let { paneId }: Props = $props();
  let collapsed = $state(true);

  // Lazy-prime history for this pane on first render so `$claudeHistoryStore`
  // emits something useful; subsequent reads hit the cached array.
  $effect(() => {
    if (!paneId) return;
    getHistoryForPane(paneId);
  });

  const entries = $derived(paneId ? $claudeHistoryStore[paneId] ?? [] : []);
  const count = $derived(entries.length);

  function timestamp(at: number): string {
    const d = new Date(at);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  }

  function preview(text: string, max = 56): string {
    const oneLine = text.replace(/\s+/g, ' ').trim();
    if (!oneLine) return '(空 prompt · REPL)';
    return oneLine.length > max ? `${oneLine.slice(0, max - 1)}…` : oneLine;
  }
</script>

{#if paneId}
  <div class="border-t border-[var(--rg-border)]/40 bg-[var(--rg-surface-2)]/30">
    <button
      type="button"
      class="w-full flex items-center gap-1 h-6 px-3 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)]/50 transition-colors"
      onclick={() => (collapsed = !collapsed)}
    >
      {#if collapsed}
        <ChevronRight class="h-3 w-3" />
      {:else}
        <ChevronDown class="h-3 w-3" />
      {/if}
      <Bot class="h-3 w-3 text-emerald-400" />
      <span class="flex-1 text-left">Claude 历史</span>
      {#if count > 0}
        <span class="text-[var(--rg-fg)] font-mono">{count}</span>
      {/if}
    </button>
    {#if !collapsed}
      {#if entries.length === 0}
        <div class="px-5 py-2 text-[11px] text-[var(--rg-fg-muted)]">
          尚无记录。在此窗格启动过的 Claude Code prompt 会出现在这里。
        </div>
      {:else}
        {#each entries.slice().reverse() as e (e.at + ':' + e.agentId)}
          <button
            type="button"
            class="group w-full flex items-start gap-2 pl-5 pr-3 py-1 text-left text-[11px] hover:bg-[var(--rg-surface)]/50 transition-colors"
            title={e.prompt || '(REPL 直接启动，无 prompt)'}
            onclick={() => {
              if (!paneId) return;
              // Click → reopen the launcher with this pane preselected.
              // Pre-filling the prompt would require API extension; today
              // we just re-trigger and let the user copy from the tooltip.
              openClaudeAgentLauncher(paneId, false);
            }}
          >
            <span class="shrink-0 font-mono text-[9px] text-[var(--rg-fg-muted)] w-8 text-right">
              {timestamp(e.at)}
            </span>
            <span class="truncate text-[var(--rg-fg)]">{preview(e.prompt)}</span>
          </button>
        {/each}
        <div class="pl-5 pr-3 py-1">
          <button
            type="button"
            class="flex items-center gap-1 h-5 px-1.5 rounded text-[10px] text-[var(--rg-fg-muted)] hover:text-red-400 hover:bg-[var(--rg-surface)]/50 transition-colors"
            onclick={() => paneId && clearHistoryForPane(paneId)}
            title="清空此窗格的 Claude 历史"
          >
            <Trash2 class="h-3 w-3" />
            清空
          </button>
        </div>
      {/if}
    {/if}
  </div>
{/if}
