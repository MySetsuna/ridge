<script lang="ts">
  import { onMount } from 'svelte';

  interface Props {
    value: string;
    size?: number;
  }

  let { value, size = 180 }: Props = $props();

  let canvas: HTMLCanvasElement;
  let drawQr: ((c: HTMLCanvasElement, v: string, s: number) => void) | null = null;

  onMount(async () => {
    const qrcode = await import('qrcode');
    drawQr = (c: HTMLCanvasElement, v: string, s: number) => {
      qrcode.toCanvas(c, v, { width: s, margin: 2, color: { dark: '#000', light: '#fff' } });
    };
    if (canvas) {
      drawQr(canvas, value, size);
    }
  });

  $effect(() => {
    if (canvas && drawQr) {
      drawQr(canvas, value, size);
    }
  });
</script>

<canvas bind:this={canvas} width={size} height={size} class="rounded-xl"></canvas>
