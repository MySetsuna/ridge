<script lang="ts">
  // src/lib/components/PaneRepoSwitcher.svelte
  //
  // Sibling-left of `PaneGitPill` — only renders when the pane's cwd
  // hosts MORE THAN ONE git repo (round 40 cwd-down semantics:
  // `find_git_repos_below(cwd, max_depth=1)` returns N>1).
  //
  // Click → dropdown lists each repo's basename (full path in tooltip);
  // pick → `setPaneSelectedRepo(paneId, repoRoot)` rewires the pill
  // pair to that repo's data.

  import { onMount, onDestroy } from 'svelte';
  import { Folder, ChevronDown, Check } from 'lucide-svelte';
  import {
    paneGitStatusStore,
    setPaneSelectedRepo,
    type PaneGitInfo,
  } from '$lib/stores/paneGitStatus';

  interface Props {
    paneId: string;
  }
  let { paneId }: Props = $props();

  const info = $derived<PaneGitInfo | null>($paneGitStatusStore[paneId] ?? null);
  const repos = $derived(info?.availableRepos ?? []);

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();

  function basename(path: string): string {
    const parts = path.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] || path;
  }

  function pick(repoRoot: string): void {
    void setPaneSelectedRepo(paneId, repoRoot);
    open = false;
  }

  function onGlobalMousedown(e: MouseEvent): void {
    if (!open) return;
    const t = e.target as HTMLElement | null;
    if (root && t && root.contains(t)) return;
    open = false;
  }
  function onGlobalKeydown(e: KeyboardEvent): void {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      open = false;
    }
  }
  onMount(() => {
    document.addEventListener('mousedown', onGlobalMousedown, true);
    document.addEventListener('keydown', onGlobalKeydown);
  });
  onDestroy(() => {
    document.removeEventListener('mousedown', onGlobalMousedown, true);
    document.removeEventListener('keydown', onGlobalKeydown);
  });
</script>

<!-- Hide entirely unless the pane has multiple discoverable repos —
     single-repo case shows the branch pill alone, no clutter. -->
{#if info && repos.length > 1}
  <div class="relative" bind:this={root}>
    <button
      type="button"
      class="flex items-center gap-1 h-5 px-1.5 rounded-full text-[10px] bg-[var(--wf-surface)]/60 text-[var(--wf-fg-muted)] border border-[var(--wf-border)] hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-colors max-w-[140px]
        {open ? 'bg-[var(--wf-surface)] text-[var(--wf-fg)]' : ''}"
      title={`此 pane cwd 中检测到 ${repos.length} 个 git 仓库\n当前：${info.repoRoot}`}
      onclick={() => (open = !open)}
    >
      <Folder class="h-3 w-3 shrink-0" />
      <span class="truncate">{basename(info.repoRoot)}</span>
      <ChevronDown class="h-2.5 w-2.5 shrink-0 opacity-70" />
    </button>

    {#if open}
      <div
        class="absolute left-0 top-[26px] z-50 min-w-[180px] max-w-[320px] rounded-lg border border-[var(--wf-border)] bg-[var(--wf-bg-raised)] shadow-xl overflow-hidden"
        role="menu"
      >
        {#each repos as repo (repo)}
          {@const isCurrent = repo === info.repoRoot}
          <button
            type="button"
            role="menuitem"
            class="w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-left hover:bg-[var(--wf-surface)] transition-colors"
            title={repo}
            onclick={() => pick(repo)}
          >
            {#if isCurrent}
              <Check class="h-3 w-3 text-[var(--wf-accent)] shrink-0" />
            {:else}
              <span class="w-3 shrink-0"></span>
            {/if}
            <Folder class="h-3 w-3 shrink-0 text-[var(--wf-fg-muted)]" />
            <span class="truncate flex-1 {isCurrent ? 'text-[var(--wf-accent)]' : 'text-[var(--wf-fg)]'}">
              {basename(repo)}
            </span>
          </button>
        {/each}
      </div>
    {/if}
  </div>
{/if}
