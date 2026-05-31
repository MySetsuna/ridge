import init, { TerminalKernel, RenderHandle } from '@ridge/term-wasm';
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
const CURSOR_BLINK_INTERVAL_MS = 530;

export class TerminalController {
  private kernel: TerminalKernel;
  private renderHandle: RenderHandle;
  private canvas: HTMLCanvasElement;
  private container: HTMLDivElement;
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
  private blinkDeadline = 0;
  private visible = true;
  private focused = false;

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
    canvas: HTMLCanvasElement,
    container: HTMLDivElement,
    opts: TermOpts,
  ) {
    this.kernel = kernel;
    this.renderHandle = renderHandle;
    this.canvas = canvas;
    this.container = container;
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
    const renderHandle = await RenderHandle.newWithWebgpuFirst(canvas);
    renderHandle.applyDefaultTheme();

    const controller = new TerminalController(kernel, renderHandle, canvas, container, opts);
    controller.fitPane();
    controller.startRenderLoop();
    controller.setupVisibilityHandler();
    controller.setupFontHandler();
    return controller;
  }

  // ── Render loop: blink-deadline driven, idle sleep when static ──

  private startRenderLoop() {
    const frame = () => {
      if (this.destroyed) return;
      this.flushDeferred();
      if (this.needsRender || this.blinkDue()) {
        if (this.visible) {
          this.renderHandle.render(this.kernel);
        }
        this.needsRender = false;
      }
      this.scheduleNextFrame();
    };
    this.rafId = requestAnimationFrame(frame);
  }

  private scheduleNextFrame() {
    if (this.destroyed) return;
    const msUntilBlink = this.blinkDeadline - performance.now();
    if (msUntilBlink <= 0) {
      this.rafId = requestAnimationFrame(() => this.startRenderLoop());
    } else if (msUntilBlink < 16) {
      this.rafId = requestAnimationFrame(() => this.startRenderLoop());
    } else {
      this.sleepTimerId = setTimeout(() => {
        if (this.destroyed) return;
        this.rafId = requestAnimationFrame(() => this.startRenderLoop());
      }, msUntilBlink - 8);
    }
  }

  private blinkDue(): boolean {
    const now = performance.now();
    if (now >= this.blinkDeadline) {
      this.blinkDeadline = now + CURSOR_BLINK_INTERVAL_MS;
      return true;
    }
    return false;
  }

  markDirty() {
    this.needsRender = true;
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
        this.needsRender = true;
        return;
      }
    }
    const resp = this.kernel.takePendingResponse();
    if (resp.length > 0 && this.onStdin) {
      this.onStdin(new TextDecoder().decode(resp));
    }
    this.needsRender = true;
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
    this.needsRender = true;
  }

  // ── External resize (called when server notifies of PTY resize) ──

  kernelResize(rows: number, cols: number) {
    if (this.destroyed) return;
    this.rows = rows;
    this.cols = cols;
    this.kernel.resize(rows, cols);
    this.needsRender = true;
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
    this.needsRender = true;
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

  // ── Theme ──

  applyTheme(theme: Record<string, string>) {
    if (this.destroyed) return;
    this.renderHandle.applyTheme(theme);
    this.needsRender = true;
  }

  // ── Focus / visibility ──

  setFocused(f: boolean) {
    this.focused = f;
    this.renderHandle.setFocused(f);
    this.needsRender = true;
  }

  private setupVisibilityHandler() {
    this._visibilityHandler = () => {
      if (this.destroyed) return;
      this.visible = !document.hidden;
      if (this.visible) {
        this.needsRender = true;
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
    this.needsRender = true;
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
    this.needsRender = true;
  }

  get isComposing() { return this._isComposing; }

  // ── Selection ──

  startSelection(row: number, col: number) {
    if (this.destroyed) return;
    this.isSelecting = true;
    this.selAnchorRow = row;
    this.selAnchorCol = col;
    this.kernel.clearSelection();
    this.needsRender = true;
  }

  extendSelection(row: number, col: number) {
    if (this.destroyed || !this.isSelecting) return;
    const absRow = this.kernel.scrollbackLen() > 0
      ? row + (this.kernel.scrollOffset() > 0 ? this.kernel.scrollOffset() : 0)
      : row;
    this.kernel.setSelectionAbs(this.selAnchorRow, this.selAnchorCol, absRow, col);
    this.needsRender = true;
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
    this.needsRender = true;
  }

  // ── Key encoding ──

  encodeKey(key: string, ctrl: boolean, alt: boolean, shift: boolean, meta: boolean = false): Uint8Array {
    if (this.destroyed) return new Uint8Array(0);
    return this.kernel.encodeKey(key, ctrl, alt, shift, meta);
  }

  encodeMouse(row: number, col: number, button: number, state: number, shift: boolean, alt: boolean, ctrl: boolean): Uint8Array {
    if (this.destroyed) return new Uint8Array(0);
    return this.kernel.encodeMouse(row, col, button, state, shift, alt, ctrl);
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

  scrollUp(lines: number) { this.kernel.scrollUp(lines); this.needsRender = true; }
  scrollDown(lines: number) { this.kernel.scrollDown(lines); this.needsRender = true; }
  scrollOffset(): number { return this.destroyed ? 0 : this.kernel.scrollOffset(); }
  scrollbackLen(): number { return this.destroyed ? 0 : this.kernel.scrollbackLen(); }

  // ── Coordinate mapping ──

  getCellSize(): { w: number; h: number } {
    return { w: this.cellW, h: this.cellH };
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
  }
}