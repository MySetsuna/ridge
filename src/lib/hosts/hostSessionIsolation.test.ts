import { describe, expect, it } from 'vitest';
import {
  cancelHostTask,
  checkHostTaskIsolation,
  isolationHeader,
  onIntentionalDisconnect,
  scheduleReconnectTask,
  stepHostTask,
} from './hostSessionIsolation';

describe('hostSessionIsolation (C56)', () => {
  it('detects shared pane claims', () => {
    const r = checkHostTaskIsolation([
      {
        hostId: 'h1',
        phase: 'Waiting',
        attempt: 0,
        attachedPaneIds: ['p1'],
        cancelled: false,
      },
      {
        hostId: 'h2',
        phase: 'Waiting',
        attempt: 0,
        attachedPaneIds: ['p1'],
        cancelled: false,
      },
    ]);
    expect(r.ok).toBe(false);
  });

  it('schedule cancels prior semantics via new task', () => {
    const t = scheduleReconnectTask(undefined, 'h1', ['a', 'a', 'b']);
    expect(t.attachedPaneIds).toEqual(['a', 'b']);
    expect(t.phase).toBe('Waiting');
  });

  it('step fails after max attempts when unreachable', () => {
    let t = scheduleReconnectTask(undefined, 'h1', ['p']);
    for (let i = 0; i < 4; i++) {
      t = stepHostTask(t, { hostReachable: false, maxAttempts: 4 });
    }
    expect(t.phase).toBe('Failed');
  });

  it('step succeeds when reachable', () => {
    let t = scheduleReconnectTask(undefined, 'h1', ['p']);
    t = stepHostTask(t, { hostReachable: true });
    expect(t.phase).toBe('Succeeded');
  });

  it('intentional disconnect cancels', () => {
    const t = scheduleReconnectTask(undefined, 'h1', ['p']);
    const c = onIntentionalDisconnect(t);
    expect(c?.phase).toBe('Cancelled');
    expect(cancelHostTask(t).cancelled).toBe(true);
  });

  it('header reports isolation', () => {
    const h = isolationHeader([
      {
        hostId: 'h1',
        phase: 'Waiting',
        attempt: 1,
        attachedPaneIds: ['p'],
        cancelled: false,
      },
    ]);
    expect(h).toMatch(/重连中/);
  });
});
