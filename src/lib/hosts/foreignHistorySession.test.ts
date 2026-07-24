import { describe, expect, it } from 'vitest';
import {
  appendCappedPlan,
  assertSessionIsolation,
  clampHistoryCap,
  DEFAULT_HISTORY_TAIL_CAP,
  historyPullBudget,
  planAttachSeed,
  planDetachHistory,
  showHistoryStrip,
  summarizeHistoryBadge,
} from './foreignHistorySession';

describe('foreignHistorySession (C50)', () => {
  it('pull budget clamps', () => {
    expect(historyPullBudget(24, 80)).toBeGreaterThanOrEqual(1024);
    expect(historyPullBudget(10_000, 500)).toBeLessThanOrEqual(DEFAULT_HISTORY_TAIL_CAP);
    expect(clampHistoryCap(10)).toBe(1024);
    expect(clampHistoryCap(999_999_999)).toBe(DEFAULT_HISTORY_TAIL_CAP * 4);
  });

  it('attach seeds local tail once', () => {
    const p = planAttachSeed({
      localTailBytes: 4096,
      rows: 24,
      cols: 80,
      reattach: false,
      hostHistoryKnown: false,
    });
    expect(p.seedBeforeLive).toBe(true);
    expect(p.seedBytes).toBe(4096);
    expect(p.clearAfterSeed).toBe(false);
  });

  it('reattach clears after seed to avoid double feed', () => {
    const p = planAttachSeed({
      localTailBytes: 100,
      rows: 24,
      cols: 80,
      reattach: true,
      hostHistoryKnown: true,
    });
    expect(p.clearAfterSeed).toBe(true);
  });

  it('detach never kills remote', () => {
    const d = planDetachHistory({ keepLocalTail: true });
    expect(d.killRemote).toBe(false);
    expect(d.clearLocalTail).toBe(false);
  });

  it('append capped plan matches ring semantics', () => {
    const a = appendCappedPlan(0, 10, 8);
    expect(a.finalLen).toBe(8);
    expect(a.keepIncomingFrom).toBe(2);
    const b = appendCappedPlan(6, 4, 8);
    expect(b.dropFromHead).toBe(2);
    expect(b.finalLen).toBe(8);
  });

  it('session isolation detects collisions', () => {
    const ok = assertSessionIsolation([
      { hostId: 'h1', sessionId: 's1' },
      { hostId: 'h2', sessionId: 's1' },
    ]);
    expect(ok.ok).toBe(true);
    const bad = assertSessionIsolation([
      { hostId: 'h1', sessionId: 's1' },
      { hostId: 'h1', sessionId: 's1' },
    ]);
    expect(bad.ok).toBe(false);
  });

  it('badge and strip', () => {
    expect(summarizeHistoryBadge(null)).toBe('');
    expect(summarizeHistoryBadge({ hostId: 'h', sessionId: 's', bytes: 500, cap: 1024 })).toMatch(
      /历史/,
    );
    expect(showHistoryStrip(100, false)).toBe(true);
    expect(showHistoryStrip(100, true)).toBe(false);
  });
});
