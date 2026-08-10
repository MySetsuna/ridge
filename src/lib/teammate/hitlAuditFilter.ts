/**
 * CONTRACT-53 / OP-AGENT-CP: HITL audit filter + redaction product model.
 */

import type { HitlAuditItem } from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';

export type AuditVerdict = string;
export type AuditSource = string;

export interface AuditFilter {
  verdict?: AuditVerdict;
  source?: AuditSource;
  initiatorSubstr?: string;
  riskLevel?: string;
  limit: number;
}

export interface AuditFilterResult {
  items: HitlAuditItem[];
  totalMatched: number;
  truncated: boolean;
  summary: string;
}

export const DEFAULT_FILTER: AuditFilter = {
  verdict: 'all',
  source: 'all',
  limit: 20,
};

/** Redact free-text that might leak secrets in reason summaries. */
export function redactReasonSummary(raw: string, maxLen = 120): string {
  let s = (raw || '').replace(/\s+/g, ' ').trim();
  // Strip common secret-looking tokens
  s = s.replace(/(api[_-]?key|token|secret|password|authorization)\s*[:=]\s*\S+/gi, '$1=***');
  s = s.replace(/\b[A-Za-z0-9_-]{32,}\b/g, '***');
  if (s.length > maxLen) s = s.slice(0, maxLen - 1) + '…';
  return s;
}

export function filterAuditItems(
  items: HitlAuditItem[],
  filter: Partial<AuditFilter> = {},
): AuditFilterResult {
  const f: AuditFilter = { ...DEFAULT_FILTER, ...filter };
  const lim = Math.max(1, Math.min(f.limit || 20, 50));
  let matched = items.filter((it) => {
    if (f.verdict && f.verdict !== 'all' && it.verdict !== f.verdict) return false;
    if (f.source && f.source !== 'all' && it.source !== f.source) return false;
    if (f.riskLevel && f.riskLevel !== 'all' && it.riskLevel !== f.riskLevel) return false;
    if (f.initiatorSubstr) {
      const sub = f.initiatorSubstr.toLowerCase();
      if (!(it.initiator || '').toLowerCase().includes(sub)) return false;
    }
    return true;
  });
  // newest first if ts present
  matched = [...matched].sort((a, b) => (b.ts || 0) - (a.ts || 0));
  const totalMatched = matched.length;
  const truncated = totalMatched > lim;
  const slice = matched.slice(0, lim).map((it) => ({
    ...it,
    reasonSummary: redactReasonSummary(it.reasonSummary || ''),
  }));
  const approved = slice.filter((i) => i.verdict === 'approve').length;
  const rejected = slice.filter((i) => i.verdict === 'reject' || i.verdict === 'timeout').length;
  const summary = `匹配 ${totalMatched} · 展示 ${slice.length} · ✓${approved} ✗${rejected}`;
  return { items: slice, totalMatched, truncated, summary };
}

export function auditRiskBadge(risk: string | undefined): string {
  switch ((risk || '').toLowerCase()) {
    case 'dangerous':
      return '危';
    case 'caution':
      return '慎';
    case 'safe':
      return '安';
    default:
      return risk || '';
  }
}

export function groupByVerdict(items: HitlAuditItem[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const it of items) {
    const k = it.verdict || 'unknown';
    out[k] = (out[k] || 0) + 1;
  }
  return out;
}

/** Whether remote list may include this field (redaction contract). */
export const REMOTE_AUDIT_ALLOWED_KEYS = new Set([
  'id',
  'ts',
  'source',
  'initiator',
  'verdict',
  'riskLevel',
  'reasonSummary',
  'outcome',
]);

export const REMOTE_AUDIT_FORBIDDEN_KEYS = [
  'action',
  'command',
  'commandText',
  'payload',
  'nonce',
  'fullReason',
];

export function assertRemoteAuditShape(obj: Record<string, unknown>): string[] {
  const leaks: string[] = [];
  for (const k of REMOTE_AUDIT_FORBIDDEN_KEYS) {
    if (k in obj) leaks.push(k);
  }
  return leaks;
}

export function formatAuditTimeline(items: HitlAuditItem[], max = 8): string[] {
  return items.slice(0, max).map((it) => {
    const t = it.ts ? new Date(it.ts).toISOString().slice(11, 19) : '--:--:--';
    let v = '?';
    if (it.verdict === 'approve') v = '✓';
    else if (it.verdict === 'reject') v = '✗';
    else if (it.verdict === 'timeout') v = '⏱';
    return `${t} ${v} ${it.initiator || '?'} · ${redactReasonSummary(it.reasonSummary || '', 40)}`;
  });
}
