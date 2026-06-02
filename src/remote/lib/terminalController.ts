import init, { TerminalKernel, RenderHandle, SurfaceHostHandle } from '@ridge/term-wasm';
import wasmUrl from '@ridge/term-wasm/ridge_term_bg.wasm?url';

export interface TermOpts {
  fontSize?: number;
  scrollback?: number;
  fontFamily?: string;
}

export const FONT_STACK = '"JetBrains Mono","Cascadia Code","SF Mono",ui-monospace,Consolas,"SimHei","Heiti SC","Microsoft YaHei","Apple Color Emoji","Segoe UI Emoji","Noto Color Emoji",monospace';

const FEED_CHUNK_BYTES = 16 * 1024;
const FEED_PER_CALL_BUDGET_MS = 4;
const COALESCE_WINDOW_MS = 8;
const RESIZE_DEBOUNCE_MS = 500;

export class TerminalController {
  private kernel: TerminalKernel;
  private renderHandle: RenderHandle;
  private canvas: HTMLCanvasElement;
  private container: HTMLDivElement;
  private surfaceHost: SurfaceHostHandle | null;
  private themeBg: Uint8Array;
  private rafId: number | null = null;
  private sleepTimerId: ReturnType<typeof setTimeout> | null = null;
  private destroyed = false;

  private fontSize: number;
  private scrollback: number;
  private fontFamily: string;
  private cols = 80;
  private rows = 24;
  private cellW = 0;
  private cellH = 0;

  // ── Dirty-detection render loop ──
  private needsRender = true;
  private visible = true;
  private focused = false;
  // True only while `tick` is executing, so `wake()` never schedules a second
  // frame on top of the one `scheduleNextFrame` is about to queue.
  private inTick = false;

  // ── Feed coalescing ──
  private feedDeferred: Uint8Array[] = [];
  private coalesceBuffer: Uint8Array[] = [];
  private coalesceTimer: ReturnType<typeof setTimeout> | null = null;

  // ── Resize ──
  private resizeTimer: ReturnType<typeof setTimeout> | null = null;
  private lastDpr = 1;

  // ── IME ──
  private _isComposing = false;
  private imeAnchorRow = -1;
  private imeAnchorCol = -1;

  // ── Selection ──
  isSelecting = false;
  private selAnchorRow = 0;
  private selAnchorCol = 0;

  // ── Callbacks ──
  onStdin: ((data: string) => void) | null = null;
  onResize: ((rows: number, cols: number, pixelWidth: number, pixelHeight: number) => void) | null = null;

  // ── Document event listeners (stored for cleanup) ──
  private _visibilityHandler: (() => void) | null = null;
  private _fontHandler: (() => void) | null = null;

  private static initialized = false;

  private constructor(
    kernel: TerminalKernel,
    renderHandle: RenderHandle,
    surfaceHost: SurfaceHostHandle | null,
    canvas: HTMLCanvasElement,
    container: HTMLDivElement,
    opts: TermOpts,
  ) {
    this.kernel = kernel;
    this.renderHandle = renderHandle;
    this.surfaceHost = surfaceHost;
    this.canvas = canvas;
    this.container = container;
    this.themeBg = new Uint8Array([0x1e, 0x1e, 0x2e, 0xff]);
    this.fontSize = opts.fontSize ?? 14;
    this.scrollback = opts.scrollback ?? 5000;
    this.fontFamily = opts.fontFamily ?? FONT_STACK;
  }

  static async create(canvas: HTMLCanvasElement, container: HTMLDivElement, opts: TermOpts = {}): Promise<TerminalController> {
    if (!TerminalController.initialized) {
      await init(wasmUrl);
      TerminalController.initialized = true;
    }
    const rows = 24;
    const cols = 80;
    const scrollback = opts.scrollback ?? 5000;
    const kernel = new TerminalKernel(rows, cols, scrollback);

    let surfaceHost: SurfaceHostHandle | null = null;
    try {
      surfaceHost = await SurfaceHostHandle.init(canvas);
    } catch {
      // WebGPU adapter unavailable — will fall back to Canvas2D
    }
    // `newWithWebgpuFirst` consumes its `host` argument
    // (wasm-bindgen `Option<T>` moves the JS wrapper into Rust and frees
    // it on return). Clone the wrapper so the controller's stored handle
    // stays alive across the render loop.
    const hostArg = surfaceHost?.clone() ?? surfaceHost;
    const renderHandle = await RenderHandle.newWithWebgpuFirst(canvas, hostArg);
    renderHandle.applyDefaultTheme();

    const controller = new TerminalController(kernel, renderHandle, surfaceHost, canvas, container, opts);
    controller.fitPane();
    controller.startRenderLoop();
    controller.setupVisibilityHandler();
    controller.setupFontHandler();
    return controller;
  }

