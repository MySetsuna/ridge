/**
 * Regression guard for keyboard → PTY → echo → cursor-advance.
 *
 * Symptom this locks down: when a user types into the focused pane, the
 * kernel's reported cursor should advance one cell per ASCII char, and
 * the visible grid should contain the typed sequence in order. Mismatch
 * between cursor position and grid content is the "input flickering /
 * misaligned" bug — typically a delta-frame regression in P3.x rust
 * parser mode that left cursor and cells out of sync.
 *
 * We drive bytes through the same `write_to_pty` invoke the key encoder
 * uses (not `feedPty`, which short-circuits the producer pipeline), so
 * the spec covers the WHOLE round trip — Rust producer, Channel/event
 * delivery, wasm consumer, and rendering. Sampling the kernel cursor +
 * the visible grid after the echo arrives proves end-to-end consistency.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

/** Wait until `pred()` returns truthy within `timeoutMs`. Polls every
 *  `intervalMs`. Returns the truthy value or throws. Used to give the
 *  PTY round-trip enough time without hammering the kernel. */
async function waitFor<T>(
  pred: () => Promise<T | null | false | 0 | ''>,
  timeoutMs = 4_000,
  intervalMs = 80,
): Promise<T> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const v = await pred();
    if (v) return v as T;
    await browser.pause(intervalMs);
  }
  throw new Error(`waitFor timed out after ${timeoutMs}ms`);
}

describe('input echo — typed bytes reach the kernel and advance the cursor', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('writePty("abc") advances the kernel cursor by ≥ 3 cells', async () => {
    const paneId = await firstPaneId();

    const before = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.kernelCursor(id) as { row: number; col: number } | null;
    }, paneId);
    expect(before).not.toBeNull();

    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      void (window as any).__windE2E.writePty(id, 'abc');
    }, paneId);

    // Poll until cursor advances OR timeout. Shell echo latency on a
    // cold ConPTY is ~80-300ms; pre-spawned shell ~30ms. 4s ceiling
    // covers the cold path.
    const after = await waitFor(async () => {
      const cur = await browser.execute((id) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return (window as any).__windE2E.kernelCursor(id) as { row: number; col: number } | null;
      }, paneId);
      if (!cur) return null;
      // Same row → col must advance by ≥ 3. New row → cursor wrapped
      // (e.g. prompt re-emitted on the next line); accept that too.
      if (cur.row === before!.row && cur.col - before!.col >= 3) return cur;
      if (cur.row > before!.row) return cur;
      return null;
    });

    expect(after).toBeTruthy();
  });

  it('typed text "echo-probe-xyz" appears in the visible grid', async () => {
    const paneId = await firstPaneId();
    // Use a deterministic, unlikely-to-collide string so a noisy
    // prompt doesn't false-positive the substring search.
    const probe = `echo-probe-${Math.random().toString(36).slice(2, 8)}`;

    await browser.execute((id, text) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      void (window as any).__windE2E.writePty(id, text);
    }, paneId, probe);

    const found = await waitFor(async () => {
      const rows = await browser.execute((id) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return (window as any).__windE2E.visibleText(id) as string[];
      }, paneId);
      const joined = rows.join('\n');
      return joined.includes(probe) ? joined : null;
    });

    expect(found).toContain(probe);
  });
});
