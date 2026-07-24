/**
 * CONTRACT-58 / OP-WS-PTY+LIFE: outbound session lifecycle pure state machine.
 * Complements Rust OutboundClient / MockOutboundTransport tests.
 */

export type OutboundPhase =
  | 'Idle'
  | 'Hello'
  | 'Listed'
  | 'Subscribed'
  | 'Live'
  | 'Reconnecting'
  | 'Detached'
  | 'Error';

export interface OutboundSession {
  hostId: string;
  remotePaneId: string;
  phase: OutboundPhase;
  subscribed: boolean;
  writeOk: number;
  resizeOk: number;
  fanoutBytes: number;
  lastError?: string;
}

export type LifecycleEvent =
  | { type: 'hello_ok' }
  | { type: 'list_ok'; sessions: string[] }
  | { type: 'subscribe'; paneId: string }
  | { type: 'unsubscribe'; paneId: string }
  | { type: 'write_ok' }
  | { type: 'resize_ok' }
  | { type: 'fanout'; bytes: number }
  | { type: 'disconnect'; intentional: boolean }
  | { type: 'error'; message: string }
  | { type: 'detach_view' };

export function createSession(hostId: string, remotePaneId: string): OutboundSession {
  return {
    hostId,
    remotePaneId,
    phase: 'Idle',
    subscribed: false,
    writeOk: 0,
    resizeOk: 0,
    fanoutBytes: 0,
  };
}

export function reduceLifecycle(s: OutboundSession, ev: LifecycleEvent): OutboundSession {
  switch (ev.type) {
    case 'hello_ok':
      return { ...s, phase: 'Hello', lastError: undefined };
    case 'list_ok':
      return { ...s, phase: 'Listed', lastError: undefined };
    case 'subscribe':
      if (ev.paneId !== s.remotePaneId) return s;
      return { ...s, phase: 'Subscribed', subscribed: true, lastError: undefined };
    case 'unsubscribe':
      if (ev.paneId !== s.remotePaneId) return s;
      return { ...s, phase: 'Detached', subscribed: false };
    case 'write_ok':
      return { ...s, writeOk: s.writeOk + 1, phase: s.subscribed ? 'Live' : s.phase };
    case 'resize_ok':
      return { ...s, resizeOk: s.resizeOk + 1 };
    case 'fanout':
      return {
        ...s,
        fanoutBytes: s.fanoutBytes + ev.bytes,
        phase: s.subscribed ? 'Live' : s.phase,
      };
    case 'disconnect':
      if (ev.intentional) {
        return { ...s, phase: 'Detached', subscribed: false };
      }
      return { ...s, phase: 'Reconnecting', subscribed: false };
    case 'error':
      return { ...s, phase: 'Error', lastError: ev.message, subscribed: false };
    case 'detach_view':
      // detach view ≠ kill remote; mark unsubscribed locally
      return { ...s, phase: 'Detached', subscribed: false };
    default:
      return s;
  }
}

/** No double-subscribe: if already subscribed, subscribe is idempotent. */
export function safeSubscribe(s: OutboundSession, paneId: string): OutboundSession {
  if (s.subscribed && s.remotePaneId === paneId) {
    return s;
  }
  return reduceLifecycle(s, { type: 'subscribe', paneId });
}

/** Multi-host: map hostId → sessions; operations must not cross hosts. */
export function assertNoCrossHostFanout(
  sessions: OutboundSession[],
  hostId: string,
  paneId: string,
): boolean {
  return sessions
    .filter((s) => s.remotePaneId === paneId)
    .every((s) => s.hostId === hostId);
}

export function lifecycleSummary(s: OutboundSession): string {
  return `${s.phase} · sub=${s.subscribed ? 1 : 0} · w${s.writeOk}/r${s.resizeOk} · ${s.fanoutBytes}B`;
}

/** Full happy path for tests / product docs. */
export function simulateHappyPath(hostId: string, paneId: string): OutboundSession {
  let s = createSession(hostId, paneId);
  s = reduceLifecycle(s, { type: 'hello_ok' });
  s = reduceLifecycle(s, { type: 'list_ok', sessions: [paneId] });
  s = safeSubscribe(s, paneId);
  s = reduceLifecycle(s, { type: 'write_ok' });
  s = reduceLifecycle(s, { type: 'fanout', bytes: 128 });
  return s;
}

export function simulateDetachDoesNotError(hostId: string, paneId: string): OutboundSession {
  let s = simulateHappyPath(hostId, paneId);
  s = reduceLifecycle(s, { type: 'detach_view' });
  return s;
}
