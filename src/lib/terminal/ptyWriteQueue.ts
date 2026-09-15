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
  // §B generation guard：mountToken 变化时（旧 Svelte 实例 unmount → 新实例
  // mount）撞车 → 旧 lane 提前清空，bytes 不跟到新 PTY。
  generation: number;
  mountToken: unknown;
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

/** Coalesce fast keyboard input while one desktop IPC write is in flight.
 *
 * §B 代际保护：`mountToken` 是调用方提供的「当前 mount 代次」标识（不透明值
 * 即可：通常是 Svelte 组件实例自身 / 一个 monotonically increasing 计数器）。
 * 每次 enqueuePtyInput 会把 lane 里的 mountToken 跟入参对比，不同 → 旧 lane
 * 立即清理，queued bytes 丢弃，drain 路径不再触发旧闭包。新的闭包 / 新实例
 * 从干净 lane 开始。同一实例内连续 enqueue（mountToken 不变）共享 lane，合并
 * coalesce 窗口。`write`/`onError` 闭包身份**不**作为信号——同一 onPtyData
 * 每次 keystroke 都新建 arrow function，identity 比对会产生误判。 */
export function enqueuePtyInput(
  key: string,
  data: string,
  write: (data: string) => Promise<unknown>,
  options: {
    maxQueuedBytes?: number;
    onError?: (error: unknown) => void;
    coalesceWindowMs?: number;
    /** 不透明 mount 代次标识；变化时旧 lane 立即清理。强烈建议传组件实例
     *  或显式 mount 计数器。省略则退化为「只比 write 闭包身份」（不推荐）。 */
    mountToken?: unknown;
  } = {},
): boolean {
  if (!data) return true;
  let lane = inputLanes.get(key);
  if (lane) {
    // §B generation guard：mountToken 变化 → unmount→remount race，丢弃旧 lane。
    // 旧挂起输入**不得**跟随新 props 发给新终端（Goal #1）。
    const incomingToken = options.mountToken;
    const prevToken = lane.mountToken;
    const tokensDiffer = (incomingToken === undefined && prevToken === undefined)
      ? (lane.write !== write || lane.onError !== options.onError)
      : incomingToken !== prevToken;
    if (tokensDiffer) {
      if (lane.flushTimer !== null) clearTimeout(lane.flushTimer);
      inputLanes.delete(key);
      lane = undefined;
    }
  }
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
      generation: 0,
      mountToken: options.mountToken,
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
        if (inputLanes.get(key) !== lane) return;
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
  void drainPtyInput(key, lane);
}

async function drainPtyInput(
  key: string,
  lane: PtyInputLane,
): Promise<void> {
  // §B generation guard：drain 期间 lane 被换（unmount→remount 走 enqueuePtyInput
  // 的 mountToken diff 分支删了旧 lane），立即退出 — 旧 bytes 已被清理，不需要
  // 试图把已属于「过去」的队列灌到「新」的 PTY。同时也防止 enqueuePtyWrite
  // 内部的 lanes.generation 检查与本 lane 失配。
  //
  // 每次迭代从 `lane` 上**现读** write/onError：同一实例 mountToken 不变
  // → lane 不被换 → lane.write 仍是当前最新 enqueuePtyInput 的闭包 → 同实例
  // 内多次 enqueue 的 write 闭包变更能正确传递（修复前 drain 在启动时
  // 快照 write，导致后续 enqueue 的新闭包不会被调用，出现 'a','bc' 而不是
  // 'abc' 的分裂 bug）。
  while (inputLanes.get(key) === lane && lane.queued.length > 0) {
    const data = lane.queued;
    const write = lane.write;
    const onError = lane.onError;
    lane.queued = '';
    lane.queuedBytes = 0;
    lane.activeBytes = inputEncoder.encode(data).byteLength;
    try {
      await enqueuePtyWrite(key, () => {
        if (inputLanes.get(key) !== lane) return;
        return write(data);
      });
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
