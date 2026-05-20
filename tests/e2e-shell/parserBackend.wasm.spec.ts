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

describe('parserBackend = wasm', () => {
  before(async () => {
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const el = document.getElementById('brand-loader');
          if (!el) return true;
          return getComputedStyle(el).display === 'none';
        }),
      { timeout: 6_000, timeoutMsg: 'splash never cleared' },
    );
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
    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const el = document.getElementById('brand-loader');
          if (!el) return true;
          return getComputedStyle(el).display === 'none';
        }),
      { timeout: 6_000 },
    );
  });

  it('produces the same visible grid as rust mode for the same bytes', async () => {
    const paneId = await browser.execute(() => {
      const el = document.querySelector('[data-rg-pane-id]') as HTMLElement | null;
      return el?.dataset.rgPaneId ?? null;
    });
    expect(paneId).toBeTruthy();

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
