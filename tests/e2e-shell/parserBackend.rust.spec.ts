/**
 * P3.14 — rust parser mode end-to-end (default).
 *
 * Drives a real Tauri build: opens the app, waits for the splash to
 * dismiss, finds the first pane, feeds a string through the dev-only
 * `window.__windE2E` harness, asserts that the mirror's visible grid
 * contains the expected text. The rust parser path is the default
 * (Settings.parserBackend = 'rust' on first launch), so this spec
 * doesn't toggle anything — it asserts the default works.
 *
 * Spec depends on optional dev deps not committed to package.json; see
 * `tests/e2e-shell/README.md` for setup. The `@ts-nocheck` keeps
 * svelte-check happy on machines that haven't installed WebdriverIO.
 */
// @ts-nocheck
import { browser, expect, $ } from '@wdio/globals';

describe('parserBackend = rust (default)', () => {
  before(async () => {
    // Wait for the splash to clear: app.html ships an inline splash
    // that fades out on the `ridge:app-ready` event. The DOM element
    // is `#brand-loader` and becomes `display: none` once fade-out
    // completes. 6 s ceiling is twice the splash's 3 s fallback timer.
    await browser.waitUntil(
      async () => {
        return browser.execute(() => {
          const el = document.getElementById('brand-loader');
          if (!el) return true;
          return getComputedStyle(el).display === 'none';
        });
      },
      { timeout: 6_000, timeoutMsg: 'splash never cleared' },
    );
  });

  it('feeds PTY bytes and the mirror reflects them', async () => {
    // The first pane's id lives on the [data-rg-pane-id] container.
    const paneId = await browser.execute(() => {
      const el = document.querySelector('[data-rg-pane-id]') as HTMLElement | null;
      return el?.dataset.rgPaneId ?? null;
    });
    expect(paneId).toBeTruthy();

    // Feed a short ASCII string through the dev hook. No real shell
    // is invoked — the manager pushes the bytes straight to the wasm
    // kernel (or, in rust mode, to whichever path is active).
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'hello world\\n');
    }, paneId!);

    // Give one RAF tick for the feed to reach kernel + delta path.
    await browser.pause(50);

    const text: string[] = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.visibleText(id) as string[];
    }, paneId!);

    // 'hello world' appears on the first row; subsequent rows are
    // blank (or contain the shell prompt — for a deterministic
    // assertion just check that the substring is present).
    const joined = text.join('\\n');
    expect(joined).toContain('hello world');
  });
});
