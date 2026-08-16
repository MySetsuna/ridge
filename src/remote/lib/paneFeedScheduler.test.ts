import { describe, expect, it, vi } from 'vitest';
import { MAX_PANE_FEED_FRAME_BUDGET_MS, PaneFeedScheduler } from './paneFeedScheduler';

describe('PaneFeedScheduler', () => {
  it('schedules one frame for a burst and drains it through the callback', () => {
    let run: (() => void) | undefined;
    let scheduled = 0;
    const delivered: number[] = [];
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      delivered.push(...bytes);
      return { accepted: true };
    }, {
      schedule: (callback) => {
        scheduled += 1;
        run = callback;
      },
      frameBudgetMs: MAX_PANE_FEED_FRAME_BUDGET_MS,
    });

    scheduler.enqueue('pane', new Uint8Array([1, 2]));
    scheduler.enqueue('pane', new Uint8Array([3, 4]));

    expect(scheduled).toBe(1);
    run?.();
    expect(delivered).toEqual([1, 2, 3, 4]);
    expect(scheduler.queuedBytes()).toBe(0);
  });

  it('ignores a scheduled callback after disposal', () => {
    let run: (() => void) | undefined;
    const cancel = vi.fn();
    const delivered: number[] = [];
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      delivered.push(...bytes);
      return { accepted: true };
    }, { schedule: (callback) => { run = callback; return 'frame-1'; }, cancel });

    scheduler.enqueue('pane', new Uint8Array([1]));
    scheduler.dispose();
    expect(cancel).toHaveBeenCalledWith('frame-1');
    run?.();

    expect(delivered).toEqual([]);
    expect(scheduler.queuedBytes()).toBe(0);
  });

  it('cancels the last scheduled frame when clearing its queue', () => {
    const cancel = vi.fn();
    const scheduler = new PaneFeedScheduler(() => ({ accepted: true }), {
      schedule: () => 'frame-2',
      cancel,
    });

    scheduler.enqueue('pane', new Uint8Array([1]));
    scheduler.clear('pane');

    expect(cancel).toHaveBeenCalledWith('frame-2');
    expect(scheduler.queuedBytes()).toBe(0);
  });

  it('reschedules after clearing the last queue', () => {
    const runs: Array<() => void> = [];
    let scheduled = 0;
    const delivered: number[] = [];
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      delivered.push(...bytes);
      return { accepted: true };
    }, {
      schedule: (callback) => {
        scheduled += 1;
        runs.push(callback);
        return scheduled;
      },
      cancel: vi.fn(),
    });

    scheduler.enqueue('old-pane', new Uint8Array([1]));
    scheduler.clear('old-pane');
    scheduler.enqueue('new-pane', new Uint8Array([2]));
    runs.at(-1)?.();

    expect(scheduled).toBe(2);
    expect(delivered).toEqual([2]);
  });

  it('reschedules after clearAll', () => {
    const runs: Array<() => void> = [];
    let scheduled = 0;
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      expect(bytes).toEqual(new Uint8Array([2]));
      return { accepted: true };
    }, {
      schedule: (callback) => {
        scheduled += 1;
        runs.push(callback);
        return scheduled;
      },
      cancel: vi.fn(),
    });

    scheduler.enqueue('old-pane', new Uint8Array([1]));
    scheduler.clearAll();
    scheduler.enqueue('new-pane', new Uint8Array([2]));
    runs.at(-1)?.();

    expect(scheduled).toBe(2);
  });

  it('serves the active pane first and bounds one drain slice', () => {
    const delivered: string[] = [];
    const scheduler = new PaneFeedScheduler((key, bytes) => {
      delivered.push(`${key}:${bytes.length}`);
      return { accepted: true };
    }, {
      frameBudgetMs: MAX_PANE_FEED_FRAME_BUDGET_MS,
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
    }, { frameBudgetMs: MAX_PANE_FEED_FRAME_BUDGET_MS, schedule: () => {} });
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

  it('clamps non-finite queue settings to finite defaults', () => {
    let clock = 0;
    const delivered: Uint8Array[] = [];
    const scheduler = new PaneFeedScheduler((_key, bytes) => {
      delivered.push(bytes);
      clock += 5;
      return { accepted: true };
    }, {
      maxBytesPerPane: Infinity,
      frameBudgetMs: Infinity,
      maxBytesPerFrame: Infinity,
      stepBytes: Infinity,
      now: () => clock,
      schedule: () => {},
    });

    scheduler.enqueue('pane', new Uint8Array(100_000));
    scheduler.drainNow();

    expect(delivered).toHaveLength(1);
    expect(delivered[0]).toHaveLength(32 * 1024);
    expect(scheduler.queuedBytes('pane')).toBe(100_000 - 32 * 1024);
  });
});
