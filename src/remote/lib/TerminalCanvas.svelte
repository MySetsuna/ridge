<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { TerminalController } from './terminalController';
  import VirtualKeyboard from './VirtualKeyboard.svelte';
  import { anyMod, consumeMods } from './modState.svelte';

  let { paneId, onStdin, onResize, showKeyboard = false, selectionMode = false, backendName = $bindable('Canvas2D') }: {
    paneId: string | null;
    onStdin: (data: string) => void;
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    showKeyboard?: boolean;
    // §selection: when on, single-finger drag selects (Shell: kernel selection +
    // copy pill; TUI: forwards mouse so the TUI runs its own selection). When
    // off, single-finger drag scrolls and never selects (two-finger always scrolls).
    selectionMode?: boolean;
    backendName?: string;
  } = $props();

  let canvasEl: HTMLCanvasElement | undefined = $state();
  let containerEl: HTMLDivElement | undefined = $state();
  // Hidden, focusable textarea: the only way to (a) raise the mobile soft
  // keyboard on tap and (b) receive IME composition events on desktop. The
  // canvas itself can't be focused, so without this Chinese input never starts.
  let hiddenInput: HTMLTextAreaElement | undefined = $state();
  let ctrl: TerminalController | null = null;
  let ready = $state(false);
  // §theme-cache: hold the most-recent theme so a push that arrived before the
  // (async) controller existed — or before a pane switch re-creates state — is
  // re-applied once the kernel is ready, instead of being dropped by the `ctrl?.`
  // guard in applyTheme(). Mirrors the desktop manager caching opts.theme and
  // applying it to every pane on attach (the gap that left the mobile terminal
  // painted in the default palette).
  let lastTheme: Record<string, string> | null = null;

  // Mouse drag-select state (desktop; only when the app isn't grabbing mouse).
  let mouseSelecting = false;

  const td = new TextDecoder();

  // Keyboard offset (mobile): the canvas is pushed up by `keyboardOffset` — a
  // DYNAMIC amount that lifts only as far as needed to keep the input cursor
  // clear of the soft keyboard (full lift when the cursor sits at the bottom of
  // the screen, none when it's already above the keyboard, partial in between).
  // `keyboardHeight` is the full soft-keyboard height, used to dock the quick-key
  // bar just above the soft keyboard regardless of the canvas's dynamic lift.
  let keyboardOffset = $state(0);
  let keyboardHeight = $state(0);
  const VK_BAR_HEIGHT = 96;    // approx quick-key bar height (2 rows + padding)
  const KB_CURSOR_MARGIN = 12; // gap kept between the cursor bottom and the keyboard

  // Touch state. Gesture model: two-finger pan = scroll; single-finger tap =
  // focus (+ click in mouse-reporting apps); single-finger drag = selection
  // (forwarded to the TUI as mouse events in mouse-reporting mode, local kernel
  // selection otherwise).
  let touchStartY = 0;
  let touchStartX = 0;
  let touchScrollAccum = 0;
  let touchLastY = 0;
  let isTwoFinger = false;
  let twoFingerLastY = 0;
  let singleDragging = false;   // selectionMode OFF: a single-finger scroll drag
  let touchStartTime = 0;
  const TOUCH_DRAG_THRESHOLD_PX = 8;
  const TOUCH_TAP_MAX_MS = 250;

  // §selection drag state.
  let selDragging = false;                 // a selection drag is in progress
  let hasSelectionState = $state(false);    // drives the floating copy pill (Shell only)
  let lastSelCell: { row: number; col: number } | null = null;
  let edgeScrollTimer: ReturnType<typeof setInterval> | null = null;
  let edgeScrollDir = 0;                    // -1 = up, +1 = down, 0 = none
  const EDGE_ZONE_PX = 48;

  onMount(async () => {
    if (!canvasEl || !containerEl) return;
    ctrl = await TerminalController.create(canvasEl, containerEl);
    ctrl.onStdin = (data) => { if (paneId) onStdin(data); };
    ctrl.onResize = (r, c, pw, ph) => {
      if (paneId && onResize) onResize(paneId, r, c, pw, ph);
    };
    backendName = ctrl.backendName;
    // Re-apply a theme that the host pushed before this async create finished —
    // without this the kernel keeps its compile-time default palette.
    if (lastTheme) ctrl.applyTheme(lastTheme);
    ready = true;
    ctrl.setFocused(true);
    focusInput();
  });

  function focusInput() {
    const el = hiddenInput;
    if (!el) return;
    el.focus({ preventScroll: true });
    // §A iOS sometimes drops focus on the tiny invisible textarea — the soft
    // keyboard flashes open then closes (needed a second tap). Re-assert focus on
    // the next frame and give it a caret (setSelectionRange) so the keyboard
    // reliably stays up even when nothing is selected / the field is empty.
    requestAnimationFrame(() => {
      if (!el) return;
      if (document.activeElement !== el) el.focus({ preventScroll: true });
      try { el.setSelectionRange(el.value.length, el.value.length); } catch { /* ignore */ }
    });
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

  onDestroy(() => { stopEdgeScrollTimer(); ctrl?.destroy(); });

  // §A raise the soft keyboard when the user opens the quick-key bar.
  $effect(() => { if (showKeyboard) focusInput(); });

  let ro: ResizeObserver | undefined;
  onMount(() => {
    // `autocorrect` is a non-standard (iOS Safari) attribute missing from
    // Svelte's textarea typings — set it via the DOM to keep iOS from rewriting
    // terminal input without tripping svelte-check.
    hiddenInput?.setAttribute('autocorrect', 'off');
    ro = new ResizeObserver(() => ctrl?.requestResize());
    if (containerEl) ro.observe(containerEl);
    return () => ro?.disconnect();
  });

  // ── Public API ──
  export function feed(data: string) {
    if (ctrl) ctrl.feed(new TextEncoder().encode(data));
  }
  export function feedUtf8(bytes: Uint8Array) { ctrl?.feed(bytes); scheduleKbRecompute(); }
  export function applyDelta(bytes: Uint8Array) { ctrl?.applyDelta(bytes); }
  export function resizeKernel(rows: number, cols: number) {
    if (ctrl) {
      ctrl.kernelResize(rows, cols);
    }
  }
  export function getDims() { return ctrl?.getDims() ?? null; }
  export function applyTheme(theme: Record<string, string>) { lastTheme = theme; ctrl?.applyTheme(theme); }
  export function applyDeltaBase64(b64: string) {
    const binary = atob(b64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
    ctrl?.applyDelta(bytes);
  }
  /** Wipe the local kernel (screen + scrollback) so the next pane's content can't
   *  bleed in. Called by the host on pane switch / reconnect; the new pane's
   *  scrollback replay repaints a clean, isolated view. */
  export function resetForSwitch() { ctrl?.resetForSwitch(); }

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

  /** Send a mouse wheel event (or scroll scrollback) matching handleWheel(). */
  function touchWheel(deltaY: number, clientX: number, clientY: number) {
    if (!ctrl) return;
    if (ctrl.isMouseReporting()) {
      const cell = ctrl.clientToCell(clientX, clientY) ?? { row: 0, col: 0 };
      const btn = deltaY < 0 ? 64 : 65; // wheel up / down
      const bytes = ctrl.encodeMouse(cell.row, cell.col, btn, 0, false, false, false);
      if (bytes.length > 0) onStdin(td.decode(bytes));
    } else {
      const lines = deltaY > 0 ? 3 : -3;
      if (lines < 0) ctrl.scrollUp(-lines); else ctrl.scrollDown(lines);
    }
  }

  /** Centroid Y of a two-finger touch, used for pan-scroll tracking. */
  function twoFingerCentroidY(e: TouchEvent): number {
    return (e.touches[0].clientY + e.touches[1].clientY) / 2;
  }

  // ── Selection (explicit selectionMode) ──

  /** While selecting near the top/bottom edge, keep scrolling so the selection
   *  can extend past the visible area; re-extend to the last finger cell each tick. */
  function startEdgeScroll(dir: number) {
    if (edgeScrollDir === dir) return;
    edgeScrollDir = dir;
    if (edgeScrollTimer) { clearInterval(edgeScrollTimer); edgeScrollTimer = null; }
    if (dir === 0) return;
    edgeScrollTimer = setInterval(() => {
      if (!ctrl || edgeScrollDir === 0) return;
      if (edgeScrollDir < 0) ctrl.scrollUp(1); else ctrl.scrollDown(1);
      if (lastSelCell && !ctrl.isMouseReporting()) ctrl.extendSelection(lastSelCell.row, lastSelCell.col);
    }, 60);
  }
  function stopEdgeScrollTimer() {
    if (edgeScrollTimer) { clearInterval(edgeScrollTimer); edgeScrollTimer = null; }
    edgeScrollDir = 0;
  }

  /** Abort an in-progress selection drag (e.g. a second finger landed → scroll). */
  function cancelSelectionDrag() {
    selDragging = false;
    stopEdgeScrollTimer();
    lastSelCell = null;
    if (ctrl && !ctrl.isMouseReporting()) { ctrl.clearSelection(); hasSelectionState = false; }
  }

  /** Copy the current (Shell) selection to the clipboard and clear it. */
  async function copyAndClear() {
    if (!ctrl) return;
    const text = ctrl.getSelectionText();
    if (text) { try { await navigator.clipboard.writeText(text); } catch { /* clipboard blocked */ } }
    ctrl.clearSelection();
    hasSelectionState = false;
  }

  function handleTouchStart(e: TouchEvent) {
    if (!ctrl) return;
    if (e.touches.length >= 2) {
      // Two fingers → pan-scroll ONLY. Cancel any in-progress single-finger
      // selection so a finger-down-then-second-finger never leaves a stray
      // selection (§C: two-finger must never select).
      if (selDragging) cancelSelectionDrag();
      isTwoFinger = true;
      singleDragging = false;
      twoFingerLastY = twoFingerCentroidY(e);
      touchScrollAccum = 0;
      e.preventDefault();
      return;
    }
    if (e.touches.length !== 1) return;
    touchStartY = e.touches[0].clientY;
    touchStartX = e.touches[0].clientX;
    touchLastY = e.touches[0].clientY;
    touchScrollAccum = 0;
    touchStartTime = Date.now();
    singleDragging = false;
    selDragging = false;
  }

  function handleTouchMove(e: TouchEvent) {
    if (!ctrl) return;
    // Two-finger pan = scroll (wheel for mouse-reporting apps, scrollback else).
    if (isTwoFinger && e.touches.length === 2) {
      e.preventDefault();
      const y = twoFingerCentroidY(e);
      touchScrollAccum += twoFingerLastY - y; // fingers up → scroll content down
      twoFingerLastY = y;
      if (Math.abs(touchScrollAccum) > 24) {
        touchWheel(touchScrollAccum, e.touches[0].clientX, e.touches[0].clientY);
        touchScrollAccum = 0;
      }
      return;
    }
    if (e.touches.length !== 1 || isTwoFinger) return;
    const t = e.touches[0];
    const cell = ctrl.clientToCell(t.clientX, t.clientY);
    if (!cell) return;
    const moved = Math.abs(t.clientY - touchStartY) + Math.abs(t.clientX - touchStartX);

    if (selectionMode) {
      // §D Selection drag: Shell → kernel selection (+ copy pill); TUI → forward
      // mouse press/motion so the TUI program runs its own selection.
      if (!selDragging) {
        if (moved < TOUCH_DRAG_THRESHOLD_PX) return;
        selDragging = true;
        const start = ctrl.clientToCell(touchStartX, touchStartY) ?? cell;
        if (ctrl.isMouseReporting()) {
          const b = ctrl.encodeMouse(start.row, start.col, 0, 0, false, false, false);
          if (b.length > 0) onStdin(td.decode(b));
        } else {
          ctrl.startSelection(start.row, start.col);
        }
      }
      e.preventDefault();
      lastSelCell = cell;
      if (ctrl.isMouseReporting()) {
        const b = ctrl.encodeMouse(cell.row, cell.col, 0, 2, false, false, false); // motion w/ button
        if (b.length > 0) onStdin(td.decode(b));
      } else {
        ctrl.extendSelection(cell.row, cell.col);
        hasSelectionState = true;
      }
      // §D Edge auto-scroll while selecting.
      const rect = canvasEl?.getBoundingClientRect();
      if (rect) {
        if (t.clientY < rect.top + EDGE_ZONE_PX) startEdgeScroll(-1);
        else if (t.clientY > rect.bottom - EDGE_ZONE_PX) startEdgeScroll(1);
        else stopEdgeScrollTimer();
      }
      return;
    }

    // §C selectionMode OFF: single-finger drag = scroll, never select.
    if (!singleDragging) {
      if (moved < TOUCH_DRAG_THRESHOLD_PX) return;
      singleDragging = true;
      touchScrollAccum = 0;
      touchLastY = t.clientY;
    }
    e.preventDefault();
    touchScrollAccum += touchLastY - t.clientY;
    touchLastY = t.clientY;
    if (Math.abs(touchScrollAccum) > 24) {
      touchWheel(touchScrollAccum, t.clientX, t.clientY);
      touchScrollAccum = 0;
    }
  }

  function handleTouchEnd(e: TouchEvent) {
    if (isTwoFinger) { isTwoFinger = false; touchScrollAccum = 0; return; }
    const touch = e.changedTouches[0];
    // §D End a selection drag: TUI → mouse release; Shell → finalize selection
    // (the copy pill then offers copy). Edge auto-scroll stops.
    if (selDragging) {
      selDragging = false;
      stopEdgeScrollTimer();
      lastSelCell = null;
      const cell = touch ? ctrl?.clientToCell(touch.clientX, touch.clientY) : null;
      if (ctrl?.isMouseReporting()) {
        if (cell) {
          const b = ctrl.encodeMouse(cell.row, cell.col, 0, 1, false, false, false); // release
          if (b.length > 0) onStdin(td.decode(b));
        }
      } else {
        ctrl?.endSelection();
        hasSelectionState = !!ctrl?.hasSelection();
      }
      return;
    }
    // §C A single-finger scroll drag (selectionMode off) — just end it.
    if (singleDragging) { singleDragging = false; return; }
    // Tap.
    const elapsed = Date.now() - touchStartTime;
    if (elapsed < TOUCH_TAP_MAX_MS && ctrl) {
      // §D Light tap clears an existing selection (and re-raises the keyboard).
      if (hasSelectionState || ctrl.hasSelection()) {
        ctrl.clearSelection();
        hasSelectionState = false;
        focusInput();
        return;
      }
      // Otherwise: focus (raise the soft keyboard) + click-through in TUI apps.
      if (touch) {
        const cell = ctrl.clientToCell(touch.clientX, touch.clientY);
        if (cell && ctrl.isMouseReporting()) {
          const press = ctrl.encodeMouse(cell.row, cell.col, 0, 0, false, false, false);
          if (press.length > 0) onStdin(td.decode(press));
          requestAnimationFrame(() => {
            if (ctrl) {
              const rel = ctrl.encodeMouse(cell.row, cell.col, 3, 1, false, false, false);
              if (rel.length > 0) onStdin(td.decode(rel));
            }
          });
        }
      }
      focusInput();
    }
  }

  // ── Composition (IME) + plain text input, both via the hidden textarea ──
  //
  // Mobile IMEs commit text via BOTH `compositionend` and a trailing `input`
  // event, and the two can arrive in either order. We send the commit exactly
  // once with a content-matched, time-windowed dedup that — crucially — only
  // arms around composition, so genuinely repeated typing (e.g. "aa") is never
  // dropped.
  const IME_DUP_WINDOW_MS = 200;
  let imeCommitExpect = '';     // commit from compositionend; the matching trailing `input` is a dup
  let imeCommitExpectTime = 0;
  let lastInputText = '';       // text just emitted by `input`; a matching compositionend is a dup
  let lastInputTime = 0;

  // §1 英文「逐字实时发送 + 空格提交再发整词」去重：滚动记录最近经 `input` 实际
  // 发出的字面文本；compositionend 若发现提交内容正是刚实发文本的尾部，就不再
  // 重复提交（iOS 英文预测会逐字 input 后在 commit 时把整词再发一次）。
  const RECENT_SENT_WINDOW_MS = 1200;
  let recentSent = '';
  let recentSentTime = 0;

  function handleCompositionStart() {
    ctrl?.startComposition();
    parkInputAtCursor();
  }
  function handleCompositionUpdate(e: CompositionEvent) {
    ctrl?.updateComposition(e.data);
  }
  function handleCompositionEnd(e: CompositionEvent) {
    ctrl?.finishComposition();
    const data = e.data ?? '';
    // Clear the textarea so a late `input` can't resend the committed text.
    if (hiddenInput) hiddenInput.value = '';
    if (!data) return;
    // §1 If the live `input` stream already emitted these exact characters
    // (iOS English predictive streams each letter, then fires compositionend
    // with the whole word on space), committing again duplicates the word.
    // `trimEnd()` tolerates the space `input` landing before OR after
    // compositionend; clear the buffer on a hit so the re-commit is skipped.
    if (Date.now() - recentSentTime < RECENT_SENT_WINDOW_MS && recentSent.trimEnd().endsWith(data)) {
      recentSent = '';
      return;
    }
    // If an `input` already emitted this exact commit (some IMEs fire `input`
    // before `compositionend`), don't send it again.
    if (data === lastInputText && Date.now() - lastInputTime < IME_DUP_WINDOW_MS) {
      lastInputText = '';
      return;
    }
    ctrl?.commitText(data);
    // Arm dedup for the trailing `input` event that normally follows.
    imeCommitExpect = data;
    imeCommitExpectTime = Date.now();
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
    if (!text) return;
    const inputType = (e as InputEvent).inputType || '';
    // §1 Robust IME-commit dedup: after `compositionend` the browser re-inserts
    // the committed text as a non-composing `input` whose inputType is
    // `insertCompositionText`. handleCompositionEnd already sent it via
    // commitText, so swallow this echo by TYPE — independent of the fragile
    // content/time window that mis-fires on slow mobile (→ 大量重复语句).
    if (inputType === 'insertCompositionText') return;
    // Fallback content/time window for IMEs that report a plain inputType.
    if (text === imeCommitExpect && Date.now() - imeCommitExpectTime < IME_DUP_WINDOW_MS) {
      imeCommitExpect = '';
      return;
    }
    // §1 Autocorrect / predictive replacement (iOS fires this on space /
    // punctuation to swap the typed word for a suggestion). The literal
    // characters were already streamed live, so applying the replacement
    // duplicates the word; terminals shouldn't silently rewrite input → drop it
    // and keep what the user literally typed.
    if (inputType === 'insertReplacementText') return;
    // §2 Sticky on-screen modifier armed → form a chord (Ctrl+C …) per character
    // instead of sending the literal text. One-shot: consumed after this key, so
    // the floating Ctrl/Alt/Shift finally combine with soft-keyboard characters.
    if (anyMod()) {
      const sm = consumeMods();
      for (const ch of text) {
        const bytes = ctrl.encodeKey(ch, sm.ctrl, sm.alt, sm.shift, false);
        onStdin(bytes.length > 0 ? td.decode(bytes) : ch);
      }
      return;
    }
    onStdin(text);
    lastInputText = text;
    lastInputTime = Date.now();
    // Track the literal stream so a following compositionend can detect it
    // already sent these chars (see handleCompositionEnd §1).
    recentSent = (Date.now() - recentSentTime < RECENT_SENT_WINDOW_MS ? recentSent : '') + text;
    recentSentTime = Date.now();
  }

  // ── Keyboard ──
  function handleKeydown(e: KeyboardEvent) {
    if (ctrl?.isComposing || e.isComposing) return;
    if (!paneId || !ctrl) return;
    // Unmodified printable keys flow into the hidden textarea; its `input` event
    // emits them (keeps IME + mobile prediction working, avoids double-send).
    if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) return;
    // Clipboard: handle paste/copy before the generic ctrl/meta passthrough.
    // Ctrl/Cmd+V reads the clipboard directly — the hidden input's native paste
    // only fires when it happens to hold focus, so desktop paste was unreliable.
    // Ctrl/Cmd+C (incl. Ctrl+Shift+C) copies an active selection; with no
    // selection it falls through to send ^C (interrupt), as a terminal should.
    if ((e.ctrlKey || e.metaKey) && (e.key === 'v' || e.key === 'V')) {
      e.preventDefault();
      void pasteFromClipboard();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === 'c' || e.key === 'C') && ctrl.hasSelection()) {
      e.preventDefault();
      void copySelection();
      return;
    }
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

  /** Encode arbitrary text as a bracketed paste and forward it to the host. */
  function sendPaste(text: string) {
    if (!ctrl || !text) return;
    const bytes = ctrl.encodePaste(text);
    if (bytes.length > 0) onStdin(td.decode(bytes));
  }

  /** Read the system clipboard and paste it. Driven by Ctrl/Cmd+V — the keydown
   *  is the user gesture the Clipboard API requires, and the LAN serves over TLS
   *  (secure context), so readText() is permitted. */
  async function pasteFromClipboard() {
    if (!ctrl) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) sendPaste(text);
    } catch { /* clipboard blocked: no permission / insecure context */ }
  }

  /** Copy the active selection to the system clipboard (desktop Ctrl/Cmd+C). */
  async function copySelection() {
    if (!ctrl) return;
    const text = ctrl.getSelectionText();
    if (!text) return;
    try { await navigator.clipboard.writeText(text); } catch { /* clipboard blocked */ }
    ctrl.clearSelection();
  }

  // Native paste fallback (right-click → paste, middle-click) on the focused
  // hidden textarea. Ctrl/Cmd+V is handled in handleKeydown instead.
  function handlePaste(e: ClipboardEvent) {
    if (!paneId || !ctrl) return;
    e.preventDefault();
    const text = e.clipboardData?.getData('text') ?? '';
    sendPaste(text);
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
    }
  }

  function handleMouseMove(e: MouseEvent) {
    if (!ctrl) return;
    if (mouseSelecting) {
      const cell = ctrl.clientToCell(e.clientX, e.clientY);
      if (cell) ctrl.extendSelection(cell.row, cell.col);
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
      hasSelectionState = !!ctrl.hasSelection();
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
    if (ctrl && paneId) {
      ctrl.markDirty();
      ctrl.requestResizeImmediate();
    }
  });

  // Track keyboard show/hide via visualViewport AND drive the auto-refit.
  //
  // The container ResizeObserver catches box changes, but a real-device /
  // CDP-emulated viewport change (orientation, browser-chrome collapse, address
  // bar show/hide) changes the *visible* viewport without always resizing the
  // flex container synchronously — so without this the canvas can stay clipped
  // until a manual refresh. requestResize() recomputes dims from the post-layout
  // rect + current DPR and, when the grid changed, claims the new size on the
  // host (full reflow). It's debounced + idempotent, so keyboard show/hide that
  // doesn't change the grid is a cheap no-op.
  // ── Dynamic keyboard avoidance ──
  // Lift the canvas only as far as needed to keep the input cursor clear of the
  // soft keyboard: full lift when the cursor sits at the bottom of the screen,
  // none when it's already above the keyboard, partial in between.
  function computeCanvasOffset(kbHeight: number): number {
    if (kbHeight <= 0) return 0;
    const vv = window.visualViewport;
    if (!vv || !ctrl || !canvasEl) return kbHeight;
    const cur = ctrl.getCursorPixel();
    if (!cur) return kbHeight; // cursor position unknown → safe full lift
    // Canvas natural top in viewport coords (undo the current upward transform).
    const rect = canvasEl.getBoundingClientRect();
    const naturalTop = rect.top + keyboardOffset;
    const cursorBottom = naturalTop + cur.y + cur.h;
    // Obstruction top = soft-keyboard top (vv.height), minus the quick-key bar
    // height when it's shown (the bar sits above the soft keyboard).
    const obstructionTop = (vv.height || 0) - (showKeyboard ? VK_BAR_HEIGHT : 0);
    const need = cursorBottom + KB_CURSOR_MARGIN - obstructionTop;
    return Math.max(0, Math.min(kbHeight, need));
  }

  function updateKeyboardMetrics() {
    const vv = window.visualViewport;
    if (!vv) return;
    const kh = Math.max(0, window.innerHeight - (vv.height || 0));
    const wasUp = keyboardHeight > 0;
    keyboardHeight = kh;
    keyboardOffset = computeCanvasOffset(kh);
    // §B keyboard just raised → snap to the live grid so the prompt is visible.
    if (kh > 0 && !wasUp) ctrl?.scrollToBottom();
  }

  // Recompute the dynamic offset when the cursor likely moved (input / output)
  // while the keyboard is up — visualViewport 'resize' only fires on show/hide.
  let _kbRaf = 0;
  function scheduleKbRecompute() {
    if (keyboardHeight <= 0 || _kbRaf) return;
    _kbRaf = requestAnimationFrame(() => {
      _kbRaf = 0;
      keyboardOffset = computeCanvasOffset(keyboardHeight);
    });
  }

  $effect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    function onViewportResize() {
      updateKeyboardMetrics();
      ctrl?.requestResize();
    }
    vv.addEventListener('resize', onViewportResize);
    onViewportResize();
    return () => vv.removeEventListener('resize', onViewportResize);
  });

  // orientationchange fires the most disruptive grid change; the visualViewport
  // 'resize' may lag a frame behind the new layout on some browsers, so refit
  // explicitly too (idempotent + debounced — at most one extra fitPane).
  $effect(() => {
    function onOrientation() { ctrl?.requestResize(); }
    window.addEventListener('orientationchange', onOrientation);
    return () => window.removeEventListener('orientationchange', onOrientation);
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
  style="transform: translateY(-{keyboardOffset}px)"
>
  {#if !ready}
    <div class="loading">{$t('mobile.initializingTerminal')}</div>
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

  <!-- §D Floating copy pill — shown while a Shell-mode selection exists. Stops
       touch propagation so tapping it copies instead of clearing the selection
       (the container's tap handler clears). -->
  {#if hasSelectionState}
    <button
      class="copy-pill"
      onclick={copyAndClear}
      ontouchstart={(e) => e.stopPropagation()}
      ontouchend={(e) => e.stopPropagation()}
    >{$t('mobile.copy')}</button>
  {/if}
</div>

{#if showKeyboard}
  <VirtualKeyboard keyboardOffset={keyboardHeight} onKey={handleVirtualKey} onArm={focusInput} />
{/if}

<style>
  .container{position:relative;flex:1;overflow:hidden;background:var(--rg-bg);touch-action:manipulation;transition:transform .2s ease}
  .term-canvas{display:block;width:100%;height:100%;touch-action:none}
  .term-canvas.hidden{opacity:0}
  /* Near-invisible input sink parked at the cursor. pointer-events:none keeps it
     from stealing canvas clicks. Opacity must be >0 so the IME candidate window
     (Windows 拼音 / 搜狗 / 微软 IME) anchors to a detectable element. */
  .hidden-input{position:absolute;top:0;left:0;width:1px;height:1em;margin:0;padding:0;border:0;
    opacity:0.01;pointer-events:none;resize:none;overflow:hidden;white-space:nowrap;z-index:5;
    background:transparent;color:transparent;caret-color:transparent;outline:none;font:inherit}
  .loading{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;color:var(--rg-fg-muted);font-size:14px}
  .copy-pill{position:absolute;top:8px;right:8px;z-index:6;display:flex;align-items:center;justify-content:center;height:32px;padding:0 16px;border:1px solid var(--rg-accent);border-radius:16px;background:color-mix(in srgb,var(--rg-accent) 22%,var(--rg-surface));color:var(--rg-fg);font-size:13px;font-weight:600;cursor:pointer;box-shadow:0 4px 14px -2px rgba(0,0,0,.5);-webkit-tap-highlight-color:transparent}
  .copy-pill:active{background:color-mix(in srgb,var(--rg-accent) 36%,var(--rg-surface))}
</style>