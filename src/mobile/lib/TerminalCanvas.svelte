<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import init, { TerminalKernel, RenderHandle } from '@ridge/term-wasm';
  import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';
  import { peekMods, consumeMods, clearMods } from './modState.svelte';
  let { paneId, onStdin, onResize, onRefresh, shiftY = 0 }: {
    paneId: string | null;
    onStdin: (data: string) => void;
    // §multi-size: called whenever this client's viewport grid changes → resize
    // ONLY this client's per-sub parser (never the shared PTY). Keeps the client
    // kernel and the server-side sub parser in lock-step so input + mouse map to
    // the right cells.
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    onRefresh?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    // §3: shift the rendered canvas up (CSS px) so the cursor row clears the
    // soft keyboard. The canvas is NOT resized — only translated.
    shiftY?: number;
  } = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let containerEl: HTMLDivElement | undefined = $state();
  // §IME: hidden, focused <textarea> that captures soft-keyboard input.
  // Mobile keyboards emit composition + beforeinput (insertText /
  // deleteContentBackward) rather than reliable keydown, so a real editable
  // element is required for IME candidates, predictive words, and even plain
  // space to work. See handleBeforeInput / composition handlers below.
  let imeInput: HTMLTextAreaElement | undefined = $state();

  let kernel: TerminalKernel | null = null;
  let renderHandle: RenderHandle | null = null;
  let rafId: number | null = null;
  let ready = $state(false);

  const pendingData: Uint8Array[] = [];
  let fitPending = false;
  // §multi-size: the last viewport grid we reported to the server (debounces the
  // onResize calls). Reset on every pane switch.
  let sentRows = 0;
  let sentCols = 0;
  // §5: the pane this kernel currently mirrors. Drives the reset-on-switch
  // effect and lets us drop stray deltas addressed to a different pane.
  let currentPaneId: string | null = null;
  let isComposing = false;
  let compositionStdinTarget: string | null = null;

  let fontSize = $state(12);
  let cols = $state(80);
  let rows = $state(24);
  let cellW = $state(8);
  let cellH = $state(16);

  let copySuccess = $state(false);
  // §reactivity: mirror kernel.hasSelection() into reactive state (updated in the
  // render loop) so the copy/dismiss buttons toggle without making `kernel` a
  // reactive proxy.
  let selectionActive = $state(false);

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
    currentPaneId = paneId;
    renderHandle = await RenderHandle.newWithWebgpuFirst(canvasEl!);
    renderHandle.applyDefaultTheme();
    fitPane();
    flushPending();
    ready = true;
    // `autocorrect` is a WebKit-only attribute (not in the standard textarea
    // prop types) — set it via the DOM to suppress iOS autocorrect, which is
    // what made plain space delete the previous char.
    imeInput?.setAttribute('autocorrect', 'off');
    focusInput();
    function frame() {
      if (kernel && renderHandle) renderHandle.render(kernel);
      // Drive the copy/dismiss UI reactively without making `kernel` itself a
      // reactive ($state) proxy — wrapping the wasm object would break it.
      const sel = kernel?.hasSelection() ?? false;
      if (sel !== selectionActive) selectionActive = sel;
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
    // §TUI-mouse: desktop-browser mouse handling. Attached imperatively so the
    // wheel listener can be non-passive (it preventDefaults to forward scroll
    // to the TUI). Touch is handled by the on:touch* template handlers.
    const el = containerEl;
    el?.addEventListener('pointerdown', handlePointerDown);
    el?.addEventListener('pointermove', handlePointerMove);
    el?.addEventListener('pointerup', handlePointerUp);
    el?.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      if (fitDebounceTimer) clearTimeout(fitDebounceTimer);
      ro?.disconnect();
      el?.removeEventListener('pointerdown', handlePointerDown);
      el?.removeEventListener('pointermove', handlePointerMove);
      el?.removeEventListener('pointerup', handlePointerUp);
      el?.removeEventListener('wheel', handleWheel);
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
      // §multi-size: report our new grid so the server reflows our per-sub
      // parser (NOT the shared PTY). The explicit refresh button (onRefresh) is
      // what resizes the real PTY for full-screen TUIs.
      notifyViewport();
    }
  }

  // §multi-size: tell the server this client's current viewport grid so it
  // resizes ONLY our per-sub parser (never the shared PTY). Debounced via the
  // last-sent size. Called from fitPane on any grid change.
  function notifyViewport() {
    if (!paneId || !onResize || !containerEl) return;
    if (rows === sentRows && cols === sentCols) return;
    sentRows = rows;
    sentCols = cols;
    onResize(
      paneId,
      rows,
      cols,
      Math.round(containerEl.clientWidth),
      Math.round(containerEl.clientHeight),
    );
  }

  // §5: drop and rebuild the kernel when the pane changes so a workspace/pane
  // switch never shows the previous pane's grid or scrollback. The server
  // re-bootstraps the new pane with a full frame on subscribe (see MainApp
  // $effect → ws.subscribePane), which arrives async — after this synchronous
  // reset — so we always start clean.
  function resetKernel() {
    try { kernel?.free(); } catch { /* already freed */ }
    kernel = new TerminalKernel(rows, cols, 5000);
    pendingData.length = 0;
    flushPending();
    // Reset per-pane interaction / input / gesture state.
    sentRows = 0;
    sentCols = 0;
    isComposing = false;
    compositionStdinTarget = null;
    clearMods();
    isSelecting = false;
    didLongPress = false;
    isTwoFinger = false;
    touchMousePressed = false;
    lastMouseCell = null;
    touchScrollAccum = 0;
    if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
    mouseSelecting = false;
    mouseSelAnchor = null;
    mouseLast = null;
  }

  // §3: cursor Y position in CSS pixels, for MainApp's translateY keyboard math.
  export function getCursorY(): number {
    if (!kernel) return 0;
    const dpr = window.devicePixelRatio || 1;
    const row = kernel.cursorRow?.() ?? 0;
    return (row * cellH) / dpr;
  }

  $effect(() => {
    const id = paneId;
    if (!ready) return;
    if (id === currentPaneId) return;
    currentPaneId = id;
    resetKernel();
  });

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

  // ─── Quick-key bar integration (exported for the always-visible bar) ──
  export function sendKey(key: string, ctrl: boolean, alt: boolean, shift: boolean) {
    if (!paneId || !kernel) return;
    // §2: a quick-key tap must NOT open or close the soft keyboard — so no
    // focusInput() here.
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

  // ─── Pointer/touch input: TUI mouse forwarding + local select/scroll ──
  let touchStartY = 0;
  let touchStartX = 0;
  let touchScrollAccum = 0;
  let lastTwoFingerY = 0;
  let isTwoFinger = false;
  let touchStartTime = 0;
  let isSelecting = false;
  let selAnchorRow = 0;
  let selAnchorCol = 0;
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  let didLongPress = false;
  // §7: true once a single-finger swipe has produced a scroll, so touchend does
  // not mistake a quick flick for a tap.
  let didScroll = false;
  // Retained (reset by resetKernel); no longer used for touch drag-select.
  let touchMousePressed = false;
  let lastMouseCell: { row: number; col: number } | null = null;

  function sendBytes(bytes: Uint8Array) {
    if (bytes.length > 0) onStdin(new TextDecoder().decode(bytes));
  }

  /** Viewport row → absolute (scrollback-aware) row, for local selection. */
  function absRowOf(row: number): number {
    if (!kernel) return row;
    const off = kernel.scrollbackLen() > 0 && kernel.scrollOffset() > 0 ? kernel.scrollOffset() : 0;
    return row + off;
  }

  function touchToCell(clientX: number, clientY: number): { row: number; col: number } | null {
    if (!canvasEl || cellW <= 0 || cellH <= 0) return null;
    const rect = canvasEl.getBoundingClientRect();
    const col = Math.max(0, Math.floor((clientX - rect.left) / cellW));
    const row = Math.max(0, Math.floor((clientY - rect.top) / cellH));
    return { row, col };
  }

  function handleTouchStart(e: TouchEvent) {
    if (e.touches.length === 2) {
      isTwoFinger = true;
      lastTwoFingerY = (e.touches[0].clientY + e.touches[1].clientY) / 2;
      if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
      e.preventDefault();
      return;
    }
    if (e.touches.length !== 1) return;
    const touch = e.touches[0];
    touchStartY = touch.clientY;
    touchStartX = touch.clientX;
    touchScrollAccum = 0;
    touchStartTime = Date.now();
    isSelecting = false;
    didLongPress = false;
    didScroll = false;
    touchMousePressed = false;
    lastMouseCell = null;

    if (longPressTimer) clearTimeout(longPressTimer);
    longPressTimer = setTimeout(() => {
      if (!kernel || isTwoFinger) return;
      const cell = touchToCell(touch.clientX, touch.clientY);
      if (!cell) return;
      didLongPress = true;
      // §7: long-press ALWAYS enters local text-selection mode (regardless of
      // TUI mouse reporting). A plain swipe is reserved for scrolling.
      isSelecting = true;
      selAnchorRow = cell.row;
      selAnchorCol = cell.col;
      kernel.clearSelection();
      try { navigator.vibrate(15); } catch {}
    }, 400);
  }

  function handleTouchMove(e: TouchEvent) {
    // Two-finger vertical pan → scroll (wheel to TUI when reporting, else local).
    if (e.touches.length === 2 && isTwoFinger) {
      if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
      const y = (e.touches[0].clientY + e.touches[1].clientY) / 2;
      const dy = lastTwoFingerY - y;
      if (Math.abs(dy) > 6 && kernel) {
        const lines = Math.trunc(dy / 12);
        if (lines !== 0) {
          if (kernel.isMouseReporting()) {
            const cell = touchToCell(e.touches[0].clientX, e.touches[0].clientY) ?? { row: 0, col: 0 };
            const btn = lines < 0 ? 64 : 65; // wheel up : down
            const n = Math.min(Math.abs(lines), 5);
            for (let i = 0; i < n; i++) sendBytes(kernel.encodeMouse(cell.row, cell.col, btn, 0, false, false, false));
          } else if (lines < 0) {
            kernel.scrollUp(-lines);
          } else {
            kernel.scrollDown(lines);
          }
          lastTwoFingerY = y;
        }
      }
      e.preventDefault();
      return;
    }
    if (e.touches.length === 1 && isTwoFinger) return;
    if (e.touches.length !== 1) return;
    const touch = e.touches[0];

    // §7: long-press selection mode → extend the selection as the finger moves.
    if (isSelecting && kernel) {
      if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
      e.preventDefault();
      const cell = touchToCell(touch.clientX, touch.clientY);
      if (cell) kernel.setSelectionAbs(selAnchorRow, selAnchorCol, absRowOf(cell.row), cell.col);
      return;
    }
    if (didLongPress) return;

    // §7: a single-finger swipe is ALWAYS a scroll (the default gesture). When
    // the TUI has mouse reporting on, forward wheel events (SGR 64/65) so the
    // app scrolls; otherwise scroll the local scrollback. Text selection is only
    // entered via long-press (above) — never via a drag.
    const dy = touch.clientY - touchStartY;
    if (Math.abs(dy) > 10 && longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }
    touchScrollAccum += dy;
    if (Math.abs(touchScrollAccum) > 24 && kernel) {
      // dy > 0 (finger moved down) → reveal older content → scroll up / wheel-up.
      const lines = Math.trunc(touchScrollAccum / 24);
      if (lines !== 0) {
        didScroll = true;
        if (kernel.isMouseReporting()) {
          const cell = touchToCell(touch.clientX, touch.clientY) ?? { row: 0, col: 0 };
          const btn = lines > 0 ? 64 : 65; // wheel up : wheel down
          const n = Math.min(Math.abs(lines), 5);
          for (let i = 0; i < n; i++) sendBytes(kernel.encodeMouse(cell.row, cell.col, btn, 0, false, false, false));
        } else if (lines > 0) {
          kernel.scrollUp(lines);
        } else {
          kernel.scrollDown(-lines);
        }
        touchScrollAccum = 0;
      }
    }
    e.preventDefault();
    touchStartY = touch.clientY;
  }

  function handleTouchEnd(e: TouchEvent) {
    if (longPressTimer) { clearTimeout(longPressTimer); longPressTimer = null; }

    if (isTwoFinger) { isTwoFinger = false; touchScrollAccum = 0; isSelecting = false; didScroll = false; return; }
    if (isSelecting) { isSelecting = false; touchScrollAccum = 0; didScroll = false; return; }
    if (didLongPress) { didLongPress = false; touchScrollAccum = 0; didScroll = false; return; }

    // §7 quick tap (not a swipe): focus input (raises soft keyboard) +
    // left-click when the TUI has mouse reporting on.
    const elapsed = Date.now() - touchStartTime;
    if (elapsed < 300 && !didScroll) {
      focusInput();
      if (kernel && cellW > 0 && cellH > 0 && kernel.isMouseReporting()) {
        const touch = e.changedTouches[0];
        const cell = touch ? touchToCell(touch.clientX, touch.clientY) : null;
        if (cell) {
          sendBytes(kernel.encodeMouse(cell.row, cell.col, 0, 0, false, false, false));
          requestAnimationFrame(() => {
            if (kernel) sendBytes(kernel.encodeMouse(cell.row, cell.col, 3, 1, false, false, false));
          });
        }
      }
    }

    isTwoFinger = false;
    touchScrollAccum = 0;
    didScroll = false;
  }

  // ─── Mouse (desktop browser remote): mirror the ridge desktop contract ─
  // When the TUI has mouse reporting on, ALL buttons forward to it (SGR);
  // otherwise the drag does host text selection. rAF-batched + deduped like
  // src/lib/terminal/manager.ts.
  let mouseSelecting = false;
  let mouseSelAnchor: { row: number; col: number } | null = null;
  let mouseLast: { row: number; col: number; buttons: number; action: number } | null = null;
  let mousePending: PointerEvent | null = null;
  let mouseRaf: number | null = null;

  function handlePointerDown(e: PointerEvent) {
    if (e.pointerType !== 'mouse' || !kernel) return;
    // §1b: focus on pointerdown (so typing works) — NOT on a trailing click,
    // which used to steal focus right after a drag-selection finished.
    focusInput();
    mouseLast = null;
    const cell = touchToCell(e.clientX, e.clientY);
    if (!cell) return;
    if (kernel.mouseReportingModes() !== 0) {
      sendBytes(kernel.encodeMouse(cell.row, cell.col, e.button, 0, e.shiftKey, e.ctrlKey, e.altKey));
      mouseLast = { row: cell.row, col: cell.col, buttons: e.buttons, action: 0 };
      try { (e.target as Element).setPointerCapture?.(e.pointerId); } catch {}
      e.preventDefault();
    } else {
      mouseSelecting = true;
      mouseSelAnchor = { row: absRowOf(cell.row), col: cell.col };
      kernel.clearSelection();
      try { (e.target as Element).setPointerCapture?.(e.pointerId); } catch {}
    }
  }

  function flushPointerMove() {
    mouseRaf = null;
    const e = mousePending;
    mousePending = null;
    if (!e || !kernel) return;
    const cell = touchToCell(e.clientX, e.clientY);
    if (!cell) return;
    const modes = kernel.mouseReportingModes();
    if (modes !== 0) {
      const motion = (modes & 0x2) !== 0 || (modes & 0x4) !== 0; // ?1002 | ?1003
      if (!motion) return;
      if ((modes & 0x4) === 0 && e.buttons === 0) return; // ?1002 only while a button is held
      const btn = e.buttons & 1 ? 0 : e.buttons & 2 ? 2 : e.buttons & 4 ? 1 : 0;
      const l = mouseLast;
      if (!l || l.row !== cell.row || l.col !== cell.col || l.buttons !== e.buttons || l.action !== 2) {
        sendBytes(kernel.encodeMouse(cell.row, cell.col, btn, 2, e.shiftKey, e.ctrlKey, e.altKey));
        mouseLast = { row: cell.row, col: cell.col, buttons: e.buttons, action: 2 };
      }
    } else if (mouseSelecting && mouseSelAnchor) {
      kernel.setSelectionAbs(mouseSelAnchor.row, mouseSelAnchor.col, absRowOf(cell.row), cell.col);
    }
  }

  function handlePointerMove(e: PointerEvent) {
    if (e.pointerType !== 'mouse' || !kernel) return;
    mousePending = e;
    if (mouseRaf == null) mouseRaf = requestAnimationFrame(flushPointerMove);
  }

  function handlePointerUp(e: PointerEvent) {
    if (e.pointerType !== 'mouse' || !kernel) return;
    if (kernel.mouseReportingModes() !== 0) {
      const cell = touchToCell(e.clientX, e.clientY);
      if (cell) sendBytes(kernel.encodeMouse(cell.row, cell.col, 3, 1, e.shiftKey, e.ctrlKey, e.altKey));
    }
    mouseSelecting = false;
    mouseLast = null;
    try { (e.target as Element).releasePointerCapture?.(e.pointerId); } catch {}
  }

  function handleWheel(e: WheelEvent) {
    if (!kernel || e.deltaY === 0) return;
    if (kernel.mouseReportingModes() !== 0) {
      const cell = touchToCell(e.clientX, e.clientY);
      if (!cell) return;
      const btn = e.deltaY < 0 ? 64 : 65;
      sendBytes(kernel.encodeMouse(cell.row, cell.col, btn, 0, e.shiftKey, e.ctrlKey, e.altKey));
      e.preventDefault();
    } else {
      if (e.deltaY < 0) kernel.scrollUp(3); else kernel.scrollDown(3);
      e.preventDefault();
    }
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

  // ─── Hidden IME textarea: focus + cursor anchoring ─────────────────
  // Exported so the quick-key bar's modifier taps (VirtualKeyboard onSummon)
  // can raise the soft keyboard. §2/§3.
  export function focusInput() {
    try { imeInput?.focus({ preventScroll: true }); } catch { imeInput?.focus(); }
  }

  /** Park the hidden textarea over the cursor cell so the OS IME candidate
   *  window pops up next to the caret instead of at the page origin. */
  function repositionInput() {
    if (!imeInput || !kernel) return;
    const r = kernel.cursorRow?.() ?? 0;
    const c = kernel.cursorCol?.() ?? 0;
    if (cellW > 0 && cellH > 0) {
      imeInput.style.left = `${Math.max(0, c) * cellW}px`;
      imeInput.style.top = `${Math.max(0, r) * cellH}px`;
    }
  }

  function handleCompositionStart(_e: CompositionEvent) {
    isComposing = true;
    compositionStdinTarget = null;
    repositionInput();
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
    if (imeInput) imeInput.value = '';
    if (!paneId || !kernel || !renderHandle) return;
    const h = renderHandle as unknown as { clearPreedit?: () => void };
    h.clearPreedit?.();
    // Commit the chosen text RAW (NOT bracketed-paste): IME words / candidates
    // are normal typed input, so the shell/TUI must receive the bytes as-is.
    const text = e.data;
    if (text) onStdin(text);
  }

  // ─── beforeinput: the reliable text path for soft keyboards ─────────
  // Plain typing, space, predictive words and backspace on mobile arrive here
  // as InputEvents rather than keydown. Composition is owned by the
  // composition handlers above, so we ignore composition/paste input types to
  // avoid double-emitting.
  function handleBeforeInput(e: InputEvent) {
    if (!paneId || !kernel) return;
    const t = e.inputType;
    if (t === 'insertCompositionText') return; // handled by compositionend
    if (isComposing || e.isComposing) return;
    switch (t) {
      case 'insertText': {
        const d = e.data ?? '';
        if (d) {
          // §2: if a sticky modifier is armed (tapped Ctrl/Alt/Shift on the
          // quick-key bar), encode this soft-keyboard char as a chord (e.g.
          // Ctrl+C) then clear the modifiers. Otherwise send the raw text.
          const m = peekMods();
          if (m.ctrl || m.alt || m.shift) {
            const bytes = kernel.encodeKey(d, m.ctrl, m.alt, m.shift, false);
            consumeMods();
            onStdin(bytes.length > 0 ? new TextDecoder().decode(bytes) : d);
          } else {
            onStdin(d);
          }
        }
        e.preventDefault();
        break;
      }
      case 'insertLineBreak':
      case 'insertParagraph':
        onStdin('\r');
        e.preventDefault();
        break;
      case 'deleteContentBackward':
        onStdin('\x7f');
        e.preventDefault();
        break;
      case 'deleteContentForward':
        onStdin('\x1b[3~');
        e.preventDefault();
        break;
      case 'deleteWordBackward':
        onStdin('\x17'); // Ctrl+W
        e.preventDefault();
        break;
      case 'insertFromPaste':
        // Let the dedicated `paste` handler wrap it (bracketed paste).
        break;
      default:
        if (e.data) {
          onStdin(e.data);
          e.preventDefault();
        }
    }
  }

  // ─── Keyboard handling ────────────────────────────────────────────
  function handleKeydown(e: KeyboardEvent) {
    if (isComposing || e.isComposing) return;
    if (!paneId || !kernel) return;
    // §2: a sticky modifier armed via the quick-key bar combines with this key
    // (hardware keyboards / special soft-keyboard keys). Skip bare modifier keys
    // so arming Ctrl then pressing it again doesn't clear prematurely.
    if (!['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) {
      const m = peekMods();
      if (m.ctrl || m.alt || m.shift) {
        const bytes = kernel.encodeKey(
          e.key, e.ctrlKey || m.ctrl, e.altKey || m.alt, e.shiftKey || m.shift, e.metaKey,
        );
        if (bytes.length > 0) {
          consumeMods();
          e.preventDefault();
          onStdin(new TextDecoder().decode(bytes));
          return;
        }
      }
    }
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
    // Modified single chars (Ctrl/Alt/Cmd + key) are control sequences, not
    // text — encode them here. Plain printable keys (incl. space) are NOT
    // handled in keydown: they flow through `beforeinput` (insertText) so that
    // soft keyboards, predictive text, and IME all work and never double-emit.
    if ((e.ctrlKey || e.metaKey || e.altKey) && e.key.length === 1) {
      const bytes = kernel.encodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) {
        e.preventDefault();
        onStdin(new TextDecoder().decode(bytes));
      }
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

<!-- §1b: no container onclick={focusInput} — it stole focus on the trailing
     click after a mouse drag-selection. Focus is acquired in handlePointerDown
     (mouse) and the handleTouchEnd tap branch instead. -->
<div class="container" bind:this={containerEl} role="application"
  ontouchstart={handleTouchStart}
  ontouchmove={handleTouchMove}
  ontouchend={handleTouchEnd}
>
  {#if !ready}
    <div class="loading">初始化终端引擎…</div>
  {/if}
  <canvas
    bind:this={canvasEl}
    class="term-canvas"
    class:hidden={!ready}
    style="transform: translateY({-shiftY}px)"
  ></canvas>

  <!-- §IME: hidden, focused editable that captures soft-keyboard input,
       composition, predictive words and paste. Parked over the caret so the
       OS candidate window appears next to the cursor. -->
  <textarea
    bind:this={imeInput}
    class="ime-input"
    aria-label="终端输入"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    rows="1"
    onkeydown={handleKeydown}
    onbeforeinput={handleBeforeInput}
    oncompositionstart={handleCompositionStart}
    oncompositionupdate={handleCompositionUpdate}
    oncompositionend={handleCompositionEnd}
    onpaste={handlePaste}
  ></textarea>

  {#if ready && selectionActive}
    <div class="selection-actions">
      <button class="copy-btn" onclick={handleCopy}>
        {copySuccess ? '✓ 已复制' : '复制'}
      </button>
      <button class="dismiss-btn" onclick={handleDismissSelection}>✕</button>
    </div>
  {/if}
</div>

<style>
  .container{position:relative;flex:1;overflow:hidden;background:#0d1117;touch-action:manipulation}
  .term-canvas{display:block;width:100%;height:100%;touch-action:none}
  .term-canvas.hidden{opacity:0}

  /* §IME: invisible, focusable editable parked over the caret. width:1px +
     transparent text/caret keep it out of sight; we render the real cursor.
     pointer-events:none so taps reach the canvas (focus is set programmatically
     from the tap handler so the soft keyboard still appears). */
  .ime-input{position:absolute;top:0;left:0;width:1px;height:1.2em;padding:0;margin:0;border:0;outline:none;background:transparent;color:transparent;caret-color:transparent;opacity:0;z-index:5;resize:none;overflow:hidden;white-space:nowrap;pointer-events:none}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:#8b949e;font-size:14px}

  .selection-actions{position:absolute;bottom:12px;right:12px;display:flex;gap:6px;z-index:10}
  .copy-btn,.dismiss-btn{display:flex;align-items:center;justify-content:center;height:36px;padding:0 14px;border:none;border-radius:8px;font-size:13px;font-weight:500;cursor:pointer;transition:all .12s;touch-action:manipulation}
  .copy-btn{background:#238636;color:#fff}
  .copy-btn:active{background:#2ea043}
  .dismiss-btn{background:#21262d;color:#8b949e;min-width:36px}
  .dismiss-btn:active{background:#30363d}
</style>
