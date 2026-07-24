import { describe, expect, it } from 'vitest';
import {
  auditPanelTitle,
  buildHitlAuditPanel,
  shouldShowAuditSection,
} from './hitlAuditPanel';
import type { HitlAuditItem } from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';

const item = (over: Partial<HitlAuditItem> = {}): HitlAuditItem => ({
  id: '1',
  ts: 1,
  source: 'remote',
  initiator: 'a',
  verdict: 'approve',
  riskLevel: 'Dangerous',
  reasonSummary: 'rm',
  outcome: 'consumed',
  ...over,
});

describe('hitlAuditPanel', () => {
  it('builds counts and lines', () => {
    const m = buildHitlAuditPanel([
      item(),
      item({ id: '2', verdict: 'reject' }),
    ]);
    expect(m.approved).toBe(1);
    expect(m.rejected).toBe(1);
    expect(m.lines).toHaveLength(2);
    expect(auditPanelTitle(m)).toContain('✓1');
    expect(shouldShowAuditSection(0, 2)).toBe(true);
    expect(shouldShowAuditSection(0, 0)).toBe(false);
  });

  it('empty title', () => {
    const m = buildHitlAuditPanel([]);
    expect(m.empty).toBe(true);
    expect(auditPanelTitle(m)).toMatch(/空/);
  });
});
