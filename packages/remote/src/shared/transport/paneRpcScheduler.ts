import {
  RpcCancelledError,
  RpcQueueFullError,
  RpcReconnectError,
  RpcTimeoutError,
  type RpcRequestOptions,
} from './types';
import { paneRefKey, type PaneRef } from './paneRef';

export const DEFAULT_MAX_QUEUED_INPUT_BYTES = 256 * 1024;
export const DEFAULT_INPUT_BATCH_WINDOW_MS = 0;
export const DEFAULT_INPUT_THROTTLE_MS = 8;
export const DEFAULT_RESIZE_DEBOUNCE_MS = 40;
export const DEFAULT_RPC_BACKOFF_BASE_MS = 100;
export const DEFAULT_RPC_BACKOFF_MAX_MS = 4_000;
export const DEFAULT_RPC_PAUSE_AFTER_FAILURES = 5;

interface RpcPort {
  request<T = unknown>(method: string, params?: unknown, options?: RpcRequestOptions): Promise<T>;
  cancelScope(scope: string): number;
}

interface InputBatch {
  data: string;
  bytes: number;
  sequence: number;
}

interface InputLane {
  pane: PaneRef;
  sourceId: string;
  nextSequence: number;
  queued: string;
  queuedBytes: number;
  active: InputBatch | null;
  inFlight: boolean;
  failures: number;
  paused: boolean;
  retryTimer: ReturnType<typeof setTimeout> | null;
  throttleTimer: ReturnType<typeof setTimeout> | null;
  nextSendAt: number;
}

interface ResizeValue {
  rows: number;
  cols: number;
  params?: Readonly<Record<string, unknown>>;
}

interface ResizeWaiter {
  resolve: () => void;
  reject: (error: Error) => void;
}

interface ResizeLane {
  pane: PaneRef;
  latest: ResizeValue | null;
  inFlight: boolean;
  activeSignature: string | null;
  lastAppliedSignature: string | null;
  failures: number;
  paused: boolean;
  timer: ReturnType<typeof setTimeout> | null;
  waiters: ResizeWaiter[];
}

export interface PaneRpcSchedulerOptions {
  inputSourceId?: string;
  maxQueuedInputBytes?: number;
  inputBatchWindowMs?: number;
  inputThrottleMs?: number;
  resizeDebounceMs?: number;
  backoffBaseMs?: number;
  backoffMaxMs?: number;
  pauseAfterFailures?: number;
  onError?: (error: Error, operation: 'input' | 'resize', pane: PaneRef) => void;
}

export interface PaneRpcSchedulerDiagnostics {
  inputCalls: number;
  inputRequests: number;
  inputBytesAccepted: number;
  inputBytesCompleted: number;
  inputRejected: number;
  resizeCalls: number;
  resizeRequests: number;
  resizeSuppressed: number;
  inputFailures: number;
  resizeFailures: number;
  timeoutFailures: number;
  retries: number;
  pausedLanes: number;
  queuedInputBytes: number;
}

export class PaneInputQueueFullError extends Error {
  constructor(readonly limit: number) {
    super(`pane input queue reached ${limit} bytes`);
    this.name = 'PaneInputQueueFullError';
  }
}

const utf8 = new TextEncoder();

function normalizeSourceId(value: string): string {
  const normalized = value.replace(/[^A-Za-z0-9_-]/g, '_').slice(0, 48);
  return normalized || 'ridge_remote';
}

function defaultSourceId(): string {
  const random = globalThis.crypto?.randomUUID?.() ?? Math.random().toString(36).slice(2);
  return normalizeSourceId(`ridge_remote_${random}`);
}

function retryable(error: unknown): boolean {
  return (
    error instanceof RpcTimeoutError ||
    error instanceof RpcReconnectError ||
    error instanceof RpcQueueFullError
  );
}

function resizeSignature(value: ResizeValue | null): string | null {
  return value ? `${value.rows}x${value.cols}` : null;
}

/** Per-pane RPC admission: ordered input, latest resize, bounded memory, one in-flight of each. */
export class PaneRpcScheduler {
  private readonly inputLanes = new Map<string, InputLane>();
  private readonly resizeLanes = new Map<string, ResizeLane>();
  private readonly sourcePrefix: string;
  private readonly maxQueuedInputBytes: number;
  private readonly inputBatchWindowMs: number;
  private readonly inputThrottleMs: number;
  private readonly resizeDebounceMs: number;
  private readonly backoffBaseMs: number;
  private readonly backoffMaxMs: number;
  private readonly pauseAfterFailures: number;
  private readonly onError: NonNullable<PaneRpcSchedulerOptions['onError']>;
  private laneSequence = 0;
  private counters = {
    inputCalls: 0,
    inputRequests: 0,
    inputBytesAccepted: 0,
    inputBytesCompleted: 0,
    inputRejected: 0,
    resizeCalls: 0,
    resizeRequests: 0,
    resizeSuppressed: 0,
    inputFailures: 0,
    resizeFailures: 0,
    timeoutFailures: 0,
    retries: 0,
  };

