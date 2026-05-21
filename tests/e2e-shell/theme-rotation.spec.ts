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
 * Both ends agreed on the *Theme struct* (`kernelThemeProbe` returned
 * the new color) but the GPU output didn't match, because the cache
 * replay path bypasses `Backend::clear()` which is where `theme.bg`
 * gets re-sampled into the bg quad.
 *
 * Implementation note — pixel readback timing:
 *   `drawImage(<webgpu canvas>)` only captures the most recently
 *   PRESENTED texture, and only while the swap chain still owns it.
 *   Crossing a mocha `it` boundary (which round-trips through WebDriver
 *   BiDi) lets the swap chain hand the texture back, after which
 *   drawImage paints nothing and getImageData returns (0,0,0,0).
 *   So each setTheme + assertion must happen inside ONE
 *   `browser.execute` block — `awaitPromise:true` keeps the call
 *   alive until our `requestAnimationFrame`s have completed.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

const RED = '#ff0000ff';
const GREEN = '#00cc00ff';

/** Drive a setTheme + return {kernel, pixel} from the same realm round-trip. */
async function rotateAndProbe(
  paneId: string,
  theme: Record<string, string>,
): Promise<{
  kernel: { bg: string; fg: string; cursor: string; tuiBg: string };
  pixel: { r: number; g: number; b: number; a: number };
}> {
  const out = await browser.execute(
    async (paneIdArg: string, themeArg: Record<string, string>) => {
      const w = window as any;
      w.__windE2E.setTheme(themeArg);
      // 2 RAFs: one to consume the wake → frame encode, one to let
      // the just-presented texture stabilise so drawImage captures it.
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      return {
        kernel: w.__windE2E.kernelThemeProbe(paneIdArg),
        pixel: w.__windE2E.sampleHostPixel(0.5, 0.85),
      };
    },
    paneId,
    theme,
  );
  return out as {
    kernel: { bg: string; fg: string; cursor: string; tuiBg: string };
    pixel: { r: number; g: number; b: number; a: number };
  };
}

describe('theme rotation — setTheme reaches the GPU output, not just the kernel', () => {
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

  it('first rotation: kernel Theme.bg AND host canvas pixel both go red', async () => {
    const r = await rotateAndProbe(paneId, {
      background: RED,
      foreground: '#ffffffff',
      cursor: '#ffffffff',
    });
    // Kernel side
    expect(r.kernel.bg.toLowerCase()).toBe(RED);
    // GPU output: sample at (0.5, 0.85) — bottom-mid of canvas, below
    // the PS prompt's first row, so we hit empty bg quads rather than
    // a glyph stroke. The cache-staleness regression would paint the
    // previous theme's bg here (cached CellInstance bg_rgba) so a
    // strict R-dominant check is enough to catch it.
    expect(r.pixel.r).toBeGreaterThan(180);
    expect(r.pixel.g).toBeLessThan(80);
    expect(r.pixel.b).toBeLessThan(80);
    expect(r.pixel.a).toBeGreaterThan(200);
  });

  it('second rotation: kernel + canvas BOTH follow into green (cache re-invalidates)', async () => {
    // Catches a partial fix where only the first setTheme invalidates.
    // After the previous it()'s red rotation, this call must invalidate
    // again so the new green bg replaces red on screen.
    const r = await rotateAndProbe(paneId, {
      background: GREEN,
      foreground: '#ffffffff',
      cursor: '#ffffffff',
    });
    expect(r.kernel.bg.toLowerCase()).toBe(GREEN);
    expect(r.pixel.g).toBeGreaterThan(150);
    expect(r.pixel.r).toBeLessThan(80);
    expect(r.pixel.b).toBeLessThan(80);
    expect(r.pixel.a).toBeGreaterThan(200);
  });
});
