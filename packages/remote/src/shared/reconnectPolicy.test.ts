import { describe, it, expect } from 'vitest';
import { backoffMs, shouldRetry, RECONNECT_BASE_MS, RECONNECT_MAX_MS } from './reconnectPolicy';

describe('R17-RECONN reconnectPolicy (parity with Rust reconnect_policy)', () => {
  it('grows exponentially and caps', () => {
    expect(backoffMs(0, 500, 30_000)).toBe(500);
    expect(backoffMs(1, 500, 30_000)).toBe(1000);
    expect(backoffMs(2, 500, 30_000)).toBe(2000);
    expect(backoffMs(10, 500, 30_000)).toBe(30_000);
    expect(backoffMs(20, 500, 30_000)).toBe(30_000);
  });

  it('matches cloud provider constants', () => {
    expect(backoffMs(0)).toBe(RECONNECT_BASE_MS);
    expect(backoffMs(1)).toBe(2_000);
    expect(backoffMs(4)).toBe(RECONNECT_MAX_MS); // 1000*16=16000 > 15000
  });

  it('retry gate', () => {
    expect(shouldRetry(0, 5)).toBe(true);
    expect(shouldRetry(5, 5)).toBe(false);
  });
});
