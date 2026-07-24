import { describe, expect, it } from 'vitest';
import {
  buildOrchControlModel,
  healthPollMs,
  remoteRosterDot,
  shouldRefreshHealth,
} from './orchControlPlane';

describe('orchControlPlane (C57)', () => {
  it('degraded when suspended', () => {
    const m = buildOrchControlModel({
      suspendedAgents: 2,
      pendingHitl: 0,
      hitlEnabled: false,
      degraded: true,
      level: 'degraded',
      generation: 3,
    });
    expect(m.level).toBe('degraded');
    expect(m.badge).toMatch(/暂停/);
    expect(m.showPauseAllHint).toBe(true);
  });

  it('watch with foreign hosts', () => {
    const m = buildOrchControlModel({
      suspendedAgents: 0,
      pendingHitl: 0,
      hitlEnabled: true,
      foreignAttached: 2,
      outboundHostsConnected: 1,
      level: 'watch',
      generation: 1,
    });
    expect(m.level).toBe('watch');
    expect(m.lines.some((l) => l.includes('foreign'))).toBe(true);
  });

  it('hitl strip when pending', () => {
    const m = buildOrchControlModel({
      suspendedAgents: 0,
      pendingHitl: 1,
      hitlEnabled: true,
      degraded: true,
    });
    expect(m.showHitlStrip).toBe(true);
    expect(remoteRosterDot(m, false)).toBe('bad');
  });

  it('poll timing', () => {
    const ok = buildOrchControlModel({ level: 'ok', degraded: false });
    const deg = buildOrchControlModel({ level: 'degraded', degraded: true, suspendedAgents: 1 });
    expect(healthPollMs(deg)).toBeLessThan(healthPollMs(ok));
    expect(shouldRefreshHealth(1, 2, 5000, 0, 100)).toBe(true);
    expect(shouldRefreshHealth(1, 1, 5000, 0, 100)).toBe(false);
    expect(shouldRefreshHealth(1, 1, 5000, 0, 6000)).toBe(true);
  });
});
