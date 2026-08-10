/**
 * Serialize input *intents* per pane, including work that must await an
 * asynchronous source (for example navigator.clipboard.readText()).
 *
 * The PTY/RPC queues only see bytes after the source promise resolves.  This
 * small gate reserves the ordering slot before that await, so later keyboard,
 * drag/drop, or Agent writes cannot overtake the pending paste.  It is only an
 * admission/order gate; the existing PTY/RPC queue remains responsible for
 * byte limits, retries, timeouts, and transport backpressure.
 */

// Intent records are tiny (operation closure + promise); keep this above the
// existing 256 KiB byte queue's typical keystroke burst while still bounding a
// pathological producer that never reaches the transport queue.
export const DEFAULT_MAX_PANE_INPUT_INTENTS = 4096;

interface InputLane {
  tail: Promise<void>;
  pending: number;
  generation: number;
}

export class PaneInputGateFullError extends Error {
  constructor(readonly key: string, readonly limit: number) {
    super(`pane input intent gate reached ${limit} pending operations for ${key}`);
    this.name = 'PaneInputGateFullError';
  }
}

export class PaneInputGateRetiredError extends Error {
  constructor(readonly key: string) {
    super(`pane input intent gate retired for ${key}`);
    this.name = 'PaneInputGateRetiredError';
  }
}

const lanes = new Map<string, InputLane>();

function schedule(
  key: string,
  operation: () => void | Promise<void>,
  limit: number,
): Promise<void> | null {
  let lane = lanes.get(key);
  if (!lane) {
    lane = { tail: Promise.resolve(), pending: 0, generation: 0 };
    lanes.set(key, lane);
  }
  if (lane.pending >= limit) {
    return null;
  }

  const wasIdle = lane.pending === 0;
  const generation = lane.generation;
  lane.pending += 1;
  const run = async () => {
    if (lanes.get(key) !== lane || lane.generation !== generation) {
      throw new PaneInputGateRetiredError(key);
    }
    await operation();
  };
  // Start the first intent synchronously. Apart from avoiding an unnecessary
  // input-frame delay, this keeps existing `sendStdin` admission semantics:
  // the underlying scheduler sees a normal key immediately. Later intents
  // remain strictly chained behind the prior operation (including an async
  // clipboard read).
  const next = wasIdle ? run() : lane.tail.catch(() => undefined).then(run);
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

/** Reserve and serialize one input intent. Errors never strand later intents. */
export function enqueuePaneInput(
  key: string,
  operation: () => void | Promise<void>,
  options: { maxPending?: number } = {},
): Promise<void> {
  const limit = Math.max(1, Math.floor(options.maxPending ?? DEFAULT_MAX_PANE_INPUT_INTENTS));
  const promise = schedule(key, operation, limit);
  return promise ?? Promise.reject(new PaneInputGateFullError(key, limit));
}

/** Fire-and-forget admission used by synchronous input callbacks. */
export function tryEnqueuePaneInput(
  key: string,
  operation: () => void | Promise<void>,
  options: { maxPending?: number } = {},
): boolean {
  const limit = Math.max(1, Math.floor(options.maxPending ?? DEFAULT_MAX_PANE_INPUT_INTENTS));
  const promise = schedule(key, operation, limit);
  if (promise === null || promise === undefined) return false;
  void promise.catch(() => undefined);
  return true;
}

/**
 * Admit a synchronous keystroke without a promise turn when no async intent
 * owns the pane. The RPC scheduler already serializes/coalesces these bytes;
 * routing every key through the async intent chain otherwise splits a burst
 * into one request per microtask and adds round-trip latency. If a paste or
 * other async intent is pending, fall back to the normal gate so ordering is
 * preserved behind it.
 */
export function tryEnqueuePaneInputImmediate(
  key: string,
  operation: () => void,
  options: { maxPending?: number } = {},
): boolean {
  const lane = lanes.get(key);
  if (lane && lane.pending > 0) return tryEnqueuePaneInput(key, operation, options);
  try {
    operation();
    return true;
  } catch {
    return false;
  }
}

/** Drop pending intents when the corresponding PTY/pane is destroyed. */
export function retirePaneInput(key: string): void {
  const lane = lanes.get(key);
  if (!lane) return;
  lane.generation += 1;
  lanes.delete(key);
}

/** Pane-id fallback for desktop teardown paths that do not retain workspace id. */
export function retirePaneInputsForPane(paneId: string): void {
  for (const key of lanes.keys()) {
    if (key === paneId || key.endsWith(`:${paneId}`)) retirePaneInput(key);
  }
}

/** Test/diagnostic helper; does not expose operation payloads. */
export function pendingPaneInputIntents(key: string): number {
  return lanes.get(key)?.pending ?? 0;
}
