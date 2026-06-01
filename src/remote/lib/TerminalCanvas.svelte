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
  // Hidden, focusable textarea: the only way to (a) raise the mobile soft
  // keyboard on tap and (b) receive IME composition events on desktop. The
  // canvas itself can't be focused, so without this Chinese input never starts.
  let hiddenInput: HTMLTextAreaElement | undefined = $state();
  let ctrl: TerminalController | null = null;
  let ready = $state(false);
  let hasSelection = $state(false);
  let copySuccess = $state(false);

  // Mouse drag-select state (desktop; only when the app isn't grabbing mouse).
  let mouseSelecting = false;

  const td = new TextDecoder();

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
    ctrl.setFocused(true);
    focusInput();
  });

  function focusInput() {
    hiddenInput?.focus({ preventScroll: true });
  }

  /** Park the hidden textarea at the cursor so the IME candidate window shows
   *  in place; falls back to the top-left when the cursor position is unknown. */
  function parkInputAtCursor() {
    if (!hiddenInput || !ctrl) return;
    const p = ctrl.getCursorPixel();
    if (!p) return;
    hiddenInput.style.left = `${Math.round(p.x)}px`;
    hiddenInput.style.top = `${Math.round(p.y)}px`;
    hiddenInput.style.height = `${Math.round(p.h)}px`;
  }

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
  export function getDims() { return ctrl?.getDims() ?? null; }
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
      // A tap focuses the hidden textarea, which raises the mobile soft keyboard.
      focusInput();
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

  // ── Composition (IME) + plain text input, both via the hidden textarea ──
  function handleCompositionStart() {
    ctrl?.startComposition();
    parkInputAtCursor();
  }
  function handleCompositionUpdate(e: CompositionEvent) {
    ctrl?.updateComposition(e.data);
  }
  function handleCompositionEnd(e: CompositionEvent) {
    ctrl?.endComposition(e.data);
    // Clear before the trailing `input` event fires so the committed text isn't
    // sent twice (compositionend already emitted it via endComposition).
    if (hiddenInput) hiddenInput.value = '';
  }

  // Fires for plain typed / predicted / autocorrected text that isn't an IME
  // composition. Printable keystrokes deliberately fall through `keydown` to
  // land here — that's what makes mobile typing and CJK work without double-input.
  function handleInput(e: Event) {
    if (!paneId || !ctrl) return;
    if (ctrl.isComposing || (e as InputEvent).isComposing) return;
    const ta = e.target as HTMLTextAreaElement;
    const text = ta.value;
    ta.value = '';
    if (text) onStdin(text);
  }

  // ── Keyboard ──
  function handleKeydown(e: KeyboardEvent) {
    if (ctrl?.isComposing || e.isComposing) return;
    if (!paneId || !ctrl) return;
    // Unmodified printable keys flow into the hidden textarea; its `input` event
    // emits them (keeps IME + mobile prediction working, avoids double-send).
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) return;
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

  // ── Mouse (desktop) ──
  function mouseButton(e: MouseEvent): number {
    return e.button === 1 ? 1 : e.button === 2 ? 2 : 0; // left=0 middle=1 right=2
  }

  function handleMouseDown(e: MouseEvent) {
    if (!paneId || !ctrl) return;
    focusInput();
    const cell = ctrl.clientToCell(e.clientX, e.clientY);
    if (!cell) return;
    if (ctrl.isMouseReporting()) {
      e.preventDefault();
      const bytes = ctrl.encodeMouse(cell.row, cell.col, mouseButton(e), 0, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else if (e.button === 0) {
      e.preventDefault();
      mouseSelecting = true;
      ctrl.startSelection(cell.row, cell.col);
      hasSelection = ctrl.hasSelection();
    }
  }

  function handleMouseMove(e: MouseEvent) {
    if (!ctrl) return;
    if (mouseSelecting) {
      const cell = ctrl.clientToCell(e.clientX, e.clientY);
      if (cell) { ctrl.extendSelection(cell.row, cell.col); hasSelection = ctrl.hasSelection(); }
      return;
    }
    // Drag with a button held while the app captures the mouse → motion report.
    if (e.buttons !== 0 && ctrl.isMouseReporting()) {
      const cell = ctrl.clientToCell(e.clientX, e.clientY);
      if (!cell) return;
      const btn = (e.buttons & 1) ? 0 : (e.buttons & 4) ? 1 : (e.buttons & 2) ? 2 : 0;
      const bytes = ctrl.encodeMouse(cell.row, cell.col, btn, 2, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    }
  }

  function handleMouseUp(e: MouseEvent) {
    if (!ctrl) return;
    if (ctrl.isMouseReporting()) {
      const cell = ctrl.clientToCell(e.clientX, e.clientY);
      if (!cell) return;
      e.preventDefault();
      const bytes = ctrl.encodeMouse(cell.row, cell.col, mouseButton(e), 1, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else if (mouseSelecting) {
      mouseSelecting = false;
      ctrl.endSelection();
      hasSelection = ctrl.hasSelection();
    }
  }

  function handleWheel(e: WheelEvent) {
    if (!ctrl) return;
    if (ctrl.isMouseReporting()) {
      const cell = ctrl.clientToCell(e.clientX, e.clientY) ?? { row: 0, col: 0 };
      e.preventDefault();
      const btn = e.deltaY < 0 ? 64 : 65; // wheel up / down
      const bytes = ctrl.encodeMouse(cell.row, cell.col, btn, 0, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else {
      e.preventDefault();
      const lines = e.deltaY > 0 ? 3 : -3;
      if (lines < 0) ctrl.scrollUp(-lines); else ctrl.scrollDown(lines);
    }
  }

  function handleContextMenu(e: MouseEvent) {
    // Hand right-click to mouse-capturing apps; otherwise leave the native menu.
    if (ctrl?.isMouseReporting()) e.preventDefault();
  }

  $effect(() => {
    if (ready && ctrl) {
      const poll = setInterval(() => { hasSelection = ctrl?.hasSelection() ?? false; }, 300);
      return () => clearInterval(poll);
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="container" bind:this={containerEl} role="application"
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
  onmousedown={handleMouseDown}
  onmousemove={handleMouseMove}
  onmouseup={handleMouseUp}
  onwheel={handleWheel}
  oncontextmenu={handleContextMenu}
>
  {#if !ready}
    <div class="loading">初始化终端引擎…</div>
  {/if}
  <canvas bind:this={canvasEl} class="term-canvas" class:hidden={!ready}></canvas>

  <!-- Hidden, focusable input sink: raises the mobile keyboard on tap and
       receives IME composition. pointer-events:none so it never steals canvas
       clicks; it is focused programmatically. -->
  <textarea
    bind:this={hiddenInput}
    class="hidden-input"
    autocapitalize="off"
    autocomplete="off"
    autocorrect="off"
    spellcheck="false"
    aria-hidden="true"
    tabindex="-1"
    onkeydown={handleKeydown}
    oninput={handleInput}
    oncompositionstart={handleCompositionStart}
    oncompositionupdate={handleCompositionUpdate}
    oncompositionend={handleCompositionEnd}
    onpaste={handlePaste}
    onfocus={() => ctrl?.setFocused(true)}
    onblur={() => ctrl?.setFocused(false)}
  ></textarea>

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
  /* Invisible input sink parked at the cursor. pointer-events:none keeps it from
     stealing canvas clicks; opacity/transparent colors keep it unseen. */
  .hidden-input{position:absolute;top:0;left:0;width:2px;height:1em;margin:0;padding:0;border:0;
    opacity:0;pointer-events:none;resize:none;overflow:hidden;white-space:nowrap;z-index:5;
    background:transparent;color:transparent;caret-color:transparent;outline:none;font:inherit}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:#8b949e;font-size:14px}
  .selection-actions{position:absolute;bottom:12px;right:12px;display:flex;gap:6px;z-index:10}
  .copy-btn,.dismiss-btn{display:flex;align-items:center;justify-content:center;height:36px;padding:0 14px;border:none;border-radius:8px;font-size:13px;font-weight:500;cursor:pointer;transition:all .12s;touch-action:manipulation}
  .copy-btn{background:#238636;color:#fff}
  .copy-btn:active{background:#2ea043}
  .dismiss-btn{background:#21262d;color:#8b949e;min-width:36px}
  .dismiss-btn:active{background:#30363d}
</style>