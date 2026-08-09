import { beforeEach, describe, expect, it, vi } from 'vitest';
import { cancelThrottledResize, throttledUpdateResize } from './resizeThrottle';

describe('resize throttle', () => {
  let callbacks: FrameRequestCallback[];
  let cancel: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    callbacks = [];
    cancel = vi.fn();
    Object.assign(globalThis, {
      requestAnimationFrame: vi.fn((callback: FrameRequestCallback) => {
        callbacks.push(callback);
        return callbacks.length;
      }),
      cancelAnimationFrame: cancel,
    });
    cancelThrottledResize();
  });

  it('coalesces pointer updates into the newest frame value', () => {
    const callback = vi.fn();
    throttledUpdateResize({ x: 1, y: 2 }, callback);
    throttledUpdateResize({ x: 3, y: 4 }, callback);
    expect(callback).not.toHaveBeenCalled();
    callbacks[0](0);
    expect(callback).toHaveBeenCalledOnce();
    expect(callback).toHaveBeenCalledWith({ x: 3, y: 4 });
  });

  it('cancels a pending frame and drops its pointer', () => {
    const callback = vi.fn();
    throttledUpdateResize({ x: 1, y: 2 }, callback);
    cancelThrottledResize();
    callbacks[0](0);
    expect(cancel).toHaveBeenCalledWith(1);
    expect(callback).not.toHaveBeenCalled();
  });
});
