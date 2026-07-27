/**
 * Hosts control-surface pure logic (thickens C5/C6/C8 product path).
 * UI decisions for multi-host: reconnect, history, backpressure, attach.
 */
import {
  backpressureLevel,
  formatBackpressureBadge,
  hostsLineAlert,
  snapshotFromOutboundStats,
  type BackpressureSnapshot,
} from '../../../packages/remote/src/shared/hosts/liveBackpressure';
import {
  decideForeignPaneBadge,
  foreignCloseConfirmMessage,
  type ForeignHostLink,
} from '../../../packages/remote/src/shared/hosts/foreignPaneStatus';
import { sleepMsForAttempt } from '../stores/hostReconnect';

export type HostKindUi = 'headless' | 'remote' | 'rdg' | 'shared' | 'sharing';

export interface HostRowModel {
  id: string;
  kind: HostKindUi;
  label: string;
  status: ForeignHostLink;
  detail?: string;
  sessionCount: number;
  attachedCount: number;
  reconnectAttempt: number;
  reconnectPhase?: string;
  outbound?: {
    state: string;
    fanoutBytes: number;
    writeOk: number;
    liveBufferCap?: number;
    liveBufferBytes?: number;
    liveDroppedBytes?: number;
  };
  historyBytes?: number;
}

export interface HostSessionRowModel {
  hostId: string;
  sessionId: string;
  title: string;
  attached: boolean;
  canAttach: boolean;
  canDetachView: boolean;
}

export function buildHostRowAlerts(row: HostRowModel): string[] {
  const alerts: string[] = [];
  if (row.status === 'error') {
    alerts.push(row.detail || '主机错误');
  }
  if (row.status === 'disconnected' || row.status === 'connecting') {
    alerts.push(
      hostsLineAlert({
        backpressure: {
          bufferedBytes: 0,
          capBytes: 1,
          droppedBytes: 0,
          highWaterMark: 0,
        },
        reconnectAttempt: row.reconnectAttempt,
      }) || `状态 ${row.status}`,
    );
  }
  if (row.outbound) {
    const bp = snapshotFromOutboundStats(row.outbound);
    const b = formatBackpressureBadge(bp);
    if (b) alerts.push(b);
    if (row.outbound.state && row.outbound.state !== 'Subscribed' && row.outbound.state !== 'Listed') {
      alerts.push(`出站 ${row.outbound.state}`);
    }
  }
  if ((row.historyBytes ?? 0) > 0 && row.attachedCount === 0) {
    alerts.push(`历史尾 ${row.historyBytes}B 待接入`);
  }
  return alerts.filter(Boolean);
}

export function hostStatusToLink(status: string): ForeignHostLink {
  switch (status) {
    case 'connected':
      return 'connected';
    case 'connecting':
      return 'connecting';
    case 'error':
      return 'error';
    default:
      return 'disconnected';
  }
}

export function badgeForHostSession(opts: {
  hostStatus: string;
  hostLabel: string;
  attached: boolean;
  subscribed: boolean;
  reconnectAttempt: number;
  lastError?: string;
}) {
  return decideForeignPaneBadge({
    hostStatus: hostStatusToLink(opts.hostStatus),
    hostLabel: opts.hostLabel,
    attachedLocally: opts.attached,
    subscribed: opts.subscribed,
    reconnectAttempt: opts.reconnectAttempt,
    lastError: opts.lastError,
  });
}

export function confirmDetachMessage(hostLabel: string): string {
  return foreignCloseConfirmMessage(hostLabel);
}

/** Whether reconnect controls should show for a host row. */
export function showReconnectControls(row: HostRowModel): boolean {
  if (row.kind === 'headless') return false;
  return (
    row.status === 'disconnected' ||
    row.status === 'error' ||
    row.status === 'connecting' ||
    (row.reconnectAttempt > 0 && row.reconnectPhase !== 'Succeeded')
  );
}

/** Next poll delay for Hosts panel when any host is reconnecting. */
export function hostsPollIntervalMs(rows: HostRowModel[], defaultMs = 5000): number {
  const reconnecting = rows.some(
    (r) =>
      r.reconnectAttempt > 0 ||
      r.status === 'connecting' ||
      r.reconnectPhase === 'Waiting' ||
      r.reconnectPhase === 'Resubscribing',
  );
  if (!reconnecting) return defaultMs;
  // Use smallest scheduled delay among attempts (floor 500ms)
  let min = defaultMs;
  for (const r of rows) {
    const d = sleepMsForAttempt(Math.max(0, r.reconnectAttempt - 1));
    if (d != null) min = Math.min(min, Math.max(500, d));
  }
  return min;
}

export function summarizeOutbound(row: HostRowModel): string {
  if (!row.outbound) return '';
  const o = row.outbound;
  const bp = snapshotFromOutboundStats(o);
  const lvl = backpressureLevel(bp);
  return `写${o.writeOk} · 扇出${o.fanoutBytes}B · 缓冲${lvl}`;
}

export function sortHostRows(rows: HostRowModel[]): HostRowModel[] {
  const order = { error: 0, connecting: 1, disconnected: 2, connected: 3 };
  return [...rows].sort((a, b) => {
    const da = order[a.status] ?? 9;
    const db = order[b.status] ?? 9;
    if (da !== db) return da - db;
    return a.label.localeCompare(b.label);
  });
}

/** Aggregate control-plane line for Hosts header. */
export function hostsHeaderSummary(rows: HostRowModel[]): string {
  const remote = rows.filter((r) => r.kind !== 'headless');
  const connected = remote.filter((r) => r.status === 'connected').length;
  const attached = remote.reduce((n, r) => n + r.attachedCount, 0);
  const alerts = remote.flatMap(buildHostRowAlerts).length;
  return `${connected}/${remote.length} 已连 · ${attached} 视图 · ${alerts} 告警`;
}
