/**
 * Agent Center HITL audit panel pure model (thickens C7 product path).
 */
import {
  countTerminalOutcomes,
  formatAuditLine,
  type HitlAuditItem,
} from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';

export interface HitlAuditPanelModel {
  items: HitlAuditItem[];
  approved: number;
  rejected: number;
  other: number;
  lines: string[];
  empty: boolean;
}

export function buildHitlAuditPanel(items: HitlAuditItem[], maxLines = 12): HitlAuditPanelModel {
  const counts = countTerminalOutcomes(items);
  const lines = items.slice(0, maxLines).map(formatAuditLine);
  return {
    items,
    ...counts,
    lines,
    empty: items.length === 0,
  };
}

export function auditPanelTitle(model: HitlAuditPanelModel): string {
  if (model.empty) return '审批历史（空）';
  return `审批历史 · ✓${model.approved} ✗${model.rejected}`;
}

/** Whether to surface audit section (pending or history). */
export function shouldShowAuditSection(pending: number, historyLen: number): boolean {
  return pending > 0 || historyLen > 0;
}
