import { describe, expect, it, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import {
  hostIsolationTasks,
  scheduleIsolationTask,
  isolationBadge,
  sleepMsForAttempt,
} from './hostReconnect';
import { checkHostTaskIsolation } from '$lib/hosts/hostSessionIsolation';

describe('hostReconnect product path (C56 isolation wiring)', () => {
  beforeEach(() => {
    hostIsolationTasks.set({});
  });

  it('scheduleIsolationTask registers and badges', () => {
    const t = scheduleIsolationTask('h1', ['p1', 'p1', 'p2']);
    expect(t.attachedPaneIds).toEqual(['p1', 'p2']);
    expect(get(hostIsolationTasks)['h1']?.phase).toBe('Waiting');
    // Waiting with attempt 0 may show reconnect badge
    const badge = isolationBadge('h1');
    expect(typeof badge).toBe('string');
  });

  it('multi-host isolation ok when panes distinct', () => {
    scheduleIsolationTask('h1', ['a']);
    scheduleIsolationTask('h2', ['b']);
    const check = checkHostTaskIsolation(Object.values(get(hostIsolationTasks)));
    expect(check.ok).toBe(true);
  });

  it('sleepMsForAttempt mirrors outbound schedule', () => {
    expect(sleepMsForAttempt(0)).toBe(200);
    expect(sleepMsForAttempt(99)).toBeNull();
  });
});