  // ── Render loop: blink-deadline driven, idle sleep when static ──
  //
  // A single `tick` is kept alive by exactly one pending wake-up at a time —
  // either a queued rAF (`rafId`) or an idle-sleep timer (`sleepTimerId`).
  // When the terminal is static the loop sleeps up to a blink interval; the
  // crucial part is that any new content/dirty (feed, delta, scroll, IME…)
  // calls `wake()`, which cancels that sleep and renders on the next frame —
  // otherwise idle→active transitions stalled up to ~520ms ("慢半拍").

  private startRenderLoop() {
    if (this.destroyed) return;
    if (this.rafId !== null || this.sleepTimerId !== null) return;
    this.rafId = requestAnimationFrame(this.tick);
  }

  private tick = () => {
    this.rafId = null;
    if (this.destroyed) return;
    this.inTick = true;
    this.flushDeferred();
    if (this.needsRender || this.blinkDue()) {
      if (this.visible) {
        const hostOpened = this.surfaceHost ? this.surfaceHost.beginFrame(this.themeBg) : false;
        try {
          this.renderHandle.render(this.kernel);
        } finally {
          if (hostOpened) {
            try { this.surfaceHost!.endFrame(); } catch {}
          }
        }
      }
      this.needsRender = false;
    }
    this.inTick = false;
    this.scheduleNextFrame();
  };

  private scheduleNextFrame() {
    if (this.destroyed) return;
    const msUntilBlink = this.renderHandle.nextBlinkDeadlineMs(this.kernel, Date.now());
    const capped = Math.min(msUntilBlink, 1000);
    if (capped < 16) {
      this.rafId = requestAnimationFrame(this.tick);
    } else {
      this.sleepTimerId = setTimeout(() => {
        this.sleepTimerId = null;
        if (this.destroyed) return;
        this.rafId = requestAnimationFrame(this.tick);
      }, capped - 8);
    }
  }

  private blinkDue(): boolean {
    return this.renderHandle.nextBlinkDeadlineMs(this.kernel, Date.now()) < 16;
  }

  markDirty() {
    this.wake(true);
  }

  /**
   * Mark the surface dirty (when `dirty`) and ensure a frame is pending. If the
   * loop is asleep, cancel the sleep timer and render on the next animation
   * frame; if a frame is already queued, just leave the dirty flag set. No-op
   * while `tick` runs — `scheduleNextFrame` queues the next wake-up itself.
   */
  private wake(dirty = false) {
    this.needsRender ||= dirty;
    if (this.destroyed || this.inTick) return;
    if (this.rafId !== null) return;
    if (this.sleepTimerId !== null) {
      clearTimeout(this.sleepTimerId);
      this.sleepTimerId = null;
    }
    this.rafId = requestAnimationFrame(this.tick);
  }

  // ── Feed with coalescing and time-budget chunking ──

  feed(data: Uint8Array) {
    if (data.length === 0) return;
    const hasEscape = data.some(b => b === 0x1b);
    if (hasEscape && this.coalesceBuffer.length === 0) {
      this.coalesceBuffer.push(data);
      if (!this.coalesceTimer) {
        this.coalesceTimer = setTimeout(() => this.flushCoalesce(), COALESCE_WINDOW_MS);
      }
      return;
    }
    if (this.coalesceTimer) {
      this.coalesceBuffer.push(data);
      return;
    }
    this.feedChunked(data);
  }

  private flushCoalesce() {
    this.coalesceTimer = null;
    if (this.coalesceBuffer.length === 0) return;
    const totalLen = this.coalesceBuffer.reduce((s, c) => s + c.length, 0);
    const merged = new Uint8Array(totalLen);
    let off = 0;
    for (const c of this.coalesceBuffer) {
      merged.set(c, off);
      off += c.length;
    }
    this.coalesceBuffer.length = 0;
    this.feedChunked(merged);
  }

