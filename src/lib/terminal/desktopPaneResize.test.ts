import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  PaneRpcScheduler,
  type PaneRpcSchedulerOptions,
} from '@ridge/remote/shared/transport/paneRpcScheduler';
import type { RpcRequestOptions } from '@ridge/remote/shared/transport/types';
import type { PaneRef } from '@ridge/remote/shared/transport/paneRef';
import { scheduleForcedPaneResize } from './desktopPaneResize';

interface PendingCall {
  method: string;
  params: unknown;
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

class FakeRpc {
  readonly calls: PendingCall[] = [];

  request<T = unknown>(method: string, params?: unknown, _options?: RpcRequestOptions): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      this.calls.push({
        method,
        params,
        resolve: (value) => resolve(value as T),
        reject,
      });
    });
  }

  cancelScope(): number {
    return 0;
  }
}

const pane: PaneRef = { workspaceId: 'ws-remote', paneId: 'pane-remote' };

function createScheduler(rpc: FakeRpc, options: PaneRpcSchedulerOptions = {}): PaneRpcScheduler {
  return new PaneRpcScheduler(rpc, { inputSourceId: 'desktop_resize', ...options });
}

const ridgePaneSource = readFileSync(new URL('../components/RidgePane.svelte', import.meta.url), 'utf8');

describe('WEB_REMOTE / desktop claimed resize remount', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('re-issues resize_pane when a second same-size PC refresh claims the measured grid', async () => {
    expect(ridgePaneSource).toContain('scheduleForcedPaneResize(');
    expect(ridgePaneSource).toContain('function refreshForRemote()');
    expect(ridgePaneSource).toContain('synchronizePaneSize(paneId)');
    expect(ridgePaneSource).toContain('function onPtyResize(');

    const rpc = new FakeRpc();
    const scheduler = createScheduler(rpc, { resizeDebounceMs: 0 });
    const measured = { rows: 36, cols: 110, isAlt: false, isInlineTui: false };

    const first = scheduleForcedPaneResize(scheduler, pane, measured.rows, measured.cols, {
      isAlt: measured.isAlt,
      isInlineTui: measured.isInlineTui,
    });
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls).toHaveLength(1);
    expect(rpc.calls[0].method).toBe('resize_pane');
    expect(rpc.calls[0].params).toMatchObject({
      workspaceId: pane.workspaceId,
      paneId: pane.paneId,
      rows: 36,
      cols: 110,
    });
    expect(rpc.calls[0].params).not.toMatchObject({ rows: 24, cols: 80 });
    rpc.calls[0].resolve(undefined);
    await first;

    expect(scheduler.scheduleResize(pane, 36, 110, { isAlt: false, isInlineTui: false })).toBe(false);
    expect(rpc.calls).toHaveLength(1);

    const second = scheduleForcedPaneResize(scheduler, pane, measured.rows, measured.cols, {
      isAlt: measured.isAlt,
      isInlineTui: measured.isInlineTui,
    });
    await vi.runOnlyPendingTimersAsync();
    expect(rpc.calls).toHaveLength(2);
    expect(rpc.calls[1].method).toBe('resize_pane');
    expect(rpc.calls[1].params).toEqual(rpc.calls[0].params);
    rpc.calls[1].resolve(undefined);
    await second;
  });
});
