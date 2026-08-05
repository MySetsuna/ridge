import { CHANNEL } from '../transport/cloudMux';

/**
 * Keeps control/input frames ahead of pane output while preserving a bounded
 * legacy single-lane fallback. Frames are queued before E2EE sealing so each
 * lane's receive-side counter remains strictly monotonic.
 */
export interface PriorityFrameQueueOptions {
  send: (frame: Uint8Array) => void;
  canSendPane: () => boolean;
  maxPendingPaneBytes?: number;
  maxPendingControlBytes?: number;
  onPaneDrop?: () => void;
}

export const DEFAULT_MAX_PENDING_PANE_BYTES = 256 * 1024;
export const DEFAULT_MAX_PENDING_CONTROL_BYTES = 128 * 1024;

interface QueuedFrame {
  frame: Uint8Array;
  bytes: number;
}

/** Small, synchronous priority pump; no timers or unbounded promise chains. */
export class PriorityFrameQueue {
  private readonly send: (frame: Uint8Array) => void;
  private readonly canSendPane: () => boolean;
  private readonly maxPendingPaneBytes: number;
  private readonly maxPendingControlBytes: number;
  private readonly onPaneDrop?: () => void;
  private readonly controls: QueuedFrame[] = [];
  private readonly panes: QueuedFrame[] = [];
  private pendingPaneBytes = 0;
  private pendingControlBytes = 0;
  private pumping = false;

  constructor(options: PriorityFrameQueueOptions) {
    this.send = options.send;
    this.canSendPane = options.canSendPane;
    this.maxPendingPaneBytes = options.maxPendingPaneBytes ?? DEFAULT_MAX_PENDING_PANE_BYTES;
    this.maxPendingControlBytes = options.maxPendingControlBytes ?? DEFAULT_MAX_PENDING_CONTROL_BYTES;
    this.onPaneDrop = options.onPaneDrop;
  }

  /** Enqueue a mux frame and synchronously send whatever the channel admits. */
  enqueue(frame: Uint8Array): boolean {
    const copy = frame.slice();
    const item = { frame: copy, bytes: copy.byteLength };
    if (copy[0] === CHANNEL.PANE_RAW) {
      if (!this.admitPane(item)) return false;
      this.panes.push(item);
      this.pendingPaneBytes += item.bytes;
    } else {
      // Control is bounded too: a disconnected/reconnecting peer must not
      // retain an unbounded notification burst. Drop oldest notifications only
      // after the hard cap; request/response traffic is kept newest-first.
      while (
        this.pendingControlBytes + item.bytes > this.maxPendingControlBytes &&
        this.controls.length > 0
      ) {
        const dropped = this.controls.shift()!;
        this.pendingControlBytes -= dropped.bytes;
      }
      if (item.bytes > this.maxPendingControlBytes) return false;
      this.controls.push(item);
      this.pendingControlBytes += item.bytes;
    }
    this.pump();
    return true;
  }

  /** Retry pane output after `bufferedamountlow`; controls always go first. */
  resume(): void {
    this.pump();
  }

  clear(): void {
    this.controls.length = 0;
    this.panes.length = 0;
    this.pendingPaneBytes = 0;
    this.pendingControlBytes = 0;
  }

  getPendingPaneBytes(): number {
    return this.pendingPaneBytes;
  }

  getPendingControlBytes(): number {
    return this.pendingControlBytes;
  }

  private admitPane(item: QueuedFrame): boolean {
    if (item.bytes > this.maxPendingPaneBytes) {
      this.onPaneDrop?.();
      return false;
    }
    // Keep the newest live tail. Older pane bytes are recoverable through the
    // existing pane resync path, while retaining them only increases latency.
    while (
      this.pendingPaneBytes + item.bytes > this.maxPendingPaneBytes &&
      this.panes.length > 0
    ) {
      const dropped = this.panes.shift()!;
      this.pendingPaneBytes -= dropped.bytes;
      this.onPaneDrop?.();
    }
    return this.pendingPaneBytes + item.bytes <= this.maxPendingPaneBytes;
  }

  private pump(): void {
    if (this.pumping) return;
    this.pumping = true;
    try {
      for (;;) {
        const control = this.controls.shift();
        if (control) {
          this.pendingControlBytes -= control.bytes;
          this.send(control.frame);
          continue;
        }
        if (this.panes.length === 0 || !this.canSendPane()) return;
        const pane = this.panes.shift()!;
        this.pendingPaneBytes -= pane.bytes;
        this.send(pane.frame);
      }
    } finally {
      this.pumping = false;
    }
  }
}