  constructor(private readonly rpc: RpcPort, options: PaneRpcSchedulerOptions = {}) {
    this.sourcePrefix = normalizeSourceId(options.inputSourceId ?? defaultSourceId());
    this.maxQueuedInputBytes = Math.max(
      1,
      Math.floor(options.maxQueuedInputBytes ?? DEFAULT_MAX_QUEUED_INPUT_BYTES),
    );
    this.inputBatchWindowMs = Math.max(
      0,
      Math.floor(options.inputBatchWindowMs ?? DEFAULT_INPUT_BATCH_WINDOW_MS),
    );
    this.inputThrottleMs = Math.max(
      0,
      Math.floor(options.inputThrottleMs ?? DEFAULT_INPUT_THROTTLE_MS),
    );
    this.resizeDebounceMs = Math.max(
      0,
      Math.floor(options.resizeDebounceMs ?? DEFAULT_RESIZE_DEBOUNCE_MS),
    );
    this.backoffBaseMs = Math.max(
      1,
      Math.floor(options.backoffBaseMs ?? DEFAULT_RPC_BACKOFF_BASE_MS),
    );
    this.backoffMaxMs = Math.max(
      this.backoffBaseMs,
      Math.floor(options.backoffMaxMs ?? DEFAULT_RPC_BACKOFF_MAX_MS),
    );
    this.pauseAfterFailures = Math.max(
      1,
      Math.floor(options.pauseAfterFailures ?? DEFAULT_RPC_PAUSE_AFTER_FAILURES),
    );
    this.onError = options.onError ?? (() => {});
  }

  enqueueInput(pane: PaneRef, data: string): boolean {
    this.counters.inputCalls += 1;
    if (!data) return true;
    const lane = this.inputLane(pane);
    const bytes = utf8.encode(data).byteLength;
    const retained = lane.queuedBytes + (lane.active?.bytes ?? 0);
    if (retained + bytes > this.maxQueuedInputBytes) {
      this.counters.inputRejected += 1;
      this.reportError(new PaneInputQueueFullError(this.maxQueuedInputBytes), 'input', pane);
      return false;
    }
    lane.queued += data;
    lane.queuedBytes += bytes;
    this.counters.inputBytesAccepted += bytes;
    if (
      this.inputBatchWindowMs > 0 &&
      !lane.active &&
      !lane.inFlight &&
      !lane.paused &&
      !lane.retryTimer &&
      !lane.throttleTimer
    ) {
      lane.throttleTimer = setTimeout(() => {
        lane.throttleTimer = null;
        this.drainInput(paneRefKey(pane), lane);
      }, this.inputBatchWindowMs);
      return true;
    }
    this.drainInput(paneRefKey(pane), lane);
    return true;
  }

  scheduleResize(
    pane: PaneRef,
    rows: number,
    cols: number,
    params?: Readonly<Record<string, unknown>>,
  ): boolean {
    return this.enqueueResize(pane, rows, cols, undefined, params);
  }

