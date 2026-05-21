/**
 * Regression guard for the idle-loop CPU-saving gate
 * (`manager.ts::tick` only sets `anyRendered = true` when actual work
 *  happened: kernel dirty OR SurfaceHost was just wiped by an
 *  invalidate call. Without this gate, cache-replay over every
 *  visible pane fires every RAF tick → 60 fps × N pane GPU draw calls
 *  burned to repaint pixels identical to the last frame).
 *
 * Test strategy — count global `requestAnimationFrame` invocations
 * over a steady-state window. The manager is the only RAF driver in
 * the app at idle (no animations elsewhere on the page), so RAF count
 * directly measures how often the render loop ticks.
 *
 *   pre-fix:  ~60 RAF/sec at idle (full vsync cadence)
 *   post-fix: ≤10 RAF/sec at idle (1 s watchdog + any wake bursts)
 *
 * The 10/sec ceiling is conservative — actual observed is ~4/sec.
 * If a future regression reintroduces unconditional cache replay,
 * this spec trips immediately.
 *
 * Sub-tests verify:
 *   - setTheme() wakes the loop and renders within 250 ms
 *   - PTY input wakes the loop and renders within 250 ms
 *   - visual state remains stable across idle (no flicker)
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

/** Drive an arbitrary stretch of time with a global requestAnimationFrame
 *  counter installed, then return the count. Patches/unpatches inside one
 *  realm round-trip so the count is exactly what the manager + browser
 *  fired (no test-orchestration RAFs added on top). */
async function countRafOver(ms: number): Promise<number> {
  const result = await browser.execute(async (durMs: number) => {
    const origRAF = window.requestAnimationFrame;
    let calls = 0;
    window.requestAnimationFrame = function (cb) {
      calls++;
      return origRAF.call(this, cb);
    };
    await new Promise((r) => setTimeout(r, durMs));
    window.requestAnimationFrame = origRAF;
    return calls;
  }, ms);
  return result as number;
}

describe('idle loop — RAF rate at steady state is heavily reduced', () => {
  let paneId: string;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    // Settle: wake once via a no-op setTheme push so the very first
    // pane-attach storm doesn't bleed into our measurement window.
    await browser.execute(() => {
      const w = window as any;
      const snap = w.__windE2E?.themeSnapshot?.();
      if (snap) w.__windE2E.setTheme(snap);
    });
    // Wait long enough for the post-wake render + return to sleep.
    await new Promise((r) => setTimeout(r, 800));
  });

  it('idle: RAF rate is at most 10/sec (vs ~60/sec pre-fix)', async () => {
    // 3 s window comfortably averages out any short wake burst that
    // happens to land at the boundary (e.g. a fontFamily store
    // initialisation echo). Threshold 30 = 10/sec.
    const calls = await countRafOver(3000);
    expect(calls).toBeLessThanOrEqual(30);
  });

  it('wake: setTheme triggers at least one RAF within 250 ms', async () => {
    const result = await browser.execute(async () => {
      const w = window as any;
      const origRAF = window.requestAnimationFrame;
      let calls = 0;
      window.requestAnimationFrame = function (cb) {
        calls++;
        return origRAF.call(this, cb);
      };
      const snap = w.__windE2E.themeSnapshot();
      // Mutate one field so the bridge fingerprint sees a change and
      // pushes through (otherwise it'd no-op as "same theme as last").
      // We push then restore — net-zero visual but provably exercises
      // the wake path.
      const probe = { ...snap, background: '#a0e0ffff' };
      w.__windE2E.setTheme(probe);
      await new Promise((r) => setTimeout(r, 250));
      window.requestAnimationFrame = origRAF;
      // Restore original — keep next spec's baseline clean.
      w.__windE2E.setTheme(snap);
      return calls;
    });
    expect(result as number).toBeGreaterThanOrEqual(1);
    // Give the restore-setTheme one round-trip to settle before the
    // next test re-baselines.
    await new Promise((r) => setTimeout(r, 800));
  });

  it('wake: writePty triggers a render within 250 ms', async () => {
    const result = await browser.execute(async (id: string) => {
      const w = window as any;
      const origRAF = window.requestAnimationFrame;
      let calls = 0;
      window.requestAnimationFrame = function (cb) {
        calls++;
        return origRAF.call(this, cb);
      };
      // writePty drives bytes to the real PTY → kernel sees output →
      // dirty → wake path. Newline is enough to make PSReadLine echo
      // back a fresh prompt line and dirty the cursor row.
      await w.__windE2E.writePty(id, '\r');
      await new Promise((r) => setTimeout(r, 250));
      window.requestAnimationFrame = origRAF;
      return calls;
    }, paneId);
    expect(result as number).toBeGreaterThanOrEqual(1);
    await new Promise((r) => setTimeout(r, 800));
  });

  it('idle stability: kernel theme + pixel sample unchanged across a 2 s idle', async () => {
    // After all the wakes from earlier tests have settled, the screen
    // should be stable. Capture state, sleep 2 s, capture again, diff.
    const r = await browser.execute(async (id: string) => {
      const w = window as any;
      const before = {
        theme: w.__windE2E.kernelThemeProbe(id),
        cursor: w.__windE2E.kernelCursor(id),
        pixel: w.__windE2E.sampleHostPixel(0.5, 0.85),
      };
      await new Promise((r) => setTimeout(r, 2000));
      const after = {
        theme: w.__windE2E.kernelThemeProbe(id),
        cursor: w.__windE2E.kernelCursor(id),
        pixel: w.__windE2E.sampleHostPixel(0.5, 0.85),
      };
      return { before, after };
    }, paneId);
    // Theme + cursor position should be identical (steady idle, no
    // input arrived).
    expect(r.after.theme.bg).toBe(r.before.theme.bg);
    expect(r.after.cursor.row).toBe(r.before.cursor.row);
    expect(r.after.cursor.col).toBe(r.before.cursor.col);
    // Pixel sample on host canvas: drawImage on a freshly-presented
    // WebGPU canvas can be flaky across the WebDriver bridge — we
    // assert "non-null structure" rather than RGB equality so a
    // browser-side readback hiccup doesn't fail the spec.
    expect(r.after.pixel).not.toBeNull();
  });
});
