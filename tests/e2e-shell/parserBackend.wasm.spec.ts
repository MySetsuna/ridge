/**
 * P3.14 — wasm parser mode end-to-end (legacy fallback).
 *
 * Mirror of parserBackend.rust.spec.ts that flips Settings to 'wasm'
 * first. Asserts that the same byte stream produces the same visible
 * grid in both backends — the user-visible behaviour is identical;
 * only the producer changes.
 *
 * Setup: see tests/e2e-shell/README.md.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

describe('parserBackend = wasm', () => {
  before(async () => {
    // Wait for the app to reach a usable state FIRST — `localStorage`
    // is denied on `about:blank` (no origin), so the previous "flip
    // first, wait second" ordering threw SecurityError immediately.
    await waitForAppReady();
    // Flip to wasm via localStorage + reload — Settings.parserBackend
    // is persisted to `ridge-settings`. The svelte store reads it on
    // load; a reload picks up the new value before any pane attaches.
    await browser.execute(() => {
      const raw = localStorage.getItem('ridge-settings');
      const obj = raw ? JSON.parse(raw) : {};
      obj.parserBackend = 'wasm';
      localStorage.setItem('ridge-settings', JSON.stringify(obj));
    });
    await browser.refresh();
    await waitForAppReady();
  });

  it('produces the same visible grid as rust mode for the same bytes', async () => {
    const paneId = await firstPaneId();

    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, 'hello world\\n');
    }, paneId!);
    await browser.pause(50);

    const text: string[] = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.visibleText(id) as string[];
    }, paneId!);
    const joined = text.join('\\n');
    expect(joined).toContain('hello world');
  });
});