  private feedChunked(data: Uint8Array) {
    const start = performance.now();
    let offset = 0;
    while (offset < data.length) {
      const chunk = data.subarray(offset, offset + FEED_CHUNK_BYTES);
      this.kernel.feed(chunk);
      offset += chunk.length;
      if (performance.now() - start > FEED_PER_CALL_BUDGET_MS) {
        this.feedDeferred.push(data.subarray(offset));
        this.markDirty();
        return;
      }
    }
    const resp = this.kernel.takePendingResponse();
    if (resp.length > 0 && this.onStdin) {
      this.onStdin(new TextDecoder().decode(resp));
    }
    this.markDirty();
  }

  private flushDeferred() {
    const deferred = this.feedDeferred.splice(0);
    for (const d of deferred) {
      this.kernel.feed(d);
    }
    const resp = this.kernel.takePendingResponse();
    if (resp.length > 0 && this.onStdin) {
      this.onStdin(new TextDecoder().decode(resp));
    }
  }

  applyDelta(bytes: Uint8Array) {
    this.kernel.applyDeltaFrame(bytes);
    this.markDirty();
  }

  // ── External resize (called when server notifies of PTY resize) ──

  kernelResize(rows: number, cols: number) {
    if (this.destroyed) return;
    this.rows = rows;
    this.cols = cols;
    this.kernel.resize(rows, cols);
    this.markDirty();
  }

  // ── Fit pane with DPR drift detection, cell quantization ──

  fitPane() {
    if (this.destroyed) return;
    const dpr = window.devicePixelRatio || 1;
    const w = this.container.clientWidth;
    const h = this.container.clientHeight;
    if (w <= 0 || h <= 0) return;

    this.canvas.width = Math.round(w * dpr);
    this.canvas.height = Math.round(h * dpr);
    this.canvas.style.width = w + 'px';
    this.canvas.style.height = h + 'px';

    this.renderHandle.resize(w, h, dpr);
    const dims = this.renderHandle.configure(this.fontFamily, this.fontSize, dpr);
    if (dims.length >= 2) {
      this.cellW = TerminalController.quantizeCellSize(dims[0]);
      this.cellH = TerminalController.quantizeCellSize(dims[1]);
    }
    if (this.surfaceHost) {
      try { this.surfaceHost.resize(w, h, dpr); this.surfaceHost.invalidate(); } catch {}
    }

    if (this.cellW > 0 && this.cellH > 0) {
      const newCols = Math.max(1, Math.floor(w / this.cellW));
      const newRows = Math.max(1, Math.floor(h / this.cellH));
      if (newCols !== this.cols || newRows !== this.rows || dpr !== this.lastDpr) {
        this.cols = newCols;
        this.rows = newRows;
        this.lastDpr = dpr;
        this.kernel.resize(this.rows, this.cols);
        this.onResize?.(this.rows, this.cols, Math.round(w), Math.round(h));
      }
    }
    this.markDirty();
  }

  static quantizeCellSize(px: number): number {
    return Math.round(px * 64) / 64;
  }

  // ── Public resize with debounce ──

  requestResize() {
    if (this.resizeTimer) clearTimeout(this.resizeTimer);
    this.resizeTimer = setTimeout(() => this.fitPane(), RESIZE_DEBOUNCE_MS);
  }

  requestResizeImmediate() {
    if (this.resizeTimer) clearTimeout(this.resizeTimer);
    this.fitPane();
  }

  /** Current grid + pixel size, used by the "lock size" / refresh button to
   *  claim the shared PTY at this client's viewport (refresh-pane). */
  getDims(): { rows: number; cols: number; pixelWidth: number; pixelHeight: number } {
    return {
      rows: this.rows,
      cols: this.cols,
      pixelWidth: Math.round(this.container.clientWidth),
      pixelHeight: Math.round(this.container.clientHeight),
    };
  }

  // ── Theme ──

  applyTheme(theme: Record<string, string>) {
    if (this.destroyed) return;
    this.renderHandle.applyTheme(theme);
    const bg = theme.background ?? theme['ansiBlack'] ?? '#1e1e2e';
    this.themeBg = cssColorToRgba(bg);
    if (this.surfaceHost) this.surfaceHost.invalidate();
    this.markDirty();
  }

