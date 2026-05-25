/**
 * Shared bring-up helpers for the tauri-driver shell e2e suite.
 *
 * Why this exists: tauri-driver attaches a WebDriver session as soon as
 * WebView2 hands one out, which is BEFORE the bundled SvelteKit app has
 * navigated. At that moment `document` exists but is effectively
 * `about:blank` — no origin, no DOM. The previous per-spec splash check
 * (`if (!brandLoader) return true`) returned true on that empty
 * document, then the spec proceeded and either:
 *   - hit `Received: null` from `document.querySelector('[data-rg-pane-id]')`
 *   - hit `SecurityError: Failed to read 'localStorage'` (about:blank
 *     has no origin → localStorage is denied)
 *
 * `waitForAppReady` only returns when ALL of the following are true:
 *   1. The page is past about:blank (location.protocol !== 'about:').
 *   2. `#brand-loader` was rendered AND then hidden (positive evidence
 *      that SvelteKit hydrated + dispatched `ridge:app-ready`).
 *   3. `window.__windE2E` exists (manager.ts installs this inside
 *      `attach()`, so its presence is the strongest possible signal
 *      that a pane has actually mounted).
 *   4. A `[data-rg-pane-id]` element exists (the pane DOM is in the
 *      tree, which is the prerequisite for every other selector the
 *      specs run).
 *
 * 30 s ceiling covers Tauri's worst-case cold start (release build +
 * theme bootstrap + PTY spawn + WASM module fetch). On a warm box the
 * real wait is ~2 s.
 */
// @ts-nocheck
import { browser } from '@wdio/globals';

export async function waitForAppReady(timeoutMs = 60_000): Promise<void> {
  // Capture WHICH predicate stalled so the thrown error points at the
  // actual stuck stage (origin not navigated / splash not hidden / no
  // __windE2E / no pane DOM) instead of the catch-all "never reached"
  // text. `timeoutMsg` is captured at waitUntil-call time, so it can't
  // see the running closure's `lastFailReason` — we re-throw manually
  // with the dynamic message in the catch.
  let lastFailReason = 'pre-flight (no checks ran yet)';
  try {
    await browser.waitUntil(
      async () => {
        const reason = (await browser.execute(() => {
          try {
            if (location.protocol === 'about:') return 'still on about:blank';
          } catch {
            return 'location.protocol read threw';
          }
          const splash = document.getElementById('brand-loader');
          if (!splash) return 'no #brand-loader (page may not have hydrated)';
          if (getComputedStyle(splash).display !== 'none') {
            return 'splash still visible (SvelteKit hydration in progress)';
          }
          const e2e = (window as { __windE2E?: unknown }).__windE2E;
          if (!e2e) return 'window.__windE2E not installed (manager.attach not run)';
          const pane = document.querySelector('[data-rg-pane-id]') as HTMLElement | null;
          if (!pane) return 'no [data-rg-pane-id] in DOM (no pane mounted yet)';
          return '__ready__';
        })) as string;
        if (reason === '__ready__') return true;
        lastFailReason = reason;
        return false;
      },
      { timeout: timeoutMs, interval: 250 },
    );
  } catch (e) {
    const baseMsg = e instanceof Error ? e.message : String(e);
    throw new Error(
      `app never reached pane-attached state — last stall: ${lastFailReason}` +
        ` (waitUntil: ${baseMsg})`,
    );
  }
}

/** Convenience: read the first pane id from the DOM. Returns the
 *  string id or throws — call AFTER `waitForAppReady` so it never
 *  returns null. */
export async function firstPaneId(): Promise<string> {
  const id = await browser.execute(() => {
    const el = document.querySelector('[data-rg-pane-id]') as HTMLElement | null;
    return el?.dataset.rgPaneId ?? null;
  });
  if (!id) throw new Error('no [data-rg-pane-id] in DOM — call waitForAppReady first');
  return id;
}

/** Wipe the visible grid + home the cursor by feeding CSI 2J + CSI H
 *  directly to the kernel. Use this BEFORE any spec that calls
 *  `feedPty` and then asserts on `visibleText` / `setSelectionAbs`.
 *
 *  Why this exists: `waitForAppReady` only proves the pane mounted —
 *  the PowerShell prompt arrives async via the real PTY a few ms
 *  later. If your `feedPty('hello\\n')` lands BEFORE the prompt, your
 *  content is at row 0; if AFTER, the prompt clobbered row 0 and your
 *  content is on a different row (or partially overwritten). The race
 *  is unpredictable across cold / warm runs. CSI 2J wipes the grid,
 *  CSI H homes the cursor — neither scrolls anything into scrollback,
 *  so `scrollbackLen()` stays stable and visible row 0 is guaranteed
 *  empty for the next feedPty. Specs that previously inline-fixed
 *  this: selection-tui-refresh, worker-path-shadow, parserBackend.rust. */
export async function clearVisibleGrid(paneId: string): Promise<void> {
  await browser.execute((id: string) => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (window as any).__windE2E.feedPty(id, '\x1b[2J\x1b[H');
  }, paneId);
}

/** Poll `__windE2E.visibleText(paneId)` until any row contains
 *  `needle`, or fail loudly. Replaces the brittle `browser.pause(N)`
 *  pattern that races the wasm feed pipeline on cold ridge.exe
 *  starts. 4 s is well over the worst-case observed latency on this
 *  harness (~150 ms on a warm box, ~700 ms cold). */
export async function waitForVisibleText(
  paneId: string,
  needle: string,
  timeoutMs = 4_000,
): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const found = (await browser.execute((id: string, n: string) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const rows = (window as any).__windE2E.visibleText(id) as string[];
      return rows.some((r) => r.includes(n));
    }, paneId, needle)) as boolean;
    if (found) return;
    await browser.pause(60);
  }
  throw new Error(`waitForVisibleText: "${needle}" never appeared in ${timeoutMs}ms`);
}
