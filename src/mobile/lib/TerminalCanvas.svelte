<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import init, { TerminalKernel, RenderHandle } from '@ridge/term-wasm';
  import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';

  let { paneId, onStdin, onResize }: {
    paneId: string | null;
    onStdin: (data: string) => void;
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
  } = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let containerEl: HTMLDivElement | undefined = $state();

  let kernel: TerminalKernel | null = null;
  let renderHandle: RenderHandle | null = null;
  let rafId: number | null = null;
  let ready = $state(false);

  const pendingData: Uint8Array[] = [];
  let fitPending = false;

  let fontSize = $state(12);
  let cols = $state(80);
  let rows = $state(24);

  function calcFontSize(): number {
    const w = window.innerWidth;
    if (w < 360) return 10;
    if (w < 420) return 11;
    if (w < 540) return 12;
    if (w < 720) return 13;
    return 14;
  }

  function calcThemeBg(): string {
    return '#0d1117';
  }

  onMount(async () => {
    await init(wasmUrl);
    fontSize = calcFontSize();
    kernel = new TerminalKernel(rows, cols, 5000);
    renderHandle = new RenderHandle(canvasEl!);
    renderHandle.applyDefaultTheme();
    fitPane();
    flushPending();
    ready = true;
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

    fontSize = calcFontSize();

    canvasEl.width = Math.round(w * dpr);
    canvasEl.height = Math.round(h * dpr);
    canvasEl.style.width = w + 'px';
    canvasEl.style.height = h + 'px';
    renderHandle.resize(w, h, dpr);
    const [cellW, cellH] = renderHandle.configure(
      '"Cascadia Code", "Fira Code", "JetBrains Mono", monospace',
      fontSize,
      dpr,
    );
    if (cellW > 0 && cellH > 0) {
      cols = Math.max(1, Math.floor(w / cellW));
      rows = Math.max(1, Math.floor(h / cellH));
      kernel.resize(rows, cols);
      if (paneId && onResize) {
        onResize(paneId, rows, cols, Math.round(w), Math.round(h));
      }
    }
  }

  export function feed(data: string) {
    const bytes = new TextEncoder().encode(data);
    if (kernel) {
      kernel.feed(bytes);
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

  export function applyDelta(bytes: Uint8Array) {
    if (kernel) {
      kernel.applyDeltaFrame(bytes);
    } else {
      pendingData.push(bytes);
    }
  }

  export function applyDeltaBase64(b64: string) {
    const binary = atob(b64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    applyDelta(bytes);
  }

  function flushPending() {
    for (const b of pendingData) {
      kernel?.feed(b);
    }
    pendingData.length = 0;
  }

  // ─── Touch support: scroll with two-finger drag ──────────────────────
  let touchStartY = 0;
  let touchScrollAccum = 0;
  let lastTouchDistance = 0;
  let isTwoFinger = false;

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 2) {
      isTwoFinger = true;
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      lastTouchDistance = Math.sqrt(dx * dx + dy * dy);
      e.preventDefault();
    } else if (e.touches.length === 1 && !isTwoFinger) {
      touchStartY = e.touches[0].clientY;
      touchScrollAccum = 0;
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length === 2 && isTwoFinger) {
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const delta = lastTouchDistance - dist;
      if (Math.abs(delta) > 3) {
        const lines = Math.round(delta / 20);
        if (lines !== 0 && kernel) {
          if (lines < 0) kernel.scrollUp(-lines);
          else kernel.scrollDown(lines);
        }
        lastTouchDistance = dist;
      }
      e.preventDefault();
    } else if (e.touches.length === 1 && isTwoFinger) {
      // transition from 2-finger back to 1-finger — ignore
    } else if (e.touches.length === 1 && !isTwoFinger) {
      const dy = e.touches[0].clientY - touchStartY;
      touchScrollAccum += dy;
      if (Math.abs(touchScrollAccum) > 30) {
        const lines = touchScrollAccum > 0 ? -3 : 3;
        if (kernel) {
          if (lines < 0) kernel.scrollUp(-lines);
          else kernel.scrollDown(lines);
        }
        touchScrollAccum = 0;
      }
      touchStartY = e.touches[0].clientY;
    }
  }

  function handleTouchEnd(_e: TouchEvent) {
    isTwoFinger = false;
    touchScrollAccum = 0;
  }

  // ─── Keyboard handling ────────────────────────────────────────────
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

<div class="container" bind:this={containerEl} role="application"
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
>
  {#if !ready}
    <div class="loading">初始化终端引擎…</div>
  {/if}
  <canvas bind:this={canvasEl} class="term-canvas" class:hidden={!ready}></canvas>
</div>

<style>
  .container{position:relative;flex:1;overflow:hidden;background:#0d1117;touch-action:manipulation}
  .term-canvas{display:block;touch-action:none}
  .term-canvas.hidden{opacity:0}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:#8b949e;font-size:14px}
</style>