  /** Queue a resize and settle only after the latest coalesced value applies. */
  scheduleResizeAndWait(
    pane: PaneRef,
    rows: number,
    cols: number,
    params?: Readonly<Record<string, unknown>>,
  ): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      this.enqueueResize(pane, rows, cols, { resolve, reject }, params);
    });
  }

  private enqueueResize(
    pane: PaneRef,
    rows: number,
    cols: number,
    waiter?: ResizeWaiter,
    params?: Readonly<Record<string, unknown>>,
  ): boolean {
    this.counters.resizeCalls += 1;
    const normalized = { rows: Math.floor(rows), cols: Math.floor(cols) };
    if (!Number.isFinite(rows) || !Number.isFinite(cols) || normalized.rows <= 0 || normalized.cols <= 0) {
      this.counters.resizeSuppressed += 1;
      waiter?.reject(new TypeError('invalid pane resize dimensions'));
      return false;
    }
    const key = paneRefKey(pane);
    let lane = this.resizeLanes.get(key);
    if (!lane) {
      lane = {
        pane,
        latest: null,
        inFlight: false,
        activeSignature: null,
        lastAppliedSignature: null,
        failures: 0,
        paused: false,
        timer: null,
        waiters: [],
      };
      this.resizeLanes.set(key, lane);
    }
    const signature = resizeSignature(normalized);
    if (waiter) lane.waiters.push(waiter);
    if (
      resizeSignature(lane.latest) === signature ||
      lane.activeSignature === signature ||
      (!lane.inFlight && !lane.latest && lane.lastAppliedSignature === signature)
    ) {
      this.counters.resizeSuppressed += 1;
      if (!lane.inFlight && !lane.latest && lane.lastAppliedSignature === signature) {
        const waiters = lane.waiters.splice(0);
        for (const current of waiters) current.resolve();
      }
      return false;
    }
    lane.latest = { ...normalized, params };
    this.scheduleResizeDrain(key, lane, this.resizeDebounceMs);
    return true;
  }

  resume(pane: PaneRef): void {
    const key = paneRefKey(pane);
    const input = this.inputLanes.get(key);
    if (input) {
      this.clearTimer(input);
      input.paused = false;
      input.failures = 0;
      input.nextSendAt = 0;
      this.drainInput(key, input);
    }
    const resize = this.resizeLanes.get(key);
    if (resize) {
      if (resize.timer) clearTimeout(resize.timer);
      resize.timer = null;
      resize.paused = false;
      resize.failures = 0;
      this.scheduleResizeDrain(key, resize, 0);
    }
  }

  resumeAll(): void {
    const panes = new Map<string, PaneRef>();
    for (const [key, lane] of this.inputLanes) panes.set(key, lane.pane);
    for (const [key, lane] of this.resizeLanes) panes.set(key, lane.pane);
    for (const pane of panes.values()) this.resume(pane);
  }

  retire(pane: PaneRef): void {
    this.retireScope(paneRefKey(pane));
  }

  retireScope(scope: string): void {
    const input = this.inputLanes.get(scope);
    if (input) this.clearTimer(input);
    const resize = this.resizeLanes.get(scope);
    if (resize?.timer) clearTimeout(resize.timer);
    this.inputLanes.delete(scope);
    if (resize) {
      const error = new RpcCancelledError('resize_pane');
      for (const waiter of resize.waiters.splice(0)) waiter.reject(error);
    }
    this.resizeLanes.delete(scope);
    this.rpc.cancelScope(scope);
  }

  dispose(): void {
    const panes = new Map<string, PaneRef>();
    for (const [key, lane] of this.inputLanes) panes.set(key, lane.pane);
    for (const [key, lane] of this.resizeLanes) panes.set(key, lane.pane);
    for (const key of panes.keys()) this.retireScope(key);
  }

  prune(liveScopes: ReadonlySet<string>): string[] {
    const tracked = new Set([...this.inputLanes.keys(), ...this.resizeLanes.keys()]);
    const retired: string[] = [];
    for (const scope of tracked) {
      if (liveScopes.has(scope)) continue;
      this.retireScope(scope);
      retired.push(scope);
    }
    return retired;
  }

  get diagnostics(): PaneRpcSchedulerDiagnostics {
    let queuedInputBytes = 0;
    let pausedLanes = 0;
    for (const lane of this.inputLanes.values()) {
      queuedInputBytes += lane.queuedBytes + (lane.active?.bytes ?? 0);
      if (lane.paused) pausedLanes += 1;
    }
    for (const lane of this.resizeLanes.values()) if (lane.paused) pausedLanes += 1;
    return { ...this.counters, pausedLanes, queuedInputBytes };
  }

  private inputLane(pane: PaneRef): InputLane {
    const key = paneRefKey(pane);
    let lane = this.inputLanes.get(key);
    if (!lane) {
      this.laneSequence += 1;
      lane = {
        pane,
        sourceId: `${this.sourcePrefix}_${this.laneSequence}`.slice(0, 64),
        nextSequence: 1,
        queued: '',
        queuedBytes: 0,
        active: null,
        inFlight: false,
        failures: 0,
        paused: false,
        retryTimer: null,
        throttleTimer: null,
        nextSendAt: 0,
      };
      this.inputLanes.set(key, lane);
    }
    return lane;
  }

  private drainInput(key: string, lane: InputLane): void {
    if (
      this.inputLanes.get(key) !== lane ||
      lane.inFlight ||
      lane.paused ||
      lane.retryTimer ||
      lane.throttleTimer
    ) return;
    if (!lane.active && !lane.queued) return;
    const throttleDelay = lane.nextSendAt - Date.now();
    if (throttleDelay > 0) {
      lane.throttleTimer = setTimeout(() => {
        lane.throttleTimer = null;
        this.drainInput(key, lane);
      }, throttleDelay);
      return;
    }
    if (!lane.active) {
      lane.active = {
        data: lane.queued,
        bytes: lane.queuedBytes,
        sequence: lane.nextSequence,
      };
      lane.queued = '';
      lane.queuedBytes = 0;
    }
    const batch = lane.active;
    lane.inFlight = true;
    lane.nextSendAt = Date.now() + this.inputThrottleMs;
    this.counters.inputRequests += 1;
    void this.rpc.request('write_to_pty', {
      workspaceId: lane.pane.workspaceId,
      paneId: lane.pane.paneId,
      data: batch.data,
      inputSourceId: lane.sourceId,
      inputSequence: batch.sequence,
    }, { scope: key }).then(
      () => {
        if (this.inputLanes.get(key) !== lane) return;
        lane.inFlight = false;
        lane.active = null;
        lane.nextSequence += 1;
        lane.failures = 0;
        this.counters.inputBytesCompleted += batch.bytes;
        this.drainInput(key, lane);
      },
      (error: unknown) => {
        if (this.inputLanes.get(key) !== lane) return;
        lane.inFlight = false;
        this.failLane(error, 'input', lane.pane, lane, () => this.drainInput(key, lane));
      },
    );
  }

  private scheduleResizeDrain(key: string, lane: ResizeLane, delay: number): void {
    if (
      this.resizeLanes.get(key) !== lane ||
      lane.inFlight ||
      lane.paused ||
      lane.timer ||
      !lane.latest
    ) return;
    lane.timer = setTimeout(() => {
      lane.timer = null;
      this.drainResize(key, lane);
    }, delay);
  }

  private drainResize(key: string, lane: ResizeLane): void {
    if (this.resizeLanes.get(key) !== lane || lane.inFlight || lane.paused || !lane.latest) return;
    const value = lane.latest;
    lane.latest = null;
    lane.inFlight = true;
    lane.activeSignature = resizeSignature(value);
    this.counters.resizeRequests += 1;
    void this.rpc.request('resize_pane', {
      ...value.params,
      workspaceId: lane.pane.workspaceId,
      paneId: lane.pane.paneId,
      rows: value.rows,
      cols: value.cols,
    }, { scope: key }).then(
      () => {
        if (this.resizeLanes.get(key) !== lane) return;
        lane.inFlight = false;
        lane.lastAppliedSignature = resizeSignature(value);
        lane.activeSignature = null;
        lane.failures = 0;
        if (lane.latest) {
          this.scheduleResizeDrain(key, lane, this.resizeDebounceMs);
        } else {
          const waiters = lane.waiters.splice(0);
          for (const waiter of waiters) waiter.resolve();
        }
      },
      (error: unknown) => {
        if (this.resizeLanes.get(key) !== lane) return;
        lane.inFlight = false;
        lane.activeSignature = null;
        if (!lane.latest) lane.latest = value;
        this.failLane(error, 'resize', lane.pane, lane, () => this.drainResize(key, lane));
        if (lane.paused) {
          const failure = error instanceof Error ? error : new Error(String(error));
          for (const waiter of lane.waiters.splice(0)) waiter.reject(failure);
        }
      },
    );
  }

  private failLane(
    error: unknown,
    operation: 'input' | 'resize',
    pane: PaneRef,
    lane: { failures: number; paused: boolean; retryTimer?: ReturnType<typeof setTimeout> | null; timer?: ReturnType<typeof setTimeout> | null },
    retry: () => void,
  ): void {
    const failure = error instanceof Error ? error : new Error(String(error));
    if (failure instanceof RpcCancelledError) return;
    if (operation === 'input') this.counters.inputFailures += 1;
    else this.counters.resizeFailures += 1;
    if (failure instanceof RpcTimeoutError) this.counters.timeoutFailures += 1;
    lane.failures += 1;
    this.reportError(failure, operation, pane);
    if (!retryable(failure) || lane.failures >= this.pauseAfterFailures) {
      lane.paused = true;
      return;
    }
    this.counters.retries += 1;
    const delay = Math.min(this.backoffMaxMs, this.backoffBaseMs * 2 ** (lane.failures - 1));
    const timer = setTimeout(() => {
      if ('retryTimer' in lane) lane.retryTimer = null;
      if ('timer' in lane) lane.timer = null;
      retry();
    }, delay);
    if ('retryTimer' in lane) lane.retryTimer = timer;
    else lane.timer = timer;
  }

  private clearTimer(lane: InputLane): void {
    if (lane.retryTimer) clearTimeout(lane.retryTimer);
    if (lane.throttleTimer) clearTimeout(lane.throttleTimer);
    lane.retryTimer = null;
    lane.throttleTimer = null;
  }

  private reportError(error: Error, operation: 'input' | 'resize', pane: PaneRef): void {
    try {
      this.onError(error, operation, pane);
    } catch {
      // Observability hooks must not corrupt admission or retry state.
    }
  }
}
