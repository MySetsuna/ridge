import { describe, expect, it, vi } from 'vitest';
import {
  countTerminalOutcomes,
  fetchHitlAuditRemote,
  formatAuditLine,
  isRedactedAuditItem,
  type HitlAuditItem,
} from './hitlAuditRemote';

const sample: HitlAuditItem = {
  id: 'aud_1',
  ts: 1,
  source: 'remote',
  initiator: 'agent-a',
  verdict: 'approve',
  riskLevel: 'Dangerous',
  reasonSummary: 'rm',
  outcome: 'consumed',
};

describe('hitlAuditRemote', () => {
  it('fetchHitlAuditRemote maps invoke payload', async () => {
    const invoke = vi.fn(async () => ({
      items: [sample],
      cap: 50,
    }));
    const list = await fetchHitlAuditRemote(invoke, 10);
    expect(invoke).toHaveBeenCalledWith('list_hitl_audit_remote', { limit: 10 });
    expect(list.items).toHaveLength(1);
    expect(list.cap).toBe(50);
  });

  it('countTerminalOutcomes', () => {
    expect(
      countTerminalOutcomes([
        sample,
        { ...sample, id: '2', verdict: 'reject' },
        { ...sample, id: '3', verdict: 'timeout' },
      ]),
    ).toEqual({ approved: 1, rejected: 1, other: 1 });
  });

  it('formatAuditLine and redaction guard', () => {
    expect(formatAuditLine(sample)).toContain('remote:approve');
    expect(isRedactedAuditItem({ ...sample })).toBe(true);
    expect(isRedactedAuditItem({ ...sample, action: 'rm -rf' })).toBe(false);
  });
});
