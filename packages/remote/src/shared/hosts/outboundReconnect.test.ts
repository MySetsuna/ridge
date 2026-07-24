import { describe, expect, it } from 'vitest';
import {
  decideOutboundReconnect,
  outboundReconnectDelayMs,
  uniquePaneIds,
} from './outboundReconnect';

describe('outboundReconnectDelayMs', () => {
  it('matches Rust schedule 200/400/800/1600 then null', () => {
    expect(outboundReconnectDelayMs(0)).toBe(200);
    expect(outboundReconnectDelayMs(1)).toBe(400);
    expect(outboundReconnectDelayMs(2)).toBe(800);
    expect(outboundReconnectDelayMs(3)).toBe(1600);
    expect(outboundReconnectDelayMs(4)).toBeNull();
    expect(outboundReconnectDelayMs(-1)).toBeNull();
  });
});

describe('decideOutboundReconnect', () => {
  it('stops on intentional close', () => {
    expect(
      decideOutboundReconnect({
        attempt: 0,
        hostReachable: false,
        attachedPaneIds: ['a'],
        intentionalClose: true,
      }),
    ).toEqual({ action: 'stop', reason: 'intentional_close' });
  });

  it('waits with backoff when host down', () => {
    expect(
      decideOutboundReconnect({
        attempt: 1,
        hostReachable: false,
        attachedPaneIds: ['a'],
        intentionalClose: false,
      }),
    ).toEqual({ action: 'wait', delayMs: 400, attempt: 1 });
  });

  it('resubscribes when host back', () => {
    expect(
      decideOutboundReconnect({
        attempt: 2,
        hostReachable: true,
        attachedPaneIds: ['a', 'b'],
        intentionalClose: false,
      }),
    ).toEqual({ action: 'resubscribe', paneIds: ['a', 'b'] });
  });

  it('stops after max attempts', () => {
    expect(
      decideOutboundReconnect({
        attempt: 4,
        hostReachable: false,
        attachedPaneIds: [],
        intentionalClose: false,
      }),
    ).toEqual({ action: 'stop', reason: 'max_attempts' });
  });
});

describe('uniquePaneIds', () => {
  it('dedupes', () => {
    expect(uniquePaneIds(['a', 'a', '', 'b'])).toEqual(['a', 'b']);
  });
});
