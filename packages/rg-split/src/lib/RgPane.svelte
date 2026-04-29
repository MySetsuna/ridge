<!--
@component
RgPane — percentage-sized pane inside an <RgSplit>.

Reads the parent <RgSplit>'s `direction` from context and writes the appropriate
inline dimension (`width` for horizontal layouts, `height` for vertical).
The pane never grows or shrinks beyond its declared `size` — `flex-grow: 0` +
`flex-shrink: 0`. Children are responsible for their own overflow handling
inside the pane.
-->
<script lang="ts">
  import { getContext, type Snippet } from 'svelte';
  import { RG_SPLIT_CTX, type RgSplitContext } from './context.js';

  interface Props {
    /** Percentage of parent's main axis. 0–100. */
    size: number;
    /** Forwarded to the pane root <div>. */
    class?: string;
    children: Snippet;
  }

  let { size, class: className = '', children }: Props = $props();

  const ctx = getContext<RgSplitContext>(RG_SPLIT_CTX);
  const dim = $derived(ctx?.direction === 'horizontal' ? 'width' : 'height');
</script>

<div
  class="rg-pane {className}"
  style="{dim}: {size}%;"
>
  {@render children()}
</div>

<style>
  :global(.rg-pane) {
    flex-grow: 0;
    flex-shrink: 0;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    box-sizing: border-box;
  }
</style>
