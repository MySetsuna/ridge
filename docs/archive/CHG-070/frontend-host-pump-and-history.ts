
import {
  applyPumpBatch,
  formatPumpBadge,
  initialPumpState,
  type PumpState,
} from '../../../packages/remote/src/shared/hosts/livePumpPolicy';

import {
  snapshotFromOutboundStats,
  hostsLineAlert,
} from '../../../packages/remote/src/shared/hosts/liveBackpressure';

import {
  reduceLifecycle,
  safeSubscribe,
  createSession,
  type OutboundSession,
} from '../../../packages/remote/src/shared/hosts/outboundLifecycle';

import {
  planAttachSeed,
  summarizeHistoryBadge,
  type HistoryTailSnapshot,
} from '$lib/hosts/foreignHistorySession';


// ── Outbound foreign PTY (OP-WS-PTY / OP-WS-LIFE / OP-BP-GUARD) ──────────

export interface OutboundStats {
  hostId: string;
  state: string;
  subscribed: string[];
  helloOk: number;
  listOk: number;
  subscribeOk: number;
  writeOk: number;
  resizeOk: number;
  resizeSuppressed?: number;
  fanoutBytes: number;
  reconnectAttempts: number;
  resubscribeOk: number;
  errors: number;
  liveBufferCap?: number;
  liveBufferBytes?: number;
  liveDroppedBytes?: number;
}


/** Per-host reconnect attempt counters (desktop UI). */
export const outboundReconnectAttempts = writable<Record<string, number>>({});


/** C54 product path: client-side pump shed model keyed by hostId. */
export const outboundPumpByHost = writable<Record<string, PumpState>>({});


/** C58 product path: outbound lifecycle mirror for attached sessions. */
export const outboundLifecycleByKey = writable<Record<string, OutboundSession>>({});


/** C50 product path: history tail badges from get_foreign_history_tail. */
export const foreignHistoryByKey = writable<Record<string, HistoryTailSnapshot>>({});


/** Drain outbound inbound buffers into foreign parsers (Hosts poll path). */
export async function pumpHostOutput(hostId: string): Promise<number> {
  try {
    const n = await invoke<number>('pump_host_output', { hostId });
    // C54: feed livePumpPolicy after each real pump so UI badges track shed.
    if (n > 0) {
      notePumpBatch(hostId, n);
      noteLifecycleFanout(hostId, n);
    }
    return n;
  } catch {
    return 0;
  }
}

/** Apply a pump batch into client-side backpressure model (product path). */
export function notePumpBatch(hostId: string, byteLength: number, capHint = 256 * 1024): void {
  outboundPumpByHost.update((m) => {
    const prev = m[hostId] ?? initialPumpState(capHint);
    const { state } = applyPumpBatch(prev, {
      hostId,
      sessionId: '*',
      byteLength,
    });
    // Align cap with server stats when larger
    return { ...m, [hostId]: state };
  });
}

export function pumpBadgeForHost(hostId: string): string {
  const st = get(outboundPumpByHost)[hostId];
  if (!st) return '';
  return formatPumpBadge(st);
}

export function noteLifecycleFanout(hostId: string, bytes: number): void {
  outboundLifecycleByKey.update((m) => {
    const next = { ...m };
    for (const [k, sess] of Object.entries(m)) {
      if (sess.hostId !== hostId || !sess.subscribed) continue;
      next[k] = reduceLifecycle(sess, { type: 'fanout', bytes });
    }
    return next;
  });
}

export function noteLifecycleSubscribe(hostId: string, sessionId: string): void {
  const key = `${hostId}\0${sessionId}`;
  outboundLifecycleByKey.update((m) => {
    const prev = m[key] ?? createSession(hostId, sessionId);
    let s = reduceLifecycle(prev, { type: 'hello_ok' });
    s = reduceLifecycle(s, { type: 'list_ok', sessions: [sessionId] });
    s = safeSubscribe(s, sessionId);
    return { ...m, [key]: s };
  });
}

export function noteLifecycleDetach(hostId: string, sessionId: string): void {
  const key = `${hostId}\0${sessionId}`;
  outboundLifecycleByKey.update((m) => {
    const prev = m[key];
    if (!prev) return m;
    return {
      ...m,
      [key]: reduceLifecycle(prev, { type: 'detach_view' }),
    };
  });
}

export async function fetchOutboundStats(hostId: string): Promise<OutboundStats | null> {
  try {
    const st = await invoke<OutboundStats>('get_outbound_stats', { hostId });
    // Sync client pump cap/bytes from server DTO when present.
    if (st && (st.liveBufferCap || st.liveBufferBytes || st.liveDroppedBytes)) {
      outboundPumpByHost.update((m) => {
        const cap = Number(st.liveBufferCap ?? m[hostId]?.capBytes ?? 256 * 1024);
        const prev = m[hostId] ?? initialPumpState(cap);
        return {
          ...m,
          [hostId]: {
            ...prev,
            capBytes: cap > 0 ? cap : prev.capBytes,
            bufferedBytes: Number(st.liveBufferBytes ?? prev.bufferedBytes),
            droppedBytes: Number(st.liveDroppedBytes ?? prev.droppedBytes),
            highWaterMark: Math.max(
              prev.highWaterMark,
              Number(st.liveBufferBytes ?? 0),
            ),
          },
        };
      });
    }
    // Perf (iter 50): do NOT auto-fetch get_live_backpressure here — that was
    // a second IPC on every Hosts poll. Callers that need aggregate BP invoke
    // fetchLiveBackpressure explicitly (or rely on liveDroppedBytes above).
    return st;
  } catch {
    return null;
  }
}

