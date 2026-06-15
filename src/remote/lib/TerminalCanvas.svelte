<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import { TerminalController } from './terminalController';
  import { anyMod, consumeMods } from './modState.svelte';
  import { keyboardShiftPx } from './keyboardOffset';

  let { paneId, onStdin, onResize, onHostClipboard, selectionMode = $bindable(false), backendName = $bindable('Canvas2D') }: {
    paneId: string | null;
    onStdin: (data: string) => void;
    onResize?: (paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void;
    /** Mirror a copied selection onto the desktop host's clipboard (so the host's
     *  native Ctrl+V paste picks it up). The control end's copy writes BOTH. */
    onHostClipboard?: (text: string) => void;
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

  // Keyboard offset (mobile): when the system soft keyboard appears, the canvas
  // is pushed up by exactly enough to seat the INPUT ROW just above the keyboard
  // top — computed from the cursor's pixel position, NOT a blind full-keyboard
  // shift (that over-shifted by the bottom bar's height, lifting the input line
  // well above the keyboard).
  let keyboardOffset = $state(0);
  // §kb-stable (2026-06-15): the vertical gap (CSS px) between the canvas's
  // BOTTOM edge and the layout-viewport bottom — i.e. the bottom tab bar + safe
  // area. Measured ONLY while the keyboard is hidden (transform is 0, so the
  // bounding rect is the canvas's natural position). The keyboard-offset formula
  // reads this cached value instead of the live, transform-affected
  // `getBoundingClientRect().top`, which is what made the offset spiral: the soft
  // keyboard slide-in fires many visualViewport `resize` events, and the previous
  // `naturalTop = rect.top + keyboardOffset` undid the ANIMATING transition with
  // the TARGET offset → the mismatch flung the canvas off-screen (blank terminal)
  // and thrashed the page (apparent freeze). With this gap cached, the formula is
  // fully transform-independent, so recomputing per resize converges cleanly.
  let gapBelowCanvas = 0;
  // True while the soft keyboard is up. Tracks the hidden→shown edge so the
  // one-shot `scrollToBottom()` (snap to the live prompt) fires exactly once per
  // show — not on every intermediate resize event during the slide-in.
  let keyboardVisible = false;

  // Touch state. Single-finger swipe = scroll (simulates mouse wheel).
  // In TUI mode (mouse reporting), the scroll is forwarded to the app.
  // Single-finger tap = focus (+ click-through in mouse-reporting apps).
  let touchStartY = 0;
  let touchStartX = 0;
  let touchScrollAccum = 0;
  let touchLastY = 0;
  let touchStartTime = 0;
  const TOUCH_DRAG_THRESHOLD_PX = 8;
  const TOUCH_TAP_MAX_MS = 250;

  let hasSelectionState = $state(false);    // drives the floating copy pill
  let selDragging = false;                  // selection drag in progress

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

  onDestroy(() => {
    if (gapRemeasureTimer) clearTimeout(gapRemeasureTimer);
    ctrl?.destroy();
  });



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
  export function feedUtf8(bytes: Uint8Array) { ctrl?.feed(bytes); }
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

  // ── Virtual Keyboard (called from MainApp header) ──
  export function handleVirtualKey(key: string, ctrlKey: boolean, alt: boolean, shift: boolean) {
    if (!paneId || !ctrl) return;
    const bytes = ctrl.encodeKey(key, ctrlKey, alt, shift, false);
    if (bytes.length > 0) { onStdin(td.decode(bytes)); return; }
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

  /** Write `text` to the control device's clipboard, with a legacy
   *  `execCommand('copy')` fallback for mobile browsers that reject the async
   *  Clipboard API (older WebViews / non-secure quirks). */
  async function writeClipboard(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch { /* fall through to the legacy textarea path */ }
    try {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed';
      ta.style.top = '0';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      // finally so a throwing execCommand can't leak the textarea into the DOM.
      try { document.execCommand('copy'); }
      finally { document.body.removeChild(ta); }
    } catch { /* clipboard truly unavailable — nothing more we can do */ }
  }

  /** Copy the selection to the control device's clipboard, then clear it.
   *  §copy-no-interrupt: copying must NOT send `\x03` to the PTY — the old
   *  unconditional ^C cancelled the shell line / SIGINT'd the foreground process
   *  every time you copied. Copy is a read-only clipboard action now. */
  function copyAndClear() {
    if (!ctrl) return;
    try {
      const text = ctrl.getSelectionText();
      if (text) {
        void writeClipboard(text);   // control device (this phone/browser)
        onHostClipboard?.(text);     // + desktop host, for its native Ctrl+V paste
      }
    } catch { /* kernel may have no selection */ }
    ctrl.clearSelection();
    hasSelectionState = false;
  }

  /** Paste arbitrary text (the control device's clipboard) into the terminal as
   *  a bracketed paste. Driven by the bottom-bar paste button in MainApp — that
   *  onclick is the user gesture the Clipboard API requires, and the LAN/cloud
   *  link is a secure context, so the read in MainApp is permitted. */
  export function pasteText(text: string) {
    sendPaste(text);
  }

  function handleTouchStart(e: TouchEvent) {
    if (!ctrl) return;
    if (e.touches.length !== 1) return;
    const t = e.touches[0];
    touchStartY = t.clientY;
    touchStartX = t.clientX;
    touchLastY = t.clientY;
    touchScrollAccum = 0;
    touchStartTime = Date.now();
    // §select-as-mouse: the select toggle SIMULATES A MOUSE — it just emits mouse
    // signals and lets the receiving terminal decide what to do (parity with the
    // desktop mouse path, handleMouseDown). When the app captures the mouse
    // (mouse-reporting TUI: vim/htop/tmux…) we forward a press and the TUI owns the
    // gesture/selection. ONLY a plain shell — which doesn't accept mouse reporting
    // — falls back to LOCAL text selection + copy pill.
    if (selectionMode) {
      const cell = ctrl.clientToCell(t.clientX, t.clientY);
      if (cell) {
        if (ctrl.isMouseReporting()) {
          const bytes = ctrl.encodeMouse(cell.row, cell.col, 0, 0, false, false, false); // press
          if (bytes.length > 0) onStdin(td.decode(bytes));
        } else {
          ctrl.startSelection(cell.row, cell.col);
        }
      }
    }
  }

  function handleTouchMove(e: TouchEvent) {
    if (!ctrl || e.touches.length !== 1) return;
    const t = e.touches[0];
    const moved = Math.abs(t.clientY - touchStartY) + Math.abs(t.clientX - touchStartX);
    if (moved < TOUCH_DRAG_THRESHOLD_PX) return;
    e.preventDefault();
    if (selectionMode) {
      selDragging = true;
      const cell = ctrl.clientToCell(t.clientX, t.clientY);
      // §select-as-mouse: mouse-reporting TUI → motion report (the TUI extends its
      // own selection); plain shell → local text selection.
      if (cell) {
        if (ctrl.isMouseReporting()) {
          const bytes = ctrl.encodeMouse(cell.row, cell.col, 0, 2, false, false, false); // drag
          if (bytes.length > 0) onStdin(td.decode(bytes));
        } else {
          ctrl.extendSelection(cell.row, cell.col);
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
    if (!ctrl) return;
    if (selectionMode) {
      const wasDragging = selDragging;
      selDragging = false;
      const cell = touch ? ctrl.clientToCell(touch.clientX, touch.clientY) : null;
      if (ctrl.isMouseReporting()) {
        // §select-as-mouse: complete the simulated gesture with a release — a tap
        // becomes a click, a drag becomes a drag-end. The TUI handles the rest.
        if (cell) {
          const bytes = ctrl.encodeMouse(cell.row, cell.col, 0, 1, false, false, false); // release
          if (bytes.length > 0) onStdin(td.decode(bytes));
        }
      } else if (wasDragging) {
        // Plain shell: finish the local text selection + surface the copy pill.
        ctrl.endSelection();
        hasSelectionState = !!ctrl.hasSelection();
      } else {
        // A tap in shell selection mode clears any existing selection.
        ctrl.clearSelection();
        hasSelectionState = false;
      }
      // §select-tap-keyboard: a TAP (not a drag) in selection mode also raises the
      // soft keyboard so you can type without first leaving select mode. Drags are
      // the selection gesture itself, so they don't pop the keyboard.
      if (!wasDragging) focusInput();
      return;
    }
    const elapsed = Date.now() - touchStartTime;
    if (elapsed >= TOUCH_TAP_MAX_MS) return;
    // Light tap clears an existing selection (and re-raises the keyboard).
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
    await writeClipboard(text);    // control device
    onHostClipboard?.(text);       // + desktop host (native Ctrl+V paste)
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
  // Small gap so the input line isn't flush against the keyboard's top edge.
  const KB_GAP_PX = 8;

  /** Offset (CSS px) to translate the canvas up so the cursor's INPUT ROW sits
   *  just above the keyboard. Anchors the cursor cell's BOTTOM at (keyboard top −
   *  gap); falls back to the canvas bottom row when the cursor pixel is
   *  unavailable. Returns 0 when the keyboard is hidden.
   *
   *  §kb-stable: every term here is TRANSFORM-INDEPENDENT, so recomputing on each
   *  visualViewport `resize` during the keyboard slide-in converges instead of
   *  spiraling (the earlier `rect.top + keyboardOffset` undid the in-flight CSS
   *  transition with the target offset → off-screen canvas + page thrash):
   *   • kh                    keyboard height = innerHeight − visualViewport.height
   *   • gapBelowCanvas        canvas-bottom → layout-bottom gap, cached while hidden
   *   • cursorFromCanvasBottom canvas height − cursor cell bottom (intrinsic to the
   *                            canvas, unaffected by a translateY)
   *  offset = kh + gap_to_keyboard − gapBelowCanvas − cursorFromCanvasBottom. */
  function computeKeyboardOffset(): number {
    const vv = window.visualViewport;
    if (!vv || !canvasEl) return 0;
    const kh = Math.max(0, window.innerHeight - (vv.height || 0));
    const canvasH = canvasEl.clientHeight; // layout height — a translateY can't change it
    const cur = ctrl?.getCursorPixel();
    const cursorBottom = cur ? cur.y + cur.h : canvasH;
    return keyboardShiftPx({
      keyboardHeightPx: kh,
      gapBelowCanvasPx: gapBelowCanvas,
      cursorFromCanvasBottomPx: Math.max(0, canvasH - cursorBottom),
      gapPx: KB_GAP_PX,
    });
  }

  /** Re-measure the stable canvas-bottom → layout-bottom gap. Safe only while the
   *  keyboard is hidden (keyboardOffset === 0): with no transform applied the
   *  bounding rect reflects the canvas's natural position. */
  function measureGapBelowCanvas(): void {
    if (!canvasEl || keyboardOffset !== 0) return;
    const r = canvasEl.getBoundingClientRect();
    gapBelowCanvas = Math.max(0, Math.round(window.innerHeight - r.bottom));
  }

  /** Re-measure once the un-shift transition (.2s) has settled, so the gap reads
   *  the canvas's natural box rather than a mid-animation one. Self-heals a
   *  mount-time transient (canvas not yet laid out) and layout drift. */
  let gapRemeasureTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleGapRemeasure(): void {
    if (gapRemeasureTimer) clearTimeout(gapRemeasureTimer);
    gapRemeasureTimer = setTimeout(() => {
      gapRemeasureTimer = null;
      measureGapBelowCanvas();
    }, 260);
  }

  // ── Cursor-anchored keyboard offset ──
  // When the system keyboard appears, push the canvas up just enough to keep the
  // input row visible above it. DO NOT call requestResize() here — the transform
  // moves the canvas without changing the terminal grid; resize only on real
  // viewport/orientation change.
  $effect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    function onViewportResize() {
      if (!vv) return;
      const kh = Math.max(0, window.innerHeight - (vv.height || 0));
      if (kh <= 0) {
        // Keyboard hidden: drop the shift. Do NOT re-measure the gap here — the
        // un-shift transition is still animating, so the bounding rect would read
        // a mid-animation (still-shifted) box. The gap is static layout, seeded on
        // mount and refreshed on orientationchange (both transform-free moments).
        keyboardVisible = false;
        keyboardOffset = 0;
        scheduleGapRemeasure(); // refresh once the un-shift settles (guarded)
        return;
      }
      // First show: snap the terminal to the prompt so the cursor we anchor on is
      // the live input row. Done once per show — not on every slide-in resize.
      if (!keyboardVisible) {
        keyboardVisible = true;
        ctrl?.scrollToBottom();
      }
      keyboardOffset = computeKeyboardOffset();
    }
    vv.addEventListener('resize', onViewportResize);
    // Seed the gap measurement, then schedule a settled re-measure (in case the
    // canvas isn't fully laid out yet), then capture any already-open keyboard.
    measureGapBelowCanvas();
    scheduleGapRemeasure();
    onViewportResize();
    return () => vv.removeEventListener('resize', onViewportResize);
  });

  // orientationchange fires the most disruptive grid change; the visualViewport
  // 'resize' may lag a frame behind the new layout on some browsers, so refit
  // explicitly too (idempotent + debounced — at most one extra fitPane).
  $effect(() => {
    function onOrientation() {
      ctrl?.requestResize();
      // Layout changed → the canvas-bottom gap (bottom bar + safe area) may have
      // changed too. Re-measure after the rotation settles (guarded so it only
      // reads a transform-free box).
      scheduleGapRemeasure();
    }
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