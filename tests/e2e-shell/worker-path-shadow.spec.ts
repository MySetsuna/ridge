/**
 * P4.6 Part B / P4.7 / P4.8 regression — worker-mirror shadow path.
 *
 * Asserts the invariant that Iter 7-10's accumulated worker
 * scaffolding does NOT regress the legacy main-thread render path:
 *
 *   With `localStorage.RIDGE_USE_WORKER = '1'` set BEFORE the app boots,
 *   `manager.attach → fitPane` invokes `workerRendererBridge.attach`
 *   which lazily spins up the shared render worker, loads the wasm
 *   kernel inside it, and starts mirroring `applyDelta`/`resize`/`destroy`
 *   to the worker on every frame. The worker still does NOT paint
 *   (`createRenderer` is wired only after Iter 11's Rust OffscreenCanvas
 *   work lands). So the visible pane MUST continue to render exactly
 *   like the flag-off path. This spec proves it:
 *
 *     1. Flag on → app boots → first pane mounts.
 *     2. Feed bytes through `__windE2E.feedPty` (same as
 *        parserBackend.rust.spec.ts).
 *     3. `__windE2E.visibleText` reflects the bytes — proving the
 *        main-thread kernel still consumed them despite the parallel
 *        mirror.
 *
 * If this spec fails after a future iter touches the worker plumbing,
 * the worker mirror is no longer additive — it's pre-empting the
 * main-thread path somehow. That is a regression.
 *
 * The localStorage write happens via `browser.execute` AFTER WebDriver
 * navigates but BEFORE `waitForAppReady` resolves; the manager reads
 * the flag on first attach via `workerRendererBridge.isActive() →
 * isWorkerRenderingEnabled()` which inspects localStorage when the
 * `__RIDGE_USE_WORKER` global isn't set. See
 * `src/lib/terminal/workerRendererSingleton.ts::isWorkerRenderingEnabled`.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId, clearVisibleGrid, waitForVisibleText } from './helpers';

describe('worker-mirror shadow path (flag on, worker can\'t paint yet)', () => {
  before(async () => {
    // Set the opt-in BEFORE the SvelteKit hydration races the first
    // `attach()`. Tauri-driver gives us the window before SvelteKit
    // navigates — see helpers.ts for the about:blank caveat. Once we're
    // on the real origin, localStorage is writable. Write then reload
    // to guarantee the first attach reads the new flag.
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
      // Reload from the same origin so the next boot's `manager.attach`
      // sees the flag during its very first `fitPane` call. Without
      // this, the singleton creation would lag the first attach by
      // one viewport-change tick and the worker would never get an
      // `init` for the pane.
      location.reload();
    });
    await waitForAppReady();
  });

  after(async () => {
    // Leave a clean slate for the next spec. The other shell-specs
    // assume `RIDGE_USE_WORKER` is unset.
    await browser.execute(() => {
      window.localStorage.removeItem('RIDGE_USE_WORKER');
    });
  });

  it('legacy main-thread render still echoes PTY bytes', async () => {
    const paneId = await firstPaneId();

    // §1.35: clear screen + home cursor BEFORE the feed so the async
    // shell prompt doesn't clobber the start of "hello from worker
    // mirror" (see clearVisibleGrid docs).
    await clearVisibleGrid(paneId);
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'hello from worker mirror\\n');
    }, paneId!);
    await waitForVisibleText(paneId, 'hello from worker mirror');

    const text: string[] = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.visibleText(id) as string[];
    }, paneId!);
    expect(text.join('\\n')).toContain('hello from worker mirror');
  });

  it('worker singleton actually spun up (proves mirror was exercised, not just flag set)', async () => {
    // Iter 17 (2026-05-22) exposes `__windE2E.workerBridge()` which
    // returns the bridge's `{ active, pending }` at call time. `active`
    // is true iff `getWorkerRenderer()` returned a non-null singleton
    // when last polled — strong evidence that the Worker constructor
    // ran, the module URL resolved, and the wasm load was initiated.
    // Before Iter 17, this spec could only check the localStorage
    // precondition — a setup-only proof. Now we can assert the
    // postcondition: the worker is actually live.
    const probe = await browser.execute(() => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.workerBridge() as {
        active: boolean;
        pending: number;
      };
    });
    expect(probe.active).toBe(true);
    // `pending` is the in-flight request count. After the first
    // `fitPane` fired `bridge.attach` and the worker ack'd, this
    // should settle to 0. We don't pin a number — just that it's
    // non-negative finite. (A non-zero stable value here would
    // indicate the worker never acked, which is a different bug.)
    expect(Number.isFinite(probe.pending) && probe.pending >= 0).toBe(true);
  });
});
