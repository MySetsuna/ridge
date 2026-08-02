import { describe, expect, it } from 'vitest';
import {
  allRequiredTreeKillSitesCovered,
  cancelRefreshCopy,
  clampGitConcurrency,
  GIT_CONCURRENCY_MAX,
  GIT_CONCURRENCY_MIN,
  pressureFromStats,
  shouldSurfaceGitGuard,
} from './processGuardPolicy';
import type { GitGuardStats } from './gitGuardStats';

const base: GitGuardStats = {
  activeChildren: 0,
  peakActiveChildren: 0,
  timeoutKills: 0,
  acquireTimeouts: 0,
  logicalConcurrencyCap: 12,
  concurrencyMin: 2,
  concurrencyMax: 12,
};

describe('processGuardPolicy (C52)', () => {
  it('clamps concurrency to dual-end constants', () => {
    expect(clampGitConcurrency(0)).toBe(GIT_CONCURRENCY_MIN);
    expect(clampGitConcurrency(99)).toBe(GIT_CONCURRENCY_MAX);
    expect(clampGitConcurrency(2)).toBe(2);
  });

  it('critical on timeout kills', () => {
    const v = pressureFromStats({ ...base, timeoutKills: 1 });
    expect(v.pressure).toBe('critical');
    expect(shouldSurfaceGitGuard({ ...base, timeoutKills: 1 })).toBe(true);
  });

  it('elevated at cap', () => {
    const v = pressureFromStats({ ...base, activeChildren: 12 });
    expect(v.pressure).toBe('elevated');
    expect(v.badge).toMatch(/git/);
  });

  it('spawn catalog covered', () => {
    expect(allRequiredTreeKillSitesCovered()).toBe(true);
  });

  it('cancel copy explains abort≠kill', () => {
    const v = pressureFromStats({ ...base, activeChildren: 2 });
    expect(cancelRefreshCopy(v)).toMatch(/杀进程树/);
  });
});
