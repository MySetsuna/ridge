/**
 * Frame-budgeted PTY delivery for Remote.
 *
 * DataChannel callbacks run on the browser main thread. Feeding every pane
 * frame synchronously lets a noisy PTY consume one 4ms kernel slice after
 * another before the browser can dispatch the next key event. This scheduler
 * keeps a bounded per-pane FIFO, drains the active pane first, then yields at
 * a frame budget so input/paint tasks get a turn.
 */

export const DEFAULT_PANE_FEED_MAX_BYTES = 512 * 1024;
export const DEFAULT_PANE_FEED_FRAME_BUDGET_MS = 4;
export const DEFAULT_PANE_FEED_FRAME_BYTES = 64 * 1024;
export const DEFAULT_PANE_FEED_STEP_BYTES = 32 * 1024;
export const MAX_PANE_FEED_FRAME_BUDGET_MS = 8;

export interface PaneFeedDelivery {
  accepted: boolean;
  stats?: { needsResync?: boolean } | null;
}

export interface PaneFeedSchedulerOptions {
  maxBytesPerPane?: number;
  frameBudgetMs?: number;
  maxBytesPerFrame?: number;
  stepBytes?: number;
  now?: () => number;
  schedule?: (run: () => void) => unknown;
  cancel?: (handle: unknown) => void;
  onDrop?: (paneKey: string, bytes: number) => void;
}

interface FeedQueue {
  frames: Uint8Array[];
  bytes: number;
}

export type PaneFeedFn = (paneKey: string, bytes: Uint8Array) => PaneFeedDelivery;

export class PaneFeedScheduler {
  private readonly maxBytesPerPane: number;
  private readonly frameBudgetMs: number;
  private readonly maxBytesPerFrame: number;
  private readonly stepBytes: number;
  private readonly now: () => number;
  private readonly scheduleFrame: (run: () => void) => void;
  private readonly cancelFrame: (handle: unknown) => void;
  private readonly onDrop?: (paneKey: string, bytes: number) => void;
  private readonly queues = new Map<string, FeedQueue>();
  private activePaneKey: string | null = null;
  private scheduled = false;
  private scheduledHandle: unknown = null;
  private disposed = false;

  constructor(private readonly feed: PaneFeedFn, options: PaneFeedSchedulerOptions = {}) {
    const maxBytesPerPane = options.maxBytesPerPane ?? DEFAULT_PANE_FEED_MAX_BYTES;
    const frameBudgetMs = options.frameBudgetMs ?? DEFAULT_PANE_FEED_FRAME_BUDGET_MS;
    const maxBytesPerFrame = options.maxBytesPerFrame ?? DEFAULT_PANE_FEED_FRAME_BYTES;
    const stepBytes = options.stepBytes ?? DEFAULT_PANE_FEED_STEP_BYTES;
    this.maxBytesPerPane = Number.isFinite(maxBytesPerPane)
      ? Math.max(1, Math.floor(maxBytesPerPane))
      : DEFAULT_PANE_FEED_MAX_BYTES;
    this.frameBudgetMs = Number.isFinite(frameBudgetMs)
      ? Math.min(Math.max(0, frameBudgetMs), MAX_PANE_FEED_FRAME_BUDGET_MS)
      : DEFAULT_PANE_FEED_FRAME_BUDGET_MS;
    this.maxBytesPerFrame = Number.isFinite(maxBytesPerFrame)
      ? Math.max(1, Math.floor(maxBytesPerFrame))
      : DEFAULT_PANE_FEED_FRAME_BYTES;
    this.stepBytes = Number.isFinite(stepBytes)
      ? Math.max(1, Math.floor(stepBytes))
      : DEFAULT_PANE_FEED_STEP_BYTES;
    this.now = options.now ?? (() => performance.now());
    this.scheduleFrame = (run) => {
      if (options.schedule) return options.schedule(run);
      if (typeof requestAnimationFrame === 'function') return requestAnimationFrame(() => run());
      return setTimeout(run, 0);
    };
    this.cancelFrame = options.cancel ?? ((handle) => {
      if (typeof handle === 'number' && typeof cancelAnimationFrame === 'function') {
        cancelAnimationFrame(handle);
      } else if (handle !== null && handle !== undefined) {
        clearTimeout(handle as ReturnType<typeof setTimeout>);
      }
    });
    this.onDrop = options.onDrop;
  }

  setActive(paneKey: string | null): void {
    this.activePaneKey = paneKey || null;
    this.schedule();
  }

