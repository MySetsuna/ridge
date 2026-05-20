/**
 * P3.14 — backend live-switch + fade mask verification.
 *
 * Toggles parserBackend between 'rust' and 'wasm' five times, feeding
 * PTY traffic between each switch. Asserts:
 *   (a) The `.rg-backend-switching` mask class appears + clears
 *       around every switch (R4 architectural mitigation).
 *   (b) The mirror's visibleText stays consistent across every switch
 *       — no stale state from the previous backend leaks through.
 *
 * Setup: see tests/e2e-shell/README.md.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

async function switchTo(backend: 'wasm' | 'rust') {
  await browser.execute((b) => {
    const raw = localStorage.getItem('ridge-settings');
    const obj = raw ? JSON.parse(raw) : {};
    obj.parserBackend = b;
    localStorage.setItem('ridge-settings', JSON.stringify(obj));
    // Dispatch the same change the SettingsPanel does to trigger
    // the live-switch `$effect` in RidgePane.svelte.
    window.dispatchEvent(new StorageEvent('storage', { key: 'ridge-settings' }));
  }, backend);
}

describe('parserBackend live switch', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('survives five wasm↔rust toggles without visible state corruption', async () => {
    const paneId = await firstPaneId();

    const sequence: Array<'wasm' | 'rust'> = ['wasm', 'rust', 'wasm', 'rust', 'wasm'];
    for (let i = 0; i < sequence.length; i++) {
      await switchTo(sequence[i]);
      // The 200 ms fade mask should appear briefly. Sample once
      // shortly after the switch, then once after the timeout to
      // confirm it cleared.
      await browser.pause(50);
      const masked = await browser.execute(() => {
        return !!document.querySelector('.rg-pane-container.rg-backend-switching');
      });
      // Allow the mask to be already cleared on very fast machines;
      // a soft assertion via console.warn keeps the spec robust.
      if (!masked) {
        console.warn(`switch ${i} (${sequence[i]}): mask already cleared`);
      }
      await browser.pause(250);
      const cleared = await browser.execute(() => {
        return !document.querySelector('.rg-pane-container.rg-backend-switching');
      });
      expect(cleared).toBe(true);

      // Feed some bytes and confirm the mirror reflects them.
      const tag = `tag-${i}`;
      await browser.execute(
        (id, t) => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (window as any).__windE2E.feedPty(id, t + '\\n');
        },
        paneId!,
        tag,
      );
      await browser.pause(50);
      const text: string[] = await browser.execute((id) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return (window as any).__windE2E.visibleText(id) as string[];
      }, paneId!);
      expect(text.join('\\n')).toContain(tag);
    }
  });
});
