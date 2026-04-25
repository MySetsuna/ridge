<script lang="ts">
  // src/lib/components/GitGraph.svelte
  //
  // SVG branch-graph renderer for the SCM "图谱" panel. Pure layout logic
  // lives in `gitGraphLayout.ts` so vitest can exercise it without a
  // Svelte compiler in the test pipeline; this file just maps the
  // computed primitives into <path> + <circle> elements.

  import { layoutGraph, DEFAULT_DX, DEFAULT_DY, type GraphCommit } from './gitGraphLayout';

  interface Props {
    commits: GraphCommit[];
    /** Optional layout overrides — defaults sized for the SCM panel. */
    dx?: number;
    dy?: number;
  }

  let { commits, dx = DEFAULT_DX, dy = DEFAULT_DY }: Props = $props();

  // Re-layout whenever the commit list identity changes. Cheap (~µs per
  // commit); no need to memoise beyond Svelte's $derived.
  const layout = $derived(layoutGraph(commits, { dx, dy }));
</script>

<svg
  width={layout.width}
  height={layout.totalHeight}
  class="block shrink-0"
  aria-hidden="true"
>
  <!-- Lines first so dots paint on top — z-order is paint order in SVG. -->
  {#each layout.lines as line, i (i)}
    <path d={line.d} stroke={line.color} stroke-width="1.5" fill="none" stroke-linecap="round" />
  {/each}
  {#each layout.dots as dot (dot.hash)}
    <circle
      cx={dot.cx}
      cy={dot.cy}
      r="4"
      fill={dot.color}
      stroke="var(--wf-bg)"
      stroke-width="1.5"
    />
  {/each}
</svg>
