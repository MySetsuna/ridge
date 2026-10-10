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
  | 'pane-first-paint'
  | 'transport-queue';

export interface RemotePerfSample {
  stage: RemotePerfStage;
  at: number;
  durationMs?: number;
  bytes?: number;
  queueBytes?: number;
  paneKey?: string;
  sourceId?: string;
  sequence?: number;
  channelId?: number;
  transport?: string;
  candidateType?: string;
  remoteCandidateType?: string;
  protocol?: string;
  relayProtocol?: string;
  ordered?: boolean;
  oldestAgeKnown?: boolean;
  maxRetransmits?: number;
  maxPacketLifeTime?: number;
  queueDepth?: number;
  oldestAgeMs?: number;
  enqueuedFrames?: number;
  dequeuedFrames?: number;
  droppedFrames?: number;
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
  droppedSamples: number;
}

export interface RemotePerfToken {
  stage: RemotePerfStage;
  startedAt: number;
  meta?: RemotePerfMeta;
}

const MAX_SAMPLES = 256;

function metricNumber(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;
}
const samples: RemotePerfSample[] = [];
let droppedSamples = 0;
let lastStatsAt = new WeakMap<object, number>();

export function remotePerfEnabled(): boolean {
  return (globalThis as unknown as { __RIDGE_REMOTE_PERF_TRACE?: boolean })
    .__RIDGE_REMOTE_PERF_TRACE === true;
}

function now(): number {
  return typeof performance !== 'undefined' ? performance.now() : Date.now();
}

function append(sample: RemotePerfSample): void {
  if (!remotePerfEnabled()) return;
  // Runtime allowlist also rejects payload/address fields passed through an unsafe cast.
  const clean: RemotePerfSample = { stage: sample.stage, at: sample.at };
  for (const key of ['durationMs', 'bytes', 'queueBytes', 'rttMs', 'availableOutgoingBitrate', 'bytesSent', 'bytesReceived', 'packetsLost', 'maxRetransmits', 'maxPacketLifeTime', 'queueDepth', 'oldestAgeMs', 'enqueuedFrames', 'dequeuedFrames', 'droppedFrames', 'sequence', 'channelId'] as const) {
    const value = metricNumber(sample[key]);
    if (value !== undefined) clean[key] = value;
  }
  for (const key of ['paneKey', 'sourceId', 'transport', 'candidateType', 'remoteCandidateType', 'protocol', 'relayProtocol'] as const) {
    if (typeof sample[key] === 'string') clean[key] = sample[key].slice(0, 96);
  }
  for (const key of ['ordered', 'oldestAgeKnown'] as const) if (typeof sample[key] === 'boolean') clean[key] = sample[key];
  samples.push(clean);
  if (samples.length > MAX_SAMPLES) { droppedSamples += samples.length - MAX_SAMPLES; samples.splice(0, samples.length - MAX_SAMPLES); }
}

export function remotePerfStart(
  stage: RemotePerfStage,
  meta?: RemotePerfMeta,
): RemotePerfToken | null {
  if (!remotePerfEnabled()) return null;
  return { stage, startedAt: now(), meta };
}

export function remotePerfEnd(token: RemotePerfToken | null, meta?: RemotePerfMeta): void {
  if (!token || !remotePerfEnabled()) return;
  const endedAt = now();
  append({
    ...token.meta,
    ...meta,
    stage: token.stage,
    at: endedAt,
    durationMs: Math.max(0, endedAt - token.startedAt),
  });
}

export function remotePerfMark(
  stage: RemotePerfStage,
  meta?: RemotePerfMeta,
): void {
  if (remotePerfEnabled()) append({ ...meta, stage, at: now() });
}

export function remotePerfSnapshot(): RemotePerfSnapshot {
  return { enabled: remotePerfEnabled(), samples: samples.map(sample => ({ ...sample })), droppedSamples };
}

export function resetRemotePerfTrace(): void {
  samples.length = 0;
  droppedSamples = 0;
  lastStatsAt = new WeakMap();
}

/** Sample WebRTC path facts without retaining the stats report or payloads. */
export async function remotePerfSamplePeerConnection(
  pc: Pick<RTCPeerConnection, 'getStats'>,
  channel?: (Pick<RTCDataChannel, 'ordered' | 'maxRetransmits' | 'maxPacketLifeTime' | 'bufferedAmount'> & Partial<Pick<RTCDataChannel, 'id'>>) | null,
): Promise<void> {
  if (!remotePerfEnabled()) return;
  const nowMs = now();
  const key = pc as object;
  const previous = lastStatsAt.get(key);
  if (previous !== undefined && nowMs - previous < 1000) return;
  lastStatsAt.set(key, nowMs);
  try {
    const report = await pc.getStats();
    let selected: Record<string, unknown> | undefined;
    let localCandidate: Record<string, unknown> | undefined;
    let remoteCandidate: Record<string, unknown> | undefined;
    let inbound: Record<string, unknown> | undefined;
    let outbound: Record<string, unknown> | undefined;
    let dataChannel: Record<string, unknown> | undefined;
    const records: Record<string, unknown>[] = [];
    report.forEach((raw) => {
      const value = raw as unknown as Record<string, unknown>;
      records.push(value);
      if (value.type === 'candidate-pair' && value.selected === true) selected = value;
      if (value.type === 'inbound-rtp' && value.kind === 'application') inbound = value;
      if (value.type === 'outbound-rtp' && value.kind === 'application') outbound = value;
    });
    const transport = records.find(value => value.type === 'transport' && typeof value.selectedCandidatePairId === 'string');
    if (transport) selected = records.find(value => value.type === 'candidate-pair' && value.id === transport.selectedCandidatePairId);
    const channels = records.filter(value => value.type === 'data-channel');
    dataChannel = channel?.id != null ? channels.find(value => value.dataChannelIdentifier === channel.id) : channels.length === 1 ? channels[0] : undefined;
    if (selected) {
      localCandidate = records.find(
        (value) => value.type === 'local-candidate' && value.id === selected?.localCandidateId,
      );
      remoteCandidate = records.find(value => value.type === 'remote-candidate' && value.id === selected?.remoteCandidateId);
    }
    append({
      stage: 'transport-stats',
      at: nowMs,
      transport: 'cloud-webrtc',
      candidateType: typeof localCandidate?.candidateType === 'string' ? localCandidate.candidateType : undefined,
      remoteCandidateType: typeof remoteCandidate?.candidateType === 'string' ? remoteCandidate.candidateType : undefined,
      protocol: typeof localCandidate?.protocol === 'string' ? localCandidate.protocol : undefined,
      relayProtocol: typeof localCandidate?.relayProtocol === 'string' ? localCandidate.relayProtocol : undefined,
      ordered: channel?.ordered,
      channelId: channel?.id ?? undefined,
      maxRetransmits: channel?.maxRetransmits ?? undefined,
      maxPacketLifeTime: channel?.maxPacketLifeTime ?? undefined,
      queueBytes: channel?.bufferedAmount,
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
