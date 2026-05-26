<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import init, { TerminalKernel, RenderHandle } from '@ridge/term-wasm';
  import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';

  let { paneId, onStdin }: {
    paneId: string | null;
    onStdin: (data: string) => void;
  } = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let containerEl: HTMLDivElement | undefined = $state();

  let kernel: TerminalKernel | null = null;
  let renderHandle: RenderHandle | null = null;
  let rafId: number | null = null;
  let ready = $state(false);

  const pendingData: Uint8Array[] = [];
  let fitPending = false;

  // ――― Lifecycle ――――――――――――――――――――――――――――――――――――――――――
  onMount(async () => {
    await init(wasmUrl);
    kernel = new TerminalKernel(24, 80, 5000);
    renderHandle = new RenderHandle(canvasEl!);
    renderHandle.applyDefaultTheme();
    fitPane();
    flushPending();
    ready = true;
    // Start the rendering loop.
    function frame() {
      if (kernel && renderHandle) renderHandle.render(kernel);
      rafId = requestAnimationFrame(frame);
    }
    rafId = requestAnimationFrame(frame);
  });

  onDestroy(() => {
    if (rafId !== null) cancelAnimationFrame(rafId);
    renderHandle?.free();
    kernel?.free();
  });

  // ――― Resize observation ――――――――――――――――――――――――――――――
  let ro: ResizeObserver | undefined;
  onMount(() => {
    ro = new ResizeObserver(() => fitPane());
    if (containerEl) ro.observe(containerEl);
    return () => ro?.disconnect();
  });

  function fitPane() {
    if (!renderHandle || !canvasEl || !kernel || !containerEl) {
      fitPending = true;
      return;
    }
    fitPending = false;
    const dpr = window.devicePixelRatio || 1;
    const w = containerEl.clientWidth;
    const h = containerEl.clientHeight;
    if (w <= 0 || h <= 0) return;
    canvasEl.width = Math.round(w * dpr);
    canvasEl.height = Math.round(h * dpr);
    canvasEl.style.width = w + 'px';
    canvasEl.style.height = h + 'px';
    renderHandle.resize(w, h, dpr);
    const [cellW, cellH] = renderHandle.configure(
      '"Cascadia Code", "Fira Code", "JetBrains Mono", monospace',
      14,
      dpr,
    );
    if (cellW > 0 && cellH > 0) {
      const cols = Math.max(1, Math.floor(w / cellW));
      const rows = Math.max(1, Math.floor(h / cellH));
      kernel.resize(rows, cols);
    }
  }

  // ――― Public: feed output data into kernel ――――――――――
  export function feed(data: string) {
    const bytes = new TextEncoder().encode(data);
    if (kernel) {
      kernel.feed(bytes);
      // Handle DSR/DA response bytes.
      const resp = kernel.takePendingResponse();
      if (resp.length > 0 && paneId && onStdin) {
        onStdin(new TextDecoder().decode(resp));
      }
    } else {
      pendingData.push(bytes);
    }
  }

  export function feedUtf8(bytes: Uint8Array) {
    if (kernel) {
      kernel.feed(bytes);
    } else {
      pendingData.push(bytes);
    }
  }

  function flushPending() {
    for (const b of pendingData) {
      kernel?.feed(b);
    }
    pendingData.length = 0;
  }

  // ――― Keyboard handling ―――――――――――――――――――――――――――――
  function handleKeydown(e: KeyboardEvent) {
    if (!paneId || !kernel || e.ctrlKey || e.metaKey) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      onStdin('\r');
      return;
    }
    if (e.key === 'Backspace') {
      e.preventDefault();
      const bytes = kernel.encodeKey('Backspace', false, false, false, false);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x7f');
      return;
    }
    if (e.key === 'Tab') {
      e.preventDefault();
      onStdin('\t');
      return;
    }
    if (e.key.length === 1) {
      e.preventDefault();
      const bytes = kernel.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, false);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin(e.key);
    }
    if (e.key.startsWith('Arrow')) {
      e.preventDefault();
      const map: Record<string, string> = {
        ArrowUp: '\x1b[A', ArrowDown: '\x1b[B',
        ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D',
      };
      const bytes = kernel.encodeKey(e.key, false, e.altKey, e.shiftKey, false);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else if (map[e.key]) onStdin(map[e.key]);
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="container" bind:this={containerEl}>
  {#if !ready}
    <div class="loading">初始化终端引擎…</div>
  {/if}
  <canvas bind:this={canvasEl} class="term-canvas" class:hidden={!ready}></canvas>
</div>

<style>
  .container{position:relative;flex:1;overflow:hidden;background:#0d1117}
  .term-canvas{display:block}
  .term-canvas.hidden{opacity:0}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:#8b949e;font-size:14px}
</style>
