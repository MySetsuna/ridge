<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import init, { TerminalKernel, RenderHandle } from '@ridge/term-wasm';
  import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';
  import VirtualKeyboard from './VirtualKeyboard.svelte';

  let { paneId, onStdin, onResize, onRefresh, showKeyboard = false }: {
    paneId: string | null;
    onStdin: (data: string) => void;
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    onRefresh?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    showKeyboard?: boolean;
  } = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let containerEl: HTMLDivElement | undefined = $state();

  let kernel: TerminalKernel | null = null;
  let renderHandle: RenderHandle | null = null;
  let rafId: number | null = null;
  let ready = $state(false);

  const pendingData: Uint8Array[] = [];
  let fitPending = false;
  // §multi-size: true once this endpoint has auto-claimed the shared PTY size.
  let claimed = false;
  let isComposing = false;
  let compositionStdinTarget: string | null = null;

  let fontSize = $state(12);
  let cols = $state(80);
  let rows = $state(24);
  let cellW = $state(8);
  let cellH = $state(16);

  let copySuccess = $state(false);

  function calcFontSize(): number {
    const w = containerEl ? containerEl.clientWidth : window.innerWidth;
    if (w < 360) return 10;
    if (w < 420) return 11;
    if (w < 540) return 12;
    if (w < 720) return 13;
    return 14;
  }

  onMount(async () => {
    await init(wasmUrl);
    fontSize = calcFontSize();
    kernel = new TerminalKernel(rows, cols, 5000);
    renderHandle = await RenderHandle.newWithWebgpuFirst(canvasEl!);
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
  let fitDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  onMount(() => {
    ro = new ResizeObserver(() => {
      if (fitDebounceTimer) clearTimeout(fitDebounceTimer);
      fitDebounceTimer = setTimeout(() => fitPane(), 150);
    });
    if (containerEl) ro.observe(containerEl);
    return () => {
      if (fitDebounceTimer) clearTimeout(fitDebounceTimer);
      ro?.disconnect();
    };
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
    const dims = renderHandle.configure(
      '"Cascadia Code", "Fira Code", "JetBrains Mono", monospace',
      fontSize,
      dpr,
    );
    if (dims.length >= 2) {
      cellW = dims[0];
      cellH = dims[1];
    }
    if (cellW > 0 && cellH > 0) {
      cols = Math.max(1, Math.floor(w / cellW));
      rows = Math.max(1, Math.floor(h / cellH));
      // §multi-size: the kernel grid is driven by the SHARED canonical delta
      // stream (Resize ops), not the local viewport — so we do NOT
      // kernel.resize() here. A fresh endpoint claims the shared PTY size
      // exactly once on first fit; later viewport changes don't touch the
      // PTY (use refresh() to re-claim on demand).
      if (!claimed && paneId && onResize) {
        claimed = true;
        onResize(paneId, rows, cols, Math.round(w), Math.round(h));
      }
    }
  }

  /// Re-claim the shared PTY at this client's current viewport size and
  /// trigger a full repaint. Wired to the toolbar refresh button.
  export function refresh() {
    fitPane();
    if (paneId && onRefresh && containerEl) {
      onRefresh(
        paneId,
        rows,
        cols,
        Math.round(containerEl.clientWidth),
        Math.round(containerEl.clientHeight),
      );
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

  // ─── Virtual Keyboard integration ──────────────────────────────────
  function handleVirtualKey(key: string, ctrl: boolean, alt: boolean, shift: boolean) {
    if (!paneId || !kernel) return;
    const bytes = kernel.encodeKey(key, ctrl, alt, shift, false);
    if (bytes.length > 0) {
      onStdin(new TextDecoder().decode(bytes));
      return;
    }
    if (key === 'Tab') {
      onStdin(shift ? '\x1b[Z' : '\t');
      return;
    }
    if (key === 'Escape') { onStdin('\x1b'); return; }
    if (key === 'Enter') { onStdin('\r'); return; }
    if (key === 'Backspace') { onStdin('\x7f'); return; }
    if (key === 'Delete') { onStdin('\x1b[3~'); return; }
    if (key === 'Home') { onStdin('\x1b[H'); return; }
    if (key === 'End') { onStdin('\x1b[F'); return; }
    if (key === 'PageUp') { onStdin('\x1b[5~'); return; }
    if (key === 'PageDown') { onStdin('\x1b[6~'); return; }
    if (key === 'Insert') { onStdin('\x1b[2~'); return; }
    if (key.startsWith('Arrow')) {
      const map: Record<string, string> = {
        ArrowUp: '\x1b[A', ArrowDown: '\x1b[B',
        ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D',
      };
      onStdin(map[key] || '');
    }
  }

  // ─── Touch support: scroll + selection + mouse ─────────────────────
  let touchStartY = 0;
  let touchStartX = 0;
  let touchScrollAccum = 0;
  let lastTouchDistance = 0;
  let isTwoFinger = false;
  let touchStartTime = 0;
  let isSelecting = false;
  let selAnchorRow = 0;
  let selAnchorCol = 0;
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  let didLongPress = false;

  function touchToCell(clientX: number, clientY: number): { row: number; col: number } | null {
    if (!canvasEl || cellW <= 0 || cellH <= 0) return null;
    const rect = canvasEl.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    const col = Math.max(0, Math.floor(x / cellW));
    const row = Math.max(0, Math.floor(y / cellH));
    return { row, col };
  }

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 2) {
      isTwoFinger = true;
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      lastTouchDistance = Math.sqrt(dx * dx + dy * dy);
      e.preventDefault();
      return;
    }
    if (e.touches.length !== 1) return;
    touchStartY = e.touches[0].clientY;
    touchStartX = e.touches[0].clientX;
    touchScrollAccum = 0;
    touchStartTime = Date.now();
    isSelecting = false;
    didLongPress = false;

    const touch = e.touches[0];
    if (longPressTimer) clearTimeout(longPressTimer);
    longPressTimer = setTimeout(() => {
      if (!kernel || isTwoFinger) return;
      didLongPress = true;
      const cell = touchToCell(touch.clientX, touch.clientY);
      if (cell) {
        isSelecting = true;
        selAnchorRow = cell.row;
        selAnchorCol = cell.col;
        kernel.clearSelection();
        try { navigator.vibrate(15); } catch {}
      }
    }, 400);
  }

  function handleTouchMove(e: TouchEvent) {
    if (e.touches.length === 2 && isTwoFinger) {
      if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
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
      return;
    }
    if (e.touches.length === 1 && isTwoFinger) return;

    if (e.touches.length === 1 && !isTwoFinger) {
      if (isSelecting && kernel) {
        if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
        e.preventDefault();
        const cell = touchToCell(e.touches[0].clientX, e.touches[0].clientY);
        if (cell) {
          const absRow = kernel.scrollbackLen() > 0
            ? cell.row + (kernel.scrollOffset() > 0 ? kernel.scrollOffset() : 0)
            : cell.row;
          kernel.setSelectionAbs(selAnchorRow, selAnchorCol, absRow, cell.col);
        }
        return;
      }
      if (didLongPress) return;
      const dy = e.touches[0].clientY - touchStartY;
      if (Math.abs(dy) > 10) {
        if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
      }
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

  function handleTouchEnd(e: TouchEvent) {
    if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
    if (isTwoFinger) {
      isTwoFinger = false;
      touchScrollAccum = 0;
      isSelecting = false;
      return;
    }

    if (isSelecting) {
      isSelecting = false;
      touchScrollAccum = 0;
      return;
    }

    if (didLongPress) {
      didLongPress = false;
      touchScrollAccum = 0;
      return;
    }

    // Quick tap: forward as mouse click if terminal is in mouse reporting mode
    const elapsed = Date.now() - touchStartTime;
    if (elapsed < 250 && kernel && cellW > 0 && cellH > 0) {
      const touch = e.changedTouches[0];
      if (touch) {
        const cell = touchToCell(touch.clientX, touch.clientY);
        if (cell && kernel.isMouseReporting()) {
          const bytes = kernel.encodeMouse(cell.row, cell.col, 0, 0, false, false, false);
          if (bytes.length > 0) {
            onStdin(new TextDecoder().decode(bytes));
          }

          // Send mouse release
          requestAnimationFrame(() => {
            if (kernel) {
              const releaseBytes = kernel.encodeMouse(cell.row, cell.col, 3, 1, false, false, false);
              if (releaseBytes.length > 0) {
                onStdin(new TextDecoder().decode(releaseBytes));
              }
            }
          });
        }
      }
    }

    isTwoFinger = false;
    touchScrollAccum = 0;
  }

  // ─── Copy handler ──────────────────────────────────────────────────
  async function handleCopy() {
    if (!kernel || !kernel.hasSelection()) return;
    const text = kernel.getSelectionText();
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      copySuccess = true;
      setTimeout(() => copySuccess = false, 1500);
    } catch {}
  }

  function handleDismissSelection() {
    kernel?.clearSelection();
  }

  function handleCompositionStart(_e: CompositionEvent) {
    isComposing = true;
    compositionStdinTarget = null;
  }

  function handleCompositionUpdate(e: CompositionEvent) {
    if (!paneId || !kernel || !renderHandle) return;
    const r = kernel.cursorRow?.() ?? -1;
    const c = kernel.cursorCol?.() ?? -1;
    const h = renderHandle as unknown as { setPreedit?: (t: string, r: number, c: number) => void };
    h.setPreedit?.(e.data, r, c);
  }

  function handleCompositionEnd(e: CompositionEvent) {
    isComposing = false;
    if (!paneId || !kernel || !renderHandle) return;
    const h = renderHandle as unknown as { clearPreedit?: () => void };
    h.clearPreedit?.();
    const text = e.data;
    if (text) {
      const bytes = kernel.encodePaste(text);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
    }
  }

  // ─── Keyboard handling ────────────────────────────────────────────
  function handleKeydown(e: KeyboardEvent) {
    if (isComposing || e.isComposing) return;
    if (!paneId || !kernel) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      onStdin('\r');
      return;
    }
    if (e.key === 'Backspace') {
      e.preventDefault();
      const bytes = kernel.encodeKey('Backspace', e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x7f');
      return;
    }
    if (e.key === 'Delete') {
      e.preventDefault();
      const bytes = kernel.encodeKey('Delete', e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x1b[3~');
      return;
    }
    if (e.key === 'Tab') {
      e.preventDefault();
      if (e.shiftKey) {
        onStdin('\x1b[Z');
      } else {
        onStdin('\t');
      }
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      onStdin('\x1b');
      return;
    }
    if (e.key === 'Home') {
      e.preventDefault();
      const bytes = kernel.encodeKey('Home', e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x1b[H');
      return;
    }
    if (e.key === 'End') {
      e.preventDefault();
      const bytes = kernel.encodeKey('End', e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x1b[F');
      return;
    }
    if (e.key === 'PageUp') {
      e.preventDefault();
      const bytes = kernel.encodeKey('PageUp', e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x1b[5~');
      return;
    }
    if (e.key === 'PageDown') {
      e.preventDefault();
      const bytes = kernel.encodeKey('PageDown', e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin('\x1b[6~');
      return;
    }
    if (e.key === 'Insert') {
      e.preventDefault();
      onStdin('\x1b[2~');
      return;
    }
    if (e.key.startsWith('F') && e.key.length >= 2) {
      e.preventDefault();
      const fn = parseInt(e.key.slice(1));
      if (fn >= 1 && fn <= 12) {
        let seq = '';
        if (fn === 1) seq = '\x1bOP';
        else if (fn === 2) seq = '\x1bOQ';
        else if (fn === 3) seq = '\x1bOR';
        else if (fn === 4) seq = '\x1bOS';
        else if (fn <= 5) seq = '\x1b[15~';
        else if (fn === 6) seq = '\x1b[17~';
        else if (fn === 7) seq = '\x1b[18~';
        else if (fn === 8) seq = '\x1b[19~';
        else if (fn === 9) seq = '\x1b[20~';
        else if (fn === 10) seq = '\x1b[21~';
        else if (fn === 11) seq = '\x1b[23~';
        else if (fn === 12) seq = '\x1b[24~';
        if (seq) onStdin(seq);
      }
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      const bytes = kernel.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) {
        e.preventDefault();
        onStdin(new TextDecoder().decode(bytes));
      }
      return;
    }
    if (e.key.length === 1) {
      e.preventDefault();
      const bytes = kernel.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin(e.key);
      return;
    }
    if (e.key.startsWith('Arrow')) {
      e.preventDefault();
      const map: Record<string, string> = {
        ArrowUp: '\x1b[A', ArrowDown: '\x1b[B',
        ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D',
      };
      const bytes = kernel.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else if (map[e.key]) onStdin(map[e.key]);
    }
  }

  async function handlePaste(e: ClipboardEvent) {
    if (!paneId || !kernel) return;
    e.preventDefault();
    const text = e.clipboardData?.getData('text') ?? '';
    if (!text) return;
    const bytes = kernel.encodePaste(text);
    if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
  }
</script>

<svelte:window
  onkeydown={handleKeydown}
  oncompositionstart={handleCompositionStart}
  oncompositionupdate={handleCompositionUpdate}
  oncompositionend={handleCompositionEnd}
  onpaste={handlePaste}
/>

<div class="container" bind:this={containerEl} role="application"
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
>
  {#if !ready}
    <div class="loading">初始化终端引擎…</div>
  {/if}
  <canvas bind:this={canvasEl} class="term-canvas" class:hidden={!ready}></canvas>

  {#if ready && kernel?.hasSelection()}
    <div class="selection-actions">
      <button class="copy-btn" onclick={handleCopy}>
        {copySuccess ? '✓ 已复制' : '复制'}
      </button>
      <button class="dismiss-btn" onclick={handleDismissSelection}>✕</button>
    </div>
  {/if}
</div>

{#if showKeyboard}
  <VirtualKeyboard onKey={handleVirtualKey} />
{/if}

<style>
  .container{position:relative;flex:1;overflow:hidden;background:#0d1117;touch-action:manipulation}
  .term-canvas{display:block;width:100%;height:100%;touch-action:none}
  .term-canvas.hidden{opacity:0}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:#8b949e;font-size:14px}

  .selection-actions{position:absolute;bottom:12px;right:12px;display:flex;gap:6px;z-index:10}
  .copy-btn,.dismiss-btn{display:flex;align-items:center;justify-content:center;height:36px;padding:0 14px;border:none;border-radius:8px;font-size:13px;font-weight:500;cursor:pointer;transition:all .12s;touch-action:manipulation}
  .copy-btn{background:#238636;color:#fff}
  .copy-btn:active{background:#2ea043}
  .dismiss-btn{background:#21262d;color:#8b949e;min-width:36px}
  .dismiss-btn:active{background:#30363d}
</style>
