import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RpcCancelledError, RpcTimeoutError, type RpcRequestOptions } from './types';
import {
  PaneInputQueueFullError,
  PaneRpcScheduler,
  type PaneRpcSchedulerOptions,
} from './paneRpcScheduler';
import type { PaneRef } from './wsRemote';

interface PendingCall {
  method: string;
  params: unknown;
  options: RpcRequestOptions | undefined;
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

class FakeRpc {
  readonly calls: PendingCall[] = [];
  readonly cancelledScopes: string[] = [];

  request<T = unknown>(
    method: string,
    params?: unknown,
    options?: RpcRequestOptions,
  ): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.calls.push({
        method,
        params,
        options,
        resolve: (value) => resolve((method === 'write_terminal_input' && value === undefined ? { sequence: (params as { input: { sequence: number } }).input.sequence, alreadyApplied: false } : value) as T),
        reject,
      });
    });
  }

  cancelScope(scope: string): number {
    this.cancelledScopes.push(scope);
    return 0;
  }
}

const pane: PaneRef = { workspaceId: 'workspace-a', paneId: 'pane-a' };

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

function createScheduler(rpc: FakeRpc, options: PaneRpcSchedulerOptions = {}): PaneRpcScheduler {
  return new PaneRpcScheduler(rpc, {
    inputSourceId: 'test-source',
    ...options,
  });
}

