/**
 * Multi-host outbound reconnect schedule (OP-RECONN-HOST).
 * Parity with Rust `OutboundClient::reconnect_delay_ms` + `reconnect_policy.backoff_ms`.
 */

export type OutboundReconnectDecision =
  | { action: 'wait'; delayMs: number; attempt: number }
  | { action: 'stop'; reason: string }
  | { action: 'resubscribe'; paneIds: string[] };

/** Same formula as hosts::outbound::OutboundClient::reconnect_delay_ms */
export function outboundReconnectDelayMs(attempt: number): number | null {
  if (attempt < 0 || attempt >= 4) return null;
  // base 200, max 1600, exp 2^attempt
  const base = 200;
  const max = 1600;
  const exp = base * 2 ** Math.min(attempt, 16);
  return Math.min(max, exp);
}

export function decideOutboundReconnect(opts: {
  attempt: number;
  hostReachable: boolean;
  attachedPaneIds: string[];
  intentionalClose: boolean;
}): OutboundReconnectDecision {
  if (opts.intentionalClose) {
    return { action: 'stop', reason: 'intentional_close' };
  }
  if (!opts.hostReachable) {
    const delay = outboundReconnectDelayMs(opts.attempt);
    if (delay === null) {
      return { action: 'stop', reason: 'max_attempts' };
    }
    return { action: 'wait', delayMs: delay, attempt: opts.attempt };
  }
  return { action: 'resubscribe', paneIds: [...opts.attachedPaneIds] };
}

/** Dedupe pane ids before resubscribe (no double-sub). */
export function uniquePaneIds(ids: string[]): string[] {
  return [...new Set(ids.filter(Boolean))];
}
