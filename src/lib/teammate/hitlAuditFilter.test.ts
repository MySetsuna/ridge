import { describe, expect, it } from 'vitest';
import {
  assertRemoteAuditShape,
  filterAuditItems,
  formatAuditTimeline,
  groupByVerdict,
  redactReasonSummary,
} from './hitlAuditFilter';
import type { HitlAuditItem } from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';

const items: HitlAuditItem[] = [
  {
    id: '1',
    ts: 3000,
    source: 'desktop',
    initiator: 'agent-a',
    verdict: 'approve',
    riskLevel: 'Dangerous',
    reasonSummary: 'rm -rf /tmp/x api_key=supersecrettokenvalue123456789012',
    outcome: 'consumed',
  },
  {
    id: '2',
    ts: 2000,
    source: 'remote',
    initiator: 'agent-b',
    verdict: 'reject',
    riskLevel: 'Dangerous',
    reasonSummary: 'curl evil',
    outcome: 'blocked',
  },
  {
    id: '3',
    ts: 1000,
    source: 'timeout',
    initiator: 'agent-a',
    verdict: 'timeout',
    riskLevel: 'Dangerous',
    reasonSummary: 'slow',
    outcome: 'timeout',
  },
];

describe('hitlAuditFilter (C53)', () => {
  it('redacts secrets in reason', () => {
    const r = redactReasonSummary(items[0]!.reasonSummary);
    expect(r).not.toMatch(/supersecret/);
    expect(r).toMatch(/api_key=\*\*\*/i);
  });

  it('filters by verdict and initiator', () => {
    const r = filterAuditItems(items, { verdict: 'approve', limit: 10 });
    expect(r.items).toHaveLength(1);
    expect(r.items[0]!.id).toBe('1');
    const a = filterAuditItems(items, { initiatorSubstr: 'agent-a', limit: 10 });
    expect(a.totalMatched).toBe(2);
  });

  it('truncates and summarizes', () => {
    const r = filterAuditItems(items, { limit: 1 });
    expect(r.truncated).toBe(true);
    expect(r.items).toHaveLength(1);
    expect(r.summary).toMatch(/匹配/);
  });

  it('forbids command fields on remote shape', () => {
    expect(assertRemoteAuditShape({ id: '1', command: 'rm' })).toContain('command');
    expect(assertRemoteAuditShape({ id: '1', verdict: 'approve' })).toEqual([]);
  });

  it('groups and timeline', () => {
    expect(groupByVerdict(items).approve).toBe(1);
    expect(formatAuditTimeline(items, 2)).toHaveLength(2);
  });
});