describe('PaneRpcScheduler input admission', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('keeps terminal input ordered and coalesces bytes behind one in-flight request', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);

    expect(scheduler.enqueueInput(pane, 'a', [{ ...pane }, 'epoch', 1] as const)).toBe(true);
    expect(scheduler.enqueueInput(pane, 'b', [{ ...pane }, 'epoch', 1] as const)).toBe(true);
    expect(scheduler.enqueueInput(pane, 'c', [{ ...pane }, 'epoch', 1] as const)).toBe(true);
    expect(rpc.calls).toHaveLength(1);
    expect(rpc.calls[0]).toMatchObject({
      method: 'write_terminal_input',
      params: {
        input: { pane, terminalEpoch: 'epoch', activationId: 1, bytes: [97], sourceId: 'test-source_1', sequence: 1 },
      },
      options: { scope: 'workspace-a:pane-a' },
    });

    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toMatchObject({
      input: { bytes: [98, 99], sourceId: 'test-source_1', sequence: 2 },
    });

    rpc.calls[1].resolve({ sequence: 2, alreadyApplied: false });
    await flushPromises();
    expect(scheduler.diagnostics).toMatchObject({
      inputCalls: 3,
      inputRequests: 2,
      inputBytesAccepted: 3,
      inputBytesCompleted: 3,
      queuedInputBytes: 0,
    });
  });

  it('preserves non-UTF8 mouse bytes and owns admitted buffers', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { inputBatchWindowMs: 4 });
    const bytes = new Uint8Array([27, 91, 77, 32, 255, 128]);
    scheduler.enqueueInput(pane, bytes, [pane, 'epoch', 1]);
    bytes.fill(0);
    await vi.advanceTimersByTimeAsync(4);
    expect(rpc.calls[0].params).toMatchObject({ input: { bytes: [27, 91, 77, 32, 255, 128] } });
  });

  it('splits large paste on the canonical byte limit without changing order', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);
    const bytes = new Uint8Array(70_000).map((_, index) => index % 256);
    scheduler.enqueueInput(pane, bytes, [pane, 'epoch', 1]);
    const first = (rpc.calls[0].params as { input: { bytes: number[] } }).input.bytes;
    expect(first).toHaveLength(65_536);
    rpc.calls[0].resolve(undefined);
    await flushPromises();
    const second = (rpc.calls[1].params as { input: { bytes: number[] } }).input.bytes;
    expect([...first, ...second]).toEqual(Array.from(bytes));
    expect(rpc.calls[1].params).toMatchObject({ input: { sequence: 2 } });
  });

  it('retires uncertain bytes on a new activation and rejects late old tasks', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);
    scheduler.enqueueInput(pane, 'old-active', [pane, 'old', 1]);
    scheduler.enqueueInput(pane, 'old-queued', [pane, 'old', 1]);
    scheduler.enqueueInput(pane, 'new', [pane, 'new', 2]);
    expect(scheduler.enqueueInput(pane, 'late', [pane, 'old', 1])).toBe(false);
    expect(scheduler.enqueueInput(pane, 'wrong', [pane, 'replacement', 2])).toBe(false);
    rpc.calls[0].resolve(undefined);
    await flushPromises();
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toMatchObject({ input: { terminalEpoch: 'new', activationId: 2, sequence: 1, sourceId: 'test-source_2' } });
    expect(scheduler.diagnostics.inputRejected).toBe(2);
  });

  it('keeps confirmed sequence identity across more than 64 view activations', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);
    for (let activation = 1; activation <= 80; activation++) {
      expect(scheduler.enqueueInput(pane, 'x', [pane, 'epoch', activation])).toBe(true);
      expect(rpc.calls[activation - 1].params).toMatchObject({ input: { sourceId: 'test-source_1', sequence: activation, activationId: activation } });
      rpc.calls[activation - 1].resolve({ sequence: activation, alreadyApplied: false });
      await flushPromises();
    }
    expect(scheduler.diagnostics.inputBytesCompleted).toBe(80);
  });

  it('pauses on malformed acknowledgement without dispatching further input', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);
    scheduler.enqueueInput(pane, 'a', [pane, 'epoch', 1]);
    scheduler.enqueueInput(pane, 'b', [pane, 'epoch', 1]);
    rpc.calls[0].resolve({ sequence: 2, alreadyApplied: false });
    await flushPromises();
    expect(rpc.calls).toHaveLength(1);
    expect(scheduler.diagnostics).toMatchObject({ inputBytesCompleted: 0, queuedInputBytes: 2, pausedLanes: 1 });
  });

  it('exposes bounded input latency and queue high-water diagnostics', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);
    scheduler.enqueueInput(pane, 'abc', [{ ...pane }, 'epoch', 1] as const);
    expect(scheduler.diagnostics.inputQueueHighWaterBytes).toBe(3);
    await vi.advanceTimersByTimeAsync(12);
    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    expect(scheduler.diagnostics).toMatchObject({
      inputLatencyP50Ms: 12,
      inputLatencyP95Ms: 12,
      queuedInputBytes: 0,
    });
  });

  it('optionally batches a synchronous input burst before the first wire send', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { inputBatchWindowMs: 4 });

    scheduler.enqueueInput(pane, 'a', [{ ...pane }, 'epoch', 1] as const);
    scheduler.enqueueInput(pane, 'b', [{ ...pane }, 'epoch', 1] as const);
    scheduler.enqueueInput(pane, 'c', [{ ...pane }, 'epoch', 1] as const);
    expect(rpc.calls).toHaveLength(0);

    await vi.advanceTimersByTimeAsync(4);
    expect(rpc.calls).toHaveLength(1);
    expect(rpc.calls[0].params).toMatchObject({ input: { bytes: [97, 98, 99], sequence: 1 } });
  });

  it('retries a timed-out batch with the same idempotency identity', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { backoffBaseMs: 100 });

    scheduler.enqueueInput(pane, 'hello', [{ ...pane }, 'epoch', 1] as const);
    const first = rpc.calls[0];
    first.reject(new RpcTimeoutError('write_to_pty', 1_000));
    await flushPromises();
    expect(rpc.calls).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(100);
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toEqual(first.params);
    expect(scheduler.diagnostics).toMatchObject({
      retries: 1,
      inputFailures: 1,
      timeoutFailures: 1,
    });

    rpc.calls[1].resolve({ sequence: 1, alreadyApplied: true });
    await flushPromises();
    expect(scheduler.diagnostics.queuedInputBytes).toBe(0);
  });

  it('rejects overflow explicitly without dropping already admitted input', () => {
    const rpc = new FakeRpc();
    const errors: Error[] = [];
    const scheduler = createScheduler(rpc, {
      maxQueuedInputBytes: 4,
      onError: (error) => errors.push(error),
    });

    expect(scheduler.enqueueInput(pane, 'ab', [{ ...pane }, 'epoch', 1] as const)).toBe(true);
    expect(scheduler.enqueueInput(pane, 'c', [{ ...pane }, 'epoch', 1] as const)).toBe(true);
    expect(scheduler.enqueueInput(pane, 'de', [{ ...pane }, 'epoch', 1] as const)).toBe(false);
    expect(rpc.calls).toHaveLength(1);
    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(PaneInputQueueFullError);
    expect(scheduler.diagnostics).toMatchObject({
      inputBytesAccepted: 3,
      inputRejected: 1,
      queuedInputBytes: 3,
    });
  });

  it('pauses after the failure threshold and resumes the retained batch explicitly', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, {
      backoffBaseMs: 10,
      pauseAfterFailures: 2,
    });

    scheduler.enqueueInput(pane, 'x', [{ ...pane }, 'epoch', 1] as const);
    const identity = rpc.calls[0].params;
    rpc.calls[0].reject(new RpcTimeoutError('write_to_pty', 1_000));
    await flushPromises();
    await vi.advanceTimersByTimeAsync(10);
    rpc.calls[1].reject(new RpcTimeoutError('write_to_pty', 1_000));
    await flushPromises();

    await vi.advanceTimersByTimeAsync(10_000);
    expect(rpc.calls).toHaveLength(2);
    expect(scheduler.diagnostics).toMatchObject({ pausedLanes: 1, queuedInputBytes: 1 });

    scheduler.resume(pane);
    expect(rpc.calls).toHaveLength(3);
    expect(rpc.calls[2].params).toEqual(identity);
  });
});

