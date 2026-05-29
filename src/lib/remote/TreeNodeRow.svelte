<script lang="ts" module>
  export interface TreeNode {
    name: string;
    type: 'dir' | 'file';
    children: TreeNode[];
  }
</script>

<script lang="ts">
  import TreeNodeRow from './TreeNodeRow.svelte';
  import { FileCode, FolderOpen } from 'lucide-svelte';

  interface Props {
    node: TreeNode;
    expanded: Record<string, boolean>;
    onToggle: (name: string) => void;
    depth?: number;
  }

  let { node, expanded, onToggle, depth = 0 }: Props = $props();

  let indent = $derived(depth * 16);
  let isDir = $derived(node.type === 'dir');
  let isExp = $derived(expanded[node.name]);
</script>
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  role="button"
  tabindex="0"
  class="flex items-center gap-1.5 py-0.5 rounded hover:bg-[var(--rg-surface)] cursor-pointer transition-colors"
  style="padding-left: {indent + 4}px"
  onclick={() => isDir && onToggle(node.name)}
  onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); if (isDir) onToggle(node.name); } }}
>
  {#if isDir}
    <span class="shrink-0 text-sm text-[var(--rg-fg-muted)]">{isExp ? '▼' : '▶'}</span>
    <FolderOpen class="w-4 h-4 shrink-0 text-yellow-500" />
  {:else}
    <FileCode class="w-4 h-4 shrink-0 text-[var(--rg-fg-muted)]" />
  {/if}
  <span class="text-sm truncate text-[var(--rg-fg)]">{node.name}{isDir ? '/' : ''}</span>
</div>

{#if isDir && isExp}
  {#each node.children as child}
    <TreeNodeRow node={child} {expanded} {onToggle} depth={depth + 1} />
  {/each}
{/if}
