/**
 * CONTRACT-50 / OP-WS-LIFE+C6: foreign history attach-seed product path.
 * Pure planning for when to pull/seed/clear tails; Hosts + attach use this.
 */

export const DEFAULT_HISTORY_TAIL_CAP = 64 * 1024;
export const MIN_HISTORY_CAP = 1024;
export const MAX_HISTORY_CAP = DEFAULT_HISTORY_TAIL_CAP * 4;

export interface HistoryTailSnapshot {
  hostId: string;
  sessionId: string;
  bytes: number;
  cap: number;
  dataB64?: string;
}

export interface AttachSeedPlan {
  /** Whether attach should feed seed into local parser before live. */
  seedBeforeLive: boolean;
  /** Bytes expected (0 = skip seed). */
  seedBytes: number;
  /** Pull budget hint for host-side history RPC. */
  pullBudget: number;
  /** Clear prior tail after successful seed (avoid double-seed on re-attach). */
  clearAfterSeed: boolean;
  reason: string;
}

export interface HistorySessionKey {
  hostId: string;
  sessionId: string;
}

export function historyKey(hostId: string, sessionId: string): string {
  return `${hostId}\0${sessionId}`;
}

/** Protocol hint: lines * cols * 2, clamped. */
export function historyPullBudget(
  wantLines: number,
  cols: number,
  cap = DEFAULT_HISTORY_TAIL_CAP,
): number {
  const lines = Math.max(1, Math.min(wantLines | 0, 500));
  const c = Math.max(1, Math.min(cols | 0, 512));
  const raw = lines * c * 2;
  return Math.max(MIN_HISTORY_CAP, Math.min(cap, raw));
}

export function clampHistoryCap(cap: number): number {
  if (!Number.isFinite(cap)) return DEFAULT_HISTORY_TAIL_CAP;
  return Math.max(MIN_HISTORY_CAP, Math.min(MAX_HISTORY_CAP, Math.floor(cap)));
}

/**
 * Decide attach seed policy.
 * - No local history → optionally request host pull if rows known
 * - Has local history → seed once, clear after to prevent double feed on reattach
 * - Detach only: keep host-side tail (caller does not clear host on detach)
 */
export function planAttachSeed(opts: {
  localTailBytes: number;
  rows: number;
  cols: number;
  reattach: boolean;
  hostHistoryKnown: boolean;
}): AttachSeedPlan {
  const pullBudget = historyPullBudget(opts.rows || 24, opts.cols || 80);
  if (opts.localTailBytes > 0) {
    return {
      seedBeforeLive: true,
      seedBytes: opts.localTailBytes,
      pullBudget,
      clearAfterSeed: opts.reattach,
      reason: opts.reattach ? 'reattach_local_tail' : 'first_attach_local_tail',
    };
  }
  if (opts.hostHistoryKnown) {
    return {
      seedBeforeLive: true,
      seedBytes: 0,
      pullBudget,
      clearAfterSeed: false,
      reason: 'host_pull_then_seed',
    };
  }
  return {
    seedBeforeLive: false,
    seedBytes: 0,
    pullBudget,
    clearAfterSeed: false,
    reason: 'no_history_live_only',
  };
}

/** Detach policy: never wipe remote history; only drop local mapping. */
export function planDetachHistory(opts: {
  keepLocalTail: boolean;
}): { clearLocalTail: boolean; killRemote: boolean } {
  return {
    clearLocalTail: !opts.keepLocalTail,
    killRemote: false,
  };
}

export function summarizeHistoryBadge(snap: HistoryTailSnapshot | null): string {
  if (!snap || snap.bytes <= 0) return '';
  if (snap.bytes < 1024) return `历史 ${snap.bytes}B`;
  return `历史 ${(snap.bytes / 1024).toFixed(1)}KB`;
}

/**
 * Multi-session isolation: keys must not collide across hosts.
 */
export function assertSessionIsolation(
  entries: HistorySessionKey[],
): { ok: boolean; collisions: string[] } {
  const seen = new Map<string, HistorySessionKey>();
  const collisions: string[] = [];
  for (const e of entries) {
    const k = historyKey(e.hostId, e.sessionId);
    if (seen.has(k)) collisions.push(k.replace('\0', '/'));
    else seen.set(k, e);
  }
  return { ok: collisions.length === 0, collisions };
}

/** Merge append policy for ring buffer (mirrors Rust append_capped semantics). */
export function appendCappedPlan(
  currentLen: number,
  incomingLen: number,
  cap: number,
): { dropFromHead: number; keepIncomingFrom: number; finalLen: number } {
  const c = Math.max(1, cap);
  if (incomingLen >= c) {
    return {
      dropFromHead: currentLen,
      keepIncomingFrom: incomingLen - c,
      finalLen: c,
    };
  }
  const next = currentLen + incomingLen;
  if (next <= c) {
    return { dropFromHead: 0, keepIncomingFrom: 0, finalLen: next };
  }
  const drop = next - c;
  return { dropFromHead: drop, keepIncomingFrom: 0, finalLen: c };
}

export function formatHistoryDiag(snap: HistoryTailSnapshot): string {
  const pct = snap.cap > 0 ? Math.round((100 * snap.bytes) / snap.cap) : 0;
  return `${snap.hostId.slice(0, 8)}…/${snap.sessionId.slice(0, 8)}… ${snap.bytes}/${snap.cap}B (${pct}%)`;
}

/** Hosts panel: should show history strip for session row. */
export function showHistoryStrip(bytes: number, attached: boolean): boolean {
  return bytes > 0 && !attached;
}