  // ── Focus / visibility ──

  setFocused(f: boolean) {
    this.focused = f;
    this.renderHandle.setFocused(f);
    this.markDirty();
  }

  private setupVisibilityHandler() {
    this._visibilityHandler = () => {
      if (this.destroyed) return;
      this.visible = !document.hidden;
      if (this.visible) {
        this.markDirty();
        this.startRenderLoop();
      }
    };
    document.addEventListener('visibilitychange', this._visibilityHandler);
  }

  private setupFontHandler() {
    this._fontHandler = () => {
      if (this.destroyed) return;
      setTimeout(() => {
        if (!this.destroyed) this.fitPane();
      }, 250);
    };
    document.fonts?.addEventListener('loadingdone', this._fontHandler);
  }

  // ── IME ──

  startComposition() {
    this._isComposing = true;
    this.imeAnchorRow = this.kernel.cursorRow?.() ?? -1;
    this.imeAnchorCol = this.kernel.cursorCol?.() ?? -1;
  }

  updateComposition(text: string) {
    if (this.destroyed) return;
    const r = this.kernel.cursorRow?.() ?? -1;
    const c = this.kernel.cursorCol?.() ?? -1;
    const h = this.renderHandle as unknown as { setPreedit?: (t: string, r: number, c: number) => void };
    h.setPreedit?.(text, r, c);
    this.markDirty();
  }

  endComposition(text: string) {
    this._isComposing = false;
    if (this.destroyed) return;
    const h = this.renderHandle as unknown as { clearPreedit?: () => void };
    h.clearPreedit?.();
    if (text) {
      const bytes = this.kernel.encodePaste(text);
      if (bytes.length > 0 && this.onStdin) {
        this.onStdin(new TextDecoder().decode(bytes));
      }
    }
    this.imeAnchorRow = -1;
    this.imeAnchorCol = -1;
    this.markDirty();
  }

  get isComposing() { return this._isComposing; }

  get backendName(): string {
    const h = this.renderHandle as unknown as { backendName?: () => string };
    return h.backendName?.() ?? 'Canvas2D';
  }

  // ── Selection ──

  startSelection(row: number, col: number) {
    if (this.destroyed) return;
    this.isSelecting = true;
    this.selAnchorRow = row;
    this.selAnchorCol = col;
    this.kernel.clearSelection();
    this.markDirty();
  }

  extendSelection(row: number, col: number) {
    if (this.destroyed || !this.isSelecting) return;
    const absRow = this.kernel.scrollbackLen() > 0
      ? row + (this.kernel.scrollOffset() > 0 ? this.kernel.scrollOffset() : 0)
      : row;
    this.kernel.setSelectionAbs(this.selAnchorRow, this.selAnchorCol, absRow, col);
    this.markDirty();
  }

  endSelection() {
    this.isSelecting = false;
  }

  hasSelection(): boolean {
    return !this.destroyed && this.kernel.hasSelection();
  }

  getSelectionText(): string | null {
    if (this.destroyed) return null;
    return this.kernel.getSelectionText();
  }

  clearSelection() {
    if (this.destroyed) return;
    this.kernel.clearSelection();
    this.markDirty();
  }

  // ── Key encoding ──

  encodeKey(key: string, ctrl: boolean, alt: boolean, shift: boolean, meta: boolean = false): Uint8Array {
    if (this.destroyed) return new Uint8Array(0);
    return this.kernel.encodeKey(key, ctrl, alt, shift, meta);
  }

  encodeMouse(row: number, col: number, button: number, state: number, shift: boolean, alt: boolean, ctrl: boolean): Uint8Array {
    if (this.destroyed) return new Uint8Array(0);
    // Kernel signature is encodeMouse(row, col, button, action, shift, ctrl, alt)
    // — note ctrl before alt. Forward in that order so modifiers aren't swapped.
    return this.kernel.encodeMouse(row, col, button, state, shift, ctrl, alt);
  }

  encodePaste(text: string): Uint8Array {
    if (this.destroyed) return new Uint8Array(0);
    return this.kernel.encodePaste(text);
  }

  isMouseReporting(): boolean {
    if (this.destroyed) return false;
    return this.kernel.isMouseReporting();
  }

