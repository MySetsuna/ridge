<script lang="ts">
  // Static import (not a dynamic `await import('qrcode')`): the dynamic form
  // intermittently failed in Vite dev with "Failed to fetch dynamically
  // imported module" when the optimized-dep hash went stale after a
  // re-optimization. Importing statically bundles it with the entry.
  import { toCanvas } from 'qrcode';

  interface Props {
    value: string;
    size?: number;
  }

  let { value, size = 180 }: Props = $props();

  let canvas: HTMLCanvasElement;

  $effect(() => {
    if (canvas && value) {
      void toCanvas(canvas, value, {
        width: size,
        margin: 2,
        color: { dark: '#000', light: '#fff' },
      });
    }
  });
</script>

<canvas bind:this={canvas} width={size} height={size} class="rounded-xl"></canvas>
