/**
 * Live output backpressure policy (AC4-C8).
 * Pure decisions for clamp/drop; Hosts / outbound stats surface counters.
 */

export interface BackpressureSnapshot {
  bufferedBytes: number;
  capBytes: number;
  droppedBytes: number;
  highWaterMark: number;
}

export type BackpressureLevel = 'ok' | 'elevated' | 'shedding';

export function backpressureLevel(s: BackpressureSnapshot): BackpressureLevel {
  if (s.capBytes <= 0) return 'ok';
  const ratio = s.bufferedBytes / s.capBytes;
  if (s.droppedBytes > 0 || ratio >= 0.95) return 'shedding';
  if (ratio >= 0.7) return 'elevated';
  return 'ok';
}

/** How many bytes to drop from head when appending `incoming` under cap. */
export function bytesToDropOnAppend(
  currentLen: number,
  incomingLen: number,
  cap: number,
): number {
  if (cap <= 0) return incomingLen;
  if (incomingLen >= cap) return currentLen + (incomingLen - cap);
  const next = currentLen + incomingLen;
  if (next <= cap) return 0;
  return next - cap;
}

export function shouldWarnOperator(s: BackpressureSnapshot): boolean {
  return backpressureLevel(s) !== 'ok';
}

export function formatBackpressureBadge(s: BackpressureSnapshot): string {
  const lvl = backpressureLevel(s);
  if (lvl === 'ok') return '';
  if (lvl === 'elevated') return `缓冲 ${Math.round((100 * s.bufferedBytes) / s.capBytes)}%`;
  return `丢弃 ${s.droppedBytes}B`;
}

/** Map outbound stats DTO → backpressure snapshot. */
export function snapshotFromOutboundStats(st: {
  liveBufferCap?: number;
  liveBufferBytes?: number;
  liveDroppedBytes?: number;
}): BackpressureSnapshot {
  return {
    bufferedBytes: Number(st.liveBufferBytes ?? 0),
    capBytes: Number(st.liveBufferCap ?? 0),
    droppedBytes: Number(st.liveDroppedBytes ?? 0),
    highWaterMark: Number(st.liveBufferBytes ?? 0),
  };
}

/** Hosts panel: whether to show reconnect vs buffer warning. */
export function hostsLineAlert(opts: {
  backpressure: BackpressureSnapshot;
  reconnectAttempt: number;
}): string | null {
  if (opts.reconnectAttempt > 0) {
    return `重连 #${opts.reconnectAttempt}`;
  }
  if (shouldWarnOperator(opts.backpressure)) {
    return formatBackpressureBadge(opts.backpressure) || '背压';
  }
  return null;
}

/** Merge server outbound DTO drops into client snapshot (Hosts poll product path). */
export function mergeOutboundIntoSnapshot(
  prev: BackpressureSnapshot | null,
  st: {
    liveBufferCap?: number;
    liveBufferBytes?: number;
    liveDroppedBytes?: number;
  },
): BackpressureSnapshot {
  const next = snapshotFromOutboundStats(st);
  if (!prev) return next;
  return {
    bufferedBytes: next.bufferedBytes,
    capBytes: next.capBytes || prev.capBytes,
    droppedBytes: Math.max(prev.droppedBytes, next.droppedBytes),
    highWaterMark: Math.max(prev.highWaterMark, next.bufferedBytes, prev.bufferedBytes),
  };
}

/** Whether Hosts poll should accelerate due to shedding. */
export function shouldAccelerateHostsPoll(s: BackpressureSnapshot): boolean {
  return backpressureLevel(s) === 'shedding';
}

/** Clamp display percentage 0–100. */
export function bufferFillPercent(s: BackpressureSnapshot): number {
  if (s.capBytes <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((100 * s.bufferedBytes) / s.capBytes)));
}

/** Multi-host aggregate for header: total dropped across hosts. */
export function aggregateDropped(
  snaps: BackpressureSnapshot[],
): { totalDropped: number; sheddingHosts: number } {
  let totalDropped = 0;
  let sheddingHosts = 0;
  for (const s of snaps) {
    totalDropped += s.droppedBytes;
    if (backpressureLevel(s) === 'shedding') sheddingHosts++;
  }
  return { totalDropped, sheddingHosts };
}

export function formatAggregateDropBadge(agg: {
  totalDropped: number;
  sheddingHosts: number;
}): string {
  if (agg.sheddingHosts <= 0 && agg.totalDropped <= 0) return '';
  if (agg.sheddingHosts > 0) return `${agg.sheddingHosts} 主机丢包`;
  return `累计丢 ${agg.totalDropped}B`;
}
