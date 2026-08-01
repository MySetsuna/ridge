import { describe, expect, it, vi } from 'vitest';
import { onceCleanup } from './listenerCleanup';

describe('onceCleanup', () => {
  it('unsubscribes every listener once, even when teardown repeats', () => {
    const first = vi.fn();
    const second = vi.fn();
    const cleanup = onceCleanup([first, () => { throw new Error('late unsubscribe'); }, second]);

    cleanup();
    cleanup();

    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
  });
});
