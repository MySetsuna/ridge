/**
 * Bounded Remote interaction trace. Disabled by default; enable with
 * `globalThis.__RIDGE_REMOTE_PERF_TRACE = true` before reproducing a stall.
 * Samples are timestamped so transport, queue, feed, and paint stages can be
 * compared without retaining payloads or creating a second connection.
 */

export type RemotePerfStage =
  | 'input-rpc'
  | 'resize-rpc'
  | 'transport-send'
  | 'transport-stats'
  | 'raw-receive'
  | 'raw-feed'
  | 'pane-switch'
  | 'pane-first-paint';

export interface RemotePerfSample {
  stage: RemotePerfStage;
  at: number;
  durationMs?: number;
  bytes?: number;
  queueBytes?: number;
  paneKey?: string;
  transport?: string;
  candidateType?: string;
  rttMs?: number;
  availableOutgoingBitrate?: number;
  bytesSent?: number;
  bytesReceived?: number;
  packetsLost?: number;
}

type RemotePerfMeta = Omit<RemotePerfSample, 'stage' | 'at' | 'durationMs'>;

export interface RemotePerfSnapshot {
  enabled: boolean;
  samples: RemotePerfSample[];
}

export interface RemotePerfToken {
  stage: RemotePerfStage;
  startedAt: number;
  meta?: RemotePerfMeta;
}

const MAX_SAMPLES = 256;

function metricNumber(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined;
}
const samples: RemotePerfSample[] = [];
const lastStatsAt = new WeakMap<object, number>();

function enabled(): boolean {
  return (globalThis as unknown as { __RIDGE_REMOTE_PERF_TRACE?: boolean })
    .__RIDGE_REMOTE_PERF_TRACE === true;
}

function now(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now();
}

function append(sample: RemotePerfSample): void {
  if (!enabled()) return;
  samples.push(sample);
  if (samples.length > MAX_SAMPLES) samples.splice(0, samples.length - MAX_SAMPLES);
}

export function remotePerfStart(
  stage: RemotePerfStage,
  meta?: RemotePerfMeta,
): RemotePerfToken | null {
  if (!enabled()) return null;
  return { stage, startedAt: now(), meta };
}

export function remotePerfEnd(token: RemotePerfToken | null, meta?: RemotePerfMeta): void {
  if (!token || !enabled()) return;
  append({
    stage: token.stage,
    at: now(),
    durationMs: Math.max(0, now() - token.startedAt),
    ...token.meta,
    ...meta,
  });
}

export function remotePerfMark(
  stage: RemotePerfStage,
  meta?: RemotePerfMeta,
): void {
  append({ stage, at: now(), ...meta });
}

export function remotePerfSnapshot(): RemotePerfSnapshot {
  return { enabled: enabled(), samples: samples.slice() };
}

export function resetRemotePerfTrace(): void {
  samples.length = 0;
}

/** Sample WebRTC path facts without retaining the stats report or payloads. */
export async function remotePerfSamplePeerConnection(
  pc: Pick<RTCPeerConnection, 'getStats'>,
): Promise<void> {
  if (!enabled()) return;
  const nowMs = now();
  const key = pc as object;
  const previous = lastStatsAt.get(key);
  if (previous !== undefined && nowMs - previous < 1000) return;
  lastStatsAt.set(key, nowMs);
  try {
    const report = await pc.getStats();
    let selected: Record<string, unknown> | undefined;
    let localCandidate: Record<string, unknown> | undefined;
    let inbound: Record<string, unknown> | undefined;
    let outbound: Record<string, unknown> | undefined;
    let dataChannel: Record<string, unknown> | undefined;
    const records: Record<string, unknown>[] = [];
    report.forEach((raw) => {
      const value = raw as unknown as Record<string, unknown>;
      records.push(value);
      if (value.type === 'candidate-pair' && (value.selected || value.nominated)) selected = value;
      if (value.type === 'inbound-rtp' && value.kind === 'application') inbound = value;
      if (value.type === 'outbound-rtp' && value.kind === 'application') outbound = value;
      if (value.type === 'data-channel') dataChannel = value;
    });
    if (selected) {
      localCandidate = records.find(
        (value) => value.type === 'local-candidate' && value.id === selected?.localCandidateId,
      );
    }
    append({
      stage: 'transport-stats',
      at: nowMs,
      transport: 'cloud-webrtc',
      candidateType: typeof localCandidate?.candidateType === 'string' ? localCandidate.candidateType : undefined,
      rttMs: typeof selected?.currentRoundTripTime === 'number' ? selected.currentRoundTripTime * 1000 : undefined,
      availableOutgoingBitrate:
        typeof selected?.availableOutgoingBitrate === 'number' ? selected.availableOutgoingBitrate : undefined,
      bytesSent: metricNumber(outbound?.bytesSent) ?? metricNumber(dataChannel?.bytesSent),
      bytesReceived: metricNumber(inbound?.bytesReceived) ?? metricNumber(dataChannel?.bytesReceived),
      packetsLost: typeof inbound?.packetsLost === 'number' ? inbound.packetsLost : undefined,
    });
  } catch {
    // getStats is optional on older WebViews; tracing must never affect input.
  }
}

if (typeof globalThis !== 'undefined') {
  const root = globalThis as unknown as {
    __RIDGE_REMOTE_PERF?: { snapshot: typeof remotePerfSnapshot; reset: typeof resetRemotePerfTrace };
  };
  root.__RIDGE_REMOTE_PERF ??= { snapshot: remotePerfSnapshot, reset: resetRemotePerfTrace };
}
