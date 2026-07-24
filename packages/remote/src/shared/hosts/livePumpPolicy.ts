/**
 * CONTRACT-54 / OP-BP-GUARD: live pump + shed policy (TS mirror of hosts inject).
 */

import {
  backpressureLevel,
  bytesToDropOnAppend,
  type BackpressureSnapshot,
} from './liveBackpressure';

export interface PumpBatch {
  hostId: string;
  sessionId: string;
  byteLength: number;
}

export interface PumpDecision {
  acceptBytes: number;
  dropBytes: number;
  level: ReturnType<typeof backpressureLevel>;
  shouldWarn: boolean;
  reason: string;
}

export interface PumpState {
  bufferedBytes: number;
  capBytes: number;
  droppedBytes: number;
  highWaterMark: number;
}

export function initialPumpState(capBytes: number): PumpState {
  return {
    bufferedBytes: 0,
    capBytes: Math.max(1, capBytes),
    droppedBytes: 0,
    highWaterMark: 0,
  };
}

/** Apply one pump batch under cap; returns new state + decision. */
export function applyPumpBatch(
  state: PumpState,
  batch: PumpBatch,
): { state: PumpState; decision: PumpDecision } {
  const dropHead = bytesToDropOnAppend(state.bufferedBytes, batch.byteLength, state.capBytes);
  let buffered = state.bufferedBytes;
  let dropped = state.droppedBytes;
  let accept = batch.byteLength;

  if (batch.byteLength >= state.capBytes) {
    // Replace buffer with tail of incoming
    dropped += buffered + (batch.byteLength - state.capBytes);
    buffered = state.capBytes;
    accept = state.capBytes;
  } else if (dropHead > 0) {
    buffered = buffered - dropHead + batch.byteLength;
    dropped += dropHead;
    accept = batch.byteLength;
  } else {
    buffered = buffered + batch.byteLength;
  }

  buffered = Math.min(buffered, state.capBytes);
  const highWaterMark = Math.max(state.highWaterMark, buffered);
  const next: PumpState = {
    bufferedBytes: buffered,
    capBytes: state.capBytes,
    droppedBytes: dropped,
    highWaterMark,
  };
  const snap: BackpressureSnapshot = {
    bufferedBytes: next.bufferedBytes,
    capBytes: next.capBytes,
    droppedBytes: next.droppedBytes,
    highWaterMark: next.highWaterMark,
  };
  const level = backpressureLevel(snap);
  return {
    state: next,
    decision: {
      acceptBytes: accept,
      dropBytes: dropHead > 0 || batch.byteLength >= state.capBytes ? Math.max(0, dropHead) : 0,
      level,
      shouldWarn: level !== 'ok',
      reason:
        level === 'shedding'
          ? 'shedding'
          : level === 'elevated'
            ? 'elevated'
            : 'ok',
    },
  };
}

/** Fair multi-session pump: prefer sessions with lowest buffer fill. */
export function orderSessionsForPump(
  sessions: { sessionId: string; bufferedBytes: number; capBytes: number }[],
): string[] {
  return [...sessions]
    .sort((a, b) => {
      const fa = a.capBytes > 0 ? a.bufferedBytes / a.capBytes : 0;
      const fb = b.capBytes > 0 ? b.bufferedBytes / b.capBytes : 0;
      return fa - fb;
    })
    .map((s) => s.sessionId);
}

export function aggregateDrops(states: PumpState[]): number {
  return states.reduce((n, s) => n + s.droppedBytes, 0);
}

export function pumpIntervalMs(level: ReturnType<typeof backpressureLevel>, base = 100): number {
  if (level === 'shedding') return Math.max(50, Math.floor(base / 2));
  if (level === 'elevated') return base;
  return base * 2;
}

export function formatPumpBadge(state: PumpState): string {
  const snap: BackpressureSnapshot = {
    bufferedBytes: state.bufferedBytes,
    capBytes: state.capBytes,
    droppedBytes: state.droppedBytes,
    highWaterMark: state.highWaterMark,
  };
  const lvl = backpressureLevel(snap);
  if (lvl === 'ok') return '';
  if (lvl === 'elevated') {
    return `泵 ${Math.round((100 * state.bufferedBytes) / state.capBytes)}%`;
  }
  return `泵丢 ${state.droppedBytes}B`;
}
