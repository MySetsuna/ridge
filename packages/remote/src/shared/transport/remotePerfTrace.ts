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
}

export interface RemotePerfSnapshot {
  enabled: boolean;
  samples: RemotePerfSample[];
}

export interface RemotePerfToken {
  stage: RemotePerfStage;
  startedAt: number;
  meta?: Omit<RemotePerfSample, 'stage' | 'at' | 'durationMs'>;
}

const MAX_SAMPLES = 256;
const samples: RemotePerfSample[] = [];

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
  meta?: Omit<RemotePerfSample, 'stage' | 'at' | 'durationMs'>,
): RemotePerfToken | null {
  if (!enabled()) return null;
  return { stage, startedAt: now(), meta };
}

export function remotePerfEnd(token: RemotePerfToken | null, meta?: Omit<RemotePerfSample, 'stage' | 'at' | 'durationMs'>): void {
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
  meta?: Omit<RemotePerfSample, 'stage' | 'at' | 'durationMs'>,
): void {
  append({ stage, at: now(), ...meta });
}

export function remotePerfSnapshot(): RemotePerfSnapshot {
  return { enabled: enabled(), samples: samples.slice() };
}

export function resetRemotePerfTrace(): void {
  samples.length = 0;
}

if (typeof globalThis !== 'undefined') {
  const root = globalThis as unknown as {
    __RIDGE_REMOTE_PERF?: { snapshot: typeof remotePerfSnapshot; reset: typeof resetRemotePerfTrace };
  };
  root.__RIDGE_REMOTE_PERF ??= { snapshot: remotePerfSnapshot, reset: resetRemotePerfTrace };
}
