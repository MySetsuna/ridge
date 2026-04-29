<!--
@component
RgSplit — flex-based split container.

The container is a pure visual scaffold: it picks the right `flex-direction`
from `direction` and lets each child <RgPane> control its own percentage size
through inline `width` / `height`. There is no internal drag state machine —
callers wire mousedown / mousemove on the splitters themselves and update the
`size` props on the panes.

Usage:
  <RgSplit direction="horizontal" class="h-full w-full">
    <RgPane size={30}>left</RgPane>
    <RgSplitter onmousedown={...} />
    <RgPane size={70}>right</RgPane>
  </RgSplit>
-->
<script lang="ts">
  import { setContext, type Snippet } from 'svelte';
  import { RG_SPLIT_CTX, type RgSplitContext } from './context.js';

  interface Props {
    /** `horizontal` = panes laid out left-to-right; `vertical` = top-to-bottom. */
    direction: 'horizontal' | 'vertical';
    /** Forwarded to the root `<div>` so callers can compose Tailwind / scoped CSS. */
    class?: string;
    children: Snippet;
  }

  let { direction, class: className = '', children }: Props = $props();

  // Ratios live with the consumer; this context just lets RgPane / RgSplitter
  // know which dimension to write or which axis to listen for.
  const ctx: RgSplitContext = {
    get direction() {
      return direction;
    },
  };
  setContext(RG_SPLIT_CTX, ctx);
</script>

<div
  class="rg-split rg-split-{direction === 'horizontal' ? 'row' : 'col'} {className}"
  data-rg-split-direction={direction}
>
  {@render children()}
</div>

<style>
  /* Use :global so the styles ship even when the consumer bundles this package
     without a Svelte scoped-CSS pass (e.g. pnpm workspace + Vite that doesn't
     run `svelte-package` on the source). The class names are unique enough
     that the global leak is acceptable. */
  :global(.rg-split) {
    display: flex;
    box-sizing: border-box;
    min-width: 0;
    min-height: 0;
  }
  :global(.rg-split.rg-split-row) {
    flex-direction: row;
  }
  :global(.rg-split.rg-split-col) {
    flex-direction: column;
  }
</style>
