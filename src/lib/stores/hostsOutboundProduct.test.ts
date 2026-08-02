/**
 * Product-path tests: hosts store uses livePumpPolicy / outboundLifecycle /
 * foreignHistorySession (not compositionHarness-only).
 */
import { describe, expect, it, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import {
  notePumpBatch,
  pumpBadgeForHost,
  noteLifecycleSubscribe,
  noteLifecycleDetach,
  noteLifecycleFanout,
  outboundPumpByHost,
  outboundLifecycleByKey,
  foreignHistoryByKey,
  attachSeedPlanForSession,
  hostOperatorAlert,
  collectAttachedRemotePanes,
  isRemotePaneAttached,
  remotePaneKey,
} from './hosts';

describe('hosts product path (C50/C54/C58 wiring)', () => {
  beforeEach(() => {
    outboundPumpByHost.set({});
    outboundLifecycleByKey.set({});
    foreignHistoryByKey.set({});
  });

  it('notePumpBatch updates pump store and badge', () => {
    notePumpBatch('h1', 40, 100);
    notePumpBatch('h1', 80, 100);
    const st = get(outboundPumpByHost)['h1'];
    expect(st).toBeTruthy();
    expect(st!.bufferedBytes).toBeLessThanOrEqual(100);
    expect(st!.droppedBytes).toBeGreaterThan(0);
    expect(pumpBadgeForHost('h1')).toMatch(/丢|泵/);
  });

  it('lifecycle subscribe → fanout → detach', () => {
    noteLifecycleSubscribe('h1', 's1');
    noteLifecycleFanout('h1', 64);
    const key = 'h1\0s1';
    let s = get(outboundLifecycleByKey)[key];
    expect(s?.subscribed).toBe(true);
    expect(s?.fanoutBytes).toBe(64);
    noteLifecycleDetach('h1', 's1');
    s = get(outboundLifecycleByKey)[key];
    expect(s?.phase).toBe('Detached');
    expect(s?.subscribed).toBe(false);
  });

  it('attach seed plan from foreign history store', () => {
    foreignHistoryByKey.set({
      'h1\0s1': { hostId: 'h1', sessionId: 's1', bytes: 2048, cap: 65536 },
    });
    const plan = attachSeedPlanForSession('h1', 's1', false);
    expect(plan.seedBeforeLive).toBe(true);
    expect(plan.seedBytes).toBe(2048);
  });

  it('operator alert surfaces reconnect or backpressure', () => {
    notePumpBatch('h2', 200, 100);
    const a = hostOperatorAlert('h2', 0);
    expect(a).toBeTruthy();
    const b = hostOperatorAlert('h3', 2);
    expect(b).toMatch(/重连/);
  });
  it('keeps attached state scoped to workspace when pane ids repeat', () => {
    const index = collectAttachedRemotePanes([
      { workspaceId: 'workspace-a', remoteSessionId: 'pane-1', attached: true },
      { workspaceId: 'workspace-b', remoteSessionId: 'pane-1', attached: false },
    ]);

    expect(remotePaneKey('workspace-a', 'pane-1')).not.toBe(remotePaneKey('workspace-b', 'pane-1'));
    expect(isRemotePaneAttached(index, 'workspace-a', 'pane-1', 2)).toBe(true);
    expect(isRemotePaneAttached(index, 'workspace-b', 'pane-1', 2)).toBe(false);
  });

  it('only applies legacy unscoped attachment when pane id is unambiguous', () => {
    const index = collectAttachedRemotePanes([
      { remoteSessionId: 'legacy-pane', attached: true },
    ]);

    expect(isRemotePaneAttached(index, 'workspace-a', 'legacy-pane', 1)).toBe(true);
    expect(isRemotePaneAttached(index, 'workspace-a', 'legacy-pane', 2)).toBe(false);
  });
});
