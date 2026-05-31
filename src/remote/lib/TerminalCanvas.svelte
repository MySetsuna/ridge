<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { TerminalController } from './terminalController';
  import VirtualKeyboard from './VirtualKeyboard.svelte';

  let { paneId, onStdin, onResize, showKeyboard = false }: {
    paneId: string | null;
    onStdin: (data: string) => void;
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    showKeyboard?: boolean;
  } = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let containerEl: HTMLDivElement | undefined = $state();
  let ctrl: TerminalController | null = null;
  let ready = $state(false);
  let hasSelection = $state(false);
  let copySuccess = $state(false);

  // Touch state
  let touchStartY = 0;
  let touchStartX = 0;
  let touchScrollAccum = 0;
  let lastTouchDistance = 0;
  let isTwoFinger = false;
  let touchStartTime = 0;
  let didLongPress = false;
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;

  onMount(async () => {
    if (!canvasEl || !containerEl) return;
    ctrl = await TerminalController.create(canvasEl, containerEl);
    ctrl.onStdin = (data) => { if (paneId) onStdin(data); };
    ctrl.onResize = (r, c, pw, ph) => {
      if (paneId && onResize) onResize(paneId, r, c, pw, ph);
    };
    ready = true;
  });

  onDestroy(() => { ctrl?.destroy(); });

  let ro: ResizeObserver | undefined;
  onMount(() => {
    ro = new ResizeObserver(() => ctrl?.requestResize());
    if (containerEl) ro.observe(containerEl);
    return () => ro?.disconnect();
  });

  // ── Public API ──
  export function feed(data: string) {
    if (ctrl) ctrl.feed(new TextEncoder().encode(data));
  }
  export function feedUtf8(bytes: Uint8Array) { ctrl?.feed(bytes); }
  export function applyDelta(bytes: Uint8Array) { ctrl?.applyDelta(bytes); }
  export function resizeKernel(rows: number, cols: number) {
    if (ctrl) {
      ctrl.kernelResize(rows, cols);
    }
  }
  export function applyDeltaBase64(b64: string) {
    const binary = atob(b64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    ctrl?.applyDelta(bytes);
  }

  // ── Virtual Keyboard ──
  function handleVirtualKey(key: string, ctrlKey: boolean, alt: boolean, shift: boolean) {
    if (!paneId || !ctrl) return;
    const bytes = ctrl.encodeKey(key, ctrlKey, alt, shift, false);
    if (bytes.length > 0) { onStdin(new TextDecoder().decode(bytes)); return; }
    const map: Record<string, string> = { Tab: '\t', Escape: '\x1b', Enter: '\r', Backspace: '\x7f', Delete: '\x1b[3~', Home: '\x1b[H', End: '\x1b[F', PageUp: '\x1b[5~', PageDown: '\x1b[6~', Insert: '\x1b[2~' };
    if (map[key]) { onStdin(shift && key === 'Tab' ? '\x1b[Z' : map[key]); return; }
    if (key.startsWith('Arrow')) {
      const arrows: Record<string, string> = { ArrowUp: '\x1b[A', ArrowDown: '\x1b[B', ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D' };
      if (arrows[key]) onStdin(arrows[key]);
    }
  }

  // ── Touch ──
  function handleTouchStart(e: TouchEvent) {
    if (!ctrl) return;
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
    didLongPress = false;
    const touch = e.touches[0];
    if (longPressTimer) clearTimeout(longPressTimer);
    longPressTimer = setTimeout(() => {
      if (!ctrl || isTwoFinger) return;
      didLongPress = true;
      const cell = ctrl.clientToCell(touch.clientX, touch.clientY);
      if (cell) {
        ctrl.startSelection(cell.row, cell.col);
        hasSelection = ctrl.hasSelection();
        try { navigator.vibrate(15); } catch {}
      }
    }, 400);
  }

  function handleTouchMove(e: TouchEvent) {
    if (!ctrl) return;
    if (e.touches.length === 2 && isTwoFinger) {
      if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
      const dx = e.touches[0].clientX - e.touches[1].clientX;
      const dy = e.touches[0].clientY - e.touches[1].clientY;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const delta = lastTouchDistance - dist;
      if (Math.abs(delta) > 3) {
        const lines = Math.round(delta / 20);
        if (lines < 0) ctrl.scrollUp(-lines);
        else ctrl.scrollDown(lines);
        lastTouchDistance = dist;
      }
      e.preventDefault();
      return;
    }
    if (e.touches.length === 1 && !isTwoFinger) {
      if (ctrl.isSelecting) {
        if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
        e.preventDefault();
        const cell = ctrl.clientToCell(e.touches[0].clientX, e.touches[0].clientY);
        if (cell) { ctrl.extendSelection(cell.row, cell.col); hasSelection = ctrl.hasSelection(); }
        return;
      }
      if (didLongPress) return;
      const dy = e.touches[0].clientY - touchStartY;
      if (Math.abs(dy) > 10 && longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
      touchScrollAccum += dy;
      if (Math.abs(touchScrollAccum) > 30) {
        const lines = touchScrollAccum > 0 ? -3 : 3;
        if (lines < 0) ctrl.scrollUp(-lines);
        else ctrl.scrollDown(lines);
        touchScrollAccum = 0;
      }
      touchStartY = e.touches[0].clientY;
    }
  }

  function handleTouchEnd(e: TouchEvent) {
    if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
    if (isTwoFinger) { isTwoFinger = false; touchScrollAccum = 0; return; }
    if (ctrl?.isSelecting) { ctrl.endSelection(); hasSelection = ctrl.hasSelection(); touchScrollAccum = 0; return; }
    if (didLongPress) { didLongPress = false; touchScrollAccum = 0; return; }
    const elapsed = Date.now() - touchStartTime;
    if (elapsed < 250 && ctrl) {
      const touch = e.changedTouches[0];
      if (touch) {
        const cell = ctrl.clientToCell(touch.clientX, touch.clientY);
        if (cell && ctrl.isMouseReporting()) {
          const bytes = ctrl.encodeMouse(cell.row, cell.col, 0, 0, false, false, false);
          if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
          requestAnimationFrame(() => {
            if (ctrl) {
              const releaseBytes = ctrl.encodeMouse(cell.row, cell.col, 3, 1, false, false, false);
              if (releaseBytes.length > 0) onStdin(new TextDecoder().decode(releaseBytes));
            }
          });
        }
      }
    }
    isTwoFinger = false; touchScrollAccum = 0;
  }

  // ── Copy / Selection ──
  async function handleCopy() {
    if (!ctrl || !ctrl.hasSelection()) return;
    const text = ctrl.getSelectionText();
    if (!text) return;
    try { await navigator.clipboard.writeText(text); copySuccess = true; setTimeout(() => copySuccess = false, 1500); } catch {}
  }

  // ── Composition ──
  function handleCompositionStart() { ctrl?.startComposition(); }
  function handleCompositionUpdate(e: CompositionEvent) { ctrl?.updateComposition(e.data); }
  function handleCompositionEnd(e: CompositionEvent) { ctrl?.endComposition(e.data); }

  // ── Keyboard ──
  function handleKeydown(e: KeyboardEvent) {
    if (ctrl?.isComposing || e.isComposing) return;
    if (!paneId || !ctrl) return;
    const specialKeys: Record<string, string> = { Enter: '\r', Escape: '\x1b', Tab: '\t', Insert: '\x1b[2~' };
    const shiftSpecial: Record<string, string> = { Tab: '\x1b[Z' };
    if (e.shiftKey && shiftSpecial[e.key]) { e.preventDefault(); onStdin(shiftSpecial[e.key]); return; }
    if (specialKeys[e.key]) { e.preventDefault(); onStdin(specialKeys[e.key]); return; }
    if (['Backspace','Delete','Home','End','PageUp','PageDown'].includes(e.key) || e.key.startsWith('F') && e.key.length >= 2) {
      e.preventDefault();
      const bytes = ctrl.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const bytes = ctrl.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      return;
    }
    if (e.key.length === 1) {
      e.preventDefault();
      const bytes = ctrl.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else onStdin(e.key);
      return;
    }
    if (e.key.startsWith('Arrow')) {
      e.preventDefault();
      const bytes = ctrl.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
      else {
        const arrows: Record<string, string> = { ArrowUp: '\x1b[A', ArrowDown: '\x1b[B', ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D' };
        if (arrows[e.key]) onStdin(arrows[e.key]);
      }
    }
  }

  function handlePaste(e: ClipboardEvent) {
    if (!paneId || !ctrl) return;
    e.preventDefault();
    const text = e.clipboardData?.getData('text') ?? '';
    if (!text) return;
    const bytes = ctrl.encodePaste(text);
    if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
  }

  $effect(() => {
    if (ready && ctrl) {
      const poll = setInterval(() => { hasSelection = ctrl?.hasSelection() ?? false; }, 300);
      return () => clearInterval(poll);
    }
  });
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

  {#if ready && hasSelection}
    <div class="selection-actions">
      <button class="copy-btn" onclick={handleCopy}>{copySuccess ? '✓ 已复制' : '复制'}</button>
      <button class="dismiss-btn" onclick={() => { ctrl?.clearSelection(); hasSelection = false; }}>✕</button>
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