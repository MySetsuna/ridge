import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RpcTimeoutError, type RpcRequestOptions } from './types';
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
        resolve: (value) => resolve(value as T),
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

    expect(scheduler.enqueueInput(pane, 'a')).toBe(true);
    expect(scheduler.enqueueInput(pane, 'b')).toBe(true);
    expect(scheduler.enqueueInput(pane, 'c')).toBe(true);
    expect(rpc.calls).toHaveLength(1);
    expect(rpc.calls[0]).toMatchObject({
      method: 'write_to_pty',
      params: {
        workspaceId: 'workspace-a',
        paneId: 'pane-a',
        data: 'a',
        inputSourceId: 'test-source_1',
        inputSequence: 1,
      },
      options: { scope: 'workspace-a:pane-a' },
    });

    rpc.calls[0].resolve(undefined);
    await flushPromises();
    expect(rpc.calls).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(8);
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].params).toMatchObject({
      data: 'bc',
      inputSourceId: 'test-source_1',
      inputSequence: 2,
    });

    rpc.calls[1].resolve(undefined);
    await flushPromises();
    expect(scheduler.diagnostics).toMatchObject({
      inputCalls: 3,
      inputRequests: 2,
      inputBytesAccepted: 3,
      inputBytesCompleted: 3,
      queuedInputBytes: 0,
    });
  });

  it('retries a timed-out batch with the same idempotency identity', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { backoffBaseMs: 100 });

    scheduler.enqueueInput(pane, 'hello');
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

    rpc.calls[1].resolve(undefined);
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

    expect(scheduler.enqueueInput(pane, 'ab')).toBe(true);
    expect(scheduler.enqueueInput(pane, 'c')).toBe(true);
    expect(scheduler.enqueueInput(pane, 'de')).toBe(false);
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

    scheduler.enqueueInput(pane, 'x');
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
    rpc.calls[0].resolve(undefined);
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

  it('suppresses invalid and already active or applied resize values', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 10 });

    scheduler.scheduleResize(pane, 0, 80);
    scheduler.scheduleResize(pane, 24, 80);
    scheduler.scheduleResize(pane, 24, 80);
    await vi.advanceTimersByTimeAsync(10);
    expect(rpc.calls).toHaveLength(1);

    rpc.calls[0].resolve(undefined);
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

  it('retires the lane, cancels its scope, and prevents delayed work from resurrecting', async () => {
    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 40 });

    scheduler.enqueueInput(pane, 'x');
    scheduler.scheduleResize(pane, 24, 80);
    scheduler.retire(pane);
    expect(rpc.cancelledScopes).toEqual(['workspace-a:pane-a']);
    expect(scheduler.diagnostics.queuedInputBytes).toBe(0);

    rpc.calls[0].reject(new RpcTimeoutError('write_to_pty', 1_000));
    await flushPromises();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(rpc.calls).toHaveLength(1);
  });
});