  enqueue(paneKey: string, bytes: Uint8Array): void {
    if (this.disposed || !paneKey || bytes.byteLength === 0) return;
    let queue = this.queues.get(paneKey);
    if (!queue) {
      queue = { frames: [], bytes: 0 };
      this.queues.set(paneKey, queue);
    }
    let frame = bytes.slice();
    if (frame.byteLength > this.maxBytesPerPane) {
      const dropped = frame.byteLength - this.maxBytesPerPane;
      frame = frame.slice(dropped);
      this.onDrop?.(paneKey, dropped);
    }
    queue.frames.push(frame);
    queue.bytes += frame.byteLength;
    while (queue.bytes > this.maxBytesPerPane && queue.frames.length > 0) {
      const dropped = queue.frames.shift()!;
      queue.bytes -= dropped.byteLength;
      this.onDrop?.(paneKey, dropped.byteLength);
    }
    this.schedule();
  }

  /** Drain one bounded visual slice immediately (used by deterministic tests). */
  drainNow(): void {
    if (this.disposed) return;
    this.scheduled = false;
    this.cancelScheduledFrame();
    const started = this.now();
    let processed = 0;
    let activeTurn = false;
    let backgroundTurn = false;
    while (true) {
      let paneKey: string | null = null;
      if (!activeTurn && this.activePaneKey && (this.queues.get(this.activePaneKey)?.bytes ?? 0) > 0) {
        paneKey = this.activePaneKey;
        activeTurn = true;
      } else if (!backgroundTurn) {
        paneKey = this.nextNonActivePaneKey();
        backgroundTurn = true;
        if (!paneKey) paneKey = this.nextPaneKey();
      } else {
        paneKey = this.nextPaneKey();
      }
      if (!paneKey) break;
      if (processed > 0 && this.now() - started >= this.frameBudgetMs) break;
      if (processed >= this.maxBytesPerFrame) break;
      const queue = this.queues.get(paneKey);
      if (!queue || queue.frames.length === 0) continue;
      const frame = queue.frames.shift()!;
      queue.bytes -= frame.byteLength;
      const size = Math.min(this.stepBytes, frame.byteLength);
      const chunk = size === frame.byteLength ? frame : frame.slice(0, size);
      if (size < frame.byteLength) {
        const remainder = frame.slice(size);
        queue.frames.unshift(remainder);
        queue.bytes += remainder.byteLength;
      }
      let delivery: PaneFeedDelivery;
      try {
        delivery = this.feed(paneKey, chunk);
      } catch {
        this.onDrop?.(paneKey, chunk.byteLength);
        delivery = { accepted: true };
      }
      if (!delivery.accepted) {
        queue.frames.unshift(chunk);
        queue.bytes += chunk.byteLength;
        break;
      }
      processed += chunk.byteLength;
      if (queue.frames.length === 0) this.queues.delete(paneKey);
    }
    if (this.hasQueued()) this.schedule();
  }

  clear(paneKey: string): void {
    this.queues.delete(paneKey);
    if (!this.hasQueued()) this.cancelScheduledFrame();
  }

  clearAll(): void {
    this.queues.clear();
    this.cancelScheduledFrame();
  }

  dispose(): void {
    this.disposed = true;
    this.scheduled = false;
    this.queues.clear();
    this.cancelScheduledFrame();
  }

  queuedBytes(paneKey?: string): number {
    if (paneKey) return this.queues.get(paneKey)?.bytes ?? 0;
    let total = 0;
    for (const queue of this.queues.values()) total += queue.bytes;
    return total;
  }

  private schedule(): void {
    if (this.disposed || this.scheduled || !this.hasQueued()) return;
    this.scheduled = true;
    this.scheduledHandle = this.scheduleFrame(() => {
      this.scheduledHandle = null;
      if (!this.scheduled || this.disposed) return;
      this.drainNow();
    });
  }

  private cancelScheduledFrame(): void {
    if (this.scheduledHandle === null || this.scheduledHandle === undefined) return;
    const handle = this.scheduledHandle;
    this.scheduledHandle = null;
    this.cancelFrame(handle);
  }

  private hasQueued(): boolean {
    return this.queues.size > 0;
  }

  private nextPaneKey(): string | null {
    const active = this.activePaneKey;
    if (active && (this.queues.get(active)?.bytes ?? 0) > 0) return active;
    for (const [key, queue] of this.queues) {
      if (queue.bytes > 0) return key;
    }
    return null;
  }

  private nextNonActivePaneKey(): string | null {
    for (const [key, queue] of this.queues) {
      if (key !== this.activePaneKey && queue.bytes > 0) return key;
    }
    return null;
  }
}
