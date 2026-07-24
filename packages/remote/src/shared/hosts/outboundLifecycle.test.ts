import { describe, expect, it } from 'vitest';
import {
  assertNoCrossHostFanout,
  createSession,
  lifecycleSummary,
  reduceLifecycle,
  safeSubscribe,
  simulateDetachDoesNotError,
  simulateHappyPath,
} from './outboundLifecycle';

describe('outboundLifecycle (C58)', () => {
  it('happy path reaches Live with counters', () => {
    const s = simulateHappyPath('h1', 'p1');
    expect(s.phase).toBe('Live');
    expect(s.subscribed).toBe(true);
    expect(s.writeOk).toBe(1);
    expect(s.fanoutBytes).toBe(128);
  });

  it('detach view clears subscribe without Error', () => {
    const s = simulateDetachDoesNotError('h1', 'p1');
    expect(s.phase).toBe('Detached');
    expect(s.subscribed).toBe(false);
    expect(s.phase).not.toBe('Error');
  });

  it('safeSubscribe is idempotent', () => {
    let s = createSession('h', 'p');
    s = safeSubscribe(s, 'p');
    const s2 = safeSubscribe(s, 'p');
    expect(s2).toEqual(s);
  });

  it('disconnect unintentional → Reconnecting', () => {
    let s = simulateHappyPath('h', 'p');
    s = reduceLifecycle(s, { type: 'disconnect', intentional: false });
    expect(s.phase).toBe('Reconnecting');
  });

  it('cross-host fanout guard', () => {
    const sessions = [
      createSession('h1', 'p1'),
      { ...createSession('h2', 'p1'), hostId: 'h2' },
    ];
    expect(assertNoCrossHostFanout(sessions, 'h1', 'p1')).toBe(false);
    expect(assertNoCrossHostFanout([createSession('h1', 'p1')], 'h1', 'p1')).toBe(true);
  });

  it('summary string', () => {
    expect(lifecycleSummary(simulateHappyPath('h', 'p'))).toMatch(/Live/);
  });
});
