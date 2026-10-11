import { CHANNEL, BULK_RPC_THRESHOLD_BYTES, isPaneOutputFrame } from '../transport/cloudMux';
import terminalHello from './signaling/fixtures/terminal/hello.json';
import { remotePerfEnabled, remotePerfMark, remotePerfStart } from '../transport/remotePerfTrace';

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
export const DEFAULT_MAX_PENDING_CONTROL_BYTES = BULK_RPC_THRESHOLD_BYTES;

interface QueuedFrame {
  frame: Uint8Array;
  bytes: number;
  queuedAt?: number;
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
  private enqueuedFrames = 0;
  private dequeuedFrames = 0;
  private droppedFrames = 0;

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
    const item = { frame: copy, bytes: copy.byteLength, queuedAt: remotePerfStart('transport-queue')?.startedAt };
    if (isPaneOutputFrame(copy)) {
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
        this.observeDrop();
      }
      if (item.bytes > this.maxPendingControlBytes) { this.observeDrop(); return false; }
      this.controls.push(item);
      this.pendingControlBytes += item.bytes;
    }
    if (remotePerfEnabled()) this.enqueuedFrames++;
    this.observe();
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
    this.observe();
  }

  getPendingPaneBytes(): number {
    return this.pendingPaneBytes;
  }

  getPendingControlBytes(): number {
    return this.pendingControlBytes;
  }

  get diagnostics() {
    const heads = [this.controls[0], this.panes[0]].filter(Boolean);
    const oldestAgeKnown = heads.every(item => item.queuedAt !== undefined);
    const timestamps = heads.map(item => item.queuedAt!);
    return { queueBytes: this.pendingControlBytes + this.pendingPaneBytes, queueDepth: this.controls.length + this.panes.length,
      oldestAgeMs: heads.length === 0 ? 0 : oldestAgeKnown ? Math.max(0, performance.now() - Math.min(...timestamps)) : undefined,
      oldestAgeKnown,
      enqueuedFrames: this.enqueuedFrames, dequeuedFrames: this.dequeuedFrames, droppedFrames: this.droppedFrames };
  }

  private observe() {
    if (remotePerfEnabled()) remotePerfMark('transport-queue', { transport: 'cloud-priority', ...this.diagnostics });
  }

  private observeDrop() { if (remotePerfEnabled()) this.droppedFrames++; this.observe(); }

  private admitPane(item: QueuedFrame): boolean {
    // A snapshot cannot be split into independent terminal packets. Retain at
    // most one large canonical packet, bounded by the negotiated frame limit.
    const canonical = item.frame[0] === CHANNEL.TERMINAL || item.frame[0] === CHANNEL.JSON;
    const limit = canonical ? Math.max(this.maxPendingPaneBytes, Math.min(item.bytes, terminalHello.maxFrameBytes)) : this.maxPendingPaneBytes;
    if ((canonical && item.bytes > terminalHello.maxFrameBytes) || item.bytes > limit) {
      this.observeDrop();
      this.onPaneDrop?.();
      return false;
    }
    // Keep the newest live tail. Older pane bytes are recoverable through the
    // existing pane resync path, while retaining them only increases latency.
    while (
      this.pendingPaneBytes + item.bytes > limit &&
      this.panes.length > 0
    ) {
      const dropped = this.panes.shift()!;
      this.pendingPaneBytes -= dropped.bytes;
      this.observeDrop();
      this.onPaneDrop?.();
    }
    return this.pendingPaneBytes + item.bytes <= limit;
  }

  private pump(): void {
    if (this.pumping) return;
    this.pumping = true;
    try {
      for (;;) {
        const control = this.controls.shift();
        if (control) {
          this.pendingControlBytes -= control.bytes;
          if (remotePerfEnabled()) this.dequeuedFrames++;
          this.observe();
          this.send(control.frame);
          continue;
        }
        if (this.panes.length === 0 || !this.canSendPane()) return;
        const pane = this.panes.shift()!;
        this.pendingPaneBytes -= pane.bytes;
        if (remotePerfEnabled()) this.dequeuedFrames++;
        this.observe();
        this.send(pane.frame);
      }
    } finally {
      this.pumping = false;
    }
  }
}
