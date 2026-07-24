/**
 * CONTRACT-56 / OP-RECONN-HOST: multi-host session isolation + reconnect task model.
 */

export type ReconnectPhaseUi =
  | 'Idle'
  | 'Waiting'
  | 'Resubscribing'
  | 'Succeeded'
  | 'Failed'
  | 'Cancelled';

export interface HostTask {
  hostId: string;
  phase: ReconnectPhaseUi;
  attempt: number;
  attachedPaneIds: string[];
  cancelled: boolean;
  lastError?: string;
}

export interface IsolationCheck {
  ok: boolean;
  issues: string[];
}

/** Ensure two hosts never share pane ids or cancel flags. */
export function checkHostTaskIsolation(tasks: HostTask[]): IsolationCheck {
  const issues: string[] = [];
  const paneOwner = new Map<string, string>();
  for (const t of tasks) {
    for (const p of t.attachedPaneIds) {
      const prev = paneOwner.get(p);
      if (prev && prev !== t.hostId) {
        issues.push(`pane ${p} claimed by ${prev} and ${t.hostId}`);
      } else {
        paneOwner.set(p, t.hostId);
      }
    }
  }
  // Cancelled host must not still be Resubscribing
  for (const t of tasks) {
    if (t.cancelled && t.phase === 'Resubscribing') {
      issues.push(`host ${t.hostId} cancelled but still Resubscribing`);
    }
  }
  return { ok: issues.length === 0, issues };
}

export function scheduleReconnectTask(
  existing: HostTask | undefined,
  hostId: string,
  attachedPaneIds: string[],
): HostTask {
  // Prior task cancelled when rescheduled
  return {
    hostId,
    phase: 'Waiting',
    attempt: 0,
    attachedPaneIds: unique(attachedPaneIds),
    cancelled: false,
    lastError: undefined,
  };
}

export function cancelHostTask(task: HostTask): HostTask {
  return {
    ...task,
    cancelled: true,
    phase: 'Cancelled',
  };
}

export function stepHostTask(
  task: HostTask,
  opts: { hostReachable: boolean; maxAttempts?: number },
): HostTask {
  if (task.cancelled || task.phase === 'Succeeded' || task.phase === 'Failed') {
    return task;
  }
  const max = opts.maxAttempts ?? 4;
  if (!opts.hostReachable) {
    const nextAttempt = task.attempt + 1;
    if (nextAttempt >= max) {
      return {
        ...task,
        attempt: nextAttempt,
        phase: 'Failed',
        lastError: 'max reconnect attempts',
      };
    }
    return { ...task, attempt: nextAttempt, phase: 'Waiting' };
  }
  // reachable → resubscribe
  return {
    ...task,
    phase: 'Succeeded',
    lastError: undefined,
  };
}

export function unique(ids: string[]): string[] {
  return [...new Set(ids.filter(Boolean))];
}

/** Intentional disconnect must stop auto-reconnect. */
export function onIntentionalDisconnect(task: HostTask | undefined): HostTask | null {
  if (!task) return null;
  return cancelHostTask(task);
}

export function reconnectBadge(task: HostTask | null | undefined): string {
  if (!task) return '';
  if (task.phase === 'Failed') return `重连失败${task.lastError ? ': ' + task.lastError : ''}`;
  if (task.phase === 'Cancelled') return '已取消重连';
  if (task.phase === 'Waiting' || task.phase === 'Resubscribing') {
    return `重连 #${task.attempt + 1}`;
  }
  if (task.phase === 'Succeeded') return '';
  return '';
}

export function canStep(task: HostTask): boolean {
  return (
    !task.cancelled &&
    (task.phase === 'Waiting' || task.phase === 'Resubscribing' || task.phase === 'Idle')
  );
}

/** Merge control-plane snapshot for Hosts header. */
export function isolationHeader(tasks: HostTask[]): string {
  const active = tasks.filter((t) => t.phase === 'Waiting' || t.phase === 'Resubscribing').length;
  const failed = tasks.filter((t) => t.phase === 'Failed').length;
  const check = checkHostTaskIsolation(tasks);
  return `重连中 ${active} · 失败 ${failed} · 隔离${check.ok ? 'OK' : '破'}`;
}
