import { describe, expect, it } from 'vitest';
import { parsePhaseMessage, sleepMsForAttempt } from './hostReconnect';
import { outboundReconnectDelayMs } from '../../../packages/remote/src/shared/hosts/outboundReconnect';

describe('hostReconnect pure helpers', () => {
  it('sleepMsForAttempt matches outbound schedule', () => {
    expect(sleepMsForAttempt(0)).toBe(outboundReconnectDelayMs(0));
    expect(sleepMsForAttempt(3)).toBe(1600);
    expect(sleepMsForAttempt(4)).toBeNull();
  });

  it('parses step_host_reconnect messages with attempt', () => {
    expect(parsePhaseMessage('phase=Waiting attempt=1 cancelled=0 next_delay_ms=200')).toEqual({
      phase: 'Waiting',
      nextDelayMs: 200,
      attempt: 1,
      cancelled: false,
    });
    expect(parsePhaseMessage('phase=Idle attempt=0 cancelled=0 terminal')).toEqual({
      phase: 'Idle',
      nextDelayMs: null,
      attempt: 0,
      cancelled: false,
    });
    expect(parsePhaseMessage('phase=Cancelled attempt=2 cancelled=1 terminal')).toEqual({
      phase: 'Cancelled',
      nextDelayMs: null,
      attempt: 2,
      cancelled: true,
    });
  });
});
