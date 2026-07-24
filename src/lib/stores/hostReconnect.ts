/**
 * AC4-C5 product path: multi-host reconnect supervisor (desktop).
 * Invokes step_host_reconnect / cancel_host_reconnect; pure schedule helpers.
 * C56: hostSessionIsolation models multi-host pane ownership.
 */
import { writable, get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { outboundReconnectDelayMs } from '../../../packages/remote/src/shared/hosts/outboundReconnect';
import {
  cancelHostTask,
  checkHostTaskIsolation,
  reconnectBadge,
  scheduleReconnectTask,
  stepHostTask,
  type HostTask,
} from '$lib/hosts/hostSessionIsolation';

export type ReconnectPhase =
  | 'Idle'
  | 'Waiting'
  | 'Resubscribing'
  | 'Succeeded'
  | 'Failed'
  | 'Cancelled'
  | 'Unknown';

export interface HostReconnectStatus {
  hostId: string;
  phase: ReconnectPhase;
  attempt: number;
  lastMessage: string;
  nextDelayMs: number | null;
}

export const hostReconnectById = writable<Record<string, HostReconnectStatus>>({});

/** C56 product path: pure isolation tasks parallel to IPC status. */
export const hostIsolationTasks = writable<Record<string, HostTask>>({});

/** Exported for unit tests — parses step_host_reconnect response line. */
export function parsePhaseMessage(msg: string): {
  phase: ReconnectPhase;
  nextDelayMs: number | null;
  attempt: number | null;
  cancelled: boolean;
} {
  // "phase=Waiting attempt=1 cancelled=0 next_delay_ms=200" | "phase=Idle ... terminal"
  const phaseMatch = msg.match(/phase=(\w+)/);
  const delayMatch = msg.match(/next_delay_ms=(\d+)/);
  const attemptMatch = msg.match(/attempt=(\d+)/);
  const cancelled = /cancelled=1/.test(msg);
  const phase = (phaseMatch?.[1] as ReconnectPhase) || 'Unknown';
  const nextDelayMs = delayMatch ? Number(delayMatch[1]) : null;
  const attempt = attemptMatch ? Number(attemptMatch[1]) : null;
  return { phase, nextDelayMs, attempt, cancelled };
}

/** Schedule isolation task when UI starts reconnect for a host. */
export function scheduleIsolationTask(hostId: string, attachedPaneIds: string[]): HostTask {
  const prev = get(hostIsolationTasks)[hostId];
  const task = scheduleReconnectTask(prev, hostId, attachedPaneIds);
  hostIsolationTasks.update((m) => {
    const next = { ...m, [hostId]: task };
    const check = checkHostTaskIsolation(Object.values(next));
    if (!check.ok) {
      console.warn('[hostReconnect] isolation issues', check.issues);
    }
    return next;
  });
  return task;
}

export function isolationBadge(hostId: string): string {
  return reconnectBadge(get(hostIsolationTasks)[hostId]);
}

export async function stepHostReconnect(
  hostId: string,
  hostReachable: boolean,
): Promise<HostReconnectStatus> {
  if (!isTauri()) {
    return {
      hostId,
      phase: 'Idle',
      attempt: 0,
      lastMessage: 'not-tauri',
      nextDelayMs: null,
    };
  }
  // C56: step pure isolation model before/with IPC (product path).
  hostIsolationTasks.update((m) => {
    const prevTask = m[hostId] ?? scheduleReconnectTask(undefined, hostId, []);
    const stepped = stepHostTask(prevTask, { hostReachable });
    return { ...m, [hostId]: stepped };
  });
  const msg = await invoke<string>('step_host_reconnect', {
    hostId,
    hostReachable,
  });
  const parsed = parsePhaseMessage(msg);
  const prev = get(hostReconnectById)[hostId];
  // Prefer server attempt when present (uses reconnect_supervisor::attempt).
  const attempt =
    parsed.attempt != null
      ? parsed.attempt
      : phaseAttemptFallback(parsed.phase, prev?.attempt ?? 0);
  const st: HostReconnectStatus = {
    hostId,
    phase: parsed.phase,
    attempt,
    lastMessage: msg,
    nextDelayMs: parsed.nextDelayMs,
  };
  hostReconnectById.update((m) => ({ ...m, [hostId]: st }));
  return st;
}

function phaseAttemptFallback(phase: ReconnectPhase, prev: number): number {
  if (phase === 'Idle' || phase === 'Succeeded' || phase === 'Cancelled') return 0;
  if (phase === 'Waiting' || phase === 'Resubscribing') return prev + (phase === 'Waiting' ? 1 : 0);
  return prev;
}

export async function cancelHostReconnect(hostId: string): Promise<boolean> {
  if (!isTauri()) return false;
  const ok = await invoke<boolean>('cancel_host_reconnect', { hostId });
  if (ok) {
    hostIsolationTasks.update((m) => {
      const prev = m[hostId];
      if (!prev) return m;
      return { ...m, [hostId]: cancelHostTask(prev) };
    });
    hostReconnectById.update((m) => ({
      ...m,
      [hostId]: {
        hostId,
        phase: 'Cancelled',
        attempt: m[hostId]?.attempt ?? 0,
        lastMessage: 'cancelled',
        nextDelayMs: null,
      },
    }));
  }
  return ok;
}

/** Pure: compute sleep before next step from attempt index. */
export function sleepMsForAttempt(attempt: number): number | null {
  return outboundReconnectDelayMs(attempt);
}

/** Drive reconnect until terminal or max steps (for tests with mock host). */
export async function runReconnectLoop(
  hostId: string,
  opts: {
    maxSteps: number;
    isReachable: (step: number) => boolean;
    sleep?: (ms: number) => Promise<void>;
  },
): Promise<HostReconnectStatus> {
  const sleep = opts.sleep ?? (async () => {});
  let last: HostReconnectStatus = {
    hostId,
    phase: 'Idle',
    attempt: 0,
    lastMessage: '',
    nextDelayMs: null,
  };
  for (let i = 0; i < opts.maxSteps; i++) {
    last = await stepHostReconnect(hostId, opts.isReachable(i));
    if (
      last.phase === 'Succeeded' ||
      last.phase === 'Failed' ||
      last.phase === 'Cancelled'
    ) {
      return last;
    }
    if (last.nextDelayMs != null && last.nextDelayMs > 0) {
      await sleep(Math.min(last.nextDelayMs, 50)); // tests use short sleep
    }
  }
  return last;
}
