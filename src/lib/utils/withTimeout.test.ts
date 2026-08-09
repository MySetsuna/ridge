import { describe, expect, it, vi } from 'vitest';
import { withTimeout } from './withTimeout';

describe('withTimeout', () => {
  it('returns the operation result and clears the timer', async () => {
    vi.useFakeTimers();
    try {
      await expect(withTimeout(Promise.resolve('ready'), 100, 'late')).resolves.toBe('ready');
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it('rejects with the timeout error and clears the timer', async () => {
    vi.useFakeTimers();
    try {
      const pending = withTimeout(new Promise(() => {}), 100, 'kernel reattach timed out');
      const rejected = expect(pending).rejects.toThrow('kernel reattach timed out');
      await vi.advanceTimersByTimeAsync(100);
      await rejected;
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
});
