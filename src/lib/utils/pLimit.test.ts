import { describe, it, expect } from 'vitest';
import { mapLimit, recommendedGitConcurrency } from './pLimit';

const tick = () => new Promise((r) => setTimeout(r, 0));

describe('mapLimit', () => {
  it('returns results in input order regardless of resolve order', async () => {
    // Arrange: later items resolve sooner, so completion order ≠ input order.
    const items = [30, 20, 10, 0];
    // Act
    const out = await mapLimit(items, 4, async (ms) => {
      await new Promise((r) => setTimeout(r, ms));
      return ms;
    });
    // Assert: order matches the input array, not resolution timing.
    expect(out).toEqual([30, 20, 10, 0]);
  });

  it('never runs more than `limit` workers concurrently', async () => {
    // Arrange
    let active = 0;
    let peak = 0;
    const items = Array.from({ length: 20 }, (_, i) => i);
    // Act
    await mapLimit(items, 3, async () => {
      active += 1;
      peak = Math.max(peak, active);
      await tick();
      active -= 1;
    });
    // Assert
    expect(peak).toBeLessThanOrEqual(3);
  });

  it('stops launching new workers once the signal aborts', async () => {
    // Arrange: a controller we trip after the first wave of workers starts.
    const controller = new AbortController();
    const started: number[] = [];
    const items = Array.from({ length: 50 }, (_, i) => i);

    // Act: abort on the very first worker so the remaining 47+ never launch.
    await mapLimit(
      items,
      2,
      async (i) => {
        started.push(i);
        if (i === 0) controller.abort();
        await tick();
      },
      { signal: controller.signal }
    );

    // Assert: only the in-flight wave (≤ limit) ran; the backlog was skipped.
    expect(started.length).toBeLessThanOrEqual(2);
    expect(started.length).toBeGreaterThanOrEqual(1);
  });

  it('returns an empty array immediately for a pre-aborted signal', async () => {
    // Arrange
    const controller = new AbortController();
    controller.abort();
    let ran = false;
    // Act
    const out = await mapLimit(
      [1, 2, 3],
      2,
      async () => {
        ran = true;
      },
      { signal: controller.signal }
    );
    // Assert: no worker ever runs.
    expect(ran).toBe(false);
    expect(out).toEqual([]);
  });

  it('handles an empty input list', async () => {
    expect(await mapLimit([], 4, async () => 1)).toEqual([]);
  });
});

describe('recommendedGitConcurrency', () => {
  it('returns a sane bounded integer', () => {
    const n = recommendedGitConcurrency();
    expect(Number.isInteger(n)).toBe(true);
    expect(n).toBeGreaterThanOrEqual(2);
    expect(n).toBeLessThanOrEqual(12);
  });
});
