/**
 * AC4-C7: remote HITL audit client helpers (redacted history).
 */

export type HitlInvoke = (
  method: string,
  params?: Record<string, unknown>,
) => Promise<unknown>;

export interface HitlAuditItem {
  id: string;
  ts: number;
  source: string;
  initiator: string;
  verdict: string;
  riskLevel: string;
  reasonSummary: string;
  outcome: string;
}

export interface HitlAuditList {
  items: HitlAuditItem[];
  cap: number;
}

export async function fetchHitlAuditRemote(
  invoke: HitlInvoke,
  limit = 20,
): Promise<HitlAuditList> {
  const raw = (await invoke('list_hitl_audit_remote', { limit })) as HitlAuditList;
  return {
    items: Array.isArray(raw?.items) ? raw.items : [],
    cap: Number(raw?.cap ?? 50),
  };
}

/** Pure filter: only terminal outcomes for badge count. */
export function countTerminalOutcomes(items: HitlAuditItem[]): {
  approved: number;
  rejected: number;
  other: number;
} {
  let approved = 0;
  let rejected = 0;
  let other = 0;
  for (const i of items) {
    const v = (i.verdict || '').toLowerCase();
    if (v === 'approve' || v === 'approved') approved++;
    else if (v === 'reject' || v === 'rejected') rejected++;
    else other++;
  }
  return { approved, rejected, other };
}

export function formatAuditLine(i: HitlAuditItem): string {
  return `${i.source}:${i.verdict} · ${i.initiator} · ${i.reasonSummary || '—'} · ${i.outcome}`;
}

/** Whether an audit item is safe for remote display (no command body fields). */
export function isRedactedAuditItem(i: Record<string, unknown>): boolean {
  if ('action' in i || 'command' in i || 'replacement' in i) return false;
  return typeof i.id === 'string' && typeof i.verdict === 'string';
}
