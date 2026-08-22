/**
 * Regression guard for the theme-rotation cache-staleness bug
 * (`manager.ts::setTheme` updated `opts.theme` + wasm Theme struct but
 *  didn't invalidate per-pane CellInstance cache, so the next frame's
 *  `recordCachedOnly()` replayed quads with the OLD bg color baked in).
 *
 * The bug presented as:
 *   - boot: first frame rendered with `Theme::default_dark` (#071009)
 *   - bridge: pushed boundless-light → kernel Theme.bg = #ffffffff
 *   - screen: stayed near-black; cursor-blink frames flashed white then
 *     reverted (full-render vs cache-replay diverge)
 *
 * The renderer invalidation contract is covered deterministically in Rust;
 * this WebDriver guard verifies the JS bridge reaches the live wasm theme.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

const RED = '#ff0000ff';
const GREEN = '#00cc00ff';

async function rotateAndProbe(
  paneId: string,
  theme: Record<string, string>,
): Promise<{
  kernel: { bg: string; fg: string; cursor: string; tuiBg: string };
}> {
  const out = await browser.execute(
    async (paneIdArg: string, themeArg: Record<string, string>) => {
      const w = window as any;
      w.__windE2E.setTheme(themeArg);
      // Two frames let the theme invalidation and redraw complete.
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      return {
        kernel: w.__windE2E.kernelThemeProbe(paneIdArg),
      };
    },
    paneId,
    theme,
  );
  return out as {
    kernel: { bg: string; fg: string; cursor: string; tuiBg: string };
  };
}

describe('theme rotation — setTheme reaches the live wasm renderer', () => {
  let paneId: string;
  let originalTheme: Record<string, string> | null = null;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    // Snapshot boot-time theme so we can restore after — wdio sessions
    // are torn down per spec, but if specs ever share a session we
    // shouldn't leave the next one staring at a red canvas.
    originalTheme = await browser.execute(() => {
      const w = window as any;
      return w.__windE2E?.themeSnapshot?.() ?? null;
    });
    expect(originalTheme).not.toBeNull();
  });

  after(async () => {
    if (originalTheme) {
      await browser.execute((t: Record<string, string>) => {
        (window as any).__windE2E?.setTheme?.(t);
      }, originalTheme);
    }
  });

  it('first rotation: kernel Theme.bg goes red', async () => {
    const r = await rotateAndProbe(paneId, {
      background: RED,
      foreground: '#ffffffff',
      cursor: '#ffffffff',
    });
    // Kernel side — strong assertion: this is the bug the spec exists
    // to catch (`setTheme` would update `opts.theme` but the wasm
    // renderer's `Theme` struct stayed at the previous palette).
    expect(r.kernel.bg.toLowerCase()).toBe(RED);
  });

  it('second rotation: kernel follows into green (cache re-invalidates)', async () => {
    const r = await rotateAndProbe(paneId, {
      background: GREEN,
      foreground: '#ffffffff',
      cursor: '#ffffffff',
    });
    expect(r.kernel.bg.toLowerCase()).toBe(GREEN);
  });
});
