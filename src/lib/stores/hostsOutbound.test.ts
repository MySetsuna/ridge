import { describe, expect, it, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import {
  noteOutboundReconnectAttempt,
  outboundReconnectAttempts,
  resetOutboundReconnectAttempt,
} from './hosts';
import {
  decideOutboundReconnect,
  outboundReconnectDelayMs,
} from '../../../packages/remote/src/shared/hosts/outboundReconnect';

describe('hosts outbound reconnect counters', () => {
  beforeEach(() => {
    outboundReconnectAttempts.set({});
  });

  it('increments and resets per host', () => {
    expect(noteOutboundReconnectAttempt('lan:a')).toBe(1);
    expect(noteOutboundReconnectAttempt('lan:a')).toBe(2);
    expect(noteOutboundReconnectAttempt('lan:b')).toBe(1);
    expect(get(outboundReconnectAttempts)).toEqual({ 'lan:a': 2, 'lan:b': 1 });
    resetOutboundReconnectAttempt('lan:a');
    expect(get(outboundReconnectAttempts)['lan:a']).toBeUndefined();
    expect(get(outboundReconnectAttempts)['lan:b']).toBe(1);
  });

  it('pairs with shared delay schedule', () => {
    const attempt = noteOutboundReconnectAttempt('h');
    const delay = outboundReconnectDelayMs(attempt - 1);
    expect(delay).toBe(200);
    const dec = decideOutboundReconnect({
      attempt: attempt - 1,
      hostReachable: false,
      attachedPaneIds: ['p'],
      intentionalClose: false,
    });
    expect(dec.action).toBe('wait');
  });
});
