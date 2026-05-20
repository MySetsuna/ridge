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

export async function waitForAppReady(timeoutMs = 30_000): Promise<void> {
  await browser.waitUntil(
    async () =>
      browser.execute(() => {
        try {
          if (location.protocol === 'about:') return false;
        } catch {
          return false;
        }
        const splash = document.getElementById('brand-loader');
        // Require the splash to have been rendered AND dismissed. The
        // dismiss path sets `display: none` after the fade transition.
        // (If splash is absent entirely we're on about:blank, not the
        // app — return false.)
        if (!splash) return false;
        if (getComputedStyle(splash).display !== 'none') return false;
        const e2e = (window as { __windE2E?: unknown }).__windE2E;
        if (!e2e) return false;
        const pane = document.querySelector('[data-rg-pane-id]') as HTMLElement | null;
        if (!pane) return false;
        return true;
      }),
    {
      timeout: timeoutMs,
      timeoutMsg:
        'app never reached pane-attached state — check that __windE2E is ' +
        'installed during pane attach and that the default workspace ' +
        'auto-creates a pane on first launch',
      interval: 250,
    },
  );
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
