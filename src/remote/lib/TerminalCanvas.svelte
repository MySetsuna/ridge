<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { TerminalManager } from '@ridge/remote/shared/terminal/manager';
  import { paneRefKey } from '@ridge/remote';
  import { anyMod, consumeMods } from './modState.svelte';
  import { resolveInputAnchor, terminalVisualShiftPx } from './keyboardOffset';
  import { writeClipboard } from './clipboard';
  import { copySelectionOnly } from '@ridge/remote/shared/terminal/mobileCopy';
  import {
    decideTouchMouseGesture,
    decideTouchScroll,
  } from '@ridge/remote/shared/terminal/mobileTouchScroll';
  import {
    imeCommitDelta,
    updatePendingWord,
    pendingWordBackspace,
    trailingWord,
  } from '@ridge/remote/shared/terminal/imeDelta';
  import { SentenceBuffer, SENTENCE_FLUSH_MS } from '@ridge/remote/shared/terminal/sentenceBuffer';

  // P4 (2026-07-25): this component no longer owns a single `TerminalController`
  // + canvas. It is now the MOBILE INPUT-ADAPTATION LAYER over the SHARED
  // multi-kernel `TerminalManager` (same manager the desktop `RidgePane` uses).
  // It owns ONE pane (paneId, fixed for the component's lifetime — MainApp keys
  // the component on activePaneId), attaches that pane's keep-alive kernel into
  // its container, and PARKS it on unmount (kernel survives → zero white-screen
  // + scrollback preserved across pane switches). All touch / soft-keyboard /
  // IME / selection-as-mouse / copy-pill logic is retargeted from `ctrl.*` to
  // `manager.*(paneId)` / `manager.getKernel(paneId)?.*`.
  let { paneId: remotePaneId, workspaceId, onStdin, onResize, onHostClipboard, onNearTop, onKeyboardShift, scrollbackLoading = false, selectionMode = $bindable(false), backendName = $bindable('Canvas2D'), sentenceBuffer = false }: {
    paneId: string;
    workspaceId: string;
    onStdin: (data: string) => void;
    /** iter-60：句级输入缓冲开关（语音/高频改写场景；alt-screen/TUI 鼠标态自动旁路）。 */
    sentenceBuffer?: boolean;
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    /** Mirror a copied selection onto the desktop host's clipboard (so the host's
     *  native Ctrl+V paste picks it up). The control end's copy writes BOTH. */
    onHostClipboard?: (text: string) => void;
    /** §history-pull: fired when the user scrolls the viewport near the top of the
     *  in-kernel scrollback, so MainApp can lazily fetch + prepend older history. */
    onNearTop?: () => void;
    onKeyboardShift?: (shift: number) => void;
    scrollbackLoading?: boolean;
    selectionMode?: boolean;
    backendName?: string;
  } = $props();

  const paneId = $derived(paneRefKey({ workspaceId, paneId: remotePaneId }));
  const manager = TerminalManager.instance();
  const td = new TextDecoder();

  /** Scroll-up rows-from-top threshold that triggers a lazy older-history fetch.
   *  ~1.5 screens of headroom so the fetch lands before the user hits the very top. */
  const NEAR_TOP_ROWS = 24;

  /** Fire onNearTop when the viewport is within NEAR_TOP_ROWS of the buffer top. */
  function maybeLoadOlder() {
    if (!attached || !onNearTop) return;
    if (rowsAboveViewport() <= NEAR_TOP_ROWS) onNearTop();
  }

  let containerEl: HTMLDivElement | undefined = $state();
  // Hidden, focusable textarea: the only way to (a) raise the mobile soft
  // keyboard on tap and (b) receive IME composition events on desktop.
  let hiddenInput: HTMLTextAreaElement | undefined = $state();
  // true once this pane's kernel is attached (or unparked) into the container.
  let attached = $state(false);
  // Lifetime flag: false the moment the component starts tearing down, so the
  // async attach IIFE bails and (if attach already landed) parks the pane.
  let alive = true;
  // Local IME composition guard (was `ctrl.isComposing`).
  let isComposing = false;

  // Mouse drag-select state (desktop; only when the app isn't grabbing mouse).
  let mouseSelecting = false;

  // ── Local selection state (mobile bypasses the manager's built-in pointer
  //    selection path — see the capture-phase neutralizer below — and drives
  //    the kernel selection directly, mirroring the retired TerminalController). ──
  let selAnchorRow = 0;
  let selAnchorCol = 0;
  let isSelectingLocal = false;

  // Touch state. Single-finger swipe = scroll; tap = focus (+ click-through).
  let touchStartY = 0;
  let touchStartX = 0;
  let touchScrollAccum = 0;
  let touchLastY = 0;
  let touchStartTime = 0;
  const TOUCH_DRAG_THRESHOLD_PX = 8;
  const TOUCH_TAP_MAX_MS = 250;

  let hasSelectionState = $state(false);    // drives the floating copy pill
  let selDragging = false;                  // selection drag in progress

  // ── kernel-level helpers (retarget of the old TerminalController surface) ──

  /** kernel signature is encodeMouse(row, col, button, action, shift, ctrl, alt)
   *  — note ctrl BEFORE alt. Call sites keep the old (…, shift, alt, ctrl) order;
   *  this wrapper forwards in the kernel's order so modifiers aren't swapped. */
  function kEncodeMouse(row: number, col: number, button: number, action: number, shift: boolean, alt: boolean, ctrlMod: boolean): Uint8Array {
    return manager.getKernel(paneId)?.encodeMouse(row, col, button, action, shift, ctrlMod, alt) ?? new Uint8Array(0);
  }
  function kEncodeKey(key: string, ctrlMod: boolean, alt: boolean, shift: boolean, meta = false): Uint8Array {
    return manager.getKernel(paneId)?.encodeKey(key, ctrlMod, alt, shift, meta) ?? new Uint8Array(0);
  }
  function kEncodePaste(text: string): Uint8Array {
    return manager.getKernel(paneId)?.encodePaste(text) ?? new Uint8Array(0);
  }
  function isMouseReporting(): boolean { return manager.isMouseReporting(paneId); }
  function isAltScreen(): boolean { return manager.isAltScreen(paneId); }
  function clientToCell(clientX: number, clientY: number) {
    return manager.cellFromEvent(paneId, { clientX, clientY });
  }
  /** How many scrollback rows sit ABOVE the current viewport top. offset 0 = at
   *  bottom; rowsAboveViewport = max(0, total - offset). Mirrors the retired
   *  TerminalController.rowsAboveViewport used for lazy older-history prefetch. */
  function rowsAboveViewport(): number {
    const { offset, total } = manager.scrollState(paneId);
    return Math.max(0, total - offset);
  }
  function hasSelection(): boolean { return manager.getKernel(paneId)?.hasSelection() ?? false; }

  function startSelection(row: number, col: number) {
    // 绝对行 = viewport-relative row + 视口上方的 scrollback 行数（= total - offset）。
    selAnchorRow = rowsAboveViewport() + row;
    selAnchorCol = col;
    isSelectingLocal = true;
    manager.clearSelection(paneId);
  }
  function extendSelection(row: number, col: number) {
    if (!isSelectingLocal) return;
    const absRow = rowsAboveViewport() + row;
    manager.getKernel(paneId)?.setSelectionAbs(selAnchorRow, selAnchorCol, absRow, col);
    // setSelectionAbs on the raw kernel doesn't wake the manager's rAF loop;
    // forceFullRedraw invalidates + wakes so the highlight repaints immediately.
    manager.forceFullRedraw(paneId);
  }
  function endSelectionLocal() { isSelectingLocal = false; }

  /** Input-cell pixel (TUI-aware anchor), used to park the hidden IME textarea
   *  and to anchor the keyboard offset. Mirrors the old ctrl.getCursorPixel. */
  function getCursorPixel(): { x: number; y: number; h: number } | null {
    const a = manager.inputAnchorResolved(paneId);
    return a ? { x: a.x, y: a.y, h: a.cellH } : null;
  }

  onMount(() => {
    // `autocorrect` is a non-standard (iOS Safari) attribute missing from
    // Svelte's textarea typings — set it via the DOM to keep iOS from rewriting
    // terminal input without tripping svelte-check.
    hiddenInput?.setAttribute('autocorrect', 'off');

    // §pointer-neutralize (P4): the shared manager attaches its OWN
    // pointerdown/move/up listeners to the container for DESKTOP mouse selection
    // + TUI mouse forwarding. On mobile, touch input synthesises pointer events,
    // so those listeners would DOUBLE every gesture this component already
    // handles via its touch/mouse handlers (stray selection while scrolling,
    // duplicate mouse reports in TUIs). Capture-phase listeners fire before the
    // manager's bubble-phase ones; stopPropagation neutralises the manager's
    // pointer path without touching this component's touch/mouse handlers
    // (separate event types).
    const stopPointer = (e: PointerEvent) => { e.stopPropagation(); };
    const el = containerEl;
    const onContextLost = (e: Event) => { e.preventDefault(); };
    const onContextRestored = () => {
      if (!alive || !attached) return;
      manager.forceFullRedraw(paneId);
      manager.fitPaneNow(paneId);
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') onContextRestored();
    };
    if (el) {
      el.addEventListener('pointerdown', stopPointer, true);
      el.addEventListener('pointermove', stopPointer, true);
      el.addEventListener('pointerup', stopPointer, true);
      el.addEventListener('pointercancel', stopPointer, true);
      el.addEventListener('webglcontextlost', onContextLost);
      el.addEventListener('webglcontextrestored', onContextRestored);
    }
    document.addEventListener('visibilitychange', onVisibility);

    void (async () => {
      await manager.ready();
      if (!alive || !containerEl) return;
      try {
        if (manager.isParked(paneId)) {
          await manager.unpark(paneId, containerEl);
        } else {
          await manager.attach(paneId, containerEl, workspaceId);
        }
      } catch (err) {
        console.error('[mobile-term] attach/unpark failed', paneId, err);
        return;
      }
      // Component tore down during the async attach → park so a later remount
      // can unpark the (still-alive) kernel instead of leaking / double-attaching.
      if (!alive) { manager.park(paneId); return; }
      attached = true;
      // iter-60 G3：手机 SPA = raw 字节模式，本地网格权威在 fit（cloud 腿无
      // pty-resized 回执，不能等 host 改格——P4 回归根因）。
      manager.setLocalGridAuthority(paneId, true);
      // iter-60 G4: report the ACTUAL render backend (P4 refactor lost this
      // binding — footer showed the 'Canvas2D' default even under WebGPU).
      backendName = manager.backendName(paneId) ?? 'Canvas2D';
      // Outbound: kernel-generated responses (DSR/DA) from feed + IME
      // write/paste → PTY via the host WS (onStdin → ws.sendStdin).
      manager.onData(paneId, (bytes) => onStdin(td.decode(bytes)));
      // Grid change → claim this viewport's size on the host (auto 自适应全屏).
      manager.onResize(paneId, (rows, cols) => {
        if (onResize && containerEl) {
          onResize(remotePaneId, rows, cols, Math.round(containerEl.clientWidth), Math.round(containerEl.clientHeight));
        }
      });
      manager.setFocused(paneId, true);
      // Immediate fit (kernel grid + host claim) instead of waiting out the
      // ResizeObserver's debounce, so first paint is correctly sized.
      manager.fitPaneNow(paneId);
      focusInput();
    })();

    return () => {
      if (el) {
        el.removeEventListener('pointerdown', stopPointer, true);
        el.removeEventListener('pointermove', stopPointer, true);
        el.removeEventListener('pointerup', stopPointer, true);
        el.removeEventListener('pointercancel', stopPointer, true);
        el.removeEventListener('webglcontextlost', onContextLost);
        el.removeEventListener('webglcontextrestored', onContextRestored);
      }
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });

  onDestroy(() => {
    alive = false;
    manager.setVisualOffsetY(paneId, 0);
    onKeyboardShift?.(0);
    // 句级缓冲：卸载前落笔，防切 pane 丢缓冲文本。
    if (attached && !sbuf.empty) sbufFlush();
    if (sbufTimer !== null) { clearTimeout(sbufTimer); sbufTimer = null; }
    // Keep-alive: PARK (kernel survives, scrollback preserved), never detach.
    // Real teardown (manager.detach) happens in MainApp only when the host
    // actually closes the pane.
    if (attached) manager.park(paneId);
  });

  function positionInputAtCursorOrCenter() {
    const el = hiddenInput;
    if (!el) return;
    const rect = containerEl?.getBoundingClientRect();
    if (rect) {
      const vv = window.visualViewport;
      const anchor = resolveInputAnchor(getCursorPixel(), {
        containerLeft: rect.left,
        containerTop: rect.top,
        containerWidth: rect.width,
        containerHeight: rect.height,
        visualLeft: vv?.offsetLeft ?? 0,
        visualTop: vv?.offsetTop ?? 0,
        visualWidth: vv?.width ?? window.innerWidth,
        visualHeight: vv?.height ?? window.innerHeight,
      });
      el.style.left = `${Math.round(anchor.x)}px`;
      el.style.top = `${Math.round(anchor.y)}px`;
      el.style.height = `${Math.max(1, Math.round(anchor.h))}px`;
    }
  }

  function focusInput() {
    const el = hiddenInput;
    if (!el) return;
    positionInputAtCursorOrCenter();
    el.focus({ preventScroll: true });
    // §A iOS sometimes drops focus on the tiny invisible textarea — re-assert
    // on the next frame and give it a caret so the keyboard reliably stays up.
    requestAnimationFrame(() => {
      if (!el) return;
      if (document.activeElement !== el) el.focus({ preventScroll: true });
      try { el.setSelectionRange(el.value.length, el.value.length); } catch { /* ignore */ }
    });
  }

  /** Explicit soft-keyboard opening is the only path that snaps history back
   * to live output. Pointer coordinates never participate in the IME anchor. */
  function openSoftKeyboard() {
    if (!attached) return;
    manager.scrollToBottom(paneId);
    focusInput();
  }

  // ── Public API (called by MainApp via bind:this) ──
  /** Current grid + pixel size, used by the refresh button / reconnect claim. */
  export function getDims() {
    if (!attached) return null;
    return {
      rows: manager.rows(paneId),
      cols: manager.cols(paneId),
      pixelWidth: Math.round(containerEl?.clientWidth ?? 0),
      pixelHeight: Math.round(containerEl?.clientHeight ?? 0),
    };
  }
  /** Feed raw PTY bytes into THIS pane's kernel (MainApp routes the active
   *  pane's stream here; the manager holds each pane's history). */
  export function feedUtf8(bytes: Uint8Array) { manager.feed(paneId, bytes); }
  /** Route bytes to any live/parked pane kernel, not only this mounted surface. */
  export function feedPane(targetPaneId: string, bytes: Uint8Array) {
    manager.feed(targetPaneId, bytes);
  }
  /** §history-pull: prepend older PTY history at the oldest end of the ring. */
  export function prependScrollback(bytes: Uint8Array) { manager.prependScrollback(paneId, bytes); }
  export function prependScrollbackForPane(targetPaneId: string, bytes: Uint8Array) {
    return manager.prependScrollback(targetPaneId, bytes);
  }
  /** Theme is GLOBAL on the manager (all panes); fine for mobile (one theme). */
  export function applyTheme(theme: Record<string, string>) { manager.setTheme(theme); }
  /** Host told us the PTY resized → resize this pane's kernel grid + repaint. */
  export function resizeKernel(rows: number, cols: number) {
    manager.getKernel(paneId)?.resize(rows, cols);
    manager.forceFullRedraw(paneId);
  }

  // ── Virtual Keyboard (called from MainApp header) ──
  export function handleVirtualKey(key: string, ctrlKey: boolean, alt: boolean, shift: boolean) {
    if (!attached) return;
    // 句级缓冲：同物理键——Backspace 先耗缓冲，其余控制键先落笔再发。
    if (key === 'Backspace' && !ctrlKey && !alt && sbufActive() && sbuf.backspace()) {
      sbufPaint();
      sbufSchedule();
      return;
    }
    if (!sbuf.empty) sbufFlush();
    // G11：虚拟键盘同物理键界规则——Backspace 削已发词段，其余控制键清段。
    pendingWord = key === 'Backspace' && !ctrlKey && !alt ? pendingWordBackspace(pendingWord) : '';
    const bytes = kEncodeKey(key, ctrlKey, alt, shift, false);
    if (bytes.length > 0) { onStdin(td.decode(bytes)); return; }
    const map: Record<string, string> = { Tab: '\t', Escape: '\x1b', Enter: '\r', Backspace: '\x7f', Delete: '\x1b[3~', Home: '\x1b[H', End: '\x1b[F', PageUp: '\x1b[5~', PageDown: '\x1b[6~', Insert: '\x1b[2~' };
    if (map[key]) { onStdin(shift && key === 'Tab' ? '\x1b[Z' : map[key]); return; }
    if (key.startsWith('Arrow')) {
      const arrows: Record<string, string> = { ArrowUp: '\x1b[A', ArrowDown: '\x1b[B', ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D' };
      if (arrows[key]) onStdin(arrows[key]);
    }
  }

  // ── Touch ──

  /** Swipe → wheel/mouse/arrows/scrollback — SSOT in decideTouchScroll (desktop parity). */
  function touchWheel(deltaY: number, clientX: number, clientY: number) {
    if (!attached) return;
    const decision = decideTouchScroll({
      deltaY,
      isMouseReporting: isMouseReporting(),
      isAltScreen: isAltScreen(),
      pixelLike: true,
    });
    if (!decision) return;
    if (decision.kind === 'mouse_wheel') {
      const cell = clientToCell(clientX, clientY) ?? { row: 0, col: 0 };
      const bytes = kEncodeMouse(cell.row, cell.col, decision.btn, 0, false, false, false);
      if (bytes.length > 0) onStdin(td.decode(bytes));
      return;
    }
    if (decision.kind === 'alt_arrows') {
      const one = kEncodeKey(decision.key, false, false, false, false);
      if (one.length === 0) return;
      for (let i = 0; i < decision.presses; i++) onStdin(td.decode(one));
      return;
    }
    // local_scroll
    if (decision.lines < 0) {
      manager.scrollUp(paneId, -decision.lines);
      maybeLoadOlder();
    } else {
      manager.scrollDown(paneId, decision.lines);
    }
  }

  /** Copy the selection to the control device's clipboard, then clear it.
   *  §copy-no-interrupt: copying must NOT send `\x03` to the PTY.
   *  V-MOB-CP: never focus hidden input / never paste. */
  function copyAndClear() {
    if (!attached) return;
    let text = '';
    try {
      text = manager.getSelectionText(paneId) || '';
    } catch { /* kernel may have no selection */ }
    copySelectionOnly(text, {
      writeText: (t) => {
        void writeClipboard(t);
        onHostClipboard?.(t);
      },
      clearSelection: () => {
        manager.clearSelection(paneId);
        hasSelectionState = false;
      },
      // Explicitly pass focus/paste so tests of pure helper prove we never call them.
      focusInput,
      paste: (t) => sendPaste(t),
    });
  }

  /** Paste arbitrary text (the control device's clipboard) into the terminal as
   *  a bracketed paste. Driven by the bottom-bar paste button in MainApp. */
  export function pasteText(text: string) {
    sendPaste(text);
  }

  function handleTouchStart(e: TouchEvent) {
    if (!attached) return;
    if (e.touches.length !== 1) return;
    const t = e.touches[0];
    touchStartY = t.clientY;
    touchStartX = t.clientX;
    touchLastY = t.clientY;
    touchScrollAccum = 0;
    touchStartTime = Date.now();
    // §select-as-mouse (R5): the select toggle SIMULATES A MOUSE — emit mouse
    // signals and let the receiving terminal decide (parity with desktop). When
    // the app captures the mouse (mouse-reporting TUI) we forward a press and the
    // TUI owns the gesture/selection/scroll. ONLY a plain shell falls back to
    // LOCAL text selection + copy pill.
    if (selectionMode) {
      const cell = clientToCell(t.clientX, t.clientY);
      if (cell) {
        if (isMouseReporting()) {
          const g = decideTouchMouseGesture('press');
          const bytes = kEncodeMouse(cell.row, cell.col, g.button, g.action, false, false, false);
          if (bytes.length > 0) onStdin(td.decode(bytes));
        } else {
          startSelection(cell.row, cell.col);
        }
      }
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (!attached || e.touches.length !== 1) return;
    const t = e.touches[0];
    const moved = Math.abs(t.clientY - touchStartY) + Math.abs(t.clientX - touchStartX);
    if (moved < TOUCH_DRAG_THRESHOLD_PX) return;
    e.preventDefault();
    if (selectionMode) {
      selDragging = true;
      const cell = clientToCell(t.clientX, t.clientY);
      // §select-as-mouse (R5): mouse-reporting TUI → motion report (the TUI
      // extends its own selection); plain shell → local text selection.
      if (cell) {
        if (isMouseReporting()) {
          const g = decideTouchMouseGesture('drag');
          const bytes = kEncodeMouse(cell.row, cell.col, g.button, g.action, false, false, false);
          if (bytes.length > 0) onStdin(td.decode(bytes));
        } else {
          extendSelection(cell.row, cell.col);
        }
      }
      return;
    }
    touchScrollAccum += touchLastY - t.clientY;
    touchLastY = t.clientY;
    if (Math.abs(touchScrollAccum) > 24) {
      touchWheel(touchScrollAccum, t.clientX, t.clientY);
      touchScrollAccum = 0;
    }
  }

  function handleTouchEnd(e: TouchEvent) {
    if (e.changedTouches.length !== 1) return;
    const touch = e.changedTouches[0];
    if (!attached) return;
    if (selectionMode) {
      const wasDragging = selDragging;
      selDragging = false;
      const cell = touch ? clientToCell(touch.clientX, touch.clientY) : null;
      if (isMouseReporting()) {
        // §select-as-mouse (R5): complete the simulated gesture with a release —
        // a tap becomes a click (the TUI focuses its own input), a drag becomes a
        // drag-end. decideTouchMouseGesture is the SSOT for button/action.
        if (cell) {
          const g = decideTouchMouseGesture('release');
          const bytes = kEncodeMouse(cell.row, cell.col, g.button, g.action, false, false, false);
          if (bytes.length > 0) onStdin(td.decode(bytes));
        }
      } else if (wasDragging) {
        // Plain shell: finish the local text selection + surface the copy pill.
        endSelectionLocal();
        hasSelectionState = hasSelection();
      } else {
        // A tap in shell selection mode clears any existing selection.
        manager.clearSelection(paneId);
        hasSelectionState = false;
      }
      // §select-tap-keyboard: a TAP (not a drag) in selection mode also raises
      // the soft keyboard so you can type without first leaving select mode.
      if (!wasDragging) openSoftKeyboard();
      return;
    }
    const elapsed = Date.now() - touchStartTime;
    if (elapsed >= TOUCH_TAP_MAX_MS) return;
    // Light tap clears an existing selection (and re-raises the keyboard).
    if (hasSelectionState || hasSelection()) {
      manager.clearSelection(paneId);
      hasSelectionState = false;
      openSoftKeyboard();
      return;
    }
    // Otherwise: focus (raise the soft keyboard) + click-through in TUI apps.
    if (touch) {
      const cell = clientToCell(touch.clientX, touch.clientY);
      if (cell && isMouseReporting()) {
        const p = decideTouchMouseGesture('press');
        const press = kEncodeMouse(cell.row, cell.col, p.button, p.action, false, false, false);
        if (press.length > 0) onStdin(td.decode(press));
        requestAnimationFrame(() => {
          if (attached) {
            const r = decideTouchMouseGesture('release');
            const rel = kEncodeMouse(cell.row, cell.col, r.button, r.action, false, false, false);
            if (rel.length > 0) onStdin(td.decode(rel));
          }
        });
      }
    }
    openSoftKeyboard();
  }

  // ── Composition (IME) + plain text input, both via the hidden textarea ──
  const IME_DUP_WINDOW_MS = 200;
  let imeCommitExpect = '';     // commit from compositionend; the matching trailing `input` is a dup
  let imeCommitExpectTime = 0;
  let lastInputText = '';       // text just emitted by `input`; a matching compositionend is a dup
  let lastInputTime = 0;

  // §1 英文「逐字实时发送 + 空格提交再发整词」去重
  const RECENT_SENT_WINDOW_MS = 1200;
  let recentSent = '';
  let recentSentTime = 0;
  // iter-60 G11 二修：已发送「当前词段」精确追踪（无时窗）。补全 commit 与它求
  // 公共前缀只发差量；控制键/回车/粘贴置空，Backspace 削尾。时窗法实测漏
  // `Ev`+停顿+选 `Everything` → `EvEverything`。
  let pendingWord = '';

  // ── 句级输入缓冲（sentenceBuffer prop 开启时）────────────────────────────
  const sbuf = new SentenceBuffer();
  let sbufTimer: ReturnType<typeof setTimeout> | null = null;

  /** 缓冲激活 = 开关开 && 非 TUI 场景（alt-screen / 鼠标上报要逐键，旁路）。 */
  function sbufActive(): boolean {
    return sentenceBuffer && attached && !isAltScreen() && !isMouseReporting();
  }
  /** 把缓冲文本画到预编辑覆盖层（与 IME preedit 同通道；组合中则拼上组合段）。 */
  function sbufPaint(composing = '') {
    const text = sbuf.preview + composing;
    const a = manager.inputAnchorResolved(paneId);
    if (a && text) manager.setPreedit(paneId, text, a.row, a.col);
    else if (!text && !isComposing) manager.clearPreedit(paneId);
  }
  function sbufSchedule() {
    if (sbufTimer !== null) clearTimeout(sbufTimer);
    sbufTimer = setTimeout(() => { sbufTimer = null; sbufFlush(); }, SENTENCE_FLUSH_MS);
  }
  /** 落笔：缓冲全文一次发 PTY，词段随之更新（后续差量仍可用）。 */
  function sbufFlush() {
    if (sbufTimer !== null) { clearTimeout(sbufTimer); sbufTimer = null; }
    const text = sbuf.takeFlush();
    if (!text) return;
    onStdin(text);
    pendingWord = updatePendingWord(pendingWord, text);
    if (!isComposing) manager.clearPreedit(paneId);
  }

  function handleCompositionStart() {
    // §R4 IME: mark the input-start anchor + capture the preedit anchor cell.
    isComposing = true;
    manager.markInputStart(paneId);
    positionInputAtCursorOrCenter();
  }
  function handleCompositionUpdate(e: CompositionEvent) {
    // Renderer-side preedit overlay: painted on top of the cell grid without
    // touching kernel cells (a TUI redraw can't corrupt it and vice-versa).
    // 句级缓冲开启且有缓冲文本时，预览 = 缓冲 + 组合中段（一体显示）。
    if (sbufActive() && !sbuf.empty) {
      sbufPaint(e.data ?? '');
      return;
    }
    const a = manager.inputAnchorResolved(paneId);
    if (a) manager.setPreedit(paneId, e.data ?? '', a.row, a.col);
  }
  function handleCompositionEnd(e: CompositionEvent) {
    isComposing = false;
    manager.clearPreedit(paneId);
    const data = e.data ?? '';
    // Clear the textarea so a late `input` can't resend the committed text.
    if (hiddenInput) hiddenInput.value = '';
    // §1.27: repaint cells under the (now-cleared) preedit overlay.
    manager.forceFullRedraw(paneId);
    if (!data) return;
    // §1 iOS English predictive streams each letter then fires compositionend
    // with the whole word on space → committing again duplicates it.
    if (Date.now() - recentSentTime < RECENT_SENT_WINDOW_MS && recentSent.trimEnd().endsWith(data)) {
      recentSent = '';
      return;
    }
    // Some IMEs fire `input` before `compositionend`; don't send twice.
    if (data === lastInputText && Date.now() - lastInputTime < IME_DUP_WINDOW_MS) {
      lastInputText = '';
      return;
    }
    // 句级缓冲：commit 在缓冲内前缀合并（Ev+Everything→Everything），零 PTY 字节。
    if (sbufActive()) {
      if (sbuf.empty) manager.markInputStart(paneId);
      sbuf.commit(data);
      sbufPaint();
      sbufSchedule();
      imeCommitExpect = data;
      imeCommitExpectTime = Date.now();
      return;
    }
    // iter-60 G11（二修，无时窗）：自动补全提交去重——已发送「当前词段」与
    // commit 求公共前缀，只发「退格×N + 差量」（Spac+Space→e；Ev+Everything→
    // erything；Spac+Spice→␡␡ice）。差量含退格须走 raw onStdin（bracketed
    // paste 会把 \x7f 变字面量）。
    {
      const delta = imeCommitDelta(pendingWord, data);
      if (delta !== data) {
        if (delta) onStdin(delta);
        pendingWord = trailingWord(data);
        recentSent = data;
        recentSentTime = Date.now();
        imeCommitExpect = data;
        imeCommitExpectTime = Date.now();
        return;
      }
    }
    // Commit: encodePaste + ship to PTY via the registered dataHandler (onStdin).
    manager.paste(paneId, data);
    pendingWord = trailingWord(data);
    // Arm dedup for the trailing `input` event that normally follows.
    imeCommitExpect = data;
    imeCommitExpectTime = Date.now();
  }

  // Fires for plain typed / predicted / autocorrected text that isn't an IME
  // composition.
  function handleInput(e: Event) {
    if (!attached) return;
    if (isComposing || (e as InputEvent).isComposing) return;
    const ta = e.target as HTMLTextAreaElement;
    const text = ta.value;
    ta.value = '';
    if (!text) return;
    const inputType = (e as InputEvent).inputType || '';
    // §1 Robust IME-commit dedup: the browser re-inserts committed text as a
    // non-composing `input` with inputType `insertCompositionText`; already
    // sent via manager.paste in handleCompositionEnd → swallow by TYPE.
    if (inputType === 'insertCompositionText') return;
    // Fallback content/time window for IMEs that report a plain inputType.
    if (text === imeCommitExpect && Date.now() - imeCommitExpectTime < IME_DUP_WINDOW_MS) {
      imeCommitExpect = '';
      return;
    }
    // §1 Autocorrect / predictive replacement：旧行为是整帧丢弃（保字面），用户
    // 点的修正词就此丢失。iter-60 G11 改为差量应用：与已发词段求公共前缀，退格
    // 删多余尾巴再补差量——修正生效且不重复。无已发词段上下文时仍丢弃防误发。
    if (inputType === 'insertReplacementText') {
      // 句级缓冲：回改在缓冲内整词替换（跨词回改也不丢，语音主收益点）。
      if (sbufActive() && sbuf.replaceTrailing(text)) {
        sbufPaint();
        sbufSchedule();
        return;
      }
      const delta = imeCommitDelta(pendingWord, text);
      if (delta && delta !== text) {
        onStdin(delta);
        pendingWord = trailingWord(text);
        recentSent = text;
        recentSentTime = Date.now();
      }
      return;
    }
    // §2 Sticky on-screen modifier armed → form a chord (Ctrl+C …) per character.
    if (anyMod()) {
      if (!sbuf.empty) sbufFlush(); // 句级缓冲：组合键先落笔。
      pendingWord = ''; // G11：组合键=词界。
      const sm = consumeMods();
      for (const ch of text) {
        const bytes = kEncodeKey(ch, sm.ctrl, sm.alt, sm.shift, false);
        onStdin(bytes.length > 0 ? td.decode(bytes) : ch);
      }
      return;
    }
    // 句级缓冲：明文进缓冲画预编辑，停顿才落笔（语音/高频改写零退格窗口）。
    if (sbufActive()) {
      if (sbuf.empty) manager.markInputStart(paneId);
      sbuf.insert(text);
      sbufPaint();
      sbufSchedule();
      lastInputText = text;
      lastInputTime = Date.now();
      return;
    }
    onStdin(text);
    pendingWord = updatePendingWord(pendingWord, text);
    lastInputText = text;
    lastInputTime = Date.now();
    recentSent = (Date.now() - recentSentTime < RECENT_SENT_WINDOW_MS ? recentSent : '') + text;
    recentSentTime = Date.now();
  }

  // ── Keyboard ──
  function handleKeydown(e: KeyboardEvent) {
    if (isComposing || e.isComposing) return;
    if (!attached) return;
    // Unmodified printable keys flow into the hidden textarea; its `input` event
    // emits them (keeps IME + mobile prediction working, avoids double-send).
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) return;
    // Clipboard: handle paste/copy before the generic ctrl/meta passthrough.
    if ((e.ctrlKey || e.metaKey) && (e.key === 'v' || e.key === 'V')) {
      e.preventDefault();
      void pasteFromClipboard();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === 'c' || e.key === 'C') && hasSelection()) {
      e.preventDefault();
      void copySelection();
      return;
    }
    const specialKeys: Record<string, string> = { Enter: '\r', Escape: '\x1b', Tab: '\t', Insert: '\x1b[2~' };
    const shiftSpecial: Record<string, string> = { Tab: '\x1b[Z' };
    // 句级缓冲：Backspace 先耗缓冲（本地删，不发 PTY）；其余控制键先把缓冲落笔
    // 再发控制字节（保证 "word\r" 顺序）。
    if (e.key === 'Backspace' && !e.ctrlKey && !e.metaKey && !e.altKey && sbufActive() && sbuf.backspace()) {
      e.preventDefault();
      sbufPaint();
      sbufSchedule();
      return;
    }
    if (!sbuf.empty) sbufFlush();
    // G11：控制键=词界。Backspace 削已发词段一格，其余控制/组合键直接清段——
    // 光标/行内容已非我们可见，宁可放弃去重也不误退格。
    if (e.shiftKey && shiftSpecial[e.key]) { e.preventDefault(); pendingWord = ''; onStdin(shiftSpecial[e.key]); return; }
    if (specialKeys[e.key]) { e.preventDefault(); pendingWord = ''; onStdin(specialKeys[e.key]); return; }
    if (['Backspace','Delete','Home','End','PageUp','PageDown'].includes(e.key) || e.key.startsWith('F') && e.key.length >= 2) {
      e.preventDefault();
      pendingWord = e.key === 'Backspace' ? pendingWordBackspace(pendingWord) : '';
      const bytes = kEncodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      pendingWord = '';
      const bytes = kEncodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
      return;
    }
    if (e.key.length === 1) {
      e.preventDefault();
      pendingWord = '';
      const bytes = kEncodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
      else onStdin(e.key);
      return;
    }
    if (e.key.startsWith('Arrow')) {
      e.preventDefault();
      pendingWord = '';
      const bytes = kEncodeKey(e.key, e.ctrlKey, e.altKey, e.shiftKey, e.metaKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
      else {
        const arrows: Record<string, string> = { ArrowUp: '\x1b[A', ArrowDown: '\x1b[B', ArrowRight: '\x1b[C', ArrowLeft: '\x1b[D' };
        if (arrows[e.key]) onStdin(arrows[e.key]);
      }
    }
  }

  /** Encode arbitrary text as a bracketed paste and forward it to the host. */
  function sendPaste(text: string) {
    if (!attached || !text) return;
    if (!sbuf.empty) sbufFlush(); // 句级缓冲：粘贴前先落笔，保输入顺序。
    pendingWord = ''; // G11：粘贴=词界（粘贴内容不参与补全去重）。
    const bytes = kEncodePaste(text);
    if (bytes.length > 0) onStdin(td.decode(bytes));
  }

  /** Read the system clipboard and paste it (Ctrl/Cmd+V is the user gesture). */
  async function pasteFromClipboard() {
    if (!attached) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) sendPaste(text);
    } catch { /* clipboard blocked: no permission / insecure context */ }
  }

  /** Copy the active selection to the system clipboard (desktop Ctrl/Cmd+C).
   *  V-MOB-CP: write + clear only — no focusInput / no paste. */
  async function copySelection() {
    if (!attached) return;
    const text = manager.getSelectionText(paneId) || '';
    copySelectionOnly(text, {
      writeText: (t) => {
        void writeClipboard(t);
        onHostClipboard?.(t);
      },
      clearSelection: () => manager.clearSelection(paneId),
      focusInput,
      paste: (t) => sendPaste(t),
    });
  }

  // Native paste fallback (right-click → paste, middle-click) on the focused
  // hidden textarea. Ctrl/Cmd+V is handled in handleKeydown instead.
  function handlePaste(e: ClipboardEvent) {
    if (!attached) return;
    e.preventDefault();
    const text = e.clipboardData?.getData('text') ?? '';
    sendPaste(text);
  }

  // ── Mouse (desktop) ──
  function mouseButton(e: MouseEvent): number {
    return e.button === 1 ? 1 : e.button === 2 ? 2 : 0; // left=0 middle=1 right=2
  }

  function handleMouseDown(e: MouseEvent) {
    if (!attached) return;
    focusInput();
    const cell = clientToCell(e.clientX, e.clientY);
    if (!cell) return;
    if (isMouseReporting()) {
      e.preventDefault();
      const bytes = kEncodeMouse(cell.row, cell.col, mouseButton(e), 0, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else if (e.button === 0) {
      e.preventDefault();
      mouseSelecting = true;
      startSelection(cell.row, cell.col);
    }
  }

  function handleMouseMove(e: MouseEvent) {
    if (!attached) return;
    if (mouseSelecting) {
      const cell = clientToCell(e.clientX, e.clientY);
      if (cell) extendSelection(cell.row, cell.col);
      return;
    }
    // Drag with a button held while the app captures the mouse → motion report.
    if (e.buttons !== 0 && isMouseReporting()) {
      const cell = clientToCell(e.clientX, e.clientY);
      if (!cell) return;
      const btn = (e.buttons & 1) ? 0 : (e.buttons & 4) ? 1 : (e.buttons & 2) ? 2 : 0;
      const bytes = kEncodeMouse(cell.row, cell.col, btn, 2, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    }
  }

  function handleMouseUp(e: MouseEvent) {
    if (!attached) return;
    if (isMouseReporting()) {
      const cell = clientToCell(e.clientX, e.clientY);
      if (!cell) return;
      e.preventDefault();
      const bytes = kEncodeMouse(cell.row, cell.col, mouseButton(e), 1, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else if (mouseSelecting) {
      mouseSelecting = false;
      endSelectionLocal();
      hasSelectionState = hasSelection();
    }
  }

  function handleWheel(e: WheelEvent) {
    if (!attached) return;
    if (isMouseReporting()) {
      const cell = clientToCell(e.clientX, e.clientY) ?? { row: 0, col: 0 };
      e.preventDefault();
      const btn = e.deltaY < 0 ? 64 : 65; // wheel up / down
      const bytes = kEncodeMouse(cell.row, cell.col, btn, 0, e.shiftKey, e.altKey, e.ctrlKey);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else {
      e.preventDefault();
      const lines = e.deltaY > 0 ? 3 : -3;
      if (lines < 0) { manager.scrollUp(paneId, -lines); maybeLoadOlder(); } else manager.scrollDown(paneId, lines);
    }
  }

  function handleContextMenu(e: MouseEvent) {
    // Hand right-click to mouse-capturing apps; otherwise leave the native menu.
    if (isMouseReporting()) e.preventDefault();
  }

  // §passive-fix: touchmove (selection drag) and wheel (scroll) call
  // preventDefault, which Chrome rejects inside its default-passive listeners.
  // Svelte's declarative on* attributes can't set {passive:false}, so attach
  // these two manually on the container.
  $effect(() => {
    const el = containerEl;
    if (!el) return;
    el.addEventListener('touchmove', handleTouchMove, { passive: false });
    el.addEventListener('wheel', handleWheel, { passive: false });
    return () => {
      el.removeEventListener('touchmove', handleTouchMove);
      el.removeEventListener('wheel', handleWheel);
    };
  });

  // ── §kb-transform：只移动视觉投影，绝不 refit/resize grid ──
  let containerTopWhenIdle = 0;

  function computeKeyboardShift(): number {
    const vv = window.visualViewport;
    const anchor = manager.inputAnchorResolved(paneId);
    if (!vv || !containerEl || !anchor) return 0;
    return terminalVisualShiftPx({
      layoutHeightPx: window.innerHeight,
      visualHeightPx: vv.height || 0,
      visualOffsetTopPx: vv.offsetTop || 0,
      stageTopPx: containerTopWhenIdle,
      cursorYPx: anchor.y,
      cellHeightPx: anchor.cellH,
    });
  }

  function applyKeyboardShift(): void {
    const next = computeKeyboardShift();
    manager.setVisualOffsetY(paneId, next);
    onKeyboardShift?.(next);
  }

  $effect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    function onViewportChange() {
      if (window.innerHeight - vv!.height <= 0 && containerEl) {
        containerTopWhenIdle = Math.round(containerEl.getBoundingClientRect().top);
      }
      applyKeyboardShift();
    }
    if (containerEl) containerTopWhenIdle = Math.round(containerEl.getBoundingClientRect().top);
    vv.addEventListener('resize', onViewportChange);
    vv.addEventListener('scroll', onViewportChange);
    onViewportChange();
    return () => {
      vv.removeEventListener('resize', onViewportChange);
      vv.removeEventListener('scroll', onViewportChange);
    };
  });

  // orientationchange fires the most disruptive grid change; refit explicitly
  // (idempotent + debounced by the manager — at most one extra fit).
  $effect(() => {
    function onOrientation() {
      if (attached) manager.viewportChanged(paneId);
      if (containerEl) containerTopWhenIdle = Math.round(containerEl.getBoundingClientRect().top);
      applyKeyboardShift();
    }
    window.addEventListener('orientationchange', onOrientation);
    return () => window.removeEventListener('orientationchange', onOrientation);
  });

</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="container" bind:this={containerEl} role="application" aria-busy={scrollbackLoading}
  ontouchstart={handleTouchStart}
  ontouchend={handleTouchEnd}
  onmousedown={handleMouseDown}
  onmousemove={handleMouseMove}
  onmouseup={handleMouseUp}
  oncontextmenu={handleContextMenu}
>
  {#if scrollbackLoading}
    <div class="scrollback-loading" role="progressbar" aria-label="Loading older terminal output"></div>
  {/if}
  {#if !attached}
    <div class="loading">{$t('mobile.initializingTerminal')}</div>
  {/if}

  <!-- The TerminalManager appends this pane's <canvas> here on attach. -->

  <!-- Hidden, focusable input sink: raises the mobile keyboard on tap and
       receives IME composition. pointer-events:none so it never steals canvas
       clicks; it is focused programmatically. -->
  <textarea
    bind:this={hiddenInput}
    class="hidden-input"
    autocapitalize="off"
    autocomplete="off"
    spellcheck="false"
    aria-hidden="true"
    tabindex="-1"
    onkeydown={handleKeydown}
    oninput={handleInput}
    oncompositionstart={handleCompositionStart}
    oncompositionupdate={handleCompositionUpdate}
    oncompositionend={handleCompositionEnd}
    onpaste={handlePaste}
    onfocus={() => manager.setFocused(paneId, true)}
    onblur={() => manager.setFocused(paneId, false)}
  ></textarea>

  <!-- §D Floating copy pill (R8) — shown while a text selection exists. Copy
       fires on touchend directly (with preventDefault) so the tap never falls
       through to the container's synthesized mousedown → focusInput (which
       popped the keyboard and swallowed the copy). Mouse events are stopped so a
       pill tap can't reach the container's mouse handlers. -->
  {#if hasSelectionState}
    <button
      class="copy-pill"
      onclick={copyAndClear}
      ontouchstart={(e) => e.stopPropagation()}
      ontouchend={(e) => { e.preventDefault(); e.stopPropagation(); copyAndClear(); }}
      onmousedown={(e) => e.stopPropagation()}
      onmouseup={(e) => e.stopPropagation()}
    >{$t('mobile.copy')}</button>
  {/if}
</div>

<style>
  .container{position:relative;flex:1;overflow:hidden;background:var(--rg-bg);touch-action:manipulation}
  .scrollback-loading{position:absolute;top:0;left:0;right:0;height:2px;z-index:8;overflow:hidden;background:color-mix(in srgb,var(--rg-accent) 20%,transparent)}
  .scrollback-loading::after{content:"";position:absolute;inset:0;width:35%;background:var(--rg-accent);animation:scrollback-progress .9s ease-in-out infinite}
  @keyframes scrollback-progress{from{transform:translateX(-100%)}to{transform:translateX(385%)}}
  /* Near-invisible input sink parked at the cursor. pointer-events:none keeps it
     from stealing canvas clicks. Opacity must be >0 so the IME candidate window
     anchors to a detectable element. */
  .hidden-input{position:absolute;top:0;left:0;width:1px;height:1em;margin:0;padding:0;border:0;font-size:16px;
    opacity:0.01;pointer-events:none;resize:none;overflow:hidden;white-space:nowrap;z-index:5;
    background:transparent;color:transparent;caret-color:transparent;outline:none;font-family:inherit}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:var(--rg-fg-muted);font-size:14px;z-index:4}
  .copy-pill{position:absolute;top:8px;right:8px;z-index:6;display:flex;align-items:center;justify-content:center;height:32px;padding:0 16px;border:1px solid var(--rg-accent);border-radius:16px;background:color-mix(in srgb,var(--rg-accent) 22%,var(--rg-surface));color:var(--rg-fg);font-size:13px;font-weight:600;cursor:pointer;box-shadow:0 4px 14px -2px rgba(0,0,0,.5);-webkit-tap-highlight-color:transparent}
  .copy-pill:active{background:color-mix(in srgb,var(--rg-accent) 36%,var(--rg-surface))}
</style>
