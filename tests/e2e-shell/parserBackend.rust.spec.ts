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
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId, clearVisibleGrid, waitForVisibleText } from './helpers';

describe('parserBackend = rust (default)', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('feeds PTY bytes and the mirror reflects them', async () => {
    const paneId = await firstPaneId();

    // §1.35 (2026-05-24): clear screen first so the async shell prompt
    // doesn't clobber the feed (see clearVisibleGrid docs).
    await clearVisibleGrid(paneId);

    // Feed a short ASCII string through the dev hook. No real shell
    // is invoked — the manager pushes the bytes straight to the wasm
    // kernel (or, in rust mode, to whichever path is active).
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'hello world\\n');
    }, paneId!);

    // Poll until the bytes surface — replaces the bare 50 ms pause
    // that raced the wasm + delta-frame pipeline on cold starts.
    await waitForVisibleText(paneId, 'hello world');

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
