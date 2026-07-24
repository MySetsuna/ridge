import { describe, expect, it } from 'vitest';
import { gitGuardNeedsAttention, type GitGuardStats } from './gitGuardStats';

const base: GitGuardStats = {
  activeChildren: 0,
  peakActiveChildren: 0,
  timeoutKills: 0,
  acquireTimeouts: 0,
  logicalConcurrencyCap: 4,
  concurrencyMin: 2,
  concurrencyMax: 12,
};

describe('gitGuardNeedsAttention', () => {
  it('false when null or clean', () => {
    expect(gitGuardNeedsAttention(null)).toBe(false);
    expect(gitGuardNeedsAttention(base)).toBe(false);
  });

  it('true on timeout kills or acquire timeouts', () => {
    expect(gitGuardNeedsAttention({ ...base, timeoutKills: 1 })).toBe(true);
    expect(gitGuardNeedsAttention({ ...base, acquireTimeouts: 2 })).toBe(true);
  });

  it('true when active at cap', () => {
    expect(
      gitGuardNeedsAttention({ ...base, activeChildren: 4, logicalConcurrencyCap: 4 }),
    ).toBe(true);
  });
});
