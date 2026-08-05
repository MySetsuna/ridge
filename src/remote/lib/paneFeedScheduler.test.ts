import { describe, expect, it, vi } from 'vitest';
import { PaneFeedScheduler } from './paneFeedScheduler';

describe('PaneFeedScheduler', () => {
  it('serves the active pane first and bounds one drain slice', () => {
    const delivered: string[] = [];
    const scheduler = new PaneFeedScheduler((key, bytes) => {
      delivered.push(`${key}:${bytes.length}`);
      return { accepted: true };
    }, {
      frameBudgetMs: Infinity,
      maxBytesPerFrame: 4,
      stepBytes: 2,
      maxBytesPerPane: 8,
      schedule: () => {},
    });
    scheduler.enqueue('background', new Uint8Array([1, 2, 3, 4]));
    scheduler.enqueue('active', new Uint8Array([5, 6, 7, 8]));
    scheduler.setActive('active');

    scheduler.drainNow();

    expect(delivered.slice(0, 2)).toEqual(['active:2', 'background:2']);
    expect(scheduler.queuedBytes('background')).toBe(2);
  });

  it('drops oldest bytes at the per-pane cap and reports resync pressure', () => {
    const dropped = vi.fn();
    const scheduler = new PaneFeedScheduler(() => ({ accepted: true }), {
      maxBytesPerPane: 3,
      onDrop: dropped,
      schedule: () => {},
    });
    scheduler.enqueue('pane', new Uint8Array([1, 2]));
    scheduler.enqueue('pane', new Uint8Array([3, 4]));

    expect(scheduler.queuedBytes('pane')).toBe(2);
    expect(dropped).toHaveBeenCalledWith('pane', 2);
  });

  it('keeps bytes queued while the pane kernel is not attached', () => {
    let attached = false;
    const delivered: number[] = [];
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      if (!attached) return { accepted: false };
      delivered.push(...bytes);
      return { accepted: true };
    }, { frameBudgetMs: Infinity, schedule: () => {} });
    scheduler.enqueue('pane', new Uint8Array([1, 2, 3]));
    scheduler.drainNow();
    expect(scheduler.queuedBytes('pane')).toBe(3);

    attached = true;
    scheduler.drainNow();
    expect(delivered).toEqual([1, 2, 3]);
    expect(scheduler.queuedBytes('pane')).toBe(0);
  });

  it('yields after the frame budget instead of draining the whole burst', () => {
    let clock = 0;
    const delivered: number[] = [];
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      delivered.push(...bytes);
      clock += 5;
      return { accepted: true };
    }, {
      now: () => clock,
      frameBudgetMs: 4,
      stepBytes: 2,
      maxBytesPerPane: 16,
      maxBytesPerFrame: 16,
      schedule: () => {},
    });
    scheduler.enqueue('pane', new Uint8Array([1, 2, 3, 4]));

    scheduler.drainNow();

    expect(delivered).toEqual([1, 2]);
    expect(scheduler.queuedBytes('pane')).toBe(2);
  });
});
