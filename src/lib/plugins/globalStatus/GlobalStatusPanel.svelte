<script lang="ts">
  // Minimal global-scope plugin. Proves the mount point works and gives users
  // a persistent "where am I" breadcrumb at the sidebar footer: current
  // workspace name + total pane count across the active tree.
  //
  // Kept intentionally small — the footer is thin real estate; anything
  // beefier should go into a dedicated tab or pane plugin.

  import { Activity, Layout } from 'lucide-svelte';
  import {
    paneTreeStore,
    workspacesList,
    activeWorkspaceId,
  } from '$lib/stores/paneTree';
  import type { PaneNode } from '$lib/types';

  function countLeaves(node: PaneNode | null): number {
    if (!node) return 0;
    if (node.type === 'leaf') return 1;
    return node.children.reduce((n, c) => n + countLeaves(c), 0);
  }

  const leafCount = $derived(countLeaves($paneTreeStore));
  const activeWs = $derived(
    $workspacesList.find((w) => w.id === $activeWorkspaceId) ?? null
  );
  const wsLabel = $derived(
    activeWs?.name?.trim() || (activeWs ? `工作区 ${activeWs.displaySeq}` : '—')
  );
</script>

<div
  class="px-3 py-1.5 flex items-center gap-2 text-[10px] text-[var(--rg-fg-muted)]"
  title={`${wsLabel} · ${leafCount} 个 pane`}
>
  <Layout class="h-3 w-3 text-[var(--rg-accent)]/70" />
  <span class="truncate flex-1">{wsLabel}</span>
  <span class="flex items-center gap-1 shrink-0">
    <Activity class="h-3 w-3" />
    <span class="font-mono">{leafCount}</span>
  </span>
</div>
