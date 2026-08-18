/** Preserve per-pane input order across asynchronous desktop IPC writes. */
export const DEFAULT_MAX_PENDING_PTY_WRITES = 256;
export const DEFAULT_MAX_QUEUED_PTY_INPUT_BYTES = 256 * 1024;

interface QueueLane {
  tail: Promise<void>;
  pending: number;
  generation: number;
}

const lanes = new Map<string, QueueLane>();

interface PtyInputLane {
  active: boolean;
  draining: boolean;
  write: (data: string) => Promise<unknown>;
  onError?: (error: unknown) => void;
  activeBytes: number;
  queued: string;
  queuedBytes: number;
  flushTimer: ReturnType<typeof setTimeout> | null;
}

const inputLanes = new Map<string, PtyInputLane>();
const inputEncoder = new TextEncoder();

export class PtyWriteQueueFullError extends Error {
  constructor(readonly key: string, readonly limit: number) {
    super(`PTY write queue reached ${limit} pending operations for ${key}`);
    this.name = 'PtyWriteQueueFullError';
  }
}

export class PtyWriteQueueRetiredError extends Error {
  constructor(readonly key: string) {
    super(`PTY write queue retired for ${key}`);
    this.name = 'PtyWriteQueueRetiredError';
  }
}

export interface PtyWriteQueueOptions {
  maxPending?: number;
}

export function enqueuePtyWrite(
  key: string,
  write: () => Promise<unknown>,
  options: PtyWriteQueueOptions = {},
): Promise<void> {
  const limit = Math.max(1, Math.floor(options.maxPending ?? DEFAULT_MAX_PENDING_PTY_WRITES));
  let lane = lanes.get(key);
  if (!lane) {
    lane = { tail: Promise.resolve(), pending: 0, generation: 0 };
    lanes.set(key, lane);
  }
  if (lane.pending >= limit) {
    return Promise.reject(new PtyWriteQueueFullError(key, limit));
  }

  const generation = lane.generation;
  lane.pending += 1;
  const previous = lane.tail;
  const next = previous.catch(() => undefined).then(async () => {
    // A real pane close retires this lane. Do not dispatch queued writes into
    // a newly-created PTY that happens to reuse the same UUID.
    if (lanes.get(key) !== lane || lane.generation !== generation) {
      throw new PtyWriteQueueRetiredError(key);
    }
    await write();
  });
  lane.tail = next;
  void next
    .finally(() => {
      if (lanes.get(key) !== lane) return;
      lane.pending -= 1;
      if (lane.pending === 0) lanes.delete(key);
    })
    .catch(() => undefined);
  return next;
}

/** Coalesce fast keyboard input while one desktop IPC write is in flight. */
export function enqueuePtyInput(
  key: string,
  data: string,
  write: (data: string) => Promise<unknown>,
  options: {
    maxQueuedBytes?: number;
    onError?: (error: unknown) => void;
    coalesceWindowMs?: number;
  } = {},
): boolean {
  if (!data) return true;
  let lane = inputLanes.get(key);
  if (!lane) {
    lane = {
      active: false,
      draining: false,
      write,
      onError: options.onError,
      activeBytes: 0,
      queued: '',
      queuedBytes: 0,
      flushTimer: null,
    };
    inputLanes.set(key, lane);
  }
  const bytes = inputEncoder.encode(data).byteLength;
  const maxBytes = Math.max(1, Math.floor(options.maxQueuedBytes ?? DEFAULT_MAX_QUEUED_PTY_INPUT_BYTES));
  if (lane.activeBytes + lane.queuedBytes + bytes > maxBytes) return false;
  lane.queued += data;
  lane.queuedBytes += bytes;
  if (!lane.active) {
    lane.active = true;
    const windowMs = Math.max(0, Math.floor(options.coalesceWindowMs ?? 0));
    if (windowMs > 0) {
      lane.flushTimer = setTimeout(() => {
        lane.flushTimer = null;
        startPtyInputDrain(key, lane!);
      }, windowMs);
    } else {
      startPtyInputDrain(key, lane);
    }
  }
  return true;
}

function startPtyInputDrain(
  key: string,
  lane: PtyInputLane,
): void {
  if (lane.draining) return;
  lane.draining = true;
  void drainPtyInput(key, lane, lane.write, lane.onError);
}

async function drainPtyInput(
  key: string,
  lane: PtyInputLane,
  write: (data: string) => Promise<unknown>,
  onError?: (error: unknown) => void,
): Promise<void> {
  while (inputLanes.get(key) === lane && lane.queued.length > 0) {
    const data = lane.queued;
    lane.queued = '';
    lane.queuedBytes = 0;
    lane.activeBytes = inputEncoder.encode(data).byteLength;
    try {
      await enqueuePtyWrite(key, () => write(data));
    } catch (error) {
      onError?.(error);
    } finally {
      lane.activeBytes = 0;
    }
  }
  if (inputLanes.get(key) === lane) inputLanes.delete(key);
}

/** Flush a pending keyboard coalesce window before an ordered paste/write. */
export function flushPtyInput(key: string): void {
  const lane = inputLanes.get(key);
  if (!lane || lane.draining || lane.queued.length === 0) return;
  if (lane.flushTimer !== null) {
    clearTimeout(lane.flushTimer);
    lane.flushTimer = null;
  }
  startPtyInputDrain(key, lane);
}

/** Stop queued writes for a pane after its backend PTY has been destroyed. */
export function retirePtyWriteQueue(key: string): void {
  const lane = lanes.get(key);
  if (lane) {
    lane.generation += 1;
    lanes.delete(key);
  }
  const inputLane = inputLanes.get(key);
  if (inputLane && inputLane.flushTimer !== null) clearTimeout(inputLane.flushTimer);
  inputLanes.delete(key);
}

/** Pane lifecycle helper for callers that only have paneId, not workspaceId. */
export function retirePtyWriteQueuesForPane(paneId: string): void {
  for (const key of new Set([...lanes.keys(), ...inputLanes.keys()])) {
    if (key === paneId || key.endsWith(`:${paneId}`)) retirePtyWriteQueue(key);
  }
}