describe('PaneRpcScheduler resize and lifecycle admission', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('passes one bounded timeout to input and resize RPCs', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { rpcTimeoutMs: 75, resizeDebounceMs: 0 });

    scheduler.enqueueInput(pane, 'x', [{ ...pane }, 'epoch', 1] as const);
    expect(rpc.calls[0].options).toMatchObject({ scope: 'workspace-a:pane-a', timeoutMs: 75 });
    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();

    scheduler.scheduleResize(pane, 24, 80);
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls[1].options).toMatchObject({ scope: 'workspace-a:pane-a', timeoutMs: 75 });
    scheduler.dispose();
  });

  it('debounces resize and keeps only the latest dimensions while one request is active', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 40 });

    scheduler.scheduleResize(pane, 24, 80);
    scheduler.scheduleResize(pane, 30, 100);
    await vi.advanceTimersByTimeAsync(39);
    expect(rpc.calls).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(rpc.calls).toHaveLength(1);
    expect(rpc.calls[0]).toMatchObject({
      method: 'resize_pane',
      params: { workspaceId: 'workspace-a', paneId: 'pane-a', rows: 30, cols: 100 },
      options: { scope: 'workspace-a:pane-a' },
    });

    scheduler.scheduleResize(pane, 40, 120);
    scheduler.scheduleResize(pane, 50, 140);
    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    await vi.advanceTimersByTimeAsync(40);
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toMatchObject({ rows: 50, cols: 140 });
  });

  it('retries only the latest resize after timeout backoff', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 0, backoffBaseMs: 25 });

    scheduler.scheduleResize(pane, 24, 80);
    await vi.runOnlyPendingTimersAsync();
    rpc.calls[0].reject(new RpcTimeoutError('resize_pane', 1_000));
    scheduler.scheduleResize(pane, 40, 120);
    await flushPromises();
    await vi.advanceTimersByTimeAsync(25);

    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toMatchObject({ rows: 40, cols: 120 });
  });

  it('settles coalesced waiters after the latest resize and carries request context', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 10 });

    const first = scheduler.scheduleResizeAndWait(pane, 24, 80, { isAlt: false });
    const latest = scheduler.scheduleResizeAndWait(pane, 30, 100, { isAlt: true, isInlineTui: true });
    await vi.advanceTimersByTimeAsync(10);
    expect(rpc.calls[0].params).toMatchObject({
      rows: 30,
      cols: 100,
      isAlt: true,
      isInlineTui: true,
    });
    let firstSettled = false;
    let latestSettled = false;
    void first.then(() => { firstSettled = true; });
    void latest.then(() => { latestSettled = true; });

    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    expect(firstSettled).toBe(true);
    expect(latestSettled).toBe(true);
  });

  it('rejects a pending resize waiter when its pane is retired', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 40 });
    const pending = scheduler.scheduleResizeAndWait(pane, 24, 80);

    scheduler.retire(pane);
    await expect(pending).rejects.toBeInstanceOf(RpcCancelledError);
  });

  it('suppresses invalid and already active or applied resize values', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 10 });

    scheduler.scheduleResize(pane, 0, 80);
    scheduler.scheduleResize(pane, 24, 80);
    scheduler.scheduleResize(pane, 24, 80);
    await vi.advanceTimersByTimeAsync(10);
    expect(rpc.calls).toHaveLength(1);

    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    scheduler.scheduleResize(pane, 24, 80);
    await vi.advanceTimersByTimeAsync(10);
    expect(rpc.calls).toHaveLength(1);
    expect(scheduler.diagnostics).toMatchObject({
      resizeCalls: 4,
      resizeRequests: 1,
      resizeSuppressed: 3,
    });
  });

  it('force-sends a same-size resize after the last applied grid', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 0 });

    expect(scheduler.scheduleResize(pane, 36, 110)).toBe(true);
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls).toHaveLength(1);
    expect(rpc.calls[0].params).toMatchObject({ rows: 36, cols: 110 });
    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();

    expect(scheduler.scheduleResize(pane, 36, 110)).toBe(false);
    expect(scheduler.scheduleResize(pane, 36, 110, undefined, { force: true })).toBe(true);
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].method).toBe('resize_pane');
    expect(rpc.calls[1].params).toMatchObject({
      workspaceId: pane.workspaceId,
      paneId: pane.paneId,
      rows: 36,
      cols: 110,
    });
    expect(rpc.calls[1].params).not.toMatchObject({ rows: 24, cols: 80 });
  });

  it('does not suppress a same-size resize when PTY mode context changes', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 0 });

    scheduler.scheduleResize(pane, 24, 80, { isAlt: false, isInlineTui: false });
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls).toHaveLength(1);

    scheduler.scheduleResize(pane, 24, 80, { isInlineTui: true, isAlt: false });
    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    await vi.runOnlyPendingTimersAsync();

    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toMatchObject({
      rows: 24,
      cols: 80,
      isAlt: false,
      isInlineTui: true,
    });
  });

  it('suppresses an identical contextual resize while the request is active', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 0 });
    const context = { isAlt: true, isInlineTui: false };

    expect(scheduler.scheduleResize(pane, 24, 80, context)).toBe(true);
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls).toHaveLength(1);

    expect(scheduler.scheduleResize(pane, 24, 80, { ...context })).toBe(false);
    rpc.calls[0].resolve({ sequence: 1, alreadyApplied: false });
    await flushPromises();
    await vi.runOnlyPendingTimersAsync();

    expect(rpc.calls).toHaveLength(1);
    expect(scheduler.diagnostics.resizeSuppressed).toBe(1);
  });

  it('retires the lane, cancels its scope, and prevents delayed work from resurrecting', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 40 });

    scheduler.enqueueInput(pane, 'x', [{ ...pane }, 'epoch', 1] as const);
    scheduler.scheduleResize(pane, 24, 80);
    scheduler.retire(pane);
    expect(rpc.cancelledScopes).toEqual(['workspace-a:pane-a']);
    expect(scheduler.diagnostics.queuedInputBytes).toBe(0);

    rpc.calls[0].reject(new RpcTimeoutError('write_to_pty', 1_000));
    await flushPromises();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(rpc.calls).toHaveLength(1);
  });

  it('prunes only stale pane scopes and reports exactly which lanes retired', () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc);
    const livePane: PaneRef = { workspaceId: 'workspace-a', paneId: 'live' };
    const stalePane: PaneRef = { workspaceId: 'workspace-a', paneId: 'stale' };

    scheduler.enqueueInput(livePane, 'a', [{ ...livePane }, 'epoch', 1] as const);
    scheduler.enqueueInput(stalePane, 'b', [{ ...stalePane }, 'epoch', 1] as const);
    const retired = scheduler.prune(new Set(['workspace-a:live']));

    expect(retired).toEqual(['workspace-a:stale']);
    expect(rpc.cancelledScopes).toEqual(['workspace-a:stale']);
    expect(scheduler.diagnostics.queuedInputBytes).toBe(1);
  });
});
