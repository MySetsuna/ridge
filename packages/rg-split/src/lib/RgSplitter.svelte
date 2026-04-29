<!--
@component
RgSplitter — visual + interactive splitter sitting between two <RgPane>s.

This is also a pure scaffold. The actual drag is delegated to the consumer
through `onmousedown`. The component's job is purely the visual: a 1px center
line that scales 4× on hover / drag, plus a wider invisible hit area for
ergonomics (8px around the line via padding+negative margin).

The CSS uses `:global(.rg-split-row)` / `:global(.rg-split-col)` from the
parent <RgSplit> wrapper to pick orientation.
-->
<script lang="ts">
  import { getContext } from 'svelte';
  import { RG_SPLIT_CTX, type RgSplitContext } from './context.js';

  interface Props {
    /** Forwarded to the splitter root <div>. */
    class?: string;
    /** Marks the splitter as in drag state. Caller drives this from its own state. */
    dragging?: boolean;
    onmousedown?: (e: MouseEvent) => void;
  }

  let {
    class: className = '',
    dragging = false,
    onmousedown,
  }: Props = $props();

  const ctx = getContext<RgSplitContext>(RG_SPLIT_CTX);
  const orient = $derived(ctx?.direction === 'horizontal' ? 'col' : 'row');
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="rg-splitter rg-splitter-{orient} {dragging ? 'rg-splitter-dragging' : ''} {className}"
  role="separator"
  aria-orientation={orient === 'col' ? 'vertical' : 'horizontal'}
  {onmousedown}
></div>

<style>
  /* :global so the styles ship even without scoped-CSS pass (see RgSplit). */
  :global(.rg-splitter) {
    position: relative;
    flex-shrink: 0;
    border: none;
    background: transparent;
    box-sizing: content-box;
    overflow: visible;
    z-index: 1;
  }
  :global(.rg-splitter-col) {
    width: 1px;
    min-width: 0;
    padding: 0 5px;
    margin: 0 -5px;
    cursor: col-resize;
  }
  :global(.rg-splitter-row) {
    height: 1px;
    min-height: 0;
    padding: 5px 0;
    margin: -5px 0;
    cursor: row-resize;
  }
  :global(.rg-splitter::before) {
    content: '';
    position: absolute;
    background: var(--rg-splitter-color, rgba(255, 255, 255, 0.06));
    border-radius: 1px;
    transition:
      transform 0.12s ease,
      background 0.12s ease,
      box-shadow 0.12s ease;
    pointer-events: none;
  }
  :global(.rg-splitter-col::before) {
    top: 0;
    bottom: 0;
    left: 50%;
    width: 1px;
    transform: translateX(-50%) scaleX(1);
    transform-origin: center;
  }
  :global(.rg-splitter-row::before) {
    left: 0;
    right: 0;
    top: 50%;
    height: 1px;
    transform: translateY(-50%) scaleY(1);
    transform-origin: center;
  }
  :global(.rg-splitter-col:hover::before),
  :global(.rg-splitter-col.rg-splitter-dragging::before) {
    transform: translateX(-50%) scaleX(4);
    background: var(--rg-splitter-active-color, #a78bfa);
    box-shadow: 0 0 12px var(--rg-splitter-active-glow, rgba(167, 139, 250, 0.45));
  }
  :global(.rg-splitter-row:hover::before),
  :global(.rg-splitter-row.rg-splitter-dragging::before) {
    transform: translateY(-50%) scaleY(4);
    background: var(--rg-splitter-active-color, #a78bfa);
    box-shadow: 0 0 12px var(--rg-splitter-active-glow, rgba(167, 139, 250, 0.45));
  }
</style>