  // ── Scrolling ──

  scrollUp(lines: number) { this.kernel.scrollUp(lines); this.markDirty(); }
  scrollDown(lines: number) { this.kernel.scrollDown(lines); this.markDirty(); }
  scrollOffset(): number { return this.destroyed ? 0 : this.kernel.scrollOffset(); }
  scrollbackLen(): number { return this.destroyed ? 0 : this.kernel.scrollbackLen(); }

  // ── Coordinate mapping ──

  getCellSize(): { w: number; h: number } {
    return { w: this.cellW, h: this.cellH };
  }

  /**
   * Cursor position in CSS px relative to the canvas top-left. Used to park the
   * hidden IME textarea at the cursor so the candidate window appears in place.
   * `cellW`/`cellH` are CSS px (see `fitPane` cols/rows math), so no DPR scaling.
   */
  getCursorPixel(): { x: number; y: number; h: number } | null {
    if (this.destroyed || this.cellW <= 0 || this.cellH <= 0) return null;
    const row = this.kernel.cursorRow?.() ?? -1;
    const col = this.kernel.cursorCol?.() ?? -1;
    if (row < 0 || col < 0) return null;
    return { x: col * this.cellW, y: row * this.cellH, h: this.cellH };
  }

  clientToCell(clientX: number, clientY: number): { row: number; col: number } | null {
    if (this.cellW <= 0 || this.cellH <= 0) return null;
    const rect = this.canvas.getBoundingClientRect();
    const x = clientX - rect.left;
    const y = clientY - rect.top;
    return {
      col: Math.max(0, Math.floor(x / this.cellW)),
      row: Math.max(0, Math.floor(y / this.cellH)),
    };
  }

  // ── Search ──

  search(query: string, caseSensitive: boolean = false): number {
    if (this.destroyed) return 0;
    return this.kernel.searchSetQuery(query, caseSensitive);
  }

  searchNext(): boolean {
    if (this.destroyed) return false;
    return this.kernel.searchNext() !== 4294967295;
  }

  searchPrev(): boolean {
    if (this.destroyed) return false;
    return this.kernel.searchPrev() !== 4294967295;
  }

  // ── Cleanup ──

  destroy() {
    this.destroyed = true;
    if (this.rafId !== null) cancelAnimationFrame(this.rafId);
    if (this.sleepTimerId !== null) clearTimeout(this.sleepTimerId);
    if (this.coalesceTimer) clearTimeout(this.coalesceTimer);
    if (this.resizeTimer) clearTimeout(this.resizeTimer);
    if (this._visibilityHandler) {
      document.removeEventListener('visibilitychange', this._visibilityHandler);
      this._visibilityHandler = null;
    }
    if (this._fontHandler) {
      document.fonts?.removeEventListener('loadingdone', this._fontHandler);
      this._fontHandler = null;
    }
    this.renderHandle.free();
    this.kernel.free();
    // SurfaceHostHandle is a JS wrapper around an Rc<RefCell<SurfaceHost>>;
    // no explicit free needed — GC will collect it when the JS wrapper is dropped.
  }
}

function cssColorToRgba(css: string): Uint8Array {
  const c = css.trim();
  if (c.startsWith('#')) {
    const hex = c.slice(1);
    if (hex.length === 3) {
      const r = parseInt(hex[0] + hex[0], 16);
      const g = parseInt(hex[1] + hex[1], 16);
      const b = parseInt(hex[2] + hex[2], 16);
      return new Uint8Array([r, g, b, 255]);
    }
    if (hex.length === 6) {
      const r = parseInt(hex.slice(0, 2), 16);
      const g = parseInt(hex.slice(2, 4), 16);
      const b = parseInt(hex.slice(4, 6), 16);
      return new Uint8Array([r, g, b, 255]);
    }
    if (hex.length === 8) {
      const r = parseInt(hex.slice(0, 2), 16);
      const g = parseInt(hex.slice(2, 4), 16);
      const b = parseInt(hex.slice(4, 6), 16);
      const a = parseInt(hex.slice(6, 8), 16);
      return new Uint8Array([r, g, b, a]);
    }
  }
  return new Uint8Array([0x1e, 0x1e, 0x2e, 0xff]);
}