export interface LiveBackpressureDto {
  hostId: string;
  cap: number;
  buffered: number;
  dropped: number;
  highWater: number;
  sessions: number;
  sheddingSessions: number;
  level: string;
  totalDroppedGlobal: number;
  injects: number;
}

export const liveBackpressureByHost = writable<Record<string, LiveBackpressureDto>>({});

/** C8 product path: desktop get_live_backpressure aggregate. */
export async function fetchLiveBackpressure(
  hostId: string,
): Promise<LiveBackpressureDto | null> {
  try {
    const raw = await invoke<LiveBackpressureDto>('get_live_backpressure', { hostId });
    liveBackpressureByHost.update((m) => ({ ...m, [hostId]: raw }));
    if (raw && (raw.dropped > 0 || raw.buffered > 0)) {
      outboundPumpByHost.update((m) => {
        const prev = m[hostId] ?? initialPumpState(Number(raw.cap) || 256 * 1024);
        return {
          ...m,
          [hostId]: {
            ...prev,
            capBytes: Number(raw.cap) || prev.capBytes,
            bufferedBytes: Number(raw.buffered),
            droppedBytes: Math.max(prev.droppedBytes, Number(raw.dropped)),
            highWaterMark: Math.max(prev.highWaterMark, Number(raw.highWater)),
          },
        };
      });
    }
    return raw;
  } catch {
    return null;
  }
}

/**
 * After refresh: for each Connected remote host, pump outbound live output.
 * This is the shipped UI-side read-loop companion to pump_host_output.
 */
export async function pumpAllConnectedOutbound(): Promise<number> {
  const hosts = get(hostsStore);
  const targets = hosts.filter((h) =>
    h.kind !== 'headless'
    && h.status === 'connected'
    && !registeredHostLinks.has(h.id));
  // Perf (iter 50): pump hosts in parallel — sequential IPC stacked latency.
  const parts = await Promise.all(targets.map((h) => pumpHostOutput(h.id)));
  return parts.reduce((a, b) => a + b, 0);
}

/** C50: pull foreign history tail for attach badge (desktop-only command). */
export async function fetchForeignHistoryTail(
  hostId: string,
  sessionId: string,
): Promise<HistoryTailSnapshot | null> {
  try {
    const raw = await invoke<{
      hostId: string;
      sessionId: string;
      bytes: number;
      cap: number;
      dataB64?: string;
    }>('get_foreign_history_tail', { hostId, sessionId });
    const snap: HistoryTailSnapshot = {
      hostId: raw.hostId,
      sessionId: raw.sessionId,
      bytes: raw.bytes,
      cap: raw.cap,
      dataB64: raw.dataB64,
    };
    const key = `${hostId}\0${sessionId}`;
    foreignHistoryByKey.update((m) => ({ ...m, [key]: snap }));
    return snap;
  } catch {
    return null;
  }
}

export function historyBadgeForSession(hostId: string, sessionId: string): string {
  const snap = get(foreignHistoryByKey)[`${hostId}\0${sessionId}`];
  return summarizeHistoryBadge(snap ?? null);
}

/** Plan attach seed from last known history (product decision for UI). */
export function attachSeedPlanForSession(
  hostId: string,
  sessionId: string,
  reattach: boolean,
): ReturnType<typeof planAttachSeed> {
  const snap = get(foreignHistoryByKey)[`${hostId}\0${sessionId}`];
  return planAttachSeed({
    localTailBytes: snap?.bytes ?? 0,
    rows: 24,
    cols: 80,
    reattach,
    hostHistoryKnown: (snap?.bytes ?? 0) > 0,
  });
}

/** Operator line for a host from pump + reconnect (Hosts panel). */
export function hostOperatorAlert(
  hostId: string,
  reconnectAttempt: number,
): string | null {
  const pump = get(outboundPumpByHost)[hostId];
  const bp = pump
    ? {
        bufferedBytes: pump.bufferedBytes,
        capBytes: pump.capBytes,
        droppedBytes: pump.droppedBytes,
        highWaterMark: pump.highWaterMark,
      }
    : snapshotFromOutboundStats({});
  return hostsLineAlert({ backpressure: bp, reconnectAttempt });
}

/** Record a reconnect attempt for UI badge (pure counter; schedule in outboundReconnect). */
export function noteOutboundReconnectAttempt(hostId: string): number {
  let next = 0;
  outboundReconnectAttempts.update((m) => {
    next = (m[hostId] ?? 0) + 1;
    return { ...m, [hostId]: next };
  });
  return next;
}

export function resetOutboundReconnectAttempt(hostId: string): void {
  outboundReconnectAttempts.update((m) => {
    const copy = { ...m };
    delete copy[hostId];
    return copy;
  });
}

