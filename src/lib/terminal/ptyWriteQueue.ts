/** Preserve per-pane input order across asynchronous desktop IPC writes. */
export const DEFAULT_MAX_PENDING_PTY_WRITES = 256;

interface QueueLane {
  tail: Promise<void>;
  pending: number;
  generation: number;
}

const lanes = new Map<string, QueueLane>();

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

/** Stop queued writes for a pane after its backend PTY has been destroyed. */
export function retirePtyWriteQueue(key: string): void {
  const lane = lanes.get(key);
  if (!lane) return;
  lane.generation += 1;
  lanes.delete(key);
}

/** Pane lifecycle helper for callers that only have paneId, not workspaceId. */
export function retirePtyWriteQueuesForPane(paneId: string): void {
  for (const key of lanes.keys()) {
    if (key === paneId || key.endsWith(`:${paneId}`)) retirePtyWriteQueue(key);
  }
}
