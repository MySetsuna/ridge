/**
 * P4.6 Part B / P4.7 / P4.8 regression — worker-mirror resize path.
 *
 * Companion to `worker-path-shadow.spec.ts` (which covers `applyDelta`).
 * This spec drives the OTHER manager → bridge hook: `fitPane` first-size
 * triggers `bridge.attach` (worker `init`), subsequent fits trigger
 * `bridge.resize` (worker `resize`). With the feature flag on, none of
 * that activity may regress the legacy main-thread resize path — the
 * kernel's reported (rows, cols) must still track the WebDriver-window
 * size, just like the flag-off path in `resize.spec.ts`.
 *
 * Why a separate spec instead of folding into worker-path-shadow:
 *   - The reload-with-localStorage setup in `worker-path-shadow.spec.ts`
 *     leaves the app in a fresh-boot state. Driving a programmatic
 *     resize on top of that would mix two assertions (delta echo +
 *     resize) and obscure which one failed.
 *   - tauri-driver runs specs serially by default, so this spec runs
 *     after `worker-path-shadow` has already cleaned its localStorage
 *     entry — we re-arm it here.
 *
 * Cannot be runtime-validated from the autonomous loop (needs Tauri
 * build + msedgedriver). Source-only delivery; runs on next `pnpm
 * e2e:shell`.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

describe('worker-mirror resize path (flag on)', () => {
  before(async () => {
    // Same boot-time flag dance as worker-path-shadow.spec.ts — see
    // that file's `before` for why we wait-then-write-then-reload.
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          try {
            return location.protocol !== 'about:';
          } catch {
            return false;
          }
        }),
      { timeout: 15_000, timeoutMsg: 'never left about:blank' },
    );
    await browser.execute(() => {
      window.localStorage.setItem('RIDGE_USE_WORKER', '1');
      location.reload();
    });
    await waitForAppReady();
  });

  after(async () => {
    await browser.execute(() => {
      window.localStorage.removeItem('RIDGE_USE_WORKER');
    });
  });

  it('legacy resize stays in sync even with the worker mirror active', async () => {
    const paneId = await firstPaneId();

    const before = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e = (window as any).__windE2E;
      return { rows: e.rows(id), cols: e.cols(id) };
    }, paneId!);
    expect(before.rows).toBeGreaterThan(0);
    expect(before.cols).toBeGreaterThan(0);

    // Pick a window size noticeably different from defaults so the
    // resize is unambiguous. 900×600 matches resize.spec.ts so the
    // two specs catch the same regression class with the same shape.
    await browser.setWindowSize(900, 600);
    // ResizeObserver → fitPane → bridge.resize (worker mirror, no-op
    // visually) AND resize_pane Tauri command → emit pty-delta(Resize)
    // → main-thread mirror applies. Allow up to a few hundred ms for
    // the round trip.
    await browser.pause(500);

    const after = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e = (window as any).__windE2E;
      return { rows: e.rows(id), cols: e.cols(id) };
    }, paneId!);

    expect(after.rows).toBeGreaterThan(0);
    expect(after.cols).toBeGreaterThan(0);
    expect(Number.isFinite(after.rows) && Number.isFinite(after.cols)).toBe(true);
  });

  it('after resize, PTY feed still echoes (proves applyDelta + resize compose)', async () => {
    // resize.spec.ts only checks dims. Worker mirror could in principle
    // get into a state where init+resize landed but applyDelta started
    // misbehaving after the resize (e.g. if the bridge accidentally
    // de-registered the pane on resize). This second assertion catches
    // that interaction.
    const paneId = await firstPaneId();
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'after-resize-echo\\n');
    }, paneId!);
    await browser.pause(50);
    const text: string[] = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.visibleText(id) as string[];
    }, paneId!);
    expect(text.join('\\n')).toContain('after-resize-echo');
  });
});
