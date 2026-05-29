<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    value: string;
    size?: number;
  }

  let { value, size = 180 }: Props = $props();

  let canvas: HTMLCanvasElement;
  // Loaded client-only in onMount. A top-level static `import 'qrcode'` pulls
  // the package into SvelteKit's SSR pass and 500s the page (qrcode's browser
  // build touches DOM/canvas). The earlier "Failed to fetch dynamically
  // imported module" flakiness in Vite dev is fixed by pre-bundling qrcode via
  // `optimizeDeps.include` in vite.config.js, not by importing it statically.
  let toCanvas = $state<
    | ((c: HTMLCanvasElement, v: string, opts: Record<string, unknown>) => Promise<void>)
    | null
  >(null);

  onMount(async () => {
    // Vite dev can transiently fail this dynamic import with "Failed to fetch
    // dynamically imported module" right as it force-re-optimizes deps (the
    // ?v= hash changes mid-session — e.g. when remote control is toggled and a
    // second Vite server spins up). qrcode is already in optimizeDeps.include;
    // a short retry rides over the re-optimization and resolves the fresh hash.
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        const mod = await import('qrcode');
        toCanvas = mod.toCanvas;
        return;
      } catch (e) {
        if (attempt === 2) {
          console.warn('[qrcode] dynamic import failed after retries', e);
          return;
        }
        await new Promise((r) => setTimeout(r, 150));
      }
    }
  });

  $effect(() => {
    if (canvas && value && toCanvas) {
      void toCanvas(canvas, value, {
        width: size,
        margin: 2,
        color: { dark: '#000', light: '#fff' },
      });
    }
  });
</script>

<canvas bind:this={canvas} width={size} height={size} class="rounded-xl"></canvas>
