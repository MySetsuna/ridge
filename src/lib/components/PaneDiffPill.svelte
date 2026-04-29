<script lang="ts">
  // src/lib/components/PaneDiffPill.svelte
  //
  // Sibling to PaneGitPill — renders ONLY the working-tree diff summary
  // (changed file count + added/removed line totals) and exposes a
  // single-click affordance: open the SCM sidebar tab AND scroll/focus the
  // matching repository within it.
  //
  // Why split out of PaneGitPill: the original combined pill mixed two
  // distinct affordances (switch branch vs. inspect changes) under one
  // click target, which made the diff numbers feel inert. Splitting also
  // matches VS Code's status-bar pattern (branch on the left, +N/-N
  // separately to its right).
  //
  // Hides itself entirely when there is no git repo / branch (same gate
  // as PaneGitPill) so non-git panes stay visually clean.

  import { Plus, Minus, FileText } from 'lucide-svelte';
  import { paneGitStatusStore, type PaneGitInfo } from '$lib/stores/paneGitStatus';

  interface Props {
    paneId: string;
  }

  let { paneId }: Props = $props();

  const info = $derived<PaneGitInfo | null>($paneGitStatusStore[paneId] ?? null);

  /**
   * Open the SCM sidebar tab and tell `SourceControl.svelte` to scroll the
   * given repo header into view + ensure its groups are expanded. Two
   * separate window events so a future power-user (or test) can listen
   * to either independently.
   */
  function openAndFocus(): void {
    if (!info) return;
    try {
      window.dispatchEvent(
        new CustomEvent('ridge:open-sidebar-tab', { detail: 'git' })
      );
      window.dispatchEvent(
        new CustomEvent('ridge:scm-focus-repo', { detail: info.repoRoot })
      );
    } catch {
      /* ignore — non-DOM env (vitest) */
    }
  }

  // We render even when the diff is clean (0/0/0) so the user has a
  // consistent "click here to inspect" target — but only when this is a
  // real git repo. The `±0` flat state is rendered with extra opacity
  // discount so it doesn't fight for attention.
  const isClean = $derived(
    !!info && info.dirtyFiles === 0 && info.added === 0 && info.removed === 0
  );
</script>

{#if info && info.branch}
  <button
    type="button"
    title={`改动文件：${info.dirtyFiles}\n+${info.added} -${info.removed}\n点击在源代码管理中查看此仓库`}
    class="flex items-center gap-1 h-5 px-1.5 rounded-full text-[10px] border transition-colors max-w-[200px]
      {isClean
        ? 'bg-[var(--rg-surface)]/40 border-[var(--rg-border)]/60 text-[var(--rg-fg-muted)]/70 hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]/80'
        : 'bg-[var(--rg-accent)]/10 border-[var(--rg-accent)]/25 text-[var(--rg-accent)]/90 hover:bg-[var(--rg-accent)]/22'}"
    onclick={openAndFocus}
  >
    <FileText class="h-3 w-3 shrink-0" />
    <span class="font-mono text-[9px] leading-none">{info.dirtyFiles}</span>
    {#if info.added > 0}
      <span class="flex items-center shrink-0 text-emerald-400 font-mono text-[9px] leading-none">
        <Plus class="h-2.5 w-2.5" />{info.added}
      </span>
    {/if}
    {#if info.removed > 0}
      <span class="flex items-center shrink-0 text-rose-400 font-mono text-[9px] leading-none">
        <Minus class="h-2.5 w-2.5" />{info.removed}
      </span>
    {/if}
  </button>
{/if}
