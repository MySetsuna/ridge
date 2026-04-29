<script lang="ts">
  // Round 37 follow-up: dropped the Claude history badge ("N pane · M
  // 历史") per user request — Claude UI now lives exclusively in the
  // dedicated Claude Code tab (round 27 + 34). This panel keeps the
  // generic per-workspace pane count, which is genuinely useful as a
  // workspace overview hint independent of any extension.
  import { Activity } from 'lucide-svelte';
  import { paneTreeStore } from '$lib/stores/paneTree';
  import type { PaneNode } from '$lib/types';

  interface Props {
    /** Supplied by SidebarPluginRegion for scope='workspace'. */
    workspaceId?: string;
  }

  let { workspaceId }: Props = $props();

  // paneTreeStore holds the *active* workspace's tree only. This plugin
  // gets mounted for every workspace group, so without cross-workspace
  // pane data the count reflects the active workspace.
  function flattenLeaves(node: PaneNode | null): string[] {
    if (!node) return [];
    if (node.type === 'leaf') return [node.id];
    return node.children.flatMap(flattenLeaves);
  }

  const localLeaves = $derived(flattenLeaves($paneTreeStore));
</script>

<div class="px-3 py-1 flex items-center gap-2 text-[10px] text-[var(--rg-fg-muted)] border-b border-[var(--rg-border)]/40 bg-[var(--rg-surface-2)]/20">
  <span class="flex items-center gap-1 ml-auto" title="当前工作区 pane 数">
    <Activity class="h-3 w-3" />
    <span class="font-mono">{localLeaves.length}</span>
    <span>pane</span>
  </span>
</div>
